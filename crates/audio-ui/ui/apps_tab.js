import { isSliderDragging, pct, setSliderDragging, syncKeyedList } from "./dom.js";

const { invoke } = window.__TAURI__.core;

const collapsedActivePids = new Set();
const expandedInactivePids = new Set();
const explicitlyUncheckedPids = new Set();
const rememberedOutputVolumes = new Map();
let showInactiveApps = false;

export function setupAppsTabControls() {
  document.getElementById("apps-inactive-toggle").onclick = () => {
    showInactiveApps = !showInactiveApps;
    document.getElementById("apps-inactive-chevron").textContent = showInactiveApps ? "▾" : "▸";
    document.getElementById("apps-inactive-list").classList.toggle("collapsed", !showInactiveApps);
  };
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

  const muteBtn = document.createElement("button");
  muteBtn.className = "btn-icon-mute";

  const meta = document.createElement("div");
  meta.className = "card-meta";
  const outCountBadge = document.createElement("span");
  outCountBadge.className = "badge-muted";
  const pidBadge = document.createElement("span");
  pidBadge.className = "badge-tag";
  meta.append(outCountBadge, pidBadge);

  header.append(titleToggle, muteBtn, meta);

  const body = document.createElement("div");
  body.className = "routing-outputs";

  card.append(header, body);

  const toggleExpand = () => {
    const s = card._item;
    if (!s) return;
    if (s.is_active) {
      if (collapsedActivePids.has(s.pid)) {
        collapsedActivePids.delete(s.pid);
      } else {
        collapsedActivePids.add(s.pid);
      }
    } else if (expandedInactivePids.has(s.pid)) {
      expandedInactivePids.delete(s.pid);
    } else {
      expandedInactivePids.add(s.pid);
    }
    const expanded = s.is_active
      ? !collapsedActivePids.has(s.pid)
      : expandedInactivePids.has(s.pid);
    chevron.textContent = expanded ? "▾" : "▸";
    body.classList.toggle("collapsed", !expanded);
  };
  titleToggle.onclick = toggleExpand;
  meta.onclick = toggleExpand;
  meta.style.cursor = "pointer";

  muteBtn.onclick = async () => {
    const s = card._item;
    explicitlyUncheckedPids.delete(s.pid);
    await invoke("set_session_mute", {
      endpointId: s.endpoint_id,
      pid: s.pid,
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

  const slider = document.createElement("input");
  slider.type = "range";
  slider.min = "0";
  slider.max = "100";

  const label = document.createElement("span");
  label.className = "vol-label";

  row.append(cb, name, primaryTag, slider, label);

  cb.onchange = async () => {
    const ctx = row._ctx;
    if (!ctx) return;
    const { session, endpoint, isPrimary, isLoopback, activeLoopbacks } = ctx;
    if (isPrimary) {
      if (cb.checked) {
        explicitlyUncheckedPids.delete(session.pid);
        await invoke("set_session_mute", {
          endpointId: session.endpoint_id,
          pid: session.pid,
          muted: false,
        });
      } else {
        const fallback = activeLoopbacks.find((lb) => lb.target_endpoint_id !== endpoint.id);
        if (fallback) {
          explicitlyUncheckedPids.delete(session.pid);
          const fallbackVol = fallback.volume;
          await invoke("route_app", {
            pid: session.pid,
            flow: "Render",
            endpointId: fallback.target_endpoint_id,
          });
          await new Promise((r) => setTimeout(r, 250));
          await invoke("stop_loopback", {
            pid: session.pid,
            targetEndpointId: fallback.target_endpoint_id,
          });
          await invoke("set_app_output_volume", {
            pid: session.pid,
            endpointId: fallback.target_endpoint_id,
            volume: fallbackVol,
            finalize: true,
          });
        } else {
          explicitlyUncheckedPids.add(session.pid);
          await invoke("set_session_mute", {
            endpointId: session.endpoint_id,
            pid: session.pid,
            muted: true,
          });
        }
      }
    } else if (cb.checked && !isLoopback) {
      const remembered =
        (session.saved_volumes && session.saved_volumes[endpoint.id]) ??
        rememberedOutputVolumes.get(`${session.pid}:${endpoint.id}`) ??
        0.5;
      if (explicitlyUncheckedPids.has(session.pid) && activeLoopbacks.length === 0) {
        explicitlyUncheckedPids.delete(session.pid);
        await invoke("route_app", {
          pid: session.pid,
          flow: "Render",
          endpointId: endpoint.id,
        });
        await new Promise((r) => setTimeout(r, 250));
        await invoke("set_session_mute", {
          endpointId: endpoint.id,
          pid: session.pid,
          muted: false,
        });
        await invoke("set_app_output_volume", {
          pid: session.pid,
          endpointId: endpoint.id,
          volume: remembered,
          finalize: true,
        });
      } else {
        await invoke("start_loopback", {
          pid: session.pid,
          targetEndpointId: endpoint.id,
          volume: remembered,
        });
        await invoke("set_app_output_volume", {
          pid: session.pid,
          endpointId: endpoint.id,
          volume: remembered,
          finalize: true,
        });
      }
    } else if (!cb.checked && isLoopback) {
      await invoke("stop_loopback", {
        pid: session.pid,
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
    rememberedOutputVolumes.set(`${session.pid}:${endpoint.id}`, v);
    label.textContent = `${e.target.value}%`;
    await invoke("set_app_output_volume", {
      pid: session.pid,
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
    rememberedOutputVolumes.set(`${session.pid}:${endpoint.id}`, v);
    await invoke("set_app_output_volume", {
      pid: session.pid,
      endpointId: endpoint.id,
      volume: v,
      finalize: true,
    });
    await onRefresh(true);
  };

  return row;
}

function renderAppsList(container, sessions, renderEndpoints, loopbacks, onRefresh) {
  syncKeyedList(
    container,
    sessions,
    (s) => `app:${s.pid}`,
    "No audio sessions.",
    () => buildAppCard(onRefresh),
    (card, s) => {
      card._item = s;
      card.classList.toggle("inactive-card", !s.is_active);
      card.classList.toggle("app-muted", Boolean(s.muted));
      const name = s.display_name || s.root_exe_name || s.exe_name || `PID ${s.pid}`;
      const titleEl = card.querySelector(".card-title");
      titleEl.textContent = name;
      titleEl.title = name;
      card.querySelector(".app-header-row > .card-meta > .badge-tag").textContent = `PID ${s.pid}`;

      const muteBtn = card.querySelector(".btn-icon-mute");
      muteBtn.classList.toggle("muted", s.muted);
      muteBtn.textContent = s.muted ? "🔇" : "🔉";
      muteBtn.title = s.muted ? "Unmute" : "Mute";

      if (!s.muted) {
        explicitlyUncheckedPids.delete(s.pid);
      }
      const activeLoopbacks = loopbacks.filter((lb) => lb.pid === s.pid);
      const primaryChecked = !explicitlyUncheckedPids.has(s.pid);
      const activeCount = (primaryChecked ? 1 : 0) + activeLoopbacks.length;
      card.querySelector(".badge-muted").textContent = s.muted
        ? "Muted"
        : activeCount === 1
          ? "1 out"
          : `${activeCount} outs`;

      const expanded = s.is_active
        ? !collapsedActivePids.has(s.pid)
        : expandedInactivePids.has(s.pid);
      card.querySelector(".chevron").textContent = expanded ? "▾" : "▸";
      const body = card.querySelector(".routing-outputs");
      body.classList.toggle("collapsed", !expanded);

      const visibleEndpoints = renderEndpoints.filter(
        (ep) =>
          !ep.hidden ||
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
          const enabled = (isPrimary && !explicitlyUncheckedPids.has(s.pid)) || isLoopback;

          row._ctx = { session: s, endpoint: ep, isPrimary, isLoopback, activeLoopbacks };
          row.classList.toggle("disabled", !enabled);

          const cb = row.querySelector('input[type="checkbox"]');
          cb.checked = enabled;

          const nameEl = row.querySelector(".routing-output-name");
          nameEl.textContent = ep.friendly_name;
          nameEl.title = ep.friendly_name;

          const primaryTag = row.querySelector(".primary-slot");
          primaryTag.style.visibility = isPrimary ? "visible" : "hidden";

          const slider = row.querySelector('input[type="range"]');
          const label = row.querySelector(".vol-label");
          slider.disabled = !enabled;

          const savedVol =
            (s.saved_volumes && s.saved_volumes[ep.id]) ??
            rememberedOutputVolumes.get(`${s.pid}:${ep.id}`) ??
            0.5;
          const vol = isPrimary ? s.volume : isLoopback ? lb.volume : savedVol;
          rememberedOutputVolumes.set(`${s.pid}:${ep.id}`, vol);
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

export function renderAppsTab(sessions, endpoints, loopbacks, onRefresh) {
  const renderEndpoints = endpoints.filter((e) => e.flow === "Render");
  const active = sessions.filter((s) => s.is_active);
  const inactive = sessions.filter((s) => !s.is_active);

  renderAppsList(
    document.getElementById("apps-active-list"),
    active,
    renderEndpoints,
    loopbacks,
    onRefresh
  );
  document.getElementById("apps-inactive-count").textContent = String(inactive.length);
  document.getElementById("apps-inactive-group").style.display =
    inactive.length > 0 ? "" : "none";
  renderAppsList(
    document.getElementById("apps-inactive-list"),
    inactive,
    renderEndpoints,
    loopbacks,
    onRefresh
  );
}
