use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub fn window_state_path() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata)
            .join("winsounddeck")
            .join("window_state.json")
    } else {
        PathBuf::from("window_state.json")
    }
}

pub fn load_window_state(path: &Path) -> Option<WindowState> {
    let raw = fs::read_to_string(path).ok()?;
    let st: WindowState = serde_json::from_str(&raw).ok()?;
    if st.width >= 380 && st.height >= 480 {
        Some(st)
    } else {
        None
    }
}

pub fn save_window_state(path: &Path, state: &WindowState) {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = fs::write(path, json);
    }
}

pub fn is_position_visible_on_monitors(
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

pub fn restore_window_state(win: &tauri::WebviewWindow) {
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

pub fn capture_window_state(win: &tauri::WebviewWindow) -> Option<WindowState> {
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

pub fn flush_window_state(slot: &Mutex<Option<WindowState>>) {
    if let Ok(guard) = slot.lock() {
        if let Some(st) = *guard {
            save_window_state(&window_state_path(), &st);
        }
    }
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_position_visible_on_monitors, load_window_state, save_window_state, WindowState,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

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
}
