#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::time::Duration;

use audio_core::ipc::{
    install_ctrl_c_daemon_shutdown, send_ipc_request, DaemonStatus, IpcRequest, IpcResponse,
    PeakLevels, PresetSummary, DEFAULT_PIPE_NAME,
};
use audio_core::{DataFlow, DeviceRole};
use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize)]
struct PresetsList {
    active_preset_id: Option<String>,
    presets: Vec<PresetSummary>,
}

fn ipc_call(req: IpcRequest) -> Result<IpcResponse, String> {
    match send_ipc_request(DEFAULT_PIPE_NAME, &req).map_err(|e| e.to_string())? {
        IpcResponse::Error { message } => Err(message),
        other => Ok(other),
    }
}

fn ensure_daemon_running() {
    if matches!(
        send_ipc_request(DEFAULT_PIPE_NAME, &IpcRequest::Ping),
        Ok(IpcResponse::Pong)
    ) {
        return;
    }

    let daemon_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("audio-daemon.exe")))
        .unwrap_or_else(|| "audio-daemon.exe".into());

    if Command::new(daemon_path)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .is_ok()
    {
        for _ in 0..20 {
            if matches!(
                send_ipc_request(DEFAULT_PIPE_NAME, &IpcRequest::Ping),
                Ok(IpcResponse::Pong)
            ) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[tauri::command]
fn get_status() -> Result<DaemonStatus, String> {
    match ipc_call(IpcRequest::GetStatus)? {
        IpcResponse::Status(status) => Ok(status),
        _ => Err("Unexpected response to GetStatus".to_string()),
    }
}

#[tauri::command]
fn get_peaks() -> Result<PeakLevels, String> {
    match ipc_call(IpcRequest::GetPeaks)? {
        IpcResponse::Peaks(p) => Ok(p),
        other => Err(format!("Unexpected response: {other:?}")),
    }
}

#[tauri::command]
fn list_presets() -> Result<PresetsList, String> {
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
fn apply_preset(app: AppHandle, id: String) -> Result<(), String> {
    ipc_call(IpcRequest::ApplyPreset { id })?;
    refresh_tray_menu(&app);
    Ok(())
}

#[tauri::command]
fn save_current_as_preset(app: AppHandle, name: String) -> Result<(), String> {
    ipc_call(IpcRequest::SaveCurrentAsPreset { name })?;
    refresh_tray_menu(&app);
    Ok(())
}

#[tauri::command]
fn delete_preset(app: AppHandle, id: String) -> Result<(), String> {
    ipc_call(IpcRequest::DeletePreset { id })?;
    refresh_tray_menu(&app);
    Ok(())
}

#[tauri::command]
fn clear_active_preset(app: AppHandle) -> Result<(), String> {
    ipc_call(IpcRequest::ClearActivePreset)?;
    refresh_tray_menu(&app);
    Ok(())
}

#[tauri::command]
fn set_endpoint_volume(flow: DataFlow, endpoint_id: String, volume: f32) -> Result<(), String> {
    ipc_call(IpcRequest::SetEndpointVolume {
        flow,
        endpoint_id,
        volume,
    })?;
    Ok(())
}

#[tauri::command]
fn set_endpoint_mute(flow: DataFlow, endpoint_id: String, muted: bool) -> Result<(), String> {
    ipc_call(IpcRequest::SetEndpointMute {
        flow,
        endpoint_id,
        muted,
    })?;
    Ok(())
}

#[tauri::command]
fn set_session_mute(
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
fn route_app(
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
fn start_loopback(
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
fn stop_loopback(
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
fn set_app_output_volume(
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
fn set_app_pinned(pid: u32, app_key: Option<String>, pinned: bool) -> Result<(), String> {
    ipc_call(IpcRequest::SetAppPinned {
        pid,
        app_key,
        pinned,
    })?;
    Ok(())
}

#[tauri::command]
fn clear_unpinned_apps() -> Result<(), String> {
    ipc_call(IpcRequest::ClearUnpinnedApps)?;
    Ok(())
}

#[tauri::command]
fn set_endpoint_allowed(endpoint_id: String, allowed: bool) -> Result<(), String> {
    ipc_call(IpcRequest::SetEndpointAllowed {
        endpoint_id,
        allowed,
    })?;
    Ok(())
}

#[tauri::command]
fn forget_endpoint(endpoint_id: String) -> Result<(), String> {
    ipc_call(IpcRequest::ForgetEndpoint { endpoint_id })?;
    Ok(())
}

fn pnp_instance_id_for_endpoint(endpoint_id: &str) -> Result<String, String> {
    let valid = !endpoint_id.is_empty()
        && endpoint_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '{' | '}' | '.' | '-'));
    if !valid {
        return Err(format!("Invalid endpoint ID: '{endpoint_id}'"));
    }
    Ok(format!("SWD\\MMDEVAPI\\{endpoint_id}"))
}

#[tauri::command]
fn remove_windows_endpoint(endpoint_id: String) -> Result<(), String> {
    let instance_id = pnp_instance_id_for_endpoint(&endpoint_id)?;
    let script = format!(
        "$p = Start-Process -FilePath 'pnputil.exe' -ArgumentList @('/remove-device', '{instance_id}') -Verb RunAs -Wait -PassThru; exit $p.ExitCode"
    );
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("Failed to launch elevated pnputil: {e}"))?;
    if !status.success() {
        return Err(format!(
            "pnputil /remove-device exited with status {}",
            status.code().unwrap_or(-1)
        ));
    }
    ipc_call(IpcRequest::ForgetEndpoint { endpoint_id })?;
    Ok(())
}

#[tauri::command]
fn set_default_endpoint(endpoint_id: String, role: DeviceRole) -> Result<(), String> {
    ipc_call(IpcRequest::SetDefaultEndpoint { endpoint_id, role })?;
    Ok(())
}

#[tauri::command]
fn set_include_disconnected(include: bool) -> Result<(), String> {
    ipc_call(IpcRequest::SetIncludeDisconnected { include })?;
    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

fn build_tray_icon() -> tauri::image::Image<'static> {
    let mut rgba = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32u32 {
        for x in 0..32u32 {
            let in_bar1 = (6..10).contains(&x) && (12..26).contains(&y);
            let in_bar2 = (14..18).contains(&x) && (6..26).contains(&y);
            let in_bar3 = (22..26).contains(&x) && (10..26).contains(&y);
            if in_bar1 || in_bar2 || in_bar3 {
                rgba.extend_from_slice(&[99, 102, 241, 255]);
            } else {
                rgba.extend_from_slice(&[24, 24, 32, 255]);
            }
        }
    }
    tauri::image::Image::new_owned(rgba, 32, 32)
}

fn build_tray_menu_with_presets(
    app: &AppHandle,
    preset_data: Option<(Option<String>, Vec<PresetSummary>)>,
) -> tauri::Result<Menu<tauri::Wry>> {
    let open_item = MenuItem::with_id(app, "open", "Open Mixer", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let mut items: Vec<Box<dyn tauri::menu::IsMenuItem<tauri::Wry>>> = vec![Box::new(open_item)];

    if let Some((active_preset_id, presets)) = preset_data {
        if !presets.is_empty() {
            items.push(Box::new(PredefinedMenuItem::separator(app)?));
            for p in presets {
                let is_active = active_preset_id.as_deref() == Some(p.id.as_str());
                let label = if is_active {
                    format!("✓ {}", p.name)
                } else {
                    p.name
                };
                let item = MenuItem::with_id(
                    app,
                    format!("preset:{}", p.id),
                    label,
                    true,
                    None::<&str>,
                )?;
                items.push(Box::new(item));
            }
        }
    }

    items.push(Box::new(PredefinedMenuItem::separator(app)?));
    items.push(Box::new(quit_item));

    let refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> =
        items.iter().map(|b| b.as_ref()).collect();
    Menu::with_items(app, &refs)
}

fn build_tray_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let preset_data = match ipc_call(IpcRequest::ListPresets) {
        Ok(IpcResponse::Presets {
            active_preset_id,
            presets,
        }) => Some((active_preset_id, presets)),
        _ => None,
    };
    build_tray_menu_with_presets(app, preset_data)
}

fn refresh_tray_menu(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        if let Ok(menu) = build_tray_menu(app) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

fn main() {
    audio_core::logging::init_logging("audio-ui");
    audio_core::log_info!("Starting audio-ui");
    install_ctrl_c_daemon_shutdown();

    tauri::Builder::default()
        .setup(|app| {
            // Keep Tauri's STA setup hook non-blocking; spawn daemon & populate presets off-thread.
            let menu = build_tray_menu_with_presets(app.handle(), None)?;

            TrayIconBuilder::with_id("main-tray")
                .icon(build_tray_icon())
                .menu(&menu)
                .tooltip("Audio Mixer")
                .on_menu_event(|app, event| {
                    let id = event.id.as_ref();
                    if id == "open" {
                        show_main_window(app);
                    } else if id == "quit" {
                        let _ = ipc_call(IpcRequest::Shutdown);
                        app.exit(0);
                    } else if let Some(preset_id) = id.strip_prefix("preset:") {
                        let preset_id = preset_id.to_string();
                        let handle = app.clone();
                        std::thread::spawn(move || {
                            let _ = ipc_call(IpcRequest::ApplyPreset { id: preset_id });
                            refresh_tray_menu(&handle);
                        });
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            let handle = app.handle().clone();
            std::thread::spawn(move || {
                ensure_daemon_running();
                refresh_tray_menu(&handle);
            });

            show_main_window(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_peaks,
            list_presets,
            apply_preset,
            save_current_as_preset,
            delete_preset,
            clear_active_preset,
            set_endpoint_volume,
            set_endpoint_mute,
            set_session_mute,
            route_app,
            start_loopback,
            stop_loopback,
            set_app_output_volume,
            set_app_pinned,
            clear_unpinned_apps,
            set_endpoint_allowed,
            forget_endpoint,
            remove_windows_endpoint,
            set_default_endpoint,
            set_include_disconnected,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::pnp_instance_id_for_endpoint;

    #[test]
    fn pnp_instance_id_formats_valid_endpoint_and_rejects_injection() {
        let valid = "{0.0.0.00000000}.{2d70e42c-d838-48f4-966d-8623cd910f4f}";
        assert_eq!(
            pnp_instance_id_for_endpoint(valid).unwrap(),
            format!("SWD\\MMDEVAPI\\{valid}")
        );
        assert!(pnp_instance_id_for_endpoint("").is_err());
        assert!(pnp_instance_id_for_endpoint("foo'; calc.exe").is_err());
    }
}
