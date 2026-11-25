import { loadBoolPref, saveBoolPref, syncKeyedList } from "./dom.js";
import {
  appDisplayName,
  appIdFor,
  applyAppIcon,
  groupSessionsByApp,
  loopbacksForSession,
  sortEndpointsByRoleAndName,
  stateBadgeFor,
} from "./app_utils.js";
import { clearPrimaryUnchecked, isPrimaryUnchecked } from "./routing_actions.js";
import { buildRoutingRow, updateRoutingRow } from "./routing_row.js";

const { invoke } = window.__TAURI__.core;

const PREF_APPS_IDLE = "winsounddeck.apps.show_idle";
const PREF_APPS_OFFLINE = "winsounddeck.apps.show_offline";

const collapsedActiveIds = new Set();
const expandedInactiveIds = new Set();
const expandedSubPidGroups = new Set();
let showIdleApps = loadBoolPref(PREF_APPS_IDLE, false);
let showOfflineApps = loadBoolPref(PREF_APPS_OFFLINE, false);

export function hasActiveAppsFilters() {
  return showIdleApps || showOfflineApps;
}

export function setupAppsTabControls(onRefresh, onFilterChange) {
  const idleCb = document.getElementById("filter-apps-idle-cb");
  const offlineCb = document.getElementById("filter-apps-offline-cb");
  if (idleCb) {
    idleCb.checked = showIdleApps;
    idleCb.onchange = () => {
      showIdleApps = idleCb.checked;
      saveBoolPref(PREF_APPS_IDLE, showIdleApps);
      if (onFilterChange) onFilterChange();
      if (onRefresh) onRefresh(true);
    };
  }
  if (offlineCb) {
    offlineCb.checked = showOfflineApps;
    offlineCb.onchange = () => {
      showOfflineApps = offlineCb.checked;
      saveBoolPref(PREF_APPS_OFFLINE, showOfflineApps);
      if (onFilterChange) onFilterChange();
      if (onRefresh) onRefresh(true);
    };
  }
  const clearBtn = document.getElementById("apps-clear-unpinned-btn");
  if (clearBtn) {
    clearBtn.onclick = async (e) => {
      e.stopPropagation();
      await invoke("clear_unpinned_apps");
      if (onRefresh) await onRefresh(true);
    };
  }
}

function buildAppCard(onRefresh) {
  const card = document.createElement("div");
  card.className = "card";

  const header = document.createElement("div");
  header.className = "app-header-row";

  const titleToggle = document.createElement("div");
  titleToggle.className = "app-title-toggle";
  titleToggle.style.flex = "1";
  titleToggle.style.minWidth = "0";
  titleToggle.style.maxWidth = "none";
  const chevron = document.createElement("span");
  chevron.className = "chevron";
  const icon = document.createElement("img");
  icon.className = "app-icon collapsed";
  icon.alt = "";
  const title = document.createElement("span");
  title.className = "card-title";
  const stateBadge = document.createElement("span");
  stateBadge.className = "badge-tag strip-state-badge collapsed";
  titleToggle.append(chevron, icon, title, stateBadge);

  const pinBtn = document.createElement("button");
  pinBtn.className = "btn-pin";
  pinBtn.textContent = "📌";

  const muteBtn = document.createElement("button");
  muteBtn.className = "btn-icon-mute";

  const meta = document.createElement("div");
  meta.className = "card-meta";
  const outCountBadge = document.createElement("span");
  outCountBadge.className = "badge-muted";
  const pidBadge = document.createElement("span");
  pidBadge.className = "badge-tag pid-badge";
  meta.append(pidBadge, outCountBadge);

  header.append(titleToggle, meta, pinBtn, muteBtn);

  const body = document.createElement("div");
  body.className = "routing-outputs";

  const subPidsList = document.createElement("div");
  subPidsList.className = "sub-pids-list collapsed";

  card.append(header, body, subPidsList);

  const toggleExpand = () => {
    const group = card._group;
    if (!group) return;
    const s = group.primary;
    if (s.is_active) {
      if (collapsedActiveIds.has(group.key)) {
        collapsedActiveIds.delete(group.key);
      } else {
        collapsedActiveIds.add(group.key);
      }
    } else if (expandedInactiveIds.has(group.key)) {
      expandedInactiveIds.delete(group.key);
    } else {
      expandedInactiveIds.add(group.key);
    }
    const expanded = s.is_active
      ? !collapsedActiveIds.has(group.key)
      : expandedInactiveIds.has(group.key);
    chevron.textContent = expanded ? "▾" : "▸";
    body.classList.toggle("collapsed", !expanded);
    subPidsList.classList.toggle(
      "collapsed",
      !expanded || group.sessions.length <= 1 || !expandedSubPidGroups.has(group.key)
    );
  };
  titleToggle.onclick = toggleExpand;
  outCountBadge.onclick = toggleExpand;
  outCountBadge.style.cursor = "pointer";

  pidBadge.onclick = (e) => {
    e.stopPropagation();
    const group = card._group;
    if (!group || group.sessions.length <= 1) {
      toggleExpand();
      return;
    }
    if (expandedSubPidGroups.has(group.key)) {
      expandedSubPidGroups.delete(group.key);
    } else {
      expandedSubPidGroups.add(group.key);
    }
    const showSubs = expandedSubPidGroups.has(group.key);
    pidBadge.textContent = `${group.sessions.length} PIDs ${showSubs ? "▾" : "▸"}`;
    subPidsList.classList.toggle("collapsed", !showSubs);
  };

  pinBtn.onclick = async () => {
    const group = card._group;
    if (!group) return;
    const s = group.primary;
    await invoke("set_app_pinned", {
      pid: s.pid,
      appKey: s.app_key || null,
      pinned: !s.is_pinned,
    });
    await onRefresh(true);
  };

  muteBtn.onclick = async () => {
    const group = card._group;
    if (!group) return;
    const targetMuted = !group.sessions.every((x) => x.muted);
    for (const s of group.sessions) {
      clearPrimaryUnchecked(appIdFor(s));
      await invoke("set_session_mute", {
        endpointId: s.endpoint_id,
        pid: s.pid,
        appKey: s.app_key || null,
        muted: targetMuted,
      });
    }
    await onRefresh(true);
  };

  return card;
}

