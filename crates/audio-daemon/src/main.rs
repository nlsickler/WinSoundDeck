use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use audio_core::ipc::{
    serve_named_pipe, DaemonStatus, EndpointStatus, IpcRequest, IpcResponse, LoopbackStatus,
    PresetSummary, SessionStatus, DEFAULT_PIPE_NAME,
};
use audio_core::{
    resolve_app_display_name, resolve_root_exe_name, AppPolicyClient, AudioEnumerator, AudioEvent,
    ComGuard, DataFlow, DeviceRole, LoopbackHandle, LoopbackTee, PresetConfig, PresetEngine,
};

struct ActiveLoopback {
    handle: LoopbackHandle,
    volume: f32,
    muted: bool,
}

struct DaemonState {
    config: PresetConfig,
    loopbacks: HashMap<(u32, String), ActiveLoopback>,
    shutdown: bool,
}

fn default_config_path() -> PathBuf {
    if let Ok(appdata) = env::var("APPDATA") {
        PathBuf::from(appdata).join("audio-mixer").join("presets.json")
    } else {
        PathBuf::from("presets.json")
    }
}

fn parse_args() -> (PathBuf, String) {
    let mut config_path = default_config_path();
    let mut pipe_name = DEFAULT_PIPE_NAME.to_string();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                if let Some(p) = args.next() {
                    config_path = PathBuf::from(p);
                }
            }
            "--pipe" => {
                if let Some(p) = args.next() {
                    pipe_name = p;
                }
            }
            _ => {}
        }
    }
    (config_path, pipe_name)
}

fn load_or_default_config(path: &PathBuf) -> PresetConfig {
    if path.exists() {
        if let Ok(cfg) = PresetConfig::load_from_file(path) {
            return cfg;
        }
    }
    PresetConfig {
        version: "1.0.0".to_string(),
        active_preset_id: None,
        presets: Vec::new(),
    }
}

fn collect_status(
    enumerator: &AudioEnumerator,
    active_preset_id: Option<String>,
) -> audio_core::Result<DaemonStatus> {
    let def_render_mm = enumerator
        .default_endpoint_id(DataFlow::Render, DeviceRole::Multimedia)
        .unwrap_or_default();
    let def_render_comm = enumerator
        .default_endpoint_id(DataFlow::Render, DeviceRole::Communications)
        .unwrap_or_default();
    let def_cap_mm = enumerator
        .default_endpoint_id(DataFlow::Capture, DeviceRole::Multimedia)
        .unwrap_or_default();
    let def_cap_comm = enumerator
        .default_endpoint_id(DataFlow::Capture, DeviceRole::Communications)
        .unwrap_or_default();

    let mut endpoints = Vec::new();
    let mut sessions = Vec::new();

    for flow in [DataFlow::Render, DataFlow::Capture] {
        for ep in enumerator.list_active_endpoints(flow)? {
            let vol_state = ep.master_volume()?;
            let (mm_id, comm_id) = match flow {
                DataFlow::Render => (&def_render_mm, &def_render_comm),
                DataFlow::Capture => (&def_cap_mm, &def_cap_comm),
            };
            endpoints.push(EndpointStatus {
                id: ep.id.clone(),
                friendly_name: ep.friendly_name.clone(),
                flow,
                volume: vol_state.volume,
                muted: vol_state.muted,
                is_default_multimedia: ep.id == *mm_id,
                is_default_communications: ep.id == *comm_id,
            });

            if flow == DataFlow::Render {
                if let Ok(ep_sessions) = ep.sessions() {
                    for s in ep_sessions {
                        let vol = s.volume().unwrap_or(1.0);
                        let muted = s.is_muted().unwrap_or(false);
                        let root_exe_name =
                            resolve_root_exe_name(s.pid).unwrap_or_else(|| s.exe_name.clone());
                        let display_name = resolve_app_display_name(s.pid)
                            .unwrap_or_else(|| root_exe_name.clone());
                        sessions.push(SessionStatus {
                            endpoint_id: ep.id.clone(),
                            pid: s.pid,
                            exe_name: s.exe_name,
                            root_exe_name,
                            display_name,
                            volume: vol,
                            muted,
                        });
                    }
                }
            }
        }
    }

    Ok(DaemonStatus {
        active_preset_id,
        endpoints,
        sessions,
        loopbacks: Vec::new(),
    })
}

