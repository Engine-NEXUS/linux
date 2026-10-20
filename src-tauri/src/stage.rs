//! Single fullscreen stage window + blackout watchdog.
//!
//! Step 1 (2026-09-25) proved the shell empty and parallel-run: windowing
//! (fullscreen transparent overlay, hitbox click-through, video-safe
//! toolwindow flag) and resilience (heartbeat, blackout auto-shutdown,
//! background auto-fix) first, before anything depended on it.
//!
//! Step 2 (this pass) moves the voice orb and loading indicator in as
//! positioned divs (see `window_manager.rs`'s orb_rect/emit_loading_rect
//! + the frontend's OrbFrame/LoadingIndicator components) — `stage` is
//! now shown at boot and stays up for the life of the app, instead of the
//! old on-demand/ghost-session-only model. A blackout now briefly hides
//! the orb + captions too, not just the ghost ring; the existing
//! auto-fix watchdog below is unchanged and still recovers within
//! seconds — this tradeoff was discussed and accepted.
//!
//! Blackout policy (user requirement: never a full blackout):
//! - DETECT: window missing OR renderer heartbeat stale >6s while marked
//!   visible (2s watchdog cadence; 8s grace after (re)show for cold boot).
//! - SHUTDOWN: destroy the dead window immediately — a black fullscreen
//!   surface is worse than no surface. Mark not-visible.
//! - INFORM: one orb-spoken message per incident (voice-first product;
//!   skipped when a real user turn is active so it never talks over one).
//! - AUTO-FIX: exponential backoff recreate (2s, 4s, 8s — budget caps
//!   attempts before the cap could matter), wait up to 10s for a fresh
//!   heartbeat, then healthy (fail counter reset). After 3 failed fixes:
//!   stop recreating, one final orb message, stay hidden until restart
//!   or manual `stage_show`.
//! - KILL-SWITCH: Ctrl+Alt+X destroys the stage and disables it for the
//!   session (watchdog + hitbox loop go quiet). `stage_show` re-arms.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

const LABEL: &str = "stage";
const HEARTBEAT_STALE_SECS: u64 = 10;
/// Cold-boot grace before a silent renderer counts as a blackout. The 2026-10-10 log shows a
/// cold WebView2 (profile cleared at every start) missing the old 20 s window.
const SHOW_GRACE_SECS: u64 = 45;
const MAX_AUTO_FIXES: u32 = 3;

/// Interactive rect in PHYSICAL pixels (frontend multiplies CSS px by
/// devicePixelRatio before sending). The hitbox loop opens mouse holes
/// exactly on these; everywhere else stays click-through.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct StageRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl StageRect {
    fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    /// Minimum distance in pixels from a point (px, py) to the rectangle boundary.
    /// Returns 0 if the point is inside.
    fn distance_to_point(&self, px: i32, py: i32) -> i32 {
        let dx = if px < self.x {
            self.x - px
        } else if px > self.x + self.w {
            px - (self.x + self.w)
        } else {
            0
        };
        let dy = if py < self.y {
            self.y - py
        } else if py > self.y + self.h {
            py - (self.y + self.h)
        } else {
            0
        };
        dx.max(dy)
    }
}

/// Hitboxes keyed by source so independent consumers (the orb, the
/// loading indicator, spatial annotations) can register/clear their own
/// interactive rects without clobbering each other. `stage_set_hitboxes`
/// (frontend-facing, legacy single-source callers) writes under the
/// "legacy" key; `set_hitbox_source` (Rust-internal, window_manager.rs /
/// calibration.rs) writes under a named key.
static HITBOXES: once_cell::sync::Lazy<parking_lot::Mutex<std::collections::HashMap<String, Vec<StageRect>>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

/// Rust-internal: replace one named source's hitbox rects. Empty = that
/// source has nothing interactive right now (does not affect other
/// sources). Used by window_manager.rs (orb) and calibration.rs (orb +
/// loading preview, both interactive for drag during a calibration
/// session).
pub fn set_hitbox_source(source: &str, rects: Vec<StageRect>) {
    let mut map = HITBOXES.lock();
    if rects.is_empty() {
        map.remove(source);
    } else {
        map.insert(source.to_string(), rects);
    }
    drop(map);
    #[cfg(target_os = "linux")]
    crate::overlay_linux::apply_input_region();
}

