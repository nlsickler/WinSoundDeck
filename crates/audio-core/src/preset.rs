use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
    resolve_root_exe_name, AppPolicyClient, AudioEndpoint, AudioEnumerator, DataFlow, DeviceRole,
    EndpointVolumeState, PolicyClient, Result,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetConfig {
    pub version: String,
    pub active_preset_id: Option<String>,
    pub presets: Vec<Preset>,
}

impl PresetConfig {
    pub fn load_from_file(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let raw = fs::read_to_string(path)?;
        serde_json::from_str(&raw)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    pub fn save_to_file(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(path, json)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub endpoints: Vec<EndpointTarget>,
    #[serde(default)]
    pub app_routes: Vec<AppRouteRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointTarget {
    pub flow: DataFlow,
    #[serde(default)]
    pub endpoint_id: Option<String>,
    #[serde(default)]
    pub friendly_name_pattern: Option<String>,
    #[serde(default)]
    pub default_roles: Vec<DeviceRole>,
    #[serde(default)]
    pub master_volume: Option<f32>,
    #[serde(default)]
    pub master_mute: Option<bool>,
    #[serde(default)]
    pub default_fallback_volume: Option<f32>,
    #[serde(default)]
    pub app_rules: Vec<AppVolumeRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppVolumeRule {
    pub executable_name: String,
    pub target_volume_scalar: f32,
    #[serde(default)]
    pub is_muted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRouteRule {
    pub executable_name: String,
    pub flow: DataFlow,
    #[serde(default)]
    pub roles: Vec<DeviceRole>,
    #[serde(default)]
    pub target_endpoint_id: Option<String>,
    #[serde(default)]
    pub target_friendly_name_pattern: Option<String>,
}

/// Captures pre-preset OS defaults, endpoint master levels, and session levels for clean undo.
pub struct PresetSnapshot {
    default_roles: Vec<(DeviceRole, DataFlow, String)>,
    endpoint_volumes: Vec<(DataFlow, String, EndpointVolumeState)>,
    session_volumes: Vec<(DataFlow, String, u32, f32, bool)>,
    app_routes: Vec<(u32, DataFlow, DeviceRole, String)>,
}

impl PresetSnapshot {
    pub fn restore(self, enumerator: &AudioEnumerator) -> Result<()> {
        if !self.app_routes.is_empty() {
            if let Ok(app_policy) = AppPolicyClient::new() {
                for (pid, flow, role, orig_swd) in self.app_routes {
                    let _ = app_policy.set_app_endpoint_swd(pid, flow, role, &orig_swd);
                }
            }
        }

        if !self.default_roles.is_empty() {
            let policy = PolicyClient::new()?;
            for (role, _flow, orig_id) in self.default_roles {
                let _ = policy.set_default_endpoint(&orig_id, role);
            }
        }

        for (flow, ep_id, state) in self.endpoint_volumes {
            if let Some(ep) = find_endpoint_by_id(enumerator, flow, &ep_id)? {
                let _ = ep.set_master_volume(state.volume);
                let _ = ep.set_master_mute(state.muted);
            }
        }

        for (flow, ep_id, pid, vol, muted) in self.session_volumes {
            if let Some(ep) = find_endpoint_by_id(enumerator, flow, &ep_id)? {
                if let Ok(sessions) = ep.sessions() {
                    for s in sessions.into_iter().filter(|s| s.pid == pid) {
                        let _ = s.set_volume(vol);
                        let _ = s.set_mute(muted);
                    }
                }
            }
        }

        Ok(())
    }
}

/// Deterministic enforcement engine for multi-endpoint `Preset` configurations.
pub struct PresetEngine<'a> {
    enumerator: &'a AudioEnumerator,
}

impl<'a> PresetEngine<'a> {
    pub fn new(enumerator: &'a AudioEnumerator) -> Self {
        Self { enumerator }
    }

    pub fn apply_preset(&self, preset: &Preset) -> Result<PresetSnapshot> {
        let render_eps = self.enumerator.list_active_endpoints(DataFlow::Render)?;
        let capture_eps = self.enumerator.list_active_endpoints(DataFlow::Capture)?;

        let mut snapshot = PresetSnapshot {
            default_roles: Vec::new(),
            endpoint_volumes: Vec::new(),
            session_volumes: Vec::new(),
            app_routes: Vec::new(),
        };

        let policy = PolicyClient::new()?;

        for target in &preset.endpoints {
            let pool = match target.flow {
                DataFlow::Render => &render_eps,
                DataFlow::Capture => &capture_eps,
            };
            let Some(ep) = match_endpoint(
                pool,
                target.endpoint_id.as_deref(),
                target.friendly_name_pattern.as_deref(),
            ) else {
                continue;
            };

            for &role in &target.default_roles {
                if let Ok(orig_id) = self.enumerator.default_endpoint_id(target.flow, role) {
                    snapshot.default_roles.push((role, target.flow, orig_id));
                }
                policy.set_default_endpoint(&ep.id, role)?;
            }

            if target.master_volume.is_some() || target.master_mute.is_some() {
                if let Ok(orig_state) = ep.master_volume() {
                    snapshot
                        .endpoint_volumes
                        .push((target.flow, ep.id.clone(), orig_state));
                }
                if let Some(vol) = target.master_volume {
                    ep.set_master_volume(vol)?;
                }
                if let Some(mute) = target.master_mute {
                    ep.set_master_mute(mute)?;
                }
            }

            if target.flow == DataFlow::Render
                && (!target.app_rules.is_empty() || target.default_fallback_volume.is_some())
            {
                if let Ok(sessions) = ep.sessions() {
                    for session in sessions {
                        let root = resolve_root_exe_name(session.pid)
                            .unwrap_or_else(|| session.exe_name.clone());
                        if let Some((vol, muted)) =
                            resolve_rule_for_app(target, &session.exe_name, &root)
                        {
                            if let (Ok(orig_v), Ok(orig_m)) =
                                (session.volume(), session.is_muted())
                            {
                                snapshot.session_volumes.push((
                                    target.flow,
                                    ep.id.clone(),
                                    session.pid,
                                    orig_v,
                                    orig_m,
                                ));
                            }
                            let _ = session.set_volume(vol);
                            let _ = session.set_mute(muted);
                        }
                    }
                }
            }
        }

        if !preset.app_routes.is_empty() {
            let app_policy = AppPolicyClient::new()?;
            for route in &preset.app_routes {
                let pool = match route.flow {
                    DataFlow::Render => &render_eps,
                    DataFlow::Capture => &capture_eps,
                };
                let Some(target_ep) = match_endpoint(
                    pool,
                    route.target_endpoint_id.as_deref(),
                    route.target_friendly_name_pattern.as_deref(),
                ) else {
                    continue;
                };

                let needle = route.executable_name.to_lowercase();
                for ep in pool {
                    if let Ok(sessions) = ep.sessions() {
                        for s in sessions {
                            let root = resolve_root_exe_name(s.pid)
                                .unwrap_or_else(|| s.exe_name.clone());
                            if s.exe_name.to_lowercase() == needle || root.to_lowercase() == needle
                            {
                                let roles = if route.roles.is_empty() {
                                    &[DeviceRole::Console, DeviceRole::Multimedia][..]
                                } else {
                                    &route.roles[..]
                                };
                                for &role in roles {
                                    if let Ok(orig_swd) =
                                        app_policy.get_app_endpoint(s.pid, route.flow, role)
                                    {
                                        snapshot.app_routes.push((
                                            s.pid,
                                            route.flow,
                                            role,
                                            orig_swd,
                                        ));
                                    }
                                    app_policy.set_app_endpoint(
                                        s.pid,
                                        route.flow,
                                        role,
                                        &target_ep.id,
                                    )?;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(snapshot)
    }

    /// Reactive catch for `AudioEvent::SessionCreated`: applies matching preset rules to the new session.
    pub fn handle_session_created(
        &self,
        preset: &Preset,
        endpoint_id: &str,
        pid: u32,
        exe_name: &str,
        root_exe_name: &str,
    ) -> Result<bool> {
        let Some(ep) = find_endpoint_by_id(self.enumerator, DataFlow::Render, endpoint_id)? else {
            return Ok(false);
        };
        let Some(target) = preset.endpoints.iter().find(|t| {
            t.flow == DataFlow::Render
                && (t.endpoint_id.as_deref() == Some(endpoint_id)
                    || t.friendly_name_pattern.as_deref().is_some_and(|p| {
                        ep.friendly_name.to_lowercase().contains(&p.to_lowercase())
                    }))
        }) else {
            return Ok(false);
        };

        let Some((vol, muted)) = resolve_rule_for_app(target, exe_name, root_exe_name) else {
            return Ok(false);
        };

        let mut applied = false;
        for session in ep.sessions()? {
            if session.pid == pid {
                session.set_volume(vol)?;
                session.set_mute(muted)?;
                applied = true;
            }
        }
        Ok(applied)
    }
}

fn resolve_rule_for_app(
    target: &EndpointTarget,
    exe_name: &str,
    root_exe_name: &str,
) -> Option<(f32, bool)> {
    let exe_l = exe_name.to_lowercase();
    let root_l = root_exe_name.to_lowercase();
    if let Some(rule) = target.app_rules.iter().find(|r| {
        let rule_l = r.executable_name.to_lowercase();
        rule_l == exe_l || rule_l == root_l
    }) {
        return Some((rule.target_volume_scalar, rule.is_muted));
    }
    target.default_fallback_volume.map(|v| (v, false))
}

fn match_endpoint<'a>(
    pool: &'a [AudioEndpoint],
    endpoint_id: Option<&str>,
    pattern: Option<&str>,
) -> Option<&'a AudioEndpoint> {
    if let Some(id) = endpoint_id {
        if let Some(ep) = pool.iter().find(|ep| ep.id == id) {
            return Some(ep);
        }
    }
    if let Some(pat) = pattern {
        let needle = pat.to_lowercase();
        return pool
            .iter()
            .find(|ep| ep.friendly_name.to_lowercase().contains(&needle));
    }
    None
}

fn find_endpoint_by_id(
    enumerator: &AudioEnumerator,
    flow: DataFlow,
    endpoint_id: &str,
) -> Result<Option<AudioEndpoint>> {
    let eps = enumerator.list_active_endpoints(flow)?;
    Ok(eps.into_iter().find(|ep| ep.id == endpoint_id))
}
