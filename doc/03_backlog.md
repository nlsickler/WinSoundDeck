# Backlog

All planned enhancements from the initial backlog have been completed. Future feature candidates are prioritized below using **MoSCoW** (`Should Have` → `Could Have` → `Won't Have`).

---

## Should Have

- [ ] **Preset Rename & Reorder:** Clicking on `PRESETS` opens a quick edit dialog to rename and reorder presets.
- [ ] **Scroll-Wheel & Double-Click Faders:** Scroll over any slider (or vertical fader strip) for `2%` / `5%` volume steps, and double-click to snap to `100%`.
- [ ] **Hardware Auto-Switch Rules:** Bind a preset to a device connect/disconnect event (e.g., *"When `HyperX Cloud II Wireless` connects → apply `🎧 Only`; when disconnected → apply `🔊 Only`"*).
- [ ] **Virtual "Focus" / Solo Mode:** Right-click an app or strip to **Solo** it (temporarily muting other apps, restoring them when un-soloed).
- [ ] **Apps / Devices Quick Search:** Lightweight filter input when managing 15+ apps or 10+ endpoints.

---

## Could Have

- [ ] **Start Minimized to Tray (`--minimized`):** When **Launch at Windows login** starts WinSoundDeck on boot, keep the main window hidden in the tray instead of popping it open over the desktop.
- [ ] **Update Checker:** A non-intrusive **Check for Updates** button in `⚙️` → **Advanced** querying GitHub Releases.
- [ ] **CLI / Stream Deck Triggering:** Add a lightweight CLI flag (e.g., `WinSoundDeck.exe --apply-preset "Headset"`) so users can trigger presets from Stream Deck, AutoHotkey, or shortcuts.
- [ ] **Communications Auto-Ducking:** Automatically lower music/browser/game volume by a configurable percentage while Discord/Teams/Zoom is active, then smoothly restore it.
- [ ] **Compact "Mini Deck" Mode:** A slim always-on-top window showing just Presets + the **Devices** vertical fader shelf.
- [ ] **Windows Authenticode Code Signing:** Set up a Windows-accepted signing solution (e.g., SignPath Foundation or Azure Artifact Signing) for the portable `.exe`, sidecar, and NSIS installer. *Gate: gated on user adoption; GitHub Artifact Attestations are already enabled in the release pipeline.*

---

## Won't Have (Out of Scope)

- **Preset Quick-Update:** Saving a preset with the same name already overwrites the existing definition.
- **Global Hotkeys:** System-wide shortcuts for presets or mic mute.
- **MIDI / USB Volume Knob Mapping:** External rotary encoder / MIDI fader bindings.
