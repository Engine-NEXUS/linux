//! On-screen animation calibration (direct drag + scroll-wheel scaling).
//!
//! Entry: Command Hub → "Drag & Position on Desktop" → `show_calibration_hud`.
//! The companion pill HUD (440×54, top-center) selects the target
//! (Wakeup/Waves/Loading); the preview window receives native drag
//! (`startDragging`) and wheel events and reports normalized pct / size
//! back here. Drafts live in Rust (`CALIBRATION`), so both WebViews stay
//! dumb: they only report and render. Save writes the 9 parameters to
//! settings.json (read-modify-write of exactly those keys — unknown fields
//! survive); Cancel restores the pre-calibration snapshot. Both end paths
//! destroy the HUD and re-open the Command Hub.
//!
//! Waves runtime note: the waves render inside the orb's stage-hosted
//! rect (Avatar `ghost-waves`, absolute inset-0 of its OrbFrame div). The
//! saved waves rect is applied to that same rect during ghost sessions —
//! `window_manager::orb_rect` branches on `ghost::session_active()` — so
//! `waves_*` settings have real runtime meaning without a second runtime
//! window (RAM law).
//!
//! Preview windows (single-stage migration): Wakeup/Waves/Loading
//! previews all render as positioned divs inside the one `stage`
//! fullscreen overlay now, not separate OS windows. `apply_main`/
//! `apply_loading` below emit rects + register stage hitboxes (so the
//! user can grab and drag them) instead of moving/showing real windows.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

// ─── Model ──────────────────────────────────────────────────────────────

/// One positionable animation: center-anchored fractions + logical px.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    pub h: f64,
    pub v: f64,
    pub size: u32,
}

impl Draft {
    const fn new(h: f64, v: f64, size: u32) -> Self {
        Self { h, v, size }
    }
}

/// Calibration targets, indexed 0..2 for the draft array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Wakeup = 0,
    Waves = 1,
    Loading = 2,
}

impl Target {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "wakeup" => Some(Self::Wakeup),
            "waves" => Some(Self::Waves),
            "loading" => Some(Self::Loading),
            _ => None,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::Wakeup => "wakeup",
            Self::Waves => "waves",
            Self::Loading => "loading",
        }
    }
}

/// Per-target size clamp rails (task §4; orb/waves max raised 300→400
/// for the fullscreen-era orb so the particle sphere can breathe). Orb/waves
/// share the startup range so a calibrated size survives a restart.
pub fn clamp_size(target: Target, size: u32) -> u32 {
    use crate::window_manager::{ORB_SIZE_MAX, ORB_SIZE_MIN};
    let (min, max) = match target {
        Target::Wakeup | Target::Waves => (ORB_SIZE_MIN, ORB_SIZE_MAX),
        Target::Loading => (40, 160),
    };
    size.max(min).min(max)
}

/// Per-target size step for wheel events (+10 / −10).
#[allow(dead_code)]
pub fn wheel_step(delta_y: f64) -> i32 {
    if delta_y < 0.0 {
        10
    } else {
        -10
    }
}

/// Defaults per target (waves default = the user invariant: wherever
/// wakeup is, waves are; loading default ≈ today's top-right corner).
pub fn default_draft(target: Target) -> Draft {
    match target {
        Target::Wakeup => Draft::new(0.5, 1.0, 200),
        Target::Waves => Draft::new(0.5, 1.0, 200),
        Target::Loading => Draft::new(0.95, 0.05, 80),
    }
}

// ─── Session state ──────────────────────────────────────────────────────

struct CalibrationSession {
    target: Target,
    /// Pre-session snapshot (disk values at entry). Compared against
    /// `draft` for the HUD dirty dots + the scoped save toast; never
    /// written — cancel restores by re-reading the (unwritten) disk.
    initial: [Draft; 3],
    draft: [Draft; 3],
}

static CALIBRATION: once_cell::sync::Lazy<parking_lot::Mutex<Option<CalibrationSession>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

/// True while the calibration HUD is live. `window_manager::position_orb`
/// early-returns on this so show-path repositioning never fights the drag.
pub fn is_active() -> bool {
    CALIBRATION.lock().is_some()
}

