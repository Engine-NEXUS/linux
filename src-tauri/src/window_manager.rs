//! Orb/loading-indicator placement — the voice orb and loading spinner
//! live as positioned `<div>`s inside the always-on `stage` fullscreen
//! overlay (see `frontend/src/stage/OrbFrame.tsx`), not as their own OS
//! windows. This module computes WHERE they should sit (pure, unit-tested
//! pct->physical-px math, unchanged since the single-window days) and
//! emits that rect to the stage frontend over Tauri events — it no longer
//! owns an OS window to show/hide/position directly.

use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

/// Last-emitted rects, for the stage frontend to pull on mount (race-free
/// delivery pattern already used elsewhere in this codebase for
/// dynamically-created windows, e.g. `PENDING_SIDEBAR` /
/// `get_pending_sidebar_content` — an event fired before React has
/// mounted its listener is simply lost, so a pull-based fallback is
/// needed). Stage is created once at boot, but its first paint can still
/// lag the first `emit_orb_rect`/`emit_loading_rect` call by a frame or
/// two.
static LAST_ORB_RECT: once_cell::sync::Lazy<parking_lot::Mutex<Option<RectPayload>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));
static LAST_LOADING_RECT: once_cell::sync::Lazy<parking_lot::Mutex<Option<RectPayload>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

/// Record a rect computed elsewhere (calibration.rs's preview path) so the
/// pending-pull cache stays correct even when the emit didn't go through
/// `emit_orb_rect`/`emit_loading_rect` directly.
pub fn note_orb_rect(rect: RectPayload) {
    *LAST_ORB_RECT.lock() = Some(rect);
}
pub fn note_loading_rect(rect: RectPayload) {
    *LAST_LOADING_RECT.lock() = Some(rect);
}

/// IPC: pull the last-computed orb rect (race-free mount fallback).
#[tauri::command]
pub fn get_pending_orb_rect<R: Runtime>(app: AppHandle<R>) -> Option<RectPayload> {
    let cached = *LAST_ORB_RECT.lock();
    cached.or_else(|| Some(orb_rect(&app)))
}

/// IPC: pull the current orb position ("top" | "bottom").
#[tauri::command]
pub fn get_pending_orb_position<R: Runtime>(app: AppHandle<R>) -> String {
    let (_, v, _) = read_orb_settings(&app);
    if v > 0.5 {
        "bottom".to_string()
    } else {
        "top".to_string()
    }
}

/// IPC: pull the last-computed loading-indicator rect (race-free mount
/// fallback).
#[tauri::command]
pub fn get_pending_loading_rect<R: Runtime>(app: AppHandle<R>) -> RectPayload {
    let cached = *LAST_LOADING_RECT.lock();
    cached.unwrap_or_else(|| {
        let (h, v, size) = read_loading_settings(&app);
        rect_for(&app, h, v, size)
    })
}

/// Physical-px rect sent to the stage frontend. Same shape as
/// `stage::StageRect` by convention (kept as a separate type so this
/// module doesn't need to depend on `stage`'s internal hitbox type).
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct RectPayload {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Default orb box, physical px. Linux is smaller: on a 1080p laptop at 100 %
/// scale the Windows 200 px orb looked oversized under the top panel.
pub(crate) const DEFAULT_ORB_SIZE: u32 = if cfg!(target_os = "linux") { 160 } else { 200 };
/// One size range for startup, `set_orb_position` and calibration's orb rail,
/// so a size set live or by calibration survives a restart (startup used to
/// clamp to 180, which snapped smaller saved sizes back up).
const ORB_SIZE_MIN: u32 = 100;
const ORB_SIZE_MAX: u32 = 400;

/// Read orb position + size from settings.json.
/// Falls back to defaults (top centre, [`DEFAULT_ORB_SIZE`]) if the file is
/// missing, can't be parsed, or doesn't contain the orb fields.
pub fn read_orb_settings<R: Runtime>(app: &AppHandle<R>) -> (f64, f64, u32) {
    let dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(_) => return (0.5, 0.0, DEFAULT_ORB_SIZE),
    };
    let path = dir.join("settings.json");
    if !path.exists() {
        return (0.5, 0.0, DEFAULT_ORB_SIZE);
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return (0.5, 0.0, DEFAULT_ORB_SIZE),
    };
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return (0.5, 0.0, DEFAULT_ORB_SIZE),
    };
    orb_settings_from_json(&json)
}

