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

export function setupTabs() {
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
