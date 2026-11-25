import {
  bindMeterSlider,
  createMeterSlider,
  isSliderDragging,
  pct,
} from "./dom.js";
import { appIdFor, applyAppIcon } from "./app_utils.js";
import {
  isPrimaryUnchecked,
  openStripContextMenu,
  rememberedOutputVolumes,
  toggleRouting,
} from "./routing_actions.js";

const { invoke } = window.__TAURI__.core;

export function buildRoutingRow(onRefresh, { vertical = false } = {}) {
  const row = document.createElement("div");
  row.className = vertical
    ? "routing-output-row device-fader-strip"
    : "routing-output-row";

  const icon = document.createElement("img");
  icon.className = vertical
    ? "app-icon app-icon-lg collapsed"
    : "app-icon app-icon-sm collapsed";
  icon.alt = "";

  const name = document.createElement("span");
  name.className = vertical
    ? "routing-output-name strip-name"
    : "routing-output-name";

  const { wrap: sliderWrap, slider, volFill, meterFill } = createMeterSlider();

  const label = document.createElement("span");
  label.className = vertical ? "vol-label strip-vol" : "vol-label";

  if (vertical) {
    const stateBadge = document.createElement("span");
    stateBadge.className = "badge-tag strip-state-badge collapsed";

    const stage = document.createElement("div");
    stage.className = "device-fader-stage";
    stage.append(sliderWrap);

    const toggleFromStrip = () => {
      if (!row._ctx) return;
      toggleRouting(row._ctx, !row._ctx.enabled, onRefresh);
    };
    icon.onclick = toggleFromStrip;
    name.onclick = toggleFromStrip;
    row.oncontextmenu = (e) => openStripContextMenu(e, row._ctx, onRefresh);

    row.append(stateBadge, icon, name, stage, label);
  } else {
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.onchange = () => toggleRouting(row._ctx, cb.checked, onRefresh);

    const primaryTag = document.createElement("span");
    primaryTag.className = "badge-tag primary-slot";
    primaryTag.textContent = "P";
    primaryTag.title = "Primary Output";

    row.oncontextmenu = (e) =>
      openStripContextMenu(e, row._ctx, onRefresh, { includePin: false });

    row.append(cb, icon, name, primaryTag, sliderWrap, label);
  }

  bindMeterSlider({
    slider,
    volFill,
    meterFill,
    label,
    onInput: async (v) => {
      const ctx = row._ctx;
      if (!ctx) return;
      const { session, endpoint } = ctx;
      rememberedOutputVolumes.set(`${appIdFor(session)}:${endpoint.id}`, v);
      await invoke("set_app_output_volume", {
        pid: session.pid,
        appKey: session.app_key || null,
        endpointId: endpoint.id,
        volume: v,
        finalize: false,
      });
    },
    onChange: async (v) => {
      const ctx = row._ctx;
      if (!ctx) return;
      const { session, endpoint } = ctx;
      rememberedOutputVolumes.set(`${appIdFor(session)}:${endpoint.id}`, v);
      await invoke("set_app_output_volume", {
        pid: session.pid,
        appKey: session.app_key || null,
        endpointId: endpoint.id,
        volume: v,
        finalize: true,
      });
      await onRefresh(true);
    },
  });

  return row;
}

export function updateRoutingRow(
  row,
  {
    session,
    endpoint,
    activeLoopbacks,
    peakPidAttr,
    labelText,
    labelTitle,
    iconUrl = null,
    stateBadge = null,
  }
) {
  const appId = appIdFor(session);
  const isPrimary = endpoint.id === session.endpoint_id;
  const lb = activeLoopbacks.find((x) => x.target_endpoint_id === endpoint.id);
  const isLoopback = Boolean(lb);
  const isVertical = row.classList.contains("device-fader-strip");
  const primaryEnabled = isPrimary && !session.muted && !isPrimaryUnchecked(appId);
  const enabled = isVertical
    ? !session.muted && (primaryEnabled || isLoopback)
    : primaryEnabled || isLoopback;

  row._ctx = { session, endpoint, isPrimary, isLoopback, activeLoopbacks, enabled };
  row.classList.toggle("disabled", !enabled);

  const cb = row.querySelector('input[type="checkbox"]');
  if (cb) cb.checked = enabled;

  const badgeEl = row.querySelector(".strip-state-badge");
  if (badgeEl) {
    if (stateBadge) {
      badgeEl.textContent = stateBadge.text;
      badgeEl.title = stateBadge.title || stateBadge.text;
      badgeEl.className = `badge-tag strip-state-badge strip-state-${stateBadge.variant}`;
    } else {
      badgeEl.textContent = "";
      badgeEl.title = "";
      badgeEl.className = "badge-tag strip-state-badge collapsed";
    }
  }

  const iconEl = row.querySelector(".app-icon");
  applyAppIcon(iconEl, iconUrl, iconUrl !== undefined && isVertical);

  const nameEl = row.querySelector(".routing-output-name");
  nameEl.textContent = labelText;
  nameEl.title = labelTitle || labelText;
  row.title = row.classList.contains("device-fader-strip")
    ? `${labelTitle || labelText} — Click icon to toggle • Right-click for options`
    : labelTitle || labelText;

  const primaryTag = row.querySelector(".primary-slot");
  if (primaryTag) primaryTag.style.visibility = isPrimary ? "visible" : "hidden";

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