fn session_target() -> Option<Target> {
    CALIBRATION.lock().as_ref().map(|s| s.target)
}

fn draft_for(target: Target) -> Option<Draft> {
    CALIBRATION
        .lock()
        .as_ref()
        .map(|s| s.draft[target as usize])
}

fn set_draft(target: Target, d: Draft) {
    if let Some(s) = CALIBRATION.lock().as_mut() {
        s.draft[target as usize] = d;
    }
}

// ─── Apply (Rust owns every window mutation) ────────────────────────────

/// Apply a draft to the orb's stage-hosted preview (Wakeup/Waves) —
/// emit its rect and register it as an interactive stage hitbox (drag
/// target) for the duration of the calibration session.
fn apply_main<R: Runtime>(app: &AppHandle<R>, d: Draft) {
    let rect = crate::window_manager::rect_for(app, d.h, d.v, d.size);
    crate::window_manager::note_orb_rect(rect);
    let _ = app.emit("stage:orb_rect", rect);
    let _ = app.emit("stage:orb_visible", true);
    crate::stage::set_hitbox_source(
        "orb",
        vec![crate::stage::StageRect { x: rect.x, y: rect.y, w: rect.w, h: rect.h }],
    );
}

/// Apply a draft to the loading indicator's stage-hosted preview —
/// emit its rect and register it as an interactive stage hitbox.
fn apply_loading<R: Runtime>(app: &AppHandle<R>, d: Draft) {
    let rect = crate::window_manager::rect_for(app, d.h, d.v, d.size);
    crate::window_manager::note_loading_rect(rect);
    let _ = app.emit("stage:loading_rect", rect);
    let _ = app.emit("stage:loading_visible", true);
    crate::stage::set_hitbox_source(
        "loading",
        vec![crate::stage::StageRect { x: rect.x, y: rect.y, w: rect.w, h: rect.h }],
    );
}

/// Apply the active target's draft to its preview window and broadcast
/// the state event (HUD selector + preview badges react).
fn apply_active<R: Runtime>(app: &AppHandle<R>) {
    let Some(target) = session_target() else { return };
    let Some(d) = draft_for(target) else { return };
    match target {
        Target::Wakeup | Target::Waves => apply_main(app, d),
        Target::Loading => apply_loading(app, d),
    }
    emit_state(app);
}

/// Broadcast current calibration state to every window. A second delayed
/// emit re-syncs windows created ON DEMAND (the loading window's WebView
/// may still be loading when the first event fires). `dirty` tells the
/// HUD which targets the user actually positioned (✓ dots) — untouched
/// targets keep their disk values on save.
fn emit_state<R: Runtime>(app: &AppHandle<R>) {
    let (target, draft, dirty) = match CALIBRATION.lock().as_ref() {
        Some(s) => {
            let d = s.draft[s.target as usize];
            let dirty = [
                s.draft[0] != s.initial[0],
                s.draft[1] != s.initial[1],
                s.draft[2] != s.initial[2],
            ];
            (s.target, d, dirty)
        }
        None => return,
    };
    let payload = serde_json::json!({
        "active": true,
        "target": target.as_str(),
        "h": draft.h,
        "v": draft.v,
        "size": draft.size,
        "dirty": {
            "wakeup": dirty[0],
            "waves": dirty[1],
            "loading": dirty[2],
        },
    });
    let _ = app.emit("calibration:state", &payload);
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        let _ = app2.emit("calibration:state", &payload);
    });
}

/// Scoped save toast (pure, unit-tested): names exactly the targets the
/// user positioned, so partial saves ("save 1 animation too") read back
/// truthfully instead of claiming everything.
pub fn save_toast(dirty: [bool; 3]) -> String {
    let mut names = Vec::new();
    if dirty[0] {
        names.push("Wakeup");
    }
    if dirty[1] {
        names.push("Waves");
    }
    if dirty[2] {
        names.push("Loading");
    }
    match names.len() {
        0 => "No changes — placement unchanged.".to_string(),
        3 => "All 3 animations saved.".to_string(),
        _ => format!("{} saved.", names.join(" + ")),
    }
}

