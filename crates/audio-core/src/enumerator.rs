use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eCapture, eRender, EDataFlow, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ};

use crate::{AudioEndpoint, DeviceRole, Result};

/// Direction of an audio endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        let collection = unsafe {
            self.inner
                .EnumAudioEndpoints(flow.into(), DEVICE_STATE_ACTIVE)?
        };
        let count = unsafe { collection.GetCount()? };
        let mut endpoints = Vec::with_capacity(count as usize);

        for i in 0..count {
            let device = unsafe { collection.Item(i)? };
            let id = read_device_id(&device).unwrap_or_else(|_| "<unknown>".to_string());
            let friendly_name =
                read_device_friendly_name(&device).unwrap_or_else(|_| "<unknown>".to_string());
            endpoints.push(AudioEndpoint::new(id, friendly_name, flow, device));
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
