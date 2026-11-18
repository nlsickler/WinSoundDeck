use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use audio_core::ipc::{IpcRequest, IpcResponse, PresetSummary};
use audio_core::{
    log_debug, log_error, log_info, log_warn, resolve_root_exe_name, AppLoopbackRule,
    AppPolicyClient, AppRouteRule, AppVolumeRule, AudioEnumerator, DataFlow, DeviceRole,
    EndpointTarget, LoopbackTee, PolicyClient, Preset, PresetEngine,
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
            let mut guard = state.lock().expect("state lock");
            match build_daemon_status(enumerator, &guard) {
                Ok(status) => {
                    let path = guard.app_settings_path.clone();
                    guard
                        .app_settings
                        .remember_seen_sessions(&status.sessions, &path);
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
                        for ((p, _), lb) in guard.loopbacks.iter_mut() {
                            if *p == pid {
                                let lb_muted = if muted { true } else { lb.volume <= 0.0 };
                                lb.muted = lb_muted;
                                lb.handle.set_mute(lb_muted);
                            }
                        }
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
        IpcRequest::ResetAppRoute { pid, flow } => handle_reset_app_route(enumerator, state, pid, flow),
        IpcRequest::StartLoopback {
            pid,
            app_key,
            target_endpoint_id,
            volume,
        } => handle_start_loopback(enumerator, state, pid, app_key, target_endpoint_id, volume),
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
            app_key,
            target_endpoint_id,
        } => {
            if let Ok(mut guard) = state.lock() {
                if pid > 0 {
                    guard.loopbacks.remove(&(pid, target_endpoint_id.clone()));
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

fn handle_route_app(
    enumerator: &AudioEnumerator,
    state: &Arc<Mutex<DaemonState>>,
    pid: u32,
    app_key: Option<String>,
    flow: DataFlow,
    endpoint_id: String,
) -> (IpcResponse, bool) {
    let active = pid > 0
        && enumerator
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
        guard
            .app_settings
            .upsert_by_key_or_pid(pid, app_key.as_deref(), &path, |s| {
                s.primary_endpoint_id = Some(endpoint_id);
            });
        if pid > 0 {
            guard.applied_pids.remove(&pid);
        }
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
    app_key: Option<String>,
    target_endpoint_id: String,
    volume: f32,
) -> (IpcResponse, bool) {
    let vol = volume.clamp(0.0, 1.0);
    let res = if pid == 0 {
        Ok(None)
    } else {
        enumerator
            .list_endpoints(DataFlow::Render, true)
            .map_err(|e| e.to_string())
            .and_then(|eps| {
                let ep = eps
                    .iter()
                    .find(|e| e.id == target_endpoint_id)
                    .ok_or_else(|| format!("Endpoint '{target_endpoint_id}' not found"))?;
                if !ep.is_connected || !is_pid_active(&eps, pid) {
                    return Ok(None);
                }
                let primary = find_primary_session(&eps, pid);
                if let Some((ref cur_primary, _)) = primary {
                    if cur_primary == &target_endpoint_id {
                        return Err("Endpoint is already the primary output".to_string());
                    }
                }
                let primary_vol = primary.map(|(_, v)| v).unwrap_or(1.0);
                let handle = LoopbackTee::start(pid, ep, vol).map_err(|e| e.to_string())?;
                let ratio = (vol / primary_vol.max(0.05)).clamp(0.0, 4.0);
                handle.set_linear_gain(ratio.powi(2));
                handle.set_mute(vol <= 0.0);
                Ok(Some(handle))
            })
    };

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
                guard.app_settings.upsert_by_key_or_pid(
                    pid,
                    app_key.as_deref(),
                    &path,
                    |s| {
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
                    },
                );
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
    app_key: Option<&str>,
    endpoint_id: &str,
    volume: f32,
    finalize: bool,
) -> Result<(), String> {
    let vol = volume.clamp(0.0, 1.0);
    if pid == 0 {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        let ep_key = endpoint_id.to_string();
        let path = guard.app_settings_path.clone();
        guard
            .app_settings
            .upsert_by_key_or_pid(pid, app_key, &path, |s| {
                s.output_volumes.insert(ep_key.clone(), vol);
                let is_primary = s.primary_endpoint_id.as_deref() == Some(ep_key.as_str())
                    || s.primary_endpoint_id.is_none();
                if is_primary {
                    if s.primary_endpoint_id.is_none() {
                        s.primary_endpoint_id = Some(ep_key.clone());
                    }
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

    let prev_primary_vol = primary_vol;
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

    if has_audible_loopback && endpoint_id == cur_primary && primary_eff < prev_primary_vol {
        // Apply ISimpleAudioVolume immediately; never raise loopback gain mid-drag,
        // and on finalize wait 100ms for Windows' mix buffer to flush before catching up.
        apply_primary_volume(&eps, &cur_primary, pid, primary_eff);
        if finalize {
            thread::sleep(Duration::from_millis(150));
            apply_loopback_ratios(&mut guard.loopbacks, pid, primary_eff);
        }
    } else {
        apply_primary_volume(&eps, &cur_primary, pid, primary_eff);
        apply_loopback_ratios(&mut guard.loopbacks, pid, primary_eff);
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
        s.output_volumes.insert(ep_key.clone(), vol);
        let mut merged = updated_secs;
        for sec in &s.secondary_outputs {
            if !merged.iter().any(|m| m.endpoint_id == sec.endpoint_id) {
                let mut kept = sec.clone();
                if kept.endpoint_id == ep_key {
                    kept.volume = vol;
                    kept.muted = vol <= 0.0;
                }
                merged.push(kept);
            }
        }
        s.secondary_outputs = merged;
    });

    Ok(())
}

fn unique_preset_id(existing: &[Preset], name: &str) -> String {
    let mut base = String::new();
    let mut prev_dash = false;
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            base.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !base.is_empty() {
            base.push('-');
            prev_dash = true;
        }
    }
    let base = base.trim_matches('-').to_string();
    let base = if base.is_empty() {
        "preset".to_string()
    } else {
        base
    };

    if let Some(found) = existing.iter().find(|p| p.name == name) {
        return found.id.clone();
    }

    if !existing.iter().any(|p| p.id == base) {
        return base;
    }

    let mut suffix = 2u32;
    loop {
        let candidate = format!("{base}-{suffix}");
        if !existing.iter().any(|p| p.id == candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

fn handle_apply_preset(
    enumerator: &AudioEnumerator,
    state: &Arc<Mutex<DaemonState>>,
    id: String,
) -> (IpcResponse, bool) {
    let preset = {
        let mut guard = state.lock().expect("state lock");
        let Some(preset) = guard.config.presets.iter().find(|p| p.id == id).cloned() else {
            return (
                IpcResponse::Error {
                    message: format!("Preset '{id}' not found"),
                },
                false,
            );
        };
        guard.config.active_preset_id = Some(id.clone());
        let path = guard.config_path.clone();
        let _ = guard.config.save_to_file(&path);
        preset
    };

    let snapshot = match PresetEngine::new(enumerator).apply_preset(&preset) {
        Ok(s) => s,
        Err(e) => {
            log_error!("ApplyPreset(id={id}) failed: {e}");
            return (
                IpcResponse::Error {
                    message: e.to_string(),
                },
                false,
            );
        }
    };

    // Re-apply secondary loopback outputs declared in the preset.
    if let Ok(eps) = enumerator.list_active_endpoints(DataFlow::Render) {
        let mut guard = state.lock().expect("state lock");
        if guard.pre_preset_snapshot.is_none() {
            guard.pre_preset_snapshot = Some(snapshot);
        }
        for target in &preset.endpoints {
            if let Some(ep) = eps.iter().find(|e| {
                target.endpoint_id.as_deref() == Some(e.id.as_str())
                    || target
                        .friendly_name_pattern
                        .as_deref()
                        .map(|pat| e.friendly_name.to_lowercase().contains(&pat.to_lowercase()))
                        .unwrap_or(false)
            }) {
                for &role in &target.default_roles {
                    guard.default_endpoints.insert((target.flow, role), ep.id.clone());
                }
            }
        }
        guard.loopbacks.clear();
        for rule in &preset.app_loopbacks {
            let needle = rule.executable_name.to_lowercase();
            let Some(target_ep) = eps.iter().find(|e| e.id == rule.target_endpoint_id) else {
                continue;
            };
            let mut matched_pid: Option<(u32, f32)> = None;
            for ep in &eps {
                if let Ok(sessions) = ep.sessions() {
                    for s in sessions {
                        if s.pid == 0 {
                            continue;
                        }
                        let root = resolve_root_exe_name(s.pid)
                            .unwrap_or_else(|| s.exe_name.clone());
                        if s.exe_name.to_lowercase() == needle || root.to_lowercase() == needle {
                            let vol = s.volume().unwrap_or(1.0);
                            if s.is_active || matched_pid.is_none() {
                                matched_pid = Some((s.pid, vol));
                            }
                        }
                    }
                }
            }
            if let Some((pid, primary_vol)) = matched_pid {
                let vol = rule.volume.clamp(0.0, 1.0);
                if let Ok(handle) = LoopbackTee::start(pid, target_ep, vol) {
                    let ratio = (vol / primary_vol.max(0.05)).clamp(0.0, 4.0);
                    handle.set_linear_gain(ratio.powi(2));
                    handle.set_mute(rule.muted || vol <= 0.0);
                    guard.loopbacks.insert(
                        (pid, target_ep.id.clone()),
                        ActiveLoopback {
                            handle,
                            volume: vol,
                            muted: rule.muted || vol <= 0.0,
                        },
                    );
                }
            }
        }
    }

    log_info!("ApplyPreset(id={id}) applied");
    (IpcResponse::Ok, false)
}

fn handle_save_current_as_preset(
    enumerator: &AudioEnumerator,
    state: &Arc<Mutex<DaemonState>>,
    raw_name: String,
) -> (IpcResponse, bool) {
    let name = raw_name.trim().to_string();
    if name.is_empty() {
        return (
            IpcResponse::Error {
                message: "Preset name cannot be empty".to_string(),
            },
            false,
        );
    }

    let mut endpoints = Vec::new();
    let mut app_routes = Vec::new();
    let mut seen_route_exes = std::collections::HashSet::new();

    let cached_defaults = state
        .lock()
        .map(|g| g.default_endpoints.clone())
        .unwrap_or_default();

    for flow in [DataFlow::Render, DataFlow::Capture] {
        let def_console = cached_defaults
            .get(&(flow, DeviceRole::Console))
            .cloned()
            .or_else(|| enumerator.default_endpoint_id(flow, DeviceRole::Console).ok());
        let def_mm = cached_defaults
            .get(&(flow, DeviceRole::Multimedia))
            .cloned()
            .or_else(|| {
                enumerator
                    .default_endpoint_id(flow, DeviceRole::Multimedia)
                    .ok()
            });
        let def_comm = cached_defaults
            .get(&(flow, DeviceRole::Communications))
            .cloned()
            .or_else(|| {
                enumerator
                    .default_endpoint_id(flow, DeviceRole::Communications)
                    .ok()
            });

        let Ok(eps) = enumerator.list_active_endpoints(flow) else {
            continue;
        };
        for ep in eps {
            let mut default_roles = Vec::new();
            if def_console.as_deref() == Some(ep.id.as_str()) {
                default_roles.push(DeviceRole::Console);
            }
            if def_mm.as_deref() == Some(ep.id.as_str()) {
                default_roles.push(DeviceRole::Multimedia);
            }
            if def_comm.as_deref() == Some(ep.id.as_str()) {
                default_roles.push(DeviceRole::Communications);
            }

            let vol_state = ep.master_volume().ok();
            let mut app_rules = Vec::new();

            if flow == DataFlow::Render {
                if let Ok(sessions) = ep.sessions() {
                    let mut seen_ep_exes = std::collections::HashSet::new();
                    for s in sessions {
                        if s.pid == 0 {
                            continue;
                        }
                        let root = resolve_root_exe_name(s.pid)
                            .unwrap_or_else(|| s.exe_name.clone());
                        let lower = root.to_ascii_lowercase();
                        if matches!(lower.as_str(), "audio-daemon.exe" | "audio-ui.exe") {
                            continue;
                        }
                        if seen_ep_exes.insert(lower.clone()) {
                            app_rules.push(AppVolumeRule {
                                executable_name: root.clone(),
                                target_volume_scalar: s.volume().unwrap_or(1.0),
                                is_muted: s.is_muted().unwrap_or(false),
                            });
                        }
                        if s.is_active && seen_route_exes.insert(lower) {
                            app_routes.push(AppRouteRule {
                                executable_name: root,
                                flow: DataFlow::Render,
                                roles: vec![DeviceRole::Console, DeviceRole::Multimedia],
                                target_endpoint_id: Some(ep.id.clone()),
                                target_friendly_name_pattern: Some(ep.friendly_name.clone()),
                            });
                        }
                    }
                }
            }

            endpoints.push(EndpointTarget {
                flow,
                endpoint_id: Some(ep.id.clone()),
                friendly_name_pattern: Some(ep.friendly_name.clone()),
                default_roles,
                master_volume: vol_state.map(|v| v.volume),
                master_mute: vol_state.map(|v| v.muted),
                default_fallback_volume: None,
                app_rules,
            });
        }
    }

    let mut guard = state.lock().expect("state lock");
    let mut app_loopbacks = Vec::new();
    let mut seen_lb_keys = std::collections::HashSet::new();
    for ((pid, target_ep_id), lb) in &guard.loopbacks {
        if let Some(root) = resolve_root_exe_name(*pid) {
            seen_lb_keys.insert((root.to_ascii_lowercase(), target_ep_id.clone()));
            app_loopbacks.push(AppLoopbackRule {
                executable_name: root,
                target_endpoint_id: target_ep_id.clone(),
                volume: lb.volume,
                muted: lb.muted,
            });
        }
    }
    for (exe_path, app_cfg) in &guard.app_settings.apps {
        let exe_name = std::path::Path::new(exe_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(exe_path)
            .to_string();
        for sec in &app_cfg.secondary_outputs {
            if seen_lb_keys.insert((exe_name.to_ascii_lowercase(), sec.endpoint_id.clone())) {
                app_loopbacks.push(AppLoopbackRule {
                    executable_name: exe_name.clone(),
                    target_endpoint_id: sec.endpoint_id.clone(),
                    volume: sec.volume,
                    muted: sec.muted,
                });
            }
        }
    }

    let id = unique_preset_id(&guard.config.presets, &name);
    let preset = Preset {
        id: id.clone(),
        name: name.clone(),
        endpoints,
        app_routes,
        app_loopbacks,
    };

    if let Some(existing) = guard.config.presets.iter_mut().find(|p| p.id == id) {
        *existing = preset;
    } else {
        guard.config.presets.push(preset);
    }
    guard.config.active_preset_id = Some(id.clone());
    let path = guard.config_path.clone();
    let res = guard.config.save_to_file(&path);
    log_info!("SaveCurrentAsPreset(id={id}, name='{name}')");
    ipc_ok(res)
}

fn apply_primary_volume(
    eps: &[audio_core::AudioEndpoint],
    cur_primary: &str,
    pid: u32,
    vol: f32,
) {
    if let Some(ep) = eps.iter().find(|e| e.id == cur_primary) {
        if let Ok(sessions) = ep.sessions() {
            for s in sessions {
                if s.pid == pid {
                    let _ = s.set_volume(vol);
                    let _ = s.set_mute(vol <= 0.0);
                }
            }
        }
    }
}

fn apply_loopback_ratios(
    loopbacks: &mut std::collections::HashMap<(u32, String), ActiveLoopback>,
    pid: u32,
    primary_eff: f32,
) {
    for ((p, _), lb) in loopbacks.iter_mut() {
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
}
