use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use audio_core::ipc::IpcResponse;
use audio_core::{log_error, log_info, log_warn, AppPolicyClient, AudioEnumerator, DataFlow, DeviceRole, LoopbackTee};

use crate::app_settings::{ActiveLoopback, SecondaryOutputSetting};
use crate::state::DaemonState;
use crate::status::{find_primary_session, is_pid_active};

pub(super) fn handle_route_app(
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

pub(super) fn handle_start_loopback(
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

pub(super) fn handle_set_app_output_volume(
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