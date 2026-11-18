import { isSliderDragging, pct, setSliderDragging, syncKeyedList } from "./dom.js";

const { invoke } = window.__TAURI__.core;

const collapsedActiveIds = new Set();
const expandedInactiveIds = new Set();
const explicitlyUncheckedIds = new Set();
const rememberedOutputVolumes = new Map();
let showIdleApps = false;
let showOfflineApps = false;

function appIdFor(s) {
  return s.app_key || String(s.pid);
}

function appDisplayName(s) {
  return s.display_name || s.root_exe_name || s.exe_name || "";
}

export function setupAppsTabControls(onRefresh) {
  document.getElementById("apps-idle-toggle").onclick = () => {
    showIdleApps = !showIdleApps;
    document.getElementById("apps-idle-chevron").textContent = showIdleApps ? "▾" : "▸";
    document.getElementById("apps-idle-list").classList.toggle("collapsed", !showIdleApps);
  };
  document.getElementById("apps-offline-toggle").onclick = () => {
    showOfflineApps = !showOfflineApps;
    document.getElementById("apps-offline-chevron").textContent = showOfflineApps ? "▾" : "▸";
    document.getElementById("apps-offline-list").classList.toggle("collapsed", !showOfflineApps);
  };
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
  titleToggle.style.maxWidth = "none";
  const chevron = document.createElement("span");
  chevron.className = "chevron";
  const title = document.createElement("span");
  title.className = "card-title";
  titleToggle.append(chevron, title);

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
  pidBadge.className = "badge-tag";
  meta.append(outCountBadge, pidBadge);

  header.append(titleToggle, pinBtn, muteBtn, meta);

  const body = document.createElement("div");
  body.className = "routing-outputs";

  card.append(header, body);

  const toggleExpand = () => {
    const s = card._item;
    if (!s) return;
    const id = appIdFor(s);
    if (s.is_active) {
      if (collapsedActiveIds.has(id)) {
        collapsedActiveIds.delete(id);
      } else {
        collapsedActiveIds.add(id);
      }
    } else if (expandedInactiveIds.has(id)) {
      expandedInactiveIds.delete(id);
    } else {
      expandedInactiveIds.add(id);
    }
    const expanded = s.is_active
      ? !collapsedActiveIds.has(id)
      : expandedInactiveIds.has(id);
    chevron.textContent = expanded ? "▾" : "▸";
    body.classList.toggle("collapsed", !expanded);
  };
  titleToggle.onclick = toggleExpand;
  meta.onclick = toggleExpand;
  meta.style.cursor = "pointer";

  pinBtn.onclick = async () => {
    const s = card._item;
    if (!s) return;
    await invoke("set_app_pinned", {
      pid: s.pid,
      appKey: s.app_key || null,
      pinned: !s.is_pinned,
    });
    await onRefresh(true);
  };

  muteBtn.onclick = async () => {
    const s = card._item;
    explicitlyUncheckedIds.delete(appIdFor(s));
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

function buildRoutingOutputRow(onRefresh) {
  const row = document.createElement("div");
  row.className = "routing-output-row";

  const cb = document.createElement("input");
  cb.type = "checkbox";

  const name = document.createElement("span");
  name.className = "routing-output-name";

  const primaryTag = document.createElement("span");
  primaryTag.className = "badge-tag primary-slot";
  primaryTag.textContent = "P";
  primaryTag.title = "Primary Output";

  const sliderWrap = document.createElement("div");
  sliderWrap.className = "slider-meter-wrap";
  const meterFill = document.createElement("div");
  meterFill.className = "slider-peak-fill";
  const slider = document.createElement("input");
  slider.type = "range";
  slider.min = "0";
  slider.max = "100";
  sliderWrap.append(meterFill, slider);

  const label = document.createElement("span");
  label.className = "vol-label";

  row.append(cb, name, primaryTag, sliderWrap, label);

  cb.onchange = async () => {
    const ctx = row._ctx;
    if (!ctx) return;
    const { session, endpoint, isPrimary, isLoopback, activeLoopbacks } = ctx;
    const appId = appIdFor(session);
    const appKey = session.app_key || null;
    if (isPrimary) {
      if (cb.checked) {
        explicitlyUncheckedIds.delete(appId);
        await invoke("set_session_mute", {
          endpointId: session.endpoint_id,
          pid: session.pid,
          appKey,
          muted: false,
        });
      } else {
        const fallback = activeLoopbacks.find((lb) => lb.target_endpoint_id !== endpoint.id);
        if (fallback) {
          explicitlyUncheckedIds.delete(appId);
          const fallbackVol = fallback.volume;
          await invoke("route_app", {
            pid: session.pid,
            appKey,
            flow: "Render",
            endpointId: fallback.target_endpoint_id,
          });
          await new Promise((r) => setTimeout(r, 250));
          await invoke("stop_loopback", {
            pid: session.pid,
            appKey,
            targetEndpointId: fallback.target_endpoint_id,
          });
          await invoke("set_app_output_volume", {
            pid: session.pid,
            appKey,
            endpointId: fallback.target_endpoint_id,
            volume: fallbackVol,
            finalize: true,
          });
        } else {
          explicitlyUncheckedIds.add(appId);
          await invoke("set_session_mute", {
            endpointId: session.endpoint_id,
            pid: session.pid,
            appKey,
            muted: true,
          });
        }
      }
    } else if (cb.checked && !isLoopback) {
      const remembered =
        (session.saved_volumes && session.saved_volumes[endpoint.id]) ??
        rememberedOutputVolumes.get(`${appId}:${endpoint.id}`) ??
        0.5;
      if (explicitlyUncheckedIds.has(appId) && activeLoopbacks.length === 0) {
        explicitlyUncheckedIds.delete(appId);
        await invoke("route_app", {
          pid: session.pid,
          appKey,
          flow: "Render",
          endpointId: endpoint.id,
        });
        await new Promise((r) => setTimeout(r, 250));
        await invoke("set_session_mute", {
          endpointId: endpoint.id,
          pid: session.pid,
          appKey,
          muted: false,
        });
        await invoke("set_app_output_volume", {
          pid: session.pid,
          appKey,
          endpointId: endpoint.id,
          volume: remembered,
          finalize: true,
        });
      } else {
        await invoke("start_loopback", {
          pid: session.pid,
          appKey,
          targetEndpointId: endpoint.id,
          volume: remembered,
        });
        await invoke("set_app_output_volume", {
          pid: session.pid,
          appKey,
          endpointId: endpoint.id,
          volume: remembered,
          finalize: true,
        });
      }
    } else if (!cb.checked && isLoopback) {
      await invoke("stop_loopback", {
        pid: session.pid,
        appKey,
        targetEndpointId: endpoint.id,
      });
    }
    await onRefresh(true);
  };

  slider.onpointerdown = () => setSliderDragging(true);
  slider.onpointercancel = () => setSliderDragging(false);
  slider.oninput = async (e) => {
    const ctx = row._ctx;
    if (!ctx) return;
    const { session, endpoint } = ctx;
    const v = Number(e.target.value) / 100;
    rememberedOutputVolumes.set(`${appIdFor(session)}:${endpoint.id}`, v);
    label.textContent = `${e.target.value}%`;
    await invoke("set_app_output_volume", {
      pid: session.pid,
      appKey: session.app_key || null,
      endpointId: endpoint.id,
      volume: v,
      finalize: false,
    });
  };
  slider.onchange = async (e) => {
    setSliderDragging(false);
    const ctx = row._ctx;
    if (!ctx) return;
    const { session, endpoint } = ctx;
    const v = Number(e.target.value) / 100;
    rememberedOutputVolumes.set(`${appIdFor(session)}:${endpoint.id}`, v);
    await invoke("set_app_output_volume", {
      pid: session.pid,
      appKey: session.app_key || null,
      endpointId: endpoint.id,
      volume: v,
      finalize: true,
    });
    await onRefresh(true);
  };

  return row;
}

function renderAppsList(
  container,
  sessions,
  renderEndpoints,
  loopbacks,
  includeDisconnected,
  onRefresh
) {
  syncKeyedList(
    container,
    sessions,
    (s) => `app:${appIdFor(s)}`,
    "No audio sessions.",
    () => buildAppCard(onRefresh),
    (card, s) => {
      const appId = appIdFor(s);
      card._item = s;
      card.classList.toggle("inactive-card", !s.is_active);
      card.classList.toggle("app-muted", Boolean(s.muted));
      const name = s.display_name || s.root_exe_name || s.exe_name || `PID ${s.pid}`;
      const titleEl = card.querySelector(".card-title");
      titleEl.textContent = name;
      titleEl.title = name;
      card.querySelector(".app-header-row > .card-meta > .badge-tag").textContent =
        s.pid > 0 ? `PID ${s.pid}` : "Not running";

      const pinBtn = card.querySelector(".btn-pin");
      pinBtn.classList.toggle("pinned", Boolean(s.is_pinned));
      pinBtn.title = s.is_pinned
        ? "Pinned (saved across restarts) — click to unpin"
        : "Pin app so it stays saved across restarts";

      const muteBtn = card.querySelector(".btn-icon-mute");
      muteBtn.classList.toggle("muted", s.muted);
      muteBtn.textContent = s.muted ? "🔇" : "🔉";
      muteBtn.title = s.muted ? "Unmute" : "Mute";

      if (!s.muted) {
        explicitlyUncheckedIds.delete(appId);
      }
      const activeLoopbacks = loopbacks.filter((lb) =>
        s.pid > 0 ? lb.pid === s.pid : s.app_key && lb.app_key === s.app_key
      );
      const primaryChecked = !explicitlyUncheckedIds.has(appId);
      const activeCount = (primaryChecked ? 1 : 0) + activeLoopbacks.length;
      card.querySelector(".badge-muted").textContent = s.muted
        ? "Muted"
        : activeCount === 1
          ? "1 out"
          : `${activeCount} outs`;

      const expanded = s.is_active
        ? !collapsedActiveIds.has(appId)
        : expandedInactiveIds.has(appId);
      card.querySelector(".chevron").textContent = expanded ? "▾" : "▸";
      const body = card.querySelector(".routing-outputs");
      body.classList.toggle("collapsed", !expanded);

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
        () => buildRoutingOutputRow(onRefresh),
        (row, ep) => {
          const isPrimary = ep.id === s.endpoint_id;
          const lb = activeLoopbacks.find((x) => x.target_endpoint_id === ep.id);
          const isLoopback = Boolean(lb);
          const enabled = (isPrimary && !explicitlyUncheckedIds.has(appId)) || isLoopback;

          row._ctx = { session: s, endpoint: ep, isPrimary, isLoopback, activeLoopbacks };
          row.classList.toggle("disabled", !enabled);

          const cb = row.querySelector('input[type="checkbox"]');
          cb.checked = enabled;

          const nameEl = row.querySelector(".routing-output-name");
          const offline = ep.is_connected === false;
          nameEl.textContent = offline ? `${ep.friendly_name} (Offline)` : ep.friendly_name;
          nameEl.title = ep.friendly_name;

          const primaryTag = row.querySelector(".primary-slot");
          primaryTag.style.visibility = isPrimary ? "visible" : "hidden";

          const slider = row.querySelector('input[type="range"]');
          const label = row.querySelector(".vol-label");
          slider.disabled = !enabled;
          const rowMeterFill = row.querySelector(".slider-peak-fill");
          if (rowMeterFill) {
            const activePid = s.is_active && enabled && s.pid > 0 ? String(s.pid) : "";
            rowMeterFill.dataset.peakPid = activePid;
            if (!activePid) rowMeterFill.style.width = "0%";
          }

          const savedVol =
            (s.saved_volumes && s.saved_volumes[ep.id]) ??
            rememberedOutputVolumes.get(`${appId}:${ep.id}`) ??
            0.5;
          const vol = isPrimary ? s.volume : isLoopback ? lb.volume : savedVol;
          rememberedOutputVolumes.set(`${appId}:${ep.id}`, vol);
          const pctVal = pct(vol);
          if (!isSliderDragging() && Number(slider.value) !== pctVal) {
            slider.value = String(pctVal);
          }
          label.textContent = enabled ? `${pctVal}%` : "--";
        }
      );
    }
  );
}

export function renderAppsTab(
  sessions,
  endpoints,
  loopbacks,
  includeDisconnected,
  onRefresh
) {
  const renderEndpoints = endpoints.filter((e) => e.flow === "Render");
  const byName = (a, b) =>
    appDisplayName(a).localeCompare(appDisplayName(b), undefined, { sensitivity: "base" });
  const byPinnedThenName = (a, b) => {
    const pinDiff = Number(Boolean(b.is_pinned)) - Number(Boolean(a.is_pinned));
    return pinDiff !== 0 ? pinDiff : byName(a, b);
  };

  const playing = sessions.filter((s) => s.is_active);
  const idle = sessions.filter((s) => !s.is_active && s.pid > 0).sort(byName);
  const offline = sessions.filter((s) => !s.is_active && s.pid === 0).sort(byPinnedThenName);

  renderAppsList(
    document.getElementById("apps-active-list"),
    playing,
    renderEndpoints,
    loopbacks,
    includeDisconnected,
    onRefresh
  );

  document.getElementById("apps-idle-count").textContent = String(idle.length);
  document.getElementById("apps-idle-group").style.display = idle.length > 0 ? "" : "none";
  renderAppsList(
    document.getElementById("apps-idle-list"),
    idle,
    renderEndpoints,
    loopbacks,
    includeDisconnected,
    onRefresh
  );

  document.getElementById("apps-offline-count").textContent = String(offline.length);
  document.getElementById("apps-offline-group").style.display =
    offline.length > 0 ? "" : "none";
  renderAppsList(
    document.getElementById("apps-offline-list"),
    offline,
    renderEndpoints,
    loopbacks,
    includeDisconnected,
    onRefresh
  );
}
