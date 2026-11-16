use std::collections::{HashMap, HashSet};
use std::path::Path;

use windows::core::{GUID, HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, MAX_PATH};
use windows::Win32::Media::Audio::ISimpleAudioVolume;
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
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
    pub is_active: bool,
    volume_ctl: ISimpleAudioVolume,
}

impl AudioSession {
    pub(crate) fn new(
        pid: u32,
        exe_name: String,
        is_active: bool,
        volume_ctl: ISimpleAudioVolume,
    ) -> Self {
        Self {
            pid,
            exe_name,
            is_active,
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

pub(crate) fn resolve_process_full_path(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
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

    Some(String::from_utf16_lossy(&buf[..size as usize]))
}

pub(crate) fn resolve_process_exe_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return Some("System Idle / Audio Engine".to_string());
    }
    let full_path = resolve_process_full_path(pid)?;
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
    resolve_root_pid_and_name(pid).map(|(_, name)| name)
}

/// Resolves a user-friendly application display name (PE `FileDescription` / `ProductName`)
/// for the root application process, falling back to the executable stem.
pub fn resolve_app_display_name(pid: u32) -> Option<String> {
    let (root_pid, root_name) = resolve_root_pid_and_name(pid)?;
    if let Some(full_path) = resolve_process_full_path(root_pid) {
        if let Some(desc) = read_pe_file_description(&full_path) {
            return Some(desc);
        }
    }
    let stem = root_name
        .strip_suffix(".exe")
        .or_else(|| root_name.strip_suffix(".EXE"))
        .unwrap_or(&root_name);
    Some(stem.to_string())
}

fn resolve_root_pid_and_name(pid: u32) -> Option<(u32, String)> {
    if pid == 0 {
        return Some((0, "System Sounds".to_string()));
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

    Some((current, last_name))
}

fn read_pe_file_description(exe_path: &str) -> Option<String> {
    let wide_path = HSTRING::from(exe_path);
    let mut handle = 0u32;
    let size = unsafe { GetFileVersionInfoSizeW(&wide_path, Some(&mut handle)) };
    if size == 0 {
        return None;
    }

    let mut buf = vec![0u8; size as usize];
    unsafe {
        GetFileVersionInfoW(&wide_path, handle, size, buf.as_mut_ptr().cast()).ok()?;
    }

    let mut lang_cp = (0x0409u16, 0x04b0u16);
    let trans_key = HSTRING::from(r"\VarFileInfo\Translation");
    let mut trans_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
    let mut trans_len = 0u32;
    // SAFETY: `buf` holds the version resource block populated by `GetFileVersionInfoW`.
    if unsafe { VerQueryValueW(buf.as_ptr().cast(), &trans_key, &mut trans_ptr, &mut trans_len) }
        .as_bool()
        && !trans_ptr.is_null()
        && trans_len >= 4
    {
        let pair = unsafe { std::slice::from_raw_parts(trans_ptr.cast::<u16>(), 2) };
        lang_cp = (pair[0], pair[1]);
    }

    let sub_blocks = [
        format!(r"\StringFileInfo\{:04x}{:04x}\FileDescription", lang_cp.0, lang_cp.1),
        format!(r"\StringFileInfo\{:04x}{:04x}\ProductName", lang_cp.0, lang_cp.1),
        r"\StringFileInfo\040904b0\FileDescription".to_string(),
        r"\StringFileInfo\040904b0\ProductName".to_string(),
    ];

    for sub in sub_blocks {
        let key = HSTRING::from(sub);
        let mut val_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut val_len = 0u32;
        if unsafe { VerQueryValueW(buf.as_ptr().cast(), &key, &mut val_ptr, &mut val_len) }
            .as_bool()
            && !val_ptr.is_null()
            && val_len > 1
        {
            let wide = unsafe {
                std::slice::from_raw_parts(val_ptr.cast::<u16>(), val_len as usize)
            };
            let trimmed_len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
            let text = String::from_utf16_lossy(&wide[..trimmed_len]).trim().to_string();
            if !text.is_empty() {
                return Some(text);
            }
        }
    }

    None
}
