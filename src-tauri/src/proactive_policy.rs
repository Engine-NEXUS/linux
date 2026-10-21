//! Proactive-speech policy — Phase 9.
//!
//! PROBLEM: the Sentinel (mail deadline alerts, …) used to speak the instant it detected something:
//! `speak_proactive_alert` → `speak_line`, gated only by urgency. It never asked whether the user was
//! mid-sentence, NEXUS was already talking (the frontend `speak()` stops the current audio first, so an
//! alert would CUT the reply off), a Ghost drill was running — and during a meeting the frontend mutes
//! all speech, so the alert was silently lost.
//!
//! RESEARCH (Eliciting Spoken Interruptions to Inform Proactive Speech Agent Design, CUI 2021,
//! arXiv 2106.02077 — abstract only; it publishes no numbers, so every threshold below is OUR hypothesis
//! to tune): people interrupt sooner when it is urgent, time interruptions around *breakpoints* in the
//! other person's task, vary delivery with urgency, and sometimes give a short heads-up first.
//!
//! DESIGN: a pure, injectable-clock `Engine` (unit-testable timelines) + thin runtime glue:
//!   * Low      → card only, never voice.
//!   * Medium   → voice only when nothing is going on AND the user has been idle ≥ 30 s; waits up to 15 min.
//!   * High     → waits for a breakpoint (user not speaking, NEXUS not speaking, no drill); in a meeting it
//!                waits for the meeting to end (≤ 30 min); gives up (card) after 10 min.
//!   * Critical → speaks within seconds; waits only briefly (≤ 8 s) for the user to finish a sentence or
//!                NEXUS to finish its own line; **speaks even during a meeting** (your decision, setting
//!                `proactiveCriticalInMeeting`, default on) via a short, explicit TTS-mute override.
//!   * Heads-up ("access ritual"): an event + 300 ms beat before the line; Critical opens with "Urgent, sir."
//!   * Rate limits for non-critical: ≥ 20 s between spoken alerts, ≤ 6 per hour; "not now" snooze.
//!   * Each alert id is handled once (dedup).

use crate::google::types::AlertUrgency;
use std::collections::HashSet;

// ── tunables (hypotheses; see module docs) ──────────────────────────────────────────────────
const MEDIUM_IDLE_MS: u64 = 30_000;
const MEDIUM_EXPIRE_MS: u64 = 15 * 60_000;
const HIGH_EXPIRE_MS: u64 = 10 * 60_000;
const HIGH_MEETING_EXPIRE_MS: u64 = 30 * 60_000;
/// How long High tolerates a running Ghost drill / request before speaking anyway (not over the user).
const HIGH_WORKING_CEILING_MS: u64 = 60_000;
/// How long Critical politely waits for the user / NEXUS to finish before speaking regardless.
const CRITICAL_WAIT_MS: u64 = 8_000;
const MIN_GAP_MS: u64 = 20_000;
const MAX_PER_HOUR: usize = 6;
const HOUR_MS: u64 = 3_600_000;
/// Heads-up beat between the nudge event and the spoken line.
pub const HEADS_UP_MS: u64 = 300;
/// How long the meeting TTS mute is lifted for one Critical line.
pub const MEETING_OVERRIDE_MS: u64 = 30_000;

