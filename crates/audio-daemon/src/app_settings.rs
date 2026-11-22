use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use audio_core::ipc::{EndpointStatus, SessionStatus};
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnownEndpoint {
    pub friendly_name: String,
    pub flow: DataFlow,
    #[serde(default)]
    pub is_bluetooth: bool,
    #[serde(default)]
    pub last_seen: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettingsStore {
    #[serde(default)]
    pub apps: HashMap<String, AppPersistentSettings>,
    #[serde(default)]
    pub allowed_endpoints: Option<HashSet<String>>,
    #[serde(default)]
    pub known_endpoints: HashMap<String, KnownEndpoint>,
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
            known_endpoints: self.known_endpoints.clone(),
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

    pub fn remember_known_endpoints(&mut self, endpoints: &[EndpointStatus], path: &Path) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut changed = false;
        for ep in endpoints {
            if !ep.is_connected || ep.id.is_empty() || ep.friendly_name.is_empty() || ep.friendly_name == "<unknown>" {
                continue;
            }
            let prev = self.known_endpoints.get(&ep.id);
            let metadata_changed = prev
                .map(|p| {
                    p.friendly_name != ep.friendly_name
                        || p.flow != ep.flow
                        || p.is_bluetooth != ep.is_bluetooth
                })
                .unwrap_or(true);
            let stale_ts = prev
                .and_then(|p| p.last_seen)
                .map(|ts| now.saturating_sub(ts) >= 60)
                .unwrap_or(true);
            if metadata_changed || stale_ts {
                self.known_endpoints.insert(
                    ep.id.clone(),
                    KnownEndpoint {
                        friendly_name: ep.friendly_name.clone(),
                        flow: ep.flow,
                        is_bluetooth: ep.is_bluetooth,
                        last_seen: Some(now),
                    },
                );
                changed = true;
            }
        }
        if changed {
            self.save(path);
        }
    }

    pub fn forget_endpoint(&mut self, endpoint_id: &str, path: &Path) {
        let removed_known = self.known_endpoints.remove(endpoint_id).is_some();
        let removed_allowed = self
            .allowed_endpoints
            .as_mut()
            .is_some_and(|set| set.remove(endpoint_id));
        if removed_known || removed_allowed {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_settings_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("winsounddeck_test_{name}_{nanos}"))
            .join("app_settings.json")
    }

    #[test]
    fn save_persists_only_pinned_apps_and_preserves_visibility_flags() {
        let path = temp_settings_path("pinned_only");
        let mut store = AppSettingsStore {
            include_disconnected: true,
            allowed_endpoints: Some(["ep-1".to_string()].into_iter().collect()),
            ..Default::default()
        };

        store.apps.insert(
            "c:\\pinned.exe".to_string(),
            AppPersistentSettings {
                pinned: true,
                display_name: Some("Pinned".to_string()),
                volume: Some(0.8),
                ..Default::default()
            },
        );
        store.apps.insert(
            "c:\\ephemeral.exe".to_string(),
            AppPersistentSettings {
                pinned: false,
                display_name: Some("Ephemeral".to_string()),
                volume: Some(0.3),
                ..Default::default()
            },
        );

        store.save(&path);
        let reloaded = AppSettingsStore::load(&path);
        assert!(reloaded.include_disconnected);
        assert!(reloaded
            .allowed_endpoints
            .as_ref()
            .is_some_and(|s| s.contains("ep-1")));
        assert!(reloaded.apps.contains_key("c:\\pinned.exe"));
        assert!(!reloaded.apps.contains_key("c:\\ephemeral.exe"));

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn clear_unpinned_and_offline_upsert_work_by_app_key() {
        let path = temp_settings_path("clear_unpinned");
        let mut store = AppSettingsStore::default();

        store.upsert_by_key_or_pid(0, Some("c:\\offline.exe"), &path, |s| {
            s.volume = Some(0.5);
        });
        store.set_app_pinned(0, Some("c:\\pinned.exe"), true, &path);

        assert_eq!(store.apps.len(), 2);
        store.clear_unpinned();
        assert_eq!(store.apps.len(), 1);
        assert!(store.apps.contains_key("c:\\pinned.exe"));
        assert!(!store.apps.contains_key("c:\\offline.exe"));

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn remember_seen_sessions_ignores_pid_zero_and_populates_metadata() {
        let path = temp_settings_path("remember");
        let mut store = AppSettingsStore::default();
        let sessions = vec![
            SessionStatus {
                endpoint_id: "ep1".to_string(),
                pid: 0,
                app_key: "".to_string(),
                exe_name: "System Sounds".to_string(),
                root_exe_name: "System Sounds".to_string(),
                display_name: "System Sounds".to_string(),
                is_active: false,
                is_pinned: false,
                volume: 1.0,
                muted: false,
                saved_volumes: HashMap::new(),
            },
            SessionStatus {
                endpoint_id: "ep1".to_string(),
                pid: 1234,
                app_key: "c:\\vivaldi.exe".to_string(),
                exe_name: "vivaldi.exe".to_string(),
                root_exe_name: "vivaldi.exe".to_string(),
                display_name: "Vivaldi".to_string(),
                is_active: true,
                is_pinned: false,
                volume: 0.65,
                muted: false,
                saved_volumes: HashMap::new(),
            },
        ];

        store.remember_seen_sessions(&sessions, &path);
        assert_eq!(store.apps.len(), 1);
        let cfg = &store.apps["c:\\vivaldi.exe"];
        assert_eq!(cfg.display_name.as_deref(), Some("Vivaldi"));
        assert_eq!(cfg.primary_endpoint_id.as_deref(), Some("ep1"));
        assert_eq!(cfg.volume, Some(0.65));
    }

    #[test]
    fn remember_known_endpoints_caches_connected_devices_for_offline_reuse() {
        let path = temp_settings_path("known_endpoints");
        let mut store = AppSettingsStore::default();
        let eps = vec![
            EndpointStatus {
                id: "ep-dock".to_string(),
                friendly_name: "Dock Speakers".to_string(),
                flow: DataFlow::Render,
                volume: 0.7,
                muted: false,
                is_default_multimedia: true,
                is_default_communications: true,
                is_allowed: true,
                is_connected: true,
                is_bluetooth: false,
                last_seen: None,
            },
            EndpointStatus {
                id: "ep-stale".to_string(),
                friendly_name: "Disconnected Ghost".to_string(),
                flow: DataFlow::Render,
                volume: 1.0,
                muted: false,
                is_default_multimedia: false,
                is_default_communications: false,
                is_allowed: false,
                is_connected: false,
                is_bluetooth: false,
                last_seen: None,
            },
        ];

        store.remember_known_endpoints(&eps, &path);
        let reloaded = AppSettingsStore::load(&path);
        assert_eq!(reloaded.known_endpoints.len(), 1);
        assert_eq!(reloaded.known_endpoints["ep-dock"].friendly_name, "Dock Speakers");
        assert!(reloaded.known_endpoints["ep-dock"].last_seen.is_some());
        assert!(!reloaded.known_endpoints.contains_key("ep-stale"));

        let mut store2 = reloaded;
        store2.allowed_endpoints = Some(["ep-dock".to_string()].into_iter().collect());
        store2.forget_endpoint("ep-dock", &path);
        let reloaded2 = AppSettingsStore::load(&path);
        assert!(!reloaded2.known_endpoints.contains_key("ep-dock"));
        assert!(!reloaded2
            .allowed_endpoints
            .as_ref()
            .unwrap()
            .contains("ep-dock"));

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
