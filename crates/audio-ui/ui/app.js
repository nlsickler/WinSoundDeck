const { invoke } = window.__TAURI__.core;

let isDraggingSlider = false;
let activeTab = "apps";
const expandedRoutingPids = new Set();

function pct(vol) {
  return Math.round(Math.max(0, Math.min(1, vol)) * 100);
}

function isUserInteracting() {
  if (isDraggingSlider) return true;
  const active = document.activeElement;
  return active && active.tagName === "SELECT";
}

function setupTabs() {
  const buttons = document.querySelectorAll(".tab-btn");
  const panels = document.querySelectorAll(".tab-panel");
  for (const btn of buttons) {
    btn.onclick = () => {
      activeTab = btn.dataset.tab;
      for (const b of buttons) {
        const selected = b.dataset.tab === activeTab;
        b.classList.toggle("active", selected);
        b.setAttribute("aria-selected", String(selected));
      }
      for (const p of panels) {
        p.classList.toggle("active", p.id === `tab-${activeTab}`);
      }
    };
  }
}

/**
 * Reconciles a container's children against `items` by stable string key,
 * preserving existing DOM elements (sliders, open <select> dropdowns, scroll) in-place.
 */
function syncKeyedList(container, items, keyFn, emptyText, createFn, updateFn) {
  if (!items || items.length === 0) {
    if (!container.querySelector(".empty-note")) {
      container.innerHTML = `<span class="empty-note">${emptyText}</span>`;
    }
    return;
  }

  const emptyEl = container.querySelector(".empty-note");
  if (emptyEl) emptyEl.remove();

  const existingByKey = new Map();
  for (const child of Array.from(container.children)) {
    if (child.dataset && child.dataset.key) {
      existingByKey.set(child.dataset.key, child);
    }
  }

  const desiredKeys = new Set();
  const orderedNodes = [];

  for (const item of items) {
    const key = keyFn(item);
    desiredKeys.add(key);
    let node = existingByKey.get(key);
    if (!node) {
      node = createFn(item);
      node.dataset.key = key;
    }
    updateFn(node, item);
    orderedNodes.push(node);
  }

  for (const [key, node] of existingByKey.entries()) {
    if (!desiredKeys.has(key)) {
      node.remove();
    }
  }

  orderedNodes.forEach((node, idx) => {
    const currentAtIdx = container.children[idx];
    if (currentAtIdx !== node) {
      container.insertBefore(node, currentAtIdx || null);
    }
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
    badge.textContent = "Daemon Connected";
    badge.className = "badge badge-ok";

    const appSessions = status.sessions.filter((s) => s.pid !== 0);

    renderPresets(presetData);
    renderAppsTab(appSessions, status.endpoints);
    renderIOTab(status.endpoints);
    renderRoutingTab(appSessions, status.endpoints, status.loopbacks || []);

    document.getElementById("count-apps").textContent = String(appSessions.length);
    document.getElementById("count-io").textContent = String(status.endpoints.length);
    document.getElementById("count-routing").textContent = String(appSessions.length);
  } catch (err) {
    badge.textContent = "Daemon Offline";
    badge.className = "badge badge-offline";
    console.error("IPC error:", err);
  }
}

function renderPresets({ active_preset_id, presets }) {
  const el = document.getElementById("presets-list");
  syncKeyedList(
    el,
    presets || [],
    (p) => p.id,
    "No presets saved yet",
    (p) => {
      const btn = document.createElement("button");
      btn.className = "preset-btn";
      btn.onclick = async () => {
        await invoke("apply_preset", { id: btn.dataset.presetId });
        await refreshAll(true);
      };
      return btn;
    },
    (btn, p) => {
      btn.dataset.presetId = p.id;
      btn.textContent = p.name;
      btn.classList.toggle("active", p.id === active_preset_id);
    }
  );
}

function buildEndpointCard() {
  const card = document.createElement("div");
  card.className = "card";

  const header = document.createElement("div");
  header.className = "card-header";
  const title = document.createElement("span");
  title.className = "card-title";
  const meta = document.createElement("div");
  meta.className = "card-meta";
  header.append(title, meta);

  const row = document.createElement("div");
  row.className = "controls-row";
  const muteBtn = document.createElement("button");
  muteBtn.className = "btn-mute";
  const slider = document.createElement("input");
  slider.type = "range";
  slider.min = "0";
  slider.max = "100";
  const label = document.createElement("span");
  label.className = "vol-label";
  row.append(muteBtn, slider, label);

  card.append(header, row);

  muteBtn.onclick = async () => {
    const ep = card._item;
    await invoke("set_endpoint_mute", {
      flow: ep.flow,
      endpointId: ep.id,
      muted: !ep.muted,
    });
    await refreshAll(true);
  };

  slider.onpointerdown = () => { isDraggingSlider = true; };
  slider.onpointerup = () => { isDraggingSlider = false; };
  slider.onpointercancel = () => { isDraggingSlider = false; };
  slider.oninput = async (e) => {
    const ep = card._item;
    const v = Number(e.target.value) / 100;
    label.textContent = `${e.target.value}%`;
    await invoke("set_endpoint_volume", {
      flow: ep.flow,
      endpointId: ep.id,
      volume: v,
    });
  };

  return card;
}

function updateEndpointCard(card, ep) {
  card._item = ep;
  card.querySelector(".card-title").textContent = ep.friendly_name;

  const meta = card.querySelector(".card-meta");
  const badges = [];
  if (ep.is_default_multimedia) badges.push('<span class="badge-tag">Default</span>');
  if (ep.is_default_communications) badges.push('<span class="badge-tag">Comms</span>');
  const nextHtml = badges.join("");
  if (meta.innerHTML !== nextHtml) meta.innerHTML = nextHtml;

  const muteBtn = card.querySelector(".btn-mute");
  muteBtn.classList.toggle("muted", ep.muted);
  muteBtn.textContent = ep.muted ? "Muted" : "Mute";

  const pctVal = pct(ep.volume);
  const slider = card.querySelector('input[type="range"]');
  if (!isDraggingSlider && Number(slider.value) !== pctVal) {
    slider.value = String(pctVal);
  }
  card.querySelector(".vol-label").textContent = `${pctVal}%`;
}

function renderIOTab(endpoints) {
  const outputs = endpoints.filter((e) => e.flow === "Render");
  const inputs = endpoints.filter((e) => e.flow === "Capture");
  syncKeyedList(
    document.getElementById("outputs-list"),
    outputs,
    (ep) => `Render:${ep.id}`,
    "No active output devices.",
    buildEndpointCard,
    updateEndpointCard
  );
  syncKeyedList(
    document.getElementById("inputs-list"),
    inputs,
    (ep) => `Capture:${ep.id}`,
    "No active input devices.",
    buildEndpointCard,
    updateEndpointCard
  );
}

function buildAppCard() {
  const card = document.createElement("div");
  card.className = "card";

  const header = document.createElement("div");
  header.className = "card-header";
  const title = document.createElement("span");
  title.className = "card-title";
  const meta = document.createElement("div");
  meta.className = "card-meta";
  const epBadge = document.createElement("span");
  epBadge.className = "badge-muted";
  const pidBadge = document.createElement("span");
  pidBadge.className = "badge-tag";
  meta.append(epBadge, pidBadge);
  header.append(title, meta);

  const row = document.createElement("div");
  row.className = "controls-row";
  const muteBtn = document.createElement("button");
  muteBtn.className = "btn-mute";
  const slider = document.createElement("input");
  slider.type = "range";
  slider.min = "0";
  slider.max = "100";
  const label = document.createElement("span");
  label.className = "vol-label";
  row.append(muteBtn, slider, label);

  card.append(header, row);

  muteBtn.onclick = async () => {
    const s = card._item;
    await invoke("set_session_mute", {
      endpointId: s.endpoint_id,
      pid: s.pid,
      muted: !s.muted,
    });
    await refreshAll(true);
  };

  slider.onpointerdown = () => { isDraggingSlider = true; };
  slider.onpointerup = () => { isDraggingSlider = false; };
  slider.onpointercancel = () => { isDraggingSlider = false; };
  slider.oninput = async (e) => {
    const s = card._item;
    const v = Number(e.target.value) / 100;
    label.textContent = `${e.target.value}%`;
    await invoke("set_session_volume", {
      endpointId: s.endpoint_id,
      pid: s.pid,
      volume: v,
    });
  };

  return card;
}

function renderAppsTab(sessions, endpoints) {
  const epNames = new Map(endpoints.map((e) => [e.id, e.friendly_name]));
  syncKeyedList(
    document.getElementById("apps-list"),
    sessions,
    (s) => `${s.endpoint_id}:${s.pid}`,
    "No active audio sessions.",
    buildAppCard,
    (card, s) => {
      card._item = s;
      const name = s.display_name || s.root_exe_name || s.exe_name || `PID ${s.pid}`;
      card.querySelector(".card-title").textContent = name;
      card.querySelector(".badge-muted").textContent = epNames.get(s.endpoint_id) || "Unknown";
      card.querySelector(".badge-tag").textContent = `PID ${s.pid}`;

      const muteBtn = card.querySelector(".btn-mute");
      muteBtn.classList.toggle("muted", s.muted);
      muteBtn.textContent = s.muted ? "Muted" : "Mute";

      const pctVal = pct(s.volume);
      const slider = card.querySelector('input[type="range"]');
      if (!isDraggingSlider && Number(slider.value) !== pctVal) {
        slider.value = String(pctVal);
      }
      card.querySelector(".vol-label").textContent = `${pctVal}%`;
    }
  );
}

function buildRoutingCard() {
  const card = document.createElement("div");
  card.className = "card";

  const header = document.createElement("div");
  header.className = "card-header routing-header";

  const titleWrap = document.createElement("div");
  titleWrap.className = "routing-title-wrap";
  const chevron = document.createElement("span");
  chevron.className = "chevron";
  const title = document.createElement("span");
  title.className = "card-title";
  titleWrap.append(chevron, title);

  const meta = document.createElement("div");
  meta.className = "card-meta";
  const outCountBadge = document.createElement("span");
  outCountBadge.className = "badge-muted";
  const pidBadge = document.createElement("span");
  pidBadge.className = "badge-tag";
  meta.append(outCountBadge, pidBadge);

  header.append(titleWrap, meta);

  const body = document.createElement("div");
  body.className = "routing-outputs";

  card.append(header, body);

  header.onclick = () => {
    const s = card._item;
    if (!s) return;
    if (expandedRoutingPids.has(s.pid)) {
      expandedRoutingPids.delete(s.pid);
    } else {
      expandedRoutingPids.add(s.pid);
    }
    const expanded = expandedRoutingPids.has(s.pid);
    chevron.textContent = expanded ? "▾" : "▸";
    body.classList.toggle("collapsed", !expanded);
  };

  return card;
}

function buildRoutingOutputRow() {
  const row = document.createElement("div");
  row.className = "routing-output-row";

  const cb = document.createElement("input");
  cb.type = "checkbox";

  const name = document.createElement("span");
  name.className = "routing-output-name";

  const primaryTag = document.createElement("span");
  primaryTag.className = "badge-tag";
  primaryTag.textContent = "Primary";

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
      // Unchecking primary promotes the first active secondary loopback to primary.
      const fallback = activeLoopbacks.find((lb) => lb.target_endpoint_id !== endpoint.id);
      if (!fallback) {
        cb.checked = true;
        return;
      }
      await invoke("stop_loopback", {
        pid: session.pid,
        targetEndpointId: fallback.target_endpoint_id,
      });
      await invoke("route_app", {
        pid: session.pid,
        flow: "Render",
        endpointId: fallback.target_endpoint_id,
      });
    } else if (cb.checked && !isLoopback) {
      await invoke("start_loopback", {
        pid: session.pid,
        targetEndpointId: endpoint.id,
        volume: 0.5,
      });
    } else if (!cb.checked && isLoopback) {
      await invoke("stop_loopback", {
        pid: session.pid,
        targetEndpointId: endpoint.id,
      });
    }
    await refreshAll(true);
  };

  slider.onpointerdown = () => { isDraggingSlider = true; };
  slider.onpointerup = () => { isDraggingSlider = false; };
  slider.onpointercancel = () => { isDraggingSlider = false; };
  slider.oninput = async (e) => {
    const ctx = row._ctx;
    if (!ctx) return;
    const { session, endpoint, isPrimary, isLoopback } = ctx;
    const v = Number(e.target.value) / 100;
    label.textContent = `${e.target.value}%`;
    if (isPrimary) {
      await invoke("set_session_volume", {
        endpointId: session.endpoint_id,
        pid: session.pid,
        volume: v,
      });
    } else if (isLoopback) {
      await invoke("set_loopback_volume", {
        pid: session.pid,
        targetEndpointId: endpoint.id,
        volume: v,
      });
    }
  };

  return row;
}

