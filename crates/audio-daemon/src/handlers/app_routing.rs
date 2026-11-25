use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use audio_core::ipc::IpcResponse;
use audio_core::{
    log_error, log_info, log_warn, AppPolicyClient, AudioEndpoint, AudioEnumerator, DataFlow,
    DeviceRole, LoopbackTee,
};

use crate::app_settings::SecondaryOutputSetting;
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

pub(super) fn select_promotion_candidate(
    cur_primary: &str,
    primary_vol: f32,
    secondary_vols: &[(String, f32)],
    finalize: bool,
) -> Option<(String, f32)> {
    if !finalize || primary_vol > 0.0 || secondary_vols.is_empty() {
        return None;
    }
    let mut best_id = cur_primary;
    let mut best_vol = primary_vol;
    for (ep_id, v) in secondary_vols {
        if *v > best_vol + 0.005 {
            best_vol = *v;
            best_id = ep_id.as_str();
        }
    }
    (best_id != cur_primary).then(|| (best_id.to_string(), best_vol))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn try_promote_secondary<'a>(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_promotion_candidate_requires_finalize_and_zero_primary() {
        let secs = vec![("ep-sec".to_string(), 0.6)];
        assert_eq!(select_promotion_candidate("ep-pri", 0.0, &secs, false), None);
        assert_eq!(select_promotion_candidate("ep-pri", 0.1, &secs, true), None);
        assert_eq!(
            select_promotion_candidate("ep-pri", 0.0, &secs, true),
            Some(("ep-sec".to_string(), 0.6))
        );
    }

    #[test]
    fn select_promotion_candidate_picks_loudest_above_threshold() {
        let secs = vec![
            ("ep-quiet".to_string(), 0.004),
            ("ep-mid".to_string(), 0.3),
            ("ep-loud".to_string(), 0.8),
        ];
        assert_eq!(
            select_promotion_candidate("ep-pri", 0.0, &secs, true),
            Some(("ep-loud".to_string(), 0.8))
        );
        let below_threshold = vec![("ep-quiet".to_string(), 0.004)];
        assert_eq!(
            select_promotion_candidate("ep-pri", 0.0, &below_threshold, true),
            None
        );
    }
}
