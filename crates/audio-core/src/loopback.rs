use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::core::{implement, Interface, HRESULT, HSTRING, PROPVARIANT};
use windows::Win32::Foundation::{CloseHandle, E_POINTER, HANDLE};
use windows::Win32::Media::Audio::{
    ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, IAudioRenderClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
    AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject, INFINITE};
use windows::Win32::System::Variant::VT_BLOB;

use crate::{AudioEndpoint, ComGuard, Result};

const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const LOOPBACK_BUFFER_100NS: i64 = 200_000; // 20 ms

// SAFETY: Matches the C `PROPVARIANT` layout for `VT_BLOB` on 64-bit Windows without invoking
// `PropVariantClear` on stack-allocated `AUDIOCLIENT_ACTIVATION_PARAMS` memory.
#[repr(C)]
struct StackBlobPropVariant {
    vt: u16,
    reserved1: u16,
    reserved2: u16,
    reserved3: u16,
    cb_size: u32,
    p_blob_data: *mut u8,
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct ActivationHandler {
    event: HANDLE,
    tx: mpsc::SyncSender<Result<IAudioClient>>,
}

impl IActivateAudioInterfaceCompletionHandler_Impl for ActivationHandler_Impl {
    fn ActivateCompleted(
        &self,
        activateoperation: Option<&IActivateAudioInterfaceAsyncOperation>,
    ) -> Result<()> {
        let result = (|| {
            let op = activateoperation.ok_or(windows::core::Error::from(E_POINTER))?;
            let mut hr = HRESULT(0);
            let mut iface = None;
            unsafe { op.GetActivateResult(&mut hr, &mut iface)? };
            hr.ok()?;
            let unknown = iface.ok_or(windows::core::Error::from(E_POINTER))?;
            unknown.cast::<IAudioClient>()
        })();
        let _ = self.tx.send(result);
        unsafe {
            let _ = SetEvent(self.event);
        }
        Ok(())
    }
}

struct PumpState {
    stop: AtomicBool,
    muted: AtomicBool,
    gain_bits: AtomicU32,
    frames_pumped: AtomicU64,
}

/// Live handle to a running WASAPI Process Loopback Tee stream.
/// Stops and joins the background audio pump thread automatically when dropped.
pub struct LoopbackHandle {
    state: Arc<PumpState>,
    worker: Option<JoinHandle<()>>,
}

impl LoopbackHandle {
    /// Sets the secondary output perceptual volume (`0.0..=1.0` clamped, mapped via `scalar^2`).
    pub fn set_volume(&self, scalar: f32) {
        let linear = scalar.clamp(0.0, 1.0).powi(2);
        self.state
            .gain_bits
            .store(linear.to_bits(), Ordering::Relaxed);
    }

    /// Sets a direct linear gain multiplier (clamped `0.0..=16.0`), used when compensating
    /// for primary session attenuation (`(secondary / primary)^2`).
    pub fn set_linear_gain(&self, linear: f32) {
        let clamped = linear.clamp(0.0, 16.0);
        self.state
            .gain_bits
            .store(clamped.to_bits(), Ordering::Relaxed);
    }

    /// Mutes or unmutes the secondary loopback output without affecting the primary stream.
    pub fn set_mute(&self, muted: bool) {
        self.state.muted.store(muted, Ordering::Relaxed);
    }

    pub fn frames_pumped(&self) -> u64 {
        self.state.frames_pumped.load(Ordering::Relaxed)
    }

    pub fn stop(mut self) {
        self.stop_inner();
    }

    fn stop_inner(&mut self) {
        self.state.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for LoopbackHandle {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

/// User-mode WASAPI Process Loopback Tee that duplicates a target process tree's audio
/// onto a secondary render endpoint with independent software volume and mute.
pub struct LoopbackTee;

impl LoopbackTee {
    /// Starts capturing `target_pid` (and its child processes) and rendering to `target_endpoint`.
    pub fn start(
        target_pid: u32,
        target_endpoint: &AudioEndpoint,
        initial_scalar: f32,
    ) -> Result<LoopbackHandle> {
        let initial_linear = initial_scalar.clamp(0.0, 1.0).powi(2);
        let state = Arc::new(PumpState {
            stop: AtomicBool::new(false),
            muted: AtomicBool::new(false),
            gain_bits: AtomicU32::new(initial_linear.to_bits()),
            frames_pumped: AtomicU64::new(0),
        });

        let target_endpoint_id = target_endpoint.id.clone();
        let state_bg = Arc::clone(&state);
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<()>>(1);

        let worker = thread::spawn(move || {
            let _com = match ComGuard::new() {
                Ok(c) => c,
                Err(e) => {
                    let _ = init_tx.send(Err(e));
                    return;
                }
            };

            let setup = setup_loopback_clients(target_pid, &target_endpoint_id);
            let (capture_client, capturer, render_client, renderer, render_buf_frames, channels) =
                match setup {
                    Ok(v) => {
                        let _ = init_tx.send(Ok(()));
                        v
                    }
                    Err(e) => {
                        let _ = init_tx.send(Err(e));
                        return;
                    }
                };

            let _ = run_pump_loop(
                &state_bg,
                &capture_client,
                &capturer,
                &render_client,
                &renderer,
                render_buf_frames,
                channels,
            );

            unsafe {
                let _ = capture_client.Stop();
                let _ = render_client.Stop();
            }
        });

        match init_rx.recv() {
            Ok(Ok(())) => Ok(LoopbackHandle {
                state,
                worker: Some(worker),
            }),
            Ok(Err(e)) => {
                let _ = worker.join();
                Err(e)
            }
            Err(_) => {
                let _ = worker.join();
                Err(windows::core::Error::from(E_POINTER))
            }
        }
    }
}

fn activate_process_loopback_client(target_pid: u32) -> Result<IAudioClient> {
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: target_pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };

    let prop = StackBlobPropVariant {
        vt: VT_BLOB.0,
        reserved1: 0,
        reserved2: 0,
        reserved3: 0,
        cb_size: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
        p_blob_data: (&mut params as *mut AUDIOCLIENT_ACTIVATION_PARAMS).cast(),
    };

    let event = unsafe { CreateEventW(None, false, false, None)? };
    let (tx, rx) = mpsc::sync_channel(1);
    let handler: IActivateAudioInterfaceCompletionHandler = ActivationHandler { event, tx }.into();

    // SAFETY: `prop` points to `params` which stays alive on this stack frame until `WaitForSingleObject`
    // completes after `ActivateCompleted` fires.
    let _op = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some((&prop as *const StackBlobPropVariant).cast::<PROPVARIANT>()),
            &handler,
        )?
    };

