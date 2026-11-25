import { openContextMenu } from "./dom.js";
import { appIdFor } from "./app_utils.js";

const { invoke } = window.__TAURI__.core;

const explicitlyUncheckedIds = new Set();
export const rememberedOutputVolumes = new Map();

export function isPrimaryUnchecked(appId) {
  return explicitlyUncheckedIds.has(appId);
}

export function clearPrimaryUnchecked(appId) {
  explicitlyUncheckedIds.delete(appId);
}

export async function toggleRouting(ctx, nextEnabled, onRefresh) {
  if (!ctx) return;
  const { session, endpoint, isPrimary, isLoopback, activeLoopbacks } = ctx;
  const appId = appIdFor(session);
  const appKey = session.app_key || null;
  if (isPrimary) {
    if (nextEnabled) {
      explicitlyUncheckedIds.delete(appId);
      await invoke("set_session_mute", {
        endpointId: session.endpoint_id,
        pid: session.pid,
        appKey,
        muted: false,
      });
    } else {
      const fallback = activeLoopbacks.find((lb) => lb.target_endpoint_id !== endpoint.id);
      if (fallback) {
        explicitlyUncheckedIds.delete(appId);
        const fallbackVol = fallback.volume;
        await invoke("route_app", {
          pid: session.pid,
          appKey,
          flow: "Render",
          endpointId: fallback.target_endpoint_id,
        });
        await new Promise((r) => setTimeout(r, 250));
        await invoke("stop_loopback", {
          pid: session.pid,
          appKey,
          targetEndpointId: fallback.target_endpoint_id,
        });
        await invoke("set_app_output_volume", {
          pid: session.pid,
          appKey,
          endpointId: fallback.target_endpoint_id,
          volume: fallbackVol,
          finalize: true,
        });
      } else {
        explicitlyUncheckedIds.add(appId);
        await invoke("set_session_mute", {
          endpointId: session.endpoint_id,
          pid: session.pid,
          appKey,
          muted: true,
        });
      }
    }
  } else if (nextEnabled && isLoopback && session.muted) {
    await invoke("set_session_mute", {
      endpointId: session.endpoint_id,
      pid: session.pid,
      appKey,
      muted: false,
    });
  } else if (nextEnabled && !isLoopback) {
    const remembered =
      (session.saved_volumes && session.saved_volumes[endpoint.id]) ??
      rememberedOutputVolumes.get(`${appId}:${endpoint.id}`) ??
      0.5;
    if (explicitlyUncheckedIds.has(appId) && activeLoopbacks.length === 0) {
      explicitlyUncheckedIds.delete(appId);
      await invoke("route_app", {
        pid: session.pid,
        appKey,
        flow: "Render",
        endpointId: endpoint.id,
      });
      await new Promise((r) => setTimeout(r, 250));
      await invoke("set_session_mute", {
        endpointId: endpoint.id,
        pid: session.pid,
        appKey,
        muted: false,
      });
      await invoke("set_app_output_volume", {
        pid: session.pid,
        appKey,
        endpointId: endpoint.id,
        volume: remembered,
        finalize: true,
      });
    } else {
      await invoke("start_loopback", {
        pid: session.pid,
        appKey,
        targetEndpointId: endpoint.id,
        volume: remembered,
      });
      await invoke("set_app_output_volume", {
        pid: session.pid,
        appKey,
        endpointId: endpoint.id,
        volume: remembered,
        finalize: true,
      });
    }
  } else if (!nextEnabled && isLoopback) {
    await invoke("stop_loopback", {
      pid: session.pid,
      appKey,
      targetEndpointId: endpoint.id,
    });
  }
  await onRefresh(true);
}

export async function makePrimaryOutput(ctx, onRefresh) {
  if (!ctx || ctx.isPrimary) return;
  const { session, endpoint, isLoopback, activeLoopbacks } = ctx;
  const appId = appIdFor(session);
  const appKey = session.app_key || null;
  const oldPrimaryId = session.endpoint_id;
  const oldPrimaryVol = session.volume ?? 1.0;
  const keepOldPrimaryAsSecondary =
    Boolean(oldPrimaryId) &&
    oldPrimaryId !== endpoint.id &&
    !session.muted &&
    !explicitlyUncheckedIds.has(appId) &&
    oldPrimaryVol > 0;
  const lb = activeLoopbacks.find((x) => x.target_endpoint_id === endpoint.id);
  const targetVol = lb
    ? lb.volume
    : (session.saved_volumes && session.saved_volumes[endpoint.id]) ??
      rememberedOutputVolumes.get(`${appId}:${endpoint.id}`) ??
      session.volume ??
      0.5;

  explicitlyUncheckedIds.delete(appId);
  await invoke("route_app", {
    pid: session.pid,
    appKey,
    flow: "Render",
    endpointId: endpoint.id,
  });
  if (isLoopback) {
    await new Promise((r) => setTimeout(r, 250));
    await invoke("stop_loopback", {
      pid: session.pid,
      appKey,
      targetEndpointId: endpoint.id,
    });
  } else if (keepOldPrimaryAsSecondary) {
    await new Promise((r) => setTimeout(r, 250));
  }
  await invoke("set_session_mute", {
    endpointId: endpoint.id,
    pid: session.pid,
    appKey,
    muted: false,
  });
  await invoke("set_app_output_volume", {
    pid: session.pid,
    appKey,
    endpointId: endpoint.id,
    volume: targetVol,
    finalize: true,
  });
  if (keepOldPrimaryAsSecondary) {
    rememberedOutputVolumes.set(`${appId}:${oldPrimaryId}`, oldPrimaryVol);
    await invoke("start_loopback", {
      pid: session.pid,
      appKey,
      targetEndpointId: oldPrimaryId,
      volume: oldPrimaryVol,
    });
  }
  await onRefresh(true);
}

export function openStripContextMenu(e, ctx, onRefresh, { includePin = true } = {}) {
  if (!ctx) {
    e.preventDefault();
    return;
  }

  const { session, isPrimary } = ctx;
  const items = [];
  if (includePin) {
    items.push({
      label: session.is_pinned ? "📌 Unpin App" : "📌 Pin App",
      onClick: async () => {
        await invoke("set_app_pinned", {
          pid: session.pid,
          appKey: session.app_key || null,
          pinned: !session.is_pinned,
        });
        await onRefresh(true);
      },
    });
  }

  if (isPrimary) {
    items.push({ label: "✓ Primary Output", disabled: true });
  } else {
    items.push({
      label: "⭐ Set as Primary Output",
      onClick: () => makePrimaryOutput(ctx, onRefresh),
    });
  }

  openContextMenu(e, items);
}
