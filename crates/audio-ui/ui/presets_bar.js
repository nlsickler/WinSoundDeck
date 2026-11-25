import { openContextMenu, syncKeyedList } from "./dom.js";

const { invoke } = window.__TAURI__.core;

let currentActivePresetId = null;
let currentActivePresetName = null;
let currentPresets = [];
let pendingConfirm = null; // "overwrite" | "delete" | null
let refreshHandler = null;

function reorderPresetAt(index, delta) {
  const target = index + delta;
  if (target < 0 || target >= currentPresets.length) return null;
  const ids = currentPresets.map((p) => p.id);
  const [moved] = ids.splice(index, 1);
  ids.splice(target, 0, moved);
  return ids;
}

function renderPresetManagerList(focusPresetId = null) {
  const listEl = document.getElementById("preset-manager-list");
  if (!listEl) return;
  listEl.innerHTML = "";
  if (currentPresets.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    empty.textContent = "No presets saved";
    listEl.append(empty);
    return;
  }

  currentPresets.forEach((p, idx) => {
    const row = document.createElement("div");
    row.className = "preset-manager-row";

    const nameInput = document.createElement("input");
    nameInput.type = "text";
    nameInput.className = "preset-manager-input";
    nameInput.value = p.name;
    nameInput.maxLength = 40;
    nameInput.title = "Press Enter or click away to rename";

    const commitRename = async () => {
      const next = nameInput.value.trim();
      if (!next) {
        nameInput.value = p.name;
        return;
      }
      if (next === p.name) return;
      await invoke("rename_preset", { id: p.id, newName: next });
      if (refreshHandler) await refreshHandler(true);
    };
    nameInput.onkeydown = (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        nameInput.blur();
      } else if (e.key === "Escape") {
        nameInput.value = p.name;
        nameInput.blur();
      }
    };
    nameInput.onblur = commitRename;

    const upBtn = document.createElement("button");
    upBtn.type = "button";
    upBtn.className = "btn-icon-action";
    upBtn.textContent = "↑";
    upBtn.title = "Move earlier";
    upBtn.disabled = idx === 0;
    upBtn.onclick = async () => {
      const ids = reorderPresetAt(idx, -1);
      if (!ids) return;
      await invoke("reorder_presets", { ids });
      if (refreshHandler) await refreshHandler(true);
    };

    const downBtn = document.createElement("button");
    downBtn.type = "button";
    downBtn.className = "btn-icon-action";
    downBtn.textContent = "↓";
    downBtn.title = "Move later";
    downBtn.disabled = idx === currentPresets.length - 1;
    downBtn.onclick = async () => {
      const ids = reorderPresetAt(idx, 1);
      if (!ids) return;
      await invoke("reorder_presets", { ids });
      if (refreshHandler) await refreshHandler(true);
    };

    const delBtn = document.createElement("button");
    delBtn.type = "button";
    delBtn.className = "btn-icon-action btn-danger";
    delBtn.textContent = "🗑️";
    delBtn.title = `Delete "${p.name}"`;
    delBtn.onclick = async () => {
      await invoke("delete_preset", { id: p.id });
      if (refreshHandler) await refreshHandler(true);
    };

    row.append(nameInput, upBtn, downBtn, delBtn);
    listEl.append(row);

    if (focusPresetId && p.id === focusPresetId) {
      requestAnimationFrame(() => {
        nameInput.focus();
        nameInput.select();
      });
    }
  });
}

function openPresetManager(focusPresetId = null) {
  const modal = document.getElementById("preset-manager-modal");
  if (!modal) return;
  modal.classList.remove("collapsed");
  renderPresetManagerList(focusPresetId);
}

function closePresetManager() {
  const modal = document.getElementById("preset-manager-modal");
  if (modal) modal.classList.add("collapsed");
}

export function setupPresetsBar(onRefresh) {
  refreshHandler = onRefresh;
  const manageBtn = document.getElementById("presets-manage-btn");
  const modal = document.getElementById("preset-manager-modal");
  const closeModalBtn = document.getElementById("preset-manager-close-btn");
  const doneModalBtn = document.getElementById("preset-manager-done-btn");
  const saveBtn = document.getElementById("preset-save-btn");
  const deleteBtn = document.getElementById("preset-delete-btn");
  const form = document.getElementById("preset-save-form");
  const input = document.getElementById("preset-name-input");
  const cancelBtn = document.getElementById("preset-cancel-btn");
  const confirmBox = document.getElementById("preset-confirm-box");
  const confirmLabel = document.getElementById("preset-confirm-label");
  const confirmYes = document.getElementById("preset-confirm-yes");
  const confirmNo = document.getElementById("preset-confirm-no");

  if (manageBtn) manageBtn.onclick = () => openPresetManager();
  if (closeModalBtn) closeModalBtn.onclick = closePresetManager;
  if (doneModalBtn) doneModalBtn.onclick = closePresetManager;
  if (modal) {
    modal.onclick = (e) => {
      if (e.target === modal) closePresetManager();
    };
  }
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && modal && !modal.classList.contains("collapsed")) {
      closePresetManager();
    }
  });

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
  refreshHandler = onRefresh;
  currentActivePresetId = active_preset_id || null;
  currentPresets = presets || [];
  const activeObj = currentPresets.find((p) => p.id === currentActivePresetId);
  currentActivePresetName = activeObj ? activeObj.name : null;

  const modal = document.getElementById("preset-manager-modal");
  if (modal && !modal.classList.contains("collapsed")) {
    renderPresetManagerList();
  }

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
    currentPresets,
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

      pill.oncontextmenu = (e) => {
        const id = pill.dataset.presetId;
        const idx = currentPresets.findIndex((x) => x.id === id);
        if (idx === -1) return;
        const preset = currentPresets[idx];
        openContextMenu(e, [
          {
            label: "✏️ Rename",
            onClick: () => openPresetManager(preset.id),
          },
          {
            label: "⬅️ Move Left",
            disabled: idx === 0,
            onClick: async () => {
              const ids = reorderPresetAt(idx, -1);
              if (!ids) return;
              await invoke("reorder_presets", { ids });
              await onRefresh(true);
            },
          },
          {
            label: "➡️ Move Right",
            disabled: idx === currentPresets.length - 1,
            onClick: async () => {
              const ids = reorderPresetAt(idx, 1);
              if (!ids) return;
              await invoke("reorder_presets", { ids });
              await onRefresh(true);
            },
          },
          {
            label: "💾 Overwrite with Current",
            onClick: async () => {
              await invoke("save_current_as_preset", { name: preset.name });
              await onRefresh(true);
            },
          },
          {
            label: "🗑️ Delete",
            onClick: async () => {
              await invoke("delete_preset", { id: preset.id });
              await onRefresh(true);
            },
          },
        ]);
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
