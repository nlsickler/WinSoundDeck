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

---

## Upcoming Validation Plan

### Step 4: Per-App Output Device Routing (`IAudioPolicyConfigFactory`)
* **Goal:** Validate moving a running application (e.g., `vivaldi.exe`) between `PC Primary` and `Speakers (HyperX Cloud II Wireless)` without restarting the app or using virtual audio cables.
* **Test Plan:**
  * Bind `IAudioPolicyConfigFactory` (WinRT/COM activation) and invoke `SetPersistedDefaultAudioEndpoint` for `vivaldi.exe`'s PID/app token.
  * Verify audio shifts to the HyperX headset and back to `PC Primary`.
  * Document any edge cases (e.g., Chromium audio service PID vs. root browser PID, or whether the app needs an active stream to reflect the move immediately).
* **Risk Retired:** Core requirement for meeting-headset vs. out-of-meeting-headset routing. Also clarifies whether simultaneous multi-output duplication (e.g., Spotify to two devices at once) is feasible natively or requires a separate mechanism.

### Step 5: Reactive COM Event Listeners (`IMMNotificationClient` & `IAudioSessionNotification`)
* **Goal:** Replace one-shot polling with zero-overhead event callbacks.
* **Test Plan:**
  * Implement `IMMNotificationClient` to log device plug/unplug, state changes, default device changes, and external hardware volume/mute changes.
  * Implement `IAudioSessionNotification` to intercept a newly started audio stream (e.g., launching or unpausing Vivaldi after the test starts) and apply volume rules immediately.
  * Add `CreateToolhelp32Snapshot` parent-process resolution for multi-process/Electron apps where the audio child process needs grouping under the root binary.
* **Risk Retired:** Validates COM callback threading, lifetime management, and ephemeral/delayed session handling.

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