/// What is going on around the user right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signals {
    /// Mic open AND voice detected (mid-sentence).
    pub user_speaking: bool,
    /// NEXUS TTS is playing (an alert would cut it off).
    pub nexus_speaking: bool,
    /// A Ghost drill or an orchestrator request is running.
    pub working: bool,
    /// A meeting / call is detected (all speech is muted by default).
    pub meeting: bool,
    /// Milliseconds since the user last interacted with NEXUS.
    pub idle_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub critical_in_meeting: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { critical_in_meeting: true }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    pub id: String,
    pub text: String,
    pub urgency: AlertUrgency,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Speak now. `lead_in` is prepended; `override_meeting` lifts the meeting mute for this line.
    Speak { id: String, text: String, lead_in: Option<&'static str>, override_meeting: bool },
    /// Do not speak; the alert stays visible as a card (already emitted by the Sentinel).
    Card { id: String, reason: &'static str },
}

#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Speak { override_meeting: bool },
    Wait,
    Card(&'static str),
}

/// The per-alert decision (before rate limits / snooze).
/// `waited_ms` = total time pending. `active_ms` = time pending while it COULD have been spoken (not in a
/// meeting, not snoozed): expiry runs on `active_ms`, so an alert held through a long meeting or a
/// snooze is still fresh when the user becomes available again.
fn evaluate(urgency: AlertUrgency, s: &Signals, cfg: &Config, waited_ms: u64, active_ms: u64) -> Verdict {
    match urgency {
        AlertUrgency::Low => Verdict::Card("low_urgency"),
        AlertUrgency::Medium => {
            if active_ms >= MEDIUM_EXPIRE_MS {
                return Verdict::Card("stale");
            }
            let busy = s.meeting || s.user_speaking || s.nexus_speaking || s.working;
            if busy || s.idle_ms < MEDIUM_IDLE_MS {
                Verdict::Wait
            } else {
                Verdict::Speak { override_meeting: false }
            }
        }
        AlertUrgency::High => {
            if s.meeting {
                return if waited_ms >= HIGH_MEETING_EXPIRE_MS { Verdict::Card("meeting_too_long") } else { Verdict::Wait };
            }
            if active_ms >= HIGH_EXPIRE_MS {
                return Verdict::Card("stale");
            }
            let working_blocks = s.working && active_ms < HIGH_WORKING_CEILING_MS;
            if s.user_speaking || s.nexus_speaking || working_blocks {
                Verdict::Wait
            } else {
                Verdict::Speak { override_meeting: false }
            }
        }
        AlertUrgency::Critical => {
            let polite = waited_ms < CRITICAL_WAIT_MS;
            if polite && (s.user_speaking || s.nexus_speaking) {
                return Verdict::Wait;
            }
            if s.meeting && !cfg.critical_in_meeting {
                // user opted out: behave like High in a meeting
                return if waited_ms >= HIGH_MEETING_EXPIRE_MS { Verdict::Card("meeting_too_long") } else { Verdict::Wait };
            }
            Verdict::Speak { override_meeting: s.meeting }
        }
    }
}

struct Pending {
    alert: Alert,
    since_ms: u64,
    /// Pending time during which the alert could have been spoken (see `evaluate`).
    active_ms: u64,
}

/// The queue + rate limiter. Pure over an injected clock (`now_ms`).
#[derive(Default)]
pub struct Engine {
    pending: Vec<Pending>,
    seen: HashSet<String>,
    last_spoken_ms: Option<u64>,
    spoken: Vec<u64>,
    snoozed_until_ms: u64,
    last_tick_ms: Option<u64>,
    last_blocked: bool,
}

impl Engine {
    pub fn new() -> Self {
        Engine::default()
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// "Not now": hold non-critical alerts until `now + minutes`. Critical ignores snooze.
    pub fn snooze(&mut self, now_ms: u64, minutes: u64) {
        self.snoozed_until_ms = now_ms + minutes * 60_000;
    }

    /// New alert. Duplicates (same id) are dropped. Returns the actions this makes possible right now.
    pub fn submit(&mut self, alert: Alert, now_ms: u64, s: &Signals, cfg: &Config) -> Vec<Action> {
        if !self.seen.insert(alert.id.clone()) {
            return Vec::new();
        }
        self.accrue(now_ms, s); // credit existing alerts BEFORE the new one exists (it has waited 0)
        self.pending.push(Pending { alert, since_ms: now_ms, active_ms: 0 });
        self.run(now_ms, s, cfg)
    }

    /// Re-evaluate everything pending. At most ONE line is spoken per tick (no pile-ups); cards are
    /// reported for every alert that gave up waiting.
    pub fn tick(&mut self, now_ms: u64, s: &Signals, cfg: &Config) -> Vec<Action> {
        self.accrue(now_ms, s);
        self.run(now_ms, s, cfg)
    }

    /// Accrue "could have been spoken" time for everything currently pending. An interval touching a
    /// meeting or a snooze (by either tick's state) does not count — conservative: it can only keep an
    /// alert alive longer.
    fn accrue(&mut self, now_ms: u64, s: &Signals) {
        let dt = self.last_tick_ms.map(|t| now_ms.saturating_sub(t)).unwrap_or(0);
        let blocked_now = s.meeting || now_ms < self.snoozed_until_ms;
        if !(blocked_now || self.last_blocked) {
            for p in self.pending.iter_mut() {
                p.active_ms += dt;
            }
        }
        self.last_tick_ms = Some(now_ms);
        self.last_blocked = blocked_now;
    }

    fn run(&mut self, now_ms: u64, s: &Signals, cfg: &Config) -> Vec<Action> {
        self.spoken.retain(|t| now_ms.saturating_sub(*t) < HOUR_MS);
        let mut actions = Vec::new();
        let mut spoke = false;
        let mut keep: Vec<Pending> = Vec::new();
        // Most urgent first, then oldest first.
        let mut queue: Vec<Pending> = std::mem::take(&mut self.pending);
        queue.sort_by(|a, b| rank(b.alert.urgency).cmp(&rank(a.alert.urgency)).then(a.since_ms.cmp(&b.since_ms)));
        for p in queue {
            let waited = now_ms.saturating_sub(p.since_ms);
            let critical = p.alert.urgency == AlertUrgency::Critical;
            match evaluate(p.alert.urgency, s, cfg, waited, p.active_ms) {
                Verdict::Card(reason) => actions.push(Action::Card { id: p.alert.id.clone(), reason }),
                Verdict::Wait => keep.push(p),
                Verdict::Speak { override_meeting } => {
                    if spoke {
                        keep.push(p);
                        continue;
                    }
                    if !critical {
                        if now_ms < self.snoozed_until_ms {
                            keep.push(p);
                            continue;
                        }
                        if self.spoken.len() >= MAX_PER_HOUR {
                            actions.push(Action::Card { id: p.alert.id.clone(), reason: "rate_limited" });
                            continue;
                        }
                        // The boot hello and the briefing that follows it are one scripted sequence.
                        let scripted_boot = p.alert.id.contains("boot_");
                        if !scripted_boot
                            && matches!(self.last_spoken_ms, Some(t) if now_ms.saturating_sub(t) < MIN_GAP_MS)
                        {
                            keep.push(p);
                            continue;
                        }
                    }
                    spoke = true;
                    self.last_spoken_ms = Some(now_ms);
                    if !critical {
                        self.spoken.push(now_ms);
                    }
                    actions.push(Action::Speak {
                        id: p.alert.id.clone(),
                        text: p.alert.text.clone(),
                        lead_in: if critical { Some("Urgent, sir. ") } else { None },
                        override_meeting,
                    });
                }
            }
        }
        self.pending = keep;
        actions
    }
}

fn rank(u: AlertUrgency) -> u8 {
    match u {
        AlertUrgency::Low => 0,
        AlertUrgency::Medium => 1,
        AlertUrgency::High => 2,
        AlertUrgency::Critical => 3,
    }
}

// ── runtime glue ────────────────────────────────────────────────────────────────────────────

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, Runtime};

static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);
static START: OnceLock<std::time::Instant> = OnceLock::new();
static LAST_USER_ACTIVITY_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64 + 1
}

