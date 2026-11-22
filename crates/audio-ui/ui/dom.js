let draggingSlider = false;

export function pct(vol) {
  return Math.round(Math.max(0, Math.min(1, vol)) * 100);
}

export function isSliderDragging() {
  return draggingSlider;
}

export function setSliderDragging(val) {
  draggingSlider = val;
}

export function isUserInteracting() {
  if (draggingSlider) return true;
  const active = document.activeElement;
  if (!active) return false;
  return (
    active.tagName === "SELECT" ||
    (active.tagName === "INPUT" && active.type === "text")
  );
}

let activeContextMenu = null;

export function closeContextMenu() {
  if (activeContextMenu) {
    activeContextMenu.remove();
    activeContextMenu = null;
  }
}

document.addEventListener("click", (e) => {
  if (activeContextMenu && !activeContextMenu.contains(e.target)) {
    closeContextMenu();
  }
});

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") closeContextMenu();
});

window.addEventListener("blur", closeContextMenu);

export function openContextMenu(e, items) {
  e.preventDefault();
  e.stopPropagation();
  closeContextMenu();
  if (!items || items.length === 0) return;

  const menu = document.createElement("div");
  menu.className = "strip-context-menu";

  for (const item of items) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "strip-context-item";
    btn.textContent = item.label;
    btn.disabled = Boolean(item.disabled);
    if (!item.disabled && item.onClick) {
      btn.onclick = async () => {
        closeContextMenu();
        await item.onClick();
      };
    }
    menu.append(btn);
  }

  document.body.appendChild(menu);
  const rect = menu.getBoundingClientRect();
  const left = Math.min(e.clientX, Math.max(4, window.innerWidth - rect.width - 8));
  const top = Math.min(e.clientY, Math.max(4, window.innerHeight - rect.height - 8));
  menu.style.left = `${left}px`;
  menu.style.top = `${top}px`;
  activeContextMenu = menu;
}

export function openEndpointContextMenu(e, ep, invoke, onRefresh) {
  if (!ep) return;
  openContextMenu(e, [
    {
      label: "⚙️ Windows Sound Properties",
      onClick: () => invoke("open_sound_properties", { endpointId: ep.id }),
    },
    {
      label: "👁️ Hide",
      onClick: async () => {
        await invoke("set_endpoint_allowed", {
          endpointId: ep.id,
          allowed: false,
        });
        await onRefresh(true);
      },
    },
  ]);
}

export function syncMuteIconButton(btn, muted, disabled = false, unmutedIcon = "🔊") {
  btn.classList.toggle("muted", Boolean(muted));
  btn.textContent = muted ? "🔇" : unmutedIcon;
  btn.title = muted ? "Unmute" : "Mute";
  btn.disabled = Boolean(disabled);
}

export function createMeterSlider() {
  const wrap = document.createElement("div");
  wrap.className = "slider-meter-wrap";
  const rail = document.createElement("div");
  rail.className = "slider-rail";
  const volFill = document.createElement("div");
  volFill.className = "slider-vol-fill";
  const meterFill = document.createElement("div");
  meterFill.className = "slider-peak-fill";
  rail.append(volFill, meterFill);
  const slider = document.createElement("input");
  slider.type = "range";
  slider.min = "0";
  slider.max = "100";
  wrap.append(rail, slider);
  return { wrap, slider, volFill, meterFill };
}

const PREF_ACTIVE_TAB = "winsounddeck.ui.active_tab";

export function loadPref(key, fallback = null) {
  try {
    const raw = window.localStorage.getItem(key);
    return raw === null ? fallback : raw;
  } catch {
    return fallback;
  }
}

export function savePref(key, value) {
  try {
    window.localStorage.setItem(key, String(value));
  } catch {
    // Ignore storage quota / privacy errors
  }
}

export function loadBoolPref(key, fallback = false) {
  const raw = loadPref(key, null);
  return raw === null ? fallback : raw === "true";
}

export function saveBoolPref(key, value) {
  savePref(key, Boolean(value));
}

export function setupTabs(onTabChange) {
  const buttons = Array.from(document.querySelectorAll(".tab-btn"));
  const panels = Array.from(document.querySelectorAll(".tab-panel"));

  const activateTab = (tab) => {
    const valid = buttons.some((b) => b.dataset.tab === tab) ? tab : "apps";
    for (const b of buttons) {
      const selected = b.dataset.tab === valid;
      b.classList.toggle("active", selected);
      b.setAttribute("aria-selected", String(selected));
    }
    for (const p of panels) {
      p.classList.toggle("active", p.id === `tab-${valid}`);
    }
    if (onTabChange) onTabChange(valid);
    return valid;
  };

  const savedTab = loadPref(PREF_ACTIVE_TAB, "apps");
  activateTab(savedTab);

  for (const btn of buttons) {
    btn.onclick = () => {
      const active = activateTab(btn.dataset.tab);
      savePref(PREF_ACTIVE_TAB, active);
    };
  }
}

/**
 * Reconciles a container's children against `items` by stable string key,
 * preserving existing DOM elements (sliders, scroll, focus) in-place.
 */
export function syncKeyedList(container, items, keyFn, emptyText, createFn, updateFn) {
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
