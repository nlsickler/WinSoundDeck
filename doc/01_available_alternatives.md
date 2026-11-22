# Available Alternatives — SteelSeries GG Sonar vs. Logitech MIXLINE vs. WinSoundDeck

This document compares **SteelSeries GG (Sonar)**, **Logitech MIXLINE**, and **WinSoundDeck** across architecture, routing model, system footprint, and ideal use cases.

> For WinSoundDeck's own architecture and trade-offs, see [00_high_level_spec.md](00_high_level_spec.md) and [04_design_decisions.md](04_design_decisions.md).

---

## 1. Executive Comparison Matrix

| Dimension | SteelSeries GG (Sonar) | Logitech MIXLINE | WinSoundDeck |
| :--- | :--- | :--- | :--- |
| **Audio Architecture** | Multiple kernel-level Virtual Audio Drivers (VADs) | Single kernel-level Virtual Audio Driver | **Zero drivers** — user-mode Windows Core Audio (WASAPI / COM / `VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK`) |
| **Windows Sound Device Clutter** | High — adds 5+ virtual playback/recording endpoints (`Game`, `Chat`, `Media`, `Aux`, `Mic`) | Low–Moderate — adds virtual routing endpoints managed by MIXLINE | **None** — routes directly to existing physical/OS endpoints; hides unwanted devices via **Device Visibility** |
| **Routing Model** | Fixed channel buckets (`Game`, `Chat`, `Media`, `Aux`, `Mic`) → 1 output device per channel | Visual drag-and-drop patchbay (inputs/apps → virtual submixes/outputs) | **Direct per-app primary route** (`IAudioPolicyConfigFactory`) + **per-app multi-output teeing** (`LoopbackTee`) with independent volume/mute per output |
| **Multi-Output Audio (Same App → 2+ Devices)** | Limited (Stream Mode splits Mixes, but standard channels target one physical output each) | Yes — route any input to multiple outputs/submixes simultaneously | **Yes** — any app can play to a primary output and mirror to any number of secondary outputs with independent faders |
| **DSP / EQ / Mic Processing** | **Strong** — 10-band parametric EQ per channel, spatial audio, ClearCast AI mic noise cancellation, ChatMix | Minimal — focuses on mixing/routing; can downmix 5.1/7.1 spatial audio to stereo | **None** — delegates hardware DSP/spatial formats directly to the physical endpoint drivers |
| **Streamer / OBS Multi-Track Submixes** | Supported via Streamer Mode (`Personal` vs. `Stream` mixes), though requires careful OBS setup | **Strong** — built specifically for streamers/creators with OBS/Streamlabs VOD track splits | Not targeted — no virtual microphone or virtual OBS capture bus |
| **Global Hardware & Routing Presets** | Per-channel config & device fallback | Saved routing/mix configurations | **Global multi-endpoint presets** (`presets.json`) with OS Default/Comms role switching, hardware volume/mute targets, and **snapshot/restore** |
| **Hot-Plug & Dock Resilience** | Can lose/reset routing across Windows updates or GG service restarts | Dependent on virtual driver stability across sleep/wake and device changes | **Reactive `300ms` debounced `IMMNotificationClient`** re-arms session watchers and re-applies active presets automatically on USB-C dock/headset changes |
| **Offline App & Device Memory** | Apps generally must be active/playing to be detected and assigned | Apps/inputs managed within active patchbay | **Pinning (`📌`)** persists routes/volumes across restarts; **Offline** shelf & `known_endpoints` cache preserve rules even when apps or docks are disconnected |
| **Footprint & Suite Lock-In** | Heavy — bundled inside SteelSeries GG (account prompts, Engine/Moments bloat, background services) | Moderate — standalone desktop app + virtual audio driver | **Lightweight** — native Rust daemon + Tauri v2 tray UI (~few MB), zero telemetry, **Portable `.exe`** or **NSIS installer** |
| **License & Platform** | Proprietary freeware (Windows 10/11) | Proprietary freeware (Windows 10/11) | **Open source** (Windows 10/11) |

---

## 2. Detailed Breakdown

### SteelSeries GG (Sonar)

**What it is:** A virtual audio mixer and DSP suite bundled inside the SteelSeries GG peripheral ecosystem. It intercepts Windows audio by registering a set of fixed virtual endpoints (`Sonar - Gaming`, `Sonar - Chat`, `Sonar - Media`, `Sonar - Aux`, `Sonar - Microphone`) and forwarding each channel to a chosen physical output.

* **Strengths:**
  * Rich built-in DSP: 10-band parametric EQ per channel, game-tuned acoustic presets, spatial audio, and ClearCast AI noise reduction for microphones.
  * Convenient **ChatMix** balance between Game and Chat channels (especially with SteelSeries headsets).
  * Streamer Mode provides separate personal monitoring and broadcast mixes without extra third-party plugins.
