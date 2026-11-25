use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use audio_core::ipc::IpcResponse;
use audio_core::{
    log_error, log_info, log_warn, AppPolicyClient, AudioEndpoint, AudioEnumerator, DataFlow,
    DeviceRole, LoopbackTee,
};

use crate::app_settings::{AppPersistentSettings, SecondaryOutputSetting};
use crate::state::DaemonState;
use crate::status::{find_primary_session, is_pid_active};

use super::volume_plan::{
    plan_volume_application, select_promotion_candidate, LoopbackRatioTiming,
};

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
                Ok(Some((handle, primary_vol)))
            })
    };

    match res {
        Ok(maybe_started) => {
            if let Some((handle, primary_vol)) = maybe_started {
                log_info!(
                    "StartLoopback(pid={pid}, target={target_endpoint_id}, vol={vol:.2}) started"
                );
                if let Ok(mut guard) = state.lock() {
                    guard.loopbacks.insert_started(
                        pid,
                        target_endpoint_id.clone(),
                        handle,
                        vol,
                        false,
                        primary_vol,
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
        let path = guard.app_settings_path.clone();
        guard
            .app_settings
            .upsert_by_key_or_pid(pid, app_key, &path, |s| {
                update_inactive_output_settings(s, endpoint_id, endpoint_id, vol);
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
        let path = guard.app_settings_path.clone();
        guard.app_settings.upsert_for_pid(pid, &path, |s| {
            update_inactive_output_settings(s, endpoint_id, &cur_primary, vol);
        });
        return Ok(());
    }

    let connected_eps: HashSet<String> = eps.iter().map(|e| e.id.clone()).collect();
    guard.loopbacks.prune_disconnected(&eps);
    let connected_refs: HashSet<&str> = connected_eps.iter().map(String::as_str).collect();

    let prev_primary_vol = primary_vol;
    if endpoint_id == cur_primary {
        primary_vol = vol;
    } else {
        guard.loopbacks.set_volume(pid, endpoint_id, vol);
    }

    let secondary_vols =
        guard
            .loopbacks
            .connected_secondaries_for_pid(pid, &cur_primary, &connected_refs);
    if let Some((best_id, best_vol)) =
        select_promotion_candidate(&cur_primary, primary_vol, &secondary_vols, finalize)
    {
        let (next_guard, promoted) = try_promote_secondary(
            enumerator,
            state,
            guard,
            &eps,
            pid,
            &cur_primary,
            primary_vol,
            &best_id,
            best_vol,
        )?;
        guard = next_guard;
        if promoted {
            cur_primary = best_id;
            primary_vol = best_vol;
        }
    }

    let has_audible_loopback =
        guard
            .loopbacks
            .has_audible_for_pid(pid, &cur_primary, &connected_refs);
    let plan = plan_volume_application(
        primary_vol,
        prev_primary_vol,
        endpoint_id == cur_primary,
        has_audible_loopback,
        finalize,
    );

    apply_primary_volume(&eps, &cur_primary, pid, plan.primary_eff);
    match plan.ratio_timing {
        LoopbackRatioTiming::Immediate => {
            guard.loopbacks.apply_ratios_for_pid(pid, plan.primary_eff);
        }
        LoopbackRatioTiming::DelayedAfterFlush => {
            let expected_gen = guard.loopbacks.generation();
            let state_bg = Arc::clone(state);
            let primary_eff = plan.primary_eff;
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(150));
                if let Ok(mut g) = state_bg.lock() {
                    if !g.shutdown {
                        g.loopbacks
                            .apply_ratios_for_pid_if_gen(pid, primary_eff, expected_gen);
                    }
                }
            });
        }
        LoopbackRatioTiming::SkipMidDrag => {}
    }

    persist_active_output_settings(&mut guard, pid, &cur_primary, primary_vol, endpoint_id, vol);
    Ok(())
}

fn update_inactive_output_settings(
    s: &mut AppPersistentSettings,
    ep_key: &str,
    default_primary: &str,
    vol: f32,
) {
    s.output_volumes.insert(ep_key.to_string(), vol);
    let primary_id = s
        .primary_endpoint_id
        .get_or_insert_with(|| default_primary.to_string());
    if primary_id == ep_key {
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
}

fn try_promote_secondary<'a>(
    enumerator: &AudioEnumerator,
    state: &'a Arc<Mutex<DaemonState>>,
    mut guard: std::sync::MutexGuard<'a, DaemonState>,
    eps: &[AudioEndpoint],
    pid: u32,
    old_primary: &str,
    old_primary_vol: f32,
    best_id: &str,
    best_vol: f32,
) -> Result<(std::sync::MutexGuard<'a, DaemonState>, bool), String> {
    if let Ok(ap) = AppPolicyClient::new() {
        let _ = ap.set_app_endpoint(pid, DataFlow::Render, DeviceRole::Console, best_id);
        let _ = ap.set_app_endpoint(pid, DataFlow::Render, DeviceRole::Multimedia, best_id);
    }
    drop(guard);
    thread::sleep(Duration::from_millis(250));
    let migrated_primary = enumerator
        .list_active_endpoints(DataFlow::Render)
        .ok()
        .and_then(|new_eps| find_primary_session(&new_eps, pid).map(|(id, _)| id));
    guard = state.lock().map_err(|e| e.to_string())?;

    if migrated_primary.as_deref() != Some(best_id) {
        return Ok((guard, false));
    }

    guard.loopbacks.remove(pid, best_id);
    if let Some(old_ep) = eps.iter().find(|e| e.id == old_primary) {
        if let Ok(handle) = LoopbackTee::start(pid, old_ep, old_primary_vol) {
            guard.loopbacks.insert_started(
                pid,
                old_primary.to_string(),
                handle,
                old_primary_vol,
                false,
                best_vol,
            );
        }
    }
    Ok((guard, true))
}

fn persist_active_output_settings(
    guard: &mut DaemonState,
    pid: u32,
    cur_primary: &str,
    primary_vol: f32,
    endpoint_id: &str,
    vol: f32,
) {
    let updated_secs = guard.loopbacks.secondary_settings_for_pid(pid);
    let path = guard.app_settings_path.clone();
    let ep_key = endpoint_id.to_string();
    let primary_owned = cur_primary.to_string();
    guard.app_settings.upsert_for_pid(pid, &path, |s| {
        s.primary_endpoint_id = Some(primary_owned);
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