fn handle_request(
    enumerator: &AudioEnumerator,
    state: &Arc<Mutex<DaemonState>>,
    req: IpcRequest,
) -> (IpcResponse, bool) {
    match req {
        IpcRequest::Ping => (IpcResponse::Pong, false),
        IpcRequest::Shutdown => {
            if let Ok(mut guard) = state.lock() {
                guard.shutdown = true;
            }
            (IpcResponse::Ok, true)
        }
        IpcRequest::GetStatus => {
            let (active_id, loopbacks) = state
                .lock()
                .map(|g| {
                    let lbs = g
                        .loopbacks
                        .iter()
                        .map(|((pid, ep_id), lb)| LoopbackStatus {
                            pid: *pid,
                            target_endpoint_id: ep_id.clone(),
                            volume: lb.volume,
                            muted: lb.muted,
                        })
                        .collect();
                    (g.config.active_preset_id.clone(), lbs)
                })
                .unwrap_or((None, Vec::new()));
            match collect_status(enumerator, active_id) {
                Ok(mut status) => {
                    status.loopbacks = loopbacks;
                    (IpcResponse::Status(status), false)
                }
                Err(e) => (
                    IpcResponse::Error {
                        message: e.to_string(),
                    },
                    false,
                ),
            }
        }
        IpcRequest::ListPresets => {
            let guard = state.lock().expect("state lock");
            let presets = guard
                .config
                .presets
                .iter()
                .map(|p| PresetSummary {
                    id: p.id.clone(),
                    name: p.name.clone(),
                })
                .collect();
            (
                IpcResponse::Presets {
                    active_preset_id: guard.config.active_preset_id.clone(),
                    presets,
                },
                false,
            )
        }
        IpcRequest::ApplyPreset { id } => {
            let mut guard = state.lock().expect("state lock");
            let Some(preset) = guard.config.presets.iter().find(|p| p.id == id).cloned() else {
                return (
                    IpcResponse::Error {
                        message: format!("Preset '{id}' not found"),
                    },
                    false,
                );
            };
            guard.config.active_preset_id = Some(id);
            drop(guard);

            let engine = PresetEngine::new(enumerator);
            match engine.apply_preset(&preset) {
                Ok(_) => (IpcResponse::Ok, false),
                Err(e) => (
                    IpcResponse::Error {
                        message: e.to_string(),
                    },
                    false,
                ),
            }
        }
        IpcRequest::SetEndpointVolume {
            flow,
            endpoint_id,
            volume,
        } => {
            let res = enumerator
                .list_active_endpoints(flow)
                .and_then(|eps| {
                    eps.into_iter()
                        .find(|e| e.id == endpoint_id)
                        .map(|e| e.set_master_volume(volume))
                        .unwrap_or(Ok(()))
                });
            match res {
                Ok(()) => (IpcResponse::Ok, false),
                Err(e) => (
                    IpcResponse::Error {
                        message: e.to_string(),
                    },
                    false,
                ),
            }
        }
        IpcRequest::SetEndpointMute {
            flow,
            endpoint_id,
            muted,
        } => {
            let res = enumerator
                .list_active_endpoints(flow)
                .and_then(|eps| {
                    eps.into_iter()
                        .find(|e| e.id == endpoint_id)
                        .map(|e| e.set_master_mute(muted))
                        .unwrap_or(Ok(()))
                });
            match res {
                Ok(()) => (IpcResponse::Ok, false),
                Err(e) => (
                    IpcResponse::Error {
                        message: e.to_string(),
                    },
                    false,
                ),
            }
        }
        IpcRequest::SetSessionVolume {
            endpoint_id,
            pid,
            volume,
        } => {
            let res = enumerator
                .list_active_endpoints(DataFlow::Render)
                .and_then(|eps| {
                    if let Some(ep) = eps.into_iter().find(|e| e.id == endpoint_id) {
                        for s in ep.sessions()? {
                            if s.pid == pid {
                                s.set_volume(volume)?;
                            }
                        }
                    }
                    Ok(())
                });
            match res {
                Ok(()) => (IpcResponse::Ok, false),
                Err(e) => (
                    IpcResponse::Error {
                        message: e.to_string(),
                    },
                    false,
                ),
            }
        }
        IpcRequest::SetSessionMute {
            endpoint_id,
            pid,
            muted,
        } => {
            let res = enumerator
                .list_active_endpoints(DataFlow::Render)
                .and_then(|eps| {
                    if let Some(ep) = eps.into_iter().find(|e| e.id == endpoint_id) {
                        for s in ep.sessions()? {
                            if s.pid == pid {
                                s.set_mute(muted)?;
                            }
                        }
                    }
                    Ok(())
                });
            match res {
                Ok(()) => (IpcResponse::Ok, false),
                Err(e) => (
                    IpcResponse::Error {
                        message: e.to_string(),
                    },
                    false,
                ),
            }
        }
        IpcRequest::RouteApp {
            pid,
            flow,
            endpoint_id,
        } => {
            let res = AppPolicyClient::new().and_then(|ap| {
                ap.set_app_endpoint(pid, flow, DeviceRole::Console, &endpoint_id)?;
                ap.set_app_endpoint(pid, flow, DeviceRole::Multimedia, &endpoint_id)
            });
            match res {
                Ok(()) => (IpcResponse::Ok, false),
                Err(e) => (
                    IpcResponse::Error {
                        message: e.to_string(),
                    },
                    false,
                ),
            }
        }
        IpcRequest::ResetAppRoute { pid, flow } => {
            let res = AppPolicyClient::new().and_then(|ap| {
                ap.reset_app_endpoint(pid, flow, DeviceRole::Console)?;
                ap.reset_app_endpoint(pid, flow, DeviceRole::Multimedia)
            });
            match res {
                Ok(()) => (IpcResponse::Ok, false),
                Err(e) => (
                    IpcResponse::Error {
                        message: e.to_string(),
                    },
                    false,
                ),
            }
        }
        IpcRequest::StartLoopback {
            pid,
            target_endpoint_id,
            volume,
        } => {
            let vol = volume.clamp(0.0, 1.0);
            let res = enumerator
                .list_active_endpoints(DataFlow::Render)
                .map_err(|e| e.to_string())
                .and_then(|eps| {
                    let ep = eps
                        .into_iter()
                        .find(|e| e.id == target_endpoint_id)
                        .ok_or_else(|| format!("Endpoint '{target_endpoint_id}' not found"))?;
                    LoopbackTee::start(pid, &ep, vol).map_err(|e| e.to_string())
                });
            match res {
                Ok(handle) => {
                    if let Ok(mut guard) = state.lock() {
                        guard.loopbacks.insert(
                            (pid, target_endpoint_id),
                            ActiveLoopback {
                                handle,
                                volume: vol,
                                muted: false,
                            },
                        );
                    }
                    (IpcResponse::Ok, false)
                }
                Err(message) => (IpcResponse::Error { message }, false),
            }
        }
        IpcRequest::SetLoopbackVolume {
            pid,
            target_endpoint_id,
            volume,
        } => {
            let vol = volume.clamp(0.0, 1.0);
            if let Ok(mut guard) = state.lock() {
                if let Some(lb) = guard.loopbacks.get_mut(&(pid, target_endpoint_id)) {
                    lb.volume = vol;
                    lb.handle.set_volume(vol);
                }
            }
            (IpcResponse::Ok, false)
        }
        IpcRequest::SetLoopbackMute {
            pid,
            target_endpoint_id,
            muted,
        } => {
            if let Ok(mut guard) = state.lock() {
                if let Some(lb) = guard.loopbacks.get_mut(&(pid, target_endpoint_id)) {
                    lb.muted = muted;
                    lb.handle.set_mute(muted);
                }
            }
            (IpcResponse::Ok, false)
        }
        IpcRequest::StopLoopback {
            pid,
            target_endpoint_id,
        } => {
            if let Ok(mut guard) = state.lock() {
                guard.loopbacks.remove(&(pid, target_endpoint_id));
            }
            (IpcResponse::Ok, false)
        }
    }
}

