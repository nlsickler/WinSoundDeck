use std::collections::HashMap;
use std::sync::Mutex;

use audio_core::icon::extract_exe_icon_data_url;
use audio_core::ipc::{DaemonStatus, IpcRequest, IpcResponse, PeakLevels, PresetSummary};
use audio_core::{DataFlow, DeviceRole};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::daemon_ipc::ipc_call;
use crate::tray::refresh_tray_menu;

#[derive(Serialize)]
pub struct PresetsList {
    active_preset_id: Option<String>,
    presets: Vec<PresetSummary>,
}

#[derive(Default)]
pub struct AppIconCache(Mutex<HashMap<String, Option<String>>>);

#[tauri::command]
pub fn get_app_icons(
    paths: Vec<String>,
    cache: tauri::State<'_, AppIconCache>,
) -> HashMap<String, String> {
    let mut missing = Vec::new();
    let mut result = HashMap::new();
    if let Ok(guard) = cache.0.lock() {
        for path in &paths {
            match guard.get(path) {
                Some(Some(url)) => {
                    result.insert(path.clone(), url.clone());
                }
                Some(None) => {}
                None => missing.push(path.clone()),
            }
        }
    }

    if !missing.is_empty() {
        let extracted: Vec<(String, Option<String>)> = missing
            .into_iter()
            .map(|p| {
                let url = extract_exe_icon_data_url(&p);
                (p, url)
            })
            .collect();
        if let Ok(mut guard) = cache.0.lock() {
            for (p, url) in extracted {
                if let Some(ref u) = url {
                    result.insert(p.clone(), u.clone());
                }
                guard.insert(p, url);
            }
        }
    }

    result
}

#[tauri::command]
pub fn get_status() -> Result<DaemonStatus, String> {
    match ipc_call(IpcRequest::GetStatus)? {
        IpcResponse::Status(status) => Ok(status),
        _ => Err("Unexpected response to GetStatus".to_string()),
    }
}

#[tauri::command]
pub fn get_peaks() -> Result<PeakLevels, String> {
    match ipc_call(IpcRequest::GetPeaks)? {
        IpcResponse::Peaks(p) => Ok(p),
        other => Err(format!("Unexpected response: {other:?}")),
    }
}

#[tauri::command]
pub fn list_presets() -> Result<PresetsList, String> {
    match ipc_call(IpcRequest::ListPresets)? {
        IpcResponse::Presets {
            active_preset_id,
            presets,
        } => Ok(PresetsList {
            active_preset_id,
            presets,
        }),
        _ => Err("Unexpected response to ListPresets".to_string()),
    }
}

#[tauri::command]
pub fn apply_preset(app: AppHandle, id: String) -> Result<(), String> {
    ipc_call(IpcRequest::ApplyPreset { id })?;
    refresh_tray_menu(&app);
    Ok(())
}

#[tauri::command]
pub fn save_current_as_preset(app: AppHandle, name: String) -> Result<(), String> {
    ipc_call(IpcRequest::SaveCurrentAsPreset { name })?;
    refresh_tray_menu(&app);
    Ok(())
}

#[tauri::command]
pub fn delete_preset(app: AppHandle, id: String) -> Result<(), String> {
    ipc_call(IpcRequest::DeletePreset { id })?;
    refresh_tray_menu(&app);
    Ok(())
}

#[tauri::command]
pub fn rename_preset(app: AppHandle, id: String, new_name: String) -> Result<(), String> {
    ipc_call(IpcRequest::RenamePreset { id, new_name })?;
    refresh_tray_menu(&app);
    Ok(())
}

#[tauri::command]
pub fn reorder_presets(app: AppHandle, ids: Vec<String>) -> Result<(), String> {
    ipc_call(IpcRequest::ReorderPresets { ids })?;
    refresh_tray_menu(&app);
    Ok(())
}

