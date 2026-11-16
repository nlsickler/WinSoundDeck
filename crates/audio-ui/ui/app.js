const { invoke } = window.__TAURI__.core;

let isDragging = false;
let latestStatus = null;

function pct(vol) {
  return Math.round(Math.max(0, Math.min(1, vol)) * 100);
}

async function refreshAll() {
  if (isDragging) return;
  const badge = document.getElementById("daemon-badge");
  try {
    const [status, presetData] = await Promise.all([
      invoke("get_status"),
      invoke("list_presets"),
    ]);
    latestStatus = status;
    badge.textContent = "Daemon Connected";
    badge.className = "badge badge-ok";
    renderPresets(presetData);
    renderEndpoints(status.endpoints);
    renderSessions(status.sessions, status.endpoints);
  } catch (err) {
    badge.textContent = "Daemon Offline";
    badge.className = "badge badge-offline";
    console.error("IPC error:", err);
  }
}

function renderPresets({ active_preset_id, presets }) {
  const el = document.getElementById("presets-list");
  el.innerHTML = "";
  if (!presets || presets.length === 0) {
    el.innerHTML = '<span class="empty-note">No presets saved in presets.json yet.</span>';
    return;
  }
  for (const p of presets) {
    const btn = document.createElement("button");
    btn.className = "preset-btn" + (p.id === active_preset_id ? " active" : "");
    btn.textContent = p.name;
    btn.onclick = async () => {
      await invoke("apply_preset", { id: p.id });
      await refreshAll();
    };
    el.appendChild(btn);
  }
}

function renderEndpoints(endpoints) {
  const outEl = document.getElementById("outputs-list");
  const inEl = document.getElementById("inputs-list");
  outEl.innerHTML = "";
  inEl.innerHTML = "";

  for (const ep of endpoints) {
    const card = document.createElement("div");
    card.className = "card";

    const header = document.createElement("div");
    header.className = "card-header";

    const title = document.createElement("span");
    title.className = "card-title";
    title.textContent = ep.friendly_name;
    header.appendChild(title);

    const meta = document.createElement("div");
    meta.className = "card-meta";
    if (ep.is_default_multimedia) {
      meta.innerHTML += '<span class="badge-tag">Default</span>';
    }
    if (ep.is_default_communications) {
      meta.innerHTML += '<span class="badge-tag">Comms</span>';
    }
    header.appendChild(meta);
    card.appendChild(header);

    const row = document.createElement("div");
    row.className = "controls-row";

    const muteBtn = document.createElement("button");
    muteBtn.className = "btn-mute" + (ep.muted ? " muted" : "");
    muteBtn.textContent = ep.muted ? "Muted" : "Mute";
    muteBtn.onclick = async () => {
      await invoke("set_endpoint_mute", {
        flow: ep.flow,
        endpointId: ep.id,
        muted: !ep.muted,
      });
      await refreshAll();
    };

    const slider = document.createElement("input");
    slider.type = "range";
    slider.min = "0";
    slider.max = "100";
    slider.value = String(pct(ep.volume));

    const label = document.createElement("span");
    label.className = "vol-label";
    label.textContent = `${pct(ep.volume)}%`;

    slider.onpointerdown = () => { isDragging = true; };
    slider.onpointerup = () => { isDragging = false; };
    slider.oninput = async (e) => {
      const v = Number(e.target.value) / 100;
      label.textContent = `${e.target.value}%`;
      await invoke("set_endpoint_volume", {
        flow: ep.flow,
        endpointId: ep.id,
        volume: v,
      });
    };

    row.append(muteBtn, slider, label);
    card.appendChild(row);

    if (ep.flow === "Render") {
      outEl.appendChild(card);
    } else {
      inEl.appendChild(card);
    }
  }
}

function renderSessions(sessions, endpoints) {
  const el = document.getElementById("sessions-list");
  el.innerHTML = "";
  if (!sessions || sessions.length === 0) {
    el.innerHTML = '<span class="empty-note">No active audio sessions.</span>';
    return;
  }

  const renderEndpointsList = endpoints.filter((e) => e.flow === "Render");

  for (const s of sessions) {
    const card = document.createElement("div");
    card.className = "card";

    const header = document.createElement("div");
    header.className = "card-header";
    const name = s.root_exe_name || s.exe_name || `PID ${s.pid}`;
    header.innerHTML = `<span class="card-title">${name}</span><span class="badge-tag">PID ${s.pid}</span>`;
    card.appendChild(header);

    const row = document.createElement("div");
    row.className = "controls-row";

    const muteBtn = document.createElement("button");
    muteBtn.className = "btn-mute" + (s.muted ? " muted" : "");
    muteBtn.textContent = s.muted ? "Muted" : "Mute";
    muteBtn.onclick = async () => {
      await invoke("set_session_mute", {
        endpointId: s.endpoint_id,
        pid: s.pid,
        muted: !s.muted,
      });
      await refreshAll();
    };

    const slider = document.createElement("input");
    slider.type = "range";
    slider.min = "0";
    slider.max = "100";
    slider.value = String(pct(s.volume));

    const label = document.createElement("span");
    label.className = "vol-label";
    label.textContent = `${pct(s.volume)}%`;

    slider.onpointerdown = () => { isDragging = true; };
    slider.onpointerup = () => { isDragging = false; };
    slider.oninput = async (e) => {
      const v = Number(e.target.value) / 100;
      label.textContent = `${e.target.value}%`;
      await invoke("set_session_volume", {
        endpointId: s.endpoint_id,
        pid: s.pid,
        volume: v,
      });
    };

    row.append(muteBtn, slider, label);
    card.appendChild(row);

    const routeSelect = document.createElement("select");
    const defaultOpt = document.createElement("option");
    defaultOpt.value = "__reset__";
    defaultOpt.textContent = "Reset to OS Default";
    routeSelect.appendChild(defaultOpt);

    for (const ep of renderEndpointsList) {
      const opt = document.createElement("option");
      opt.value = ep.id;
      opt.textContent = ep.friendly_name;
      if (ep.id === s.endpoint_id) {
        opt.selected = true;
      }
      routeSelect.appendChild(opt);
    }

    routeSelect.onchange = async (e) => {
      const val = e.target.value;
      if (val === "__reset__") {
        await invoke("reset_app_route", { pid: s.pid, flow: "Render" });
      } else {
        await invoke("route_app", { pid: s.pid, flow: "Render", endpointId: val });
      }
      await refreshAll();
    };

    card.appendChild(routeSelect);
    el.appendChild(card);
  }
}

document.getElementById("refresh-btn").onclick = refreshAll;
refreshAll();
setInterval(refreshAll, 2000);
