//! Screen pointer — Phase 2 of the clicky-style screen agent (Route C).
//!
//! The marker renders inside the always-alive main stage window (no new
//! window, no extra WebView RAM, no creation latency — the stage is
//! transparent + click-through at idle and is never natively hidden).
//! Rust evaluates the Table-B visibility conditions ([`decide`], pure +
//! unit-tested) and emits `pointer:show` / `pointer:hide`; the frontend
//! owns CSS positioning (devicePixelRatio-aware), dwell timing, and fade.

use tauri::{AppHandle, Emitter, Runtime};

/// Model point in 0-1000 normalized coordinates (from `vision.rs`).
#[derive(Debug, Clone)]
pub struct NormPoint {
    pub x: u16,
    pub y: u16,
    pub label: String,
}

/// Payload of the `pointer:show` event. Coordinates are PHYSICAL screen
/// pixels on the captured (primary) monitor; the frontend divides by
/// `devicePixelRatio` for CSS positioning.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PointerShow {
    pub x: f64,
    pub y: f64,
    pub label: String,
    pub dwell_ms: u64,
}

/// Resolved pointer settings (settings.json, clamped).
pub struct PointerSettings {
    pub enabled: bool,
    pub dwell_ms: u64,
    pub excluded_apps: Vec<String>,
    pub suppress_fullscreen: bool,
}

pub fn read_settings<R: Runtime>(app: &AppHandle<R>) -> PointerSettings {
    let s = crate::commands::get_settings(app.clone()).unwrap_or_default();
    PointerSettings {
        enabled: s.pointer_enabled,
        dwell_ms: s.pointer_dwell_seconds.clamp(3, 15) as u64 * 1000,
        excluded_apps: s
            .pointer_excluded_apps
            .split(',')
            .map(|p| p.trim().to_lowercase())
            .filter(|p| !p.is_empty())
            .collect(),
        suppress_fullscreen: s.pointer_suppress_fullscreen,
    }
}

/// Foreground context for the visibility decision.
pub struct PointerContext {
    pub response_is_locate: bool,
    pub has_point: bool,
    pub foreground_title: Option<String>,
    pub fullscreen_active: bool,
}

/// Table-B visibility decision.
pub enum PointerDecision {
    Show { dwell_ms: u64 },
    Hide { reason: &'static str },
}

/// Pure visibility matrix — every Table-B condition in one place.
pub fn decide(ctx: &PointerContext, settings: &PointerSettings) -> PointerDecision {
    if !settings.enabled {
        return PointerDecision::Hide { reason: "pointer disabled in settings" };
    }
    if !ctx.response_is_locate || !ctx.has_point {
        return PointerDecision::Hide { reason: "not a locate response" };
    }
    if let Some(title) = ctx.foreground_title.as_deref() {
        let lower = title.to_lowercase();
        if settings.excluded_apps.iter().any(|app| lower.contains(app)) {
            return PointerDecision::Hide { reason: "foreground app is excluded" };
        }
    }
    if settings.suppress_fullscreen && ctx.fullscreen_active {
        return PointerDecision::Hide { reason: "fullscreen app active" };
    }
    PointerDecision::Show { dwell_ms: settings.dwell_ms }
}

/// Foreground window title (Windows only) for the privacy exclusion
/// list. Other OSes have no stable foreground-title API wired yet →
/// None (exclusions unenforced there — see Table C).
#[cfg(target_os = "windows")]
pub fn foreground_title() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0 == 0 {
            return None;
        }
        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut buf);
        if len <= 0 {
            return None;
        }
        let title = String::from_utf16_lossy(&buf[..len as usize]).trim().to_string();
        if title.is_empty() { None } else { Some(title) }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn foreground_title() -> Option<String> {
    None
}

/// True when the foreground window covers the whole screen (Windows
/// only — games, fullscreen video). Other OSes report false.
#[cfg(target_os = "windows")]
pub fn fullscreen_active() -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetSystemMetrics, GetWindowRect, SM_CXSCREEN, SM_CYSCREEN,
    };
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0 == 0 {
            return false;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return false;
        }
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        rect.left <= 0
            && rect.top <= 0
            && (rect.right - rect.left) >= sw
            && (rect.bottom - rect.top) >= sh
    }
}