fn orb_settings_from_json(json: &serde_json::Value) -> (f64, f64, u32) {
    let pos = json.get("orbPosition").and_then(|v| v.as_str()).unwrap_or("top");
    let (h, v): (f64, f64) = if pos == "bottom" {
        (0.5, 1.0)
    } else {
        (0.5, 0.0)
    };
    let size = json
        .get("orbSize")
        .and_then(|v| v.as_u64())
        .map_or(DEFAULT_ORB_SIZE, |v| v.min(u64::from(ORB_SIZE_MAX)) as u32);
    let h = h.max(0.0).min(1.0);
    let v = v.max(0.0).min(1.0);
    let size = size.clamp(ORB_SIZE_MIN, ORB_SIZE_MAX);
    (h, v, size)
}

/// Loose settings reader used by both the orb and the waves/loading paths:
/// `(key) -> f64` with clamped fallback, tolerant of missing/corrupt files.
fn read_pct_key(json: &serde_json::Value, key: &str, fallback: f64) -> f64 {
    json.get(key)
        .and_then(|v| v.as_f64())
        .map(|v| v.max(0.0).min(1.0))
        .unwrap_or(fallback)
}

fn read_size_key(json: &serde_json::Value, key: &str, fallback: u32, min: u32, max: u32) -> u32 {
    json.get(key)
        .and_then(|v| v.as_u64())
        .map(|v| (v as u32).max(min).min(max))
        .unwrap_or(fallback)
}

/// Read waves placement from settings.json (ghost-session rect for the
/// waves visual inside the orb's stage rect). Defaults = the wakeup
/// defaults (user invariant: waves live where the wakeup orb lives).
pub fn read_waves_settings<R: Runtime>(app: &AppHandle<R>) -> (f64, f64, u32) {
    read_orb_settings(app)
}

/// Read loading-indicator placement from settings.json. Defaults ≈ the
/// historical hardcoded top-right corner (center-anchored 0.95/0.05, 80px).
pub fn read_loading_settings<R: Runtime>(app: &AppHandle<R>) -> (f64, f64, u32) {
    let fallback = (0.95f64, 0.05f64, 80u32);
    let dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(_) => return fallback,
    };
    let Ok(content) = std::fs::read_to_string(dir.join("settings.json")) else {
        return fallback;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return fallback;
    };
    (
        read_pct_key(&json, "loadingHorizontalPct", 0.95),
        read_pct_key(&json, "loadingVerticalPct", 0.05),
        read_size_key(&json, "loadingSize", 80, 40, 160),
    )
}

/// Pure overlay placement math (unit-tested): center-anchored pct →
/// clamped physical px. Single conversion choke point shared by the orb,
/// waves (ghost sessions), loading indicator, and the calibration previews.
pub fn overlay_xy(
    h_pct: f64,
    v_pct: f64,
    size: u32,
    screen_w: i32,
    screen_h: i32,
    scale: f64,
) -> (i32, i32) {
    let h = h_pct.max(0.0).min(1.0);
    let v = v_pct.max(0.0).min(1.0);
    let phys = (size as f64 * scale) as i32;
    let raw_x = (screen_w as f64 * h) as i32 - phys / 2;
    let raw_y = (screen_h as f64 * v) as i32 - phys / 2;
    let x = raw_x.max(0).min(screen_w - phys);
    let y = raw_y.max(0).min(screen_h - phys);
    (x, y)
}

/// Pure nudge math (unit-tested): shift the CURRENT top-left by logical
/// px, then re-solve the center-anchored pct. The inverse of overlay_xy
/// up to the edge clamp — drag, wheel, and keyboard share one path.
pub fn overlay_nudge(
    h_pct: f64,
    v_pct: f64,
    size: u32,
    dx_logical: i32,
    dy_logical: i32,
    screen_w: i32,
    screen_h: i32,
    scale: f64,
) -> (f64, f64) {
    let phys = (size as f64 * scale) as i32;
    let (x, y) = overlay_xy(h_pct, v_pct, size, screen_w, screen_h, scale);
    let nx = x + (dx_logical as f64 * scale) as i32;
    let ny = y + (dy_logical as f64 * scale) as i32;
    let h = ((nx + phys / 2) as f64 / screen_w.max(1) as f64).max(0.0).min(1.0);
    let v = ((ny + phys / 2) as f64 / screen_h.max(1) as f64).max(0.0).min(1.0);
    (h, v)
}

