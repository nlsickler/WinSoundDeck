use std::sync::mpsc::Sender;

use windows::core::{Interface, GUID};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    AudioSessionStateActive, AudioSessionStateExpired, IAudioSessionControl2, IAudioSessionManager2,
    IMMDevice, ISimpleAudioVolume,
};
use windows::Win32::System::Com::CLSCTX_ALL;

use crate::session::resolve_process_exe_name;
use crate::{AudioEvent, AudioSession, DataFlow, Result, SessionNotificationGuard};

/// Master volume and mute state of a hardware audio endpoint.
#[derive(Debug, Clone, Copy)]
pub struct EndpointVolumeState {
    pub volume: f32,
    pub muted: bool,
}

/// A Windows Core Audio endpoint (`IMMDevice`).
pub struct AudioEndpoint {
    pub id: String,
    pub friendly_name: String,
    pub flow: DataFlow,
    device: IMMDevice,
}

impl AudioEndpoint {
    pub(crate) fn new(id: String, friendly_name: String, flow: DataFlow, device: IMMDevice) -> Self {
        Self {
            id,
            friendly_name,
            flow,
            device,
        }
    }

    fn endpoint_volume(&self) -> Result<IAudioEndpointVolume> {
        unsafe { self.device.Activate(CLSCTX_ALL, None) }
    }

    /// Reads the endpoint's master scalar volume (`0.0..=1.0`) and mute state.
    pub fn master_volume(&self) -> Result<EndpointVolumeState> {
        let ep_vol = self.endpoint_volume()?;
        let volume = unsafe { ep_vol.GetMasterVolumeLevelScalar()? };
        let muted = unsafe { ep_vol.GetMute()? }.as_bool();
        Ok(EndpointVolumeState { volume, muted })
    }

    /// Sets the endpoint's master scalar volume (clamped to `0.0..=1.0`).
    pub fn set_master_volume(&self, volume: f32) -> Result<()> {
        let ep_vol = self.endpoint_volume()?;
        unsafe { ep_vol.SetMasterVolumeLevelScalar(volume.clamp(0.0, 1.0), &GUID::zeroed()) }
    }

    /// Sets the endpoint's master mute flag.
    pub fn set_master_mute(&self, muted: bool) -> Result<()> {
        let ep_vol = self.endpoint_volume()?;
        unsafe { ep_vol.SetMute(muted, &GUID::zeroed()) }
    }

    /// Enumerates all active audio sessions on this endpoint.
    pub fn sessions(&self) -> Result<Vec<AudioSession>> {
        let session_mgr: IAudioSessionManager2 = unsafe { self.device.Activate(CLSCTX_ALL, None)? };
        let session_enum = unsafe { session_mgr.GetSessionEnumerator()? };
        let count = unsafe { session_enum.GetCount()? };
        let mut sessions = Vec::with_capacity(count as usize);

        for i in 0..count {
            let ctrl = unsafe { session_enum.GetSession(i)? };
            let state = unsafe { ctrl.GetState()? };
            if state == AudioSessionStateExpired {
                continue;
            }
            let ctrl2: IAudioSessionControl2 = ctrl.cast()?;
            let pid = unsafe { ctrl2.GetProcessId()? };
            let exe_name = resolve_process_exe_name(pid).unwrap_or_else(|| "<unknown>".to_string());
            let volume_ctl: ISimpleAudioVolume = ctrl.cast()?;

            sessions.push(AudioSession::new(
                pid,
                exe_name,
                state == AudioSessionStateActive,
                volume_ctl,
            ));
        }
        Ok(sessions)
    }

    /// Registers an `IAudioSessionNotification` on this endpoint that forwards new sessions to `tx`.
    pub fn watch_sessions(&self, tx: Sender<AudioEvent>) -> Result<SessionNotificationGuard> {
        let session_mgr: IAudioSessionManager2 = unsafe { self.device.Activate(CLSCTX_ALL, None)? };
        SessionNotificationGuard::register(self.id.clone(), session_mgr, tx)
    }
}