/// The user interacted with NEXUS (wake, hotkey, any capture start). Feeds the "idle" signal.
pub fn note_user_activity() {
    LAST_USER_ACTIVITY_MS.store(now_ms(), Ordering::Relaxed);
}

fn read_cfg<R: Runtime>(app: &AppHandle<R>) -> Config {
    let Ok(dir) = app.path().app_data_dir() else { return Config::default() };
    let Ok(content) = std::fs::read_to_string(dir.join("settings.json")) else { return Config::default() };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else { return Config::default() };
    Config {
        critical_in_meeting: json
            .get("proactiveCriticalInMeeting")
            .or_else(|| json.get("proactive_critical_in_meeting"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
    }
}

fn collect_signals<R: Runtime>(app: &AppHandle<R>) -> Signals {
    let (meeting, tts) = app
        .try_state::<std::sync::Arc<crate::meeting_detect::MeetingState>>()
        .map(|m| (m.should_suppress_tts(), m.tts_playing.load(Ordering::Relaxed)))
        .unwrap_or((false, false));
    let last = LAST_USER_ACTIVITY_MS.load(Ordering::Relaxed);
    Signals {
        user_speaking: crate::wakeword_oww::stt_user_speaking(),
        nexus_speaking: tts,
        working: crate::ghost::drill_running() || crate::orchestrator::has_active_request(),
        meeting,
        idle_ms: if last == 0 { u64::MAX / 4 } else { now_ms().saturating_sub(last) },
    }
}

fn execute<R: Runtime>(app: &AppHandle<R>, actions: Vec<Action>) {
    for a in actions {
        match a {
            Action::Card { id, reason } => {
                tracing::info!("proactive: '{id}' not spoken ({reason}) — stays as a card");
                let _ = app.emit("proactive:card", serde_json::json!({ "id": id, "reason": reason }));
            }
            Action::Speak { id, text, lead_in, override_meeting } => {
                tracing::info!("proactive: speaking '{id}' (meeting override: {override_meeting})");
                // heads-up ("access ritual"): nudge event, then a short beat, then the line
                let _ = app.emit("proactive:nudge", serde_json::json!({ "id": id, "critical": lead_in.is_some() }));
                if override_meeting {
                    if let Some(m) = app.try_state::<std::sync::Arc<crate::meeting_detect::MeetingState>>() {
                        m.allow_tts_override(MEETING_OVERRIDE_MS);
                    }
                }
                let app2 = app.clone();
                let line = format!("{}{}", lead_in.unwrap_or(""), text);
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(HEADS_UP_MS)).await;
                    let req = format!("sentinel_{id}");
                    // Register with the main command center like any other
                    // turn — otherwise has_active_request() can't see this
                    // utterance as busy while it's speaking, and a second
                    // proactive/Medium alert could judge it safe to speak
                    // over the first one. speak_line only emits the Result
                    // event and returns immediately (no Rust-side playback
                    // callback exists); the frontend's own
                    // finishSpokenResult -> signalOrchestratorDone ->
                    // orchestrator_done IPC already clears ACTIVE_REQUEST
                    // generically once this speech actually finishes — the
                    // same handshake every WorkerBackend-success turn
                    // already relies on, so nothing extra is needed here.
                    crate::orchestrator::install_new_request_with_id(
                        req.clone(),
                        crate::orchestrator::Subsystem::LocalCommand,
                    );
                    crate::orchestrator::speak_line(&app2, line, &req);
                });
            }
        }
    }
}

