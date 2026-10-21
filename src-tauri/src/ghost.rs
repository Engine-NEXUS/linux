//! Ghost Mode — the AI drives the user's REAL cursor (Windows-only).
//!
//! What ships here:
//! - Session state machine (Idle → Active → Yielded), explicit entry/exit.
//! - NO takeover detection (2026-09-27, user directive): the user's mouse
//!   is ALWAYS free — motion is never judged, tasks are never aborted by
//!   cursor movement, the session never yields. Explicit exits only:
//!   Esc (the cancel button, armed at start), "exit ghost mode" voice,
//!   stage hide/kill.
//! - Esc panic button: dynamically registered on entry, unregistered on
//!   exit (never steals Escape globally — static registration would eat
//!   the user's Esc in every app).
//! - Ring position events for the stage overlay: rides ONLY commanded
//!   glides (truthful "AI is driving" indicator), off when idle.
//!
//! Deliberately NOT here: takeover detection (deleted after two live
//! misfires), keyboard-takeover hook (WH_KEYBOARD_LL — moot without a
//! takeover concept).

use tauri::{AppHandle, Emitter, Manager, Runtime};
#[cfg(not(target_os = "linux"))]
use tauri_plugin_global_shortcut::GlobalShortcutExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Session {
    Idle,
    Active,
    Yielded,
}

static SESSION: once_cell::sync::Lazy<parking_lot::Mutex<Session>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(Session::Idle));

/// Last commanded target + quiet-until instant (easing settle). While `now`
/// is before `quiet_until`, deltas are ours and ignored.
type ExpectedTarget = Option<((i32, i32), std::time::Instant)>;

static EXPECTED: once_cell::sync::Lazy<parking_lot::Mutex<ExpectedTarget>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

static LAST_SEEN: once_cell::sync::Lazy<parking_lot::Mutex<Option<(i32, i32)>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

static LAST_RING_SENT: once_cell::sync::Lazy<parking_lot::Mutex<Option<(i32, i32)>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

/// Stop flag for in-flight ghost task runs. Set by voice "stop"
/// (`live_cancel`), Esc panic, or takeover; every drill step checks it
/// before acting. Cleared on session entry.
static GHOST_CANCEL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Drill-run depth: >0 while a ghost drill's main steps execute.
/// Distinguishes "task running" (queue + intercept follow-ups) from a
/// bare test session (no queueing — nothing will drain it).
static GHOST_BUSY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Follow-up commands captured mid-drill, in arrival order (FIFO).
/// Entries carry their slot class so same-slot pendings can supersede
/// (Rule 2) instead of double-executing.
static FOLLOWUP_QUEUE: once_cell::sync::Lazy<parking_lot::Mutex<std::collections::VecDeque<QueuedCmd>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(std::collections::VecDeque::new()));

/// Max queued follow-ups (drop-newest beyond this) and max drained per
/// drill (bounds nested-drill ping-pong).
pub const FOLLOWUP_CAP: usize = 5;
pub const DRAIN_CAP: usize = 10;

/// Singleton resource class of a queued command. Only AppOpen and Navigate
/// pendings supersede (D8): a newer "open Chrome" replaces a pending
/// "open Brave"; repeats of tab/type/general commands APPEND (two
/// "new tab"s mean two tabs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotClass {
    AppOpen,
    Navigate,
    BrowserAction,
    Dictation,
    GeneralAction,
}

/// One queued ghost command: arrival-ordered FIFO entry.
#[derive(Debug, Clone)]
pub struct QueuedCmd {
    pub id: u64,
    pub slot: SlotClass,
    pub transcript: String,
    pub queued_at: std::time::Instant,
}

/// Pure slot classifier (unit-tested). Matches the deterministic parser's
/// verb families; conservative — unknown phrasings fall to GeneralAction
/// (append, never supersede).
pub fn classify_slot(text: &str) -> SlotClass {
    let t = text.trim().to_lowercase();
    // Dictation first: "start dictation" carries the AppOpen prefix
    // "start " but is a mode command, not an app launch.
    if t.contains("dictat") {
        SlotClass::Dictation
    } else if t.starts_with("open ")
        || t.starts_with("launch ")
        || t.starts_with("start ")
        || t.starts_with("close ")
    {
        SlotClass::AppOpen
    } else if t.contains("search ")
        || t.contains("go to ")
        || t.contains("navigate ")
        || t.contains("open youtube")
        || t.starts_with("find ")
    {
        SlotClass::Navigate
    } else if t.contains("tab")
        || t.contains("reload")
        || t.contains("refresh the page")
        || t.contains("go back")
        || t.contains("go forward")
    {
        SlotClass::BrowserAction
    } else if t.starts_with("type ") {
        SlotClass::Dictation
    } else {
        SlotClass::GeneralAction
    }
}