#[cfg(not(target_os = "windows"))]
pub fn fullscreen_active() -> bool {
    false
}

/// Evaluate conditions and emit `pointer:show` when they pass. `norm`
/// is the 0-1000 model point; `shot_w/h` the ORIGINAL capture dims.
/// Returns true when the marker was shown.
pub fn maybe_show<R: Runtime>(
    app: &AppHandle<R>,
    is_locate: bool,
    point: Option<&crate::vision::PointNorm>,
    shot_w: u32,
    shot_h: u32,
) -> bool {
    let settings = read_settings(app);
    let ctx = PointerContext {
        response_is_locate: is_locate,
        has_point: point.is_some(),
        foreground_title: foreground_title(),
        fullscreen_active: fullscreen_active(),
    };
    match decide(&ctx, &settings) {
        PointerDecision::Hide { reason } => {
            tracing::info!("pointer: hidden ({reason})");
            false
        }
        PointerDecision::Show { dwell_ms } => {
            let pt = point.expect("decide Show implies has_point");
            let show = PointerShow {
                x: pt.x as f64 / 1000.0 * shot_w as f64,
                y: pt.y as f64 / 1000.0 * shot_h as f64,
                label: pt.label.clone(),
                dwell_ms,
            };
            tracing::info!(
                "pointer: show '{}' at ({:.0}, {:.0}) for {}ms",
                show.label,
                show.x,
                show.y,
                dwell_ms
            );
            if let Err(e) = app.emit("pointer:show", &show) {
                tracing::warn!("pointer: emit failed: {e}");
                return false;
            }
            true
        }
    }
}

/// Emit `pointer:hide` (tray/menu path; the frontend also hides on new
/// turns and dwell expiry by itself).
#[tauri::command]
pub fn hide_pointer<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    app.emit("pointer:hide", ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> PointerSettings {
        PointerSettings {
            enabled: true,
            dwell_ms: 6000,
            excluded_apps: vec!["bank".to_string(), "1password".to_string()],
            suppress_fullscreen: true,
        }
    }

    fn ctx() -> PointerContext {
        PointerContext {
            response_is_locate: true,
            has_point: true,
            foreground_title: Some("VS Code".to_string()),
            fullscreen_active: false,
        }
    }

    #[test]
    fn happy_path_shows_with_dwell() {
        match decide(&ctx(), &settings()) {
            PointerDecision::Show { dwell_ms } => assert_eq!(dwell_ms, 6000),
            PointerDecision::Hide { reason } => panic!("should show, hid: {reason}"),
        }
    }

    #[test]
    fn disabled_master_toggle_hides() {
        let mut s = settings();
        s.enabled = false;
        assert!(matches!(decide(&ctx(), &s), PointerDecision::Hide { .. }));
    }

    #[test]
    fn describe_without_point_hides() {
        for c in [
            PointerContext { response_is_locate: false, has_point: false, foreground_title: None, fullscreen_active: false },
            PointerContext { response_is_locate: true, has_point: false, foreground_title: None, fullscreen_active: false },
        ] {
            assert!(matches!(decide(&c, &settings()), PointerDecision::Hide { .. }));
        }
    }

    #[test]
    fn excluded_app_hides_case_insensitive() {
        let c = PointerContext {
            foreground_title: Some("My BANK - Chrome".to_string()),
            ..ctx()
        };
        match decide(&c, &settings()) {
            PointerDecision::Hide { reason } => assert!(reason.contains("excluded")),
            PointerDecision::Show { .. } => panic!("excluded app must hide"),
        }
    }

    #[test]
    fn fullscreen_suppressed_only_when_setting_on() {
        let c = PointerContext { fullscreen_active: true, ..ctx() };
        assert!(matches!(decide(&c, &settings()), PointerDecision::Hide { .. }));
        let mut s = settings();
        s.suppress_fullscreen = false;
        assert!(matches!(decide(&c, &s), PointerDecision::Show { .. }));
    }

    #[test]
    fn no_foreground_title_never_excludes() {
        let c = PointerContext { foreground_title: None, ..ctx() };
        assert!(matches!(decide(&c, &settings()), PointerDecision::Show { .. }));
    }
}
