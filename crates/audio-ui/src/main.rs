#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use audio_core::icon::extract_exe_icon_data_url;
use audio_core::ipc::{
    install_ctrl_c_daemon_shutdown, send_ipc_request, DaemonStatus, IpcRequest, IpcResponse,
    PeakLevels, PresetSummary, DEFAULT_PIPE_NAME,
};
use audio_core::{DataFlow, DeviceRole, SingleInstanceGuard};
use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WindowEvent};

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

fn daemon_launch_command(current_exe: Option<&Path>) -> (PathBuf, &'static [&'static str]) {
    #[cfg(feature = "portable")]
    {
        let exe = current_exe
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("audio-ui.exe"));
        (exe, &["--daemon"])
    }
    #[cfg(not(feature = "portable"))]
    {
        let daemon_path = current_exe
            .and_then(|p| p.parent().map(|dir| dir.join("audio-daemon.exe")))
            .unwrap_or_else(|| PathBuf::from("audio-daemon.exe"));
        (daemon_path, &[])
    }
}

fn ensure_daemon_running() {
    if matches!(
        send_ipc_request(DEFAULT_PIPE_NAME, &IpcRequest::Ping),
        Ok(IpcResponse::Pong)
    ) {
        return;
    }

    let current_exe = std::env::current_exe().ok();
    let (daemon_path, args) = daemon_launch_command(current_exe.as_deref());

    if Command::new(daemon_path)
        .args(args)
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

#[derive(Default)]
struct AppIconCache(Mutex<HashMap<String, Option<String>>>);

#[tauri::command]
fn get_app_icons(
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
fn open_bluetooth_settings() -> Result<(), String> {
    Command::new("explorer.exe")
        .arg("ms-settings:bluetooth")
        .spawn()
        .map_err(|e| format!("Failed to open Bluetooth settings: {e}"))?;
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

const RUN_REG_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_REG_VALUE: &str = "WinSoundDeck";

fn format_startup_command(exe_path: &Path) -> String {
    format!("\"{}\"", exe_path.display())
}

#[tauri::command]
fn open_devtools(app: AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        win.open_devtools();
    }
}

const TROUBLESHOOT_SCRIPT: &str = include_str!("../../../troubleshoot.ps1");
const GITHUB_ISSUES_NEW_URL: &str = "https://github.com/nlsickler/WinSoundDeck/issues/new";

fn percent_encode_query(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn build_github_issue_url(version: &str) -> String {
    let title = format!("[Bug v{version}]: ");
    let body = "### Describe the issue\n<!-- What happened, and what did you expect? -->\n\n### Steps to reproduce\n1. \n\n### Diagnostic Report\nPlease attach the generated diagnostic `.md` file (its path has been copied to your clipboard — click **\"Paste, drop, or click to add files\"** below and paste it into the file picker).\n";
    format!(
        "{GITHUB_ISSUES_NEW_URL}?title={}&body={}",
        percent_encode_query(&title),
        percent_encode_query(body)
    )
}

#[tauri::command]
async fn run_troubleshoot_to_clipboard(open_github_issue: bool) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let temp_dir = std::env::temp_dir();
        let pid = std::process::id();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let script_path = temp_dir.join(format!("winsounddeck_troubleshoot_{pid}.ps1"));
        let report_path = temp_dir.join(format!("winsounddeck-diagnostics-{stamp}.md"));

        fs::write(&script_path, format!("\u{FEFF}{TROUBLESHOOT_SCRIPT}"))
            .map_err(|e| format!("Failed to write temporary troubleshoot script: {e}"))?;

        let run_res = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                &script_path.to_string_lossy(),
                "-OutFile",
                &report_path.to_string_lossy(),
            ])
            .current_dir(&temp_dir)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to run troubleshoot script: {e}"));

        let _ = fs::remove_file(&script_path);
        let output = run_res?;
        if !output.status.success() {
            let _ = fs::remove_file(&report_path);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Troubleshoot script failed: {stderr}"));
        }

        let report_str = report_path.to_string_lossy().to_string();
        let escaped_path = report_str.replace('\'', "''");
        let clip_status = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                &format!("Set-Clipboard -Value '{escaped_path}'"),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| format!("Failed to copy report path to clipboard: {e}"))?;

        if !clip_status.success() {
            return Err("Failed to copy diagnostic report path to clipboard".to_string());
        }

        if open_github_issue {
            let url = build_github_issue_url(env!("CARGO_PKG_VERSION"));
            let ps_url = url.replace('\'', "''");
            Command::new("powershell.exe")
                .args(["-NoProfile", "-Command", &format!("Start-Process '{ps_url}'")])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map_err(|e| format!("Report saved, but failed to open GitHub: {e}"))?;
            Ok(format!(
                "Report saved & path copied to clipboard — attach it to the GitHub Issue:\n{report_str}"
            ))
        } else {
            Ok(format!(
                "Report saved & path copied to clipboard (attach to a bug report):\n{report_str}"
            ))
        }
    })
    .await
    .map_err(|e| format!("Diagnostics task failed: {e}"))?
}

