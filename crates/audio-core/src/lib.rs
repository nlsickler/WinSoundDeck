mod com;
mod endpoint;
mod enumerator;
mod policy;
mod session;

pub type Result<T> = windows::core::Result<T>;

pub use com::ComGuard;
pub use endpoint::{AudioEndpoint, EndpointVolumeState};
pub use enumerator::{AudioEnumerator, DataFlow};
pub use policy::{DeviceRole, PolicyClient};
pub use session::AudioSession;