/// Monotonic id for queued commands (debugging + ordering proof).
static CMD_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Depth-ACK spoken flag: "Queued, sir." fires once per session when depth
/// reaches 2+ (Rule 4). Reset on every session entry.
pub static DEPTH_ACK_SPOKEN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Reset per-session queue signals (called from ghost_enter).
pub fn reset_depth_ack() {
    DEPTH_ACK_SPOKEN.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// Raw stop phrases (normalized exact match). Independent of parsing:
/// bare "cancel" parses as Greeting upstream, so the intercept must not
/// depend on the intent label alone. The parsed `cancel_action`
/// NluResult is matched too (belt and suspenders).
const STOP_PHRASES: &[&str] = &[
    "stop",
    "stop it",
    "stop please",
    "stand down",
    "cancel",
    "halt",
    "abort",
    "never mind",
    "forget it",
    "hold on",
    "exit ghost",
    "stop ghost",
    "exit ghost mode",
    "leave ghost mode",
    "exit goes to mode",
];

/// Words STT/users put around a stop ("Nexus, stop.", "okay stop please").
const STOP_FILLERS: &[&str] = &["nexus", "hey", "okay", "ok", "please", "now"];

/// Lowercase, drop punctuation (STT ends utterances with "." / "!" / "?"), collapse
/// whitespace and peel filler words off both ends. Pure.
fn normalize_stop_text(text: &str) -> String {
    let cleaned: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' })
        .collect();
    let mut words: Vec<&str> = cleaned.split_whitespace().collect();
    while words.first().map_or(false, |w| STOP_FILLERS.contains(w)) {
        words.remove(0);
    }
    while words.last().map_or(false, |w| STOP_FILLERS.contains(w)) {
        words.pop();
    }
    words.join(" ")
}

/// Pure stop-phrase check (unit-tested). Punctuation- and filler-tolerant: "Stop." and
/// "Nexus, hold on!" count; "stop the music" and "cancel my subscription" do not.
pub fn is_stop_phrase(text: &str) -> bool {
    let t = normalize_stop_text(text);
    !t.is_empty() && STOP_PHRASES.iter().any(|s| *s == t)
}

/// Request cancellation of the running ghost task (voice stop path).
/// The runner observes it at the next step boundary and aborts cleanly.
pub fn request_stop() {
    GHOST_CANCEL.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// True if cancellation was requested (drill step guard).
pub fn stop_requested() -> bool {
    GHOST_CANCEL.load(std::sync::atomic::Ordering::Relaxed)
}

fn clear_stop() {
    GHOST_CANCEL.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// Drop a stop flag left behind with no drill to consume it (a `live_cancel`
/// while idle). Called when a new top-level command starts; a stop raised
/// DURING a drill is left alone (the drill is still observing it).
pub fn clear_stale_stop() {
    if !drill_running() {
        clear_stop();
    }
}

/// True while a ghost drill's main steps execute (queue + intercept
/// follow-ups). Bare test sessions don't set this — nothing drains them.
pub fn drill_running() -> bool {
    GHOST_BUSY.load(std::sync::atomic::Ordering::Relaxed) > 0
}

/// IPC: is a drill (an action NEXUS is carrying out on screen) running right now? The frontend's
/// stall watchdog must NOT re-arm the mic over it: that re-arm used to cancel a WhatsApp drill.
#[tauri::command]
pub fn ghost_drill_running() -> bool {
    drill_running()
}

/// Mark drill start (called by runners after session entry).
pub fn drill_begin() {
    GHOST_BUSY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Mark drill end (called by runners before session exit / drain).
pub fn drill_end() {
    let prev = GHOST_BUSY.fetch_update(
        std::sync::atomic::Ordering::Relaxed,
        std::sync::atomic::Ordering::Relaxed,
        |v| Some(v.saturating_sub(1)),
    );
    // The last in-flight drill has ended, so every step that could observe the
    // stop has run: consume it. Before this, a voice "stop" stayed set until
    // the next `ghost_enter`, and every later runner answered "Stopped, sir."
    // while the session still said it was on (found in the 2026-10-12 audit).
    if matches!(prev, Ok(v) if v <= 1) {
        clear_stop();
    }
}

/// RAII drill guard: brackets an unbracketed ghost action so
/// `drill_running()` is true while it executes (mid-action speech queues
/// instead of routing a rival turn). Live drills (whatsapp_drill, mouse)
/// bracket internally — use this ONLY for orchestrator-level runners that
/// don't (run_ghost_open, run_browser_search, run_browser_search_focus).
/// Balanced nesting is safe (counter-based), but double-bracketing is
/// still avoided by construction.
pub struct GhostDrillGuard;

impl GhostDrillGuard {
    pub fn new() -> Self {
        drill_begin();
        Self
    }
}

impl Drop for GhostDrillGuard {
    fn drop(&mut self) {
        drill_end();
    }
}

impl Default for GhostDrillGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// Queue a follow-up command captured mid-drill. Rule 2: a pending item in
/// the same superseding slot (AppOpen/Navigate) is REPLACED in place.
/// Rule 4: at cap, the NEWEST is rejected (the first-spoken intent is the
/// most valuable). Returns (kept, depth_after).
pub fn enqueue_command(transcript: String) -> (bool, usize) {
    let mut q = FOLLOWUP_QUEUE.lock();
    let slot = classify_slot(&transcript);

    // Rule 2: supersede pending item in the same singleton slot.
    if slot == SlotClass::AppOpen || slot == SlotClass::Navigate {
        for existing in q.iter_mut() {
            if existing.slot == slot {
                tracing::info!(
                    "ghost-queue: superseding pending {:?} ('{}' -> '{}')",
                    slot,
                    existing.transcript,
                    transcript
                );
                existing.transcript = transcript;
                existing.queued_at = std::time::Instant::now();
                return (true, q.len());
            }
        }
    }

    // Rule 4: drop-newest at cap.
    if q.len() >= FOLLOWUP_CAP {
        tracing::warn!(
            "ghost-queue: cap reached ({}), rejecting newest: '{}'",
            FOLLOWUP_CAP,
            transcript
        );
        return (false, q.len());
    }

    let id = CMD_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    q.push_back(QueuedCmd {
        id,
        slot,
        transcript,
        queued_at: std::time::Instant::now(),
    });
    tracing::debug!("ghost-queue: enqueued cmd #{id} (depth {})", q.len());
    (true, q.len())
}

/// Dequeue the head of the FIFO (None when empty).
pub fn dequeue_command() -> Option<QueuedCmd> {
    FOLLOWUP_QUEUE.lock().pop_front()
}

/// Rule 3: re-verify grounding at dequeue time, never execute blind.
/// Browser-targeted steps require a browser to actually hold the
/// foreground (a URL read proves it); anything else re-verifies inside
/// its own runner (focus-verify before+after). Err = skip with one
/// spoken line, queue continues.
pub fn verify_grounding(cmd: &QueuedCmd) -> Result<(), String> {
    match cmd.slot {
        SlotClass::BrowserAction | SlotClass::Navigate => {
            if crate::browser_url::get_active_browser_url().is_none() {
                return Err("Target browser window lost focus".into());
            }
        }
        _ => {}
    }
    Ok(())
}

/// Drop the queue without running it (abort / main-step failure — the
/// context they were spoken into is gone).
pub fn drop_followups() -> usize {
    let n = FOLLOWUP_QUEUE.lock().len();
    FOLLOWUP_QUEUE.lock().clear();
    if n > 0 {
        tracing::info!("ghost: dropped {n} queued follow-up(s) after abort/failure");
    }
    n
}

/// True while a ghost session is driving (ring visible, Esc armed).
pub fn session_active() -> bool {
    *SESSION.lock() == Session::Active
}

/// Speak one line through the orb without touching any handshake
/// (same rails as abort notices). Used for session entry/exit narration.
/// Wry-typed (Send reason same as abort_session).
pub fn announce(app: &tauri::AppHandle<tauri::Wry>, text: &str) {
    // A lost notice = a missed spoken alert with no other signal.
    crate::commands::emit_logged(
        app,
        "stage:notice",
        serde_json::json!({ "text": text }),
    );
}

/// No takeover detection (2026-09-27, user directive — supersedes the
/// 2026-09-25 re-scope). The user's mouse is ALWAYS free during a ghost
/// session: no motion is ever judged, no task is ever aborted by cursor
/// movement, and the session never yields to a "human hand". The only
/// session exits are the explicit ones:
///   - Esc (the cancel button, armed at session start)
///   - "exit/close/turn off ghost mode" (ExitGhostControl intent)
///   - stage hide/kill (stand_down — the visual leash is gone)
///
/// The old detector (between-steps arm, then the commanded-target
/// re-scope) misfired on live sessions twice; it is deleted, not tuned.
/// `note_expected`/`note_idle` remain: they ONLY drive the ring's
/// ride-along during commanded glides.
fn is_active() -> bool {
    session_active()
}

/// Record a commanded target (Phase 2 motion calls this; tests use it).
/// `settle_ms` extends the suppress window past motion end (easing tail).
#[allow(dead_code)]
pub fn note_expected(target: (i32, i32), settle_ms: u64) {
    *EXPECTED.lock() = Some((
        target,
        std::time::Instant::now() + std::time::Duration::from_millis(settle_ms),
    ));
}

/// Clear the commanded target (motion finished, back to between-steps).
#[allow(dead_code)]
pub fn note_idle() {
    *EXPECTED.lock() = None;
}

/// Per-tick hook, called by the stage hitbox loop with the polled cursor
/// position. Drives ONLY ring position events now (no takeover judgment).
/// Cheap: a few comparisons and (at most) one emit per changed pixel.
/// Wry-typed (via the module's Wry alias) for the same Send reason as
/// abort_session: the hitbox-loop thread calls this; a generic future
/// would poison it.
pub fn observe_cursor(app: &tauri::AppHandle<tauri::Wry>, pos: Option<(i32, i32)>) {
    let Some(actual) = pos else { return };
    if !is_active() {
        return;
    }
    let now = std::time::Instant::now();
    // Copy out under the lock, drop it immediately — holding the
    // parking_lot guard (not Send) across the emit tail made
    // the drill futures non-Send (live-bug chain).
    let expected = {
        let guard = EXPECTED.lock();
        match *guard {
            Some((t, until)) if now < until => Some(t),
            Some((t, _)) => Some(t),
            None => None,
        }
    };
    *LAST_SEEN.lock() = Some(actual);

    // Ring rides ONLY while the AI commands motion (expected Some).
    // Idle-in-session: the user's cursor is theirs — no ring, no lie.
    if expected.is_none() {
        ring_off(app);
        return;
    }
    // Ring follows the commanded path; emit only on change (spam cap).
    let changed = {
        let mut sent = LAST_RING_SENT.lock();
        let changed = *sent != Some(actual);
        if changed {
            *sent = Some(actual);
        }
        changed
    };
    if changed {
        let _ = app.emit(
            "ghost:ring",
            serde_json::json!({ "x": actual.0, "y": actual.1, "visible": true }),
        );
    }
}

/// Ring off (AI not driving / task aborted). Ring-only event — the orb's
/// waves are driven by `ghost:session`, never by this.
fn ring_off(app: &tauri::AppHandle<tauri::Wry>) {
    let mut sent = LAST_RING_SENT.lock();
    if sent.is_some() {
        *sent = None;
        let _ = app.emit(
            "ghost:ring",
            serde_json::json!({ "x": 0, "y": 0, "visible": false }),
        );
    }
}

/// Clicky-style target guidance pointer: highlights an element on screen with a
/// floating visual pointer, speech bubble, and duration.
#[allow(dead_code)]
pub fn point_at_target<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    x: i32,
    y: i32,
    label: &str,
    duration_ms: u64,
) {
    let _ = app.emit(
        "ghost:point",
        serde_json::json!({
            "x": x,
            "y": y,
            "label": label,
            "duration_ms": duration_ms,
            "visible": true
        }),
    );
}

/// Hide the target guidance pointer immediately.
#[allow(dead_code)]
pub fn point_off<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let _ = app.emit(
        "ghost:point",
        serde_json::json!({
            "x": 0,
            "y": 0,
            "label": "",
            "duration_ms": 0,
            "visible": false
        }),
    );
}

/// Session event to the orb (waves + ghostActive flag). Session-scoped —
/// fires on enter/exit/stand-down only, never per turn.
/// Routed through Main Center `direct_ui(Session(..))` — call that, not this.
pub(crate) fn emit_session(app: &tauri::AppHandle<tauri::Wry>, active: bool) {
    crate::commands::emit_logged(app, "ghost:session", serde_json::json!({ "active": active }));
}

/// End the session (explicit exits only: Esc panic, "exit ghost mode",
/// stage hide/kill). Ring off + Esc release + waves off.
/// Runtime-erased on purpose: the Esc handler must be `Send + Sync +
/// 'static`, and a generic `<R>` future would drag the concrete runtime
/// type (and its non-Send parking_lot guard lifetimes) across threads.
fn abort_session(app: &tauri::AppHandle<tauri::Wry>, reason: &str) {
    {
        let mut s = SESSION.lock();
        if *s == Session::Idle {
            return;
        }
        *s = Session::Yielded;
    }
    *EXPECTED.lock() = None;
    unregister_esc(app);
    ring_off(app);
    crate::hand::stop_if_ghost_started(app);
    crate::center::direct_ui(app, crate::center::UiDirective::Session(false));
    // Every abort path (Esc panic, takeover, API/Ctrl+Space) speaks the
    // same exit line — an exit with no feedback reads as "Esc did nothing".
    // Deliberate voice exits (`ghost_exit`) stay silent here by design; the
    // orchestrator speaks for those. Bailing cancels the in-flight turn.
    let (request_id, _) =
        crate::orchestrator::install_new_request(crate::orchestrator::Subsystem::LocalCommand);
    crate::orchestrator::speak_line(app, crate::orchestrator::GHOST_EXIT_LINE.to_string(), &request_id);
    crate::orchestrator::clear_active_request(&request_id);
    tracing::warn!("ghost: session ended ({reason})");
}

#[cfg(not(target_os = "linux"))]
fn register_esc(app: &tauri::AppHandle<tauri::Wry>) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{Shortcut, ShortcutState};
    let sc: Shortcut = "Escape"
        .parse()
        .map_err(|e| format!("ghost: bad Esc shortcut: {e}"))?;
    let handle = app.clone();
    // Closure must be Send: the parking_lot-guard helpers (abort_session)
    // are fine to *call* (guard lives and dies inside), but the closure
    // itself only captures an AppHandle clone — Send by construction.
    app.global_shortcut()
        .on_shortcut(sc, move |_app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                tracing::warn!("ghost: Esc panic - aborting session");
                abort_session(&handle, "esc-panic");
            }
        })
        .map_err(|e| format!("ghost: Esc register failed: {e}"))
}