fn spawn_event_watcher(state: Arc<Mutex<DaemonState>>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let Ok(_com) = ComGuard::new() else { return };
        let Ok(enumerator) = AudioEnumerator::new() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let Ok(_dev_guard) = enumerator.watch_devices(tx.clone()) else {
            return;
        };

        let mut session_guards = Vec::new();
        if let Ok(eps) = enumerator.list_active_endpoints(DataFlow::Render) {
            for ep in eps {
                if let Ok(g) = ep.watch_sessions(tx.clone()) {
                    session_guards.push(g);
                }
            }
        }

        loop {
            if state.lock().map(|g| g.shutdown).unwrap_or(true) {
                break;
            }
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(AudioEvent::SessionCreated {
                    endpoint_id,
                    pid,
                    exe_name,
                    root_exe_name,
                }) => {
                    let active = state.lock().ok().and_then(|g| {
                        let id = g.config.active_preset_id.as_ref()?;
                        g.config.presets.iter().find(|p| &p.id == id).cloned()
                    });
                    if let Some(preset) = active {
                        let _ = PresetEngine::new(&enumerator).handle_session_created(
                            &preset,
                            &endpoint_id,
                            pid,
                            &exe_name,
                            &root_exe_name,
                        );
                    }
                }
                Ok(AudioEvent::DeviceStateChanged { .. }) | Ok(AudioEvent::DeviceAdded { .. }) => {
                    session_guards.clear();
                    if let Ok(eps) = enumerator.list_active_endpoints(DataFlow::Render) {
                        for ep in eps {
                            if let Ok(g) = ep.watch_sessions(tx.clone()) {
                                session_guards.push(g);
                            }
                        }
                    }
                    let active = state.lock().ok().and_then(|g| {
                        let id = g.config.active_preset_id.as_ref()?;
                        g.config.presets.iter().find(|p| &p.id == id).cloned()
                    });
                    if let Some(preset) = active {
                        let _ = PresetEngine::new(&enumerator).apply_preset(&preset);
                    }
                }
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    })
}

fn main() {
    let (config_path, pipe_name) = parse_args();
    let _com = ComGuard::new().expect("COM init failed");
    let enumerator = AudioEnumerator::new().expect("AudioEnumerator init failed");
    let config = load_or_default_config(&config_path);

    if let Some(ref active_id) = config.active_preset_id {
        if let Some(preset) = config.presets.iter().find(|p| &p.id == active_id) {
            let _ = PresetEngine::new(&enumerator).apply_preset(preset);
        }
    }

    let state = Arc::new(Mutex::new(DaemonState {
        config,
        loopbacks: HashMap::new(),
        shutdown: false,
    }));
    let watcher = spawn_event_watcher(Arc::clone(&state));

    if let Err(err) = serve_named_pipe(&pipe_name, |req| handle_request(&enumerator, &state, req)) {
        eprintln!("Named pipe server error: {err}");
    }

    if let Ok(mut guard) = state.lock() {
        guard.shutdown = true;
    }
    let _ = watcher.join();
}
