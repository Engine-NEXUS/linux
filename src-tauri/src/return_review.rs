//! One return decision for logon, unlock, and sleep resume.
//!
//! Windows callbacks only enqueue signals. The worker stores the absence and
//! waits for the unlocked user and the stage before submitting one review.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{mpsc, OnceLock};
use std::time::Duration;
use tauri::{Emitter, Manager};

const CHECKPOINT_KEY: &str = "return_checkpoint_v1";
const CARD_KEY: &str = "return_review_card_v1";
const GREETING_AFTER_MS: i64 = 30 * 60 * 1000;
const BOOT_FALLBACK_MIN_MS: i64 = 60 * 1000;
static EVENTS: OnceLock<mpsc::Sender<Signal>> = OnceLock::new();
static AWAY: AtomicBool = AtomicBool::new(false);
static LOCKED: AtomicBool = AtomicBool::new(false);
static UNLOCK_CONFIRMED: AtomicBool = AtomicBool::new(false);
static DELIVERY_RUNNING: AtomicBool = AtomicBool::new(false);
static LAST_NATIVE_RESUME_MS: AtomicI64 = AtomicI64::new(0);
static AWAY_SIGNAL_MS: AtomicI64 = AtomicI64::new(0);
static SUBMITTED_FOR: AtomicI64 = AtomicI64::new(0);
static REVIEWING_FOR: AtomicI64 = AtomicI64::new(0);

#[derive(Clone, Copy, Debug)]
pub enum Signal {
    Lock,
    Unlock,
    Suspend,
    ResumeAutomatic,
    ResumeUser,
    SessionEnd,
    Retry,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Checkpoint {
    away_since_ms: Option<i64>,
    away_uncertainty_ms: i64,
    last_active_ms: Option<i64>,
    last_boot_tick_ms: u64,
    last_delivered_away_ms: Option<i64>,
    last_delivery_ms: Option<i64>,
    pending_revisions: Vec<(String, i64)>,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(target_os = "windows")]
fn boot_tick_ms() -> u64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
}

#[cfg(not(target_os = "windows"))]
fn boot_tick_ms() -> u64 { 0 }

#[cfg(target_os = "windows")]
fn session_unlocked() -> Option<bool> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::RemoteDesktop::{
        WTSFreeMemory, WTSQuerySessionInformationW, WTSINFOEXW,
        WTS_CURRENT_SESSION, WTSSessionInfoEx, WTS_SESSIONSTATE_UNLOCK, WTSActive,
    };
    let mut buffer = PWSTR(std::ptr::null_mut());
    let mut bytes = 0u32;
    let ok = unsafe { WTSQuerySessionInformationW(HANDLE(0), WTS_CURRENT_SESSION, WTSSessionInfoEx, &mut buffer, &mut bytes) };
    if !ok.as_bool() || buffer.0.is_null() { return None; }
    let state = if (bytes as usize) >= std::mem::size_of::<WTSINFOEXW>() {
        let info = unsafe { &*(buffer.0 as *const WTSINFOEXW) };
        if info.Level == 1 {
            let level = unsafe { info.Data.WTSInfoExLevel1 };
            Some(level.SessionState == WTSActive && level.SessionFlags as u32 == WTS_SESSIONSTATE_UNLOCK)
        } else { None }
    } else { None };
    unsafe { WTSFreeMemory(buffer.0 as *mut std::ffi::c_void) };
    state
}

#[cfg(not(target_os = "windows"))]
fn session_unlocked() -> Option<bool> { None }

fn owner_unlocked() -> bool {
    if let Some(unlocked) = session_unlocked() {
        LOCKED.store(!unlocked, Ordering::Release);
        UNLOCK_CONFIRMED.store(unlocked, Ordering::Release);
        unlocked
    } else {
        UNLOCK_CONFIRMED.load(Ordering::Acquire) && !LOCKED.load(Ordering::Acquire)
    }
}

