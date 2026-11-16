# Backlog

Tasks and enhancements that are straightforward or non-critical for the Steel Thread validation phase, deferred until the core architecture is proven end-to-end.

## Presets & Configuration
- **Preset CRUD over Named Pipe IPC (`audio-core::ipc` & `audio-daemon`):**
  - Add `SaveCurrentAsPreset { name }` to snapshot current default roles, endpoint volumes/mutes, active session volumes/mutes, and per-app routes into `%APPDATA%\audio-mixer\presets.json`.
  - Add `SavePreset(Preset)` and `DeletePreset { id }` for editing/removing presets without hand-editing JSON.
  - Add UI controls in the mixer window for creating, renaming, updating, and deleting presets.
- **Preset Snapshot / Restore via IPC:**
  - Expose `ClearActivePreset` / `RestorePreviousState` over IPC using `PresetSnapshot`.

## Audio Engine & Routing
- **Loopback Tee Preset Integration:**
  - Wire `LoopbackTee` (Step 7) into `Preset` rules / `audio-daemon` so a preset can optionally mirror an app's audio to secondary output devices with independent volume/mute.
- **Hot-Plug Debouncing & Edge Cases:**
  - Debounce rapid `DeviceStateChanged` / `DeviceAdded` bursts when USB/wireless headsets reconnect.

## Packaging & Polish
- **Windows Startup / Tray Lifecycle:**
  - Optional "Launch at Windows login" toggle and single-instance mutex for `audio-daemon` and `audio-ui`.
- **Release Optimization:**
  - Verify release-build working set size and binary size (`strip`, LTO).