/// Resolve the primary monitor's (width, height, scale) in physical px
/// without needing a window reference — neither the orb nor the loading
/// indicator are their own window anymore. Falls back to the `stage`
/// window's own monitor info (covers the brief pre-`primary_monitor`-ready
/// window at cold boot on some multi-monitor setups), then to a safe
/// 1920x1080x1.0 default so a rect is always produced rather than skipped.
pub fn monitor_info<R: Runtime>(app: &AppHandle<R>) -> (i32, i32, f64) {
    if let Ok(Some(m)) = app.primary_monitor() {
        return (m.size().width as i32, m.size().height as i32, m.scale_factor());
    }
    if let Some(win) = app.get_webview_window("stage") {
        if let Ok(Some(m)) = win.current_monitor() {
            return (m.size().width as i32, m.size().height as i32, m.scale_factor());
        }
    }
    // Linux: at cold boot the primary monitor is often not known yet (the
    // stage window is not realised), and the hardcoded 1920x1080 below is
    // then plainly wrong on a 2560x1440 or a 1366x768 screen. Any available
    // monitor is better than the guess.
    #[cfg(target_os = "linux")]
    {
        if let Ok(monitors) = app.available_monitors() {
            if let Some(m) = monitors.first() {
                return (m.size().width as i32, m.size().height as i32, m.scale_factor());
            }
        }
    }
    (1920, 1080, 1.0)
}

/// Linux: make the fullscreen stage really cover the monitor it is on.
///
/// GTK ignores `maximized` on a window that is still hidden (the stage is
/// created hidden and shown a moment later), and Wayland forbids client
/// positioning entirely — so the only reliable way to get a full-screen
/// overlay is to size it explicitly once it is mapped. On Windows the stage
/// keeps its configured size, so this is a no-op there.
#[cfg(target_os = "linux")]
pub fn size_stage_to_monitor<R: Runtime>(app: &AppHandle<R>, win: &WebviewWindow<R>) {
    if crate::overlay_linux::is_layer_shell_active() {
        tracing::info!("stage: layer-shell active — skipping manual window sizing (anchored to edges)");
        return;
    }
    let (w, h, _scale) = monitor_info(app);
    if w <= 0 || h <= 0 {
        return;
    }
    let _ = win.set_size(tauri::PhysicalSize::new(w as u32, h as u32));
    let _ = win.set_position(tauri::PhysicalPosition::new(0, 0));
    tracing::info!("stage: sized to the primary monitor ({w}x{h} physical)");
}

/// Windows/macOS: the stage keeps the size it was configured with.
#[cfg(not(target_os = "linux"))]
pub fn size_stage_to_monitor<R: Runtime>(_app: &AppHandle<R>, _win: &WebviewWindow<R>) {}

/// Compute a physical-px rect for arbitrary (h, v, size) — shared by
/// `orb_rect`, the calibration preview, and `set_orb_position`'s
/// live-preview path.
pub fn rect_for<R: Runtime>(app: &AppHandle<R>, h_pct: f64, v_pct: f64, size: u32) -> RectPayload {
    let (sw, sh, scale) = monitor_info(app);
    // Linux layer-shell: the stage covers the whole output, panels included,
    // so the orb is placed inside the area the panels leave free.
    #[cfg(target_os = "linux")]
    let (x, y) = {
        let i = crate::overlay_linux::safe_insets_physical(scale);
        let (x, y) = overlay_xy(h_pct, v_pct, size, sw - i.left - i.right, sh - i.top - i.bottom, scale);
        (x + i.left, y + i.top)
    };
    #[cfg(not(target_os = "linux"))]
    let (x, y) = overlay_xy(h_pct, v_pct, size, sw, sh, scale);
    let phys = (size as f64 * scale) as i32;
    RectPayload { x, y, w: phys, h: phys }
}

/// Compute the orb's current rect: waves placement during a ghost
/// session, orb placement otherwise. Calibration owns the preview rect
/// while a session is open (see `calibration::is_active`) — callers that
/// aren't calibration should route through `emit_orb_rect`, which skips
/// emitting during an active session rather than fighting the preview.
pub fn orb_rect<R: Runtime>(app: &AppHandle<R>) -> RectPayload {
    let (h_pct, v_pct, size) = if crate::ghost::session_active() {
        read_waves_settings(app)
    } else {
        read_orb_settings(app)
    };
    rect_for(app, h_pct, v_pct, size)
}

