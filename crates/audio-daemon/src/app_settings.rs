use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use audio_core::ipc::SessionStatus;
use audio_core::{
    resolve_root_exe_path, AppPolicyClient, AudioEnumerator, DataFlow, DeviceRole, LoopbackHandle,
    LoopbackTee,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecondaryOutputSetting {
    pub endpoint_id: String,
    pub volume: f32,
    #[serde(default)]
    pub muted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppPersistentSettings {
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub root_exe_name: Option<String>,
    #[serde(default)]
    pub primary_endpoint_id: Option<String>,
    #[serde(default)]
    pub volume: Option<f32>,
    #[serde(default)]
    pub muted: Option<bool>,
    #[serde(default)]
    pub secondary_outputs: Vec<SecondaryOutputSetting>,
    #[serde(default)]
    pub output_volumes: HashMap<String, f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettingsStore {
    #[serde(default)]
    pub apps: HashMap<String, AppPersistentSettings>,
    #[serde(default)]
    pub allowed_endpoints: Option<HashSet<String>>,
    #[serde(default)]
    pub include_disconnected: bool,
}

pub struct ActiveLoopback {
    pub handle: LoopbackHandle,
    pub volume: f32,
    pub muted: bool,
}

impl AppSettingsStore {
    pub fn path_beside_config(config_path: &Path) -> PathBuf {
        config_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .join("app_settings.json")
    }

    pub fn load(path: &Path) -> Self {
        if let Ok(raw) = fs::read_to_string(path) {
            if let Ok(store) = serde_json::from_str(&raw) {
                return store;
            }
        }
        Self::default()
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = fs::create_dir_all(parent);
            }
        }
        // Only persist pinned applications to disk; unpinned apps stay in memory only.
        let persisted = AppSettingsStore {
            apps: self
                .apps
                .iter()
                .filter(|(_, cfg)| cfg.pinned)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            allowed_endpoints: self.allowed_endpoints.clone(),
            include_disconnected: self.include_disconnected,
        };
        if let Ok(json) = serde_json::to_string_pretty(&persisted) {
            let _ = fs::write(path, json);
        }
    }

    pub fn key_for_pid(pid: u32) -> Option<String> {
        if pid == 0 {
            return None;
        }
        resolve_root_exe_path(pid).map(|p| p.to_ascii_lowercase())
    }

    pub fn resolve_key(pid: u32, app_key: Option<&str>) -> Option<String> {
        Self::key_for_pid(pid)
            .or_else(|| app_key.filter(|k| !k.is_empty()).map(|k| k.to_string()))
    }

    pub fn upsert_for_pid(&mut self, pid: u32, path: &Path, f: impl FnOnce(&mut AppPersistentSettings)) {
        self.upsert_by_key_or_pid(pid, None, path, f);
    }

    pub fn upsert_by_key_or_pid(
        &mut self,
        pid: u32,
        app_key: Option<&str>,
        path: &Path,
        f: impl FnOnce(&mut AppPersistentSettings),
    ) {
        let Some(key) = Self::resolve_key(pid, app_key) else {
            return;
        };
        let entry = self.apps.entry(key).or_default();
        f(entry);
        if entry.pinned {
            self.save(path);
        }
    }

    pub fn set_app_pinned(&mut self, pid: u32, app_key: Option<&str>, pinned: bool, path: &Path) {
        let Some(key) = Self::resolve_key(pid, app_key) else {
            return;
        };
        let entry = self.apps.entry(key).or_default();
        entry.pinned = pinned;
        self.save(path);
    }

    pub fn clear_unpinned(&mut self) {
        self.apps.retain(|_, cfg| cfg.pinned);
    }

    pub fn remember_seen_sessions(&mut self, sessions: &[SessionStatus], path: &Path) {
        let mut save_needed = false;
        for s in sessions {
            if s.pid == 0 || s.app_key.is_empty() {
                continue;
            }
            let entry = self.apps.entry(s.app_key.clone()).or_default();
            let mut changed = false;
            if !s.display_name.is_empty()
                && entry.display_name.as_deref() != Some(s.display_name.as_str())
            {
                entry.display_name = Some(s.display_name.clone());
                changed = true;
            }
            if !s.root_exe_name.is_empty()
                && entry.root_exe_name.as_deref() != Some(s.root_exe_name.as_str())
            {
                entry.root_exe_name = Some(s.root_exe_name.clone());
                changed = true;
            }
            if entry.primary_endpoint_id.is_none() && !s.endpoint_id.is_empty() {
                entry.primary_endpoint_id = Some(s.endpoint_id.clone());
                changed = true;
            }
            if entry.volume.is_none() {
                entry.volume = Some(s.volume);
                changed = true;
            }
            if entry.muted.is_none() {
                entry.muted = Some(s.muted);
                changed = true;
            }
            if changed && entry.pinned {
                save_needed = true;
            }
        }
        if save_needed {
            self.save(path);
        }
    }

    /// Applies saved per-app settings to `pid` (if present in `self.apps` and not yet applied).
    pub fn apply_to_pid(
        &self,
        enumerator: &AudioEnumerator,
        pid: u32,
        applied_pids: &mut HashSet<u32>,
        loopbacks: &mut HashMap<(u32, String), ActiveLoopback>,
    ) {
        if pid == 0 || !applied_pids.insert(pid) {
            return;
        }
        let Some(key) = Self::key_for_pid(pid) else {
            return;
        };
        let Some(settings) = self.apps.get(&key) else {
            return;
        };

        if let Some(ref ep_id) = settings.primary_endpoint_id {
            if let Ok(ap) = AppPolicyClient::new() {
                let _ = ap.set_app_endpoint(pid, DataFlow::Render, DeviceRole::Console, ep_id);
                let _ = ap.set_app_endpoint(pid, DataFlow::Render, DeviceRole::Multimedia, ep_id);
            }
        }

        let target_vol = settings
            .primary_endpoint_id
            .as_ref()
            .and_then(|ep_id| settings.output_volumes.get(ep_id).copied())
            .or(settings.volume);

        if let Ok(eps) = enumerator.list_active_endpoints(DataFlow::Render) {
            if target_vol.is_some() || settings.muted.is_some() {
                for ep in &eps {
                    if let Ok(sessions) = ep.sessions() {
                        for s in sessions {
                            if s.pid == pid {
                                if let Some(v) = target_vol {
                                    let _ = s.set_volume(v.clamp(0.0, 1.0));
                                }
                                if let Some(m) = settings.muted {
                                    let _ = s.set_mute(m);
                                }
                            }
                        }
                    }
                }
            }

            for sec in &settings.secondary_outputs {
                if loopbacks.contains_key(&(pid, sec.endpoint_id.clone())) {
                    continue;
                }
                if let Some(ep) = eps.iter().find(|e| e.id == sec.endpoint_id) {
                    let vol = sec.volume.clamp(0.0, 1.0);
                    if let Ok(handle) = LoopbackTee::start(pid, ep, vol) {
                        handle.set_mute(sec.muted);
                        loopbacks.insert(
                            (pid, sec.endpoint_id.clone()),
                            ActiveLoopback {
                                handle,
                                volume: vol,
                                muted: sec.muted,
                            },
                        );
                    }
                }
            }
        }
    }
}