function buildSubPidCard(onRefresh) {
  const card = document.createElement("div");
  card.className = "sub-pid-card";

  const header = document.createElement("div");
  header.className = "sub-pid-header";
  const label = document.createElement("span");
  label.className = "badge-tag";
  const muteBtn = document.createElement("button");
  muteBtn.className = "btn-icon-mute";
  header.append(label, muteBtn);

  const outputs = document.createElement("div");
  outputs.className = "routing-outputs";

  card.append(header, outputs);

  muteBtn.onclick = async () => {
    const s = card._item;
    if (!s) return;
    clearPrimaryUnchecked(appIdFor(s));
    await invoke("set_session_mute", {
      endpointId: s.endpoint_id,
      pid: s.pid,
      appKey: s.app_key || null,
      muted: !s.muted,
    });
    await onRefresh(true);
  };

  return card;
}

function syncSessionOutputs(
  body,
  s,
  renderEndpoints,
  loopbacks,
  includeDisconnected,
  peakPidAttr,
  onRefresh
) {
  const activeLoopbacks = loopbacksForSession(loopbacks, s);
  const visibleEndpoints = renderEndpoints.filter(
    (ep) =>
      (ep.is_allowed && (includeDisconnected || ep.is_connected)) ||
      ep.id === s.endpoint_id ||
      activeLoopbacks.some((lb) => lb.target_endpoint_id === ep.id)
  );

  syncKeyedList(
    body,
    visibleEndpoints,
    (ep) => ep.id,
    "No active output devices.",
    () => buildRoutingRow(onRefresh),
    (row, ep) => {
      const offline = ep.is_connected === false;
      updateRoutingRow(row, {
        session: s,
        endpoint: ep,
        activeLoopbacks,
        peakPidAttr,
        labelText: offline ? `${ep.friendly_name} (Offline)` : ep.friendly_name,
        labelTitle: ep.friendly_name,
      });
    }
  );
  return activeLoopbacks;
}

