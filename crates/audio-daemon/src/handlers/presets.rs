use std::sync::{Arc, Mutex};

use audio_core::ipc::IpcResponse;
use audio_core::{
    log_error, log_info, resolve_root_exe_name, AppLoopbackRule, AppRouteRule, AppVolumeRule,
    AudioEnumerator, DataFlow, DeviceRole, EndpointTarget, LoopbackTee, Preset, PresetEngine,
};

use super::ipc_ok;
use crate::app_settings::ActiveLoopback;
use crate::state::DaemonState;

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

pub(super) fn handle_apply_preset(
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

pub(super) fn handle_save_current_as_preset(
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


#[cfg(test)]
mod tests {
    use super::*;

    fn preset(id: &str, name: &str) -> Preset {
        Preset {
            id: id.to_string(),
            name: name.to_string(),
            endpoints: Vec::new(),
            app_routes: Vec::new(),
            app_loopbacks: Vec::new(),
        }
    }

    #[test]
    fn unique_preset_id_slugifies_and_handles_collisions() {
        let existing = vec![
            preset("gaming-mode", "Gaming Mode"),
            preset("gaming-mode-2", "Gaming Mode!"),
        ];
        assert_eq!(unique_preset_id(&existing, "Gaming Mode"), "gaming-mode");
        assert_eq!(unique_preset_id(&existing, "Gaming  Mode"), "gaming-mode-3");
        assert_eq!(unique_preset_id(&existing, "   "), "preset");
    }
}