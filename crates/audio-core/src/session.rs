use std::path::Path;

use windows::core::{GUID, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, MAX_PATH};
use windows::Win32::Media::Audio::ISimpleAudioVolume;
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
