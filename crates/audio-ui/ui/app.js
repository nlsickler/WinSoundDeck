import { isUserInteracting, setupTabs } from "./dom.js";
import { renderAppsTab, setupAppsTabControls } from "./apps_tab.js";
import { renderIOTab } from "./io_tab.js";
import { renderPresets, setupPresetsBar } from "./presets_bar.js";
import { renderSettingsTab, setupSettingsControls } from "./settings_tab.js";

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
    badge.textContent = "Daemon Connected";
    badge.className = "badge badge-ok";

    includeDisconnected = Boolean(status.include_disconnected);
    const discBtn = document.getElementById("disconnected-btn");
    discBtn.classList.toggle("active", includeDisconnected);

    const appSessions = status.sessions.filter((s) => s.pid !== 0);
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
    renderIOTab(status.endpoints, includeDisconnected, refreshAll);
    renderSettingsTab(status.endpoints, refreshAll);

    document.getElementById("count-apps").textContent = String(appSessions.length);
    document.getElementById("count-io").textContent = String(visibleIO.length);
  } catch (err) {
    badge.textContent = "Daemon Offline";
    badge.className = "badge badge-offline";
    console.error("IPC error:", err);
  }
}

setupTabs();
setupAppsTabControls(refreshAll);
setupPresetsBar(refreshAll);
setupSettingsControls();
document.getElementById("disconnected-btn").onclick = async () => {
  await invoke("set_include_disconnected", { include: !includeDisconnected });
  await refreshAll(true);
};
document.getElementById("refresh-btn").onclick = () => refreshAll(true);
refreshAll(true);
setInterval(() => refreshAll(false), 2000);
