use std::collections::{HashMap, HashSet};
use std::path::Path;

use windows::core::{GUID, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, MAX_PATH};
use windows::Win32::Media::Audio::ISimpleAudioVolume;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::Result;

/// An active per-application audio session (`ISimpleAudioVolume`).
pub struct AudioSession {
    pub pid: u32,
    pub exe_name: String,
    volume_ctl: ISimpleAudioVolume,
}

impl AudioSession {
    pub(crate) fn new(pid: u32, exe_name: String, volume_ctl: ISimpleAudioVolume) -> Self {
        Self {
            pid,
            exe_name,
            volume_ctl,
        }
    }

    pub fn volume(&self) -> Result<f32> {
        unsafe { self.volume_ctl.GetMasterVolume() }
    }

    pub fn set_volume(&self, volume: f32) -> Result<()> {
        unsafe {
            self.volume_ctl
                .SetMasterVolume(volume.clamp(0.0, 1.0), &GUID::zeroed())
        }
    }

    pub fn is_muted(&self) -> Result<bool> {
        unsafe { Ok(self.volume_ctl.GetMute()?.as_bool()) }
    }

    pub fn set_mute(&self, muted: bool) -> Result<()> {
        unsafe { self.volume_ctl.SetMute(muted, &GUID::zeroed()) }
    }
}

pub(crate) fn resolve_process_exe_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return Some("System Idle / Audio Engine".to_string());
    }
    let handle: HANDLE =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };
    let mut buf = vec![0u16; MAX_PATH as usize];
    let mut size = buf.len() as u32;
    // SAFETY: `buf` is valid for `size` UTF-16 code units; `QueryFullProcessImageNameW` updates `size`.
    let ok = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
    };
    unsafe {
        let _ = CloseHandle(handle);
    }
    ok.ok()?;

    let full_path = String::from_utf16_lossy(&buf[..size as usize]);
    Some(
        Path::new(&full_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&full_path)
            .to_string(),
    )
}

const SHELL_ROOT_EXES: &[&str] = &[
    "explorer.exe",
    "services.exe",
    "svchost.exe",
    "wininit.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "windowsterminal.exe",
    "conhost.exe",
];

/// Walks the Windows process tree via `CreateToolhelp32Snapshot` to find the root application
/// executable name (stopping before system/shell launchers like `explorer.exe`).
pub fn resolve_root_exe_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return Some("System Idle / Audio Engine".to_string());
    }

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()? };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    let mut tree: HashMap<u32, (u32, String)> = HashMap::new();
    // SAFETY: `entry.dwSize` is initialized to `size_of::<PROCESSENTRY32W>()`.
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry).is_ok() };
    while has_entry {
        let len = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
        tree.insert(entry.th32ProcessID, (entry.th32ParentProcessID, name));
        has_entry = unsafe { Process32NextW(snapshot, &mut entry).is_ok() };
    }
    unsafe {
        let _ = CloseHandle(snapshot);
    }

    let mut current = pid;
    let mut visited = HashSet::new();
    let mut last_name = tree
        .get(&pid)
        .map(|(_, n)| n.clone())
        .or_else(|| resolve_process_exe_name(pid))?;

    for _ in 0..16 {
        if !visited.insert(current) {
            break;
        }
        let Some(&(ppid, _)) = tree.get(&current) else {
            break;
        };
        if ppid == 0 || ppid == 4 {
            break;
        }
        let Some((_, parent_name)) = tree.get(&ppid) else {
            break;
        };
        if SHELL_ROOT_EXES.contains(&parent_name.to_lowercase().as_str()) {
            break;
        }
        last_name = parent_name.clone();
        current = ppid;
    }

    Some(last_name)
}
