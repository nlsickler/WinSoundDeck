use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use audio_core::ipc::{IpcRequest, IpcResponse, PresetSummary};
use audio_core::{
    log_debug, log_error, log_info, log_warn, AppPolicyClient, AudioEnumerator, DataFlow,
    DeviceRole, LoopbackTee, PresetEngine,
};

use crate::app_settings::{ActiveLoopback, SecondaryOutputSetting};
use crate::state::DaemonState;
use crate::status::{build_daemon_status, find_primary_session, is_pid_active};

fn ipc_ok<E: ToString>(res: Result<(), E>) -> (IpcResponse, bool) {
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
            let guard = state.lock().expect("state lock");
            match build_daemon_status(enumerator, &guard) {
                Ok(status) => (IpcResponse::Status(status), false),
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

            ipc_ok(PresetEngine::new(enumerator).apply_preset(&preset).map(|_| ()))
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
        IpcRequest::SetSessionVolume {
            endpoint_id,
            pid,
            volume,
        } => {
            let vol = volume.clamp(0.0, 1.0);
            let res = enumerator.list_active_endpoints(DataFlow::Render).and_then(|eps| {
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
            if res.is_ok() {
                if let Ok(mut guard) = state.lock() {
                    let path = guard.app_settings_path.clone();
                    guard.app_settings.upsert_for_pid(pid, &path, |s| {
                        s.volume = Some(vol);
                        s.muted = Some(vol <= 0.0);
                    });
                }
            }
            ipc_ok(res)
        }
        IpcRequest::SetSessionMute {
            endpoint_id,
            pid,
            muted,
        } => {
            let res = enumerator.list_active_endpoints(DataFlow::Render).and_then(|eps| {
                if let Some(ep) = eps.into_iter().find(|e| e.id == endpoint_id) {
                    for s in ep.sessions()? {
                        if s.pid == pid {
                            s.set_mute(muted)?;
                        }
                    }
                }
                Ok(())
            });
            if res.is_ok() {
                if let Ok(mut guard) = state.lock() {
                    for ((p, _), lb) in guard.loopbacks.iter_mut() {
                        if *p == pid {
                            let lb_muted = if muted { true } else { lb.volume <= 0.0 };
                            lb.muted = lb_muted;
                            lb.handle.set_mute(lb_muted);
                        }
                    }
                    let path = guard.app_settings_path.clone();
                    guard.app_settings.upsert_for_pid(pid, &path, |s| {
                        s.muted = Some(muted);
                        for sec in &mut s.secondary_outputs {
                            sec.muted = if muted { true } else { sec.volume <= 0.0 };
                        }
                    });
                }
            }
            ipc_ok(res)
        }
        IpcRequest::RouteApp {
            pid,
            flow,
            endpoint_id,
        } => handle_route_app(enumerator, state, pid, flow, endpoint_id),
        IpcRequest::ResetAppRoute { pid, flow } => handle_reset_app_route(enumerator, state, pid, flow),
        IpcRequest::StartLoopback {
            pid,
            target_endpoint_id,
            volume,
        } => handle_start_loopback(enumerator, state, pid, target_endpoint_id, volume),
        IpcRequest::SetLoopbackVolume {
            pid,
            target_endpoint_id,
            volume,
        } => {
            let vol = volume.clamp(0.0, 1.0);
            let mute = vol <= 0.0;
            if let Ok(mut guard) = state.lock() {
                if let Some(lb) = guard.loopbacks.get_mut(&(pid, target_endpoint_id.clone())) {
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
                if let Some(lb) = guard.loopbacks.get_mut(&(pid, target_endpoint_id.clone())) {
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
        } => {
            log_debug!(
                "SetAppOutputVolume(pid={pid}, endpoint={endpoint_id}, vol={volume:.2}, finalize={finalize})"
            );
            let res = handle_set_app_output_volume(
                enumerator,
                state,
                pid,
                &endpoint_id,
                volume,
                finalize,
            );
            if let Err(ref message) = res {
                log_error!("SetAppOutputVolume(pid={pid}) failed: {message}");
            }
            ipc_ok(res)
        }
        IpcRequest::SetEndpointHidden {
            endpoint_id,
            hidden,
        } => {
            if let Ok(mut guard) = state.lock() {
                if hidden {
                    guard.app_settings.hidden_endpoints.insert(endpoint_id);
                } else {
                    guard.app_settings.hidden_endpoints.remove(&endpoint_id);
                }
                let path = guard.app_settings_path.clone();
                guard.app_settings.save(&path);
            }
            (IpcResponse::Ok, false)
        }
    }
}

fn handle_route_app(
    enumerator: &AudioEnumerator,
    state: &Arc<Mutex<DaemonState>>,
    pid: u32,
    flow: DataFlow,
    endpoint_id: String,
) -> (IpcResponse, bool) {
    let active = enumerator
        .list_active_endpoints(DataFlow::Render)
        .map(|eps| is_pid_active(&eps, pid))
        .unwrap_or(false);
    if active {
        let res = AppPolicyClient::new().and_then(|ap| {
            ap.set_app_endpoint(pid, flow, DeviceRole::Console, &endpoint_id)?;
            ap.set_app_endpoint(pid, flow, DeviceRole::Multimedia, &endpoint_id)
        });
        if let Err(ref e) = res {
            log_warn!("RouteApp(pid={pid}, endpoint={endpoint_id}) failed: {e}");
        } else {
            log_info!("RouteApp(pid={pid}, endpoint={endpoint_id}) applied");
        }
    } else {
        log_info!("RouteApp(pid={pid}, endpoint={endpoint_id}) persisted for inactive session");
    }
    if let Ok(mut guard) = state.lock() {
        let path = guard.app_settings_path.clone();
        guard.app_settings.upsert_for_pid(pid, &path, |s| {
            s.primary_endpoint_id = Some(endpoint_id);
        });
        guard.applied_pids.remove(&pid);
    }
    (IpcResponse::Ok, false)
}

fn handle_reset_app_route(
    enumerator: &AudioEnumerator,
    state: &Arc<Mutex<DaemonState>>,
    pid: u32,
    flow: DataFlow,
) -> (IpcResponse, bool) {
    let active = enumerator
        .list_active_endpoints(DataFlow::Render)
        .map(|eps| is_pid_active(&eps, pid))
        .unwrap_or(false);
    if active {
        let res = AppPolicyClient::new().and_then(|ap| {
            ap.reset_app_endpoint(pid, flow, DeviceRole::Console)?;
            ap.reset_app_endpoint(pid, flow, DeviceRole::Multimedia)
        });
        if let Err(ref e) = res {
            log_warn!("ResetAppRoute(pid={pid}) failed: {e}");
        } else {
            log_info!("ResetAppRoute(pid={pid}) applied");
        }
    } else {
        log_info!("ResetAppRoute(pid={pid}) cleared for inactive session");
    }
    if let Ok(mut guard) = state.lock() {
        let path = guard.app_settings_path.clone();
        guard.app_settings.upsert_for_pid(pid, &path, |s| {
            s.primary_endpoint_id = None;
        });
    }
    (IpcResponse::Ok, false)
}

fn handle_start_loopback(
    enumerator: &AudioEnumerator,
    state: &Arc<Mutex<DaemonState>>,
    pid: u32,
    target_endpoint_id: String,
    volume: f32,
) -> (IpcResponse, bool) {
    let vol = volume.clamp(0.0, 1.0);
    let res = enumerator
        .list_active_endpoints(DataFlow::Render)
        .map_err(|e| e.to_string())
        .and_then(|eps| {
            if !is_pid_active(&eps, pid) {
                return Ok(None);
            }
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
            Ok(Some(handle))
        });

    match res {
        Ok(maybe_handle) => {
            if let Some(handle) = maybe_handle {
                log_info!(
                    "StartLoopback(pid={pid}, target={target_endpoint_id}, vol={vol:.2}) started"
                );
                if let Ok(mut guard) = state.lock() {
                    guard.loopbacks.insert(
                        (pid, target_endpoint_id.clone()),
                        ActiveLoopback {
                            handle,
                            volume: vol,
                            muted: vol <= 0.0,
                        },
                    );
                }
            } else {
                log_info!(
                    "StartLoopback(pid={pid}, target={target_endpoint_id}, vol={vol:.2}) persisted for inactive session"
                );
            }
            if let Ok(mut guard) = state.lock() {
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
        Err(message) => {
            log_error!("StartLoopback(pid={pid}, target={target_endpoint_id}) failed: {message}");
            (IpcResponse::Error { message }, false)
        }
    }
}

fn handle_set_app_output_volume(
    enumerator: &AudioEnumerator,
    state: &Arc<Mutex<DaemonState>>,
    pid: u32,
    endpoint_id: &str,
    volume: f32,
    finalize: bool,
) -> Result<(), String> {
    let vol = volume.clamp(0.0, 1.0);
    let eps = enumerator
        .list_active_endpoints(DataFlow::Render)
        .map_err(|e| e.to_string())?;

    let Some((mut cur_primary, mut primary_vol)) = find_primary_session(&eps, pid) else {
        return Ok(());
    };

    let mut guard = state.lock().map_err(|e| e.to_string())?;

    if !is_pid_active(&eps, pid) {
        let ep_key = endpoint_id.to_string();
        let path = guard.app_settings_path.clone();
        guard.app_settings.upsert_for_pid(pid, &path, |s| {
            s.output_volumes.insert(ep_key.clone(), vol);
            let primary_id = s
                .primary_endpoint_id
                .get_or_insert_with(|| cur_primary.clone());
            if *primary_id == ep_key {
                s.volume = Some(vol);
                s.muted = Some(vol <= 0.0);
            } else if let Some(sec) = s
                .secondary_outputs
                .iter_mut()
                .find(|o| o.endpoint_id == ep_key)
            {
                sec.volume = vol;
                sec.muted = vol <= 0.0;
            }
        });
        return Ok(());
    }

    if endpoint_id == cur_primary {
        primary_vol = vol;
    } else if let Some(lb) = guard.loopbacks.get_mut(&(pid, endpoint_id.to_string())) {
        lb.volume = vol;
        lb.muted = vol <= 0.0;
    }

    let secondary_vols: Vec<(String, f32)> = guard
        .loopbacks
        .iter()
        .filter(|((p, _), _)| *p == pid)
        .map(|((_, ep_id), lb)| (ep_id.clone(), lb.volume))
        .collect();

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
                let _ =
                    ap.set_app_endpoint(pid, DataFlow::Render, DeviceRole::Multimedia, &best_id);
            }
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
                        let ratio = (old_primary_vol / primary_vol.max(0.05)).clamp(0.0, 4.0);
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

    let has_audible_loopback = guard
        .loopbacks
        .iter()
        .any(|((p, _), lb)| *p == pid && lb.volume > 0.0 && !lb.muted);
    let primary_eff = if has_audible_loopback && primary_vol < 0.05 {
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
