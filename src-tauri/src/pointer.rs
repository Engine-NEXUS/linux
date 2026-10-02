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
    pub foreground_title: ForegroundTitle,
    pub fullscreen_active: bool,
}

/// Pure: which exclusion entry (if any) matches a foreground title.
/// Case-insensitive substring. Shared by the overlay decision and the
/// pre-screenshot privacy gate (single rule, two enforcement points).
pub fn exclusion_hit<'a>(title: &str, excluded: &'a [String]) -> Option<&'a String> {
    let lower = title.to_lowercase();
    excluded.iter().find(|app| lower.contains(app.as_str()))
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
    match &ctx.foreground_title {
        ForegroundTitle::Known(title) => {
            if exclusion_hit(title, &settings.excluded_apps).is_some() {
                return PointerDecision::Hide { reason: "foreground app is excluded" };
            }
        }
        // Unidentified is not "unexcluded". Drawing a pointer over a window we
        // cannot vet leaks its layout to the screenshot behind it.
        ForegroundTitle::Unknown => {
            return PointerDecision::Hide { reason: "foreground window unidentified" };
        }
    }
    if settings.suppress_fullscreen && ctx.fullscreen_active {
        return PointerDecision::Hide { reason: "fullscreen app active" };
    }
    PointerDecision::Show { dwell_ms: settings.dwell_ms }
}

/// Whether the foreground window could be identified.
///
/// The distinction matters: `Unknown` is not "not excluded", it is "we cannot
/// tell", and treating those the same is how a privacy control ends up silently
/// inert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForegroundTitle {
    Known(String),
    /// No foreground-title mechanism available on this platform/session.
    Unknown,
}

impl ForegroundTitle {
    pub fn as_option(&self) -> Option<&str> {
        match self {
            ForegroundTitle::Known(t) => Some(t.as_str()),
            ForegroundTitle::Unknown => None,
        }
    }
    pub fn is_unknown(&self) -> bool {
        matches!(self, ForegroundTitle::Unknown)
    }
}

/// Foreground window title, used by the privacy exclusion list.
///
/// Windows: `GetForegroundWindow` + `GetWindowTextW`.
///
/// Linux/X11 (including XWayland): `xdotool getactivewindow getwindowname`. This
/// is the same mechanism `architect.rs` already uses, so it introduces no new
/// dependency — but it needs `$DISPLAY`, which native Wayland does not provide.
///
/// Linux/native Wayland: **`Unknown`.** There is no portable compositor API for
/// this. `org.gnome.Shell.Eval` has been disabled since GNOME 41, and there is
/// no "get active window" XDG portal. The portable route is the AT-SPI focused
/// accessible, which is Step 2 of the computer-control work order. Until then
/// this returns `Unknown` and `exclusion_gate` **refuses to capture** rather than
/// capturing a screen it cannot vet.
#[cfg(target_os = "windows")]
pub fn foreground_title() -> ForegroundTitle {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0 == 0 {
            return ForegroundTitle::Unknown;
        }
        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut buf);
        if len <= 0 {
            return ForegroundTitle::Unknown;
        }
        let title = String::from_utf16_lossy(&buf[..len as usize]).trim().to_string();
        if title.is_empty() {
            ForegroundTitle::Unknown
        } else {
            ForegroundTitle::Known(title)
        }
    }
}

#[cfg(all(target_os = "linux", not(feature = "mock-wake")))]
pub fn foreground_title() -> ForegroundTitle {
    if crate::session::is_wayland() {
        // No portable way to ask a Wayland compositor which window is focused.
        return ForegroundTitle::Unknown;
    }
    x11_foreground_title().map(ForegroundTitle::Known).unwrap_or(ForegroundTitle::Unknown)
}

