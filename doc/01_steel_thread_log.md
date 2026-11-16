# Steel Thread & Validation Log

This document tracks verified capabilities, technical learnings, and the incremental validation plan for the **Audio Preset & Device Routing Manager** (`Rust + windows-rs + Tauri`).

---

## Agent Interaction & Methodology (Red → Green → Refactor)

Each validation step follows an experimental **Red → Green → Refactor** loop:
1. **Red (Define & Probe):** Define the minimal experiment in `crates/steel-thread` against live Windows Core Audio APIs (no unit-test scaffolding required during hardware spikes).
2. **Green (Verify):** Compile and run the experiment (apply → verify readback → undo/restore) against live hardware/apps and confirm expected behavior.
3. **Refactor (Productionize):** Extract validated primitives into `crates/audio-core` following [02_development_guidelines.md](file:///c:/Projects/audio-mixer/doc/02_development_guidelines.md) (domain modules, narrow `unsafe` guards, RAII lifetimes), re-verify the build and experiment, and log learnings here before moving to the next step.

---

## Environment & Hardware Baseline

* **Toolchain:** `rustc 1.95.0` / `cargo 1.95.0`
* **Workspace:** [Cargo.toml](file:///c:/Projects/audio-mixer/Cargo.toml) (`crates/audio-core`, `crates/steel-thread`)
* **COM Crate:** `windows v0.58.0`
* **Discovered Active Playback Endpoints (2026-11-15):**
  * `Speakers (HyperX Cloud II Wireless)` — `{0.0.0.00000000}.{197a2d59-9879-4b78-b2e6-143a0362a7ac}`
  * `PC Primary (NVIDIA High Definition Audio)` — `{0.0.0.00000000}.{7c422063-8317-42c6-adfe-4608060ab04d}`
  * `Speakers (Steam Streaming Microphone)` — `{0.0.0.00000000}.{9a50bcba-c3dd-4437-a34c-74a0f00025af}`
  * `Realtek Digital Output (Realtek(R) Audio)` — `{0.0.0.00000000}.{ccc308ae-b079-4ae0-b5a5-25c06a735b64}`
  * `Speakers (Steam Streaming Speakers)` — `{0.0.0.00000000}.{eda382f7-9935-4b60-abf9-0447aeaa3c7a}`
* **Discovered Active Capture / Mic Endpoints (2026-11-15):**
  * `Microphone (HyperX Cloud II Wireless)` — `{0.0.1.00000000}.{870e370b-fa57-41cc-a892-d19cb131d18a}`
  * `Microphone (Steam Streaming Microphone)` — `{0.0.1.00000000}.{bd5f3729-4c73-4b6e-811e-43ab7026621a}`

---

## Completed Validations

### Step 1: WASAPI Endpoint Enumeration & Per-App Session Volume (Apply + Undo)
* **Status:** Passed (2026-11-15)
* **Code:** [main.rs](file:///c:/Projects/audio-mixer/crates/steel-thread/src/main.rs)
* **What Was Tested:**
  * Initialized COM (`COINIT_MULTITHREADED`) and bound `IMMDeviceEnumerator`.
  * Enumerated active `eRender` endpoints and matched `"PC Primary"` by partial friendly name.
  * Activated `IAudioSessionManager2`, enumerated active sessions, and resolved PIDs to executable names via `OpenProcess` + `QueryFullProcessImageNameW`.
  * Matched `vivaldi.exe` (`pid=15828`), read initial volume (`1.00`), set `0.30` via `ISimpleAudioVolume::SetMasterVolume`, held 3 seconds, and restored `1.00`.
* **Key Learnings:**
  * **Sandbox vs. Host Execution:** `IMMDeviceEnumerator::EnumAudioEndpoints` succeeds in the restricted terminal sandbox, but `IMMDevice::Activate(IAudioSessionManager2)` fails with `0x80070005` (*Access is denied*). Audio session control must run unsandboxed.
  * **Chromium/Vivaldi Session Identity:** Vivaldi's audio stream appeared directly under `vivaldi.exe` (`pid=15828`) on the active endpoint, so executable name matching worked without needing parent-process tree walking for this test.
  * **Interference Note:** SteelSeries Sonar was shut down prior to testing to avoid external volume/routing overrides.

### Step 2: Master Endpoint Volume/Mute & Capture (Mic) Enumeration
* **Status:** Passed + Refactored (2026-11-15)
* **Code:** [lib.rs](file:///c:/Projects/audio-mixer/crates/audio-core/src/lib.rs), [main.rs](file:///c:/Projects/audio-mixer/crates/steel-thread/src/main.rs)
* **What Was Tested:**
  * Enumerated all 5 active `eRender` endpoints and 2 active `eCapture` endpoints with `IAudioEndpointVolume` scalar volume and mute states.
  * Applied `vol=0.30, muted=true` on `PC Primary (NVIDIA High Definition Audio)`, held 3 seconds, and restored `vol=1.00, muted=false`.
* **Refactor (Red → Green → Refactor):**
  * Extracted reusable Core Audio primitives into `crates/audio-core`: `ComGuard`, `DataFlow`, `EndpointVolumeState`, `AudioEnumerator`, `AudioEndpoint`, and `AudioSession`.
  * Re-verified `cargo build --workspace --offline` and `steel-thread.exe --step 2` pass cleanly after refactoring.

### Step 3: OS Default Device Role Switching (`IPolicyConfig`)
* **Status:** Passed + Refactored (2026-11-15)
* **Code:** [policy.rs](file:///c:/Projects/audio-mixer/crates/audio-core/src/policy.rs), [enumerator.rs](file:///c:/Projects/audio-mixer/crates/audio-core/src/enumerator.rs), [main.rs](file:///c:/Projects/audio-mixer/crates/steel-thread/src/main.rs)
* **What Was Tested:**
  * Read initial OS defaults via `IMMDeviceEnumerator::GetDefaultAudioEndpoint` (`Console` & `Multimedia` on `PC Primary (NVIDIA High Definition Audio)`, `Communications` on `Speakers (Steam Streaming Speakers)`).
  * Switched all 3 roles to `Speakers (HyperX Cloud II Wireless)` via `IPolicyConfig::SetDefaultEndpoint`, verified via readback, held 3 seconds, and restored each role to its original endpoint.
* **Key Learnings:**
  * Undocumented `IPolicyConfig` (`CLSID_PolicyConfigClient` `{870af99c-...}`, `IID_IPolicyConfig` `{f8679f50-...}`) works cleanly on this Windows build via `#[windows_core::interface]`.
* **Refactor (Red → Green → Refactor):**
  * Isolated `IPolicyConfig` inside a private `mod raw` in `crates/audio-core/src/policy.rs` and exposed `DeviceRole` + `PolicyClient::set_default_endpoint`.
  * Added `AudioEnumerator::default_endpoint_id` to `enumerator.rs`.
  * Removed direct `windows` / `windows-core` dependencies from `crates/steel-thread/Cargo.toml` so `steel-thread` depends solely on `audio-core`.

### Step 4: Per-App Output Device Routing (`IAudioPolicyConfigFactory`)
* **Status:** Passed + Refactored (2026-11-15)
* **Code:** [policy.rs](file:///c:/Projects/audio-mixer/crates/audio-core/src/policy.rs), [main.rs](file:///c:/Projects/audio-mixer/crates/steel-thread/src/main.rs)
* **What Was Tested:**
  * Activated `Windows.Media.Internal.AudioPolicyConfig` via `RoGetActivationFactory` (matched `V2` IID `{ab3d4648-e242-459f-b02f-541c70306324}`).
  * Routed active `vivaldi.exe` (`pid=15828`) from `PC Primary (NVIDIA High Definition Audio)` to `Speakers (HyperX Cloud II Wireless)` for `Console` and `Multimedia` roles.
  * Confirmed via `GetPersistedDefaultAudioEndpoint` readback, session probe on `HyperX`, and audible playback switch in the headset for 5 seconds, then restored via `ClearAllPersistedApplicationDefaultEndpoints`.
* **Key Learnings:**
  * **Exact Vtable Layout:** `ab3d4648-e242-459f-b02f-541c70306324` has `3 IUnknown + 3 IInspectable + 19 internal slots` (putting `SetPersistedDefaultAudioEndpoint` at slot 25, `GetPersistedDefaultAudioEndpoint` at slot 26, and `ClearAllPersistedApplicationDefaultEndpoints` at slot 27).
  * **SWD Device Path Format:** `SetPersistedDefaultAudioEndpoint` requires the full SWD device interface path (`\\?\SWD#MMDEVAPI#<endpoint_id>#{e6327cad-dcec-4949-ae8a-991e976a79d2}` for `eRender`), not the bare `{0.0.0...}` endpoint ID.
* **Refactor (Red → Green → Refactor):**
  * Moved `IAudioPolicyConfigFactoryV2` / `V1` into `mod raw` in `crates/audio-core/src/policy.rs` and exposed `AppPolicyClient` (`set_app_endpoint`, `get_app_endpoint`, `clear_all_app_endpoints`).
  * Removed direct `windows` / `windows-core` dependencies from `crates/steel-thread/Cargo.toml`.

### Step 5: Reactive COM Event Listeners (`IMMNotificationClient` & `IAudioSessionNotification`)
* **Status:** Passed + Refactored (2026-11-15)
* **Code:** [events.rs](file:///c:/Projects/audio-mixer/crates/audio-core/src/events.rs), [session.rs](file:///c:/Projects/audio-mixer/crates/audio-core/src/session.rs), [enumerator.rs](file:///c:/Projects/audio-mixer/crates/audio-core/src/enumerator.rs), [endpoint.rs](file:///c:/Projects/audio-mixer/crates/audio-core/src/endpoint.rs)
* **What Was Tested:**
  * Implemented `IMMNotificationClient_Impl` (`DeviceNotificationGuard`) and `IAudioSessionNotification_Impl` (`SessionNotificationGuard`) using `windows-rs`'s `#[implement(...)]` macro.
  * Registered listeners across all 5 active `eRender` endpoints and triggered a `DeviceRole::Communications` switch + restore via `PolicyClient`.
  * Observed real-time `SessionCreated` and `DefaultDeviceChanged` events arriving over `std::sync::mpsc::Sender<AudioEvent>`, followed by clean RAII unregistration on drop.
  * Added `resolve_root_exe_name` in `session.rs` using `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` to walk parent PIDs up to the root application binary.
* **Key Learnings:**
  * `SessionNotificationGuard` must retain the `IAudioSessionManager2` interface and call `GetSessionEnumerator()` once after `RegisterSessionNotification` so WASAPI arms `OnSessionCreated` callbacks.
  * `windows` crate `"implement"` feature is required for `*_Impl` COM server traits.

### Step 6: Multi-Endpoint Preset Engine & Persistence
* **Status:** Passed + Refactored (2026-11-15)
* **Code:** [preset.rs](file:///c:/Projects/audio-mixer/crates/audio-core/src/preset.rs), [main.rs](file:///c:/Projects/audio-mixer/crates/steel-thread/src/main.rs)
* **What Was Tested:**
  * Serialized and deserialized `PresetConfig` JSON via `serde` + `serde_json` (`save_to_file` / `load_from_file`).
  * Applied a multi-endpoint preset (`PC Primary` Render `vol=0.85` + `Microphone (HyperX Cloud II Wireless)` Capture `vol=0.90` + `vivaldi.exe` app rule) via `PresetEngine::apply_preset`.
  * Verified state readback (leaving unlisted `Speakers (HyperX Cloud II Wireless)` untouched at `0.94`), held 3 seconds, and restored original levels via `PresetSnapshot::restore`.
* **Key Learnings:**
  * `PresetEngine::handle_session_created` shares `resolve_rule_for_app` with `apply_preset`, keeping initial and reactive enforcement identical.

### Step 7: Simultaneous Multi-Output Audio (WASAPI Process Loopback Tee)
* **Status:** Passed + Refactored (2026-11-15)
* **Code:** [loopback.rs](file:///c:/Projects/audio-mixer/crates/audio-core/src/loopback.rs), [main.rs](file:///c:/Projects/audio-mixer/crates/steel-thread/src/main.rs)
* **What Was Tested:**
  * Activated `VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK` via `ActivateAudioInterfaceAsync` (`PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE`) targeting `vivaldi.exe` (`pid=15828`) on `PC Primary`.
  * Rendered captured `48 kHz` stereo `f32` PCM frames to `Speakers (HyperX Cloud II Wireless)` with independent software gain, stepping live from `0.32` perceptual (~`0.10` linear) for 3s to `0.70` perceptual (~`0.49` linear) for 3s (`288,000` frames pumped), without affecting `PC Primary`.
* **Key Learnings:**
  * **Perceptual Taper:** Linear amplitude scaling sounds much louder than Windows Volume Mixer scalars (`0.30` linear ≈ `-10.5 dB`), so `LoopbackHandle::set_volume` applies `scalar.powi(2)` to match Windows slider feel.
  * **Stack `PROPVARIANT` Safety:** Passing a `#[repr(C)]` `StackBlobPropVariant` for `VT_BLOB` avoids `windows_core::PROPVARIANT::drop` calling `PropVariantClear` on stack-allocated `AUDIOCLIENT_ACTIVATION_PARAMS`.
  * **COM Thread Affinity:** `LoopbackTee::start` passes `endpoint.id` (`String`) to the worker thread and resolves `IMMDevice` on the worker's own MTA COM thread.
* **Refactor (Red → Green → Refactor):**
  * Extracted `LoopbackTee` and `LoopbackHandle` (with atomic `set_volume`, `set_mute`, `frames_pumped`, and RAII `Drop` thread join) into `crates/audio-core/src/loopback.rs`.
  * Removed temporary `windows` / `windows-core` dependencies from `crates/steel-thread/Cargo.toml`.

### Step 8: Single-App Route Reset (`AppPolicyClient`)
* **Status:** Passed + Refactored (2026-11-15)
* **Code:** [policy.rs](file:///c:/Projects/audio-mixer/crates/audio-core/src/policy.rs), [preset.rs](file:///c:/Projects/audio-mixer/crates/audio-core/src/preset.rs), [main.rs](file:///c:/Projects/audio-mixer/crates/steel-thread/src/main.rs)
* **What Was Tested:**
  * Opened a silent `IAudioClient` stream in `steel-thread` (`pid_a=35296`) and kept `vivaldi.exe` (`pid_b=15828`) playing uninterrupted on `PC Primary`.
  * Routed `pid_a` to `Speakers (HyperX Cloud II Wireless)` and `pid_b` to `PC Primary`, then called `reset_app_endpoint` on `pid_a` only.
  * Confirmed `pid_a` reset to `""` while `pid_b` retained its `PC Primary` SWD path intact.
* **Key Learnings:**
  * `SetPersistedDefaultAudioEndpoint` requires the target PID to have an active WASAPI audio session (`IAudioClient` initialized & started); calling it on a non-audio process returns `0x80070057` (`E_INVALIDARG`).
* **Refactor (Red → Green → Refactor):**
  * Added `AppPolicyClient::reset_app_endpoint` and `AppPolicyClient::set_app_endpoint_swd` in `policy.rs`.
  * Updated `PresetSnapshot` in `preset.rs` to capture and restore each routed app's prior SWD path individually instead of calling `clear_all_app_endpoints()`.

### Step 9: Background Daemon, Named Pipe IPC & Hot-Plug Re-Enforcement
* **Status:** Passed + Refactored (2026-11-15)
* **Code:** [ipc.rs](file:///c:/Projects/audio-mixer/crates/audio-core/src/ipc.rs), [main.rs](file:///c:/Projects/audio-mixer/crates/audio-daemon/src/main.rs), [main.rs](file:///c:/Projects/audio-mixer/crates/steel-thread/src/main.rs)
* **What Was Tested:**
  * Spawned `audio-daemon.exe` headless on `\\.\pipe\audio-mixer-daemon` with reactive `DeviceNotificationGuard` + `SessionNotificationGuard` hot-plug/session watcher threads.
  * Exercised `IpcRequest::Ping -> Pong`, `GetStatus` (7 endpoints, 6 active sessions), `ListPresets` (0 presets), and `Shutdown -> Ok` with clean exit code `0`.
  * Measured debug `audio-daemon.exe` working set at `12.39 MB` (<15 MB target).
* **Key Learnings:**
  * Single-instance `serve_named_pipe` creates a brief 0-instance gap between `drop(file)` and the next `CreateNamedPipeW`; `open_pipe_with_retry` in `send_ipc_request` retries briefly on `ErrorKind::NotFound` / `ERROR_PIPE_BUSY` (`231`) so back-to-back IPC calls never race.
* **Refactor (Red → Green → Refactor):**
  * Extracted `open_pipe_with_retry` in `crates/audio-core/src/ipc.rs` for resilient client-side pipe connection.

---

## Upcoming Validation Plan

### Step 10: Tauri System Tray & Mixer UI
* **Goal:** Build the lightweight system tray & active mixer UI client on top of the Named Pipe IPC protocol.
