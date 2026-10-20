//! Screen pointer and the pre-capture privacy gate.
//!
//! Two jobs live here, and they share one fact: **who is in front**.
//!
//! 1. The pointer marker (Phase 2 of the clicky-style screen agent). The
//!    marker renders inside the always-alive main stage window (no new window,
//!    no extra WebView RAM, no creation latency — the stage is transparent +
//!    click-through at idle and is never natively hidden). Rust evaluates the
//!    Table-B visibility conditions ([`decide`], pure + unit-tested) and emits
//!    `pointer:show` / `pointer:hide`; the frontend owns CSS positioning
//!    (devicePixelRatio-aware), dwell timing, and fade.
//!
//! 2. The **pre-capture gate** ([`capture_gate`]). Every screen capture — for
//!    vision, for the narrated tour, for the screen context — passes through
//!    it first. It refuses when the foreground window cannot be identified,
//!    because "I could not tell" is not "there is nothing to redact": a
//!    privacy control that quietly fails is worse than one that visibly
//!    refuses.
//!
//! The identity used everywhere is the **observed** one, reported by the
//! platform. Never the name the model supplied — a prompt injection can
//! rename its target, but it cannot change what the toolkit reports.

use tauri::{AppHandle, Emitter, Manager, Runtime};

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

/// Resolved pointer settings (clamped).
#[derive(Debug, Clone)]
pub struct PointerSettings {
    pub enabled: bool,
    pub dwell_ms: u64,
    pub excluded_apps: Vec<String>,
    pub suppress_fullscreen: bool,
}

impl Default for PointerSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            dwell_ms: 6000,
            excluded_apps: Vec::new(),
            suppress_fullscreen: true,
        }
    }
}

/// The on-disk name of the pointer/capture settings file.
///
/// It lives beside `settings.json` but is a **separate** file on purpose:
/// `save_settings` rewrites `settings.json` wholesale from the typed
/// `NexusSettings` struct, so any key it does not know about is dropped on the
/// next save. A dedicated file survives that, and it keeps this Linux-only
/// surface out of the shared settings struct.
const POINTER_SETTINGS_FILE: &str = "pointer.json";

/// The app's data directory, without an `AppHandle`.
///
/// `AppHandle::path()` is the authoritative source, but the capture gate also
/// runs inside the capture primitive, which has no handle. Tauri's Linux app
/// data directory is `$XDG_DATA_HOME/<identifier>`, and `dirs-next` is already
/// a dependency.
fn app_data_dir_fallback() -> Option<std::path::PathBuf> {
    let base = std::env::var("XDG_DATA_HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(dirs_next::data_dir)?;
    Some(base.join("com.nexus.assistant"))
}

/// Read the pointer/capture settings.
///
/// The exclusion list is empty by default, which does **not** weaken the gate:
/// the always-on protections (unknown foreground, sensitive window) do not
/// depend on it. It only adds the user's own list on top.
///
/// The file is hand-editable JSON:
///
/// ```json
/// { "excludedApps": ["bank", "1password"], "dwellSeconds": 6 }
/// ```
pub fn read_settings<R: Runtime>(app: &AppHandle<R>) -> PointerSettings {
    read_settings_from(app.path().app_data_dir().ok())
}

/// The same settings, read without an `AppHandle`.
///
/// Used by the capture primitive, which has no handle: the gate must be
/// enforceable at the point where the pixels are produced, not only where a
/// handle happens to be available.
pub fn read_settings_no_app() -> PointerSettings {
    read_settings_from(app_data_dir_fallback())
}

fn read_settings_from(dir: Option<std::path::PathBuf>) -> PointerSettings {
    let Some(dir) = dir else {
        return PointerSettings::default();
    };
    let path = dir.join(POINTER_SETTINGS_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return PointerSettings::default();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return PointerSettings::default();
    };
    let d = PointerSettings::default();
    PointerSettings {
        enabled: v
            .get("enabled")
            .and_then(|x| x.as_bool())
            .unwrap_or(d.enabled),
        dwell_ms: v
            .get("dwellSeconds")
            .and_then(|x| x.as_u64())
            .unwrap_or(d.dwell_ms / 1000)
            .clamp(3, 15)
            * 1000,
        excluded_apps: v
            .get("excludedApps")
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| s.as_str())
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        suppress_fullscreen: v
            .get("suppressFullscreen")
            .and_then(|x| x.as_bool())
            .unwrap_or(d.suppress_fullscreen),
    }
}

