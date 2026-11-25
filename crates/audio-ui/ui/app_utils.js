export const DEFAULT_APP_ICON_URL =
  "data:image/svg+xml;utf8," +
  encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">` +
      `<rect x="1.5" y="1.5" width="21" height="21" rx="5" fill="#282d3e" stroke="#3b4259" stroke-width="1.5"/>` +
      `<rect x="6.5" y="10" width="2.5" height="7" rx="1.2" fill="#818cf8"/>` +
      `<rect x="10.75" y="6.5" width="2.5" height="10.5" rx="1.2" fill="#a5b4fc"/>` +
      `<rect x="15" y="9" width="2.5" height="8" rx="1.2" fill="#818cf8"/>` +
      `</svg>`
  );

export function applyAppIcon(iconEl, iconUrl, showDefault = true) {
  if (!iconEl) return;
  if (!iconUrl && !showDefault) {
    iconEl.classList.add("collapsed");
    return;
  }
  const resolved = iconUrl || DEFAULT_APP_ICON_URL;
  if (iconEl.dataset.resolvedSrc !== resolved) {
    iconEl.dataset.resolvedSrc = resolved;
    iconEl.onerror = () => {
      if (iconEl.dataset.resolvedSrc !== DEFAULT_APP_ICON_URL) {
        iconEl.dataset.resolvedSrc = DEFAULT_APP_ICON_URL;
        iconEl.src = DEFAULT_APP_ICON_URL;
      }
    };
    iconEl.src = resolved;
  }
  iconEl.classList.remove("collapsed");
}

export function appIdFor(s) {
  return s.pid > 0 ? String(s.pid) : s.app_key || "0";
}

export function appDisplayName(s) {
  return s.display_name || s.root_exe_name || s.exe_name || `PID ${s.pid}`;
}

export function stateBadgeFor(s) {
  if (s.is_active) {
    return {
      text: "Playing",
      variant: "playing",
      title: "Currently playing audio",
    };
  }
  if (s.pid > 0) {
    return {
      text: "Idle",
      variant: "idle",
      title: "Idle (running, not currently playing audio)",
    };
  }
  return {
    text: "Offline",
    variant: "offline",
    title: "Offline (not currently running)",
  };
}

export function groupSessionsByApp(sessions) {
  const groups = [];
  const byKey = new Map();
  for (const s of sessions) {
    const key = (s.app_key || s.root_exe_name || s.exe_name || String(s.pid)).toLowerCase();
    let group = byKey.get(key);
    if (!group) {
      group = { key, primary: s, sessions: [] };
      byKey.set(key, group);
      groups.push(group);
    }
    group.sessions.push(s);
    if (!group.primary.is_active && s.is_active) {
      group.primary = s;
    }
  }
  return groups;
}

export function endpointRoleRank(ep) {
  if (ep.is_default_multimedia) return 0;
  if (ep.is_default_communications) return 1;
  return 2;
}

export function sortEndpointsByRoleAndName(endpoints) {
  return [...endpoints].sort((a, b) => {
    const rankDiff = endpointRoleRank(a) - endpointRoleRank(b);
    if (rankDiff !== 0) return rankDiff;
    const nameDiff = (a.friendly_name || "").localeCompare(b.friendly_name || "", undefined, {
      sensitivity: "base",
    });
    if (nameDiff !== 0) return nameDiff;
    return (a.id || "").localeCompare(b.id || "");
  });
}

export function loopbacksForSession(loopbacks, s) {
  return loopbacks.filter((lb) =>
    s.pid > 0 ? lb.pid === s.pid : s.app_key && lb.app_key === s.app_key
  );
}
