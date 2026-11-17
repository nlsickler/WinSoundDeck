import { syncKeyedList } from "./dom.js";

const { invoke } = window.__TAURI__.core;

let currentActivePresetId = null;
let currentActivePresetName = null;
let pendingConfirm = null; // "overwrite" | "delete" | null

export function setupPresetsBar(onRefresh) {
  const saveBtn = document.getElementById("preset-save-btn");
  const deleteBtn = document.getElementById("preset-delete-btn");
  const form = document.getElementById("preset-save-form");
  const input = document.getElementById("preset-name-input");
  const cancelBtn = document.getElementById("preset-cancel-btn");
  const confirmBox = document.getElementById("preset-confirm-box");
  const confirmLabel = document.getElementById("preset-confirm-label");
  const confirmYes = document.getElementById("preset-confirm-yes");
  const confirmNo = document.getElementById("preset-confirm-no");

  const resetBarControls = () => {
    pendingConfirm = null;
    form.classList.add("collapsed");
    confirmBox.classList.add("collapsed");
    saveBtn.classList.remove("collapsed");
    deleteBtn.classList.toggle("collapsed", !currentActivePresetId);
    input.value = "";
  };

  saveBtn.onclick = () => {
    if (currentActivePresetId && currentActivePresetName) {
      pendingConfirm = "overwrite";
      saveBtn.classList.add("collapsed");
      deleteBtn.classList.add("collapsed");
      confirmLabel.textContent = `Save "${currentActivePresetName}"?`;
      confirmBox.classList.remove("collapsed");
      return;
    }
    saveBtn.classList.add("collapsed");
    deleteBtn.classList.add("collapsed");
    form.classList.remove("collapsed");
    input.focus();
  };

  deleteBtn.onclick = () => {
    if (!currentActivePresetId) return;
    pendingConfirm = "delete";
    saveBtn.classList.add("collapsed");
    deleteBtn.classList.add("collapsed");
    confirmLabel.textContent = `Delete "${currentActivePresetName || currentActivePresetId}"?`;
    confirmBox.classList.remove("collapsed");
  };

  cancelBtn.onclick = resetBarControls;
  confirmNo.onclick = resetBarControls;

  confirmYes.onclick = async () => {
    const mode = pendingConfirm;
    const id = currentActivePresetId;
    const name = currentActivePresetName;
    resetBarControls();
    if (mode === "overwrite" && name) {
      await invoke("save_current_as_preset", { name });
      await onRefresh(true);
    } else if (mode === "delete" && id) {
      await invoke("delete_preset", { id });
      await onRefresh(true);
    }
  };

  form.onsubmit = async (e) => {
    e.preventDefault();
    const name = input.value.trim();
    if (!name) return;
    await invoke("save_current_as_preset", { name });
    resetBarControls();
    await onRefresh(true);
  };
}

export function renderPresets({ active_preset_id, presets }, onRefresh) {
  currentActivePresetId = active_preset_id || null;
  const activeObj = (presets || []).find((p) => p.id === currentActivePresetId);
  currentActivePresetName = activeObj ? activeObj.name : null;

  const formOpen = !document
    .getElementById("preset-save-form")
    .classList.contains("collapsed");
  const confirmOpen = !document
    .getElementById("preset-confirm-box")
    .classList.contains("collapsed");

  if (!formOpen && !confirmOpen) {
    const saveBtn = document.getElementById("preset-save-btn");
    const deleteBtn = document.getElementById("preset-delete-btn");
    saveBtn.classList.remove("collapsed");
    saveBtn.title = currentActivePresetName
      ? `Overwrite "${currentActivePresetName}" with current state`
      : "Save current state as a new preset";
    deleteBtn.classList.toggle("collapsed", !currentActivePresetId);
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
