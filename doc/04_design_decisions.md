# Design Decisions

## 1. Application Pinning & Ephemeral Unpinned State

**Date:** 2026-11-17

### Context
Users need quick access to core applications (e.g., Discord, Vivaldi) in the **Applications** tab to pre-configure output routing, volumes, and presets even before those applications open an active audio stream or launch. At the same time, tracking every transient game or one-off process permanently in `app_settings.json` would create an ever-growing, cluttered **Inactive** list.

### Decision
Implement **Application Pinning (`📌`) + In-Memory Ephemeral Unpinned State**:
- **Pinned Apps (`pinned: true`):** Persisted to `app_settings.json` across restarts. When not running, they continue to appear in **Applications → Inactive** with full support for editing primary output, secondary loopbacks, volume, and mute state; settings are automatically applied when the app next launches.
- **Unpinned Apps (`pinned: false`):** Maintained only in daemon memory (`DaemonState`) while Audio Mixer is running. They appear in **Inactive** after their audio session closes during the current Audio Mixer session, are **not** written to `app_settings.json` (so they disappear on Audio Mixer restart), and can be purged on demand via the **Clear Unpinned** button in the **Inactive** section header.

### Alternative Considered
- **7-Day Recency Eviction (`last_seen` TTL) + Manual `🗑️` Removal:** Persist customized unpinned apps to `app_settings.json` with a `last_seen` timestamp so game settings survive PC reboots during an active week of play, automatically evicting entries after 7 days of inactivity (with a `🗑️` button for immediate removal).
- This remains a straightforward future enhancement if cross-reboot persistence for unpinned games becomes desirable.

---

## 2. Device Naming & Visibility (Deferring Local Aliases & Device Pinning)

**Date:** 2026-11-17

### Context
Long Windows audio endpoint names (e.g., `Speakers (HyperX Cloud II Wireless)`) and unused virtual/monitor outputs can clutter the **System** and **Applications** tabs.

### Decision
- **Device Visibility Allowlist over Device Pinning:** Unwanted devices are already filtered out via the **Device Visibility** tab (`allowed_endpoints`), making separate device pinning unnecessary.
- **Leverage Windows Device Renaming over Local Aliases:** Audio Mixer displays Windows' `friendly_name` directly. Maintaining separate UI-only aliases in `app_settings.json` is redundant with Windows' built-in device renaming, which updates the name system-wide and flows automatically into Audio Mixer.

### Future Options
- If we want to reduce friction later, we can add a deep link from **System** / **Device Visibility** directly into Windows Sound Settings (`ms-settings:sound-devices`), or introduce optional local UI-only aliases if users want to override the parenthetical hardware suffix that Windows preserves.
