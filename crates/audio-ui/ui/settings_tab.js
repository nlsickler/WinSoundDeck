import { syncKeyedList } from "./dom.js";

const { invoke } = window.__TAURI__.core;

export function setupSettingsControls() {
  const subtabBtns = document.querySelectorAll(".subtab-btn");
  const outputsList = document.getElementById("settings-outputs-list");
  const inputsList = document.getElementById("settings-inputs-list");
  for (const btn of subtabBtns) {
    btn.onclick = () => {
      const target = btn.dataset.subtab;
      for (const b of subtabBtns) {
        b.classList.toggle("active", b.dataset.subtab === target);
      }
      outputsList.classList.toggle("collapsed", target !== "outputs");
      inputsList.classList.toggle("collapsed", target !== "inputs");
    };
  }
}

function buildAllowlistRow(onRefresh) {
  const row = document.createElement("label");
  row.className = "allowlist-row";

  const left = document.createElement("div");
  left.className = "allowlist-left";
  const cb = document.createElement("input");
  cb.type = "checkbox";
  const name = document.createElement("span");
  name.className = "card-title";
  left.append(cb, name);

  const statusTag = document.createElement("span");
  statusTag.className = "badge-muted";

  row.append(left, statusTag);

  cb.onchange = async () => {
    const ep = row._item;
    await invoke("set_endpoint_allowed", {
      endpointId: ep.id,
      allowed: cb.checked,
    });
    await onRefresh(true);
  };

  return row;
}

function updateAllowlistRow(row, ep) {
  row._item = ep;
  row.classList.toggle("inactive-card", !ep.is_connected);
  row.querySelector('input[type="checkbox"]').checked = Boolean(ep.is_allowed);
  const nameEl = row.querySelector(".card-title");
  nameEl.textContent = ep.friendly_name;
  nameEl.title = ep.friendly_name;
  row.querySelector(".badge-muted").textContent = ep.is_connected ? "Connected" : "Offline";
}

export function renderSettingsTab(endpoints, onRefresh) {
  const byName = (a, b) =>
    a.friendly_name.localeCompare(b.friendly_name, undefined, { sensitivity: "base" });
  const outputs = endpoints.filter((e) => e.flow === "Render").sort(byName);
  const inputs = endpoints.filter((e) => e.flow === "Capture").sort(byName);
  syncKeyedList(
    document.getElementById("settings-outputs-list"),
    outputs,
    (ep) => `set:Render:${ep.id}`,
    "No output devices found.",
    () => buildAllowlistRow(onRefresh),
    updateAllowlistRow
  );
  syncKeyedList(
    document.getElementById("settings-inputs-list"),
    inputs,
    (ep) => `set:Capture:${ep.id}`,
    "No input devices found.",
    () => buildAllowlistRow(onRefresh),
    updateAllowlistRow
  );
}
