//! Window management: transparent frameless always-on-top overlay with click-through control.
//!
//! The overlay starts hidden and click-through. On wake, Rust shows the window and
//! disables click-through. When the assistant goes idle, the frontend re-enables
//! click-through and eventually hides the window.

use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

const WIN: &str = "main";

/// Read orb position + size from settings.json.
/// Falls back to defaults (center-bottom, 200px) if the file is missing,
/// can't be parsed, or doesn't contain the orb fields.
fn read_orb_settings<R: Runtime>(app: &AppHandle<R>) -> (f64, f64, u32) {
    let dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(_) => return (0.5, 1.0, 200),
    };
    let path = dir.join("settings.json");
    if !path.exists() {
        return (0.5, 1.0, 200);
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return (0.5, 1.0, 200),
    };
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return (0.5, 1.0, 200),
    };
    let h = json.get("orbHorizontalPct").and_then(|v| v.as_f64()).unwrap_or(0.5);
    let v = json.get("orbVerticalPct").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let size = json.get("orbSize").and_then(|v| v.as_u64()).unwrap_or(200) as u32;
    // Clamp to safe ranges
    let h = h.max(0.0).min(1.0);
    let v = v.max(0.0).min(1.0);
    let size = size.max(100).min(300);
    (h, v, size)
}

/// Position the orb based on saved settings (orbHorizontalPct, orbVerticalPct, orbSize).
/// Falls back to center-bottom, 200px if settings are missing or invalid.
/// Called at startup, on every wake, on every hotkey press, and on show_overlay.
pub fn position_orb<R: Runtime>(win: &WebviewWindow<R>) -> Result<(), String> {
    use tauri::PhysicalPosition;
    if let Ok(Some(monitor)) = win.current_monitor() {
        let scale = monitor.scale_factor();
        let screen = monitor.size();

        let (h_pct, v_pct, orb_size) = read_orb_settings(win.app_handle());
        let orb = orb_size as i32;
        let phys_orb = (orb as f64 * scale) as i32;

        // Compute position from percentages
        let raw_x = (screen.width as f64 * h_pct) as i32 - phys_orb / 2;
        let raw_y = (screen.height as f64 * v_pct) as i32 - phys_orb / 2;

        // Clamp to keep orb fully on-screen
        let x = raw_x.max(0).min(screen.width as i32 - phys_orb);
        let y = raw_y.max(0).min(screen.height as i32 - phys_orb);

        let _ = win.set_position(PhysicalPosition::new(x, y));
        let _ = win.set_size(tauri::PhysicalSize::new(orb, orb));
        tracing::debug!("orb positioned at ({}, {}) size {}px [h={}, v={}, scale={}]",
            x, y, orb, h_pct, v_pct, scale);
    }
    Ok(())
}

/// Configure window as a non-activating floating overlay (does not steal keyboard focus from active apps)
pub fn configure_non_activating_overlay<R: Runtime>(win: &WebviewWindow<R>) -> Result<(), String> {
    let _ = position_orb(win);
    win.set_always_on_top(true).map_err(|e| e.to_string())?;
    let _ = win.set_focusable(false);
    Ok(())
}

pub fn init<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let win = app
        .get_webview_window(WIN)
        .ok_or_else(|| "main window not found".to_string())?;

    configure_non_activating_overlay(&win)?;
    // Start with click-through OFF so the user can interact with the window.
    win.set_ignore_cursor_events(false).map_err(|e| e.to_string())?;
    Ok(())
}

/// IPC: `invoke('set_click_through', { ignore: bool })`.
#[tauri::command]
pub fn set_click_through<R: Runtime>(
    app: AppHandle<R>,
    ignore: bool,
) -> Result<(), String> {
    let win = app
        .get_webview_window(WIN)
        .ok_or_else(|| "main window not found".to_string())?;
    win.set_ignore_cursor_events(ignore).map_err(|e| e.to_string())?;
    if !ignore {
        let _ = win.set_always_on_top(true);
    }
    Ok(())
}

/// Convenience: re-apply overlay state (called after show).
#[allow(dead_code)]
pub fn refresh_overlay<R: Runtime>(win: &WebviewWindow<R>) -> Result<(), String> {
    let _ = position_orb(win);
    win.set_always_on_top(true).map_err(|e| e.to_string())?;
    win.set_ignore_cursor_events(true).map_err(|e| e.to_string())
}

/// IPC: `invoke('show_overlay')`.
/// Shows the native overlay window. Used by the frontend when `visible` becomes true.
/// CSS opacity/transform alone can't reliably hide WebView2 transparent windows after
/// content has been rendered (GPU compositing caches the last frame), so we use
/// native show/hide for reliable visibility control.
#[tauri::command]
pub fn show_overlay<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let win = app
        .get_webview_window(WIN)
        .ok_or_else(|| "main window not found".to_string())?;
    win.show().map_err(|e| e.to_string())?;
    configure_non_activating_overlay(&win)?;
    win.set_ignore_cursor_events(false).map_err(|e| e.to_string())?;
    Ok(())
}

/// IPC: `invoke('hide_overlay')`.
/// Hides the native overlay window. Used by the frontend when `visible` becomes false.
#[tauri::command]
pub fn hide_overlay<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let win = app
        .get_webview_window(WIN)
        .ok_or_else(|| "main window not found".to_string())?;
    win.hide().map_err(|e| e.to_string())?;
    Ok(())
}

/// IPC: `invoke('set_orb_position', { horizontalPct, verticalPct, size })`.
/// Live-updates the orb position and size without restarting.
/// Does NOT save to settings — the frontend should call save_settings separately.
/// Used by the settings sidebar sliders for real-time preview.
#[tauri::command]
pub fn set_orb_position<R: Runtime>(
    app: AppHandle<R>,
    horizontal_pct: f64,
    vertical_pct: f64,
    size: u32,
) -> Result<(), String> {
    let win = app
        .get_webview_window(WIN)
        .ok_or_else(|| "main window not found".to_string())?;

    // Clamp inputs to safe ranges
    let h = horizontal_pct.max(0.0).min(1.0);
    let v = vertical_pct.max(0.0).min(1.0);
    let orb = size.max(100).min(300) as i32;

    if let Ok(Some(monitor)) = win.current_monitor() {
        let scale = monitor.scale_factor();
        let screen = monitor.size();
        let phys_orb = (orb as f64 * scale) as i32;

        let raw_x = (screen.width as f64 * h) as i32 - phys_orb / 2;
        let raw_y = (screen.height as f64 * v) as i32 - phys_orb / 2;
        let x = raw_x.max(0).min(screen.width as i32 - phys_orb);
        let y = raw_y.max(0).min(screen.height as i32 - phys_orb);

        let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
        let _ = win.set_size(tauri::PhysicalSize::new(orb, orb));
        tracing::debug!("set_orb_position: ({}, {}) size {}px [h={}, v={}]",
            x, y, orb, h, v);
    }
    Ok(())
}