fn unregister_esc(app: &tauri::AppHandle<tauri::Wry>) {
    #[cfg(not(target_os = "linux"))]
    if let Err(e) = app.global_shortcut().unregister("Escape") {
        tracing::debug!("ghost: Esc unregister: {e}");
    }
    #[cfg(target_os = "linux")]
    let _ = app;
}

/// Vision-key presence for diagnostics and the UIA-miss gate.
///
/// Keychain-first with settings.json fallback — the SAME read path as
/// `commands::read_groq_api_key` / `commands::read_api_key` (and the
/// vision gate in `live::commands::mouse`). A returning user whose key
/// lives in settings.json but never reached the OS keychain (boot
/// migration silent-fail, keychain write denied, manual settings edit)
/// counts as key-present. When a key is found only via the settings.json
/// fallback, heal forward by writing it to the keychain now
/// (best-effort — keychain failures only warn inside `set_api_key`).
///
/// NOTE: entry never acts on this (no popup, no speech — settings opens
/// only on explicit user command). The result feeds a debug log at entry
/// and is available for a future Ghost settings tab.
pub fn vision_keys_present<R: Runtime>(app: &AppHandle<R>) -> (bool, bool) {
    let groq = crate::commands::read_groq_api_key(app);
    let gemini = crate::commands::read_api_key(app, "gemini");
    if !groq.is_empty() && crate::auth_vault::get_api_key("groq").is_none() {
        crate::auth_vault::set_api_key("groq", &groq);
    }
    if !gemini.is_empty() && crate::auth_vault::get_api_key("gemini").is_none() {
        crate::auth_vault::set_api_key("gemini", &gemini);
    }
    (!groq.is_empty(), !gemini.is_empty())
}

