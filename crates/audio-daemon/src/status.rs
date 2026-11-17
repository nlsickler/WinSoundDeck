use std::collections::HashMap;

use audio_core::ipc::{DaemonStatus, EndpointStatus, LoopbackStatus, SessionStatus};
use audio_core::{
    resolve_app_display_name, resolve_root_exe_name, AudioEndpoint, AudioEnumerator, DataFlow,
    DeviceRole,
};

use crate::app_settings::AppSettingsStore;
use crate::state::DaemonState;

const INTERNAL_EXES: &[&str] = &["audio-daemon.exe", "audio-ui.exe"];

fn is_internal_session(exe_name: &str, root_exe_name: &str) -> bool {
    INTERNAL_EXES.contains(&exe_name.to_ascii_lowercase().as_str())
        || INTERNAL_EXES.contains(&root_exe_name.to_ascii_lowercase().as_str())
}

pub fn is_pid_active(eps: &[AudioEndpoint], pid: u32) -> bool {
    eps.iter().any(|ep| {
        ep.sessions()
            .map(|sessions| sessions.into_iter().any(|s| s.pid == pid && s.is_active))
            .unwrap_or(false)
    })
}

pub fn find_primary_session(eps: &[AudioEndpoint], pid: u32) -> Option<(String, f32)> {
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

pub fn build_daemon_status(
    enumerator: &AudioEnumerator,
    state: &DaemonState,
) -> audio_core::Result<DaemonStatus> {
    let active_id = state.config.active_preset_id.clone();
    let mut status = collect_raw_status(enumerator, active_id, &state.default_endpoints)?;

    for ep in &mut status.endpoints {
        ep.hidden = state.app_settings.hidden_endpoints.contains(&ep.id);
    }

    let mut merged_loopbacks: Vec<LoopbackStatus> = state
        .loopbacks
        .iter()
        .map(|((pid, ep_id), lb)| LoopbackStatus {
            pid: *pid,
            target_endpoint_id: ep_id.clone(),
            volume: lb.volume,
            muted: lb.muted,
        })
        .collect();

    for s in &mut status.sessions {
        let Some(key) = AppSettingsStore::key_for_pid(s.pid) else {
            continue;
        };
        let Some(cfg) = state.app_settings.apps.get(&key) else {
            continue;
        };
        s.saved_volumes = cfg.output_volumes.clone();
        if !s.is_active {
            if let Some(ref ep_id) = cfg.primary_endpoint_id {
                s.endpoint_id = ep_id.clone();
            }
            if let Some(&v) = cfg.output_volumes.get(&s.endpoint_id) {
                s.volume = v;
            } else if let Some(v) = cfg.volume {
                s.volume = v;
            }
            if let Some(m) = cfg.muted {
                s.muted = m;
            }
            for sec in &cfg.secondary_outputs {
                if !merged_loopbacks
                    .iter()
                    .any(|lb| lb.pid == s.pid && lb.target_endpoint_id == sec.endpoint_id)
                {
                    merged_loopbacks.push(LoopbackStatus {
                        pid: s.pid,
                        target_endpoint_id: sec.endpoint_id.clone(),
                        volume: sec.volume,
                        muted: sec.muted,
                    });
                }
            }
        }
    }

    status.loopbacks = merged_loopbacks;
    Ok(status)
}

fn collect_raw_status(
    enumerator: &AudioEnumerator,
    active_preset_id: Option<String>,
    default_endpoints: &HashMap<(DataFlow, DeviceRole), String>,
) -> audio_core::Result<DaemonStatus> {
    let def_render_mm = default_endpoints
        .get(&(DataFlow::Render, DeviceRole::Multimedia))
        .cloned()
        .or_else(|| {
            enumerator
                .default_endpoint_id(DataFlow::Render, DeviceRole::Multimedia)
                .ok()
        })
        .unwrap_or_default();
    let def_render_comm = default_endpoints
        .get(&(DataFlow::Render, DeviceRole::Communications))
        .cloned()
        .or_else(|| {
            enumerator
                .default_endpoint_id(DataFlow::Render, DeviceRole::Communications)
                .ok()
        })
        .unwrap_or_default();
    let def_cap_mm = default_endpoints
        .get(&(DataFlow::Capture, DeviceRole::Multimedia))
        .cloned()
        .or_else(|| {
            enumerator
                .default_endpoint_id(DataFlow::Capture, DeviceRole::Multimedia)
                .ok()
        })
        .unwrap_or_default();
    let def_cap_comm = default_endpoints
        .get(&(DataFlow::Capture, DeviceRole::Communications))
        .cloned()
        .or_else(|| {
            enumerator
                .default_endpoint_id(DataFlow::Capture, DeviceRole::Communications)
                .ok()
        })
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
                hidden: false,
            });

            if flow != DataFlow::Render {
                continue;
            }
            let Ok(ep_sessions) = ep.sessions() else {
                continue;
            };
            for s in ep_sessions {
                let root_exe_name =
                    resolve_root_exe_name(s.pid).unwrap_or_else(|| s.exe_name.clone());
                if is_internal_session(&s.exe_name, &root_exe_name) {
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
                let display_name =
                    resolve_app_display_name(s.pid).unwrap_or_else(|| root_exe_name.clone());
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