// ─── End-of-session restore ─────────────────────────────────────────────

/// End the calibration session: restore windows to their SAVED placement
/// (`position_orb` reads the disk — by this point save() has already
/// written the new values, so save and cancel share this restore path),
/// destroy the HUD, re-open the Command Hub, toast the outcome.
fn end_session<R: Runtime>(app: &AppHandle<R>, toast: &str) {
    // Drop both preview hitboxes — neither should stay grabbable once the
    // session ends — and restore the orb to its SAVED placement (CALIBRATION
    // is cleared further down, so emit_orb_rect's is_active() check passes
    // and it reads disk, which by now holds whatever Save/Cancel settled on).
    crate::stage::set_hitbox_source("orb", Vec::new());
    crate::stage::set_hitbox_source("loading", Vec::new());
    let _ = app.emit("stage:loading_visible", false);
    // Destroy the HUD.
    let _ = crate::dyn_windows::destroy_window(app, "calibrate-toolbar");
    *CALIBRATION.lock() = None;
    // Now that is_active() is false, emit_orb_rect reads the SAVED
    // placement from disk (save() already wrote it; cancel() never did,
    // so disk still holds the pre-session values — the same restore
    // either way) and restores normal wake-time positioning.
    crate::window_manager::emit_orb_rect(app);
    // Broadcast session end so preview windows clean up badges/state.
    let _ = app.emit(
        "calibration:state",
        serde_json::json!({ "active": false }),
    );
    // Re-open the Command Hub.
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::commands::show_settings_sidebar(app2).await {
            tracing::warn!("calibration: re-show settings sidebar failed: {e}");
        }
    });
    let _ = app.emit(
        "settings:toast",
        serde_json::json!({ "message": toast }),
    );
}

// ─── IPC commands ───────────────────────────────────────────────────────

/// Start the session on Wakeup with drafts snapshot from disk.
/// Cancel semantics: nothing is written until Save — the HUD's Undo
/// history + Rust's per-window repositioning from DISK on end cover the
/// restore (by save-time the disk holds the new values; by cancel-time
/// it still holds the originals, so re-reading IS the restore).
/// The `initial` copy feeds dirty dots + the scoped save toast.
#[tauri::command]
pub async fn show_calibration_hud<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let orb = crate::window_manager::read_orb_settings(&app);
    let waves = crate::window_manager::read_waves_settings(&app);
    let loading = crate::window_manager::read_loading_settings(&app);
    let initial = [
        Draft::new(orb.0, orb.1, orb.2),
        Draft::new(waves.0, waves.1, waves.2),
        Draft::new(loading.0, loading.1, loading.2),
    ];
    *CALIBRATION.lock() = Some(CalibrationSession {
        target: Target::Wakeup,
        initial,
        draft: initial,
    });

    // Wakeup preview active immediately (orb window follows the draft).
    apply_active(&app);

    // Companion pill HUD — top-center of the primary monitor.
    let hud = crate::dyn_windows::get_or_create_window(
        &app,
        crate::dyn_windows::WindowConfig::calibrate_toolbar(),
    )?;
    if let Ok(Some(monitor)) = hud.current_monitor() {
        let scale = monitor.scale_factor();
        let screen = monitor.size();
        let phys_w = (540.0f64 * scale) as i32;
        let x = ((screen.width as i32 - phys_w) / 2).max(0);
        let y = (20.0 * scale) as i32;
        let _ = hud.set_position(tauri::PhysicalPosition::new(x, y));
    }
    let _ = hud.set_ignore_cursor_events(false);
    let _ = hud.show();
    let _ = hud.set_focus();
    tracing::info!("calibration: HUD shown (wakeup preview active)");
    Ok(())
}

