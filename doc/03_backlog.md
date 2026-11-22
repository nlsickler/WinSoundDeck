# Backlog

Remaining enhancements after the 10-step Steel Thread validation and initial UI/routing polish pass.

## Packaging & Polish
- **Product Naming & Branding:**
  - Pick an official product name (replacing `Audio Mixer` / `audio-mixer`) and update app identifier, tray tooltip, window title, and multi-resolution icon assets.
- **Windows Startup / Tray Lifecycle:**
  - Optional "Launch at Windows login" toggle and single-instance mutex for `audio-daemon` and `audio-ui`.
- **Release Optimization:**
  - Verify release-build working set size and binary size (`strip`, LTO).