#[tauri::command]
pub fn clear_active_preset(app: AppHandle) -> Result<(), String> {
    ipc_call(IpcRequest::ClearActivePreset)?;
    refresh_tray_menu(&app);
    Ok(())
}

#[tauri::command]
pub fn set_endpoint_volume(flow: DataFlow, endpoint_id: String, volume: f32) -> Result<(), String> {
    ipc_call(IpcRequest::SetEndpointVolume {
        flow,
        endpoint_id,
        volume,
    })?;
    Ok(())
}

#[tauri::command]
pub fn set_endpoint_mute(flow: DataFlow, endpoint_id: String, muted: bool) -> Result<(), String> {
    ipc_call(IpcRequest::SetEndpointMute {
        flow,
        endpoint_id,
        muted,
    })?;
    Ok(())
}

#[tauri::command]
pub fn set_session_mute(
    endpoint_id: String,
    pid: u32,
    app_key: Option<String>,
    muted: bool,
) -> Result<(), String> {
    ipc_call(IpcRequest::SetSessionMute {
        endpoint_id,
        pid,
        app_key,
        muted,
    })?;
    Ok(())
}

#[tauri::command]
pub fn route_app(
    pid: u32,
    app_key: Option<String>,
    flow: DataFlow,
    endpoint_id: String,
) -> Result<(), String> {
    ipc_call(IpcRequest::RouteApp {
        pid,
        app_key,
        flow,
        endpoint_id,
    })?;
    Ok(())
}

#[tauri::command]
pub fn start_loopback(
    pid: u32,
    app_key: Option<String>,
    target_endpoint_id: String,
    volume: f32,
) -> Result<(), String> {
    ipc_call(IpcRequest::StartLoopback {
        pid,
        app_key,
        target_endpoint_id,
        volume,
    })?;
    Ok(())
}

#[tauri::command]
pub fn stop_loopback(
    pid: u32,
    app_key: Option<String>,
    target_endpoint_id: String,
) -> Result<(), String> {
    ipc_call(IpcRequest::StopLoopback {
        pid,
        app_key,
        target_endpoint_id,
    })?;
    Ok(())
}

#[tauri::command]
pub fn set_app_output_volume(
    pid: u32,
    app_key: Option<String>,
    endpoint_id: String,
    volume: f32,
    finalize: bool,
) -> Result<(), String> {
    ipc_call(IpcRequest::SetAppOutputVolume {
        pid,
        app_key,
        endpoint_id,
        volume,
        finalize,
    })?;
    Ok(())
}

#[tauri::command]
pub fn set_app_pinned(pid: u32, app_key: Option<String>, pinned: bool) -> Result<(), String> {
    ipc_call(IpcRequest::SetAppPinned {
        pid,
        app_key,
        pinned,
    })?;
    Ok(())
}

#[tauri::command]
pub fn clear_unpinned_apps() -> Result<(), String> {
    ipc_call(IpcRequest::ClearUnpinnedApps)?;
    Ok(())
}

#[tauri::command]
pub fn set_endpoint_allowed(endpoint_id: String, allowed: bool) -> Result<(), String> {
    ipc_call(IpcRequest::SetEndpointAllowed {
        endpoint_id,
        allowed,
    })?;
    Ok(())
}

#[tauri::command]
pub fn forget_endpoint(endpoint_id: String) -> Result<(), String> {
    ipc_call(IpcRequest::ForgetEndpoint { endpoint_id })?;
    Ok(())
}

#[tauri::command]
pub fn set_default_endpoint(endpoint_id: String, role: DeviceRole) -> Result<(), String> {
    ipc_call(IpcRequest::SetDefaultEndpoint { endpoint_id, role })?;
    Ok(())
}

#[tauri::command]
pub fn set_include_disconnected(include: bool) -> Result<(), String> {
    ipc_call(IpcRequest::SetIncludeDisconnected { include })?;
    Ok(())
}

#[tauri::command]
pub fn open_devtools(app: AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        win.open_devtools();
    }
}
