import { isUserInteracting, setupTabs } from "./dom.js";
import { renderAppsTab, setupAppsTabControls } from "./apps_tab.js";
import { renderSystemTab } from "./system_tab.js";
import { renderPresets, setupPresetsBar } from "./presets_bar.js";
import { renderVisibilityTab, setupVisibilityControls } from "./visibility_tab.js";

const { invoke } = window.__TAURI__.core;

let includeDisconnected = false;

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
    const discBtn = document.getElementById("disconnected-btn");
    discBtn.classList.toggle("active", includeDisconnected);

    const appSessions = status.sessions.filter((s) => s.pid !== 0 || Boolean(s.app_key));
    const visibleIO = status.endpoints.filter(
      (e) => e.is_allowed && (includeDisconnected || e.is_connected)
    );

    renderPresets(presetData, refreshAll);
    renderAppsTab(
      appSessions,
      status.endpoints,
      status.loopbacks || [],
      includeDisconnected,
      refreshAll
    );
    renderSystemTab(status.endpoints, includeDisconnected, refreshAll);
    renderVisibilityTab(status.endpoints, refreshAll);

    document.getElementById("count-apps").textContent = String(appSessions.length);
    document.getElementById("count-system").textContent = String(visibleIO.length);
  } catch (err) {
    badge.textContent = "⚠️";
    badge.title = "Daemon Offline";
    badge.className = "badge badge-offline";
    console.error("IPC error:", err);
  }
}

setupTabs();
setupAppsTabControls(refreshAll);
setupPresetsBar(refreshAll);
setupVisibilityControls();
document.getElementById("disconnected-btn").onclick = async () => {
  await invoke("set_include_disconnected", { include: !includeDisconnected });
  await refreshAll(true);
};
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