#[cfg(all(target_os = "linux", feature = "mock-wake"))]
pub fn foreground_title() -> ForegroundTitle {
    ForegroundTitle::Unknown
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn foreground_title() -> ForegroundTitle {
    ForegroundTitle::Unknown
}

/// X11 active-window title via `xdotool`. Shared by the pointer privacy gate and
/// `architect.rs`.
#[cfg(target_os = "linux")]
pub fn x11_foreground_title() -> Option<String> {
    let out = std::process::Command::new("xdotool")
        .args(["getactivewindow", "getwindowname"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let title = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
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

/// Outcome of the pre-screenshot privacy gate.
///
/// This is an enum rather than `Option<String>` precisely so that "we could not
/// identify the foreground window" cannot be collapsed into "nothing matched".
/// That collapse is the bug this type exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExclusionVerdict {
    /// No exclusion applies. Capture may proceed.
    Allow,
    /// The foreground window matches an entry on the privacy list.
    Blocked(String),
    /// The foreground window could not be identified, so no match can be ruled
    /// out. Callers must refuse capture.
    UnknownForeground,
}

/// Pre-screenshot privacy gate.
///
/// Returns [`ExclusionVerdict::Blocked`] when the foreground window is on the
/// privacy list, and [`ExclusionVerdict::UnknownForeground`] when we cannot
/// identify it. Callers must refuse capture on both — no pixels, no element names.
///
/// ## Fail-closed by design
///
/// Before this change the gate short-circuited on `foreground_title()?`, so on
/// every non-Windows platform it returned `None` — indistinguishable from
/// "no match". `docs/features/research/22-…` recorded the consequence: banking
/// and password-manager windows were screenshotted and OCR'd with the privacy
/// list fully configured and silently doing nothing.
///
/// Refusing when the title is unknown trades a feature for a guarantee. On
/// native Wayland that means screen vision is unavailable until the AT-SPI tier
/// lands, which is the correct trade: a privacy control that quietly fails is
/// worse than one that visibly refuses.
pub fn exclusion_verdict<R: Runtime>(app: &AppHandle<R>) -> ExclusionVerdict {
    let settings = read_settings(app);
    if settings.excluded_apps.is_empty() {
        return ExclusionVerdict::Allow;
    }
    match foreground_title() {
        ForegroundTitle::Unknown => ExclusionVerdict::UnknownForeground,
        ForegroundTitle::Known(title) => match exclusion_hit(&title, &settings.excluded_apps) {
            Some(hit) => ExclusionVerdict::Blocked(hit.clone()),
            None => ExclusionVerdict::Allow,
        },
    }
}

/// Backwards-compatible convenience wrapper. `true` means capture must NOT
/// proceed — including when the foreground window is unknown.
pub fn exclusion_gate<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    match exclusion_verdict(app) {
        ExclusionVerdict::Allow => None,
        ExclusionVerdict::Blocked(app_name) => Some(app_name),
        ExclusionVerdict::UnknownForeground => Some(UNKNOWN_FOREGROUND.to_string()),
    }
}

pub const UNKNOWN_FOREGROUND: &str = "<foreground window unidentified>";

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
            let x = pt.x as f64 / 1000.0 * shot_w as f64;
            let y = pt.y as f64 / 1000.0 * shot_h as f64;
            emit_physical(app, dwell_ms, x, y, pt.label.clone())
        }
    }
}

/// Show the marker at exact physical pixels (Phase-3a UIA locate-first:
/// no VLM call, no quota). Same Table-B conditions as the vision path.
pub fn show_direct<R: Runtime>(app: &AppHandle<R>, x: f64, y: f64, label: String) -> bool {
    let settings = read_settings(app);
    let ctx = PointerContext {
        response_is_locate: true,
        has_point: true,
        foreground_title: foreground_title(),
        fullscreen_active: fullscreen_active(),
    };
    match decide(&ctx, &settings) {
        PointerDecision::Hide { reason } => {
            tracing::info!("pointer: hidden ({reason})");
            false
        }
        PointerDecision::Show { dwell_ms } => emit_physical(app, dwell_ms, x, y, label),
    }
}

/// Emit `pointer:show` for physical-pixel coordinates. Shared by the
/// vision path (normalized coords) and the UIA direct path.
fn emit_physical<R: Runtime>(
    app: &AppHandle<R>,
    dwell_ms: u64,
    x: f64,
    y: f64,
    label: String,
) -> bool {
    let show = PointerShow { x, y, label, dwell_ms };
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

/// Emit `pointer:hide` (tray/menu path; the frontend also hides on new
/// turns and dwell expiry by itself).
#[tauri::command]
pub fn hide_pointer<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    emit_hide(&app)
}

/// Non-command hide for in-process callers (stop-speech, repeat flows).
pub fn emit_hide<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
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
            foreground_title: ForegroundTitle::Known("VS Code".to_string()),
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
            PointerContext { response_is_locate: false, has_point: false, foreground_title: ForegroundTitle::Unknown, fullscreen_active: false },
            PointerContext { response_is_locate: true, has_point: false, foreground_title: ForegroundTitle::Unknown, fullscreen_active: false },
        ] {
            assert!(matches!(decide(&c, &settings()), PointerDecision::Hide { .. }));
        }
    }

    #[test]
    fn excluded_app_hides_case_insensitive() {
        let c = PointerContext {
            foreground_title: ForegroundTitle::Known("My BANK - Chrome".to_string()),
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
    fn unknown_foreground_title_hides_the_pointer() {
        // This test used to assert the opposite (`no_foreground_title_never_excludes`
        // → Show). That encoded the vulnerability: with no title, the exclusion
        // list could not be consulted, so the pointer drew over whatever window
        // happened to be in front — including a banking app. Unidentified is not
        // "unexcluded", so the pointer is suppressed instead.
        let c = PointerContext { foreground_title: ForegroundTitle::Unknown, ..ctx() };
        assert!(matches!(decide(&c, &settings()), PointerDecision::Hide { .. }));
    }

    #[test]
    fn known_and_unexcluded_foreground_shows_the_pointer() {
        // The fail-closed branch must not swallow the ordinary case: once we can
        // name the window and it is not on the list, the pointer is allowed.
        let c = PointerContext {
            foreground_title: ForegroundTitle::Known("VS Code".to_string()),
            ..ctx()
        };
        assert!(matches!(decide(&c, &settings()), PointerDecision::Show { .. }));
    }

    #[test]
    fn exclusion_hit_matches_substrings() {
        let excluded = vec!["bank".to_string(), "1password".to_string()];
        assert_eq!(
            exclusion_hit("My BANK - Chrome", &excluded).map(|s| s.as_str()),
            Some("bank")
        );
        assert!(exclusion_hit("VS Code", &excluded).is_none());
        assert!(exclusion_hit("anything", &[]).is_none());
    }
}