fn read_from(store: &crate::memcore::store::Store) -> Checkpoint {
    store.meta_get(CHECKPOINT_KEY)
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn write_to(store: &crate::memcore::store::Store, checkpoint: &Checkpoint) {
    if let Ok(raw) = serde_json::to_string(checkpoint) {
        store.meta_set(CHECKPOINT_KEY, &raw);
    }
}

fn load(dir: &std::path::Path) -> Option<Checkpoint> {
    crate::memcore::with_store(dir, read_from)
}

fn save_away(dir: &std::path::Path, since_ms: i64, uncertainty_ms: i64) {
    crate::memcore::with_store(dir, |store| {
        let mut cp = read_from(store);
        if cp.away_since_ms.is_none()
            || (cp.away_since_ms.map(|old| since_ms > old).unwrap_or(false)
                && (cp.away_since_ms == Some(SUBMITTED_FOR.load(Ordering::Acquire))
                    || cp.away_since_ms == Some(REVIEWING_FOR.load(Ordering::Acquire))))
        {
            cp.away_since_ms = Some(since_ms);
            cp.away_uncertainty_ms = uncertainty_ms.max(0);
            cp.pending_revisions.clear();
        }
        write_to(store, &cp);
    });
    AWAY.store(true, Ordering::Release);
}

/// Session-end callbacks have little time before Windows tears down the
/// process, so their timestamp is written synchronously.
pub fn record_session_end(dir: &std::path::Path) {
    save_away(dir, now_ms(), 0);
}

pub fn cancel_session_end_record(dir: &std::path::Path) {
    cancel_session_end(dir);
}

fn cancel_session_end(dir: &std::path::Path) {
    crate::memcore::with_store(dir, |store| {
        let mut cp = read_from(store);
        cp.away_since_ms = None;
        cp.away_uncertainty_ms = 0;
        cp.pending_revisions.clear();
        write_to(store, &cp);
    });
    AWAY.store(false, Ordering::Release);
    AWAY_SIGNAL_MS.store(0, Ordering::Release);
}

fn mark_delivered(dir: &std::path::Path, away_since_ms: i64) {
    let next_away = if AWAY.load(Ordering::Acquire) {
        let since = AWAY_SIGNAL_MS.load(Ordering::Acquire);
        (since > away_since_ms).then_some(since)
    } else { None };
    crate::memcore::with_store(dir, |store| {
        let mut cp = read_from(store);
        if cp.away_since_ms == Some(away_since_ms) {
            cp.last_delivered_away_ms = Some(away_since_ms);
            cp.last_delivery_ms = Some(now_ms());
            cp.away_since_ms = next_away;
            cp.away_uncertainty_ms = 0;
            cp.pending_revisions.clear();
            write_to(store, &cp);
        }
    });
    if next_away.is_none() {
        AWAY.store(false, Ordering::Release);
        AWAY_SIGNAL_MS.store(0, Ordering::Release);
    }
}

fn save_card(dir: &std::path::Path, text: &str) {
    let card = serde_json::json!({ "text": text, "at": now_ms() });
    crate::memcore::with_store(dir, |store| store.meta_set(CARD_KEY, &card.to_string()));
}

#[tauri::command]
pub fn get_return_review<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Option<serde_json::Value> {
    let dir = app.path().app_data_dir().ok()?;
    crate::memcore::with_store(&dir, |store| store.meta_get(CARD_KEY)).flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
}

fn heartbeat(dir: &std::path::Path) {
    if AWAY.load(Ordering::Acquire) { return; }
    let Some(idle_secs) = crate::memcore::resume::user_idle_secs() else { return };
    if idle_secs > 30 { return; }
    let last_input = now_ms().saturating_sub((idle_secs as i64).saturating_mul(1000));
    crate::memcore::with_store(dir, |store| {
        let mut cp = read_from(store);
        if cp.away_since_ms.is_some() { return; }
        cp.last_active_ms = Some(last_input);
        cp.last_boot_tick_ms = boot_tick_ms();
        write_to(store, &cp);
    });
}

/// Called by the hidden Windows message window; no disk or Tauri work occurs
/// inside its window procedure.
pub fn signal(signal: Signal) {
    if matches!(signal, Signal::Lock | Signal::Suspend | Signal::SessionEnd) {
        if !AWAY.swap(true, Ordering::AcqRel) {
            AWAY_SIGNAL_MS.store(now_ms(), Ordering::Release);
        }
    }
    if matches!(signal, Signal::Lock | Signal::Suspend | Signal::SessionEnd) {
        LOCKED.store(true, Ordering::Release);
        UNLOCK_CONFIRMED.store(false, Ordering::Release);
    }
    if matches!(signal, Signal::Unlock) {
        LOCKED.store(false, Ordering::Release);
        UNLOCK_CONFIRMED.store(true, Ordering::Release);
    }
    if matches!(signal, Signal::ResumeAutomatic | Signal::ResumeUser) {
        LAST_NATIVE_RESUME_MS.store(now_ms(), Ordering::Release);
    }
    if let Some(tx) = EVENTS.get() {
        let _ = tx.send(signal);
    }
}

pub fn is_away() -> bool {
    AWAY.load(Ordering::Acquire) || LOCKED.load(Ordering::Acquire)
}

/// Called by the frontend's existing end-of-speech handshake. A queued or
/// failed voice attempt remains pending across restart until this receipt.
pub fn on_spoken(request_id: &str, dir: &std::path::Path) {
    let Some(since) = request_id.strip_prefix("sentinel_return_review_")
        .and_then(|value| value.parse::<i64>().ok()) else { return };
    if SUBMITTED_FOR.load(Ordering::Acquire) == since {
        SUBMITTED_FOR.store(0, Ordering::Release);
    }
    let next_away = if AWAY.load(Ordering::Acquire) {
        let after = AWAY_SIGNAL_MS.load(Ordering::Acquire);
        (after > since).then_some(after)
    } else { None };
    crate::memcore::with_store(dir, |store| store.batch(|store| {
        let mut cp = read_from(store);
        if cp.away_since_ms != Some(since) { return; }
        crate::memcore::agent_tasks::mark_announced_in(store, &cp.pending_revisions);
        cp.last_delivered_away_ms = Some(since);
        cp.last_delivery_ms = Some(now_ms());
        cp.away_since_ms = next_away;
        cp.away_uncertainty_ms = 0;
        cp.pending_revisions.clear();
        write_to(store, &cp);
        if next_away.is_none() {
            AWAY.store(false, Ordering::Release);
            AWAY_SIGNAL_MS.store(0, Ordering::Release);
        }
    }));
}

fn greeting_due(duration_ms: i64) -> bool {
    duration_ms >= GREETING_AFTER_MS
}

async fn deliver<R: tauri::Runtime>(app: tauri::AppHandle<R>, dir: std::path::PathBuf, cold_start: bool) {
    if DELIVERY_RUNNING.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return;
    }
    let Some(cp) = load(&dir) else {
        DELIVERY_RUNNING.store(false, Ordering::Release);
        return;
    };
    let Some(away_since_ms) = cp.away_since_ms else {
        DELIVERY_RUNNING.store(false, Ordering::Release);
        return;
    };
    if SUBMITTED_FOR.load(Ordering::Acquire) == away_since_ms {
        DELIVERY_RUNNING.store(false, Ordering::Release);
        return;
    }
    if cp.last_delivered_away_ms == Some(away_since_ms) {
        DELIVERY_RUNNING.store(false, Ordering::Release);
        return;
    }

    // A power event can occur before the user unlocks. Wait for an
    // unlocked session and fresh input before showing private task details.
    let returned_at_ms = loop {
        let fresh_input = crate::memcore::resume::user_idle_secs().map(|s| s < 30).unwrap_or(false);
        if owner_unlocked() && fresh_input {
            break now_ms();
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    REVIEWING_FOR.store(away_since_ms, Ordering::Release);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let away_ms = returned_at_ms.saturating_sub(away_since_ms);
    crate::agent_watch::reconcile_known(&app).await;
    let tasks = crate::memcore::agent_tasks::list(&dir);
    let due_tasks = crate::memcore::agent_tasks::due_for_review(&tasks);
    let task_line = crate::memcore::agent_tasks::spoken_review(&due_tasks);
    let greet = greeting_due(away_ms.saturating_sub(cp.away_uncertainty_ms));
    let mut parts = Vec::new();
    if greet {
        use chrono::Timelike;
        let hour = chrono::Local::now().hour();
        let name = crate::memory::read_user_profile(&dir).and_then(|p| p.name);
        let friend = crate::persona::is_friend(&crate::commands::read_persona_mode(&app));
        parts.push(crate::memcore::briefing::hello_line(hour, name.as_deref(), friend));
        if let Some(place) = crate::memcore::briefing::return_place(&dir, chrono::Utc::now().timestamp(), cold_start) {
            parts.push(place);
        }
    }
    if let Some(line) = task_line {
        parts.push(line);
    } else if greet {
        parts.push(if tasks.is_empty() {
            "No agent tasks are being tracked.".into()
        } else {
            "No new agent updates.".into()
        });
    }

    let card_text = if parts.is_empty() {
        if tasks.is_empty() { "No agent tasks are being tracked." } else { "No new agent updates." }.to_string()
    } else { parts.join(" ") };
    save_card(&dir, &card_text);
    let _ = app.emit("return:review", serde_json::json!({
        "awayMinutes": away_ms / 60_000,
        "greeting": greet,
        "tasks": tasks.len(),
        "text": card_text,
    }));
    if !parts.is_empty() {
        // The card is available first. Sound waits for the orb/caption
        // surface and a still-present owner, without dropping the episode.
        loop {
            let fresh_input = crate::memcore::resume::user_idle_secs().map(|s| s < 30).unwrap_or(false);
            if owner_unlocked() && fresh_input && crate::stage::is_ready() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        if load(&dir).and_then(|checkpoint| checkpoint.away_since_ms) != Some(away_since_ms) {
            REVIEWING_FOR.store(0, Ordering::Release);
            DELIVERY_RUNNING.store(false, Ordering::Release);
            signal(Signal::Retry);
            return;
        }
        crate::memcore::with_store(&dir, |store| {
            let mut checkpoint = read_from(store);
            if checkpoint.away_since_ms == Some(away_since_ms) {
                checkpoint.pending_revisions = due_tasks.iter().map(|task| (task.id.clone(), task.observed_at)).collect();
                write_to(store, &checkpoint);
            }
        });
        // Clear the away gate immediately before submitting the one combined
        // return turn. The proactive policy still waits for meetings/speech.
        crate::proactive_policy::merge_agent_alerts_into_return();
        SUBMITTED_FOR.store(away_since_ms, Ordering::Release);
        REVIEWING_FOR.store(0, Ordering::Release);
        AWAY.store(false, Ordering::Release);
        crate::proactive_policy::submit(
            &app,
            format!("return_review_{away_since_ms}"),
            parts.join(" "),
            crate::google::types::AlertUrgency::High,
        );
    } else {
        // The short-return, no-task case has only a quiet card.
        mark_delivered(&dir, away_since_ms);
    }
    REVIEWING_FOR.store(0, Ordering::Release);
    let next_episode = load(&dir).and_then(|checkpoint| checkpoint.away_since_ms)
        .filter(|since| *since != away_since_ms);
    DELIVERY_RUNNING.store(false, Ordering::Release);
    if next_episode.is_some() {
        signal(Signal::Retry);
    }
}

pub fn start<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    let Ok(dir) = app.path().app_data_dir() else { return };
    if !crate::memcore::enabled(&dir) || !crate::memcore::flag(&dir, "memcoreBriefing", true) {
        return;
    }
    crate::memcore::agent_tasks::mark_previous_run_unconfirmed(&dir);

    let (tx, rx) = mpsc::channel();
    if EVENTS.set(tx).is_err() { return; }
    let worker_app = app.clone();
    let worker_dir = dir.clone();
    std::thread::Builder::new().name("return-events".into()).spawn(move || {
        while let Ok(signal) = rx.recv() {
            match signal {
                Signal::Lock | Signal::Suspend | Signal::SessionEnd => {
                    let since = AWAY_SIGNAL_MS.load(Ordering::Acquire);
                    save_away(&worker_dir, if since > 0 { since } else { now_ms() }, 0);
                }
                Signal::Unlock | Signal::ResumeUser | Signal::Retry => {
                    let app = worker_app.clone();
                    let dir = worker_dir.clone();
                    tauri::async_runtime::spawn(async move { deliver(app, dir, false).await });
                }
                Signal::ResumeAutomatic => {}
            }
        }
    }).ok();

    let heartbeat_dir = dir.clone();
    std::thread::Builder::new().name("return-heartbeat".into()).spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30));
        heartbeat(&heartbeat_dir);
    }).ok();

    let cp = load(&dir).unwrap_or_default();
    if let Some(since) = cp.away_since_ms {
        AWAY.store(true, Ordering::Release);
        tracing::info!("return_review: resuming saved episode {since}");
        tauri::async_runtime::spawn(async move { deliver(app, dir, true).await });
    } else if cp.last_active_ms.is_some()
        && cp.last_boot_tick_ms > 0
        && boot_tick_ms() < cp.last_boot_tick_ms
    {
        // No clean shutdown notification (power cut/crash), but the OS tick
        // restarted. The last confirmed input is a lower-bound checkpoint.
        let since = cp.last_active_ms.unwrap_or_default();
        if now_ms().saturating_sub(since) >= BOOT_FALLBACK_MIN_MS {
            save_away(&dir, since, 30_000);
            tauri::async_runtime::spawn(async move { deliver(app, dir, true).await });
        }
    } else {
        heartbeat(&dir);
    }
}