/// Switch the desktop preview to another target (HUD segmented selector).
#[tauri::command]
pub async fn calibration_set_target<R: Runtime>(
    app: AppHandle<R>,
    target: String,
) -> Result<(), String> {
    let Some(t) = Target::from_str(&target) else {
        return Err(format!("calibration: unknown target '{target}'"));
    };
    {
        let mut guard = CALIBRATION.lock();
        let Some(s) = guard.as_mut() else {
            return Err("calibration: no active session".into());
        };
        s.target = t;
    }
    // One preview at a time: hide whichever stage-hosted preview is not
    // the target (and drop its drag hitbox — a hidden preview shouldn't
    // still be grabbable).
    match t {
        Target::Wakeup | Target::Waves => {
            let _ = app.emit("stage:loading_visible", false);
            crate::stage::set_hitbox_source("loading", Vec::new());
        }
        Target::Loading => {
            let _ = app.emit("stage:orb_visible", false);
            crate::stage::set_hitbox_source("orb", Vec::new());
        }
    }
    apply_active(&app);
    Ok(())
}

/// Preview window reports a drag-release position (already snapped in the
/// frontend's pure geometry). Clamped, applied, broadcast.
#[tauri::command]
pub async fn calibration_report_position<R: Runtime>(
    app: AppHandle<R>,
    h_pct: f64,
    v_pct: f64,
) -> Result<(), String> {
    let Some(target) = session_target() else { return Ok(()) };
    let h = h_pct.clamp(0.0, 1.0);
    let v = v_pct.clamp(0.0, 1.0);
    let size = draft_for(target).map(|d| d.size).unwrap_or(200);
    set_draft(target, Draft::new(h, v, size));
    apply_active(&app);
    tracing::info!("calibration: {} → h={h:.3} v={v:.3}", target.as_str());
    Ok(())
}

/// Preview window reports a wheel-resized logical size. Clamped per
/// target, applied, broadcast (badge renders it).
#[tauri::command]
pub async fn calibration_report_size<R: Runtime>(
    app: AppHandle<R>,
    size: u32,
) -> Result<(), String> {
    let Some(target) = session_target() else { return Ok(()) };
    let clamped = clamp_size(target, size);
    let (h, v) = draft_for(target).map(|d| (d.h, d.v)).unwrap_or((0.5, 1.0));
    set_draft(target, Draft::new(h, v, clamped));
    apply_active(&app);
    tracing::info!("calibration: {} size → {clamped}px", target.as_str());
    Ok(())
}

/// HUD Undo/Default: apply a full 3-draft snapshot (the HUD owns the
/// history); the active target's window visibly reflects it.
#[tauri::command]
pub async fn calibration_apply_drafts<R: Runtime>(
    app: AppHandle<R>,
    drafts: [Draft; 3],
) -> Result<(), String> {
    {
        let mut guard = CALIBRATION.lock();
        let Some(s) = guard.as_mut() else { return Ok(()) };
        for (i, d) in drafts.iter().enumerate() {
            s.draft[i] = Draft::new(
                d.h.clamp(0.0, 1.0),
                d.v.clamp(0.0, 1.0),
                clamp_size(
                    match i {
                        0 => Target::Wakeup,
                        1 => Target::Waves,
                        _ => Target::Loading,
                    },
                    d.size,
                ),
            );
        }
    }
    apply_active(&app);
    Ok(())
}

/// HUD ↺ Default: reset the ACTIVE target to its defaults.
#[tauri::command]
pub async fn calibration_default_target<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let Some(target) = session_target() else { return Ok(()) };
    set_draft(target, default_draft(target));
    apply_active(&app);
    tracing::info!("calibration: {} reset to defaults", target.as_str());
    Ok(())
}

/// HUD arrow keys: shift the ACTIVE target by logical px (1px, or 10px
/// with Shift held — the frontend sends the resolved delta). Math lives
/// in `overlay_nudge` (same conversion path as drag/wheel), so the
/// element moves exactly under the keypress at any DPI.
#[tauri::command]
pub async fn calibration_nudge<R: Runtime>(
    app: AppHandle<R>,
    dx_px: i32,
    dy_px: i32,
) -> Result<(), String> {
    let Some(target) = session_target() else { return Ok(()) };
    let (sw, sh, scale) = crate::window_manager::monitor_info(&app);
    let (h, v, size) = match draft_for(target) {
        Some(d) => (d.h, d.v, d.size),
        None => return Ok(()),
    };
    let (nh, nv) = crate::window_manager::overlay_nudge(h, v, size, dx_px, dy_px, sw, sh, scale);
    set_draft(target, Draft::new(nh, nv, size));
    apply_active(&app);
    tracing::info!("calibration: {} nudged ({dx_px},{dy_px})", target.as_str());
    Ok(())
}

