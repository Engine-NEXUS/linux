//! Window management: transparent frameless always-on-top overlay with click-through control.
//!
//! The overlay starts hidden and click-through. On wake, Rust shows the window and
//! disables click-through. When the assistant goes idle, the frontend re-enables
//! click-through and eventually hides the window.

use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

const WIN: &str = "main";

/// No-op since the fullscreen-stage migration: the "main" window is a
/// maximized transparent stage and the orb roams inside it via frontend
/// transform. Kept so existing callers compile; delete once callers drop it.
pub fn position_orb<R: Runtime>(_win: &WebviewWindow<R>) -> Result<(), String> {
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
    // Fullscreen stage: start click-through ON so the invisible stage never
    // eats clicks. The frontend flips it OFF when the orb is hovered/held.
    win.set_ignore_cursor_events(true).map_err(|e| e.to_string())?;
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
/// Fullscreen stage: the stage window stays shown the whole session (it is
/// click-through, so an invisible fullscreen window eats no clicks). Showing
/// the orb is purely frontend state now — this just re-applies always-on-top
/// + click-through OFF so the orb is interactive after wake.
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
/// Fullscreen stage: never hides the native window — the stage is
/// click-through when idle so it interferes with nothing. The orb hides via
/// frontend state (roam continues or parks). Re-enables click-through here
/// so a stale interactive window can't eat clicks after hide.
#[tauri::command]
pub fn hide_overlay<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let win = app
        .get_webview_window(WIN)
        .ok_or_else(|| "main window not found".to_string())?;
    win.set_ignore_cursor_events(true).map_err(|e| e.to_string())?;
    Ok(())
}