/// The old ten-second wall-clock watcher stays as a fallback when Windows
/// did not deliver a suspend event (for example, after hibernation).
pub fn on_time_gap<R: tauri::Runtime>(app: tauri::AppHandle<R>, gap_secs: u64) {
    if now_ms().saturating_sub(LAST_NATIVE_RESUME_MS.load(Ordering::Acquire)) < 120_000 {
        return;
    }
    let Ok(dir) = app.path().app_data_dir() else { return };
    if !crate::memcore::enabled(&dir) || !crate::memcore::flag(&dir, "memcoreBriefing", true) {
        return;
    }
    let cp = load(&dir).unwrap_or_default();
    if cp.last_delivery_ms.map(|ts| now_ms().saturating_sub(ts) < 120_000).unwrap_or(false) {
        return;
    }
    if cp.away_since_ms.is_none() {
        let since = now_ms().saturating_sub((gap_secs as i64).saturating_mul(1000));
        save_away(&dir, since, 10_000);
    }
    tauri::async_runtime::spawn(async move { deliver(app, dir, false).await });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_greeting_starts_at_exactly_thirty_minutes() {
        assert!(!greeting_due(0));
        assert!(!greeting_due(5 * 60_000));
        assert!(!greeting_due(10 * 60_000));
        assert!(!greeting_due(GREETING_AFTER_MS - 1));
        assert!(greeting_due(GREETING_AFTER_MS));
        assert!(greeting_due(GREETING_AFTER_MS + 1));
        assert!(!greeting_due((GREETING_AFTER_MS + 20_000).saturating_sub(30_000)));
    }
}