// ─── Screen settings page (`pointer_settings_get` / `pointer_settings_save`) ──

/// Most entries the exclusion list keeps, and the longest entry.
const MAX_EXCLUDED_APPS: usize = 64;
const MAX_EXCLUDED_APP_CHARS: usize = 128;

/// The settings as the Screen page sees them: `pointer.json`'s own keys.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointerSettingsDto {
    pub enabled: bool,
    pub dwell_seconds: u64,
    pub excluded_apps: Vec<String>,
    pub suppress_fullscreen: bool,
}

impl From<&PointerSettings> for PointerSettingsDto {
    fn from(s: &PointerSettings) -> Self {
        Self {
            enabled: s.enabled,
            dwell_seconds: s.dwell_ms / 1000,
            excluded_apps: s.excluded_apps.clone(),
            suppress_fullscreen: s.suppress_fullscreen,
        }
    }
}

/// Pure: trimmed, lowercased, de-duplicated (first wins), capped entries.
fn normalize_excluded(list: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in list {
        let entry: String = raw
            .trim()
            .to_lowercase()
            .chars()
            .take(MAX_EXCLUDED_APP_CHARS)
            .collect();
        if entry.is_empty() || out.contains(&entry) {
            continue;
        }
        out.push(entry);
        if out.len() == MAX_EXCLUDED_APPS {
            break;
        }
    }
    out
}

