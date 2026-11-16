mod app_settings;

use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use app_settings::{ActiveLoopback, AppSettingsStore, SecondaryOutputSetting};
use audio_core::ipc::{
    serve_named_pipe, DaemonStatus, EndpointStatus, IpcRequest, IpcResponse, LoopbackStatus,
    PresetSummary, SessionStatus, DEFAULT_PIPE_NAME,
};
use audio_core::{
    resolve_app_display_name, resolve_root_exe_name, AppPolicyClient, AudioEnumerator, AudioEvent,
    ComGuard, DataFlow, DeviceRole, LoopbackTee, PresetConfig, PresetEngine,
};

struct DaemonState {
    config: PresetConfig,
    app_settings: AppSettingsStore,
    app_settings_path: PathBuf,
    applied_pids: HashSet<u32>,
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
    let mut sessions_by_pid: HashMap<u32, (bool, SessionStatus)> = HashMap::new();
    let mut session_order: Vec<u32> = Vec::new();

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
                        let root_exe_name =
                            resolve_root_exe_name(s.pid).unwrap_or_else(|| s.exe_name.clone());
                        if matches!(
                            s.exe_name.to_ascii_lowercase().as_str(),
                            "audio-daemon.exe" | "audio-ui.exe"
                        ) || matches!(
                            root_exe_name.to_ascii_lowercase().as_str(),
                            "audio-daemon.exe" | "audio-ui.exe"
                        ) {
                            continue;
                        }
                        if let Some((existing_active, _)) = sessions_by_pid.get(&s.pid) {
                            if *existing_active || !s.is_active {
                                continue;
                            }
                        } else {
                            session_order.push(s.pid);
                        }
                        let vol = s.volume().unwrap_or(1.0);
                        let muted = s.is_muted().unwrap_or(false);
                        let display_name = resolve_app_display_name(s.pid)
                            .unwrap_or_else(|| root_exe_name.clone());
                        sessions_by_pid.insert(
                            s.pid,
                            (
                                s.is_active,
                                SessionStatus {
                                    endpoint_id: ep.id.clone(),
                                    pid: s.pid,
                                    exe_name: s.exe_name,
                                    root_exe_name,
                                    display_name,
                                    is_active: s.is_active,
                                    volume: vol,
                                    muted,
                                    saved_volumes: HashMap::new(),
                                },
                            ),
                        );
                    }
                }
            }
        }
    }

    let sessions = session_order
        .into_iter()
        .filter_map(|pid| sessions_by_pid.remove(&pid).map(|(_, st)| st))
        .collect();

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
            let (active_id, loopbacks, app_settings_snapshot) = state
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
                    (
                        g.config.active_preset_id.clone(),
                        lbs,
                        g.app_settings.clone(),
                    )
                })
                .unwrap_or((None, Vec::new(), AppSettingsStore::default()));
            match collect_status(enumerator, active_id) {
                Ok(mut status) => {
                    for s in &mut status.sessions {
                        if let Some(key) = AppSettingsStore::key_for_pid(s.pid) {
                            if let Some(cfg) = app_settings_snapshot.apps.get(&key) {
                                s.saved_volumes = cfg.output_volumes.clone();
                            }
                        }
                    }
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
            let vol = volume.clamp(0.0, 1.0);
            let res = enumerator
                .list_active_endpoints(DataFlow::Render)
                .and_then(|eps| {
                    if let Some(ep) = eps.into_iter().find(|e| e.id == endpoint_id) {
                        for s in ep.sessions()? {
                            if s.pid == pid {
                                s.set_volume(vol)?;
                                s.set_mute(vol <= 0.0)?;
                            }
                        }
                    }
                    Ok(())
                });
            match res {
                Ok(()) => {
                    if let Ok(mut guard) = state.lock() {
                        let path = guard.app_settings_path.clone();
                        guard.app_settings.upsert_for_pid(pid, &path, |s| {
                            s.volume = Some(vol);
                            s.muted = Some(vol <= 0.0);
                        });
                    }
                    (IpcResponse::Ok, false)
                }
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
                Ok(()) => {
                    if let Ok(mut guard) = state.lock() {
                        let path = guard.app_settings_path.clone();
                        guard.app_settings.upsert_for_pid(pid, &path, |s| {
                            s.muted = Some(muted);
                        });
                    }
                    (IpcResponse::Ok, false)
                }
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
                Ok(()) => {
                    if let Ok(mut guard) = state.lock() {
                        let path = guard.app_settings_path.clone();
                        guard.app_settings.upsert_for_pid(pid, &path, |s| {
                            s.primary_endpoint_id = Some(endpoint_id.clone());
                        });
                    }
                    (IpcResponse::Ok, false)
                }
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
                Ok(()) => {
                    if let Ok(mut guard) = state.lock() {
                        let path = guard.app_settings_path.clone();
                        guard.app_settings.upsert_for_pid(pid, &path, |s| {
                            s.primary_endpoint_id = None;
                        });
                    }
                    (IpcResponse::Ok, false)
                }
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
                    let primary = find_primary_session(&eps, pid);
                    if let Some((ref cur_primary, _)) = primary {
                        if cur_primary == &target_endpoint_id {
                            return Err("Endpoint is already the primary output".to_string());
                        }
                    }
                    let primary_vol = primary.map(|(_, v)| v).unwrap_or(1.0);
                    let ep = eps
                        .into_iter()
                        .find(|e| e.id == target_endpoint_id)
                        .ok_or_else(|| format!("Endpoint '{target_endpoint_id}' not found"))?;
                    let handle = LoopbackTee::start(pid, &ep, vol).map_err(|e| e.to_string())?;
                    let ratio = (vol / primary_vol.max(0.05)).clamp(0.0, 4.0);
                    handle.set_linear_gain(ratio.powi(2));
                    handle.set_mute(vol <= 0.0);
                    Ok(handle)
                });
            match res {
                Ok(handle) => {
                    if let Ok(mut guard) = state.lock() {
                        guard.loopbacks.insert(
                            (pid, target_endpoint_id.clone()),
                            ActiveLoopback {
                                handle,
                                volume: vol,
                                muted: vol <= 0.0,
                            },
                        );
                        let path = guard.app_settings_path.clone();
                        guard.app_settings.upsert_for_pid(pid, &path, |s| {
                            s.output_volumes.insert(target_endpoint_id.clone(), vol);
                            if let Some(existing) = s
                                .secondary_outputs
                                .iter_mut()
                                .find(|o| o.endpoint_id == target_endpoint_id)
                            {
                                existing.volume = vol;
                                existing.muted = vol <= 0.0;
                            } else {
                                s.secondary_outputs.push(SecondaryOutputSetting {
                                    endpoint_id: target_endpoint_id,
                                    volume: vol,
                                    muted: vol <= 0.0,
                                });
                            }
                        });
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
            let mute = vol <= 0.0;
            if let Ok(mut guard) = state.lock() {
                if let Some(lb) = guard
                    .loopbacks
                    .get_mut(&(pid, target_endpoint_id.clone()))
                {
                    lb.volume = vol;
                    lb.handle.set_volume(vol);
                    lb.muted = mute;
                    lb.handle.set_mute(mute);
                }
                let path = guard.app_settings_path.clone();
                guard.app_settings.upsert_for_pid(pid, &path, |s| {
                    if let Some(sec) = s
                        .secondary_outputs
                        .iter_mut()
                        .find(|o| o.endpoint_id == target_endpoint_id)
                    {
                        sec.volume = vol;
                        sec.muted = mute;
                    }
                });
            }
            (IpcResponse::Ok, false)
        }
        IpcRequest::SetLoopbackMute {
            pid,
            target_endpoint_id,
            muted,
        } => {
            if let Ok(mut guard) = state.lock() {
                if let Some(lb) = guard
                    .loopbacks
                    .get_mut(&(pid, target_endpoint_id.clone()))
                {
                    lb.muted = muted;
                    lb.handle.set_mute(muted);
                }
                let path = guard.app_settings_path.clone();
                guard.app_settings.upsert_for_pid(pid, &path, |s| {
                    if let Some(sec) = s
                        .secondary_outputs
                        .iter_mut()
                        .find(|o| o.endpoint_id == target_endpoint_id)
                    {
                        sec.muted = muted;
                    }
                });
            }
            (IpcResponse::Ok, false)
        }
        IpcRequest::StopLoopback {
            pid,
            target_endpoint_id,
        } => {
            if let Ok(mut guard) = state.lock() {
                guard.loopbacks.remove(&(pid, target_endpoint_id.clone()));
                let path = guard.app_settings_path.clone();
                guard.app_settings.upsert_for_pid(pid, &path, |s| {
                    s.secondary_outputs
                        .retain(|o| o.endpoint_id != target_endpoint_id);
                });
            }
            (IpcResponse::Ok, false)
        }
        IpcRequest::SetAppOutputVolume {
            pid,
            endpoint_id,
            volume,
            finalize,
        } => match handle_set_app_output_volume(enumerator, state, pid, &endpoint_id, volume, finalize) {
            Ok(()) => (IpcResponse::Ok, false),
            Err(message) => (IpcResponse::Error { message }, false),
        },
    }
}

fn find_primary_session(eps: &[audio_core::AudioEndpoint], pid: u32) -> Option<(String, f32)> {
    let mut fallback: Option<(String, f32)> = None;
    for ep in eps {
        if let Ok(sessions) = ep.sessions() {
            for s in sessions {
                if s.pid == pid {
                    let vol = s.volume().unwrap_or(1.0);
                    if s.is_active {
                        return Some((ep.id.clone(), vol));
                    }
                    if fallback.is_none() {
                        fallback = Some((ep.id.clone(), vol));
                    }
                }
            }
        }
    }
    fallback
}

fn handle_set_app_output_volume(
    enumerator: &AudioEnumerator,
    state: &Arc<Mutex<DaemonState>>,
    pid: u32,
    endpoint_id: &str,
    volume: f32,
    finalize: bool,
) -> std::result::Result<(), String> {
    let vol = volume.clamp(0.0, 1.0);
    let eps = enumerator
        .list_active_endpoints(DataFlow::Render)
        .map_err(|e| e.to_string())?;

    let Some((mut cur_primary, mut primary_vol)) = find_primary_session(&eps, pid) else {
        return Ok(());
    };

    let mut guard = state.lock().map_err(|e| e.to_string())?;

    if endpoint_id == cur_primary {
        primary_vol = vol;
    } else if let Some(lb) = guard.loopbacks.get_mut(&(pid, endpoint_id.to_string())) {
        lb.volume = vol;
        lb.muted = vol <= 0.0;
    }

    // Collect all enabled outputs for this PID.
    let secondary_vols: Vec<(String, f32)> = guard
        .loopbacks
        .iter()
        .filter(|((p, _), _)| *p == pid)
        .map(|((_, ep_id), lb)| (ep_id.clone(), lb.volume))
        .collect();

    // Only promote a secondary output to Primary on finalize when Primary reaches 0%
    // (otherwise f32 LoopbackTee amplifies cleanly via (secondary / primary)^2 without stream glitches).
    if finalize && primary_vol <= 0.0 && !secondary_vols.is_empty() {
        let mut best_id = cur_primary.clone();
        let mut best_vol = primary_vol;
        for (ep_id, v) in &secondary_vols {
            if *v > best_vol + 0.005 {
                best_vol = *v;
                best_id = ep_id.clone();
            }
        }
        if best_id != cur_primary {
            let old_primary = cur_primary.clone();
            let old_primary_vol = primary_vol;

            if let Ok(ap) = AppPolicyClient::new() {
                let _ = ap.set_app_endpoint(pid, DataFlow::Render, DeviceRole::Console, &best_id);
                let _ = ap.set_app_endpoint(pid, DataFlow::Render, DeviceRole::Multimedia, &best_id);
            }
            // Release lock while Windows migrates the stream, then verify actual primary.
            drop(guard);
            thread::sleep(Duration::from_millis(250));
            let migrated_primary = enumerator
                .list_active_endpoints(DataFlow::Render)
                .ok()
                .and_then(|new_eps| find_primary_session(&new_eps, pid).map(|(id, _)| id));
            guard = state.lock().map_err(|e| e.to_string())?;

            if migrated_primary.as_deref() == Some(best_id.as_str()) {
                guard.loopbacks.remove(&(pid, best_id.clone()));
                cur_primary = best_id;
                primary_vol = best_vol;

                if let Some(old_ep) = eps.iter().find(|e| e.id == old_primary) {
                    if let Ok(handle) = LoopbackTee::start(pid, old_ep, old_primary_vol) {
                        let ratio = if primary_vol > 0.0 {
                            (old_primary_vol / primary_vol).clamp(0.0, 4.0)
                        } else {
                            0.0
                        };
                        handle.set_linear_gain(ratio.powi(2));
                        handle.set_mute(old_primary_vol <= 0.0);
                        guard.loopbacks.insert(
                            (pid, old_primary),
                            ActiveLoopback {
                                handle,
                                volume: old_primary_vol,
                                muted: old_primary_vol <= 0.0,
                            },
                        );
                    }
                }
            }
        }
    }

    let has_loopbacks = guard.loopbacks.keys().any(|(p, _)| *p == pid);
    let primary_eff = if has_loopbacks && primary_vol < 0.05 {
        0.05
    } else {
        primary_vol
    };

    if let Some(ep) = eps.iter().find(|e| e.id == cur_primary) {
        if let Ok(sessions) = ep.sessions() {
            for s in sessions {
                if s.pid == pid {
                    let _ = s.set_volume(primary_eff);
                    let _ = s.set_mute(primary_eff <= 0.0);
                }
            }
        }
    }

    for ((p, _), lb) in guard.loopbacks.iter_mut() {
        if *p == pid {
            let ratio = if primary_eff > 0.0 {
                (lb.volume / primary_eff).clamp(0.0, 4.0)
            } else {
                0.0
            };
            lb.handle.set_linear_gain(ratio.powi(2));
            lb.handle.set_mute(lb.volume <= 0.0);
        }
    }

    let updated_secs: Vec<SecondaryOutputSetting> = guard
        .loopbacks
        .iter()
        .filter(|((p, _), _)| *p == pid)
        .map(|((_, ep_id), lb)| SecondaryOutputSetting {
            endpoint_id: ep_id.clone(),
            volume: lb.volume,
            muted: lb.muted,
        })
        .collect();
    let path = guard.app_settings_path.clone();
    let ep_key = endpoint_id.to_string();
    guard.app_settings.upsert_for_pid(pid, &path, |s| {
        s.primary_endpoint_id = Some(cur_primary);
        s.volume = Some(primary_vol);
        s.muted = Some(primary_vol <= 0.0);
        s.output_volumes.insert(ep_key, vol);
        s.secondary_outputs = updated_secs;
    });

    Ok(())
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
                    if let Ok(mut guard) = state.lock() {
                        let DaemonState {
                            ref app_settings,
                            ref mut applied_pids,
                            ref mut loopbacks,
                            ..
                        } = *guard;
                        app_settings.apply_to_pid(&enumerator, pid, applied_pids, loopbacks);
                    }
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

    let app_settings_path = AppSettingsStore::path_beside_config(&config_path);
    let app_settings = AppSettingsStore::load(&app_settings_path);
    let mut applied_pids = HashSet::new();
    let mut loopbacks = HashMap::new();

    if let Ok(eps) = enumerator.list_active_endpoints(DataFlow::Render) {
        for ep in eps {
            if let Ok(sessions) = ep.sessions() {
                for s in sessions {
                    app_settings.apply_to_pid(
                        &enumerator,
                        s.pid,
                        &mut applied_pids,
                        &mut loopbacks,
                    );
                }
            }
        }
    }

    let state = Arc::new(Mutex::new(DaemonState {
        config,
        app_settings,
        app_settings_path,
        applied_pids,
        loopbacks,
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
