# WinSoundDeck — High-Level Architecture & Specification

---

## 1. Executive Summary & Problem Statement

Modern Windows desktop environments lack a lightweight native solution for orchestrating dynamic audio routing, per-application multi-output volume scaling, and hot-swappable hardware presets. Existing solutions fall short for power-user workflows due to key architectural friction:

* **Flaws of Socket-Based Virtual Cable Loopbacks:** Tools relying on virtual audio cables or local TCP/gRPC loopback sockets introduce buffer latency, audio artifacts, and driver instability across device sleep/wake transitions.
* **Limitations of Consumer UI Tools:** Utilities like EarTrumpet expose per-app session volumes but lack global profile switches, OS default role orchestration, multi-output teeing, and hot-plug preset enforcement.
* **Overhead of Commercial Mixers:** Suites like Voicemeeter impose kernel-driver overhead and complex routing abstractions when the goal is deterministic, policy-based audio routing.

**WinSoundDeck** runs entirely in user mode directly against the **Windows Core Audio APIs (WASAPI)** and COM/WinRT policy interfaces—requiring zero kernel-mode drivers or network loopbacks. For key architectural trade-offs, see [doc/04_design_decisions.md](04_design_decisions.md).

### Goals

* **Zero-Driver User-Mode Architecture:** Interface directly with Windows Core Audio (`IMMDeviceEnumerator`, `IAudioSessionManager2`, `ISimpleAudioVolume`, `IAudioEndpointVolume`, `IAudioMeterInformation`, and `IPolicyConfig` / `IAudioPolicyConfigFactory`) without kernel drivers or socket-based virtual cables.
* **Decoupled Daemon & Tray UI:** Run a lightweight headless background daemon (`audio-daemon`) that reacts to hardware hot-plug and session events (`IMMNotificationClient`, `IAudioSessionNotification`) over a local Windows Named Pipe, paired with a Tauri desktop/tray UI (`audio-ui`) that remembers its window placement across monitors.
* **Deterministic Presets:** Save and restore hardware endpoint volumes/mutes, Windows Default (`Multimedia` / `Console`) and Communications defaults, per-app primary routes, and secondary loopback outputs.
* **Per-App Multi-Output Routing & Pinning:** Route apps to a primary output plus optional secondary loopback outputs (`LoopbackTee`), group multi-PID apps cleanly, and pin core apps (`📌`) so their settings persist across restarts while unpinned apps stay in memory only.
* **Device Visibility & Offline Preset Setup:** Filter out unused devices via an allowlist, cache previously seen devices (`known_endpoints`) for offline preset configuration without probing orphaned `NOTPRESENT` PnP entries, and clean up stale devices.
* **Live Feedback:** Display subtle in-track live peak meters behind volume sliders while the window is visible.

### Non-Goals

* **No Kernel-Mode Virtual Audio Drivers:** No custom WDM/APO drivers or virtual sound cards.
* **No Audio DSP / EQ / Effects Pipeline:** WinSoundDeck routes and scales streams; it does not apply EQ, compression, noise suppression, or VST plugins.
* **No Per-Tab Browser Mixing:** Modern browsers (e.g., Vivaldi, Chrome, Firefox) mix tabs internally before handing a single audio stream to Windows WASAPI; WinSoundDeck operates at the OS process/PID level exposed by Windows.
* **No Cross-Platform Support:** Built specifically for Windows 10/11 Core Audio APIs.
* **No Duplicate Device Aliasing Layer:** Relies on Windows' built-in device renaming (and WinSoundDeck's **I/O Visibility** allowlist) rather than maintaining separate UI-only device aliases.

---

## 2. Architecture Overview

### Platform Boundary & API Strategy