#[tauri::command]
fn get_launch_at_login() -> Result<bool, String> {
    let output = Command::new("reg.exe")
        .args(["query", RUN_REG_KEY, "/v", RUN_REG_VALUE])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to query Windows startup registry: {e}"))?;
    Ok(output.status.success())
}

#[tauri::command]
fn set_launch_at_login(enabled: bool) -> Result<bool, String> {
    if enabled {
        let exe = std::env::current_exe()
            .map_err(|e| format!("Failed to resolve current executable: {e}"))?;
        let cmd = format_startup_command(&exe);
        let status = Command::new("reg.exe")
            .args([
                "add",
                RUN_REG_KEY,
                "/v",
                RUN_REG_VALUE,
                "/t",
                "REG_SZ",
                "/d",
                &cmd,
                "/f",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| format!("Failed to update Windows startup registry: {e}"))?;
        if !status.success() {
            return Err("Failed to enable Launch at Windows login".to_string());
        }
    } else {
        let status = Command::new("reg.exe")
            .args(["delete", RUN_REG_KEY, "/v", RUN_REG_VALUE, "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| format!("Failed to update Windows startup registry: {e}"))?;
        if !status.success() && get_launch_at_login().unwrap_or(false) {
            return Err("Failed to disable Launch at Windows login".to_string());
        }
    }
    Ok(enabled)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
struct WindowState {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn window_state_path() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata)
            .join("winsounddeck")
            .join("window_state.json")
    } else {
        PathBuf::from("window_state.json")
    }
}

fn load_window_state(path: &Path) -> Option<WindowState> {
    let raw = fs::read_to_string(path).ok()?;
    let st: WindowState = serde_json::from_str(&raw).ok()?;
    if st.width >= 380 && st.height >= 480 {
        Some(st)
    } else {
        None
    }
}

fn save_window_state(path: &Path, state: &WindowState) {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = fs::write(path, json);
    }
}

fn is_position_visible_on_monitors(
    state: &WindowState,
    monitors: &[(i32, i32, u32, u32)],
) -> bool {
    let cx = state.x.saturating_add((state.width / 2) as i32);
    let cy = state.y.saturating_add(40);
    monitors.iter().any(|&(mx, my, mw, mh)| {
        let max_x = mx.saturating_add(mw as i32);
        let max_y = my.saturating_add(mh as i32);
        cx >= mx && cx < max_x && cy >= my && cy < max_y
    })
}

fn restore_window_state(win: &tauri::WebviewWindow) {
    let Some(saved) = load_window_state(&window_state_path()) else {
        return;
    };
    let monitors: Vec<(i32, i32, u32, u32)> = win
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            let pos = m.position();
            let size = m.size();
            (pos.x, pos.y, size.width, size.height)
        })
        .collect();
    if monitors.is_empty() || is_position_visible_on_monitors(&saved, &monitors) {
        let _ = win.set_size(PhysicalSize::new(saved.width, saved.height));
        let _ = win.set_position(PhysicalPosition::new(saved.x, saved.y));
    }
}

fn capture_window_state(win: &tauri::WebviewWindow) -> Option<WindowState> {
    if win.is_minimized().unwrap_or(false) {
        return None;
    }
    let pos = win.outer_position().ok()?;
    let size = win.inner_size().ok()?;
    if size.width < 380 || size.height < 480 {
        return None;
    }
    Some(WindowState {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
    })
}

