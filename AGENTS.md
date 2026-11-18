# Project Instructions (`audio-mixer`)

Follow [doc/02_development_guidelines.md](doc/02_development_guidelines.md) for crate boundaries, narrow `unsafe` COM guards, and Windows Core Audio safety invariants. Record significant architectural choices and trade-offs in [doc/04_design_decisions.md](doc/04_design_decisions.md).

## 1. TDD Workflow (Red → Green → Refactor)

Use **Red → Green → Refactor** as the default workflow for all new features, bug fixes, and state/logic changes:

1. **Red:**
   - Extract or isolate pure logic (state transitions, filtering/sorting, serialization, ID slugification, volume/scaling math) from Windows COM/WASAPI and DOM side effects.
   - Write a failing unit test first (or run `cargo test --workspace --offline` to confirm it fails for the expected reason).
2. **Green:**
   - Implement the minimal, correct change needed to make the test pass and satisfy the requirement.
   - Verify with `cargo test --workspace --offline` and `cargo build --workspace --offline`.
3. **Refactor (Mandatory Cleanup Step):**
   - **Prune dead surface area:** Remove superseded IPC variants, Tauri commands, unused helpers, and stale doc references replaced by the change.
   - **Eliminate duplication:** Extract shared helpers (e.g., `createMeterSlider()` in `dom.js`, `resolve_default()` in `status.rs`) rather than copying DOM or Rust blocks.
   - **Keep modules focused:** Split files into cohesive domain submodules (e.g., `handlers/apps.rs`, `handlers/presets.rs`) when a module grows large or mixes unrelated concerns.
   - Re-run `cargo build --workspace --offline` and `cargo test --workspace --offline` after refactoring to confirm all tests stay green with zero new compiler warnings.

*(For changes that strictly touch Windows COM FFI or Tauri/UI visual layout where no pure logic is altered, still keep logic decoupled and verify with a clean workspace build and test run.)*

## 2. Build, Process, & Git Hygiene

- **Running Processes on Windows:** If `audio-ui.exe` or `audio-daemon.exe` is running and locks `target\debug\*.exe` during a build, stop `audio-ui` and `audio-daemon` cleanly before rebuilding.
- **Verification Gate:** Never report a task complete until `cargo build --workspace --offline` and `cargo test --workspace --offline` pass after the final edit.
- **Commit Changes:** Commit completed, verified work with clear conventional commit messages (e.g., `feat(...)`, `fix(...)`, `refactor(...)`, `docs(...)`).