/// Pure nudge decision: true only when NEITHER vision key exists
/// anywhere. Split out so the condition is unit-testable without an
/// AppHandle (the keychain/settings reads live in `vision_keys_present`).
/// Entry only logs on true (never popup/speech); the click-time vision
/// gate in mouse.rs gives the verbal guidance instead.
pub fn needs_vision_key_nudge(groq_present: bool, gemini_present: bool) -> bool {
    !groq_present && !gemini_present
}

/// IPC: Query whether a ghost session is currently active (mount-time sync).
#[tauri::command]
pub fn get_pending_ghost_session() -> bool {
    session_active()
}

/// IPC: enter a ghost session (test/drill entry in Phase 0).
/// Ensures the stage is visible (the ring lives on it), arms Esc panic.
/// Emits the SESSION event (orb waves) — the ring itself stays OFF until
/// itself stays OFF until the AI commands motion ( truthful: ring =
/// "AI is driving").
#[tauri::command]
pub async fn ghost_enter(app: AppHandle<tauri::Wry>) -> Result<(), String> {
    crate::stage::stage_show(app.clone()).await?;
    // Always unregister first: a previous session that ended via abort_session
    // (Esc panic) sets SESSION=Yielded and may leave a stale Esc shortcut
    // registered. Calling register_esc on top of an existing registration
    // silently fails on some platforms or stacks duplicate handlers.
    // Unregistering first gives a clean slate every time.
    //
    // The cancel button is armed BEFORE the session becomes Active: a session
    // that cannot be cancelled with Esc must never exist (it used to be marked
    // Active first, so a failed registration left a live session behind an
    // error the caller dropped).
    #[cfg(not(target_os = "linux"))]
    {
        unregister_esc(&app);
        register_esc(&app)?;
    }
    *SESSION.lock() = Session::Active;
    *EXPECTED.lock() = None;
    *LAST_SEEN.lock() = None;
    *LAST_RING_SENT.lock() = None;
    clear_stop();
    // Fresh session: the queue depth-ACK ("Queued, sir.") may fire again.
    reset_depth_ack();
    // The owner just asked for Ghost mode: their next commands are the conversation.
    crate::directed::set_engagement_secs(crate::directed::read_engagement_secs(&app));
    crate::directed::engage_now();
    tracing::warn!("ghost: session ACTIVE — use the mouse freely; Esc cancels the session");
    crate::center::direct_ui(&app, crate::center::UiDirective::Session(true));
    if let Ok(dir) = app.path().app_data_dir() {
        crate::diary::log_event(&dir, "ghost_enter", "session start");
    }
    // Pure init: no settings popup, no extra speech. The single entry
    // line ("Ghost mode initialized, sir. Tell me what to do.") is spoken
    // by the orchestrator. Settings opens ONLY on explicit user command
    // (voice "open settings", tray, Ctrl+Shift+S, --settings, deep link).
    // Key status is logged for diagnostics (and self-healed into the
    // keychain when found via settings.json); a genuinely keyless user is
    // guided verbally at the click-time vision gate in mouse.rs instead.
    let (groq_present, gemini_present) = vision_keys_present(&app);
    if needs_vision_key_nudge(groq_present, gemini_present) {
        tracing::debug!(
            "ghost: no vision keys anywhere — UIA clicks only until keys are added in Settings → Accounts"
        );
    }
    Ok(())
}