/// Collect all current hitbox rects across all named sources (the Linux
/// overlay turns them into the window's input region).
#[cfg(target_os = "linux")]
pub fn all_hitboxes() -> Vec<StageRect> {
    HITBOXES.lock().values().flatten().copied().collect()
}

static LAST_BEAT: once_cell::sync::Lazy<parking_lot::Mutex<Option<std::time::Instant>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

static SHOWN_AT: once_cell::sync::Lazy<parking_lot::Mutex<Option<std::time::Instant>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

/// A final transcript emitted while no live stage could receive it. Events are fire-and-forget,
/// so a turn finished during a stage rebuild vanished and left the orb parked. The recreated
/// stage pulls this on mount (`stage_take_pending_transcript`), like `get_pending_*`.
static PENDING_TRANSCRIPT: once_cell::sync::Lazy<parking_lot::Mutex<Option<(std::time::Instant, serde_json::Value)>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));
/// How long a stashed transcript stays replayable.
const PENDING_TRANSCRIPT_TTL_SECS: u64 = 20;

static VISIBLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static DISABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static FAILS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn visible() -> bool {
    VISIBLE.load(std::sync::atomic::Ordering::Relaxed)
}

fn disabled() -> bool {
    DISABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Ensure the stage window exists (created hidden). Does NOT show it.
/// The stage is a visual-only canvas: it must NEVER intercept mouse
/// clicks. `set_ignore_cursor_events(true)` is enforced at creation on
/// Windows/macOS and right after the first show on Linux (tao aborts on a
/// not-yet-realized window), and it is re-asserted on every show, so a fresh
/// WebView incarnation can never swallow the desktop (the hitbox loop
/// re-opens holes when needed).
fn ensure_stage<R: Runtime>(app: &AppHandle<R>) -> Result<tauri::WebviewWindow<R>, String> {
    let win = crate::dyn_windows::get_or_create_window(app, crate::dyn_windows::WindowConfig::stage())?;
    // Linux: `set_ignore_cursor_events(true)` ABORTS inside tao while the GTK
    // window is unrealized (`window.window()` is None until the first show —
    // event_loop.rs unwraps it). A hidden window eats no clicks anyway, so
    // the flag is applied right after the show instead. Windows and macOS
    // keep setting it at creation, as they always have.
    #[cfg(not(target_os = "linux"))]
    win.set_ignore_cursor_events(true)
        .map_err(|e| format!("stage click-through: {e}"))?;
    Ok(win)
}

/// Make the stage click-through — it must never swallow a desktop click.
#[cfg(target_os = "windows")]
fn set_click_through<R: Runtime>(win: &tauri::WebviewWindow<R>) -> Result<(), String> {
    win.set_ignore_cursor_events(true)
        .map_err(|e| format!("stage click-through: {e}"))
}

/// macOS: same as Windows — the flag is safe to set at any time.
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn set_click_through<R: Runtime>(win: &tauri::WebviewWindow<R>) -> Result<(), String> {
    win.set_ignore_cursor_events(true)
        .map_err(|e| format!("stage click-through: {e}"))
}

/// Linux: click-through is handled via GDK input shape region (`overlay_linux::apply_input_region`).
#[cfg(target_os = "linux")]
fn set_click_through<R: Runtime>(_win: &tauri::WebviewWindow<R>) -> Result<(), String> {
    crate::overlay_linux::apply_input_region();
    Ok(())
}

/// Synchronous core of `stage_show` — no actual `.await` happens in this
/// logic (window creation/show/cursor-events are all sync Tauri calls), so
/// it's split out as a plain fn callable from `lib.rs`'s setup hook, which
/// must create+show the stage BEFORE `mic_permissions::init` looks it up
/// by label — an `async_runtime::spawn`'d call wouldn't have run yet by
/// that point in the same synchronous setup closure.
pub fn stage_show_sync<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    DISABLED.store(false, std::sync::atomic::Ordering::Relaxed);
    FAILS.store(0, std::sync::atomic::Ordering::Relaxed);
    let win = ensure_stage(app)?;
    win.show().map_err(|e| format!("stage show: {e}"))?;
    // Linux: GTK ignores `maximized` on a still-hidden window, so the overlay
    // is sized from the monitor explicitly once it is mapped (no-op elsewhere).
    crate::window_manager::size_stage_to_monitor(app, &win);
    // Re-assert on every show: a recreated window resets to the default
    // (cursor-intercepting) state — the fullscreen overlay must be
    // strictly click-through before the hitbox loop re-opens holes.
    set_click_through(&win)?;
    VISIBLE.store(true, std::sync::atomic::Ordering::Relaxed);
    *SHOWN_AT.lock() = Some(std::time::Instant::now());
    tracing::info!("stage: shown (click-through enforced)");

    #[cfg(target_os = "windows")]
    {
        if let Ok(hwnd) = win.hwnd() {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{
                SetWindowPos, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_NOACTIVATE,
            };
            unsafe {
                let _ = SetWindowPos(
                    HWND(hwnd.0 as _),
                    HWND_TOPMOST,
                    0, 0, 0, 0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }

    Ok(())
}

/// IPC: show the stage. Clears kill-switch disable + fail counter — an
/// explicit show is a deliberate re-arm.
#[tauri::command]
pub async fn stage_show<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    stage_show_sync(&app)
}

/// IPC: hide the stage (destroy — frees the WebView2 tree like all windows).
#[tauri::command]
pub async fn stage_hide<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    VISIBLE.store(false, std::sync::atomic::Ordering::Relaxed);
    crate::ghost::stand_down(crate::ghost::ghost_wry::g_wry_ref(&app));
    crate::dyn_windows::destroy_window(&app, LABEL)?;
    note_window_destroyed();
    #[cfg(target_os = "linux")]
    crate::overlay_linux::note_stage_destroyed();
    tracing::info!("stage: hidden");
    Ok(())
}

/// IPC: renderer heartbeat — the stage frontend calls this every 2s.
/// A missing window or a stale beat while marked visible == blackout.
/// The optional `client` marker identifies the frontend build talking to
/// us (logged once per incarnation) — answers "is the current UI even in
/// this binary?" without devtools.
#[tauri::command]
pub fn stage_heartbeat(client: Option<String>) -> Result<(), String> {
    let mut beat = LAST_BEAT.lock();
    let first_or_stale = beat
        .map(|t| t.elapsed().as_secs() > HEARTBEAT_STALE_SECS)
        .unwrap_or(true);
    *beat = Some(std::time::Instant::now());
    drop(beat);
    if first_or_stale {
        tracing::info!(
            "stage: frontend alive (client={})",
            client.as_deref().unwrap_or("unknown")
        );
    }
    Ok(())
}

/// IPC: replace the interactive hitbox set (physical px) for ONE named
/// source (e.g. "spatial", "tour", "legacy"). Other sources'
/// hitboxes (orb, loading) are untouched — see `set_hitbox_source`.
#[tauri::command]
pub fn stage_set_hitboxes(source: Option<String>, rects: Vec<StageRect>) -> Result<(), String> {
    let src = source.unwrap_or_else(|| "legacy".to_string());
    set_hitbox_source(&src, rects);
    Ok(())
}

/// IPC: kill-switch — destroy + disable for the session. Watchdog and
/// hitbox loop go quiet. Only `stage_show` re-arms.
#[tauri::command]
pub async fn stage_hide_kill<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    DISABLED.store(true, std::sync::atomic::Ordering::Relaxed);
    VISIBLE.store(false, std::sync::atomic::Ordering::Relaxed);
    crate::ghost::stand_down(crate::ghost::ghost_wry::g_wry_ref(&app));
    crate::dyn_windows::destroy_window(&app, LABEL)?;
    note_window_destroyed();
    #[cfg(target_os = "linux")]
    crate::overlay_linux::note_stage_destroyed();
    tracing::warn!("stage: kill-switch engaged (disabled for session)");
    Ok(())
}

