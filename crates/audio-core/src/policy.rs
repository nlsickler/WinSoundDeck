use windows::core::{GUID, HSTRING, PCWSTR};
use windows::Win32::Media::Audio::{eCommunications, eConsole, eMultimedia, ERole};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

use crate::Result;

/// Windows Core Audio device role for default endpoint assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceRole {
    Console,
    Multimedia,
    Communications,
}

impl DeviceRole {
    pub const ALL: [DeviceRole; 3] = [
        DeviceRole::Console,
        DeviceRole::Multimedia,
        DeviceRole::Communications,
    ];
}

impl From<DeviceRole> for ERole {
    fn from(role: DeviceRole) -> Self {
        match role {
            DeviceRole::Console => eConsole,
            DeviceRole::Multimedia => eMultimedia,
            DeviceRole::Communications => eCommunications,
        }
    }
}

const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

mod raw {
    #![allow(non_snake_case)]

    use std::ffi::c_void;

    use windows::core::{HRESULT, PCWSTR};
    use windows::Win32::Media::Audio::ERole;

    // SAFETY: Undocumented Windows 10/11 `IPolicyConfig` vtable layout (`f8679f50-850a-41cf-9c72-430f290290c8`).
    // Slot order after `IUnknown` matches the native Windows Audio Policy Config client.
    #[windows_core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub(super) unsafe trait IPolicyConfig: windows_core::IUnknown {
        fn GetMixFormat(&self, device_id: PCWSTR, format: *mut *mut c_void) -> HRESULT;
        fn GetDeviceFormat(
            &self,
            device_id: PCWSTR,
            default: i32,
            format: *mut *mut c_void,
        ) -> HRESULT;
        fn ResetDeviceFormat(&self, device_id: PCWSTR) -> HRESULT;
        fn SetDeviceFormat(
            &self,
            device_id: PCWSTR,
            endpoint_format: *const c_void,
            mix_format: *const c_void,
        ) -> HRESULT;
        fn GetProcessingPeriod(
            &self,
            device_id: PCWSTR,
            default: i32,
            default_period: *mut i64,
            min_period: *mut i64,
        ) -> HRESULT;
        fn SetProcessingPeriod(&self, device_id: PCWSTR, period: *const i64) -> HRESULT;
        fn GetShareMode(&self, device_id: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn SetShareMode(&self, device_id: PCWSTR, mode: *const c_void) -> HRESULT;
        fn GetPropertyValue(
            &self,
            device_id: PCWSTR,
            fx_store: i32,
            key: *const c_void,
            pv: *mut c_void,
        ) -> HRESULT;
        fn SetPropertyValue(
            &self,
            device_id: PCWSTR,
            fx_store: i32,
            key: *const c_void,
            pv: *const c_void,
        ) -> HRESULT;
        fn SetDefaultEndpoint(&self, device_id: PCWSTR, role: ERole) -> HRESULT;
        fn SetEndpointVisibility(&self, device_id: PCWSTR, visible: i32) -> HRESULT;
    }

    pub(super) unsafe fn set_default_endpoint(
        policy: &IPolicyConfig,
        device_id: PCWSTR,
        role: ERole,
    ) -> HRESULT {
        policy.SetDefaultEndpoint(device_id, role)
    }
}

use raw::{set_default_endpoint, IPolicyConfig};

/// Client for switching Windows default audio endpoints via `IPolicyConfig`.
pub struct PolicyClient {
    inner: IPolicyConfig,
}

impl PolicyClient {
    pub fn new() -> Result<Self> {
        let inner: IPolicyConfig =
            unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)? };
        Ok(Self { inner })
    }

    /// Sets the OS default audio endpoint for `role` to the given endpoint `device_id`.
    pub fn set_default_endpoint(&self, device_id: &str, role: DeviceRole) -> Result<()> {
        let wide_id = HSTRING::from(device_id);
        // SAFETY: `wide_id` stays alive for the duration of the synchronous COM call.
        unsafe { set_default_endpoint(&self.inner, PCWSTR(wide_id.as_ptr()), role.into()).ok() }
    }
}