/// Reset the observed position (stage blackout: the poll went blind, so
/// the next sighting must observe, never judge — avoids a false takeover
/// from motion that happened during the outage).
pub fn note_blackout() {
    *LAST_SEEN.lock() = None;
    *LAST_RING_SENT.lock() = None;
}

/// Stand down silently (stage hidden/killed: the visual leash is gone,
/// so no session may survive). No notice — the hide itself was deliberate.
pub fn stand_down(app: &tauri::AppHandle<tauri::Wry>) {
    {
        let mut s = SESSION.lock();
        if *s == Session::Idle {
            return;
        }
        *s = Session::Idle;
    }
    *EXPECTED.lock() = None;
    unregister_esc(app);
    ring_off(app);
    crate::center::direct_ui(app, crate::center::UiDirective::Session(false));
    tracing::info!("ghost: stood down (stage hidden)");
}

/// IPC: leave the session cleanly (no abort message — deliberate exit).
#[tauri::command]
pub async fn ghost_exit(app: AppHandle<tauri::Wry>) -> Result<(), String> {
    *SESSION.lock() = Session::Idle;
    *EXPECTED.lock() = None;
    unregister_esc(&app);
    ring_off(&app);
    crate::center::direct_ui(&app, crate::center::UiDirective::Session(false));
    tracing::info!("ghost: session ended by request");
    if let Ok(dir) = app.path().app_data_dir() {
        crate::diary::log_event(&dir, "ghost_exit", "session end");
    }
    Ok(())
}

