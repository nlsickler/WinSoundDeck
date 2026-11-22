import { syncKeyedList, syncMuteIconButton } from "./dom.js";
import {
  appDisplayName,
  appIdFor,
  buildRoutingRow,
  groupSessionsByApp,
  isPrimaryUnchecked,
  loopbacksForSession,
  updateRoutingRow,
} from "./routing_row.js";

const { invoke } = window.__TAURI__.core;

const collapsedDeviceIds = new Set();
const expandedInactiveDeviceIds = new Set();

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

  const activeList = document.createElement("div");
  activeList.className = "routing-outputs-inner";

  const inactiveSection = document.createElement("div");
  inactiveSection.className = "device-inactive-section";

  const inactiveToggle = document.createElement("div");
  inactiveToggle.className = "device-inactive-toggle";
  const inactiveChevron = document.createElement("span");
  inactiveChevron.className = "chevron";
  const inactiveLabel = document.createElement("span");
  inactiveToggle.append(inactiveChevron, inactiveLabel);

  const inactiveList = document.createElement("div");
  inactiveList.className = "routing-outputs-inner collapsed";

  inactiveSection.append(inactiveToggle, inactiveList);
  body.append(activeList, inactiveSection);
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

  inactiveToggle.onclick = () => {
    const ep = card._ep;
    if (!ep) return;
    if (expandedInactiveDeviceIds.has(ep.id)) {
      expandedInactiveDeviceIds.delete(ep.id);
    } else {
      expandedInactiveDeviceIds.add(ep.id);
    }
    const showInactive = expandedInactiveDeviceIds.has(ep.id);
    inactiveChevron.textContent = showInactive ? "▾" : "▸";
    inactiveList.classList.toggle("collapsed", !showInactive);
  };

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

function syncDeviceApps(
  container,
  groups,
  ep,
  loopbacks,
  emptyText,
  isInactiveList,
  appIconCache,
  onRefresh
) {
  syncKeyedList(
    container,
    groups,
    (g) => `devapp:${g.key}`,
    emptyText,
    () => buildRoutingRow(onRefresh),
    (row, group) => {
      const s = group.primary;
      const activeLoopbacks = loopbacksForSession(loopbacks, s);
      const peakPidAttr = group.sessions
        .filter((x) => x.pid > 0)
        .map((x) => String(x.pid))
        .join(",");
      const baseName = appDisplayName(s);
      const statusSuffix = isInactiveList ? (s.pid > 0 ? " (Idle)" : " (Offline)") : "";
      const iconUrl = s.app_key && appIconCache ? appIconCache.get(s.app_key) || null : null;
      updateRoutingRow(row, {
        session: s,
        endpoint: ep,
        activeLoopbacks,
        peakPidAttr,
        labelText: `${baseName}${statusSuffix}`,
        labelTitle: baseName,
        iconUrl,
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
  const renderEndpoints = endpoints.filter(
    (e) => e.flow === "Render" && e.is_allowed && (includeDisconnected || e.is_connected)
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
  const inactiveGroups = allGroups
    .filter((g) => !g.primary.is_active)
    .sort(byPinnedThenName);

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

      let routedPlayingCount = 0;
      for (const g of playingGroups) {
        const s = g.primary;
        const activeLoopbacks = loopbacksForSession(loopbacks, s);
        const isPrimary = s.endpoint_id === ep.id && !isPrimaryUnchecked(appIdFor(s));
        const isLoopback = activeLoopbacks.some((lb) => lb.target_endpoint_id === ep.id);
        if (isPrimary || isLoopback) routedPlayingCount += 1;
      }
      card.querySelector(".badge-muted").textContent =
        routedPlayingCount === 1 ? "1 app" : `${routedPlayingCount} apps`;

      const [activeList, inactiveSection] = body.children;
      syncDeviceApps(
        activeList,
        playingGroups,
        ep,
        loopbacks,
        "No playing applications.",
        false,
        appIconCache,
        onRefresh
      );

      inactiveSection.style.display = inactiveGroups.length > 0 ? "" : "none";
      const showInactive = expandedInactiveDeviceIds.has(ep.id);
      const inactiveToggle = inactiveSection.querySelector(".device-inactive-toggle");
      inactiveToggle.querySelector(".chevron").textContent = showInactive ? "▾" : "▸";
      inactiveToggle.lastElementChild.textContent = `Idle & Offline (${inactiveGroups.length})`;
      const inactiveList = inactiveSection.querySelector(".routing-outputs-inner");
      inactiveList.classList.toggle("collapsed", !showInactive);
      syncDeviceApps(
        inactiveList,
        inactiveGroups,
        ep,
        loopbacks,
        "",
        true,
        appIconCache,
        onRefresh
      );
    }
  );
}
