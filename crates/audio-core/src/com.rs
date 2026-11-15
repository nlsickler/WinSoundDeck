use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

use crate::Result;

/// RAII guard that initializes COM on the current thread with `COINIT_MULTITHREADED`
/// and uninitializes it when dropped.
pub struct ComGuard;

impl ComGuard {
    pub fn new() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}
