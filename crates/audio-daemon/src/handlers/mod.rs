mod app_routing;
mod app_volume;
mod presets;

use std::sync::{Arc, Mutex};

use audio_core::ipc::{IpcRequest, IpcResponse, PeakLevels, PresetSummary};
use audio_core::{
    log_debug, log_error, log_info, log_warn, AudioEnumerator, DataFlow, DeviceRole, PolicyClient,
};

use self::app_routing::{handle_route_app, handle_start_loopback};
use self::app_volume::handle_set_app_output_volume;
use self::presets::{handle_apply_preset, handle_save_current_as_preset};
use crate::state::DaemonState;
use crate::status::build_daemon_status;

fn collect_peaks(enumerator: &AudioEnumerator) -> PeakLevels {
    let mut peaks = PeakLevels::default();
    for flow in [DataFlow::Render, DataFlow::Capture] {
        let Ok(eps) = enumerator.list_active_endpoints(flow) else {
            continue;
        };
        for ep in eps {
            if let Ok(p) = ep.peak_value() {
                peaks.endpoints.insert(ep.id.clone(), p);
            }
            if flow == DataFlow::Render {
                if let Ok(sessions) = ep.sessions() {
                    for s in sessions {
                        if s.is_active && s.pid > 0 {
                            if let Ok(p) = s.peak_value() {
                                peaks
                                    .sessions
                                    .entry(s.pid.to_string())
                                    .and_modify(|cur| {
                                        if p > *cur {
                                            *cur = p;
                                        }
                                    })
                                    .or_insert(p);
                            }
                        }
                    }
                }
            }
        }
    }
    peaks
}

