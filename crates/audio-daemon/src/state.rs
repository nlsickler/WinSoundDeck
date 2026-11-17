use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use audio_core::{DataFlow, DeviceRole, PresetConfig, PresetSnapshot};

use crate::app_settings::{ActiveLoopback, AppSettingsStore};

pub struct DaemonState {
    pub config: PresetConfig,
    pub config_path: PathBuf,
    pub app_settings: AppSettingsStore,
    pub app_settings_path: PathBuf,
    pub applied_pids: HashSet<u32>,
    pub loopbacks: HashMap<(u32, String), ActiveLoopback>,
    pub default_endpoints: HashMap<(DataFlow, DeviceRole), String>,
    pub pre_preset_snapshot: Option<PresetSnapshot>,
    pub shutdown: bool,
}