/// ✓ Save: write the 9 parameters to settings.json (read-modify-write of
/// exactly those keys — unknown fields survive), then restore + close.
/// Untouched targets keep their disk values; the toast names what moved.
#[tauri::command]
pub async fn calibration_save<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let (drafts, dirty) = {
        let guard = CALIBRATION.lock();
        let Some(s) = guard.as_ref() else {
            return Err("calibration: no active session".into());
        };
        (
            s.draft,
            [
                s.draft[0] != s.initial[0],
                s.draft[1] != s.initial[1],
                s.draft[2] != s.initial[2],
            ],
        )
    };
    write_params(&app, drafts)?;
    end_session(&app, &save_toast(dirty));
    tracing::info!("calibration: saved");
    Ok(())
}

/// ✕ Cancel / Esc: restore the pre-calibration snapshot (nothing was
/// written to disk, so re-reading saved settings IS the restore), no write.
#[tauri::command]
pub async fn calibration_cancel<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    {
        let guard = CALIBRATION.lock();
        if guard.is_none() {
            return Ok(());
        }
    }
    end_session(&app, "Calibration cancelled — original placement restored.");
    tracing::info!("calibration: cancelled (initial snapshot restored)");
    Ok(())
}

/// Read-modify-write exactly the 9 calibration keys of settings.json.
/// Unknown fields survive (struct round-trips would drop them).
fn write_params<R: Runtime>(app: &AppHandle<R>, drafts: [Draft; 3]) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("settings.json");
    let mut json: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let keys = [
        ("orbHorizontalPct", drafts[0].h),
        ("orbVerticalPct", drafts[0].v),
        ("orbSize", drafts[0].size as f64),
        ("wavesHorizontalPct", drafts[1].h),
        ("wavesVerticalPct", drafts[1].v),
        ("wavesSize", drafts[1].size as f64),
        ("loadingHorizontalPct", drafts[2].h),
        ("loadingVerticalPct", drafts[2].v),
        ("loadingSize", drafts[2].size as f64),
    ];
    for (k, v) in keys {
        json[k] = serde_json::json!(v);
    }
    std::fs::write(&path, serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// ─── TEMPORARY dev tooling (remove before release) ────────────────────
/// Show ONLY the companion pill (no session, no preview moves, no sidebar
/// changes) so its styling can be cross-checked lively across rebuilds:
/// pop it, look, rebuild CSS, relaunch, repeat. Idempotent — safe to call
/// while a session is live (the pill is already up).
#[tauri::command]
pub async fn preview_companion_hud<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let hud = crate::dyn_windows::get_or_create_window(
        &app,
        crate::dyn_windows::WindowConfig::calibrate_toolbar(),
    )?;
    if let Ok(Some(monitor)) = hud.current_monitor() {
        let scale = monitor.scale_factor();
        let screen = monitor.size();
        let phys_w = (540.0f64 * scale) as i32;
        let x = ((screen.width as i32 - phys_w) / 2).max(0);
        let y = (20.0 * scale) as i32;
        let _ = hud.set_position(tauri::PhysicalPosition::new(x, y));
    }
    let _ = hud.set_ignore_cursor_events(false);
    let _ = hud.show();
    let _ = hud.set_focus();
    tracing::info!("calibration: companion pill preview shown (no session)");
    Ok(())
}

/// `calibration_dev_persist(true)` keeps the calibrator on display across
/// restarts so live cross-checks survive rebuilds: boot auto-opens the HUD
/// + wakeup preview after a short delay. This bypasses the normal Command
/// Hub entry flow and leaves a debug window open — TEMPORARY ONLY.
#[tauri::command]
pub async fn calibration_dev_persist<R: Runtime>(
    app: AppHandle<R>,
    enabled: bool,
) -> Result<bool, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("settings.json");
    let mut json: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    json["calibrationDevPersist"] = serde_json::json!(enabled);
    std::fs::write(&path, serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    tracing::warn!(
        "calibration: TEMP dev-persist {}",
        if enabled { "ON" } else { "OFF" }
    );
    Ok(enabled)
}

