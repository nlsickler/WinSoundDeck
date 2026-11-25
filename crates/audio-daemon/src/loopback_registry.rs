use std::collections::{HashMap, HashSet};

use audio_core::{AudioEndpoint, LoopbackHandle};

use crate::app_settings::SecondaryOutputSetting;

/// Minimum effective primary volume while an audible connected loopback is active,
/// preventing Windows WASAPI loopback capture from stalling on silence mid-drag.
pub const MIN_PRIMARY_FOR_LOOPBACK: f32 = 0.05;

pub struct ActiveLoopback {
    pub handle: LoopbackHandle,
    pub volume: f32,
    pub muted: bool,
}

/// Owns all active secondary `LoopbackTee` streams and centralizes gain/ratio
/// math, mute synchronization, and disconnected-endpoint pruning.
#[derive(Default)]
pub struct LoopbackRegistry {
    entries: HashMap<(u32, String), ActiveLoopback>,
}

impl LoopbackRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn contains(&self, pid: u32, endpoint_id: &str) -> bool {
        self.entries.contains_key(&(pid, endpoint_id.to_string()))
    }

    pub fn remove(&mut self, pid: u32, endpoint_id: &str) -> Option<ActiveLoopback> {
        self.entries.remove(&(pid, endpoint_id.to_string()))
    }

    /// Applies initial ratio gain and mute state to a newly started `LoopbackHandle`
    /// and registers it for `(pid, endpoint_id)`.
    pub fn insert_started(
        &mut self,
        pid: u32,
        endpoint_id: String,
        handle: LoopbackHandle,
        volume: f32,
        muted: bool,
        primary_vol: f32,
    ) {
        let vol = volume.clamp(0.0, 1.0);
        let is_muted = muted || vol <= 0.0;
        let ratio = compute_loopback_ratio(vol, primary_vol);
        handle.set_linear_gain(ratio.powi(2));
        handle.set_mute(is_muted);
        self.entries.insert(
            (pid, endpoint_id),
            ActiveLoopback {
                handle,
                volume: vol,
                muted: is_muted,
            },
        );
    }

    pub fn set_volume(&mut self, pid: u32, endpoint_id: &str, volume: f32) -> bool {
        if let Some(lb) = self.entries.get_mut(&(pid, endpoint_id.to_string())) {
            let vol = volume.clamp(0.0, 1.0);
            lb.volume = vol;
            lb.muted = vol <= 0.0;
            true
        } else {
            false
        }
    }

    pub fn set_mute_for_pid(&mut self, pid: u32, muted: bool) {
        for ((p, _), lb) in self.entries.iter_mut() {
            if *p == pid {
                let lb_muted = if muted { true } else { lb.volume <= 0.0 };
                lb.muted = lb_muted;
                lb.handle.set_mute(lb_muted);
            }
        }
    }

    pub fn apply_ratios_for_pid(&mut self, pid: u32, primary_eff: f32) {
        for ((p, _), lb) in self.entries.iter_mut() {
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

    pub fn prune_disconnected(&mut self, connected_endpoints: &[AudioEndpoint]) {
        self.entries
            .retain(|(_, ep_id), _| connected_endpoints.iter().any(|e| e.id == *ep_id));
    }

    pub fn has_audible_for_pid(
        &self,
        pid: u32,
        cur_primary: &str,
        connected_endpoints: &HashSet<&str>,
    ) -> bool {
        has_audible_connected_loopback(
            self.entries
                .iter()
                .map(|((p, ep_id), lb)| (*p, ep_id.as_str(), lb.volume, lb.muted)),
            pid,
            cur_primary,
            connected_endpoints,
        )
    }

    pub fn connected_secondaries_for_pid(
        &self,
        pid: u32,
        cur_primary: &str,
        connected_endpoints: &HashSet<&str>,
    ) -> Vec<(String, f32)> {
        connected_secondary_volumes(
            self.entries
                .iter()
                .map(|((p, ep_id), lb)| (*p, ep_id.as_str(), lb.volume, lb.muted)),
            pid,
            cur_primary,
            connected_endpoints,
        )
    }

    pub fn secondary_settings_for_pid(&self, pid: u32) -> Vec<SecondaryOutputSetting> {
        self.entries
            .iter()
            .filter(|((p, _), _)| *p == pid)
            .map(|((_, ep_id), lb)| SecondaryOutputSetting {
                endpoint_id: ep_id.clone(),
                volume: lb.volume,
                muted: lb.muted,
            })
            .collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&(u32, String), &ActiveLoopback)> {
        self.entries.iter()
    }
}

pub fn compute_loopback_ratio(target_vol: f32, primary_vol: f32) -> f32 {
    (target_vol / primary_vol.max(MIN_PRIMARY_FOR_LOOPBACK)).clamp(0.0, 4.0)
}

pub fn effective_primary_volume(primary_vol: f32, has_audible_loopback: bool) -> f32 {
    if has_audible_loopback && primary_vol < MIN_PRIMARY_FOR_LOOPBACK {
        MIN_PRIMARY_FOR_LOOPBACK
    } else {
        primary_vol
    }
}

pub fn has_audible_connected_loopback<'a>(
    loopbacks: impl IntoIterator<Item = (u32, &'a str, f32, bool)>,
    pid: u32,
    cur_primary: &str,
    connected_endpoints: &HashSet<&str>,
) -> bool {
    loopbacks.into_iter().any(|(p, ep_id, vol, muted)| {
        p == pid
            && ep_id != cur_primary
            && vol > 0.0
            && !muted
            && connected_endpoints.contains(ep_id)
    })
}

pub fn connected_secondary_volumes<'a>(
    loopbacks: impl IntoIterator<Item = (u32, &'a str, f32, bool)>,
    pid: u32,
    cur_primary: &str,
    connected_endpoints: &HashSet<&str>,
) -> Vec<(String, f32)> {
    loopbacks
        .into_iter()
        .filter(|(p, ep_id, _, _)| {
            *p == pid && *ep_id != cur_primary && connected_endpoints.contains(ep_id)
        })
        .map(|(_, ep_id, vol, _)| (ep_id.to_string(), vol))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnected_loopbacks_do_not_trigger_primary_floor() {
        let connected: HashSet<&str> = ["ep-primary"].into_iter().collect();
        let loopbacks = [
            (42u32, "ep-disconnected", 0.6f32, false),
            (99u32, "ep-primary", 0.5f32, false),
        ];

        assert!(!has_audible_connected_loopback(
            loopbacks.iter().copied(),
            42,
            "ep-primary",
            &connected,
        ));
        assert_eq!(effective_primary_volume(0.0, false), 0.0);
    }

    #[test]
    fn connected_audible_loopback_enforces_five_percent_floor() {
        let connected: HashSet<&str> = ["ep-primary", "ep-secondary"].into_iter().collect();
        let loopbacks = [
            (42u32, "ep-secondary", 0.5f32, false),
            (42u32, "ep-muted", 0.8f32, true),
            (42u32, "ep-zero", 0.0f32, false),
        ];

        assert!(has_audible_connected_loopback(
            loopbacks.iter().copied(),
            42,
            "ep-primary",
            &connected,
        ));
        assert_eq!(effective_primary_volume(0.0, true), 0.05);
        assert_eq!(effective_primary_volume(0.25, true), 0.25);
    }

    #[test]
    fn connected_secondary_volumes_excludes_disconnected_and_primary() {
        let connected: HashSet<&str> = ["ep-primary", "ep-secondary"].into_iter().collect();
        let loopbacks = [
            (42u32, "ep-primary", 0.9f32, false),
            (42u32, "ep-disconnected", 0.8f32, false),
            (42u32, "ep-secondary", 0.4f32, false),
            (77u32, "ep-secondary", 0.7f32, false),
        ];

        let secs =
            connected_secondary_volumes(loopbacks.iter().copied(), 42, "ep-primary", &connected);
        assert_eq!(secs, vec![("ep-secondary".to_string(), 0.4)]);
    }

    #[test]
    fn compute_loopback_ratio_clamps_and_avoids_div_by_zero() {
        assert_eq!(compute_loopback_ratio(0.5, 0.0), 4.0);
        assert_eq!(compute_loopback_ratio(0.5, 0.5), 1.0);
        assert_eq!(compute_loopback_ratio(0.0, 0.5), 0.0);
    }
}
