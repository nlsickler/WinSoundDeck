use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use audio_core::{
    AudioEnumerator, AudioEvent, ComGuard, DataFlow, PresetEngine, SessionNotificationGuard,
};

use crate::state::DaemonState;

fn arm_session_watchers(
    enumerator: &AudioEnumerator,
    tx: &mpsc::Sender<AudioEvent>,
) -> Vec<SessionNotificationGuard> {
    let mut guards = Vec::new();
    if let Ok(eps) = enumerator.list_active_endpoints(DataFlow::Render) {
        for ep in eps {
            if let Ok(g) = ep.watch_sessions(tx.clone()) {
                guards.push(g);
            }
        }
    }
    guards
}

pub fn spawn_event_watcher(state: Arc<Mutex<DaemonState>>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let Ok(_com) = ComGuard::new() else { return };
        let Ok(enumerator) = AudioEnumerator::new() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let Ok(_dev_guard) = enumerator.watch_devices(tx.clone()) else {
            return;
        };

        let mut _session_guards = arm_session_watchers(&enumerator, &tx);

        loop {
            if state.lock().map(|g| g.shutdown).unwrap_or(true) {
                break;
            }
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(AudioEvent::SessionCreated {
                    endpoint_id,
                    pid,
                    exe_name,
                    root_exe_name,
                }) => {
                    if let Ok(mut guard) = state.lock() {
                        let DaemonState {
                            ref app_settings,
                            ref mut applied_pids,
                            ref mut loopbacks,
                            ..
                        } = *guard;
                        app_settings.apply_to_pid(&enumerator, pid, applied_pids, loopbacks);
                    }
                    let active = state.lock().ok().and_then(|g| {
                        let id = g.config.active_preset_id.as_ref()?;
                        g.config.presets.iter().find(|p| &p.id == id).cloned()
                    });
                    if let Some(preset) = active {
                        let _ = PresetEngine::new(&enumerator).handle_session_created(
                            &preset,
                            &endpoint_id,
                            pid,
                            &exe_name,
                            &root_exe_name,
                        );
                    }
                }
                Ok(AudioEvent::DefaultDeviceChanged {
                    flow,
                    role,
                    device_id,
                }) => {
                    if let Ok(mut guard) = state.lock() {
                        guard.default_endpoints.insert((flow, role), device_id);
                    }
                }
                Ok(AudioEvent::DeviceStateChanged { .. }) | Ok(AudioEvent::DeviceAdded { .. }) => {
                    _session_guards.clear();
                    _session_guards = arm_session_watchers(&enumerator, &tx);
                    let active = state.lock().ok().and_then(|g| {
                        let id = g.config.active_preset_id.as_ref()?;
                        g.config.presets.iter().find(|p| &p.id == id).cloned()
                    });
                    if let Some(preset) = active {
                        let _ = PresetEngine::new(&enumerator).apply_preset(&preset);
                    }
                }
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    })
}
