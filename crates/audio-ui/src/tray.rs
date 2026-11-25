use std::sync::{Arc, Mutex};

use audio_core::ipc::{IpcRequest, IpcResponse, PresetSummary};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager};

use crate::daemon_ipc::ipc_call;
use crate::window_state::{
    capture_window_state, save_window_state, show_main_window, window_state_path, WindowState,
};

pub fn build_tray_menu_with_presets(
    app: &AppHandle,
    preset_data: Option<(Option<String>, Vec<PresetSummary>)>,
) -> tauri::Result<Menu<tauri::Wry>> {
    let open_item = MenuItem::with_id(app, "open", "Open WinSoundDeck", true, None::<&str>)?;
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

pub fn refresh_tray_menu(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        if let Ok(menu) = build_tray_menu(app) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

pub fn setup_tray(app: &mut App, win_state: Arc<Mutex<Option<WindowState>>>) -> tauri::Result<()> {
    let menu = build_tray_menu_with_presets(app.handle(), None)?;
    let mut tray_builder = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .tooltip("WinSoundDeck");
    if let Some(icon) = app.default_window_icon().cloned() {
        tray_builder = tray_builder.icon(icon);
    }
    tray_builder
        .on_menu_event(move |app, event| {
            let id = event.id.as_ref();
            if id == "open" {
                show_main_window(app);
            } else if id == "quit" {
                if let Some(win) = app.get_webview_window("main") {
                    if let Some(st) = capture_window_state(&win) {
                        save_window_state(&window_state_path(), &st);
                    } else if let Ok(guard) = win_state.lock() {
                        if let Some(st) = *guard {
                            save_window_state(&window_state_path(), &st);
                        }
                    }
                }
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
    Ok(())
}
