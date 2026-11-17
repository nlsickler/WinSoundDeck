# Backlog

Remaining enhancements after the 10-step Steel Thread validation and initial UI/routing polish pass.

## UI & UX Design (`crates/audio-ui`)
- **Live Peak Meters:**
  - Surface per-endpoint and per-session peak levels (`IAudioMeterInformation` in `audio-core`) so active audio sources are immediately visible.
- **Device Aliases & Pinning:**
  - Support pinning favorite devices to the top and custom display aliases for long Windows endpoint names.
- **App Session Polish:**
  - Group/collapse multiple WASAPI sessions from the same `root_exe_name` when an app spawns multiple audio PIDs, with an option to expand individual PIDs.
- **System Tray Polish:**
  - Anchor the mixer window cleanly near the system tray (flyout style) or remember window position/size.
  - Replace the programmatic 32x32 tray icon with a multi-resolution icon asset.

## Audio Engine & Routing
- **Hot-Plug Debouncing & Edge Cases:**
  - Debounce rapid `DeviceStateChanged` / `DeviceAdded` bursts when USB/wireless headsets reconnect.

## Packaging & Polish
- **Dual Distribution Targets (Portable + Installer):**
  - **Portable (`--features portable`):** Expose `audio-daemon`'s entrypoint as a library call behind a Cargo feature flag so `audio-ui.exe` can self-spawn with `--daemon` as a single self-contained `.exe`.
  - **Installer (`cargo tauri build`):** Add `tauri.installer.conf.json` with `"bundle": { "active": true, "targets": ["nsis"] }`, `audio-daemon.exe` sidecar (`externalBin`), and NSIS upgrade/uninstall process cleanup hooks.
- **Product Naming & Branding:**
  - Pick an official product name (replacing `Audio Mixer` / `audio-mixer`) and update app identifier, tray tooltip, window title, and multi-resolution icon assets.
- **Windows Startup / Tray Lifecycle:**
  - Optional "Launch at Windows login" toggle and single-instance mutex for `audio-daemon` and `audio-ui`.
- **Release Optimization:**
  - Verify release-build working set size and binary size (`strip`, LTO).
