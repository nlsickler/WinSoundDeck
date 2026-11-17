use serde::{Deserialize, Serialize};
use windows::core::{GUID, HSTRING, PCWSTR};
use windows::Win32::Media::Audio::{eCommunications, eConsole, eMultimedia, ERole};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::WinRT::RoGetActivationFactory;

use crate::{DataFlow, Result};

/// Windows Core Audio device role for default endpoint assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

    use windows::core::{HRESULT, HSTRING, PCWSTR};
    use windows::Win32::Media::Audio::{EDataFlow, ERole};

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

    // SAFETY: Undocumented `Windows.Media.Internal.AudioPolicyConfig` (Win10 21H1+ / Win11).
    // Layout: 3 IUnknown + 3 IInspectable + 19 internal slots -> Set(25), Get(26), ClearAll(27).
    #[windows_core::interface("ab3d4648-e242-459f-b02f-541c70306324")]
    pub(super) unsafe trait IAudioPolicyConfigFactoryV2: windows_core::IUnknown {
        fn _inspectable0(&self) -> HRESULT;
        fn _inspectable1(&self) -> HRESULT;
        fn _inspectable2(&self) -> HRESULT;
        fn _pad0(&self) -> HRESULT;
        fn _pad1(&self) -> HRESULT;
        fn _pad2(&self) -> HRESULT;
        fn _pad3(&self) -> HRESULT;
        fn _pad4(&self) -> HRESULT;
        fn _pad5(&self) -> HRESULT;
        fn _pad6(&self) -> HRESULT;
        fn _pad7(&self) -> HRESULT;
        fn _pad8(&self) -> HRESULT;
        fn _pad9(&self) -> HRESULT;
        fn _pad10(&self) -> HRESULT;
        fn _pad11(&self) -> HRESULT;
        fn _pad12(&self) -> HRESULT;
        fn _pad13(&self) -> HRESULT;
        fn _pad14(&self) -> HRESULT;
        fn _pad15(&self) -> HRESULT;
        fn _pad16(&self) -> HRESULT;
        fn _pad17(&self) -> HRESULT;
        fn _pad18(&self) -> HRESULT;
        fn SetPersistedDefaultAudioEndpoint(
            &self,
            process_id: u32,
            flow: EDataFlow,
            role: ERole,
            device_id: *mut c_void,
        ) -> HRESULT;
        fn GetPersistedDefaultAudioEndpoint(
            &self,
            process_id: u32,
            flow: EDataFlow,
            role: ERole,
            device_id: *mut HSTRING,
        ) -> HRESULT;
        fn ClearAllPersistedApplicationDefaultEndpoints(&self) -> HRESULT;
    }

    // SAFETY: Undocumented `Windows.Media.Internal.AudioPolicyConfig` (Win10 1803..20H2).
    // Layout: 3 IUnknown + 3 IInspectable + 19 internal slots -> Set(25), Get(26), ClearAll(27).
    #[windows_core::interface("2a59116d-6c4f-45e0-a74f-707e3fef9258")]
    pub(super) unsafe trait IAudioPolicyConfigFactoryV1: windows_core::IUnknown {
        fn _inspectable0(&self) -> HRESULT;
        fn _inspectable1(&self) -> HRESULT;
        fn _inspectable2(&self) -> HRESULT;
        fn _pad0(&self) -> HRESULT;
        fn _pad1(&self) -> HRESULT;
        fn _pad2(&self) -> HRESULT;
        fn _pad3(&self) -> HRESULT;
        fn _pad4(&self) -> HRESULT;
        fn _pad5(&self) -> HRESULT;
        fn _pad6(&self) -> HRESULT;
        fn _pad7(&self) -> HRESULT;
        fn _pad8(&self) -> HRESULT;
        fn _pad9(&self) -> HRESULT;
        fn _pad10(&self) -> HRESULT;
        fn _pad11(&self) -> HRESULT;
        fn _pad12(&self) -> HRESULT;
        fn _pad13(&self) -> HRESULT;
        fn _pad14(&self) -> HRESULT;
        fn _pad15(&self) -> HRESULT;
        fn _pad16(&self) -> HRESULT;
        fn _pad17(&self) -> HRESULT;
        fn _pad18(&self) -> HRESULT;
        fn SetPersistedDefaultAudioEndpoint(
            &self,
            process_id: u32,
            flow: EDataFlow,
            role: ERole,
            device_id: *mut c_void,
        ) -> HRESULT;
        fn GetPersistedDefaultAudioEndpoint(
            &self,
            process_id: u32,
            flow: EDataFlow,
            role: ERole,
            device_id: *mut HSTRING,
        ) -> HRESULT;
        fn ClearAllPersistedApplicationDefaultEndpoints(&self) -> HRESULT;
    }

    pub(super) unsafe fn set_default_endpoint(
        policy: &IPolicyConfig,
        device_id: PCWSTR,
        role: ERole,
    ) -> HRESULT {
        policy.SetDefaultEndpoint(device_id, role)
    }

    pub(super) unsafe fn set_persisted_v2(
        f: &IAudioPolicyConfigFactoryV2,
        pid: u32,
        flow: EDataFlow,
        role: ERole,
        device_id: *mut c_void,
    ) -> HRESULT {
        f.SetPersistedDefaultAudioEndpoint(pid, flow, role, device_id)
    }

    pub(super) unsafe fn get_persisted_v2(
        f: &IAudioPolicyConfigFactoryV2,
        pid: u32,
        flow: EDataFlow,
        role: ERole,
        out: *mut HSTRING,
    ) -> HRESULT {
        f.GetPersistedDefaultAudioEndpoint(pid, flow, role, out)
    }

    pub(super) unsafe fn clear_persisted_v2(f: &IAudioPolicyConfigFactoryV2) -> HRESULT {
        f.ClearAllPersistedApplicationDefaultEndpoints()
    }

    pub(super) unsafe fn set_persisted_v1(
        f: &IAudioPolicyConfigFactoryV1,
        pid: u32,
        flow: EDataFlow,
        role: ERole,
        device_id: *mut c_void,
    ) -> HRESULT {
        f.SetPersistedDefaultAudioEndpoint(pid, flow, role, device_id)
    }

    pub(super) unsafe fn get_persisted_v1(
        f: &IAudioPolicyConfigFactoryV1,
        pid: u32,
        flow: EDataFlow,
        role: ERole,
        out: *mut HSTRING,
    ) -> HRESULT {
        f.GetPersistedDefaultAudioEndpoint(pid, flow, role, out)
    }

    pub(super) unsafe fn clear_persisted_v1(f: &IAudioPolicyConfigFactoryV1) -> HRESULT {
        f.ClearAllPersistedApplicationDefaultEndpoints()
    }
}

