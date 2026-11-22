# Creating a Release

**WinSoundDeck** follows **Semantic Versioning (`MAJOR.MINOR.PATCH`)** with a single workspace version in [Cargo.toml](../Cargo.toml) (`[workspace.package]`) and [tauri.conf.json](../crates/audio-ui/tauri.conf.json), kept in sync by [scripts/bump-version.ps1](../scripts/bump-version.ps1).

---

## Release Categories

| Category | Version Bump | When to Use |
| :--- | :--- | :--- |
| **Patch** | `0.1.0` → `0.1.1` | Bug fixes, hang/stability fixes, refactors, or minor UI polish with no new config/feature surface. |
| **Minor** | `0.1.0` → `0.2.0` | New backwards-compatible features (new UI capabilities, new optional config fields) that work cleanly with existing `presets.json` and `app_settings.json`. |
| **Major** | `0.x.y` → `1.0.0`<br>`1.x.y` → `2.0.0` | First stable public release (`1.0.0`), or breaking changes to `presets.json` / `app_settings.json` / IPC contracts. |

---

## Pre-Release Checklist

1. Ensure you are on `main` with a clean working tree (`git status`).
2. Verify all tests pass locally:
   ```powershell
   cargo test --workspace --offline
   cargo test -p audio-ui --features portable --offline
   ```
3. *(For **Major** or **1.0.0** public releases)*:
   - Review [README.md](../README.md) and [doc/03_backlog.md](03_backlog.md) (e.g., product naming/branding, icons).
   - Confirm [troubleshoot.ps1](../troubleshoot.ps1) and [doc/00_high_level_spec.md](00_high_level_spec.md) are up to date.
   - If breaking `presets.json` or `app_settings.json`, document migration steps in the release notes.

---

## Cutting a Release

### 1. Bump Version, Commit, and Tag
Run [scripts/bump-version.ps1](../scripts/bump-version.ps1) with the desired bump type or explicit version:

```powershell
# Patch release (e.g. 0.1.0 -> 0.1.1)
.\scripts\bump-version.ps1 patch

# Minor release (e.g. 0.1.0 -> 0.2.0)
.\scripts\bump-version.ps1 minor

# Major release (e.g. 1.2.3 -> 2.0.0)
.\scripts\bump-version.ps1 major

# Explicit version (e.g. first public 1.0.0 release)
.\scripts\bump-version.ps1 1.0.0
```

This updates `Cargo.toml`, `Cargo.lock`, and `crates/audio-ui/tauri.conf.json`, creates commit `chore(release): vX.Y.Z`, and creates annotated tag `vX.Y.Z`.

### 2. Push to GitHub
```powershell
git push origin main --follow-tags
```

Pushing the `vX.Y.Z` tag triggers [.github/workflows/release.yml](../.github/workflows/release.yml) on `windows-latest`, which builds both `WinSoundDeck-portable.exe` and the NSIS installer (`*-setup.exe`) and publishes them to **GitHub Releases** with auto-generated release notes.

---

## Local Release Build (Optional)

To build both artifacts locally into `dist\`:

```powershell
.\scripts\build-release.ps1
```

---

## Rolling Back a Bad Tag (Before Publishing)

If you need to re-cut a tag before pushing (or after deleting a draft release):

```powershell
git tag -d v1.0.0
git reset --hard HEAD~1
```
