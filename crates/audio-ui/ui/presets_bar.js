import { syncKeyedList } from "./dom.js";

const { invoke } = window.__TAURI__.core;

let currentActivePresetId = null;

export function setupPresetsBar(onRefresh) {
  const actionBtn = document.getElementById("preset-new-btn");
  const form = document.getElementById("preset-save-form");
  const input = document.getElementById("preset-name-input");
  const cancelBtn = document.getElementById("preset-cancel-btn");

  const closeForm = () => {
    form.classList.add("collapsed");
    actionBtn.style.display = "";
    input.value = "";
  };

  actionBtn.onclick = async () => {
    if (currentActivePresetId) {
      await invoke("delete_preset", { id: currentActivePresetId });
      await onRefresh(true);
      return;
    }
    actionBtn.style.display = "none";
    form.classList.remove("collapsed");
    input.focus();
  };

  cancelBtn.onclick = closeForm;

  form.onsubmit = async (e) => {
    e.preventDefault();
    const name = input.value.trim();
    if (!name) return;
    await invoke("save_current_as_preset", { name });
    closeForm();
    await onRefresh(true);
  };
}

export function renderPresets({ active_preset_id, presets }, onRefresh) {
  currentActivePresetId = active_preset_id || null;

  const actionBtn = document.getElementById("preset-new-btn");
  if (currentActivePresetId) {
    actionBtn.textContent = "Delete";
    actionBtn.className = "btn-danger";
    actionBtn.title = "Delete active preset";
  } else {
    actionBtn.textContent = "+ Save";
    actionBtn.className = "btn-secondary";
    actionBtn.title = "Save current state as a preset";
  }

  const el = document.getElementById("presets-list");
  syncKeyedList(
    el,
    presets || [],
    (p) => p.id,
    "No presets saved",
    () => {
      const pill = document.createElement("div");
      pill.className = "preset-pill";

      const applyBtn = document.createElement("button");
      applyBtn.className = "preset-apply-btn";
      pill.append(applyBtn);

      applyBtn.onclick = async () => {
        if (pill.classList.contains("active")) {
          await invoke("clear_active_preset");
        } else {
          await invoke("apply_preset", { id: pill.dataset.presetId });
        }
        await onRefresh(true);
      };

      return pill;
    },
    (pill, p) => {
      pill.dataset.presetId = p.id;
      const isActive = p.id === active_preset_id;
      pill.classList.toggle("active", isActive);
      const applyBtn = pill.querySelector(".preset-apply-btn");
      applyBtn.textContent = p.name;
      applyBtn.title = isActive
        ? "Click to deactivate and restore previous settings"
        : `Apply "${p.name}"`;
    }
  );
}
