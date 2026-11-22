mod com;
mod endpoint;
mod enumerator;
mod events;
pub mod icon;
mod instance;
pub mod ipc;
pub mod logging;
mod loopback;
mod policy;
mod preset;
mod session;

pub type Result<T> = windows::core::Result<T>;

pub use com::ComGuard;
pub use endpoint::{AudioEndpoint, EndpointVolumeState};
pub use enumerator::{AudioEnumerator, DataFlow};
pub use events::{AudioEvent, DeviceNotificationGuard, SessionNotificationGuard};
pub use instance::SingleInstanceGuard;
pub use loopback::{LoopbackHandle, LoopbackTee};
pub use policy::{AppPolicyClient, DeviceRole, PolicyClient};
pub use preset::{
    AppLoopbackRule, AppRouteRule, AppVolumeRule, EndpointTarget, Preset, PresetConfig,
    PresetEngine, PresetSnapshot,
};
pub use session::{
    resolve_app_display_name, resolve_root_exe_name, resolve_root_exe_path, AudioSession,
};
