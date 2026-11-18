use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eCapture, eRender, EDataFlow, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE,
    DEVICE_STATE_ACTIVE, DEVICE_STATE_DISABLED, DEVICE_STATE_UNPLUGGED,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ};

use crate::{AudioEndpoint, AudioEvent, DeviceNotificationGuard, DeviceRole, Result};

/// Direction of an audio endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataFlow {
    Render,
    Capture,
}

impl From<DataFlow> for EDataFlow {
    fn from(flow: DataFlow) -> Self {
        match flow {
            DataFlow::Render => eRender,
            DataFlow::Capture => eCapture,
        }
    }
}

/// Entry point for enumerating Windows Core Audio endpoints.
pub struct AudioEnumerator {
    inner: IMMDeviceEnumerator,
}

impl AudioEnumerator {
    pub fn new() -> Result<Self> {
        let inner: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        Ok(Self { inner })
    }

    /// Lists all active endpoints for the given data flow.
    pub fn list_active_endpoints(&self, flow: DataFlow) -> Result<Vec<AudioEndpoint>> {
        self.list_endpoints(flow, false)
    }

    /// Lists endpoints for `flow`, optionally including unplugged/disconnected devices.
    pub fn list_endpoints(
        &self,
        flow: DataFlow,
        include_disconnected: bool,
    ) -> Result<Vec<AudioEndpoint>> {
        let mask = endpoint_state_mask(include_disconnected);
        let collection = unsafe { self.inner.EnumAudioEndpoints(flow.into(), mask)? };
        let count = unsafe { collection.GetCount()? };
        let mut endpoints = Vec::with_capacity(count as usize);

        for i in 0..count {
            let device = unsafe { collection.Item(i)? };
            let state = unsafe { device.GetState().unwrap_or(DEVICE_STATE_ACTIVE) };
            let is_connected = state == DEVICE_STATE_ACTIVE;
            let id = read_device_id(&device).unwrap_or_else(|_| "<unknown>".to_string());
            let friendly_name = match read_device_friendly_name(&device) {
                Ok(name) => name,
                Err(e) if is_orphaned_device_error(&e) => continue,
                Err(_) => "<unknown>".to_string(),
            };
            endpoints.push(AudioEndpoint::new(id, friendly_name, flow, is_connected, device));
        }
        Ok(endpoints)
    }

    /// Finds the first active endpoint whose friendly name contains `pattern` (case-insensitive).
    pub fn find_endpoint_by_name(
        &self,
        flow: DataFlow,
        pattern: &str,
    ) -> Result<Option<AudioEndpoint>> {
        let needle = pattern.to_lowercase();
        let endpoints = self.list_active_endpoints(flow)?;
        Ok(endpoints
            .into_iter()
            .find(|ep| ep.friendly_name.to_lowercase().contains(&needle)))
    }

    /// Returns the endpoint ID of the current OS default device for `flow` and `role`.
    pub fn default_endpoint_id(&self, flow: DataFlow, role: DeviceRole) -> Result<String> {
        let device = unsafe { self.inner.GetDefaultAudioEndpoint(flow.into(), role.into())? };
        read_device_id(&device)
    }

    /// Registers an `IMMNotificationClient` that forwards device events to `tx`.
    pub fn watch_devices(&self, tx: Sender<AudioEvent>) -> Result<DeviceNotificationGuard> {
        DeviceNotificationGuard::register(self.inner.clone(), tx)
    }
}

fn read_device_id(device: &IMMDevice) -> Result<String> {
    let raw = unsafe { device.GetId()? };
    let id = if raw.is_null() {
        String::new()
    } else {
        // SAFETY: `GetId` allocates a valid null-terminated UTF-16 string via COM task allocator.
        unsafe { raw.to_string() }.unwrap_or_default()
    };
    // SAFETY: `raw` was returned by `IMMDevice::GetId` and must be freed with `CoTaskMemFree`.
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    Ok(id)
}

fn read_device_friendly_name(device: &IMMDevice) -> Result<String> {
    let store = unsafe { device.OpenPropertyStore(STGM_READ)? };
    let prop = unsafe { store.GetValue(&PKEY_Device_FriendlyName)? };
    Ok(prop.to_string())
}

fn is_orphaned_device_error(err: &windows::core::Error) -> bool {
    const ERROR_NO_SUCH_DEVINST: i32 = 0xE000_020Bu32 as i32;
    const ERROR_NOT_FOUND_HR: i32 = 0x8007_0490u32 as i32;
    matches!(err.code().0, ERROR_NO_SUCH_DEVINST | ERROR_NOT_FOUND_HR)
}

pub(crate) fn endpoint_state_mask(include_disconnected: bool) -> DEVICE_STATE {
    if include_disconnected {
        DEVICE_STATE(
            DEVICE_STATE_ACTIVE.0 | DEVICE_STATE_DISABLED.0 | DEVICE_STATE_UNPLUGGED.0,
        )
    } else {
        DEVICE_STATE_ACTIVE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Media::Audio::DEVICE_STATE_NOTPRESENT;

    #[test]
    fn endpoint_state_mask_excludes_notpresent_and_includes_disabled_unplugged() {
        assert_eq!(endpoint_state_mask(false), DEVICE_STATE_ACTIVE);
        let disconnected_mask = endpoint_state_mask(true);
        assert_eq!(disconnected_mask.0, 0xB);
        assert_eq!(disconnected_mask.0 & DEVICE_STATE_NOTPRESENT.0, 0);
        assert_ne!(disconnected_mask.0 & DEVICE_STATE_DISABLED.0, 0);
        assert_ne!(disconnected_mask.0 & DEVICE_STATE_UNPLUGGED.0, 0);
    }
}