/// Submit an alert for (possibly deferred) speech. Replaces the old "speak immediately" path.
pub fn submit<R: Runtime>(app: &AppHandle<R>, id: String, text: String, urgency: AlertUrgency) {
    let sig = collect_signals(app);
    let cfg = read_cfg(app);
    let actions = {
        let Ok(mut g) = ENGINE.lock() else { return };
        g.get_or_insert_with(Engine::new).submit(Alert { id, text, urgency }, now_ms(), &sig, &cfg)
    };
    execute(app, actions);
}

/// Background ticker: re-evaluates deferred alerts every 2 s (cheap no-op when nothing is pending).
pub fn start_ticker<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let pending = ENGINE.lock().ok().and_then(|g| g.as_ref().map(|e| e.pending_len())).unwrap_or(0);
            if pending == 0 {
                continue;
            }
            let sig = collect_signals(&app);
            let cfg = read_cfg(&app);
            let actions = ENGINE
                .lock()
                .ok()
                .and_then(|mut g| g.as_mut().map(|e| e.tick(now_ms(), &sig, &cfg)))
                .unwrap_or_default();
            execute(&app, actions);
        }
    });
}

/// IPC: "not now" — hold non-critical proactive speech for `minutes` (default 15).
#[tauri::command]
pub fn proactive_snooze(minutes: Option<u64>) -> Result<u64, String> {
    let m = minutes.unwrap_or(15).clamp(1, 240);
    if let Ok(mut g) = ENGINE.lock() {
        g.get_or_insert_with(Engine::new).snooze(now_ms(), m);
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(id: &str, u: AlertUrgency) -> Alert {
        Alert { id: id.into(), text: format!("text of {id}"), urgency: u }
    }
    const CFG: Config = Config { critical_in_meeting: true };
    fn quiet() -> Signals {
        Signals { idle_ms: 120_000, ..Default::default() }
    }

    fn speaks(a: &[Action]) -> Vec<String> {
        a.iter().filter_map(|x| if let Action::Speak { id, .. } = x { Some(id.clone()) } else { None }).collect()
    }
    fn cards(a: &[Action]) -> Vec<(String, &'static str)> {
        a.iter().filter_map(|x| if let Action::Card { id, reason } = x { Some((id.clone(), *reason)) } else { None }).collect()
    }

    #[test]
    fn the_boot_hello_is_spoken_at_once_even_for_an_active_user_and_the_briefing_follows_without_the_20s_gap() {
        let mut e = Engine::new();
        // The user just typed a password: input a moment ago (idle_ms tiny). A Medium alert would wait for 30 s idle.
        let active = Signals { idle_ms: 500, ..Default::default() };
        assert_eq!(speaks(&e.submit(alert("boot_hello_1", AlertUrgency::High), 1_000, &active, &CFG)), vec!["boot_hello_1"]);
        // The scripted briefing right after is not held back by the normal 20 s gap...
        assert_eq!(speaks(&e.submit(alert("boot_briefing", AlertUrgency::High), 4_000, &active, &CFG)), vec!["boot_briefing"]);
        // ...but any other alert still is.
        assert!(e.submit(alert("mail_1", AlertUrgency::High), 5_000, &active, &CFG).is_empty());
    }

    #[test]
    fn low_is_card_only_never_voice() {
        let mut e = Engine::new();
        let a = e.submit(alert("l", AlertUrgency::Low), 1_000, &quiet(), &CFG);
        assert!(speaks(&a).is_empty());
        assert_eq!(cards(&a), vec![("l".to_string(), "low_urgency")]);
        assert_eq!(e.pending_len(), 0);
    }

    #[test]
    fn high_speaks_at_a_breakpoint_and_waits_otherwise() {
        let mut e = Engine::new();
        // user mid-sentence: wait
        let s = Signals { user_speaking: true, ..quiet() };
        assert!(e.submit(alert("h", AlertUrgency::High), 1_000, &s, &CFG).is_empty());
        assert_eq!(e.pending_len(), 1);
        // NEXUS still talking: wait (an alert would cut the reply off)
        let s = Signals { nexus_speaking: true, ..quiet() };
        assert!(e.tick(5_000, &s, &CFG).is_empty());
        // breakpoint reached: speak, exactly once
        let a = e.tick(7_000, &quiet(), &CFG);
        assert_eq!(speaks(&a), vec!["h"]);
        assert_eq!(e.pending_len(), 0);
        assert!(e.tick(9_000, &quiet(), &CFG).is_empty(), "never spoken twice");
    }

    #[test]
    fn high_tolerates_a_drill_only_for_a_while() {
        let mut e = Engine::new();
        let working = Signals { working: true, ..quiet() };
        assert!(e.submit(alert("h", AlertUrgency::High), 0, &working, &CFG).is_empty());
        assert!(e.tick(30_000, &working, &CFG).is_empty());
        // after the 60 s ceiling it speaks even though a drill is still running (but never over the user)
        assert_eq!(speaks(&e.tick(61_000, &working, &CFG)), vec!["h"]);
        let mut e2 = Engine::new();
        let both = Signals { working: true, user_speaking: true, ..quiet() };
        assert!(e2.submit(alert("h", AlertUrgency::High), 0, &both, &CFG).is_empty());
        assert!(e2.tick(120_000, &both, &CFG).is_empty(), "still never over the user");
    }

    #[test]
    fn high_in_a_meeting_waits_for_it_to_end_then_speaks_once() {
        let mut e = Engine::new();
        let meeting = Signals { meeting: true, ..quiet() };
        assert!(e.submit(alert("h", AlertUrgency::High), 0, &meeting, &CFG).is_empty());
        for t in [60_000u64, 600_000, 1_200_000] {
            assert!(e.tick(t, &meeting, &CFG).is_empty());
        }
        let a = e.tick(1_500_000, &quiet(), &CFG); // meeting ended at ~25 min
        assert_eq!(speaks(&a), vec!["h"]);
        assert!(e.tick(1_502_000, &quiet(), &CFG).is_empty());
    }

    #[test]
    fn high_gives_up_after_a_very_long_meeting_and_after_ten_idle_minutes() {
        let mut e = Engine::new();
        let meeting = Signals { meeting: true, ..quiet() };
        e.submit(alert("h", AlertUrgency::High), 0, &meeting, &CFG);
        let a = e.tick(31 * 60_000, &meeting, &CFG);
        assert_eq!(cards(&a), vec![("h".to_string(), "meeting_too_long")]);
        let mut e = Engine::new();
        let busy = Signals { user_speaking: true, ..quiet() };
        e.submit(alert("h2", AlertUrgency::High), 0, &busy, &CFG);
        let a = e.tick(11 * 60_000, &busy, &CFG);
        assert_eq!(cards(&a), vec![("h2".to_string(), "stale")]);
    }

    #[test]
    fn critical_speaks_in_a_meeting_with_the_override_and_a_lead_in() {
        let mut e = Engine::new();
        let meeting = Signals { meeting: true, ..quiet() };
        let a = e.submit(alert("c", AlertUrgency::Critical), 1_000, &meeting, &CFG);
        assert_eq!(
            a,
            vec![Action::Speak { id: "c".into(), text: "text of c".into(), lead_in: Some("Urgent, sir. "), override_meeting: true }]
        );
    }

    #[test]
    fn critical_opt_out_behaves_like_high_in_a_meeting() {
        let cfg = Config { critical_in_meeting: false };
        let mut e = Engine::new();
        let meeting = Signals { meeting: true, ..quiet() };
        assert!(e.submit(alert("c", AlertUrgency::Critical), 0, &meeting, &cfg).is_empty());
        assert_eq!(speaks(&e.tick(60_000, &quiet(), &cfg)), vec!["c"]);
    }

    #[test]
    fn critical_waits_briefly_for_the_user_then_speaks_anyway() {
        let mut e = Engine::new();
        let talking = Signals { user_speaking: true, ..quiet() };
        assert!(e.submit(alert("c", AlertUrgency::Critical), 0, &talking, &CFG).is_empty());
        assert!(e.tick(5_000, &talking, &CFG).is_empty());
        assert_eq!(speaks(&e.tick(8_500, &talking, &CFG)), vec!["c"]);
        // and it does not cut off NEXUS's own line either, within the grace period
        let mut e = Engine::new();
        let tts = Signals { nexus_speaking: true, ..quiet() };
        assert!(e.submit(alert("c2", AlertUrgency::Critical), 0, &tts, &CFG).is_empty());
        assert_eq!(speaks(&e.tick(2_000, &quiet(), &CFG)), vec!["c2"]);
    }

    #[test]
    fn medium_needs_thirty_seconds_of_idle_and_expires() {
        let mut e = Engine::new();
        let recent = Signals { idle_ms: 5_000, ..Default::default() };
        assert!(e.submit(alert("m", AlertUrgency::Medium), 0, &recent, &CFG).is_empty());
        let idle = Signals { idle_ms: 45_000, ..Default::default() };
        assert_eq!(speaks(&e.tick(40_000, &idle, &CFG)), vec!["m"]);
        let mut e = Engine::new();
        e.submit(alert("m2", AlertUrgency::Medium), 0, &recent, &CFG);
        assert_eq!(cards(&e.tick(16 * 60_000, &recent, &CFG)), vec![("m2".to_string(), "stale")]);
    }

    #[test]
    fn one_line_per_tick_most_urgent_first_and_rate_gap_for_non_critical() {
        let mut e = Engine::new();
        let s = quiet();
        // two High alerts pending behind a busy user
        let busy = Signals { user_speaking: true, ..quiet() };
        e.submit(alert("h1", AlertUrgency::High), 0, &busy, &CFG);
        e.submit(alert("h2", AlertUrgency::High), 100, &busy, &CFG);
        e.submit(alert("c", AlertUrgency::Critical), 200, &busy, &CFG);
        // breakpoint: critical first, only one line this tick
        assert_eq!(speaks(&e.tick(9_000, &s, &CFG)), vec!["c"]);
        // next tick 2 s later: High respects the 20 s minimum gap? (gap applies to non-critical spoken times;
        // the critical line set last_spoken, so High still waits)
        assert!(speaks(&e.tick(11_000, &s, &CFG)).is_empty());
        assert_eq!(speaks(&e.tick(30_000, &s, &CFG)), vec!["h1"]);
        assert!(speaks(&e.tick(31_000, &s, &CFG)).is_empty());
        assert_eq!(speaks(&e.tick(51_000, &s, &CFG)), vec!["h2"]);
    }

    #[test]
    fn hourly_cap_turns_further_non_critical_alerts_into_cards_but_never_critical() {
        let mut e = Engine::new();
        let s = quiet();
        let mut t = 0u64;
        for i in 0..MAX_PER_HOUR {
            let a = e.submit(alert(&format!("h{i}"), AlertUrgency::High), t, &s, &CFG);
            assert_eq!(speaks(&a).len(), 1, "alert {i} should speak");
            t += MIN_GAP_MS + 1_000;
        }
        let a = e.submit(alert("over", AlertUrgency::High), t, &s, &CFG);
        assert_eq!(cards(&a), vec![("over".to_string(), "rate_limited")]);
        let a = e.submit(alert("crit", AlertUrgency::Critical), t, &s, &CFG);
        assert_eq!(speaks(&a), vec!["crit"], "critical bypasses rate limits");
        // an hour later the window has cleared
        let a = e.submit(alert("later", AlertUrgency::High), t + HOUR_MS + 1, &s, &CFG);
        assert_eq!(speaks(&a), vec!["later"]);
    }

    #[test]
    fn snooze_holds_non_critical_but_not_critical() {
        let mut e = Engine::new();
        let s = quiet();
        e.snooze(0, 15);
        assert!(e.submit(alert("h", AlertUrgency::High), 1_000, &s, &CFG).is_empty());
        assert_eq!(speaks(&e.submit(alert("c", AlertUrgency::Critical), 2_000, &s, &CFG)), vec!["c"]);
        assert!(e.tick(14 * 60_000, &s, &CFG).is_empty(), "still snoozed");
        assert_eq!(speaks(&e.tick(16 * 60_000, &s, &CFG)), vec!["h"]);
    }

    #[test]
    fn duplicate_ids_are_handled_once() {
        let mut e = Engine::new();
        let a = e.submit(alert("same", AlertUrgency::High), 0, &quiet(), &CFG);
        assert_eq!(speaks(&a).len(), 1);
        assert!(e.submit(alert("same", AlertUrgency::High), 60_000, &quiet(), &CFG).is_empty());
        assert_eq!(e.pending_len(), 0);
    }

    #[test]
    fn evaluate_is_total_over_signal_combinations() {
        // no combination may panic or leave Low voiced / High voiced over the user
        let bools = [false, true];
        for &u in &[AlertUrgency::Low, AlertUrgency::Medium, AlertUrgency::High, AlertUrgency::Critical] {
            for &us in &bools {
                for &ns in &bools {
                    for &w in &bools {
                        for &m in &bools {
                            for waited in [0u64, 9_000, 61_000, 700_000, 2_000_000] {
                                let s = Signals { user_speaking: us, nexus_speaking: ns, working: w, meeting: m, idle_ms: 100_000 };
                                let v = evaluate(u, &s, &CFG, waited, waited);
                                if u == AlertUrgency::Low {
                                    assert!(matches!(v, Verdict::Card(_)));
                                }
                                if matches!(u, AlertUrgency::High | AlertUrgency::Medium) && us {
                                    assert!(!matches!(v, Verdict::Speak { .. }), "{u:?} spoke over the user");
                                }
                                if matches!(u, AlertUrgency::High | AlertUrgency::Medium) && m {
                                    assert!(!matches!(v, Verdict::Speak { .. }), "{u:?} spoke in a meeting");
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