/// Pure: the new `pointer.json`. The page's four keys are replaced; any other
/// key a user hand-edited into the file is kept.
fn merged_pointer_json(existing: Option<&str>, s: &PointerSettingsDto) -> serde_json::Value {
    let mut obj = existing
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .and_then(|v| match v {
            serde_json::Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default();
    obj.insert("enabled".into(), s.enabled.into());
    obj.insert("dwellSeconds".into(), s.dwell_seconds.clamp(3, 15).into());
    obj.insert(
        "excludedApps".into(),
        serde_json::Value::from(normalize_excluded(&s.excluded_apps)),
    );
    obj.insert("suppressFullscreen".into(), s.suppress_fullscreen.into());
    serde_json::Value::Object(obj)
}

/// Write the page's settings into `dir/pointer.json` and return what a read
/// now sees (clamped), so the page shows exactly what is in force.
fn save_settings_in(
    dir: &std::path::Path,
    s: &PointerSettingsDto,
) -> Result<PointerSettingsDto, String> {
    let path = dir.join(POINTER_SETTINGS_FILE);
    let existing = std::fs::read_to_string(&path).ok();
    let json = merged_pointer_json(existing.as_deref(), s);
    let bytes = serde_json::to_vec_pretty(&json).map_err(|e| e.to_string())?;
    if !crate::memory::atomic_write(&path, &bytes) {
        return Err(format!("could not write {}", path.display()));
    }
    Ok(PointerSettingsDto::from(&read_settings_from(Some(
        dir.to_path_buf(),
    ))))
}

/// IPC: the Screen page's current settings.
#[tauri::command]
pub fn pointer_settings_get<R: Runtime>(app: AppHandle<R>) -> PointerSettingsDto {
    PointerSettingsDto::from(&read_settings(&app))
}

/// IPC: save the Screen page. Writes `pointer.json`, never `settings.json`
/// (which `save_settings` rewrites wholesale from its own struct).
#[tauri::command]
pub fn pointer_settings_save<R: Runtime>(
    app: AppHandle<R>,
    settings: PointerSettingsDto,
) -> Result<PointerSettingsDto, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app data dir: {e}"))?;
    save_settings_in(&dir, &settings)
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
    // Unidentified is not "unexcluded". Drawing a pointer over a window we
    // cannot vet leaks its layout to the screenshot behind it.
    if ctx.foreground_title.is_unknown() {
        return PointerDecision::Hide { reason: "foreground window unidentified" };
    }
    if let ForegroundTitle::Known(title) = &ctx.foreground_title {
        if exclusion_hit(title, &settings.excluded_apps).is_some() {
            return PointerDecision::Hide { reason: "foreground app is excluded" };
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
    pub fn is_unknown(&self) -> bool {
        matches!(self, ForegroundTitle::Unknown)
    }
}

/// The observed foreground window, as the platform reports it.
///
/// `Known` and `Unknown` are deliberately distinct. Collapsing them is the bug
/// `docs/features/research/22` listed as severity 1: the privacy gate returned
/// "no match" for "I could not tell", and bank windows got screenshotted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForegroundIdentity {
    Known { app: String, title: String },
    /// No mechanism available, or the platform reported an ambiguous answer.
    Unknown { reason: String },
}

impl ForegroundIdentity {
    pub fn is_known(&self) -> bool {
        matches!(self, ForegroundIdentity::Known { .. })
    }
    /// The app name, the title, or both — whichever the platform gave us.
    pub fn labels(&self) -> Vec<String> {
        match self {
            ForegroundIdentity::Known { app, title } => {
                let mut v = Vec::new();
                if !app.trim().is_empty() {
                    v.push(app.clone());
                }
                if !title.trim().is_empty() {
                    v.push(title.clone());
                }
                v
            }
            ForegroundIdentity::Unknown { .. } => Vec::new(),
        }
    }
}

/// Foreground window title, used by the privacy exclusion list.
///
/// Windows: `GetForegroundWindow` + `GetWindowTextW`.
///
/// Linux/X11 (a real X server): `xdotool getactivewindow getwindowname`. This
/// is the same mechanism `architect.rs` already uses, so it introduces no new
/// dependency — but it needs `$DISPLAY`, which native Wayland does not provide.
///
/// Linux/Wayland **and Linux/XWayland**: the AT-SPI tier, because there is no
/// portable compositor API (`org.gnome.Shell.Eval` has been disabled since
/// GNOME 41, and there is no "get active window" XDG portal). On XWayland the
/// X11 tools are *not* used even though `$DISPLAY` is set: they see only the
/// X11 compatibility windows, so the "active window" they report is frequently
/// not the window the user is looking at.
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
    match observed_identity() {
        ForegroundIdentity::Known { app, title } => {
            let label = if !title.trim().is_empty() {
                title
            } else {
                app
            };
            if label.trim().is_empty() {
                ForegroundTitle::Unknown
            } else {
                ForegroundTitle::Known(label)
            }
        }
        ForegroundIdentity::Unknown { .. } => ForegroundTitle::Unknown,
    }
}

/// The observed foreground identity: application name and window title.
///
/// This is the fact the whole privacy model rests on, so it is obtained the
/// careful way:
///
/// 1. **AT-SPI first.** The accessibility bus knows which application holds
///    the focused window, and it works on Wayland and X11 alike. It also
///    refuses to answer when more than one window claims to be active, which
///    is the correct answer rather than a guess.
/// 2. **X11 tools only on a real X server.** On XWayland they cannot see the
///    native Wayland windows at all.
/// 3. **Otherwise: unknown.** Which makes the capture gate refuse.
///
/// Blocking by necessity: the gate is called from the orchestrator before
/// capture, and the sidecar is a separate process. Cached briefly because the
/// answer cannot change within a few milliseconds and the sidecar round trip
/// is the most expensive thing on the pre-capture path.
#[cfg(all(target_os = "linux", not(feature = "mock-wake")))]
pub fn observed_identity() -> ForegroundIdentity {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    // (instant, answer) — a short cache, not a permanent one, so closing a
    // window is reflected promptly.
    static CACHE: Mutex<Option<(Instant, ForegroundIdentity)>> = Mutex::new(None);
    const TTL: Duration = Duration::from_millis(1500);

    if let Ok(guard) = CACHE.lock() {
        if let Some((at, id)) = guard.as_ref() {
            if at.elapsed() < TTL {
                return id.clone();
            }
        }
    }

    // The query itself runs on a thread of its own (see `semantic`), so this
    // is safe from any calling context — including inside an async task.
    let answer = blocking_focus_query();

    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some((Instant::now(), answer.clone()));
    }
    answer
}