/// TEMPORARY: read the dev-persist flag (boot auto-open check).
pub fn dev_persist_enabled<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Ok(dir) = app.path().app_data_dir() else {
        return false;
    };
    let Ok(content) = std::fs::read_to_string(dir.join("settings.json")) else {
        return false;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    json.get("calibrationDevPersist")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_str_roundtrip() {
        for name in ["wakeup", "waves", "loading"] {
            let t = Target::from_str(name).expect(name);
            assert_eq!(t.as_str(), name);
        }
        assert!(Target::from_str("orb").is_none());
        assert!(Target::from_str("").is_none());
    }

    #[test]
    fn test_target_indices_distinct() {
        assert_ne!(Target::Wakeup as usize, Target::Waves as usize);
        assert_ne!(Target::Waves as usize, Target::Loading as usize);
        assert_eq!(Target::Wakeup as usize, 0);
    }

    #[test]
    fn test_clamp_size_rails() {
        // Wakeup/Waves: 80–400.
        assert_eq!(clamp_size(Target::Wakeup, 50), 80);
        assert_eq!(clamp_size(Target::Wakeup, 80), 80);
        assert_eq!(clamp_size(Target::Wakeup, 200), 200);
        assert_eq!(clamp_size(Target::Wakeup, 500), 400);
        assert_eq!(clamp_size(Target::Waves, 40), 80);
        assert_eq!(clamp_size(Target::Waves, 310), 310);
        assert_eq!(clamp_size(Target::Waves, 450), 400);
        // Loading: 40–160.
        assert_eq!(clamp_size(Target::Loading, 10), 40);
        assert_eq!(clamp_size(Target::Loading, 80), 80);
        assert_eq!(clamp_size(Target::Loading, 200), 160);
    }

    #[test]
    fn test_wheel_step_sign() {
        assert_eq!(wheel_step(-100.0), 10); // up = grow
        assert_eq!(wheel_step(0.0), -10); // down/zero = shrink
        assert_eq!(wheel_step(42.0), -10);
    }

    #[test]
    fn test_default_drafts_match_invariants() {
        // User invariant: waves default = wakeup default (same spot).
        let wake = default_draft(Target::Wakeup);
        let waves = default_draft(Target::Waves);
        assert_eq!(wake, waves);
        // Loading default ≈ today's top-right corner (center-anchored 0.95/0.05).
        let load = default_draft(Target::Loading);
        assert_eq!(load, Draft::new(0.95, 0.05, 80));
    }

    #[test]
    fn test_draft_bounds() {
        let d = Draft::new(2.0, -1.0, 999);
        // Clamping is applied at command boundaries; raw Draft carries values.
        assert_eq!(d.h, 2.0);
        // The command-layer clamp math used by report/apply:
        let h = d.h.max(0.0).min(1.0);
        let v = d.v.max(0.0).min(1.0);
        assert_eq!((h, v), (1.0, 0.0));
    }

    #[test]
    fn test_save_toast_scopes() {
        assert_eq!(save_toast([false, false, false]), "No changes — placement unchanged.");
        assert_eq!(save_toast([true, false, false]), "Wakeup saved.");
        assert_eq!(save_toast([false, true, true]), "Waves + Loading saved.");
        assert_eq!(save_toast([true, false, true]), "Wakeup + Loading saved.");
        assert_eq!(save_toast([true, true, true]), "All 3 animations saved.");
    }

    #[test]
    fn test_draft_equality_drives_dirty() {
        // Dirty detection is exact Draft equality (same source values).
        let a = Draft::new(0.5, 1.0, 200);
        let b = Draft::new(0.5, 1.0, 200);
        assert_eq!(a, b);
        let c = Draft::new(0.51, 1.0, 200);
        assert_ne!(a, c);
    }
}