/// True while the stage window is up and not kill-switched.
pub fn is_shown() -> bool {
    visible() && !disabled()
}

/// True when the stage renderer has beaten recently, i.e. events emitted now will be received.
pub fn is_ready() -> bool {
    LAST_BEAT
        .lock()
        .map(|t| t.elapsed().as_secs() <= HEARTBEAT_STALE_SECS)
        .unwrap_or(false)
}

/// Wait (up to `max_ms`) for a live renderer. Used before emitting the wake so the orb events
/// are not lost to a stage that is being rebuilt. Returns whether it became ready.
pub async fn wait_until_ready(max_ms: u64) -> bool {
    let mut waited = 0u64;
    loop {
        if is_ready() {
            return true;
        }
        if waited >= max_ms {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        waited += 100;
    }
}

/// Forget the heartbeat when the window is destroyed (otherwise `is_ready` stays true for 10 s
/// after the renderer is gone).
fn note_window_destroyed() {
    *LAST_BEAT.lock() = None;
}

/// Keep a final transcript for the next stage mount when no live stage can receive the event.
pub fn stash_transcript_if_not_ready(payload: serde_json::Value) {
    if is_ready() {
        return;
    }
    tracing::warn!("stage: not ready - stashing the transcript for replay on the next mount");
    *PENDING_TRANSCRIPT.lock() = Some((std::time::Instant::now(), payload));
}

/// Pure TTL rule (unit-tested).
pub fn pending_is_fresh(age_secs: u64) -> bool {
    age_secs <= PENDING_TRANSCRIPT_TTL_SECS
}

/// IPC: the stage calls this once on mount; returns (and clears) a transcript that arrived while
/// it was down, if still fresh.
#[tauri::command]
pub fn stage_take_pending_transcript() -> Option<serde_json::Value> {
    let taken = PENDING_TRANSCRIPT.lock().take();
    match taken {
        Some((at, v)) if pending_is_fresh(at.elapsed().as_secs()) => Some(v),
        _ => None,
    }
}

/// Toggle `WDA_EXCLUDEFROMCAPTURE` on the stage window so our own screen
/// capture (screen tour / analysis) never contains the orb, spinner or old
/// annotations. Deliberately temporary: a permanent exclusion would also
/// hide NEXUS from the user's recordings and screen shares. Returns false
/// when the window/handle is unavailable. The GDI desktop-DC capture used
/// by the sidebar backdrop already relies on this affinity being honoured
/// for the (permanently excluded) sidebar window.
pub fn set_capture_excluded<R: Runtime>(app: &AppHandle<R>, excluded: bool) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowDisplayAffinity, WINDOW_DISPLAY_AFFINITY,
        };
        let Some(win) = app.get_webview_window(LABEL) else {
            return false;
        };
        let Ok(hwnd) = win.hwnd() else {
            return false;
        };
        // WDA_EXCLUDEFROMCAPTURE = 0x11, WDA_NONE = 0.
        let affinity = if excluded { 0x11 } else { 0 };
        unsafe {
            let _ = SetWindowDisplayAffinity(HWND(hwnd.0 as _), WINDOW_DISPLAY_AFFINITY(affinity));
        }
        true
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, excluded);
        false
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn cursor_pos() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut pt = POINT { x: 0, y: 0 };
    // windows 0.36 returns BOOL here, not Result.
    let ok = unsafe { GetCursorPos(&mut pt).as_bool() };
    if ok {
        Some((pt.x, pt.y))
    } else {
        None
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn cursor_pos() -> Option<(i32, i32)> {
    None
}

/// Hitbox loop: adaptive cursor poll toggling click-through. Whole stage
/// ignores the cursor unless it sits inside a registered hitbox (the
/// documented Tauri workaround for per-pixel-alpha hit-testing, which
/// neither Tauri nor Electron implements natively).
///
/// Adaptive Pacing (Intel/AMD Package C-State & Battery Optimization):
/// - Empty hitboxes + no ghost: sleeps 250ms (eliminates 94% of wakeups).
/// - Active hitboxes, cursor far (>120px): sleeps 100ms (C8/C10 residency).
/// - Cursor in proximity (<=120px) or inside: sleeps 16ms (60Hz responsiveness).
pub fn spawn_hitbox_loop<R: Runtime>(app: AppHandle<R>) {
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        // On Linux, Wayland input regions and XShape handle click-through natively
        // via `overlay_linux::apply_input_region` without cursor polling.
        return;
    }
    #[cfg(not(target_os = "linux"))]
    std::thread::Builder::new()
        .name("stage-hitbox".into())
        .spawn(move || {
            let mut last_ignore: Option<bool> = None;
            loop {
                if !visible() || disabled() {
                    last_ignore = None;
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
                let Some(win) = app.get_webview_window(LABEL) else {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                };

                let ghost_active = crate::ghost::session_active();
                let rects: Vec<StageRect> = HITBOXES.lock().values().flatten().copied().collect();
                let has_glass = crate::live_glass::has_active_hitboxes();

                // If no hitboxes are registered and no ghost session is running,
                // the transparent stage window has ZERO interactive elements.
                // Force ignore_cursor_events(true) and deep-sleep for 250ms to allow
                // the CPU package to enter deep low-power C-states (C8/C10).
                if rects.is_empty() && !ghost_active && !has_glass {
                    if last_ignore != Some(true) {
                        let _ = win.set_ignore_cursor_events(true);
                        last_ignore = Some(true);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    continue;
                }

                let pos = cursor_pos();
                if ghost_active {
                    crate::ghost::observe_cursor(crate::ghost::ghost_wry::g_wry_ref(&app), pos);
                }

                let (inside, min_dist) = match pos {
                    Some((x, y)) => {
                        let mut in_rect = false;
                        let mut min_d = i32::MAX;
                        for r in &rects {
                            if r.contains(x, y) {
                                in_rect = true;
                                min_d = 0;
                                break;
                            }
                            let d = r.distance_to_point(x, y);
                            if d < min_d {
                                min_d = d;
                            }
                        }
                        let inside = in_rect || crate::live_glass::is_cursor_inside_hitbox(x, y);
                        (inside, if inside { 0 } else { min_d })
                    }
                    None => (false, i32::MAX),
                };

                let want_ignore = !inside;
                if last_ignore != Some(want_ignore) && win.set_ignore_cursor_events(want_ignore).is_ok() {
                    last_ignore = Some(want_ignore);
                }

                // Adaptive Sleep Interval:
                // - Proximity (inside or <= 120px): 16ms (60Hz seamless entry)
                // - Ghost active: 30ms (smooth cursor follower)
                // - Distant: 100ms (enables CPU deep sleep)
                let sleep_ms = if inside || min_dist <= 120 {
                    16
                } else if ghost_active {
                    30
                } else {
                    100
                };

                std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
            }
        })
        .ok();
}

/// Speak one line through the orb (separate window — unaffected by a
/// stage blackout). Skipped when a real user turn is active: the
/// watchdog must never talk over the user.
///
/// Uses a dedicated `stage:notice` channel — deliberately NOT
/// `orchestrator:event` Result: a Result would overwrite the orb's
/// in-flight request id and clear the long-running flag, orphaning a
/// real turn's `done` handshake (the stuck-orb class) if a wake lands
/// mid-announcement. The notice handler speaks + resets locally with
/// zero handshake contact.
fn inform_via_orb<R: Runtime>(app: &AppHandle<R>, text: &str) {
    if crate::orchestrator::has_active_request() {
        tracing::info!("stage: watchdog silent (user turn active): {text}");
        return;
    }
    let payload = serde_json::json!({ "text": text });
    let _ = app.emit("stage:notice", payload);
}

/// Blackout watchdog: 2s cadence. Detect → shutdown → inform → auto-fix
/// (see module docs). Gives up recreating after MAX_AUTO_FIXES but keeps
/// watching so a manual `stage_show` still recovers.
pub fn spawn_stage_watchdog<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if !visible() || disabled() {
                continue;
            }
            // Cold-boot grace: the renderer needs seconds to mount + beat.
            let fresh_show = SHOWN_AT
                .lock()
                .map(|t| t.elapsed().as_secs() < SHOW_GRACE_SECS)
                .unwrap_or(false);
            if fresh_show {
                continue;
            }
            let window_gone = app.get_webview_window(LABEL).is_none();
            let stale = LAST_BEAT
                .lock()
                .map(|t| t.elapsed().as_secs() > HEARTBEAT_STALE_SECS)
                .unwrap_or(true);
            if !window_gone && !stale {
                continue;
            }

            // ── DETECTED: blackout ──
            let fails = FAILS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            tracing::warn!(
                "stage: blackout detected (window_gone={window_gone}, stale_beat={stale}) — incident #{fails}"
            );

            // ── SHUTDOWN: a black fullscreen surface is worse than none ──
            VISIBLE.store(false, std::sync::atomic::Ordering::Relaxed);
            crate::ghost::note_blackout();
            let _ = crate::dyn_windows::destroy_window(&app, LABEL);
            note_window_destroyed();
            #[cfg(target_os = "linux")]
            crate::overlay_linux::note_stage_destroyed();

            if fails > MAX_AUTO_FIXES {
                // ── GIVE UP: stay hidden, say so once, keep watching ──
                // A Ghost session cannot outlive its overlay: with no stage
                // the session UI is gone, yet its global Esc shortcut would
                // keep swallowing Esc system-wide. End it, and say so.
                let was_ghost = crate::ghost::session_active();
                crate::ghost::stand_down(crate::ghost::ghost_wry::g_wry_ref(&app));
                inform_via_orb(
                    &app,
                    if was_ghost {
                        "Display overlay kept failing, sir. I switched visuals off until restart and ended Ghost mode — voice still works."
                    } else {
                        "Display overlay kept failing, sir. I switched visuals off until restart — voice still works."
                    },
                );
                tracing::error!("stage: auto-fix budget exhausted — staying hidden");
                continue;
            }

            // ── INFORM ──
            inform_via_orb(
                &app,
                "Display overlay hit a problem, sir. I hid it and I'm rebuilding it in the background.",
            );

            // ── AUTO-FIX: backoff, recreate, verify heartbeat ──
            let backoff = std::cmp::min(1u64 << fails.min(5), 30);
            tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
            if disabled() {
                // Kill-switch engaged mid-fix: stand down.
                continue;
            }
            match ensure_stage(&app) {
                Ok(win) => {
                    if win.show().is_err() {
                        continue;
                    }
                    // Same as stage_show_sync: the overlay must cover the
                    // monitor (Linux sizes it explicitly — GTK ignores
                    // `maximized` on a hidden window) and must be
                    // click-through (a recreated window starts out
                    // intercepting the cursor).
                    crate::window_manager::size_stage_to_monitor(&app, &win);
                    let _ = set_click_through(&win);
                    VISIBLE.store(true, std::sync::atomic::Ordering::Relaxed);
                    *SHOWN_AT.lock() = Some(std::time::Instant::now());
                    // Wait up to 30s for a fresh heartbeat (proves the
                    // renderer is alive, not just the window).
                    let mut healthy = false;
                    for _ in 0..60 {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        let fresh = LAST_BEAT
                            .lock()
                            .map(|t| t.elapsed().as_secs() <= HEARTBEAT_STALE_SECS)
                            .unwrap_or(false);
                        if fresh {
                            healthy = true;
                            break;
                        }
                    }
                    if healthy {
                        if disabled() {
                            // Kill-switch landed during verification: stay down.
                            VISIBLE.store(false, std::sync::atomic::Ordering::Relaxed);
                            tracing::info!("stage: healthy beat but kill-switched — staying down");
                        } else {
                            FAILS.store(0, std::sync::atomic::Ordering::Relaxed);
                            tracing::info!("stage: auto-fix verified healthy (heartbeat resumed)");
                        }
                    } else {
                        tracing::warn!("stage: recreated window never beat — will retry");
                    }
                }
                Err(e) => {
                    tracing::warn!("stage: auto-fix recreate failed: {e}");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hitbox_contains_edges() {
        let r = StageRect { x: 100, y: 100, w: 200, h: 100 };
        assert!(r.contains(100, 100));
        assert!(r.contains(299, 199));
        assert!(!r.contains(300, 100));
        assert!(!r.contains(99, 100));
        assert!(!r.contains(100, 200));
    }

    #[test]
    fn pending_transcript_expires() {
        assert!(pending_is_fresh(0));
        assert!(pending_is_fresh(20));
        assert!(!pending_is_fresh(21));
    }

    // One test: these three share the process-global heartbeat/stash, so they must not race.
    #[test]
    fn readiness_and_stash_follow_the_heartbeat() {
        note_window_destroyed();
        assert!(!is_ready(), "no beat => not ready");
        stage_heartbeat(Some("test".into())).unwrap();
        assert!(is_ready());
        note_window_destroyed();
        assert!(!is_ready(), "a destroyed window must stop counting as ready at once");

        let _ = stage_take_pending_transcript();
        stash_transcript_if_not_ready(serde_json::json!({"text": "open notepad"}));
        let got = stage_take_pending_transcript().expect("stashed while down");
        assert_eq!(got["text"], "open notepad");
        assert!(stage_take_pending_transcript().is_none(), "take clears it");

        stage_heartbeat(None).unwrap();
        stash_transcript_if_not_ready(serde_json::json!({"text": "ignored"}));
        assert!(stage_take_pending_transcript().is_none(), "no stash while the stage is alive");
        note_window_destroyed();
    }

    #[test]
    fn test_hitbox_empty_never_inside() {
        let boxes: Vec<StageRect> = vec![];
        assert!(!boxes.iter().any(|r| r.contains(960, 540)));
    }
}
