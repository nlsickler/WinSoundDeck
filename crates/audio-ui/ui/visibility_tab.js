import { syncKeyedList } from "./dom.js";

const { invoke } = window.__TAURI__.core;

const expandedIds = new Set();
let pendingWinRemoveId = null;

export function setupVisibilityControls() {
  const subtabBtns = document.querySelectorAll(".subtab-btn");
  const outputsList = document.getElementById("visibility-outputs-list");
  const inputsList = document.getElementById("visibility-inputs-list");
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

function formatLastSeen(ep) {
  if (ep.is_connected) return "Connected";
  if (!ep.last_seen) return "Offline";
  const diff = Math.max(0, Math.floor(Date.now() / 1000) - ep.last_seen);
  if (diff < 60) return "Last seen just now";
  if (diff < 3600) return `Last seen ${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `Last seen ${Math.floor(diff / 3600)}h ago`;
  return `Last seen ${Math.floor(diff / 86400)}d ago`;
}

function buildVisibilityCard(onRefresh) {
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
  const name = document.createElement("span");
  name.className = "card-title";
  titleToggle.append(chevron, name);

  const btTag = document.createElement("span");
  btTag.className = "badge-tag collapsed";
  btTag.textContent = "Bluetooth";

  const statusTag = document.createElement("span");
  statusTag.className = "badge-muted";

  const eyeBtn = document.createElement("button");
  eyeBtn.className = "btn-eye";
  eyeBtn.textContent = "👁️";

  header.append(titleToggle, btTag, statusTag, eyeBtn);

  const body = document.createElement("div");
  body.className = "visibility-details collapsed";

  const idText = document.createElement("div");
  idText.className = "visibility-id";

  const actions = document.createElement("div");
  actions.className = "visibility-actions";

  const forgetBtn = document.createElement("button");
  forgetBtn.className = "btn-secondary";
  forgetBtn.textContent = "Forget from Mixer";

  const btSettingsBtn = document.createElement("button");
  btSettingsBtn.className = "btn-secondary collapsed";
  btSettingsBtn.textContent = "Open Bluetooth Settings";
  btSettingsBtn.title = "Unpair this Bluetooth device in Windows Settings";

  const winRemoveBtn = document.createElement("button");
  winRemoveBtn.className = "btn-danger";
  winRemoveBtn.textContent = "Remove from Windows";

  const confirmBox = document.createElement("div");
  confirmBox.className = "visibility-confirm collapsed";
  const confirmLabel = document.createElement("span");
  confirmLabel.className = "preset-confirm-label";
  confirmLabel.textContent = "Uninstall from Windows?";
  const confirmYes = document.createElement("button");
  confirmYes.className = "btn-icon-action";
  confirmYes.textContent = "✅";
  confirmYes.title = "Confirm removal (UAC)";
  const confirmNo = document.createElement("button");
  confirmNo.className = "btn-icon-action";
  confirmNo.textContent = "❌";
  confirmNo.title = "Cancel";
  confirmBox.append(confirmLabel, confirmYes, confirmNo);

  const errorNote = document.createElement("div");
  errorNote.className = "visibility-error collapsed";

  actions.append(forgetBtn, btSettingsBtn, winRemoveBtn, confirmBox);
  body.append(idText, actions, errorNote);
  card.append(header, body);

  const toggleExpand = () => {
    const ep = card._item;
    if (!ep) return;
    if (expandedIds.has(ep.id)) {
      expandedIds.delete(ep.id);
      if (pendingWinRemoveId === ep.id) pendingWinRemoveId = null;
    } else {
      expandedIds.add(ep.id);
    }
    const expanded = expandedIds.has(ep.id);
    chevron.textContent = expanded ? "▾" : "▸";
    body.classList.toggle("collapsed", !expanded);
  };
  titleToggle.onclick = toggleExpand;
  statusTag.onclick = toggleExpand;
  statusTag.style.cursor = "pointer";

  eyeBtn.onclick = async () => {
    const ep = card._item;
    if (!ep) return;
    await invoke("set_endpoint_allowed", {
      endpointId: ep.id,
      allowed: !ep.is_allowed,
    });
    await onRefresh(true);
  };

  forgetBtn.onclick = async () => {
    const ep = card._item;
    if (!ep) return;
    expandedIds.delete(ep.id);
    await invoke("forget_endpoint", { endpointId: ep.id });
    await onRefresh(true);
  };

  btSettingsBtn.onclick = async () => {
    try {
      await invoke("open_bluetooth_settings");
    } catch (err) {
      errorNote.textContent = String(err);
      errorNote.classList.remove("collapsed");
    }
  };

  winRemoveBtn.onclick = () => {
    const ep = card._item;
    if (!ep) return;
    pendingWinRemoveId = ep.id;
    errorNote.classList.add("collapsed");
    winRemoveBtn.classList.add("collapsed");
    confirmBox.classList.remove("collapsed");
  };

  confirmNo.onclick = () => {
    pendingWinRemoveId = null;
    confirmBox.classList.add("collapsed");
    winRemoveBtn.classList.remove("collapsed");
  };

  confirmYes.onclick = async () => {
    const ep = card._item;
    if (!ep) return;
    pendingWinRemoveId = null;
    confirmBox.classList.add("collapsed");
    winRemoveBtn.classList.remove("collapsed");
    try {
      await invoke("remove_windows_endpoint", { endpointId: ep.id });
      expandedIds.delete(ep.id);
      await onRefresh(true);
    } catch (err) {
      errorNote.textContent = String(err);
      errorNote.classList.remove("collapsed");
    }
  };

  return card;
}

function updateVisibilityCard(card, ep) {
  card._item = ep;
  card.classList.toggle("inactive-card", !ep.is_connected);

  const expanded = expandedIds.has(ep.id);
  card.querySelector(".chevron").textContent = expanded ? "▾" : "▸";
  card.querySelector(".visibility-details").classList.toggle("collapsed", !expanded);

  const nameEl = card.querySelector(".card-title");
  nameEl.textContent = ep.friendly_name;
  nameEl.title = ep.friendly_name;

  card.querySelector(".badge-tag").classList.toggle("collapsed", !ep.is_bluetooth);
  card.querySelector(".badge-muted").textContent = formatLastSeen(ep);

  const eyeBtn = card.querySelector(".btn-eye");
  eyeBtn.classList.toggle("allowed", Boolean(ep.is_allowed));
  eyeBtn.title = ep.is_allowed
    ? "Visible in System & Applications — click to hide"
    : "Hidden from System & Applications — click to show";

  card.querySelector(".visibility-id").textContent = ep.id;

  const actions = card.querySelector(".visibility-actions");
  actions.classList.toggle("collapsed", ep.is_connected);

  const confirming = pendingWinRemoveId === ep.id;
  const isBt = Boolean(ep.is_bluetooth);
  const btBtn = actions.querySelectorAll(".btn-secondary")[1];
  if (btBtn) btBtn.classList.toggle("collapsed", !isBt);
  card.querySelector(".btn-danger").classList.toggle("collapsed", isBt || confirming);
  card.querySelector(".visibility-confirm").classList.toggle("collapsed", isBt || !confirming);
}

export function renderVisibilityTab(endpoints, onRefresh) {
  const byLastSeen = (a, b) => {
    const connDiff = Number(Boolean(b.is_connected)) - Number(Boolean(a.is_connected));
    if (connDiff !== 0) return connDiff;
    const seenDiff = (b.last_seen ?? 0) - (a.last_seen ?? 0);
    if (seenDiff !== 0) return seenDiff;
    return a.friendly_name.localeCompare(b.friendly_name, undefined, { sensitivity: "base" });
  };
  const outputs = endpoints.filter((e) => e.flow === "Render").sort(byLastSeen);
  const inputs = endpoints.filter((e) => e.flow === "Capture").sort(byLastSeen);
  syncKeyedList(
    document.getElementById("visibility-outputs-list"),
    outputs,
    (ep) => `set:Render:${ep.id}`,
    "No output devices found.",
    () => buildVisibilityCard(onRefresh),
    updateVisibilityCard
  );
  syncKeyedList(
    document.getElementById("visibility-inputs-list"),
    inputs,
    (ep) => `set:Capture:${ep.id}`,
    "No input devices found.",
    () => buildVisibilityCard(onRefresh),
    updateVisibilityCard
  );
}
