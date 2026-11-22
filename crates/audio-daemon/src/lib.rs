mod app_settings;
mod debounce;
mod handlers;
mod state;
mod status;
mod watcher;

use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use app_settings::AppSettingsStore;
use audio_core::ipc::{serve_named_pipe, DEFAULT_PIPE_NAME};
use audio_core::{
    log_error, log_info, AudioEnumerator, ComGuard, DataFlow, DeviceRole, PresetConfig,
    PresetEngine, SingleInstanceGuard,
};
use handlers::handle_request;
use state::DaemonState;
use watcher::spawn_event_watcher;

fn default_config_path() -> PathBuf {
    if let Ok(appdata) = env::var("APPDATA") {
        PathBuf::from(appdata).join("winsounddeck").join("presets.json")
    } else {
        PathBuf::from("presets.json")
    }
}

fn parse_args() -> (PathBuf, String) {
    let mut config_path = default_config_path();
    let mut pipe_name = DEFAULT_PIPE_NAME.to_string();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                if let Some(p) = args.next() {
                    config_path = PathBuf::from(p);
                }
            }
            "--pipe" => {
                if let Some(p) = args.next() {
                    pipe_name = p;
                }
            }
            _ => {}
        }
    }
    (config_path, pipe_name)
}

fn load_or_default_config(path: &PathBuf) -> PresetConfig {
    if path.exists() {
        if let Ok(cfg) = PresetConfig::load_from_file(path) {
            return cfg;
        }
    }
    PresetConfig {
        version: "1.0.0".to_string(),
        active_preset_id: None,
        presets: Vec::new(),
    }
}

pub fn run_daemon() {
    audio_core::logging::init_logging("audio-daemon");
    let _instance = match SingleInstanceGuard::acquire("Local\\WinSoundDeckDaemon") {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            log_info!("audio-daemon is already running; exiting");
            return;
        }
        Err(err) => {
            log_error!("Failed to acquire daemon single-instance mutex: {err}");
            return;
        }
    };
    let (config_path, pipe_name) = parse_args();
    log_info!("Starting audio-daemon on pipe '{pipe_name}'");

    let _com = ComGuard::new().expect("COM init failed");
    let enumerator = AudioEnumerator::new().expect("AudioEnumerator init failed");
    let config = load_or_default_config(&config_path);

    if let Some(ref active_id) = config.active_preset_id {
        if let Some(preset) = config.presets.iter().find(|p| &p.id == active_id) {
            let _ = PresetEngine::new(&enumerator).apply_preset(preset);
        }
    }

    let app_settings_path = AppSettingsStore::path_beside_config(&config_path);
    let app_settings = AppSettingsStore::load(&app_settings_path);
    let mut applied_pids = HashSet::new();
    let mut loopbacks = HashMap::new();

    if let Ok(eps) = enumerator.list_active_endpoints(DataFlow::Render) {
        for ep in eps {
            if let Ok(sessions) = ep.sessions() {
                for s in sessions {
                    app_settings.apply_to_pid(
                        &enumerator,
                        s.pid,
                        &mut applied_pids,
                        &mut loopbacks,
                    );
                }
            }
        }
    }

    let mut default_endpoints = HashMap::new();
    for flow in [DataFlow::Render, DataFlow::Capture] {
        for role in DeviceRole::ALL {
            if let Ok(id) = enumerator.default_endpoint_id(flow, role) {
                default_endpoints.insert((flow, role), id);
            }
        }
    }

    let state = Arc::new(Mutex::new(DaemonState {
        config,
        config_path,
        app_settings,
        app_settings_path,
        applied_pids,
        loopbacks,
        default_endpoints,
        pre_preset_snapshot: None,
        shutdown: false,
    }));
    let watcher = spawn_event_watcher(Arc::clone(&state));

    if let Err(err) = serve_named_pipe(&pipe_name, |req| handle_request(&enumerator, &state, req)) {
        log_error!("Named pipe server error: {err}");
    }

    if let Ok(mut guard) = state.lock() {
        guard.shutdown = true;
    }
    let _ = watcher.join();
}