pub(super) fn ipc_ok<E: ToString>(res: Result<(), E>) -> (IpcResponse, bool) {
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

pub fn handle_request(
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
            let mut guard = state.lock().expect("state lock");
            match build_daemon_status(enumerator, &guard) {
                Ok(status) => {
                    let path = guard.app_settings_path.clone();
                    guard
                        .app_settings
                        .remember_seen_sessions(&status.sessions, &path);
                    guard
                        .app_settings
                        .remember_known_endpoints(&status.endpoints, &path);
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
        IpcRequest::GetPeaks => (IpcResponse::Peaks(collect_peaks(enumerator)), false),
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
        IpcRequest::ApplyPreset { id } => handle_apply_preset(enumerator, state, id),
        IpcRequest::SaveCurrentAsPreset { name } => {
            handle_save_current_as_preset(enumerator, state, name)
        }
        IpcRequest::DeletePreset { id } => {
            let mut guard = state.lock().expect("state lock");
            guard.config.presets.retain(|p| p.id != id);
            if guard.config.active_preset_id.as_deref() == Some(id.as_str()) {
                guard.config.active_preset_id = None;
            }
            let path = guard.config_path.clone();
            let res = guard.config.save_to_file(&path);
            log_info!("DeletePreset(id={id})");
            ipc_ok(res)
        }
        IpcRequest::ClearActivePreset => {
            let (snap, res) = {
                let mut guard = state.lock().expect("state lock");
                guard.config.active_preset_id = None;
                let path = guard.config_path.clone();
                let res = guard.config.save_to_file(&path);
                (guard.pre_preset_snapshot.take(), res)
            };
            if let Some(snap) = snap {
                if let Err(e) = snap.restore(enumerator) {
                    log_warn!("ClearActivePreset restore error: {e}");
                }
            }
            log_info!("ClearActivePreset");
            ipc_ok(res)
        }
        IpcRequest::SetEndpointVolume {
            flow,
            endpoint_id,
            volume,
        } => ipc_ok(enumerator.list_active_endpoints(flow).and_then(|eps| {
            eps.into_iter()
                .find(|e| e.id == endpoint_id)
                .map(|e| e.set_master_volume(volume))
                .unwrap_or(Ok(()))
        })),
        IpcRequest::SetEndpointMute {
            flow,
            endpoint_id,
            muted,
        } => ipc_ok(enumerator.list_active_endpoints(flow).and_then(|eps| {
            eps.into_iter()
                .find(|e| e.id == endpoint_id)
                .map(|e| e.set_master_mute(muted))
                .unwrap_or(Ok(()))
        })),
        IpcRequest::SetSessionMute {
            endpoint_id,
            pid,
            app_key,
            muted,
        } => {
            let res = if pid == 0 {
                Ok(())
            } else {
                enumerator.list_active_endpoints(DataFlow::Render).and_then(|eps| {
                    if let Some(ep) = eps.into_iter().find(|e| e.id == endpoint_id) {
                        for s in ep.sessions()? {
                            if s.pid == pid {
                                s.set_mute(muted)?;
                            }
                        }
                    }
                    Ok(())
                })
            };
            if res.is_ok() {
                if let Ok(mut guard) = state.lock() {
                    if pid > 0 {
                        guard.loopbacks.set_mute_for_pid(pid, muted);
                    }
                    let path = guard.app_settings_path.clone();
                    guard.app_settings.upsert_by_key_or_pid(
                        pid,
                        app_key.as_deref(),
                        &path,
                        |s| {
                            s.muted = Some(muted);
                            for sec in &mut s.secondary_outputs {
                                sec.muted = if muted { true } else { sec.volume <= 0.0 };
                            }
                        },
                    );
                }
            }
            ipc_ok(res)
        }
        IpcRequest::RouteApp {
            pid,
            app_key,
            flow,
            endpoint_id,
        } => handle_route_app(enumerator, state, pid, app_key, flow, endpoint_id),
        IpcRequest::StartLoopback {
            pid,
            app_key,
            target_endpoint_id,
            volume,
        } => handle_start_loopback(enumerator, state, pid, app_key, target_endpoint_id, volume),
        IpcRequest::StopLoopback {
            pid,
            app_key,
            target_endpoint_id,
        } => {
            if let Ok(mut guard) = state.lock() {
                if pid > 0 {
                    guard.loopbacks.remove(pid, &target_endpoint_id);
                }
                let path = guard.app_settings_path.clone();
                guard
                    .app_settings
                    .upsert_by_key_or_pid(pid, app_key.as_deref(), &path, |s| {
                        s.secondary_outputs
                            .retain(|o| o.endpoint_id != target_endpoint_id);
                    });
            }
            (IpcResponse::Ok, false)
        }
        IpcRequest::SetAppOutputVolume {
            pid,
            app_key,
            endpoint_id,
            volume,
            finalize,
        } => {
            log_debug!(
                "SetAppOutputVolume(pid={pid}, endpoint={endpoint_id}, vol={volume:.2}, finalize={finalize})"
            );
            let res = handle_set_app_output_volume(
                enumerator,
                state,
                pid,
                app_key.as_deref(),
                &endpoint_id,
                volume,
                finalize,
            );
            if let Err(ref message) = res {
                log_error!("SetAppOutputVolume(pid={pid}) failed: {message}");
            }
            ipc_ok(res)
        }
        IpcRequest::SetAppPinned {
            pid,
            app_key,
            pinned,
        } => {
            if let Ok(mut guard) = state.lock() {
                let path = guard.app_settings_path.clone();
                guard
                    .app_settings
                    .set_app_pinned(pid, app_key.as_deref(), pinned, &path);
            }
            (IpcResponse::Ok, false)
        }
        IpcRequest::ClearUnpinnedApps => {
            if let Ok(mut guard) = state.lock() {
                guard.app_settings.clear_unpinned();
            }
            (IpcResponse::Ok, false)
        }
        IpcRequest::SetEndpointAllowed {
            endpoint_id,
            allowed,
        } => {
            if let Ok(mut guard) = state.lock() {
                if guard.app_settings.allowed_endpoints.is_none() {
                    let mut initial = std::collections::HashSet::new();
                    for flow in [DataFlow::Render, DataFlow::Capture] {
                        if let Ok(eps) = enumerator.list_active_endpoints(flow) {
                            for ep in eps {
                                initial.insert(ep.id);
                            }
                        }
                    }
                    guard.app_settings.allowed_endpoints = Some(initial);
                }
                if let Some(ref mut set) = guard.app_settings.allowed_endpoints {
                    if allowed {
                        set.insert(endpoint_id);
                    } else {
                        set.remove(&endpoint_id);
                    }
                }
                let path = guard.app_settings_path.clone();
                guard.app_settings.save(&path);
            }
            (IpcResponse::Ok, false)
        }
        IpcRequest::ForgetEndpoint { endpoint_id } => {
            if let Ok(mut guard) = state.lock() {
                let path = guard.app_settings_path.clone();
                guard.app_settings.forget_endpoint(&endpoint_id, &path);
            }
            (IpcResponse::Ok, false)
        }
        IpcRequest::SetDefaultEndpoint { endpoint_id, role } => {
            let res = PolicyClient::new().and_then(|policy| {
                if role == DeviceRole::Multimedia {
                    policy.set_default_endpoint(&endpoint_id, DeviceRole::Console)?;
                    policy.set_default_endpoint(&endpoint_id, DeviceRole::Multimedia)
                } else {
                    policy.set_default_endpoint(&endpoint_id, role)
                }
            });
            if res.is_ok() {
                if let Ok(eps_render) = enumerator.list_active_endpoints(DataFlow::Render) {
                    let flow = if eps_render.iter().any(|e| e.id == endpoint_id) {
                        Some(DataFlow::Render)
                    } else {
                        Some(DataFlow::Capture)
                    };
                    if let (Some(flow), Ok(mut guard)) = (flow, state.lock()) {
                        if role == DeviceRole::Multimedia {
                            guard
                                .default_endpoints
                                .insert((flow, DeviceRole::Console), endpoint_id.clone());
                            guard
                                .default_endpoints
                                .insert((flow, DeviceRole::Multimedia), endpoint_id.clone());
                        } else {
                            guard
                                .default_endpoints
                                .insert((flow, role), endpoint_id.clone());
                        }
                    }
                }
                log_info!("SetDefaultEndpoint(endpoint={endpoint_id}, role={role:?})");
            }
            ipc_ok(res)
        }
        IpcRequest::SetIncludeDisconnected { include } => {
            if let Ok(mut guard) = state.lock() {
                guard.app_settings.include_disconnected = include;
                let path = guard.app_settings_path.clone();
                guard.app_settings.save(&path);
            }
            log_info!("SetIncludeDisconnected(include={include})");
            (IpcResponse::Ok, false)
        }
    }
}
