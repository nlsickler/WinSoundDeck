use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use audio_core::{AudioEndpoint, AudioEnumerator, DataFlow};

use crate::app_settings::AppPersistentSettings;
use crate::loopback_registry::effective_primary_volume;
use crate::state::DaemonState;
use crate::status::{find_primary_session, is_pid_active};

use super::app_routing::{select_promotion_candidate, try_promote_secondary};

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
    let primary_eff = effective_primary_volume(primary_vol, has_audible_loopback);

    if has_audible_loopback && endpoint_id == cur_primary && primary_eff < prev_primary_vol {
        // Apply ISimpleAudioVolume immediately; never raise loopback gain mid-drag,
        // and on finalize wait 150ms for Windows' mix buffer to flush before catching up.
        apply_primary_volume(&eps, &cur_primary, pid, primary_eff);
        if finalize {
            thread::sleep(Duration::from_millis(150));
            guard.loopbacks.apply_ratios_for_pid(pid, primary_eff);
        }
    } else {
        apply_primary_volume(&eps, &cur_primary, pid, primary_eff);
        guard.loopbacks.apply_ratios_for_pid(pid, primary_eff);
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

fn apply_primary_volume(eps: &[AudioEndpoint], cur_primary: &str, pid: u32, vol: f32) {
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
