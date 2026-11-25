import { isUserInteracting, setupTabs } from "./dom.js";
import {
  hasActiveAppsFilters,
  renderAppsTab,
  setupAppsTabControls,
} from "./apps_tab.js";
import {
  hasActiveDevicesFilters,
  renderDevicesTab,
  setupDevicesTabControls,
} from "./devices_tab.js";
import { renderSystemTab } from "./system_tab.js";
import { renderPresets, setupPresetsBar } from "./presets_bar.js";
import { renderVisibilityTab, setupVisibilityControls } from "./visibility_tab.js";
import { setupAppSettingsTab } from "./settings_tab.js";

const { invoke } = window.__TAURI__.core;

let includeDisconnected = false;
let currentTab = "apps";
let appSearchQuery = "";
const appIconCache = new Map();
const iconFetchInFlight = new Set();

function updateFilterUi() {
  const fabWrap = document.getElementById("filter-fab-wrap");
  const popover = document.getElementById("filter-popover");
  const fabBtn = document.getElementById("filter-fab-btn");
  const dot = document.getElementById("filter-fab-dot");
  if (!fabWrap || !popover || !fabBtn) return;

  const showFab = currentTab === "apps" || currentTab === "devices" || currentTab === "system";
  fabWrap.classList.toggle("collapsed", !showFab);
  if (!showFab) {
    popover.classList.add("collapsed");
    fabBtn.setAttribute("aria-expanded", "false");
  }

  for (const el of popover.querySelectorAll("[data-filter-tabs]")) {
    const tabs = el.dataset.filterTabs.split(" ");
    el.classList.toggle("collapsed", !tabs.includes(currentTab));
  }

  const discCb = document.getElementById("filter-disconnected-cb");
  if (discCb) discCb.checked = includeDisconnected;

  const hasSearch =
    Boolean(appSearchQuery.trim()) && (currentTab === "apps" || currentTab === "devices");
  let anyActive = includeDisconnected || hasSearch;
  if (currentTab === "apps") anyActive = anyActive || hasActiveAppsFilters();
  if (currentTab === "devices") anyActive = anyActive || hasActiveDevicesFilters();
  if (dot) dot.classList.toggle("collapsed", !anyActive);
  fabBtn.classList.toggle("has-active", anyActive);
  fabBtn.classList.toggle("has-search", hasSearch);
  fabBtn.title = hasSearch
    ? `Filters active (search: "${appSearchQuery.trim()}")`
    : anyActive
      ? "Filters active"
      : "Filters";
}

function setupFilterMenu() {
  const fabWrap = document.getElementById("filter-fab-wrap");
  const fabBtn = document.getElementById("filter-fab-btn");
  const popover = document.getElementById("filter-popover");
  const discCb = document.getElementById("filter-disconnected-cb");
  const searchInput = document.getElementById("filter-app-search");
  const searchClear = document.getElementById("filter-app-search-clear");

  fabBtn.onclick = (e) => {
    e.stopPropagation();
    const opening = popover.classList.contains("collapsed");
    popover.classList.toggle("collapsed", !opening);
    fabBtn.setAttribute("aria-expanded", String(opening));
  };

  document.addEventListener("click", (e) => {
    if (!fabWrap.contains(e.target) && !popover.classList.contains("collapsed")) {
      popover.classList.add("collapsed");
      fabBtn.setAttribute("aria-expanded", "false");
    }
  });

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !popover.classList.contains("collapsed")) {
      popover.classList.add("collapsed");
      fabBtn.setAttribute("aria-expanded", "false");
    }
  });

  if (discCb) {
    discCb.onchange = async () => {
      await invoke("set_include_disconnected", { include: discCb.checked });
      await refreshAll(true);
    };
  }

  if (searchInput) {
    searchInput.oninput = () => {
      appSearchQuery = searchInput.value;
      if (searchClear) searchClear.classList.toggle("collapsed", !appSearchQuery);
      updateFilterUi();
      refreshAll(true);
    };
  }

  if (searchClear && searchInput) {
    searchClear.onclick = (e) => {
      e.stopPropagation();
      searchInput.value = "";
      appSearchQuery = "";
      searchClear.classList.add("collapsed");
      updateFilterUi();
      searchInput.focus();
      refreshAll(true);
    };
  }
}