```text
┌─────────────────────────────────────────────────────────────────┐
│                    User Interface (audio-ui)                    │
│       Tauri + System Tray (HTML/CSS/JS, non-blocking STA)       │
└────────────────────────────────▲────────────────────────────────┘
                                 │ Inter-Process Communication
                                 │ (Windows Named Pipe JSON)
┌────────────────────────────────▼────────────────────────────────┐
│                  Background Daemon (audio-daemon)               │
│  ┌────────────────────────┐         ┌────────────────────────┐  │
│  │ IMMNotificationClient  │         │IAudioSessionNotification│ │
│  │ + 300ms Debouncer      │         │(Session Spawn Intercept)│ │
│  └───────────┬────────────┘         └───────────┬────────────┘  │
│              │                                  │               │
│              ▼                                  ▼               │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │           Core Audio API Engine (audio-core)              │  │
│  │   IMMDeviceEnumerator (0xB) │ IAudioSessionManager2       │  │
│  │   ISimpleAudioVolume        │ IAudioEndpointVolume        │  │
│  │   IAudioMeterInformation    │ IPolicyConfig / AppPolicy   │  │
│  │   LoopbackTee (VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK)     │  │
│  └───────────────────────────────────────────────────────────┘  │
└────────────────────────────────▲────────────────────────────────┘
                                 │ Native COM / WASAPI
┌────────────────────────────────▼────────────────────────────────┐
│                     Windows OS Kernel Audio                     │
└─────────────────────────────────────────────────────────────────┘
```

### Workspace Crates

| Crate | Responsibility |
| :--- | :--- |
| **`audio-core`** | Narrow `unsafe` COM/WASAPI wrappers (`enumerator`, `endpoint`, `session`, `policy`, `loopback`, `events`, `preset`, `ipc`, `logging`). |
| **`audio-daemon`** | Headless background service hosting `\\.\pipe\winsounddeck-daemon`, `watcher` (hot-plug & session events), `handlers/{mod,apps,presets}`, `status`, and `app_settings.json` persistence. |
| **`audio-ui`** | Tauri v2 desktop + system tray UI (`Applications`, `System`, `Device Visibility`), multi-monitor `window_state.json` persistence, and on-demand UAC `pnputil` device removal. |

### Primary Windows Interfaces

* `IMMDeviceEnumerator`: Enumerates render/capture endpoints using state mask `0xB` (`DEVICE_STATE_ACTIVE | DEVICE_STATE_DISABLED | DEVICE_STATE_UNPLUGGED`), skipping orphaned `DEVICE_STATE_NOTPRESENT` (`0x4`) registry entries and guarding against `0xE000020B` / `ERROR_NOT_FOUND`.
* `IAudioSessionManager2` & `ISimpleAudioVolume`: Enumerates and controls per-process audio streams.
* `IAudioEndpointVolume`: Controls hardware endpoint volume and mute states (skipped for disconnected endpoints).
* `IAudioMeterInformation`: Reads real-time `0.0..=1.0` peak levels for endpoints and active sessions via lightweight `GetPeaks` IPC polling (`75ms` when visible).
* `IPolicyConfig` & `IAudioPolicyConfigFactory`: Sets OS default endpoints (`Multimedia`, `Console`, `Communications`) and per-app primary output routes.
* `ActivateAudioInterfaceAsync` (`VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK`): Implements `LoopbackTee` to mirror an application's audio to secondary output endpoints with independent volume scaling.

---

## 3. System Components

### Device State, Visibility & Hot-Plug Watcher

* **Reactive Hot-Plug Debouncing:** `IMMNotificationClient` events (`DeviceStateChanged`, `DeviceAdded`) are coalesced through a `300ms` `Debouncer` in `watcher.rs` so USB-C dock or headset bursts trigger a single session-watcher re-arm and preset re-apply once settled.
* **Device Visibility Allowlist & Offline Cache:** `app_settings.json` stores `allowed_endpoints` and `known_endpoints` (`{ friendly_name, flow, is_bluetooth, last_seen }`), keeping previously seen docks/displays available for preset setup even when `NOTPRESENT`, without polling orphaned WASAPI entries.
* **Bluetooth & Ghost Cleanup:** `PKEY_Device_EnumeratorName` identifies paired Bluetooth stacks (`BTHENUM`, `BTHHFENUM`, `BTHLE*`) to link directly to `ms-settings:bluetooth`, while non-Bluetooth offline ghosts can be forgotten from WinSoundDeck or uninstalled via elevated `pnputil /remove-device`.

### Session Manager & Application Pinning

