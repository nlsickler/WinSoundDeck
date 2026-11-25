use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use audio_core::ipc::{send_ipc_request, IpcRequest, IpcResponse, DEFAULT_PIPE_NAME};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn ipc_call(req: IpcRequest) -> Result<IpcResponse, String> {
    match send_ipc_request(DEFAULT_PIPE_NAME, &req).map_err(|e| e.to_string())? {
        IpcResponse::Error { message } => Err(message),
        other => Ok(other),
    }
}

pub fn daemon_launch_command(current_exe: Option<&Path>) -> (PathBuf, &'static [&'static str]) {
    #[cfg(feature = "portable")]
    {
        let exe = current_exe
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("audio-ui.exe"));
        (exe, &["--daemon"])
    }
    #[cfg(not(feature = "portable"))]
    {
        let daemon_path = current_exe
            .and_then(|p| p.parent().map(|dir| dir.join("audio-daemon.exe")))
            .unwrap_or_else(|| PathBuf::from("audio-daemon.exe"));
        (daemon_path, &[])
    }
}

pub fn ensure_daemon_running() {
    if matches!(
        send_ipc_request(DEFAULT_PIPE_NAME, &IpcRequest::Ping),
        Ok(IpcResponse::Pong)
    ) {
        return;
    }

    let current_exe = std::env::current_exe().ok();
    let (daemon_path, args) = daemon_launch_command(current_exe.as_deref());

    if Command::new(daemon_path)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .is_ok()
    {
        for _ in 0..20 {
            if matches!(
                send_ipc_request(DEFAULT_PIPE_NAME, &IpcRequest::Ping),
                Ok(IpcResponse::Pong)
            ) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::daemon_launch_command;
    use std::path::Path;

    #[test]
    fn daemon_launch_command_matches_build_mode() {
        let exe = Path::new("C:\\Apps\\audio-ui.exe");
        let (path, args) = daemon_launch_command(Some(exe));
        #[cfg(feature = "portable")]
        {
            assert_eq!(path, exe);
            assert_eq!(args, &["--daemon"]);
        }
        #[cfg(not(feature = "portable"))]
        {
            assert_eq!(path, Path::new("C:\\Apps\\audio-daemon.exe"));
            assert!(args.is_empty());
        }
    }
}