function ensureAppIcons(sessions) {
  const missing = [];
  for (const s of sessions) {
    const key = s.app_key;
    if (key && !appIconCache.has(key) && !iconFetchInFlight.has(key)) {
      iconFetchInFlight.add(key);
      missing.push(key);
    }
  }
  if (missing.length === 0) return;
  invoke("get_app_icons", { paths: missing })
    .then((icons) => {
      let updated = false;
      for (const path of missing) {
        iconFetchInFlight.delete(path);
        const url = icons && icons[path] ? icons[path] : null;
        appIconCache.set(path, url);
        if (url) updated = true;
      }
      if (updated) refreshAll(false);
    })
    .catch((err) => {
      for (const path of missing) iconFetchInFlight.delete(path);
      console.error("get_app_icons error:", err);
    });
}

async function refreshAll(force = false) {
  if (!force && isUserInteracting()) return;
  const badge = document.getElementById("daemon-badge");
  try {
    const [status, presetData] = await Promise.all([
      invoke("get_status"),
      invoke("list_presets"),
    ]);
    badge.className = "badge badge-ok collapsed";

    includeDisconnected = Boolean(status.include_disconnected);
    updateFilterUi();

    const appSessions = status.sessions.filter((s) => s.pid !== 0 || Boolean(s.app_key));
    const visibleIO = status.endpoints.filter(
      (e) => e.is_allowed && (includeDisconnected || e.is_connected)
    );

    const visibleRender = visibleIO.filter((e) => e.flow === "Render");

    renderPresets(presetData, refreshAll);
    renderAppsTab(
      appSessions,
      status.endpoints,
      status.loopbacks || [],
      includeDisconnected,
      appIconCache,
      refreshAll,
      appSearchQuery
    );
    renderDevicesTab(
      status.endpoints,
      appSessions,
      status.loopbacks || [],
      includeDisconnected,
      appIconCache,
      refreshAll,
      appSearchQuery
    );
    ensureAppIcons(appSessions);
    renderSystemTab(status.endpoints, includeDisconnected, refreshAll);
    renderVisibilityTab(status.endpoints, refreshAll);

    document.getElementById("count-apps").textContent = String(appSessions.length);
    document.getElementById("count-devices").textContent = String(visibleRender.length);
    document.getElementById("count-system").textContent = String(visibleIO.length);
  } catch (err) {
    badge.textContent = "⚠️";
    badge.title = "Daemon Offline";
    badge.className = "badge badge-offline";
    console.error("IPC error:", err);
  }
}

document.addEventListener("contextmenu", (e) => {
  e.preventDefault();
});

setupTabs((tab) => {
  currentTab = tab;
  updateFilterUi();
});
setupFilterMenu();
setupAppsTabControls(refreshAll, updateFilterUi);
setupDevicesTabControls(refreshAll, updateFilterUi);
setupPresetsBar(refreshAll);
setupVisibilityControls();
setupAppSettingsTab();
document.getElementById("refresh-btn").onclick = () => refreshAll(true);
function scalePeak(v) {
  if (!v || v <= 0.001) return 0;
  return Math.round(Math.pow(Math.max(0, Math.min(1, v)), 0.45) * 100);
}

let peaksInFlight = false;
async function pollPeaks() {
  if (document.hidden || peaksInFlight) return;
  peaksInFlight = true;
  try {
    const peaks = await invoke("get_peaks");
    for (const el of document.querySelectorAll("[data-peak-endpoint]")) {
      const id = el.dataset.peakEndpoint;
      const v = id && peaks.endpoints ? peaks.endpoints[id] ?? 0 : 0;
      const scale = Number(el.dataset.peakScale ?? 1);
      el.style.width = `${Math.round(scalePeak(v) * scale)}%`;
    }
    for (const el of document.querySelectorAll("[data-peak-pid]")) {
      const pidAttr = el.dataset.peakPid;
      let v = 0;
      if (pidAttr && peaks.sessions) {
        for (const pid of pidAttr.split(",")) {
          const cur = peaks.sessions[pid] ?? 0;
          if (cur > v) v = cur;
        }
      }
      const scale = Number(el.dataset.peakScale ?? 1);
      el.style.width = `${Math.round(scalePeak(v) * scale)}%`;
    }
  } catch (err) {
    console.error("get_peaks error:", err);
  } finally {
    peaksInFlight = false;
  }
}

refreshAll(true);
setInterval(() => refreshAll(false), 2000);
setInterval(pollPeaks, 75);
