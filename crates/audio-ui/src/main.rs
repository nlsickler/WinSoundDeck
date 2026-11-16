#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::time::Duration;

use audio_core::ipc::{
    send_ipc_request, DaemonStatus, IpcRequest, IpcResponse, PresetSummary, DEFAULT_PIPE_NAME,
};
use audio_core::DataFlow;
use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};

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
fn apply_preset(id: String) -> Result<(), String> {
    ipc_call(IpcRequest::ApplyPreset { id })?;
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
fn set_session_volume(endpoint_id: String, pid: u32, volume: f32) -> Result<(), String> {
    ipc_call(IpcRequest::SetSessionVolume {
        endpoint_id,
        pid,
        volume,
    })?;
    Ok(())
}

#[tauri::command]
fn set_session_mute(endpoint_id: String, pid: u32, muted: bool) -> Result<(), String> {
    ipc_call(IpcRequest::SetSessionMute {
        endpoint_id,
        pid,
        muted,
    })?;
    Ok(())
}

#[tauri::command]
fn route_app(pid: u32, flow: DataFlow, endpoint_id: String) -> Result<(), String> {
    ipc_call(IpcRequest::RouteApp {
        pid,
        flow,
        endpoint_id,
    })?;
    Ok(())
}

#[tauri::command]
fn reset_app_route(pid: u32, flow: DataFlow) -> Result<(), String> {
    ipc_call(IpcRequest::ResetAppRoute { pid, flow })?;
    Ok(())
}

#[tauri::command]
fn start_loopback(pid: u32, target_endpoint_id: String, volume: f32) -> Result<(), String> {
    ipc_call(IpcRequest::StartLoopback {
        pid,
        target_endpoint_id,
        volume,
    })?;
    Ok(())
}

#[tauri::command]
fn set_loopback_volume(pid: u32, target_endpoint_id: String, volume: f32) -> Result<(), String> {
    ipc_call(IpcRequest::SetLoopbackVolume {
        pid,
        target_endpoint_id,
        volume,
    })?;
    Ok(())
}

#[tauri::command]
fn set_loopback_mute(pid: u32, target_endpoint_id: String, muted: bool) -> Result<(), String> {
    ipc_call(IpcRequest::SetLoopbackMute {
        pid,
        target_endpoint_id,
        muted,
    })?;
    Ok(())
}

#[tauri::command]
fn stop_loopback(pid: u32, target_endpoint_id: String) -> Result<(), String> {
    ipc_call(IpcRequest::StopLoopback {
        pid,
        target_endpoint_id,
    })?;
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

fn main() {
    ensure_daemon_running();

    tauri::Builder::default()
        .setup(|app| {
            let open_item = MenuItem::with_id(app, "open", "Open Mixer", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_item, &quit_item])?;

            TrayIconBuilder::new()
                .icon(build_tray_icon())
                .menu(&menu)
                .tooltip("Audio Mixer")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
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
            list_presets,
            apply_preset,
            set_endpoint_volume,
            set_endpoint_mute,
            set_session_volume,
            set_session_mute,
            route_app,
            reset_app_route,
            start_loopback,
            set_loopback_volume,
            set_loopback_mute,
            stop_loopback,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