/// IPC: abort from UI/test tooling (same path as Esc/takeover).
#[tauri::command]
pub async fn ghost_abort<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    // NEXUS only ever builds with the Wry runtime; abort_session is
    // Wry-typed so the Esc handler stays Send (the live-bug chain).
    // Erase the runtime marker at this one boundary via the Wry shim
    // defined below (same layout, same process, single runtime always).
    ghost_wry::ghost_abort_wry(ghost_wry::g_wry(app))
}

/// Wry-runtime shim: NEXUS is single-runtime (Tauri default), so the
/// generic ghost_abort wrapper forwards here to call the Wry-typed
/// abort_session without infecting every caller with Send bounds.
pub mod ghost_wry {
    use super::abort_session;
    use tauri::AppHandle;

    pub fn ghost_abort_wry(app: AppHandle<tauri::Wry>) -> Result<(), String> {
        abort_session(&app, "api-abort");
        Ok(())
    }

    /// Same-layout runtime erasure. AppHandle<R> is
    /// {R::Handle, Arc<AppManager<R>>, Arc<Mutex<EventLoop>>}; the Arc
    /// and Mutex layers are runtime-independent. NEXUS builds only with
    /// Wry, so R == Wry always — do this by pointer, not by transmute
    /// (R::Handle is dependently-sized; a direct transmute is rejected).
    /// Lives in ONE place (here), not scattered through every caller.
    pub fn g_wry<R: tauri::Runtime>(app: AppHandle<R>) -> AppHandle<tauri::Wry> {
        unsafe {
            // Same memory, reinterpreted under the Wry marker — field
            // offsets are identical (Handle is a unit marker struct in
            // every runtime used).
            let raw = &app as *const AppHandle<R> as *const AppHandle<tauri::Wry>;
            std::ptr::read(raw)
        }
    }