    unsafe { WaitForSingleObject(event, INFINITE) };
    unsafe {
        let _ = CloseHandle(event);
    }

    rx.recv()
        .map_err(|_| windows::core::Error::from(E_POINTER))?
}

fn float_stereo_48k_format() -> WAVEFORMATEX {
    let channels: u16 = 2;
    let bits: u16 = 32;
    let block_align = channels * (bits / 8);
    let sample_rate: u32 = 48_000;
    WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
        nChannels: channels,
        nSamplesPerSec: sample_rate,
        nAvgBytesPerSec: sample_rate * block_align as u32,
        nBlockAlign: block_align,
        wBitsPerSample: bits,
        cbSize: 0,
    }
}

fn setup_loopback_clients(
    target_pid: u32,
    target_endpoint_id: &str,
) -> Result<(
    IAudioClient,
    IAudioCaptureClient,
    IAudioClient,
    IAudioRenderClient,
    u32,
    usize,
)> {
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    let sec_dev = unsafe { enumerator.GetDevice(&HSTRING::from(target_endpoint_id))? };

    let fmt = float_stereo_48k_format();
    let capture_client = activate_process_loopback_client(target_pid)?;
    unsafe {
        capture_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            LOOPBACK_BUFFER_100NS,
            0,
            &fmt,
            None,
        )?;
    }
    let capturer: IAudioCaptureClient = unsafe { capture_client.GetService()? };

    let render_client: IAudioClient = unsafe { sec_dev.Activate(CLSCTX_ALL, None)? };
    unsafe {
        render_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            LOOPBACK_BUFFER_100NS,
            0,
            &fmt,
            None,
        )?;
    }
    let render_buf_frames = unsafe { render_client.GetBufferSize()? };
    let renderer: IAudioRenderClient = unsafe { render_client.GetService()? };

    unsafe {
        capture_client.Start()?;
        render_client.Start()?;
    }

    Ok((
        capture_client,
        capturer,
        render_client,
        renderer,
        render_buf_frames,
        fmt.nChannels as usize,
    ))
}

fn run_pump_loop(
    state: &PumpState,
    _capture_client: &IAudioClient,
    capturer: &IAudioCaptureClient,
    render_client: &IAudioClient,
    renderer: &IAudioRenderClient,
    render_buf_frames: u32,
    channels: usize,
) -> Result<()> {
    while !state.stop.load(Ordering::Relaxed) {
        let mut packet_len = unsafe { capturer.GetNextPacketSize()? };
        while packet_len > 0 {
            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            let mut num_frames: u32 = 0;
            let mut flags: u32 = 0;
            unsafe {
                capturer.GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None)?;
            }

            if num_frames > 0 {
                let padding = unsafe { render_client.GetCurrentPadding()? };
                let avail = render_buf_frames.saturating_sub(padding);
                let write_frames = num_frames.min(avail);

                if write_frames > 0 {
                    let out_ptr = unsafe { renderer.GetBuffer(write_frames)? };
                    let sample_count = write_frames as usize * channels;
                    let is_silent = (flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)) != 0;
                    let muted = state.muted.load(Ordering::Relaxed);
                    let gain = f32::from_bits(state.gain_bits.load(Ordering::Relaxed));

                    if muted || is_silent || data_ptr.is_null() {
                        // SAFETY: `out_ptr` is valid for `write_frames * channels` `f32` samples.
                        unsafe {
                            std::ptr::write_bytes(
                                out_ptr,
                                0,
                                sample_count * std::mem::size_of::<f32>(),
                            );
                        }
                    } else {
                        // SAFETY: Both `data_ptr` and `out_ptr` are 32-bit float PCM buffers of at least `sample_count`.
                        let in_slice = unsafe {
                            std::slice::from_raw_parts(data_ptr as *const f32, sample_count)
                        };
                        let out_slice = unsafe {
                            std::slice::from_raw_parts_mut(out_ptr as *mut f32, sample_count)
                        };
                        for (dst, &src) in out_slice.iter_mut().zip(in_slice.iter()) {
                            *dst = src * gain;
                        }
                    }
                    unsafe { renderer.ReleaseBuffer(write_frames, 0)? };
                    state
                        .frames_pumped
                        .fetch_add(write_frames as u64, Ordering::Relaxed);
                }
            }

            unsafe { capturer.ReleaseBuffer(num_frames)? };
            packet_len = unsafe { capturer.GetNextPacketSize()? };
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
