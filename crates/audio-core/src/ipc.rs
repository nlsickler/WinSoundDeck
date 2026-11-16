use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::{FromRawHandle, RawHandle};

use serde::{Deserialize, Serialize};
use windows::core::HSTRING;
use windows::Win32::Foundation::{GetLastError, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{FlushFileBuffers, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Console::{
    SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_C_EVENT,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
    PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

use crate::DataFlow;

pub const DEFAULT_PIPE_NAME: &str = r"\\.\pipe\audio-mixer-daemon";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcRequest {
    Ping,
    GetStatus,
    ListPresets,
    ApplyPreset {
        id: String,
    },
    SetEndpointVolume {
        flow: DataFlow,
        endpoint_id: String,
        volume: f32,
    },
    SetEndpointMute {
        flow: DataFlow,
        endpoint_id: String,
        muted: bool,
    },
    SetSessionVolume {
        endpoint_id: String,
        pid: u32,
        volume: f32,
    },
    SetSessionMute {
        endpoint_id: String,
        pid: u32,
        muted: bool,
    },
    RouteApp {
        pid: u32,
        flow: DataFlow,
        endpoint_id: String,
    },
    ResetAppRoute {
        pid: u32,
        flow: DataFlow,
    },
    StartLoopback {
        pid: u32,
        target_endpoint_id: String,
        volume: f32,
    },
    SetLoopbackVolume {
        pid: u32,
        target_endpoint_id: String,
        volume: f32,
    },
    SetLoopbackMute {
        pid: u32,
        target_endpoint_id: String,
        muted: bool,
    },
    StopLoopback {
        pid: u32,
        target_endpoint_id: String,
    },
    SetAppOutputVolume {
        pid: u32,
        endpoint_id: String,
        volume: f32,
        finalize: bool,
    },
    SetEndpointHidden {
        endpoint_id: String,
        hidden: bool,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointStatus {
    pub id: String,
    pub friendly_name: String,
    pub flow: DataFlow,
    pub volume: f32,
    pub muted: bool,
    pub is_default_multimedia: bool,
    pub is_default_communications: bool,
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatus {
    pub endpoint_id: String,
    pub pid: u32,
    pub exe_name: String,
    pub root_exe_name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub is_active: bool,
    pub volume: f32,
    pub muted: bool,
    #[serde(default)]
    pub saved_volumes: HashMap<String, f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopbackStatus {
    pub pid: u32,
    pub target_endpoint_id: String,
    pub volume: f32,
    pub muted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub active_preset_id: Option<String>,
    pub endpoints: Vec<EndpointStatus>,
    pub sessions: Vec<SessionStatus>,
    #[serde(default)]
    pub loopbacks: Vec<LoopbackStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetSummary {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcResponse {
    Pong,
    Status(DaemonStatus),
    Presets {
        active_preset_id: Option<String>,
        presets: Vec<PresetSummary>,
    },
    Ok,
    Error {
        message: String,
    },
}

/// Registers a Windows console control handler (`Ctrl+C`, `Ctrl+Break`, console close)
/// that sends `IpcRequest::Shutdown` to `DEFAULT_PIPE_NAME` before exiting the process.
pub fn install_ctrl_c_daemon_shutdown() {
    unsafe extern "system" fn handler(ctrl_type: u32) -> windows::Win32::Foundation::BOOL {
        if matches!(ctrl_type, CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT) {
            let _ = send_ipc_request(DEFAULT_PIPE_NAME, &IpcRequest::Shutdown);
            std::process::exit(0);
        }
        false.into()
    }
    let _ = unsafe { SetConsoleCtrlHandler(Some(handler), true) };
}

/// Sends a single line-delimited JSON request over a Windows Named Pipe and awaits the response.
pub fn send_ipc_request(pipe_name: &str, req: &IpcRequest) -> std::io::Result<IpcResponse> {
    let mut file = open_pipe_with_retry(pipe_name)?;
    let mut line = serde_json::to_string(req)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    line.push('\n');
    file.write_all(line.as_bytes())?;
    file.flush()?;

    let mut reader = BufReader::new(file);
    let mut resp_line = String::new();
    reader.read_line(&mut resp_line)?;
    serde_json::from_str(&resp_line)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn open_pipe_with_retry(pipe_name: &str) -> std::io::Result<std::fs::File> {
    const MAX_ATTEMPTS: usize = 20;
    for attempt in 0..MAX_ATTEMPTS {
        match OpenOptions::new().read(true).write(true).open(pipe_name) {
            Ok(file) => return Ok(file),
            Err(e)
                if attempt + 1 < MAX_ATTEMPTS
                    && (e.kind() == std::io::ErrorKind::NotFound
                        || e.raw_os_error() == Some(231)) =>
            {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

/// Blocking Windows Named Pipe server that processes one line-delimited JSON request per connection.
pub fn serve_named_pipe<F>(pipe_name: &str, mut handler: F) -> std::io::Result<()>
where
    F: FnMut(IpcRequest) -> (IpcResponse, bool),
{
    let wide_name = HSTRING::from(pipe_name);
    loop {
        let handle = unsafe {
            CreateNamedPipeW(
                &wide_name,
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                65536,
                65536,
                0,
                None,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }

        let connected = unsafe { ConnectNamedPipe(handle, None) };
        if let Err(err) = connected {
            let last = unsafe { GetLastError() };
            if last != ERROR_PIPE_CONNECTED {
                let _ = unsafe { File::from_raw_handle(handle.0 as RawHandle) };
                return Err(std::io::Error::from_raw_os_error(err.code().0));
            }
        }

        // SAFETY: `handle` is an open, connected Named Pipe handle owned by this iteration.
        let mut file = unsafe { File::from_raw_handle(handle.0 as RawHandle) };
        let mut reader = BufReader::new(&file);
        let mut line = String::new();
        let should_exit = match reader.read_line(&mut line) {
            Ok(0) => false,
            Ok(_) => {
                let (resp, exit) = match serde_json::from_str::<IpcRequest>(&line) {
                    Ok(req) => handler(req),
                    Err(e) => (
                        IpcResponse::Error {
                            message: format!("Invalid request JSON: {e}"),
                        },
                        false,
                    ),
                };
                if let Ok(mut out) = serde_json::to_string(&resp) {
                    out.push('\n');
                    let _ = file.write_all(out.as_bytes());
                    let _ = file.flush();
                }
                exit
            }
            Err(_) => false,
        };

        unsafe {
            let _ = FlushFileBuffers(handle);
            let _ = DisconnectNamedPipe(handle);
        }
        drop(file);

        if should_exit {
            break;
        }
    }
    Ok(())
}