use raw::{
    clear_persisted_v1, clear_persisted_v2, get_persisted_v1, get_persisted_v2,
    set_default_endpoint, set_persisted_v1, set_persisted_v2, IAudioPolicyConfigFactoryV1,
    IAudioPolicyConfigFactoryV2, IPolicyConfig,
};

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

const DEVINTERFACE_AUDIO_RENDER_GUID: &str = "{e6327cad-dcec-4949-ae8a-991e976a79d2}";
const DEVINTERFACE_AUDIO_CAPTURE_GUID: &str = "{2eef81be-33fa-4800-9670-1cd474972c3f}";

enum AppPolicyInner {
    V2(IAudioPolicyConfigFactoryV2),
    V1(IAudioPolicyConfigFactoryV1),
}

/// Client for per-application audio endpoint routing via `Windows.Media.Internal.AudioPolicyConfig`.
pub struct AppPolicyClient {
    inner: AppPolicyInner,
}

impl AppPolicyClient {
    pub fn new() -> Result<Self> {
        let class_id = HSTRING::from("Windows.Media.Internal.AudioPolicyConfig");
        if let Ok(v2) = unsafe { RoGetActivationFactory::<IAudioPolicyConfigFactoryV2>(&class_id) } {
            return Ok(Self {
                inner: AppPolicyInner::V2(v2),
            });
        }
        let v1 = unsafe { RoGetActivationFactory::<IAudioPolicyConfigFactoryV1>(&class_id)? };
        Ok(Self {
            inner: AppPolicyInner::V1(v1),
        })
    }

