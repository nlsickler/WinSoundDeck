import { isSliderDragging, pct, setSliderDragging, syncKeyedList } from "./dom.js";

const { invoke } = window.__TAURI__.core;

function buildEndpointCard(onRefresh) {
  const card = document.createElement("div");
  card.className = "card";

  const header = document.createElement("div");
  header.className = "card-header";
  const title = document.createElement("span");
  title.className = "card-title";
  const meta = document.createElement("div");
  meta.className = "card-meta";
  const defaultBtn = document.createElement("button");
  defaultBtn.className = "btn-role btn-role-default";
  defaultBtn.textContent = "Default";
  defaultBtn.title = "Set as OS Default device (Multimedia & Console)";
  defaultBtn.onclick = async () => {
    const ep = card._item;
    if (!ep.is_default_multimedia) {
      await invoke("set_default_endpoint", {
        endpointId: ep.id,
        role: "Multimedia",
      });
      await onRefresh(true);
    }
  };
  const commsBtn = document.createElement("button");
  commsBtn.className = "btn-role btn-role-comms";
  commsBtn.textContent = "Comms";
  commsBtn.title = "Set as OS Communications Default device";
  commsBtn.onclick = async () => {
    const ep = card._item;
    if (!ep.is_default_communications) {
      await invoke("set_default_endpoint", {
        endpointId: ep.id,
        role: "Communications",
      });
      await onRefresh(true);
    }
  };
  const eyeBtn = document.createElement("button");
  eyeBtn.className = "btn-icon-eye";
  eyeBtn.textContent = "👁️";
  eyeBtn.onclick = async () => {
    const ep = card._item;
    await invoke("set_endpoint_hidden", {
      endpointId: ep.id,
      hidden: !ep.hidden,
    });
    await onRefresh(true);
  };
  meta.append(defaultBtn, commsBtn, eyeBtn);
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
    await onRefresh(true);
  };

  slider.onpointerdown = () => setSliderDragging(true);
  slider.onpointerup = () => setSliderDragging(false);
  slider.onpointercancel = () => setSliderDragging(false);
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
  const disconnected = ep.is_connected === false;
  card.classList.toggle("inactive-card", disconnected);
  card.querySelector(".card-title").textContent = disconnected
    ? `${ep.friendly_name} (Offline)`
    : ep.friendly_name;

  const defaultBtn = card.querySelector(".btn-role-default");
  defaultBtn.classList.toggle("active", Boolean(ep.is_default_multimedia));
  defaultBtn.disabled = disconnected;

  const commsBtn = card.querySelector(".btn-role-comms");
  commsBtn.classList.toggle("active", Boolean(ep.is_default_communications));
  commsBtn.disabled = disconnected;

  const eyeBtn = card.querySelector(".btn-icon-eye");
  eyeBtn.style.display = ep.flow === "Render" ? "" : "none";
  eyeBtn.classList.toggle("hidden-ep", Boolean(ep.hidden));
  eyeBtn.title = ep.hidden ? "Show in Applications tab" : "Hide from Applications tab";

  const muteBtn = card.querySelector(".btn-mute");
  muteBtn.classList.toggle("muted", ep.muted);
  muteBtn.textContent = ep.muted ? "Muted" : "Mute";
  muteBtn.disabled = disconnected;

  const pctVal = pct(ep.volume);
  const slider = card.querySelector('input[type="range"]');
  slider.disabled = disconnected;
  if (!isSliderDragging() && Number(slider.value) !== pctVal) {
    slider.value = String(pctVal);
  }
  card.querySelector(".vol-label").textContent = disconnected ? "--" : `${pctVal}%`;
}

export function renderIOTab(endpoints, onRefresh) {
  const outputs = endpoints.filter((e) => e.flow === "Render");
  const inputs = endpoints.filter((e) => e.flow === "Capture");
  syncKeyedList(
    document.getElementById("outputs-list"),
    outputs,
    (ep) => `Render:${ep.id}`,
    "No active output devices.",
    () => buildEndpointCard(onRefresh),
    updateEndpointCard
  );
  syncKeyedList(
    document.getElementById("inputs-list"),
    inputs,
    (ep) => `Capture:${ep.id}`,
    "No active input devices.",
    () => buildEndpointCard(onRefresh),
    updateEndpointCard
  );
}