    /// Reference variant of `g_wry` (same-layout borrows).
    pub fn g_wry_ref<R: tauri::Runtime>(app: &AppHandle<R>) -> &AppHandle<tauri::Wry> {
        unsafe { &*(app as *const AppHandle<R> as *const AppHandle<tauri::Wry>) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_enter_abort_exit_cycle() {
        *SESSION.lock() = Session::Idle;
        *SESSION.lock() = Session::Active;
        assert!(is_active());
        *SESSION.lock() = Session::Yielded;
        assert!(!is_active());
        *SESSION.lock() = Session::Idle;
    }

    /// All three exit paths (voice, Esc, Ctrl+Space/API) speak the identical
    /// line — abort_session and the voice exit share GHOST_EXIT_LINE.
    #[test]
    fn test_ghost_exit_line_single_source() {
        assert_eq!(
            crate::orchestrator::GHOST_EXIT_LINE,
            "Ghost mode off, sir."
        );
    }

    /// Double-exit safety: ending twice must settle Idle without panic.
    /// (abort_session early-returns on Idle; ghost_exit unconditionally
    /// stores Idle — both idempotent by construction.)
    #[test]
    fn test_double_exit_settles_idle() {
        *SESSION.lock() = Session::Active;
        *SESSION.lock() = Session::Yielded;
        *SESSION.lock() = Session::Idle;
        assert!(!is_active());
        *SESSION.lock() = Session::Idle;
        assert!(!is_active());
    }

    #[test]
    fn test_stop_phrases_match_normalized() {
        assert!(is_stop_phrase("stop"));
        assert!(is_stop_phrase("  Stop  "));
        assert!(is_stop_phrase("NEVER MIND"));
        assert!(is_stop_phrase("forget it"));
        assert!(!is_stop_phrase("stop the music"));
        assert!(!is_stop_phrase("unstoppable"));
        assert!(!is_stop_phrase(""));
    }

    // STT (Groq) ends every utterance with punctuation: "Stop." / "Hold on!".
    // The exact-match check used to miss all of them, so mid-drill stops only
    // worked when the parser happened to also produce a cancel_action.
    #[test]
    fn test_stop_phrases_survive_stt_punctuation_and_fillers() {
        for t in [
            "Stop.", "stop!", "Hold on.", "Never mind.", "Halt!", "Abort.", "Cancel?",
            "Nexus, stop.", "okay stop", "Please stop.", "stop please.", "Nexus stop it",
            "hey nexus, hold on",
        ] {
            assert!(is_stop_phrase(t), "{t:?} must count as a stop");
        }
        for t in [
            "stop the music", "stop playing", "hold on to that", "cancel my subscription",
            "stop typing", "open chrome", "nexus", "please", "okay",
        ] {
            assert!(!is_stop_phrase(t), "{t:?} must NOT count as a stop");
        }
    }

    #[test]
    fn test_followup_queue_cap_drops_newest() {
        FOLLOWUP_QUEUE.lock().clear();
        // GeneralAction appends (never supersedes) — use neutral phrasing.
        for i in 0..FOLLOWUP_CAP + 2 {
            let (kept, _) = enqueue_command(format!("tell me step {i}"));
            assert_eq!(kept, i < FOLLOWUP_CAP, "cap boundary at {i}");
        }
        // First-spoken five survive; the two newest were rejected.
        let head = dequeue_command().expect("head");
        assert_eq!(head.transcript, "tell me step 0");
        let mut rest = vec![head.transcript];
        while let Some(c) = dequeue_command() {
            rest.push(c.transcript);
        }
        assert_eq!(rest.len(), FOLLOWUP_CAP);
        assert_eq!(rest[FOLLOWUP_CAP - 1], format!("tell me step {}", FOLLOWUP_CAP - 1));
        // Drained.
        assert!(dequeue_command().is_none());
    }

    #[test]
    fn test_enqueue_supersedes_same_slot() {
        FOLLOWUP_QUEUE.lock().clear();
        // F3: "open Brave" then "open Chrome" → only Chrome pending.
        let (kept, depth) = enqueue_command("open brave".to_string());
        assert!(kept && depth == 1);
        let (kept, depth) = enqueue_command("open chrome".to_string());
        assert!(kept && depth == 1);
        // Navigate supersedes too ("search cats" → "search dogs").
        enqueue_command("search for cats".to_string());
        let (kept, depth) = enqueue_command("search for dogs".to_string());
        assert!(kept && depth == 2);
        let first = dequeue_command().expect("first");
        assert_eq!(first.transcript, "open chrome");
        assert_eq!(first.slot, SlotClass::AppOpen);
        let second = dequeue_command().expect("second");
        assert_eq!(second.transcript, "search for dogs");
        assert_eq!(second.slot, SlotClass::Navigate);
        assert!(dequeue_command().is_none());
        FOLLOWUP_QUEUE.lock().clear();
    }

    #[test]
    fn test_enqueue_appends_different_slots_fifo() {
        FOLLOWUP_QUEUE.lock().clear();
        enqueue_command("open brave".to_string());
        enqueue_command("create a new tab".to_string());
        enqueue_command("type hello world".to_string());
        // Three slots → three entries, strict arrival order.
        let a = dequeue_command().expect("a");
        let b = dequeue_command().expect("b");
        let c = dequeue_command().expect("c");
        assert_eq!(a.transcript, "open brave");
        assert_eq!(b.transcript, "create a new tab");
        assert_eq!(b.slot, SlotClass::BrowserAction);
        assert_eq!(c.transcript, "type hello world");
        assert_eq!(c.slot, SlotClass::Dictation);
        assert!(a.id < b.id && b.id < c.id);
        assert!(dequeue_command().is_none());
        FOLLOWUP_QUEUE.lock().clear();
    }

    #[test]
    fn test_classify_slot_table() {
        assert_eq!(classify_slot("open brave"), SlotClass::AppOpen);
        assert_eq!(classify_slot("launch calculator"), SlotClass::AppOpen);
        assert_eq!(classify_slot("close spotify"), SlotClass::AppOpen);
        assert_eq!(classify_slot("search for almonds"), SlotClass::Navigate);
        assert_eq!(classify_slot("go to youtube"), SlotClass::Navigate);
        assert_eq!(classify_slot("navigate to settings"), SlotClass::Navigate);
        assert_eq!(classify_slot("create a new tab"), SlotClass::BrowserAction);
        assert_eq!(classify_slot("reload the page"), SlotClass::BrowserAction);
        assert_eq!(classify_slot("type hello"), SlotClass::Dictation);
        assert_eq!(classify_slot("start dictation"), SlotClass::Dictation);
        assert_eq!(classify_slot("what is the capital of france"), SlotClass::GeneralAction);
        assert_eq!(classify_slot("stop"), SlotClass::GeneralAction);
    }

    #[test]
    fn test_drop_followups_purges_queue() {
        FOLLOWUP_QUEUE.lock().clear();
        enqueue_command("open brave".to_string());
        enqueue_command("create a new tab".to_string());
        assert_eq!(drop_followups(), 2);
        assert!(dequeue_command().is_none());
    }

    #[test]
    fn test_depth_ack_flag_reset() {
        DEPTH_ACK_SPOKEN.store(true, std::sync::atomic::Ordering::Relaxed);
        reset_depth_ack();
        assert!(!DEPTH_ACK_SPOKEN.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn test_vision_key_nudge_truth_table() {
        // Nudge only when NEITHER key exists anywhere (new user).
        assert!(needs_vision_key_nudge(false, false));
        // Returning user with either key (keychain OR settings.json —
        // `vision_keys_present` merges both) is never nagged.
        assert!(!needs_vision_key_nudge(true, false));
        assert!(!needs_vision_key_nudge(false, true));
        assert!(!needs_vision_key_nudge(true, true));
    }

    #[test]
    fn test_drill_busy_counter_nests_safely() {
        while drill_running() {
            drill_end();
        }
        assert!(!drill_running());
        drill_begin();
        drill_begin();
        assert!(drill_running());
        drill_end();
        assert!(drill_running());
        drill_end();
        assert!(!drill_running());
        // Saturating: never underflows.
        drill_end();
        assert!(!drill_running());
    }

    #[test]
    fn test_stop_flag_clears_when_the_last_drill_ends() {
        while drill_running() {
            drill_end();
        }
        clear_stop();
        drill_begin();
        drill_begin();
        request_stop();
        drill_end();
        assert!(stop_requested(), "one drill is still in flight and must still see the stop");
        drill_end();
        assert!(!stop_requested(), "the stop is consumed when the last drill ends");
    }

    #[test]
    fn test_stale_stop_without_a_drill_is_dropped_but_a_live_one_is_kept() {
        while drill_running() {
            drill_end();
        }
        request_stop();
        clear_stale_stop();
        assert!(!stop_requested(), "no drill is running: the flag is stale");
        drill_begin();
        request_stop();
        clear_stale_stop();
        assert!(stop_requested(), "a drill is running: it is still observing the stop");
        drill_end();
        assert!(!stop_requested());
    }

    #[test]
    fn test_drill_guard_brackets_unbracketed_runner() {
        while drill_running() {
            drill_end();
        }
        assert!(!drill_running());
        {
            let _guard = GhostDrillGuard::new();
            assert!(drill_running());
        }
        assert!(!drill_running());
    }
}
