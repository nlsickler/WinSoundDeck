import {
  loadBoolPref,
  saveBoolPref,
  syncKeyedList,
  syncMuteIconButton,
} from "./dom.js";
import {
  appDisplayName,
  appIdFor,
  buildRoutingRow,
  groupSessionsByApp,
  isPrimaryUnchecked,
  loopbacksForSession,
  sortEndpointsByRoleAndName,
  stateBadgeFor,
  updateRoutingRow,
} from "./routing_row.js";

const { invoke } = window.__TAURI__.core;

const PREF_SHOW_IDLE = "winsounddeck.devices.show_idle";
const PREF_SHOW_OFFLINE = "winsounddeck.devices.show_offline";

const collapsedDeviceIds = new Set();
let showIdleInDevices = loadBoolPref(PREF_SHOW_IDLE, false);
let showOfflineInDevices = loadBoolPref(PREF_SHOW_OFFLINE, false);

export function hasActiveDevicesFilters() {
  return showIdleInDevices || showOfflineInDevices;
}

export function setupDevicesTabControls(onRefresh, onFilterChange) {
  const idleCb = document.getElementById("filter-devices-idle-cb");
  const offlineCb = document.getElementById("filter-devices-offline-cb");
  if (idleCb) {
    idleCb.checked = showIdleInDevices;
    idleCb.onchange = () => {
      showIdleInDevices = idleCb.checked;
      saveBoolPref(PREF_SHOW_IDLE, showIdleInDevices);
      if (onFilterChange) onFilterChange();
      if (onRefresh) onRefresh(true);
    };
  }
  if (offlineCb) {
    offlineCb.checked = showOfflineInDevices;
    offlineCb.onchange = () => {
      showOfflineInDevices = offlineCb.checked;
      saveBoolPref(PREF_SHOW_OFFLINE, showOfflineInDevices);
      if (onFilterChange) onFilterChange();
      if (onRefresh) onRefresh(true);
    };
  }
}

function buildDeviceCard(onRefresh) {
  const card = document.createElement("div");
  card.className = "card";

  const header = document.createElement("div");
  header.className = "app-header-row";

  const titleToggle = document.createElement("div");
  titleToggle.className = "app-title-toggle";
  titleToggle.style.flex = "1";
  titleToggle.style.maxWidth = "none";
  const chevron = document.createElement("span");
  chevron.className = "chevron";
  const title = document.createElement("span");
  title.className = "card-title";
  titleToggle.append(chevron, title);

  const meta = document.createElement("div");
  meta.className = "card-meta";
  const appCountBadge = document.createElement("span");
  appCountBadge.className = "badge-muted";
  const muteBtn = document.createElement("button");
  muteBtn.className = "btn-icon-mute";
  const defaultBtn = document.createElement("button");
  defaultBtn.className = "btn-role btn-role-default";
  defaultBtn.textContent = "Default";
  defaultBtn.title = "Set as OS Default device (Multimedia & Console)";
  const commsBtn = document.createElement("button");
  commsBtn.className = "btn-role btn-role-comms";
  commsBtn.textContent = "Comms";
  commsBtn.title = "Set as OS Communications Default device";
  meta.append(appCountBadge, muteBtn, defaultBtn, commsBtn);

  header.append(titleToggle, meta);

  const body = document.createElement("div");
  body.className = "routing-outputs";

  const faderGrid = document.createElement("div");
  faderGrid.className = "device-fader-grid";

  body.append(faderGrid);
  card.append(header, body);

  const toggleExpand = () => {
    const ep = card._ep;
    if (!ep) return;
    if (collapsedDeviceIds.has(ep.id)) {
      collapsedDeviceIds.delete(ep.id);
    } else {
      collapsedDeviceIds.add(ep.id);
    }
    const expanded = !collapsedDeviceIds.has(ep.id);
    chevron.textContent = expanded ? "▾" : "▸";
    body.classList.toggle("collapsed", !expanded);
  };

  titleToggle.onclick = toggleExpand;
  appCountBadge.onclick = toggleExpand;
  appCountBadge.style.cursor = "pointer";

  muteBtn.onclick = async () => {
    const ep = card._ep;
    if (!ep || ep.is_connected === false) return;
    await invoke("set_endpoint_mute", {
      flow: ep.flow,
      endpointId: ep.id,
      muted: !ep.muted,
    });
    await onRefresh(true);
  };

  defaultBtn.onclick = async () => {
    const ep = card._ep;
    if (!ep || ep.is_connected === false) return;
    if (!ep.is_default_multimedia) {
      await invoke("set_default_endpoint", {
        endpointId: ep.id,
        role: "Multimedia",
      });
      await onRefresh(true);
    }
  };

  commsBtn.onclick = async () => {
    const ep = card._ep;
    if (!ep || ep.is_connected === false) return;
    if (!ep.is_default_communications) {
      await invoke("set_default_endpoint", {
        endpointId: ep.id,
        role: "Communications",
      });
      await onRefresh(true);
    }
  };

  return card;
}