function renderAppsList(
  container,
  sessions,
  renderEndpoints,
  loopbacks,
  includeDisconnected,
  appIconCache,
  onRefresh
) {
  const groups = groupSessionsByApp(sessions);
  syncKeyedList(
    container,
    groups,
    (g) => `app:${g.key}`,
    "No audio sessions.",
    () => buildAppCard(onRefresh),
    (card, group) => {
      const s = group.primary;
      const appId = appIdFor(s);
      card._group = group;
      card._item = s;
      const allMuted = group.sessions.every((x) => x.muted);
      card.classList.toggle("inactive-card", !s.is_active);
      card.classList.toggle("app-muted", allMuted);

      const name = s.display_name || s.root_exe_name || s.exe_name || `PID ${s.pid}`;
      const iconEl = card.querySelector(".app-header-row .app-icon");
      const iconUrl = s.app_key && appIconCache ? appIconCache.get(s.app_key) : null;
      applyAppIcon(iconEl, iconUrl, true);
      const titleEl = card.querySelector(".card-title");
      titleEl.textContent = name;
      titleEl.title = name;

      const stateBadgeEl = card.querySelector(".strip-state-badge");
      const badge = stateBadgeFor(s);
      if (stateBadgeEl) {
        if (badge) {
          stateBadgeEl.textContent = badge.text;
          stateBadgeEl.title = badge.title;
          stateBadgeEl.className = `badge-tag strip-state-badge strip-state-${badge.variant}`;
        } else {
          stateBadgeEl.textContent = "";
          stateBadgeEl.title = "";
          stateBadgeEl.className = "badge-tag strip-state-badge collapsed";
        }
      }

      const pidBadge = card.querySelector(".pid-badge");
      const multiPid = group.sessions.length > 1;
      const showSubs = multiPid && expandedSubPidGroups.has(group.key);
      pidBadge.classList.toggle("badge-tag-btn", multiPid);
      if (multiPid) {
        pidBadge.classList.remove("collapsed");
        pidBadge.textContent = `${group.sessions.length} PIDs ${showSubs ? "▾" : "▸"}`;
        pidBadge.title = "Click to expand individual PIDs";
      } else if (s.pid > 0) {
        pidBadge.classList.remove("collapsed");
        pidBadge.textContent = `PID ${s.pid}`;
        pidBadge.title = "";
      } else {
        pidBadge.classList.add("collapsed");
        pidBadge.textContent = "";
        pidBadge.title = "";
      }

      const pinBtn = card.querySelector(".btn-pin");
      pinBtn.classList.toggle("pinned", Boolean(s.is_pinned));
      pinBtn.title = s.is_pinned
        ? "Pinned (saved across restarts) — click to unpin"
        : "Pin app so it stays saved across restarts";

      const muteBtn = card.querySelector(".btn-icon-mute");
      muteBtn.classList.toggle("muted", allMuted);
      muteBtn.textContent = allMuted ? "🔇" : "🔉";
      muteBtn.title = allMuted ? "Unmute" : "Mute";

      if (!s.muted) {
        clearPrimaryUnchecked(appId);
      }

      const expanded = s.is_active
        ? !collapsedActiveIds.has(group.key)
        : expandedInactiveIds.has(group.key);
      card.querySelector(".chevron").textContent = expanded ? "▾" : "▸";
      const body = card.querySelector(".routing-outputs");
      body.classList.toggle("collapsed", !expanded);

      const peakPidAttr = group.sessions
        .filter((x) => x.pid > 0)
        .map((x) => String(x.pid))
        .join(",");
      const activeLoopbacks = syncSessionOutputs(
        body,
        s,
        renderEndpoints,
        loopbacks,
        includeDisconnected,
        peakPidAttr,
        onRefresh
      );

      const primaryChecked = !isPrimaryUnchecked(appId);
      const activeCount = (primaryChecked ? 1 : 0) + activeLoopbacks.length;
      card.querySelector(".badge-muted").textContent = allMuted
        ? "Muted"
        : `${activeCount} out(s)`;

      const subPidsList = card.querySelector(".sub-pids-list");
      subPidsList.classList.toggle("collapsed", !expanded || !showSubs);
      if (multiPid && expanded && showSubs) {
        syncKeyedList(
          subPidsList,
          group.sessions,
          (sub) => `sub:${sub.pid}`,
          "",
          () => buildSubPidCard(onRefresh),
          (subCard, sub) => {
            subCard._item = sub;
            subCard.querySelector(".badge-tag").textContent = `PID ${sub.pid}`;
            const subMute = subCard.querySelector(".btn-icon-mute");
            subMute.classList.toggle("muted", sub.muted);
            subMute.textContent = sub.muted ? "🔇" : "🔉";
            subMute.title = sub.muted ? "Unmute" : "Mute";
            syncSessionOutputs(
              subCard.querySelector(".routing-outputs"),
              sub,
              renderEndpoints,
              loopbacks,
              includeDisconnected,
              String(sub.pid),
              onRefresh
            );
          }
        );
      }
    }
  );
}

export function renderAppsTab(
  sessions,
  endpoints,
  loopbacks,
  includeDisconnected,
  appIconCache,
  onRefresh
) {
  const renderEndpoints = sortEndpointsByRoleAndName(
    endpoints.filter((e) => e.flow === "Render")
  );
  const byName = (a, b) =>
    appDisplayName(a).localeCompare(appDisplayName(b), undefined, { sensitivity: "base" });
  const byPinnedThenName = (a, b) => {
    const pinDiff = Number(Boolean(b.is_pinned)) - Number(Boolean(a.is_pinned));
    return pinDiff !== 0 ? pinDiff : byName(a, b);
  };

  const playing = sessions.filter((s) => s.is_active);
  const idle = sessions.filter((s) => !s.is_active && s.pid > 0).sort(byName);
  const offline = sessions.filter((s) => !s.is_active && s.pid === 0).sort(byPinnedThenName);

  const idleCountEl = document.getElementById("filter-apps-idle-count");
  if (idleCountEl) idleCountEl.textContent = String(idle.length);
  const offlineCountEl = document.getElementById("filter-apps-offline-count");
  if (offlineCountEl) offlineCountEl.textContent = String(offline.length);

  const visibleSessions = [
    ...playing,
    ...(showIdleApps ? idle : []),
    ...(showOfflineApps ? offline : []),
  ];

  renderAppsList(
    document.getElementById("apps-active-list"),
    visibleSessions,
    renderEndpoints,
    loopbacks,
    includeDisconnected,
    appIconCache,
    onRefresh
  );
}
