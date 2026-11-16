# Backlog

Tasks and enhancements deferred until after the 10-step Steel Thread validation phase.

## UI & UX Design (`crates/audio-ui`)
The Step 10 UI is a functional proof-of-concept; it needs a dedicated design pass to feel like a daily-driver Windows mixer:
- **Mixer Layout & Incremental Rendering:**
  - Redesign the single vertical card stack into a cleaner channel-strip or tabbed/split layout (Presets bar + Outputs / Inputs / Active Apps) with custom-styled faders, clear mute states, and compact density.
  - Replace full `innerHTML` 2-second DOM rebuilds with keyed/incremental DOM updates (or event-driven updates from `audio-daemon`) so dropdowns, scroll position, and focus never flicker or reset.
- **Live Peak Meters:**
  - Surface per-endpoint and per-session peak levels (`IAudioMeterInformation` in `audio-core`) so active audio sources are immediately visible.
- **Device Controls & Filtering:**
  - Add UI buttons + `SetDefaultEndpoint` IPC command to switch OS Default (`Multimedia` and `Communications`) directly from device cards.
  - Support hiding/unhiding unused endpoints (e.g., unused HDMI/virtual outputs), pinning favorites, and custom display aliases.
- **App Session & Multi-Output Controls:**
  - Group/collapse multiple WASAPI sessions from the same `root_exe_name` (e.g., browser subprocesses), with an option to expand individual PIDs.
  - Clearly distinguish an app's current active endpoint vs its persisted `AppPolicyClient` route.
  - Add a "Mirror / Tee to secondary device" toggle + independent gain slider per app (backed by `LoopbackTee`).
- **System Tray Polish:**
  - Dynamically populate the tray context menu with saved presets (showing a checkmark on the active preset) for 1-click switching without opening the window.
  - Anchor the mixer window cleanly near the system tray (flyout style) or remember window position/size.
  - Distinguish "Close UI (keep daemon running)" vs "Quit All (UI + daemon)".
  - Replace the placeholder 32x32 programmatic icon with a proper multi-resolution icon asset.

## Presets & Configuration
- **Preset CRUD over Named Pipe IPC (`audio-core::ipc` & `audio-daemon`):**
  - Add `SaveCurrentAsPreset { name }` to snapshot current default roles, endpoint volumes/mutes, active session volumes/mutes, and per-app routes into `%APPDATA%\audio-mixer\presets.json`.
  - Add `SavePreset(Preset)` and `DeletePreset { id }` for editing/removing presets without hand-editing JSON.
  - Add UI controls in the mixer window for creating, renaming, updating, and deleting presets.
- **Preset Snapshot / Restore via IPC:**
  - Expose `ClearActivePreset` / `RestorePreviousState` over IPC using `PresetSnapshot`.

## Audio Engine & Routing
- **Loopback Tee Daemon & Preset Integration:**
  - Manage `LoopbackHandle` lifecycles inside `audio-daemon` and wire them into `Preset` rules + IPC commands so a preset can mirror an app's audio to secondary output devices automatically.
- **Hot-Plug Debouncing & Edge Cases:**
  - Debounce rapid `DeviceStateChanged` / `DeviceAdded` bursts when USB/wireless headsets reconnect.

## Packaging & Polish
- **Windows Startup / Tray Lifecycle:**
  - Optional "Launch at Windows login" toggle and single-instance mutex for `audio-daemon` and `audio-ui`.
- **Release Optimization:**
  - Verify release-build working set size and binary size (`strip`, LTO).