function syncDeviceApps(container, groups, ep, loopbacks, appIconCache, onRefresh) {
  syncKeyedList(
    container,
    groups,
    (g) => `devapp:${g.key}`,
    "No applications to display.",
    () => buildRoutingRow(onRefresh, { vertical: true }),
    (row, group) => {
      const s = group.primary;
      const activeLoopbacks = loopbacksForSession(loopbacks, s);
      const peakPidAttr = group.sessions
        .filter((x) => x.pid > 0)
        .map((x) => String(x.pid))
        .join(",");
      const baseName = appDisplayName(s);
      const iconUrl = s.app_key && appIconCache ? appIconCache.get(s.app_key) || null : null;
      updateRoutingRow(row, {
        session: s,
        endpoint: ep,
        activeLoopbacks,
        peakPidAttr,
        labelText: baseName,
        labelTitle: baseName,
        iconUrl,
        stateBadge: stateBadgeFor(s),
      });
    }
  );
}

export function renderDevicesTab(
  endpoints,
  sessions,
  loopbacks,
  includeDisconnected,
  appIconCache,
  onRefresh
) {
  const renderEndpoints = sortEndpointsByRoleAndName(
    endpoints.filter(
      (e) => e.flow === "Render" && e.is_allowed && (includeDisconnected || e.is_connected)
    )
  );

  const byName = (a, b) =>
    appDisplayName(a.primary).localeCompare(appDisplayName(b.primary), undefined, {
      sensitivity: "base",
    });
  const byPinnedThenName = (a, b) => {
    const pinDiff = Number(Boolean(b.primary.is_pinned)) - Number(Boolean(a.primary.is_pinned));
    return pinDiff !== 0 ? pinDiff : byName(a, b);
  };

  const allGroups = groupSessionsByApp(sessions);
  const playingGroups = allGroups.filter((g) => g.primary.is_active);
  const idleGroups = allGroups
    .filter((g) => !g.primary.is_active && g.primary.pid > 0)
    .sort(byName);
  const offlineGroups = allGroups
    .filter((g) => !g.primary.is_active && g.primary.pid === 0)
    .sort(byPinnedThenName);

  const idleCountEl = document.getElementById("filter-devices-idle-count");
  if (idleCountEl) idleCountEl.textContent = String(idleGroups.length);
  const offlineCountEl = document.getElementById("filter-devices-offline-count");
  if (offlineCountEl) offlineCountEl.textContent = String(offlineGroups.length);

  // Left-to-right order: Active -> Idle -> Offline
  const displayGroups = [
    ...playingGroups,
    ...(showIdleInDevices ? idleGroups : []),
    ...(showOfflineInDevices ? offlineGroups : []),
  ];

  syncKeyedList(
    document.getElementById("devices-list"),
    renderEndpoints,
    (ep) => `device:${ep.id}`,
    "No active output devices.",
    () => buildDeviceCard(onRefresh),
    (card, ep) => {
      card._ep = ep;
      const disconnected = ep.is_connected === false;
      card.classList.toggle("inactive-card", disconnected);

      const titleEl = card.querySelector(".card-title");
      titleEl.textContent = disconnected ? `${ep.friendly_name} (Offline)` : ep.friendly_name;
      titleEl.title = ep.friendly_name;

      const muteBtn = card.querySelector(".btn-icon-mute");
      syncMuteIconButton(muteBtn, ep.muted, disconnected, "🔊");

      const defaultBtn = card.querySelector(".btn-role-default");
      defaultBtn.classList.toggle("active", Boolean(ep.is_default_multimedia));
      defaultBtn.disabled = disconnected;

      const commsBtn = card.querySelector(".btn-role-comms");
      commsBtn.classList.toggle("active", Boolean(ep.is_default_communications));
      commsBtn.disabled = disconnected;

      const expanded = !collapsedDeviceIds.has(ep.id);
      card.querySelector(".app-header-row .chevron").textContent = expanded ? "▾" : "▸";
      const body = card.querySelector(".routing-outputs");
      body.classList.toggle("collapsed", !expanded);

      let routedCount = 0;
      for (const g of displayGroups) {
        const s = g.primary;
        const activeLoopbacks = loopbacksForSession(loopbacks, s);
        const isPrimary = s.endpoint_id === ep.id && !isPrimaryUnchecked(appIdFor(s));
        const isLoopback = activeLoopbacks.some((lb) => lb.target_endpoint_id === ep.id);
        if (isPrimary || isLoopback) routedCount += 1;
      }
      card.querySelector(".badge-muted").textContent =
        routedCount === 1 ? "1 app" : `${routedCount} apps`;

      const faderGrid = body.querySelector(".device-fader-grid");
      syncDeviceApps(faderGrid, displayGroups, ep, loopbacks, appIconCache, onRefresh);
    }
  );
}
