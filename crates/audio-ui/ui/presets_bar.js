import { syncKeyedList } from "./dom.js";

const { invoke } = window.__TAURI__.core;

export function renderPresets({ active_preset_id, presets }, onRefresh) {
  const el = document.getElementById("presets-list");
  syncKeyedList(
    el,
    presets || [],
    (p) => p.id,
    "No presets saved yet",
    () => {
      const btn = document.createElement("button");
      btn.className = "preset-btn";
      btn.onclick = async () => {
        await invoke("apply_preset", { id: btn.dataset.presetId });
        await onRefresh(true);
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
