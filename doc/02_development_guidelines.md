# Development & Rust Implementation Guidelines

This document defines the coding standards and architectural rules for the **Audio Preset & Device Routing Manager** workspace.

---

## 1. Crate & Module Organization
* **Domain-Based Modules:** Group tightly coupled abstractions and helpers into focused modules (e.g., `com.rs`, `enumerator.rs`, `endpoint.rs`, `session.rs`, `policy.rs`, `events.rs`). Avoid both monolithic `lib.rs` files and rigid one-struct-per-file boilerplate.
* **Crate Root Re-exports:** `lib.rs` declares internal modules and exposes a flat, ergonomic public API via `pub use`.
* **Thin Experiment Harness:** `crates/steel-thread` acts strictly as a CLI test runner. Reusable WASAPI/COM logic lives in `crates/audio-core`.

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
