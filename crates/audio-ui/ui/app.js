const { invoke } = window.__TAURI__.core;

let isDraggingSlider = false;
let activeTab = "apps";

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

    renderPresets(presetData);
    renderAppsTab(status.sessions, status.endpoints);
    renderIOTab(status.endpoints);
    renderRoutingTab(status.sessions, status.endpoints);

    document.getElementById("count-apps").textContent = String(status.sessions.length);
    document.getElementById("count-io").textContent = String(status.endpoints.length);
    document.getElementById("count-routing").textContent = String(status.sessions.length);
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
      const name = s.root_exe_name || s.exe_name || `PID ${s.pid}`;
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
  header.className = "card-header";
  const title = document.createElement("span");
  title.className = "card-title";
  const pidBadge = document.createElement("span");
  pidBadge.className = "badge-tag";
  header.append(title, pidBadge);

  const row = document.createElement("div");
  row.className = "routing-row";
  const lbl = document.createElement("span");
  lbl.className = "routing-label";
  lbl.textContent = "Output";
  const select = document.createElement("select");
  row.append(lbl, select);

  card.append(header, row);

  select.onchange = async (e) => {
    const s = card._item;
    const val = e.target.value;
    select.blur();
    if (val === "__reset__") {
      await invoke("reset_app_route", { pid: s.pid, flow: "Render" });
    } else {
      await invoke("route_app", { pid: s.pid, flow: "Render", endpointId: val });
    }
    await refreshAll(true);
  };

  return card;
}

function renderRoutingTab(sessions, endpoints) {
  const renderEndpoints = endpoints.filter((e) => e.flow === "Render");
  const epSignature = renderEndpoints.map((e) => `${e.id}:${e.friendly_name}`).join("|");

  syncKeyedList(
    document.getElementById("routing-list"),
    sessions,
    (s) => `route:${s.pid}:${s.endpoint_id}`,
    "No active audio sessions to route.",
    buildRoutingCard,
    (card, s) => {
      card._item = s;
      const name = s.root_exe_name || s.exe_name || `PID ${s.pid}`;
      card.querySelector(".card-title").textContent = name;
      card.querySelector(".badge-tag").textContent = `PID ${s.pid}`;

      const select = card.querySelector("select");
      if (document.activeElement !== select && select.dataset.epSig !== epSignature) {
        select.innerHTML = "";
        const defaultOpt = document.createElement("option");
        defaultOpt.value = "__reset__";
        defaultOpt.textContent = "Reset to OS Default";
        select.appendChild(defaultOpt);
        for (const ep of renderEndpoints) {
          const opt = document.createElement("option");
          opt.value = ep.id;
          opt.textContent = ep.friendly_name;
          select.appendChild(opt);
        }
        select.dataset.epSig = epSignature;
      }

      if (document.activeElement !== select && select.value !== s.endpoint_id) {
        select.value = s.endpoint_id;
      }
    }
  );
}

setupTabs();
document.getElementById("refresh-btn").onclick = () => refreshAll(true);
refreshAll(true);
setInterval(() => refreshAll(false), 2000);
