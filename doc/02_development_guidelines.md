# Development & Rust Implementation Guidelines

This document defines the coding standards and architectural rules for the **Audio Preset & Device Routing Manager** workspace.

---

## 1. Crate & Module Organization
* **Domain-Based Modules:** Group tightly coupled abstractions and helpers into focused modules (e.g., `com.rs`, `enumerator.rs`, `endpoint.rs`, `session.rs`, `policy.rs`, `events.rs`). Avoid both monolithic `lib.rs` files and rigid one-struct-per-file boilerplate.
* **Crate Root Re-exports:** `lib.rs` declares internal modules and exposes a flat, ergonomic public API via `pub use`.
* **Core Separation:** Reusable WASAPI/COM logic lives in `crates/audio-core`; `crates/audio-daemon` handles background routing/state and `crates/audio-ui` handles presentation.

## 2. Narrow `unsafe` Guards & Safety Invariants
* **Minimal Scope:** Every `windows-rs` COM/Win32 FFI call is `unsafe`, so wrap **only** the FFI call(s) themselves in `unsafe { ... }`. Keep `Vec` allocations, string/path manipulation, control flow, and safe `windows-rs` wrappers (e.g., `.cast()`, `PROPVARIANT::to_string()`) outside `unsafe`.
* **`// SAFETY:` Comments:** Document non-trivial invariants directly above `unsafe` blocks involving raw pointers, out-buffers, manual memory freeing (`CoTaskMemFree`), or custom COM interface definitions (`IPolicyConfig`).

## 3. COM Lifetimes & Defensive Bounds
* **RAII Resource Management:** Initialize COM via `ComGuard` and pair OS handles/allocations (`OpenProcess`/`CloseHandle`, `GetId`/`CoTaskMemFree`) immediately in small helper functions so leaks cannot occur on early returns.
* **Input Clamping:** Clamp all normalized volume scalars to `0.0..=1.0` at the `audio-core` boundary before passing them to WASAPI.
* **Isolate Undocumented APIs:** Undocumented Windows interfaces (`IPolicyConfig`, `IAudioPolicyConfigFactory`) must be isolated in dedicated modules with clear trait/struct wrappers so Windows build quirks are contained in one place.
* **Verify Reference Signatures Before Coding:** When implementing any undocumented COM/WinRT interface, verify the exact IID, base interface (`IUnknown` vs. `IInspectable`), slot count, and parameter ABI types (`HSTRING` vs. `PCWSTR`, SWD path vs. endpoint ID) against a canonical reference implementation (e.g., EarTrumpet) before writing vtable stubs.

## 4. Live Hardware Testing & Experimentation Hygiene
* **Planned & Communicated Experiments:** Plan each experiment upfront and communicate what will be tested, what changes are being made, and any hypotheses before iterating. When tackling undocumented Windows behaviors, research canonical open-source references (such as MIT-licensed EarTrumpet, respecting any file/component license exclusions) before prototyping.
* **Apply & Undo:** Experimental steps should record initial state, apply the change, verify via readback, and restore the original state before exiting.
* **Conflict Avoidance:** Confirm SteelSeries Sonar (or other external audio mixers) is shut down before running live hardware tests.
* **Sandbox Boundary:** Enumeration works in the restricted terminal sandbox, but `IMMDevice::Activate` (`IAudioSessionManager2`, `IAudioEndpointVolume`) requires unsandboxed host execution (`0x80070005` otherwise).

---

## 5. Workspace Structure, Building & Packaging

### Workspace Layout

```text
crates/
  audio-core/     # Shared Windows Core Audio (COM/WASAPI) wrappers, loopback tee, icon extraction, presets, IPC
  audio-daemon/   # Background service, named-pipe server, hot-plug watcher, app settings store
  audio-ui/       # Tauri v2 desktop & system tray application (HTML/CSS/JS frontend)
doc/
  00_high_level_spec.md
  01_available_alternatives.md
  02_development_guidelines.md
  03_backlog.md
  04_design_decisions.md
  05_create_release.md
scripts/
  build-release.ps1  # Builds Portable (.exe) and NSIS installer targets into dist/
  bump-version.ps1   # Bumps workspace + Tauri version, commits, and tags vX.Y.Z
  setup-dev.ps1      # Installs build-time prerequisites (tauri-cli)
troubleshoot.ps1     # Headless Windows WASAPI & PnP diagnostic bundle script
```

### Building & Testing

```powershell
cargo build --workspace --offline
cargo test --workspace --offline
```

To launch the UI (automatically starts `audio-daemon.exe` if not already running):

```powershell
.\target\debug\audio-ui.exe
```

### Release Packaging (Portable & Installer)

Building the NSIS Windows installer requires `tauri-cli` (`^2`). On a fresh machine, run:

```powershell
.\scripts\setup-dev.ps1
```

*(Or manually: `cargo install tauri-cli --version "^2" --locked`. Tauri's bundler automatically downloads a cached copy of NSIS on its first `cargo tauri build` run.)*

Then build both distribution targets into `dist\`:

```powershell
.\scripts\build-release.ps1
```

- `dist\WinSoundDeck-portable.exe`: Self-contained single executable (`--features portable`, self-spawns with `--daemon`).
- `dist\WinSoundDeck_<version>_x64-setup.exe`: NSIS installer bundling `audio-ui.exe` + `audio-daemon.exe` sidecar with pre-install/pre-uninstall process cleanup hooks.
- Use `-PortableOnly` or `-InstallerOnly` to build a single target. For tagging and publishing releases, see [doc/05_create_release.md](05_create_release.md).