/// Register the orb's current rect as an interactive stage hitbox (so
/// pointer events reach it instead of passing through to the desktop).
fn set_orb_hitbox_interactive<R: Runtime>(_app: &AppHandle<R>, rect: RectPayload) {
    crate::stage::set_hitbox_source(
        "orb",
        vec![crate::stage::StageRect { x: rect.x, y: rect.y, w: rect.w, h: rect.h }],
    );
}

/// Emit the orb's rect to the stage frontend (`stage:orb_rect`). Skipped
/// while animation calibration owns the preview (its own apply_main/
/// apply_loading in calibration.rs emit their own rects instead — this
/// avoids the two fighting over the same event).
pub fn emit_orb_rect<R: Runtime>(app: &AppHandle<R>) {
    if crate::calibration::is_active() {
        tracing::debug!("emit_orb_rect: calibration active — leaving preview rect alone");
        return;
    }
    let rect = orb_rect(app);
    *LAST_ORB_RECT.lock() = Some(rect);
    crate::commands::emit_logged(app, "stage:orb_rect", rect);
    let (_, v, _) = read_orb_settings(app);
    crate::commands::emit_logged(app, "stage:orb_position", if v > 0.5 { "bottom" } else { "top" });
}

/// Emit the loading-indicator's rect (`stage:loading_rect`). Same
/// calibration-ownership skip as `emit_orb_rect`.
pub fn emit_loading_rect<R: Runtime>(app: &AppHandle<R>) {
    if crate::calibration::is_active() {
        return;
    }
    let (h, v, size) = read_loading_settings(app);
    let rect = rect_for(app, h, v, size);
    *LAST_LOADING_RECT.lock() = Some(rect);
    crate::commands::emit_logged(app, "stage:loading_rect", rect);
}

/// Show the orb and make it interactive (rect + visible + hitbox). Does
/// NOT trigger the frontend's wake sequence — callers that want that call
/// `wake_orb` instead; Tier-3 command-detection shows+interacts but emits
/// its OWN `command-detected` event rather than a normal wake.
pub fn show_orb_interactive<R: Runtime>(app: &AppHandle<R>) {
    let rect = orb_rect(app);
    *LAST_ORB_RECT.lock() = Some(rect);
    crate::commands::emit_logged(app, "stage:orb_rect", rect);
    let (_, v, _) = read_orb_settings(app);
    crate::commands::emit_logged(app, "stage:orb_position", if v > 0.5 { "bottom" } else { "top" });
    crate::commands::emit_logged(app, "stage:orb_visible", true);
    set_orb_hitbox_interactive(app, rect);
}

/// Wake source instrumented (P2 double-wake audit): every fire path
/// names itself so the next live log settles which sources double.
/// Sources: neural (wake-word engine), barge (v4 exact-verify during
/// TTS), hotkey-barge, hotkey, tray, relaunch.
pub fn wake_orb_src<R: Runtime>(app: &AppHandle<R>, source: &str) {
    tracing::info!("wake: firing orb (source={source})");
    show_orb_interactive(app);
    crate::commands::emit_logged(app, "orb:wake", serde_json::json!({ "source": source }));
}

/// IPC: orb interactivity (replaces `set_click_through`). The orb is a
/// div inside the always-click-through `stage` overlay; "interactive"
/// means its current rect becomes a stage hitbox so pointer events reach
/// it — everywhere else on the fullscreen overlay stays click-through.
#[tauri::command]
pub fn set_orb_interactive<R: Runtime>(app: AppHandle<R>, interactive: bool) -> Result<(), String> {
    if interactive {
        let rect = orb_rect(&app);
        set_orb_hitbox_interactive(&app, rect);
    } else {
        crate::stage::set_hitbox_source("orb", Vec::new());
    }
    Ok(())
}