    /// Formats a Core Audio endpoint ID into the SWD device interface path expected by `AudioPolicyConfig`.
    pub fn format_swd_device_id(endpoint_id: &str, flow: DataFlow) -> String {
        let iface_guid = match flow {
            DataFlow::Render => DEVINTERFACE_AUDIO_RENDER_GUID,
            DataFlow::Capture => DEVINTERFACE_AUDIO_CAPTURE_GUID,
        };
        format!(r"\\?\SWD#MMDEVAPI#{endpoint_id}#{iface_guid}")
    }

    /// Routes `pid` for `flow` and `role` to `endpoint_id` (an MMDevice ID from `AudioEndpoint::id`).
    pub fn set_app_endpoint(
        &self,
        pid: u32,
        flow: DataFlow,
        role: DeviceRole,
        endpoint_id: &str,
    ) -> Result<()> {
        let swd_id = Self::format_swd_device_id(endpoint_id, flow);
        self.set_app_endpoint_swd(pid, flow, role, &swd_id)
    }

    /// Routes `pid` for `flow` and `role` using a pre-formatted SWD device interface path
    /// (or resets to OS default if `swd_id` is empty).
    pub fn set_app_endpoint_swd(
        &self,
        pid: u32,
        flow: DataFlow,
        role: DeviceRole,
        swd_id: &str,
    ) -> Result<()> {
        if swd_id.is_empty() {
            return self.reset_app_endpoint(pid, flow, role);
        }
        let h = HSTRING::from(swd_id);
        // SAFETY: `HSTRING` is `#[repr(transparent)]` over a WinRT `HSTRING` handle and remains alive
        // for the duration of the synchronous COM call.
        let raw_ptr = unsafe { std::mem::transmute_copy::<HSTRING, *mut std::ffi::c_void>(&h) };
        let res = match &self.inner {
            AppPolicyInner::V2(f) => {
                unsafe { set_persisted_v2(f, pid, flow.into(), role.into(), raw_ptr).ok() }
            }
            AppPolicyInner::V1(f) => {
                unsafe { set_persisted_v1(f, pid, flow.into(), role.into(), raw_ptr).ok() }
            }
        };
        drop(h);
        res
    }

    /// Resets a single app's persisted endpoint override back to the OS default.
    pub fn reset_app_endpoint(&self, pid: u32, flow: DataFlow, role: DeviceRole) -> Result<()> {
        let null_hstring: *mut std::ffi::c_void = std::ptr::null_mut();
        // SAFETY: Passing a null `HSTRING` handle instructs `AudioPolicyConfig` to clear this PID's override.
        match &self.inner {
            AppPolicyInner::V2(f) => {
                unsafe { set_persisted_v2(f, pid, flow.into(), role.into(), null_hstring).ok() }
            }
            AppPolicyInner::V1(f) => {
                unsafe { set_persisted_v1(f, pid, flow.into(), role.into(), null_hstring).ok() }
            }
        }
    }

    /// Reads the persisted SWD endpoint path for `pid`, `flow`, and `role` (empty if using OS default).
    pub fn get_app_endpoint(&self, pid: u32, flow: DataFlow, role: DeviceRole) -> Result<String> {
        let mut out = HSTRING::new();
        match &self.inner {
            AppPolicyInner::V2(f) => {
                unsafe { get_persisted_v2(f, pid, flow.into(), role.into(), &mut out).ok()? };
            }
            AppPolicyInner::V1(f) => {
                unsafe { get_persisted_v1(f, pid, flow.into(), role.into(), &mut out).ok()? };
            }
        }
        Ok(out.to_string())
    }

    /// Clears all persisted per-application audio endpoint overrides.
    pub fn clear_all_app_endpoints(&self) -> Result<()> {
        match &self.inner {
            AppPolicyInner::V2(f) => unsafe { clear_persisted_v2(f).ok() },
            AppPolicyInner::V1(f) => unsafe { clear_persisted_v1(f).ok() },
        }
    }
}
