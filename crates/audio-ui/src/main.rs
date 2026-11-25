#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod daemon_ipc;
mod diagnostics;
mod tray;
mod window_state;
mod windows_shell;

use std::sync::{Arc, Mutex};

use audio_core::ipc::install_ctrl_c_daemon_shutdown;
use audio_core::SingleInstanceGuard;
use tauri::{Manager, WindowEvent};

use commands::AppIconCache;
use daemon_ipc::ensure_daemon_running;
use tray::{refresh_tray_menu, setup_tray};
use window_state::{
    capture_window_state, flush_window_state, restore_window_state, show_main_window, WindowState,
};

fn main() {
    #[cfg(feature = "portable")]
    if std::env::args().any(|a| a == "--daemon") {
        audio_daemon::run_daemon();
        return;
    }

    audio_core::logging::init_logging("audio-ui");
    let _instance = match SingleInstanceGuard::acquire("Local\\WinSoundDeckUI") {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            audio_core::log_info!("WinSoundDeck UI is already running; exiting");
            return;
        }
        Err(err) => {
            audio_core::log_error!("Failed to acquire UI single-instance mutex: {err}");
            return;
        }
    };
    audio_core::log_info!("Starting WinSoundDeck UI");
    install_ctrl_c_daemon_shutdown();

    let last_window_state = Arc::new(Mutex::new(None::<WindowState>));
    let setup_win_state = Arc::clone(&last_window_state);
    let event_win_state = Arc::clone(&last_window_state);

    tauri::Builder::default()
        .manage(AppIconCache::default())
        .setup(move |app| {
            if let Some(win) = app.get_webview_window("main") {
                restore_window_state(&win);
            }

            // Keep Tauri's STA setup hook non-blocking; spawn daemon & populate presets off-thread.
            setup_tray(app, setup_win_state)?;

            let handle = app.handle().clone();
            std::thread::spawn(move || {
                ensure_daemon_running();
                refresh_tray_menu(&handle);
            });

            show_main_window(app.handle());
            Ok(())
        })
        .on_window_event(move |window, event| match event {
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                if let Some(win) = window.app_handle().get_webview_window("main") {
                    if let Some(st) = capture_window_state(&win) {
                        if let Ok(mut guard) = event_win_state.lock() {
                            *guard = Some(st);
                        }
                    }
                }
            }
            WindowEvent::Focused(false) => flush_window_state(&event_win_state),
            WindowEvent::CloseRequested { api, .. } => {
                flush_window_state(&event_win_state);
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_peaks,
            commands::get_app_icons,
            commands::list_presets,
            commands::apply_preset,
            commands::save_current_as_preset,
            commands::delete_preset,
            commands::rename_preset,
            commands::reorder_presets,
            commands::clear_active_preset,
            commands::set_endpoint_volume,
            commands::set_endpoint_mute,
            commands::set_session_mute,
            commands::route_app,
            commands::start_loopback,
            commands::stop_loopback,
            commands::set_app_output_volume,
            commands::set_app_pinned,
            commands::clear_unpinned_apps,
            commands::set_endpoint_allowed,
            commands::forget_endpoint,
            commands::set_default_endpoint,
            commands::set_include_disconnected,
            commands::open_devtools,
            windows_shell::remove_windows_endpoint,
            windows_shell::open_bluetooth_settings,
            windows_shell::open_sound_properties,
            windows_shell::get_launch_at_login,
            windows_shell::set_launch_at_login,
            diagnostics::run_troubleshoot_to_clipboard,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