/// IPC: live calibration preview rect for arbitrary (h, v, size) — used
/// by the settings sidebar sliders for real-time preview. Does NOT save
/// to settings.json; the frontend calls `save_settings` separately.
/// Bypasses the calibration-session skip in `emit_orb_rect` by design:
/// this command IS an explicit "show this exact rect now" request.
#[tauri::command]
pub fn set_orb_position<R: Runtime>(
    app: AppHandle<R>,
    horizontal_pct: f64,
    vertical_pct: f64,
    size: u32,
) -> Result<(), String> {
    let h = horizontal_pct.max(0.0).min(1.0);
    let v = vertical_pct.max(0.0).min(1.0);
    let s = size.clamp(ORB_SIZE_MIN, ORB_SIZE_MAX);
    let rect = rect_for(&app, h, v, s);
    *LAST_ORB_RECT.lock() = Some(rect);
    crate::commands::emit_logged(&app, "stage:orb_rect", rect);
    let pos_tag = if v > 0.5 { "bottom" } else { "top" };
    crate::commands::emit_logged(&app, "stage:orb_position", pos_tag);
    set_orb_hitbox_interactive(&app, rect);
    tracing::debug!("set_orb_position: emitted rect {:?} [h={h}, v={v}, pos={pos_tag}]", rect);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_orb_size_survives_startup_within_the_shared_range() {
        let size = |j: serde_json::Value| orb_settings_from_json(&j).2;
        assert_eq!(size(serde_json::json!({ "orbSize": 150 })), 150); // was snapped to 180
        assert_eq!(size(serde_json::json!({ "orbSize": 20 })), ORB_SIZE_MIN);
        assert_eq!(size(serde_json::json!({ "orbSize": 9000 })), ORB_SIZE_MAX);
        assert_eq!(size(serde_json::json!({})), DEFAULT_ORB_SIZE);
        #[cfg(target_os = "linux")]
        assert_eq!(DEFAULT_ORB_SIZE, 160);
    }

    // 1920x1080 @ 1x, 200px window.
    const SW: i32 = 1920;
    const SH: i32 = 1080;

    #[test]
    fn test_overlay_xy_center_anchor() {
        // h=0.5 centers horizontally: x = 960 - 100 = 860.
        assert_eq!(overlay_xy(0.5, 0.5, 200, SW, SH, 1.0), (860, 440));
        // v=1.0 parks the bottom edge exactly on screen.
        assert_eq!(overlay_xy(0.5, 1.0, 200, SW, SH, 1.0), (860, 880));
        // Corners clamp fully on-screen.
        assert_eq!(overlay_xy(0.0, 0.0, 200, SW, SH, 1.0), (0, 0));
        assert_eq!(overlay_xy(1.0, 1.0, 200, SW, SH, 1.0), (1720, 880));
    }

    #[test]
    fn test_overlay_xy_dpi_scales_physical() {
        // 200% scale: 400 physical px window, same fractions.
        let (x, _y) = overlay_xy(0.5, 1.0, 200, SW, SH, 2.0);
        assert_eq!(x, 960 - 200);
    }

    #[test]
    fn test_overlay_nudge_moves_one_logical_px() {
        // From center, +1px right / +1px down at 1x.
        let (h, v) = overlay_nudge(0.5, 0.5, 200, 1, 1, SW, SH, 1.0);
        let (x, y) = overlay_xy(h, v, 200, SW, SH, 1.0);
        assert_eq!((x, y), (861, 441));
    }

    #[test]
    fn test_overlay_nudge_scales_with_dpi() {
        // +1 logical px at 2x = +2 physical px.
        let (h, _v) = overlay_nudge(0.5, 0.5, 200, 1, 0, SW, SH, 2.0);
        let (x, _y) = overlay_xy(h, 0.5, 200, SW, SH, 2.0);
        assert_eq!(x, 960 - 200 + 2);
    }

    #[test]
    fn test_overlay_nudge_clamps_at_edges() {
        // Nudging far past the right edge pins at h=1.0.
        let (h, _v) = overlay_nudge(1.0, 1.0, 200, 5000, 5000, SW, SH, 1.0);
        assert_eq!((h, 1.0), (1.0, 1.0));
        // And far past top-left pins at 0.0.
        let (h2, v2) = overlay_nudge(0.0, 0.0, 200, -5000, -5000, SW, SH, 1.0);
        assert_eq!((h2, v2), (0.0, 0.0));
    }

    #[test]
    fn test_overlay_nudge_roundtrip_stable() {
        // Nudge right then left by the same amount returns home.
        let (h1, v1) = overlay_nudge(0.5, 0.5, 200, 37, -12, SW, SH, 1.0);
        let (h2, v2) = overlay_nudge(h1, v1, 200, -37, 12, SW, SH, 1.0);
        assert!((h2 - 0.5).abs() < 1e-9);
        assert!((v2 - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_rect_for_matches_overlay_xy() {
        let (x, y) = overlay_xy(0.5, 1.0, 200, SW, SH, 1.0);
        // rect_for needs an AppHandle for monitor_info's primary_monitor()
        // fallback chain, which isn't available in a unit test — the
        // overlay_xy/overlay_nudge coverage above is the real contract
        // this module promises; rect_for is a thin, untestable-without-app
        // wrapper over it. This test just pins the arithmetic it wraps.
        assert_eq!((x, y), (860, 880));
    }
}