function renderRoutingTab(sessions, endpoints, loopbacks) {
  const renderEndpoints = endpoints.filter((e) => e.flow === "Render");

  syncKeyedList(
    document.getElementById("routing-list"),
    sessions,
    (s) => `route:${s.pid}`,
    "No active audio sessions to route.",
    buildRoutingCard,
    (card, s) => {
      card._item = s;
      const name = s.display_name || s.root_exe_name || s.exe_name || `PID ${s.pid}`;
      card.querySelector(".card-title").textContent = name;
      card.querySelector(".badge-tag").textContent = `PID ${s.pid}`;

      const activeLoopbacks = loopbacks.filter((lb) => lb.pid === s.pid);
      const activeCount = 1 + activeLoopbacks.length;
      card.querySelector(".badge-muted").textContent =
        activeCount === 1 ? "1 output" : `${activeCount} outputs`;

      const expanded = expandedRoutingPids.has(s.pid);
      card.querySelector(".chevron").textContent = expanded ? "▾" : "▸";
      const body = card.querySelector(".routing-outputs");
      body.classList.toggle("collapsed", !expanded);

      syncKeyedList(
        body,
        renderEndpoints,
        (ep) => ep.id,
        "No active output devices.",
        buildRoutingOutputRow,
        (row, ep) => {
          const isPrimary = ep.id === s.endpoint_id;
          const lb = activeLoopbacks.find((x) => x.target_endpoint_id === ep.id);
          const isLoopback = Boolean(lb);
          const enabled = isPrimary || isLoopback;

          row._ctx = { session: s, endpoint: ep, isPrimary, isLoopback, activeLoopbacks };
          row.classList.toggle("disabled", !enabled);

          const cb = row.querySelector('input[type="checkbox"]');
          cb.checked = enabled;

          const nameEl = row.querySelector(".routing-output-name");
          nameEl.textContent = ep.friendly_name;
          nameEl.title = ep.friendly_name;

          const primaryTag = row.querySelector(".badge-tag");
          primaryTag.style.display = isPrimary ? "" : "none";

          const slider = row.querySelector('input[type="range"]');
          const label = row.querySelector(".vol-label");
          slider.disabled = !enabled;

          const vol = isPrimary ? s.volume : isLoopback ? lb.volume : 0.5;
          const pctVal = pct(vol);
          if (!isDraggingSlider && Number(slider.value) !== pctVal) {
            slider.value = String(pctVal);
          }
          label.textContent = enabled ? `${pctVal}%` : "--";
        }
      );
    }
  );
}

setupTabs();
document.getElementById("refresh-btn").onclick = () => refreshAll(true);
refreshAll(true);
setInterval(() => refreshAll(false), 2000);
