mod com;
mod endpoint;
mod enumerator;
mod events;
mod policy;
mod preset;
mod session;

pub type Result<T> = windows::core::Result<T>;

pub use com::ComGuard;
pub use endpoint::{AudioEndpoint, EndpointVolumeState};
pub use enumerator::{AudioEnumerator, DataFlow};
pub use events::{AudioEvent, DeviceNotificationGuard, SessionNotificationGuard};
pub use policy::{AppPolicyClient, DeviceRole, PolicyClient};
pub use preset::{
    AppRouteRule, AppVolumeRule, EndpointTarget, Preset, PresetConfig, PresetEngine,
    PresetSnapshot,
};
pub use session::{resolve_root_exe_name, AudioSession};