#[cfg(all(target_os = "linux", feature = "mock-wake"))]
pub fn observed_identity() -> ForegroundIdentity {
    ForegroundIdentity::Unknown {
        reason: "mock wake build".into(),
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn observed_identity() -> ForegroundIdentity {
    ForegroundIdentity::Unknown {
        reason: "no foreground identity on this platform".into(),
    }
}

/// Ask the accessibility bus who is in front.
///
/// The reply is used verbatim, including its "ambiguous" verdict: several
/// windows claiming to be active is NOT resolved by picking the first one.
#[cfg(all(target_os = "linux", not(feature = "mock-wake")))]
fn blocking_focus_query() -> ForegroundIdentity {
    match crate::semantic::observed_foreground() {
        crate::atspi::Foreground::Known { app, title } => {
            // An identified app with an empty title is still an identification:
            // reporting Unknown would re-break the gate for titleless windows.
            // The app name is a worse but usable proxy.
            if app.trim().is_empty() && title.trim().is_empty() {
                ForegroundIdentity::Unknown {
                    reason: "the accessibility bus named no application".into(),
                }
            } else {
                ForegroundIdentity::Known { app, title }
            }
        }
        crate::atspi::Foreground::Unknown { reason } => {
            // Second source: the X11 tools — but only on a REAL X server. On
            // XWayland they see only the compatibility windows, so the window
            // they call "active" is frequently not the one in front.
            if crate::session::foreground_source().trusts_x11_tools() {
                if let Some(title) = x11_foreground_title() {
                    return ForegroundIdentity::Known {
                        app: String::new(),
                        title,
                    };
                }
            }
            ForegroundIdentity::Unknown { reason }
        }
    }
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

// ─── The capture gate ──────────────────────────────────────────────────────
//
// Everything above this line is the *pointer marker*. Everything below is the
// gate that decides whether pixels may leave the machine at all, and it is the
// part that matters on a desktop where the assistant shares the screen with
// the user's bank.

/// The verdict of the pre-capture gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureGate {
    /// Capture may proceed.
    Allow,
    /// The foreground window is on the user's privacy list.
    Excluded(String),
    /// The foreground window could not be identified, so nothing can be ruled
    /// out. Capture must be refused.
    UnknownForeground,
    /// The foreground window is a vault, a bank, a wallet or similar.
    SensitiveWindow,
}

impl CaptureGate {
    /// Whether the capture may proceed.
    pub fn allows_capture(&self) -> bool {
        matches!(self, CaptureGate::Allow)
    }

    /// A short sentence explaining the refusal, suitable for speaking.
    ///
    /// It names the user's own privacy-list entry (which the user typed) and
    /// never the protected window's title: the reason a vault is refused is
    /// the point, the vault's title is not.
    pub fn spoken(&self) -> String {
        match self {
            CaptureGate::Allow => String::new(),
            CaptureGate::Excluded(entry) => {
                format!("That's on your privacy list, sir — I won't look at {entry}.")
            }
            CaptureGate::UnknownForeground => {
                "I can't tell which window is in front, sir, so I won't risk looking at \
                 the screen."
                    .to_string()
            }
            CaptureGate::SensitiveWindow => {
                "That window holds credentials or money, sir — I won't look at it.".to_string()
            }
        }
    }
}

/// Pure: the capture gate over an observed identity and a privacy list.
///
/// The order matters and is deliberate:
///
/// 1. **The policy layer decides.** The built-in rules — a sensitive window
///    is refused, an unidentified window is refused — live in
///    [`crate::policy::decide`] and nowhere else, so there is exactly one
///    implementation of them.
/// 2. **The user's own privacy list**, on top: a case-insensitive substring
///    match against the OBSERVED app name and title.
///
/// The policy's `Decision` does not say WHICH rule fired, so the mapping back
/// to a spoken reason uses the same facts the decision was made from.
pub fn capture_gate_for(identity: &ForegroundIdentity, excluded: &[String]) -> CaptureGate {
    let known = identity.is_known();
    let sensitive = match identity {
        ForegroundIdentity::Known { app, title } => crate::policy::classify_window(app, title),
        ForegroundIdentity::Unknown { .. } => None,
    };

    // ONE implementation decides: the policy layer.
    match crate::policy::decide(crate::policy::Op::Capture, known, sensitive, None) {
        crate::policy::Decision::Allow => {}
        crate::policy::Decision::Deny { .. } => {
            return if sensitive.is_some() {
                CaptureGate::SensitiveWindow
            } else {
                // The window could not be identified, so nothing can be ruled
                // out. (A deny for any other reason is also treated as a
                // refusal: the gate fails closed.)
                CaptureGate::UnknownForeground
            };
        }
        crate::policy::Decision::Confirm { .. } => return CaptureGate::UnknownForeground,
    }

    // The user's own list, on top of the built-in rules.
    for label in identity.labels() {
        if let Some(hit) = exclusion_hit(&label, excluded) {
            return CaptureGate::Excluded(hit.clone());
        }
    }
    CaptureGate::Allow
}

/// The capture gate as it applies right now.
///
/// Used by the capture primitives themselves, so no path can reach the pixels
/// without passing the gate. The user's privacy list is read from disk when
/// it can be found; the always-on protections (unknown identity, sensitive
/// window) do not depend on it.
pub fn capture_gate_now() -> CaptureGate {
    let settings = read_settings_no_app();
    capture_gate_for(&observed_identity(), &settings.excluded_apps)
}

/// The capture gate with an `AppHandle` in hand (same verdict, plus the
/// user's list read from the app's own data directory).
pub fn capture_gate<R: Runtime>(app: &AppHandle<R>) -> CaptureGate {
    let settings = read_settings(app);
    capture_gate_for(&observed_identity(), &settings.excluded_apps)
}

/// Show the marker at exact physical pixels (locate-first: no VLM call, no
/// quota). Same Table-B conditions as the vision path.
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

    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nexus-pointer-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn screen_page_settings_round_trip_through_pointer_json() {
        let dir = scratch_dir("roundtrip");
        let saved = save_settings_in(
            &dir,
            &PointerSettingsDto {
                enabled: false,
                dwell_seconds: 9,
                excluded_apps: vec!["  Bank ".into(), "1Password".into()],
                suppress_fullscreen: false,
            },
        )
        .unwrap();
        assert_eq!(
            saved,
            PointerSettingsDto {
                enabled: false,
                dwell_seconds: 9,
                excluded_apps: vec!["bank".into(), "1password".into()],
                suppress_fullscreen: false,
            }
        );
        // The capture gate and the marker read the same file.
        let read = read_settings_from(Some(dir.clone()));
        assert!(!read.enabled);
        assert_eq!(read.dwell_ms, 9000);
        assert_eq!(read.excluded_apps, vec!["bank", "1password"]);
        assert!(!read.suppress_fullscreen);
        // excludedApps is stored as an array, as the hand-editable format says.
        let text = std::fs::read_to_string(dir.join(POINTER_SETTINGS_FILE)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(v["excludedApps"].is_array());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_clamps_dwell_and_keeps_hand_edited_keys() {
        let dir = scratch_dir("merge");
        std::fs::write(
            dir.join(POINTER_SETTINGS_FILE),
            r#"{ "excludedApps": ["old"], "myNote": "keep me" }"#,
        )
        .unwrap();
        let saved = save_settings_in(
            &dir,
            &PointerSettingsDto {
                enabled: true,
                dwell_seconds: 99,
                excluded_apps: vec![],
                suppress_fullscreen: true,
            },
        )
        .unwrap();
        assert_eq!(saved.dwell_seconds, 15);
        assert!(saved.excluded_apps.is_empty(), "the page's list replaces the old one");
        let text = std::fs::read_to_string(dir.join(POINTER_SETTINGS_FILE)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["myNote"], "keep me");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merged_json_replaces_a_corrupt_or_non_object_file() {
        let dto = PointerSettingsDto {
            enabled: true,
            dwell_seconds: 1,
            excluded_apps: vec!["a".into()],
            suppress_fullscreen: true,
        };
        for existing in [None, Some("not json"), Some("[1,2]")] {
            let v = merged_pointer_json(existing, &dto);
            assert_eq!(v["dwellSeconds"], 3);
            assert_eq!(v["excludedApps"], serde_json::json!(["a"]));
        }
    }

    #[test]
    fn excluded_apps_are_trimmed_deduplicated_and_capped() {
        let long = "x".repeat(MAX_EXCLUDED_APP_CHARS + 50);
        let mut input = vec![
            " Bank ".to_string(),
            "bank".to_string(),
            "".to_string(),
            "   ".to_string(),
            long,
        ];
        input.extend((0..100).map(|i| format!("app{i}")));
        let out = normalize_excluded(&input);
        assert_eq!(out[0], "bank");
        assert_eq!(out[1].chars().count(), MAX_EXCLUDED_APP_CHARS);
        assert_eq!(out.len(), MAX_EXCLUDED_APPS);
        assert_eq!(out.iter().filter(|s| s.as_str() == "bank").count(), 1);
    }

    #[test]
    fn dto_serializes_with_the_page_field_names() {
        let v = serde_json::to_value(PointerSettingsDto::from(&PointerSettings::default())).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "enabled": true,
                "dwellSeconds": 6,
                "excludedApps": [],
                "suppressFullscreen": true
            })
        );
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

    // ── The capture gate ──────────────────────────────────────────────────

    fn known(app: &str, title: &str) -> ForegroundIdentity {
        ForegroundIdentity::Known {
            app: app.into(),
            title: title.into(),
        }
    }

    fn unknown() -> ForegroundIdentity {
        ForegroundIdentity::Unknown {
            reason: "no answer".into(),
        }
    }

    #[test]
    fn an_unknown_foreground_refuses_the_capture() {
        // The whole point of the gate: "I could not tell" is not "nothing to
        // redact". It must refuse, whatever the exclusion list says.
        let g = capture_gate_for(&unknown(), &[]);
        assert_eq!(g, CaptureGate::UnknownForeground);
        assert!(!g.allows_capture());
    }

    #[test]
    fn an_unknown_foreground_refuses_even_with_an_empty_privacy_list() {
        // The old gate short-circuited on an empty list and returned "allow".
        // That is the fail-open the audit found: the list was configured and
        // silently doing nothing.
        let g = capture_gate_for(&unknown(), &[]);
        assert!(!g.allows_capture());
    }

    #[test]
    fn a_sensitive_window_refuses_the_capture() {
        for (app, title) in [
            ("1Password", "Vault"),
            ("firefox", "PayPal — Activity"),
            ("brave", "Zerodha — Holdings"),
        ] {
            let g = capture_gate_for(&known(app, title), &[]);
            assert_eq!(g, CaptureGate::SensitiveWindow, "{app}/{title}");
            assert!(!g.allows_capture());
        }
    }

    #[test]
    fn a_sensitive_window_refuses_even_when_the_list_does_not_mention_it() {
        // The built-in list is always on: it does not depend on the user
        // having configured anything.
        let g = capture_gate_for(&known("keepassxc", "My Vault"), &[]);
        assert_eq!(g, CaptureGate::SensitiveWindow);
    }

    #[test]
    fn the_users_own_privacy_list_refuses_the_capture() {
        // "steam" is not one of the built-in sensitive patterns, so this
        // exercises the user's own list rather than the built-in one.
        let excluded = vec!["steam".to_string()];
        let g = capture_gate_for(&known("firefox", "Steam — Community"), &excluded);
        assert_eq!(g, CaptureGate::Excluded("steam".to_string()));
        assert!(!g.allows_capture());
    }

    #[test]
    fn the_privacy_list_entry_is_named_because_the_user_typed_it() {
        // The user's own list is safe to echo; the window's title is not.
        let excluded = vec!["steam".to_string()];
        let g = capture_gate_for(&known("firefox", "Steam — Community"), &excluded);
        assert!(g.spoken().contains("steam"));
        assert!(!g.spoken().contains("Community"));
    }

    #[test]
    fn an_ordinary_window_is_allowed() {
        let g = capture_gate_for(&known("code", "main.rs — nexus"), &["bank".to_string()]);
        assert_eq!(g, CaptureGate::Allow);
        assert!(g.allows_capture());
    }

    #[test]
    fn the_spoken_reason_never_names_the_protected_window() {
        // The refusal must explain itself without disclosing the title of the
        // window it is protecting.
        let g = capture_gate_for(&known("1Password", "Vault — Chase"), &[]);
        let spoken = g.spoken();
        assert!(!spoken.is_empty());
        assert!(!spoken.contains("Chase"));
        assert!(!spoken.contains("1Password"));
    }

    #[test]
    fn an_allowed_gate_says_nothing() {
        assert!(CaptureGate::Allow.spoken().is_empty());
    }
}
