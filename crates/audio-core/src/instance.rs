use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;

use crate::Result;

/// RAII guard backed by a named Windows mutex to enforce single-instance execution.
pub struct SingleInstanceGuard {
    handle: HANDLE,
}

impl SingleInstanceGuard {
    /// Attempts to acquire a named single-instance mutex (e.g., `Local\WinSoundDeckUI`).
    /// Returns `Ok(None)` if another instance already holds the mutex.
    pub fn acquire(name: &str) -> Result<Option<Self>> {
        let wide = HSTRING::from(name);
        // SAFETY: `CreateMutexW` returns a valid kernel handle or an error.
        let handle = unsafe { CreateMutexW(None, false, &wide)? };
        // SAFETY: `GetLastError` reads the calling thread's last error set by `CreateMutexW`.
        let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        if already_exists {
            // SAFETY: `handle` is an open handle returned by `CreateMutexW`.
            let _ = unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(Self { handle }))
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            // SAFETY: `self.handle` is owned by this guard and closed once on drop.
            let _ = unsafe { CloseHandle(self.handle) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SingleInstanceGuard;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn single_instance_mutex_blocks_second_acquire_until_dropped() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let name = format!("Local\\WinSoundDeckTest_{nanos}");

        let first = SingleInstanceGuard::acquire(&name).expect("first acquire");
        assert!(first.is_some());

        let second = SingleInstanceGuard::acquire(&name).expect("second acquire");
        assert!(second.is_none());

        drop(first);

        let third = SingleInstanceGuard::acquire(&name).expect("re-acquire after drop");
        assert!(third.is_some());
    }
}
