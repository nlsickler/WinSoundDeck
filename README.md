# Audio Mixer

A lightweight, native Windows audio mixer, per-application router, and preset manager built in Rust on top of Windows Core Audio (WASAPI / COM) and Tauri.

## Purpose

Windows lacks a fast, native way to combine per-application volume control, multi-output routing, OS default device switching, and hardware-resilient presets without installing virtual audio cable drivers or heavy commercial mixer suites. **Audio Mixer** runs entirely in user mode against Windows Core Audio APIs to provide deterministic audio routing with a minimal footprint.

## Goals

- **Zero-Driver User-Mode Architecture:** Interface directly with Windows Core Audio (`IMMDeviceEnumerator`, `IAudioSessionManager2`, `ISimpleAudioVolume`, `IAudioEndpointVolume`, `IAudioMeterInformation`, and `IPolicyConfig` / `IAudioPolicyConfigFactory`) without kernel drivers or socket-based virtual cables.
- **Decoupled Daemon & Tray UI:** Run a lightweight headless background daemon (`audio-daemon`) that reacts to hardware hot-plug and session events (`IMMNotificationClient`, `IAudioSessionNotification`) over a local Windows Named Pipe, paired with a Tauri desktop/tray UI (`audio-ui`) that remembers its window placement across monitors.
- **Deterministic Presets:** Save and restore hardware endpoint volumes/mutes, Windows Default (`Multimedia` / `Console`) and Communications defaults, per-app primary routes, and secondary loopback outputs.
- **Per-App Multi-Output Routing & Pinning:** Route apps to a primary output plus optional secondary loopback outputs (`LoopbackTee`), group multi-PID apps cleanly, and pin core apps (`📌`) so their settings persist across restarts while unpinned apps stay in memory only.
- **Device Visibility & Offline Preset Setup:** Filter out unused devices via an allowlist, cache previously seen devices (`known_endpoints`) for offline preset configuration without probing orphaned `NOTPRESENT` PnP entries, and clean up stale devices.
- **Live Feedback:** Display subtle in-track live peak meters behind volume sliders while the window is visible.

## Non-Goals

- **No Kernel-Mode Virtual Audio Drivers:** No custom WDM/APO drivers or virtual sound cards.
- **No Audio DSP / EQ / Effects Pipeline:** Audio Mixer routes and scales streams; it does not apply EQ, compression, noise suppression, or VST plugins.
- **No Per-Tab Browser Mixing:** Modern browsers (e.g., Vivaldi, Chrome, Firefox) mix tabs internally before handing a single audio stream to Windows WASAPI; Audio Mixer operates at the OS process/PID level exposed by Windows.
- **No Cross-Platform Support:** Built specifically for Windows 10/11 Core Audio APIs.
- **No Duplicate Device Aliasing Layer:** Relies on Windows' built-in device renaming (and Audio Mixer's **Device Visibility** allowlist) rather than maintaining separate UI-only device aliases.

## Workspace Structure

```text
crates/
  audio-core/     # Shared Windows Core Audio (COM/WASAPI) wrappers, loopback tee, presets, IPC
  audio-daemon/   # Background service, named-pipe server, hot-plug watcher, app settings store
  audio-ui/       # Tauri desktop & system tray application (HTML/CSS/JS frontend)
doc/
  00_high_level_spec.md
  02_development_guidelines.md
  03_backlog.md
  04_design_decisions.md
scripts/
  build-release.ps1  # Builds Portable (.exe) and NSIS installer targets into dist/
  setup-dev.ps1      # Installs build-time prerequisites (tauri-cli)
troubleshoot.ps1     # Headless Windows WASAPI & PnP diagnostic bundle script
```

## Building & Testing

```powershell
cargo build --workspace --offline
cargo test --workspace --offline
```

To launch the mixer (automatically starts `audio-daemon.exe` if not already running):

```powershell
.\target\debug\audio-ui.exe
```

## Release Packaging (Portable & Installer)

Building the NSIS Windows installer requires `tauri-cli` (`^2`). On a fresh machine, run:

```powershell
.\scripts\setup-dev.ps1
```

*(Or manually: `cargo install tauri-cli --version "^2" --locked`. Tauri's bundler automatically downloads a cached copy of NSIS on its first `cargo tauri build` run.)*

Then build both distribution targets into `dist/`:

```powershell
.\scripts\build-release.ps1
```

- `dist\AudioMixer-portable.exe`: Self-contained single executable (`--features portable`, self-spawns with `--daemon`).
- `dist\Audio Mixer_0.1.0_x64-setup.exe`: NSIS installer bundling `audio-ui.exe` + `audio-daemon.exe` sidecar with pre-install/pre-uninstall process cleanup hooks.
- Use `-PortableOnly` or `-InstallerOnly` to build a single target.