* **Root-Process Matching:** Uses `CreateToolhelp32Snapshot` (`resolve_root_exe_path` / `resolve_root_exe_name`) so multi-process apps map consistently by lowercase root executable path (`app_key`).
* **Pinning (`📌`) vs. Ephemeral State:** Pinned apps persist their primary route, secondary loopbacks, per-output volumes, and mute state to `app_settings.json` across restarts; unpinned apps live only in daemon memory until cleared via **Clear Unpinned** or daemon exit.
* **Multi-Output Ratio Scaling:** Because WASAPI process loopback captures post-primary-fader audio, `audio-daemon` enforces a primary-session floor when secondary loopbacks are audible, scales loopback gain by `secondary_volume / primary_volume`, and migrates primary ownership if the primary output is unchecked while a loopback remains active.

### Global Preset Engine (`presets.json`)

* **Global Multi-Endpoint Presets:** Each `Preset` stores `endpoints` (`EndpointTarget` with `endpoint_id`, `friendly_name_pattern`, `volume`, `muted`, `default_roles`), `app_routes` (`AppRouteRule`), and `app_loopbacks` (`AppLoopbackRule`), coordinated by a single global `active_preset_id`.
* **Snapshot & Restore:** Applying a preset snapshots pre-preset hardware volumes/mutes and OS default roles so clearing the active preset restores the prior hardware state cleanly.

### UI & System Tray Client (`audio-ui`)

* **Non-Blocking STA Startup:** `audio-ui` initializes the tray icon immediately on Tauri's STA thread and spawns `audio-daemon` / populates tray preset items off-thread to prevent UI hangs.
* **Three-Tab Layout:**
  * **Applications:** Split into **Playing**, **Idle** (live silent WASAPI PIDs), and **Offline** (not running, sorted Pinned-first then A–Z) shelves, with multi-PID grouping (`N PIDs ▸`) and per-output routing sliders.
  * **System:** Allowed outputs and inputs with **Default** / **Comms** OS role toggles, device volume/mute, and unified in-track peak meters.
  * **Device Visibility:** Sorted by **Connected → Last Seen → Name** with `👁️` visibility toggles and expandable cleanup controls.
* **Window Persistence:** Saves `window_state.json` (`x`, `y`, `width`, `height`) validated against `available_monitors()` so multi-monitor placements (e.g., PowerToys FancyZones) restore reliably and fall back safely when a monitor is unplugged.

---

## 4. Edge Cases & Resilience Strategy

* **Ephemeral & Late Audio Sessions:** `IAudioSessionNotification` catches newly spawned streams (`OnSessionCreated`) and immediately applies both saved `app_settings` (`apply_to_pid`) and any active preset rules.
* **Zero-Socket Named Pipe IPC:** Communication between `audio-ui` and `audio-daemon` uses line-delimited JSON over `\\.\pipe\winsounddeck-daemon`, avoiding TCP port conflicts and firewall prompts.
* **Capture (Microphone) Support:** Capture endpoints support endpoint volume, mute, OS Default / Communications role assignment, peak meters, and **Device Visibility** filtering.

---

## 5. Changelog

* **2026-11-22 (Goals, Non-Goals & Branding Alignment):**
  * Moved **Goals** and **Non-Goals** from `README.md` into Section 1 and updated product naming to **WinSoundDeck**.

* **2026-11-19 (Spec Alignment Update):**
  * Replaced the initial per-device preset model and `.NET Core` comparison table with the implemented **Rust workspace** (`audio-core`, `audio-daemon`, `audio-ui`) and **global multi-endpoint preset** contract (`presets.json` + `app_settings.json`).
  * Documented user-mode **process loopback (`LoopbackTee`)**, `IAudioPolicyConfigFactory` per-app routing, `IAudioMeterInformation` live in-track peak meters, **Application Pinning (`📌`)**, and multi-PID session grouping.
  * Documented the **`0xB` WASAPI mask**, `known_endpoints` offline caching (`last_seen`, Bluetooth detection, UAC `pnputil` cleanup), `300ms` hot-plug `Debouncer`, non-blocking Tauri STA startup, and multi-monitor `window_state.json` persistence.
  * Removed the retired Section 5 **Steel Thread** spike specification (`crates/steel-thread` was retired after validating user-mode WASAPI/COM).