* **Limitations & Friction:**
  * **Virtual device clutter:** Pollutes the Windows Sound Control Panel and app device pickers with multiple virtual endpoints. If Windows or a game resets its default device, audio can silently route to the wrong endpoint.
  * **Suite bloat & fragility:** Requires the full SteelSeries GG background stack; GG updates or Windows audio updates occasionally reset channel assignments or drop audio if the service hangs.
  * **Fixed bucket abstraction:** You must fit every workflow into `Game` / `Chat` / `Media` / `Aux` rather than giving individual apps arbitrary multi-output combinations.

---

### Logitech MIXLINE

**What it is:** A free, creator-focused virtual audio patchbay and submix router from Logitech G designed to replace VoiceMeeter and hardware mixers for streamers and podcasters.

* **Strengths:**
  * **Intuitive visual patchbay:** Drag-and-drop lines connect inputs (apps, mics, hardware) on the left to outputs/submixes on the right.
  * **Cleaner than multi-VAD mixers:** Uses a single virtual audio driver architecture rather than flooding Windows with half a dozen separate virtual cables.
  * **Creator-ready submixes:** Makes it easy to send one input to multiple destinations simultaneously (e.g., headphones + stream mix + VOD-safe recording track in OBS/Streamlabs).
* **Limitations & Friction:**
  * **Spatial/surround downmixing:** Routing through its virtual mixer can downmix 5.1/7.1 or spatial audio formats to 2.1/stereo and bypass hardware-specific enhancements.
  * **Driver single-point-of-failure:** Because all routed audio passes through its software engine and virtual driver, any hitch or crash in the app or audio service interrupts the entire audio chain.
  * **Overkill for desktop switching:** Optimized for continuous broadcast submixes rather than quick hardware profile switching (e.g., Desk Dock ↔ Living Room TV ↔ Headset) or cleaning up orphaned Windows audio devices.

---

### WinSoundDeck

**What it is:** A lightweight, open-source Windows audio router and preset orchestrator written in Rust (`audio-core`, `audio-daemon`, `audio-ui`) that controls Windows Core Audio directly in user mode without installing any virtual audio drivers.

* **Strengths:**
  * **Zero virtual drivers or cables:** Uses native `IPolicyConfig` / `IAudioPolicyConfigFactory` for per-app primary routing and `ActivateAudioInterfaceAsync` (`VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK`) for secondary output mirroring. Audio passes through Windows' native stack with no extra kernel drivers, no virtual device clutter, and no forced downmixing of your primary endpoint.
  * **True per-app multi-output teeing:** Any app can output to a primary device and mirror to additional outputs with independent volume faders and mute toggles.
  * **Hardware-first preset engine:** One-click presets (from the UI or system tray) set hardware volumes/mutes, OS `Default` and `Communications` roles, per-app routes, and loopbacks—plus **snapshot & restore** when clearing a preset.
  * **Built for docks, laptops, and hot-plugging:** A `300ms` debounced `IMMNotificationClient` watcher handles USB-C dock/Bluetooth bursts cleanly; **Pinning (`📌`)** and `known_endpoints` preserve rules while apps or devices are offline; **Device Visibility** hides clutter and supports Bluetooth settings links or elevated `pnputil` ghost cleanup.
  * **Minimal footprint:** Runs as a tiny background daemon + Tauri v2 system tray UI with optional **Launch at Windows login**, available as both a **portable `.exe`** and an **NSIS installer**.
* **Limitations & Friction:**
  * **No built-in EQ, AI noise suppression, or spatial DSP:** Intentionally out of scope; users who want parametric EQ or mic AI filtering should pair it with hardware/driver DSP or tools like Equalizer APO.
  * **No virtual microphone or virtual OBS capture busses:** Cannot create synthetic virtual input devices for multi-track VOD splitting in OBS (use MIXLINE or OBS's native Application Audio Capture for that workflow).
  * **Primary-fader dependency on loopback:** Because WASAPI process loopback captures post-primary-fader audio, WinSoundDeck manages a primary-session floor and ratio scaling when mirroring audio to secondary outputs.

---

## 3. When to Choose Which

* **Choose SteelSeries GG Sonar** if you want an all-in-one gaming audio suite with built-in parametric EQ, virtual surround, AI mic noise cancellation, and ChatMix—and don't mind virtual audio devices or running the SteelSeries GG suite.
* **Choose Logitech MIXLINE** if you are a **live streamer or podcaster** who wants a visual patchbay to build separate headphone, stream, and VOD recording submixes for OBS/Streamlabs.
* **Choose WinSoundDeck** if you want a **fast, driver-free, open-source Windows utility** for per-app routing, multi-output mirroring, hardware + OS-role presets, and rock-solid USB-C dock / Bluetooth hot-plug behavior without suite bloat or virtual device clutter.
