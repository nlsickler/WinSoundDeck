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

export function setupTabs() {
  const buttons = document.querySelectorAll(".tab-btn");
  const panels = document.querySelectorAll(".tab-panel");
  for (const btn of buttons) {
    btn.onclick = () => {
      const tab = btn.dataset.tab;
      for (const b of buttons) {
        const selected = b.dataset.tab === tab;
        b.classList.toggle("active", selected);
        b.setAttribute("aria-selected", String(selected));
      }
      for (const p of panels) {
        p.classList.toggle("active", p.id === `tab-${tab}`);
      }
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
