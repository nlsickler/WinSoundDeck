import {
  createMeterSlider,
  isSliderDragging,
  pct,
  setSliderDragging,
} from "./dom.js";

const { invoke } = window.__TAURI__.core;

const explicitlyUncheckedIds = new Set();
const rememberedOutputVolumes = new Map();

export function appIdFor(s) {
  return s.pid > 0 ? String(s.pid) : s.app_key || "0";
}

export function appDisplayName(s) {
  return s.display_name || s.root_exe_name || s.exe_name || `PID ${s.pid}`;
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

export function loopbacksForSession(loopbacks, s) {
  return loopbacks.filter((lb) =>
    s.pid > 0 ? lb.pid === s.pid : s.app_key && lb.app_key === s.app_key
  );
}

export function isPrimaryUnchecked(appId) {
  return explicitlyUncheckedIds.has(appId);
}

export function clearPrimaryUnchecked(appId) {
  explicitlyUncheckedIds.delete(appId);
}

export function buildRoutingRow(onRefresh, { vertical = false } = {}) {
  const row = document.createElement("div");
  row.className = vertical
    ? "routing-output-row device-fader-strip"
    : "routing-output-row";

  const cb = document.createElement("input");
  cb.type = "checkbox";

  const icon = document.createElement("img");
  icon.className = vertical
    ? "app-icon app-icon-lg collapsed"
    : "app-icon app-icon-sm collapsed";
  icon.alt = "";

  const name = document.createElement("span");
  name.className = vertical
    ? "routing-output-name strip-name"
    : "routing-output-name";

  const primaryTag = document.createElement("span");
  primaryTag.className = "badge-tag primary-slot";
  primaryTag.textContent = "P";
  primaryTag.title = "Primary Output";

  const { wrap: sliderWrap, slider } = createMeterSlider();

  const label = document.createElement("span");
  label.className = vertical ? "vol-label strip-vol" : "vol-label";

  if (vertical) {
    const top = document.createElement("div");
    top.className = "strip-top";
    top.append(cb, primaryTag);

    const stage = document.createElement("div");
    stage.className = "device-fader-stage";
    stage.append(sliderWrap);

    row.append(top, icon, name, stage, label);
  } else {
    row.append(cb, icon, name, primaryTag, sliderWrap, label);
  }

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
    const rowVolFill = row.querySelector(".slider-vol-fill");
    if (rowVolFill) rowVolFill.style.width = `${e.target.value}%`;
    const rowMeterFill = row.querySelector(".slider-peak-fill");
    if (rowMeterFill) rowMeterFill.dataset.peakScale = String(v);
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

export function updateRoutingRow(
  row,
  { session, endpoint, activeLoopbacks, peakPidAttr, labelText, labelTitle, iconUrl = null }
) {
  const appId = appIdFor(session);
  const isPrimary = endpoint.id === session.endpoint_id;
  const lb = activeLoopbacks.find((x) => x.target_endpoint_id === endpoint.id);
  const isLoopback = Boolean(lb);
  const enabled = (isPrimary && !explicitlyUncheckedIds.has(appId)) || isLoopback;

  row._ctx = { session, endpoint, isPrimary, isLoopback, activeLoopbacks };
  row.classList.toggle("disabled", !enabled);

  const cb = row.querySelector('input[type="checkbox"]');
  cb.checked = enabled;

  const iconEl = row.querySelector(".app-icon");
  if (iconEl) {
    if (iconUrl) {
      if (iconEl.src !== iconUrl) iconEl.src = iconUrl;
      iconEl.classList.remove("collapsed");
    } else {
      iconEl.classList.add("collapsed");
    }
  }

  const nameEl = row.querySelector(".routing-output-name");
  nameEl.textContent = labelText;
  nameEl.title = labelTitle || labelText;
  row.title = labelTitle || labelText;

  const primaryTag = row.querySelector(".primary-slot");
  primaryTag.style.visibility = isPrimary ? "visible" : "hidden";

  const slider = row.querySelector('input[type="range"]');
  const label = row.querySelector(".vol-label");
  slider.disabled = !enabled;
  const rowMeterFill = row.querySelector(".slider-peak-fill");
  if (rowMeterFill) {
    const activePid = session.is_active && enabled && session.pid > 0 ? peakPidAttr : "";
    rowMeterFill.dataset.peakPid = activePid;
    if (!activePid) rowMeterFill.style.width = "0%";
  }

  const savedVol =
    (session.saved_volumes && session.saved_volumes[endpoint.id]) ??
    rememberedOutputVolumes.get(`${appId}:${endpoint.id}`) ??
    0.5;
  const vol = isPrimary ? session.volume : isLoopback ? lb.volume : savedVol;
  rememberedOutputVolumes.set(`${appId}:${endpoint.id}`, vol);
  if (rowMeterFill) {
    rowMeterFill.dataset.peakScale = session.muted || !enabled ? "0" : String(vol);
  }
  const pctVal = pct(vol);
  if (!isSliderDragging() && Number(slider.value) !== pctVal) {
    slider.value = String(pctVal);
  }
  const rowVolFill = row.querySelector(".slider-vol-fill");
  if (rowVolFill) rowVolFill.style.width = `${slider.value}%`;
  label.textContent = enabled ? `${pctVal}%` : "--";
}
