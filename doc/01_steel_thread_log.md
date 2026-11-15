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

---

## Upcoming Validation Plan

### Step 6: Multi-Endpoint Preset Engine & Persistence
* **Goal:** Extract validated COM primitives into a reusable `crates/audio-core` library and implement the multi-endpoint preset state machine.
* **Test Plan:**
  * Define JSON preset schema supporting multiple render/capture endpoints, per-app volumes, default roles, and per-app output routing overrides.
  * Validate apply/switch/restore determinism and conflict rules (manual slider overrides do not mutate saved presets).

### Step 7: Background Daemon, Named Pipe IPC & Tauri Tray UI
* **Goal:** Decouple the headless engine from the visual controller.
* **Test Plan:**
  * Run the core engine as a lightweight background daemon (<15 MB RAM target) exposing control/status over Windows Named Pipes.
  * Build the Tauri system tray & mixer UI client on top of the Named Pipe protocol.
