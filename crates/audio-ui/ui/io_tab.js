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
  meta.append(defaultBtn, commsBtn);
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

  const meter = document.createElement("div");
  meter.className = "peak-meter";
  const meterFill = document.createElement("div");
  meterFill.className = "peak-meter-fill";
  meter.append(meterFill);

  card.append(header, row, meter);

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
  const meterFill = card.querySelector(".peak-meter-fill");
  if (meterFill) {
    meterFill.dataset.peakEndpoint = disconnected ? "" : ep.id;
    if (disconnected) meterFill.style.width = "0%";
  }
  card.querySelector(".card-title").textContent = disconnected
    ? `${ep.friendly_name} (Offline)`
    : ep.friendly_name;

  const defaultBtn = card.querySelector(".btn-role-default");
  defaultBtn.classList.toggle("active", Boolean(ep.is_default_multimedia));
  defaultBtn.disabled = disconnected;

  const commsBtn = card.querySelector(".btn-role-comms");
  commsBtn.classList.toggle("active", Boolean(ep.is_default_communications));
  commsBtn.disabled = disconnected;

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

export function renderIOTab(endpoints, includeDisconnected, onRefresh) {
  const visible = endpoints.filter(
    (e) => e.is_allowed && (includeDisconnected || e.is_connected)
  );
  const outputs = visible.filter((e) => e.flow === "Render");
  const inputs = visible.filter((e) => e.flow === "Capture");
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
