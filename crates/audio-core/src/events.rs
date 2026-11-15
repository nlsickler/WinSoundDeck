use std::sync::mpsc::Sender;

use windows::core::{implement, Interface, PCWSTR};
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eMultimedia, EDataFlow, ERole, IAudioSessionControl,
    IAudioSessionControl2, IAudioSessionManager2, IAudioSessionNotification,
    IAudioSessionNotification_Impl, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, DEVICE_STATE,
};
use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;

use crate::session::{resolve_process_exe_name, resolve_root_exe_name};
use crate::{DataFlow, DeviceRole, Result};

/// Reactive Core Audio events emitted by COM notification callbacks.
#[derive(Debug, Clone)]
pub enum AudioEvent {
    DefaultDeviceChanged {
        flow: DataFlow,
        role: DeviceRole,
        device_id: String,
    },
    DeviceStateChanged {
        device_id: String,
        new_state: u32,
    },
    DeviceAdded {
        device_id: String,
    },
    DeviceRemoved {
        device_id: String,
    },
    SessionCreated {
        endpoint_id: String,
        pid: u32,
        exe_name: String,
        root_exe_name: String,
    },
}

fn pcwstr_to_string(p: PCWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: WASAPI notification callbacks pass valid null-terminated UTF-16 strings for the call duration.
    unsafe { p.to_string() }.unwrap_or_default()
}

fn map_data_flow(flow: EDataFlow) -> DataFlow {
    if flow == eCapture {
        DataFlow::Capture
    } else {
        DataFlow::Render
    }
}

fn map_role(role: ERole) -> DeviceRole {
    if role == eCommunications {
        DeviceRole::Communications
    } else if role == eMultimedia {
        DeviceRole::Multimedia
    } else {
        DeviceRole::Console
    }
}

#[implement(IMMNotificationClient)]
struct DeviceNotificationClient {
    tx: Sender<AudioEvent>,
}

impl IMMNotificationClient_Impl for DeviceNotificationClient_Impl {
    fn OnDeviceStateChanged(
        &self,
        pwstrdeviceid: &PCWSTR,
        dwnewstate: DEVICE_STATE,
    ) -> windows::core::Result<()> {
        let _ = self.tx.send(AudioEvent::DeviceStateChanged {
            device_id: pcwstr_to_string(*pwstrdeviceid),
            new_state: dwnewstate.0,
        });
        Ok(())
    }

    fn OnDeviceAdded(&self, pwstrdeviceid: &PCWSTR) -> windows::core::Result<()> {
        let _ = self.tx.send(AudioEvent::DeviceAdded {
            device_id: pcwstr_to_string(*pwstrdeviceid),
        });
        Ok(())
    }

    fn OnDeviceRemoved(&self, pwstrdeviceid: &PCWSTR) -> windows::core::Result<()> {
        let _ = self.tx.send(AudioEvent::DeviceRemoved {
            device_id: pcwstr_to_string(*pwstrdeviceid),
        });
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        pwstrdefaultdeviceid: &PCWSTR,
    ) -> windows::core::Result<()> {
        let _ = self.tx.send(AudioEvent::DefaultDeviceChanged {
            flow: map_data_flow(flow),
            role: map_role(role),
            device_id: pcwstr_to_string(*pwstrdefaultdeviceid),
        });
        Ok(())
    }

    fn OnPropertyValueChanged(
        &self,
        _pwstrdeviceid: &PCWSTR,
        _key: &PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

/// RAII guard that keeps an `IMMNotificationClient` registered until dropped.
pub struct DeviceNotificationGuard {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
}

impl DeviceNotificationGuard {
    pub(crate) fn register(
        enumerator: IMMDeviceEnumerator,
        tx: Sender<AudioEvent>,
    ) -> Result<Self> {
        let client: IMMNotificationClient = DeviceNotificationClient { tx }.into();
        unsafe { enumerator.RegisterEndpointNotificationCallback(&client)? };
        Ok(Self { enumerator, client })
    }
}

impl Drop for DeviceNotificationGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .enumerator
                .UnregisterEndpointNotificationCallback(&self.client);
        }
    }
}

#[implement(IAudioSessionNotification)]
struct SessionNotificationClient {
    endpoint_id: String,
    tx: Sender<AudioEvent>,
}

impl IAudioSessionNotification_Impl for SessionNotificationClient_Impl {
    fn OnSessionCreated(
        &self,
        newsession: Option<&IAudioSessionControl>,
    ) -> windows::core::Result<()> {
        let Some(ctrl) = newsession else {
            return Ok(());
        };
        let Ok(ctrl2) = ctrl.cast::<IAudioSessionControl2>() else {
            return Ok(());
        };
        let pid = unsafe { ctrl2.GetProcessId().unwrap_or(0) };
        let exe_name = resolve_process_exe_name(pid).unwrap_or_else(|| "<unknown>".to_string());
        let root_exe_name = resolve_root_exe_name(pid).unwrap_or_else(|| exe_name.clone());

        let _ = self.tx.send(AudioEvent::SessionCreated {
            endpoint_id: self.endpoint_id.clone(),
            pid,
            exe_name,
            root_exe_name,
        });
        Ok(())
    }
}

/// RAII guard that keeps an `IAudioSessionNotification` registered until dropped.
/// Retains the `IAudioSessionManager2` instance so WASAPI continues firing callbacks.
pub struct SessionNotificationGuard {
    manager: IAudioSessionManager2,
    client: IAudioSessionNotification,
}

impl SessionNotificationGuard {
    pub(crate) fn register(
        endpoint_id: String,
        manager: IAudioSessionManager2,
        tx: Sender<AudioEvent>,
    ) -> Result<Self> {
        let client: IAudioSessionNotification = SessionNotificationClient { endpoint_id, tx }.into();
        unsafe { manager.RegisterSessionNotification(&client)? };
        // WASAPI requires enumerating existing sessions once to arm `OnSessionCreated` callbacks.
        let _ = unsafe { manager.GetSessionEnumerator()? };
        Ok(Self { manager, client })
    }
}

impl Drop for SessionNotificationGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = self.manager.UnregisterSessionNotification(&self.client);
        }
    }
}