fn flush_window_state(slot: &Mutex<Option<WindowState>>) {
    if let Ok(guard) = slot.lock() {
        if let Some(st) = *guard {
            save_window_state(&window_state_path(), &st);
        }
    }
}

fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

fn build_tray_menu_with_presets(
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

fn refresh_tray_menu(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        if let Ok(menu) = build_tray_menu(app) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

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

    let last_window_state = std::sync::Arc::new(Mutex::new(None::<WindowState>));
    let setup_win_state = std::sync::Arc::clone(&last_window_state);
    let event_win_state = std::sync::Arc::clone(&last_window_state);

    tauri::Builder::default()
        .manage(AppIconCache::default())
        .setup(move |app| {
            // Keep Tauri's STA setup hook non-blocking; spawn daemon & populate presets off-thread.
            let menu = build_tray_menu_with_presets(app.handle(), None)?;

            if let Some(win) = app.get_webview_window("main") {
                restore_window_state(&win);
            }

            let mut tray_builder = TrayIconBuilder::with_id("main-tray")
                .menu(&menu)
                .tooltip("WinSoundDeck");
            if let Some(icon) = app.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon);
            }
            let quit_win_state = std::sync::Arc::clone(&setup_win_state);
            tray_builder
                .on_menu_event(move |app, event| {
                    let id = event.id.as_ref();
                    if id == "open" {
                        show_main_window(app);
                    } else if id == "quit" {
                        if let Some(win) = app.get_webview_window("main") {
                            if let Some(st) = capture_window_state(&win) {
                                save_window_state(&window_state_path(), &st);
                            } else if let Ok(guard) = quit_win_state.lock() {
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
            get_status,
            get_peaks,
            get_app_icons,
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
            open_bluetooth_settings,
            set_default_endpoint,
            set_include_disconnected,
            get_launch_at_login,
            set_launch_at_login,
            open_devtools,
            run_troubleshoot_to_clipboard,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{
        build_github_issue_url, daemon_launch_command, format_startup_command,
        is_position_visible_on_monitors, load_window_state, pnp_instance_id_for_endpoint,
        save_window_state, WindowState,
    };
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn github_issue_url_includes_version_without_local_paths() {
        let url = build_github_issue_url("0.9.0");
        assert!(url.starts_with("https://github.com/nlsickler/WinSoundDeck/issues/new?title="));
        assert!(url.contains("0.9.0"));
        assert!(url.contains("body="));
        assert!(!url.contains("AppData"));
    }

    #[test]
    fn daemon_launch_command_matches_build_mode() {
        let exe = Path::new("C:\\Apps\\audio-ui.exe");
        let (path, args) = daemon_launch_command(Some(exe));
        #[cfg(feature = "portable")]
        {
            assert_eq!(path, exe);
            assert_eq!(args, &["--daemon"]);
        }
        #[cfg(not(feature = "portable"))]
        {
            assert_eq!(path, Path::new("C:\\Apps\\audio-daemon.exe"));
            assert!(args.is_empty());
        }
    }

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

    #[test]
    fn window_state_round_trips_and_validates_monitor_bounds() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("winsounddeck_win_{nanos}"))
            .join("window_state.json");
        let st = WindowState {
            x: 2000,
            y: 120,
            width: 480,
            height: 700,
        };
        save_window_state(&path, &st);
        assert_eq!(load_window_state(&path), Some(st));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());

        let dual_monitors = vec![(0, 0, 1920, 1080), (1920, 0, 2560, 1440)];
        assert!(is_position_visible_on_monitors(&st, &dual_monitors));

        let laptop_only = vec![(0, 0, 1920, 1080)];
        assert!(!is_position_visible_on_monitors(&st, &laptop_only));
    }

    #[test]
    fn format_startup_command_quotes_executable_path() {
        let exe = Path::new("C:\\Program Files\\WinSoundDeck\\WinSoundDeck.exe");
        assert_eq!(
            format_startup_command(exe),
            "\"C:\\Program Files\\WinSoundDeck\\WinSoundDeck.exe\""
        );
    }
}
