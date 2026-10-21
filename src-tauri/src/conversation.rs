//! Coordinated conversation memory.
//!
//! The UI transcript is display state, `episodes.jsonl` is a raw rolling log,
//! and cloud prompts need a compact, relevant subset. This module is the
//! single coordination point between those representations:
//!
//! - one structured record per completed, failed, or clarification turn;
//! - deterministic relevance scoring with no embedding model;
//! - a rolling live window plus a compact durable brief;
//! - PII redaction before anything is formatted for a model prompt;
//! - provenance-aware admission: rejected/non-owner audio is never recorded.
//!
//! Raw episode logging remains in `memory.rs`; this module decides which
//! turns are conversationally durable and how they are compressed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::pii_filter;
use crate::voice_profile::TurnOwnership;

pub(crate) const CONVERSATION_FILE: &str = "conversation.jsonl";
pub(crate) const CONVERSATION_BRIEF_FILE: &str = "conversation_brief.json";
const MAX_RAW_TURNS: usize = 24;
const MAX_LIVE_TURNS: usize = 6;
const MAX_LIVE_CHARS: usize = 1_500;
const MAX_BRIEF_CHARS: usize = 400;
const COMPACT_RAW_CHARS: usize = 3_000;
/// Turns newer than this are always part of the live thread, whatever their relevance score.
const RECENT_SECS: i64 = 15 * 60;

static TURN_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationOutcome {
    Completed,
    Clarification,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurnInput {
    pub agent: String,
    pub intent: String,
    pub transcript: String,
    pub response: String,
    pub outcome: ConversationOutcome,
    pub unresolved: Vec<String>,
    pub owner: TurnOwnership,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub id: String,
    pub ts: i64,
    pub agent: String,
    pub intent: String,
    pub transcript: String,
    pub response: String,
    pub outcome: ConversationOutcome,
    #[serde(default)]
    pub unresolved: Vec<String>,
    pub owner: TurnOwnership,
    pub relevance: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConversationBrief {
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub active_task: String,
    #[serde(default)]
    pub unresolved: Vec<String>,
    #[serde(default)]
    pub key_points: Vec<String>,
    #[serde(default)]
    pub last_outcome: String,
    #[serde(default)]
    pub source_turns: Vec<String>,
}

fn conversation_path(dir: &Path) -> PathBuf {
    dir.join(CONVERSATION_FILE)
}

fn brief_path(dir: &Path) -> PathBuf {
    dir.join(CONVERSATION_BRIEF_FILE)
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn next_turn_id(ts: i64) -> String {
    let sequence = TURN_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("t-{ts}-{sequence}")
}

/// Deterministic relevance score used for retention, retrieval, and prompt
/// construction. Failed turns, clarifications, unresolved work, explicit
/// remembers, and action-bearing turns outrank routine acknowledgments.
pub fn turn_relevance(turn: &ConversationTurn) -> u8 {
    if turn.owner == TurnOwnership::Rejected {
        return 0;
    }
    if turn.outcome != ConversationOutcome::Completed || !turn.unresolved.is_empty() {
        return 100;
    }
    // Counsel turns (F1b): personal situations + verdicts are follow-up
    // anchors ("about what I told you yesterday") — retain precisely.
    if turn.intent == "share_concern" {
        return 95;
    }
    let text = format!("{} {}", turn.transcript, turn.response).to_lowercase();
    if text.contains("remember") {
        return 90;
    }
    if turn.intent == "unknown" {
        return 30;
    }
    if turn.intent == "greeting" {
        return 25;
    }
    if turn.transcript.chars().count() < 12 && turn.response.chars().count() < 40 {
        return 20;
    }
    70
}

/// Record one completed, failed, cancelled, or clarification turn.
/// Returns the turn ID, or `None` for rejected/non-owner audio.
pub fn record_conversation_turn(dir: &Path, input: ConversationTurnInput) -> Option<String> {
    if input.owner == TurnOwnership::Rejected {
        return None;
    }

    let ts = chrono::Utc::now().timestamp();
    let mut turn = ConversationTurn {
        id: next_turn_id(ts),
        ts,
        agent: input.agent,
        intent: input.intent,
        transcript: truncate_text(&input.transcript, 300),
        response: truncate_text(&input.response, 300),
        outcome: input.outcome,
        unresolved: input
            .unresolved
            .into_iter()
            .map(|slot| truncate_text(slot.trim(), 80))
            .filter(|slot| !slot.is_empty())
            .collect(),
        owner: input.owner,
        relevance: 0,
    };
    turn.relevance = turn_relevance(&turn);

    let mut turns = read_turns(dir);
    turns.push(turn.clone());
    let previous = read_brief(dir);
    let (live, brief) = compact_thread(&turns, previous.as_ref());
    write_turns(dir, &live);
    if let Some(brief) = brief {
        write_brief(dir, &brief);
    }
    Some(turn.id)
}

// ─── Local turns (doc 124 Q3) ────────────────────────────────────────
//
// Only cloud turns used to be recorded, so "open Chrome", "start hand mode" or a shutdown never entered
// the thread and "do it" / "what did I just ask" had nothing to read. A turn handled locally is armed
// when it is accepted and recorded with the first line NEXUS speaks for it.

/// A turn handled on-device that is waiting for the line NEXUS speaks for it.
#[derive(Debug, Clone)]
pub struct LocalTurn {
    pub transcript: String,
    pub intent: String,
    pub owner: TurnOwnership,
}

static LOCAL_TURN: std::sync::Mutex<Option<LocalTurn>> = std::sync::Mutex::new(None);

/// A new locally handled turn was accepted; replaces any turn that never got a spoken reply.
pub fn begin_local_turn(transcript: &str, intent: &str, owner: TurnOwnership) {
    if owner == TurnOwnership::Rejected {
        return;
    }
    *LOCAL_TURN.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(LocalTurn { transcript: transcript.to_string(), intent: intent.to_string(), owner });
}

/// Take the waiting turn (once).
pub fn take_local_turn() -> Option<LocalTurn> {
    LOCAL_TURN.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Record a local turn with the line NEXUS answered. Quality-gated like cloud turns.
pub fn record_local_turn(dir: &Path, turn: LocalTurn, response: &str) -> Option<String> {
    if crate::memcore::hygiene::admit_conversation_turn(&turn.transcript, response, true).is_err() {
        return None;
    }
    record_conversation_turn(
        dir,
        ConversationTurnInput {
            agent: "local".to_string(),
            intent: turn.intent,
            transcript: turn.transcript,
            response: response.to_string(),
            outcome: ConversationOutcome::Completed,
            unresolved: Vec::new(),
            owner: turn.owner,
        },
    )
}

/// The most recent exchange no older than `within_secs`: (what the user said, what NEXUS answered).
pub fn last_exchange(dir: &Path, now: i64, within_secs: i64) -> Option<(String, String)> {
    read_turns(dir)
        .into_iter()
        .rev()
        .find(|t| now - t.ts <= within_secs && t.owner != TurnOwnership::Rejected && !t.response.trim().is_empty())
        .map(|t| (t.transcript, t.response))
}

/// A bare "do it" / "go ahead" that is not an answer to anything pending. Strict: the whole sentence.
pub fn is_bare_do_it(text: &str) -> bool {
    let t: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let t = t.strip_prefix("nexus ").unwrap_or(&t);
    matches!(
        t,
        "do it"
            | "do that"
            | "go ahead"
            | "go for it"
            | "do it now"
            | "yes do it"
            | "yes go ahead"
            | "please do it"
            | "please go ahead"
            | "yes please do it"
            | "okay do it"
            | "ok do it"
            | "do it please"
            | "go ahead please"
    )
}

/// What NEXUS says to a bare "do it" with nothing pending, grounded in what it really last said.
pub fn do_what_line(last: Option<(String, String)>) -> String {
    match last {
        Some((_, said)) => {
            let said = said.trim();
            let clipped: String = said.chars().take(90).collect();
            let clipped = if said.chars().count() > 90 { format!("{clipped}...") } else { clipped };
            format!("Do what, sir? Nothing is waiting for an answer. My last words were: {clipped}")
        }
        None => "Do what, sir? Nothing is waiting for an answer.".to_string(),
    }
}

/// One-time hygiene cleanup: drop turns that fail the admit gate and delete the (possibly
/// poisoned) brief, which is rebuilt from the surviving turns. Returns (turns removed, brief deleted).
pub fn purge_junk(dir: &Path) -> (usize, bool) {
    let turns = read_turns(dir);
    let before = turns.len();
    let kept: Vec<ConversationTurn> = turns
        .into_iter()
        .filter(|t| {
            crate::memcore::hygiene::admit_conversation_turn(
                &t.transcript,
                &t.response,
                t.outcome == ConversationOutcome::Completed,
            )
            .is_ok()
        })
        .collect();
    let removed = before - kept.len();
    if removed > 0 {
        write_turns(dir, &kept);
    }
    let brief_deleted = std::fs::remove_file(brief_path(dir)).is_ok();
    (removed, brief_deleted)
}

/// Requests NEXUS could not finish in the last 48 h (failed turns, or turns
/// still waiting on a missing slot), newest first, de-duplicated, at most 3.
/// The boot briefing reads these; they are the user's own words.
pub fn unfinished_requests(dir: &Path, now: i64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for turn in read_turns(dir).into_iter().rev() {
        if now - turn.ts > 48 * 3600 {
            continue;
        }
        let unfinished = turn.outcome == ConversationOutcome::Failed
            || (turn.outcome == ConversationOutcome::Clarification && !turn.unresolved.is_empty());
        if !unfinished || turn.owner == TurnOwnership::Rejected {
            continue;
        }
        let text = turn.transcript.trim().to_string();
        if text.is_empty() || out.iter().any(|t| t.eq_ignore_ascii_case(&text)) {
            continue;
        }
        out.push(text);
        if out.len() == 3 {
            break;
        }
    }
    out
}

fn read_turns(dir: &Path) -> Vec<ConversationTurn> {
    let Ok(content) = std::fs::read_to_string(conversation_path(dir)) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn write_turns(dir: &Path, turns: &[ConversationTurn]) {
    let _ = std::fs::create_dir_all(dir);
    let mut content = String::new();
    for turn in turns {
        if let Ok(line) = serde_json::to_string(turn) {
            content.push_str(&line);
            content.push('\n');
        }
    }
    crate::memory::atomic_write(&conversation_path(dir), content.as_bytes());
}

fn read_brief(dir: &Path) -> Option<ConversationBrief> {
    let content = std::fs::read_to_string(brief_path(dir)).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_brief(dir: &Path, brief: &ConversationBrief) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(content) = serde_json::to_string_pretty(brief) {
        crate::memory::atomic_write(&brief_path(dir), content.as_bytes());
    }
}

/// Newest-first selection of unresolved or otherwise high-signal turns,
/// returned in chronological order and capped for prompt injection.
fn live_thread(turns: &[ConversationTurn]) -> Vec<ConversationTurn> {
    let mut selected: BTreeSet<usize> = BTreeSet::new();
    for (index, turn) in turns.iter().enumerate().rev() {
        if selected.len() >= MAX_LIVE_TURNS {
            break;
        }
        if turn.outcome != ConversationOutcome::Completed
            || !turn.unresolved.is_empty()
            || turn.relevance >= 60
        {
            selected.insert(index);
        }
    }
    for index in (0..turns.len()).rev() {
        if selected.len() >= MAX_LIVE_TURNS {
            break;
        }
        selected.insert(index);
    }
    selected.into_iter().map(|index| turns[index].clone()).collect()
}

fn summarize_thread(turns: &[ConversationTurn]) -> ConversationBrief {
    let mut brief = ConversationBrief::default();
    if turns.is_empty() {
        return brief;
    }

    let meaningful: Vec<&ConversationTurn> = turns
        .iter()
        .filter(|turn| turn.relevance >= 40 || turn.outcome != ConversationOutcome::Completed)
        .collect();
    let source: Vec<&ConversationTurn> = if meaningful.is_empty() {
        turns.iter().collect()
    } else {
        meaningful
    };
    let first = source.first().copied().unwrap_or(&turns[0]);
    let active = source
        .iter()
        .rev()
        .find(|turn| turn.outcome != ConversationOutcome::Completed || !turn.unresolved.is_empty())
        .copied()
        .unwrap_or_else(|| source[source.len() - 1]);

    let mut unresolved: Vec<String> = Vec::new();
    for turn in &source {
        for slot in &turn.unresolved {
            if !unresolved.contains(slot) {
                unresolved.push(slot.clone());
            }
        }
    }

    let mut ranked: Vec<&ConversationTurn> = source.to_vec();
    ranked.sort_by(|a, b| {
        b.relevance
            .cmp(&a.relevance)
            .then_with(|| b.ts.cmp(&a.ts))
    });
    let mut key_points = Vec::new();
    for turn in ranked.iter().take(3) {
        let point = truncate_text(
            &format!("{} → {}", turn.transcript, turn.response),
            120,
        );
        if !point.trim().is_empty() && !key_points.contains(&point) {
            key_points.push(point);
        }
    }

    brief.goal = truncate_text(&first.transcript, 160);
    brief.active_task = truncate_text(&active.transcript, 160);
    brief.unresolved = unresolved.into_iter().take(8).collect();
    brief.key_points = key_points;
    brief.last_outcome = truncate_text(&active.response, 160);
    brief.source_turns = source.iter().take(12).map(|turn| turn.id.clone()).collect();
    brief
}

fn merge_briefs(previous: Option<&ConversationBrief>, next: &ConversationBrief) -> ConversationBrief {
    let mut merged = ConversationBrief {
        goal: next.goal.clone(),
        active_task: next.active_task.clone(),
        unresolved: Vec::new(),
        key_points: Vec::new(),
        last_outcome: next.last_outcome.clone(),
        source_turns: Vec::new(),
    };
    if let Some(previous) = previous {
        if merged.goal.trim().is_empty() {
            merged.goal = previous.goal.clone();
        }
        for slot in previous.unresolved.iter().chain(next.unresolved.iter()) {
            if !merged.unresolved.contains(slot) {
                merged.unresolved.push(slot.clone());
            }
        }
        for point in previous.key_points.iter().chain(next.key_points.iter()) {
            if !merged.key_points.contains(point) {
                merged.key_points.push(point.clone());
            }
        }
        for id in previous.source_turns.iter().chain(next.source_turns.iter()) {
            if !merged.source_turns.contains(id) {
                merged.source_turns.push(id.clone());
            }
        }
    } else {
        merged.unresolved = next.unresolved.clone();
        merged.key_points = next.key_points.clone();
        merged.source_turns = next.source_turns.clone();
    }
    merged.unresolved.truncate(8);
    merged.key_points.truncate(5);
    merged.source_turns.truncate(12);
    merged.goal = truncate_text(&merged.goal, MAX_BRIEF_CHARS);
    merged.active_task = truncate_text(&merged.active_task, MAX_BRIEF_CHARS);
    merged.last_outcome = truncate_text(&merged.last_outcome, MAX_BRIEF_CHARS);
    merged
}

/// Compact raw turns once either the count or character budget is exceeded.
/// The compacted raw window is retained; older material survives only in the
/// merged brief.
fn compact_thread(
    turns: &[ConversationTurn],
    previous: Option<&ConversationBrief>,
) -> (Vec<ConversationTurn>, Option<ConversationBrief>) {
    let chars: usize = turns
        .iter()
        .map(|turn| turn.transcript.len() + turn.response.len())
        .sum();
    if turns.len() <= MAX_RAW_TURNS && chars <= COMPACT_RAW_CHARS {
        return (turns.to_vec(), previous.cloned());
    }

    let cutoff = turns.len().saturating_sub(MAX_RAW_TURNS);
    let (archived, live) = turns.split_at(cutoff);
    // Local (on-device) turns never go into the durable brief: it is sent to the cloud.
    let cloud_archived: Vec<ConversationTurn> = archived.iter().filter(|t| t.agent != "local").cloned().collect();
    let brief = merge_briefs(previous, &summarize_thread(&cloud_archived));
    (live.to_vec(), Some(brief))
}

/// Extract unresolved follow-up slots from Worker dialog state. This keeps the
/// coordinator's pending-slot list aligned with the backend that asked the
/// follow-up question.
pub fn unresolved_from_dialog_state(dialog_state: Option<&serde_json::Value>) -> Vec<String> {
    let Some(state) = dialog_state else {
        return Vec::new();
    };
    if state
        .get("expects_followup")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
        == false
    {
        return Vec::new();
    }
    if let Some(missing) = state
        .pointer("/context/missing")
        .and_then(|value| value.as_array())
    {
        return missing
            .iter()
            .filter_map(|slot| slot.as_str())
            .map(|slot| slot.trim().to_string())
            .filter(|slot| !slot.is_empty())
            .take(8)
            .collect();
    }
    if let Some(intent) = state
        .get("pending_intent")
        .and_then(|value| value.as_str())
    {
        return vec![format!("followup:{intent}")];
    }
    vec!["followup".to_string()]
}

/// Build the bounded prompt block used for cloud reasoning: a durable brief
/// plus the highest-signal recent turns. PII is redacted before return.
pub fn conversation_prompt(dir: &Path) -> Option<String> {
    // On-device turns ("open chrome", a WhatsApp message) stay on the device: they never reach the cloud.
    let turns: Vec<ConversationTurn> = read_turns(dir).into_iter().filter(|t| t.agent != "local").collect();
    // A brief built from garbled turns ("I don't know. I don't know...") is worse than none.
    let brief = read_brief(dir).map(|mut b| {
        let ok = crate::memcore::hygiene::brief_line_ok;
        if !ok(&b.goal) {
            b.goal.clear();
        }
        if !ok(&b.active_task) {
            b.active_task.clear();
        }
        b.key_points.retain(|p| ok(p));
        b
    });
    // Low-signal completed turns (intent "unknown", relevance < 40) are the STT-noise tail; they
    // filled the prompt's free slots and were re-sent on every cloud turn.
    // The last RECENT_SECS always count (the user's next sentence may refer to them: "do it",
    // "the second one", "what did I just ask"); only OLDER low-signal turns are pruned.
    let now = chrono::Utc::now().timestamp();
    let live: Vec<ConversationTurn> = live_thread(&turns)
        .into_iter()
        .filter(|t| t.outcome != ConversationOutcome::Completed || t.relevance >= 40 || now - t.ts <= RECENT_SECS)
        .collect();
    if live.is_empty() && brief.as_ref().map_or(true, is_empty_brief) {
        return None;
    }

    let mut out = String::new();
    if let Some(brief) = brief.filter(|brief| !is_empty_brief(brief)) {
        out.push_str("Conversation brief:\n");
        if !brief.goal.trim().is_empty() {
            out.push_str(&format!("- Goal: {}\n", brief.goal.trim()));
        }
        if !brief.active_task.trim().is_empty() {
            out.push_str(&format!("- Active: {}\n", brief.active_task.trim()));
        }
        if !brief.unresolved.is_empty() {
            out.push_str(&format!("- Unresolved: {}\n", brief.unresolved.join(", ")));
        }
        for point in brief.key_points.iter().take(3) {
            out.push_str(&format!("- Earlier: {point}\n"));
        }
    }
    if !live.is_empty() {
        out.push_str("Recent:\n");
        for turn in live {
            out.push_str(&format!("User: {}\n", turn.transcript.trim()));
            out.push_str(&format!("NEXUS: {}\n", turn.response.trim()));
        }
    }

    let clean = pii_filter::sanitize(out.trim());
    if clean.is_empty() {
        return None;
    }
    Some(truncate_text(&clean, MAX_LIVE_CHARS))
}

fn is_empty_brief(brief: &ConversationBrief) -> bool {
    brief.goal.trim().is_empty()
        && brief.active_task.trim().is_empty()
        && brief.unresolved.is_empty()
        && brief.key_points.is_empty()
        && brief.last_outcome.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(
        transcript: &str,
        response: &str,
        outcome: ConversationOutcome,
        unresolved: &[&str],
        owner: TurnOwnership,
    ) -> ConversationTurn {
        ConversationTurn {
            id: format!("t-{transcript}"),
            ts: 1,
            agent: "worker".to_string(),
            intent: "unknown".to_string(),
            transcript: transcript.to_string(),
            response: response.to_string(),
            outcome,
            unresolved: unresolved.iter().map(|slot| slot.to_string()).collect(),
            owner,
            relevance: 0,
        }
    }

    fn tempdir(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "nexus_conversation_test_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_local_turn_is_recorded_once_with_the_first_spoken_line_and_feeds_last_exchange() {
        let dir = tempdir("local_turn");
        begin_local_turn("open chrome and search for rust", "open_app", TurnOwnership::Verified);
        let turn = take_local_turn().expect("armed");
        assert!(take_local_turn().is_none(), "a turn is recorded once");
        let id = record_local_turn(&dir, turn, "Opening Chrome, sir.");
        assert!(id.is_some());
        let now = chrono::Utc::now().timestamp();
        assert_eq!(
            last_exchange(&dir, now, 600),
            Some(("open chrome and search for rust".to_string(), "Opening Chrome, sir.".to_string()))
        );
        assert!(last_exchange(&dir, now + 3_600, 600).is_none(), "an hour later it is no longer 'just now'");
        // Rejected audio never arms anything.
        begin_local_turn("some tv line about weather", "unknown", TurnOwnership::Rejected);
        assert!(take_local_turn().is_none());
    }

    #[test]
    fn recent_low_signal_turns_stay_in_the_cloud_thread_but_old_ones_do_not() {
        let dir = tempdir("recent_thread");
        let mut recent = turn("open the pod bay doors please", "I'm afraid I can't do that, sir.", ConversationOutcome::Completed, &[], TurnOwnership::Verified);
        recent.ts = chrono::Utc::now().timestamp() - 60;
        recent.relevance = 20;
        let mut old = recent.clone();
        old.id = "t-old".into();
        old.transcript = "tell me about the weather yesterday morning".into();
        old.ts = chrono::Utc::now().timestamp() - 3 * 3600;
        write_turns(&dir, &[old, recent]);
        let prompt = conversation_prompt(&dir).expect("the recent turn is in the thread");
        assert!(prompt.contains("pod bay doors"), "{prompt}");
        assert!(!prompt.contains("weather yesterday"), "{prompt}");
    }

    #[test]
    fn on_device_turns_never_enter_the_cloud_prompt() {
        let dir = tempdir("local_not_cloud");
        begin_local_turn("send mummy a whatsapp saying the secret plan", "send_whatsapp_message", TurnOwnership::Verified);
        let t = take_local_turn().unwrap();
        record_local_turn(&dir, t, "Message to mummy is ready, sir.").unwrap();
        assert!(conversation_prompt(&dir).is_none(), "a purely local thread must not produce any cloud prompt");
    }

    #[test]
    fn bare_do_it_is_strict_and_the_reply_quotes_what_was_really_said() {
        for t in ["do it", "Do it.", "nexus, go ahead", "yes do it", "OK do it!"] {
            assert!(is_bare_do_it(t), "{t}");
        }
        for t in ["do it again tomorrow", "go ahead and open chrome", "what should I do", "yes", "open notepad"] {
            assert!(!is_bare_do_it(t), "{t}");
        }
        assert!(do_what_line(None).starts_with("Do what, sir?"));
        let line = do_what_line(Some(("q".into(), "Shall I summarise the report?".into())));
        assert!(line.contains("Shall I summarise the report?"), "{line}");
    }

    #[test]
    fn test_rejected_audio_is_never_recorded() {
        let dir = tempdir("rejected");
        let recorded = record_conversation_turn(
            &dir,
            ConversationTurnInput {
                agent: "worker".to_string(),
                intent: "unknown".to_string(),
                transcript: "television dialogue".to_string(),
                response: String::new(),
                outcome: ConversationOutcome::Failed,
                unresolved: Vec::new(),
                owner: TurnOwnership::Rejected,
            },
        );
        assert_eq!(recorded, None);
        assert!(read_turns(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_relevance_prefers_unresolved_and_explicit_memory() {
        let pending = turn("Which repo?", "", ConversationOutcome::Clarification, &["repo"], TurnOwnership::Verified);
        let remembered = turn("remember my dog is Bruno", "Remembered", ConversationOutcome::Completed, &[], TurnOwnership::Verified);
        let ack = turn("okay", "Ok sir.", ConversationOutcome::Completed, &[], TurnOwnership::Verified);
        assert_eq!(turn_relevance(&pending), 100);
        assert_eq!(turn_relevance(&remembered), 90);
        assert!(turn_relevance(&ack) < 60);
    }

    #[test]
    fn test_relevance_counsel_turns_stick() {
        // F1b: shared situations + verdicts are follow-up anchors.
        let mut counsel = turn(
            "was i right to shout at him",
            "No — apologise today.",
            ConversationOutcome::Completed,
            &[],
            TurnOwnership::Verified,
        );
        counsel.intent = "share_concern".to_string();
        assert_eq!(turn_relevance(&counsel), 95);
    }

    #[test]
    fn test_live_window_caps_long_threads_and_preserves_unresolved() {
        let mut turns = Vec::new();
        for index in 0..10 {
            turns.push(turn(
                &format!("routine status {index}"),
                "Ok sir.",
                ConversationOutcome::Completed,
                &[],
                TurnOwnership::Verified,
            ));
        }
        turns.push(turn(
            "analyse the outstanding repository request",
            "Which repository should I use, sir?",
            ConversationOutcome::Clarification,
            &["repo"],
            TurnOwnership::Verified,
        ));
        let live = live_thread(&turns);
        assert_eq!(live.len(), MAX_LIVE_TURNS);
        assert!(live.iter().any(|turn| turn.unresolved == vec!["repo".to_string()]));
    }

    #[test]
    fn test_compaction_produces_brief_and_bounded_history() {
        let mut turns = Vec::new();
        for index in 0..(MAX_RAW_TURNS + 4) {
            turns.push(turn(
                &format!("completed command {index} with substantial detail"),
                &format!("completed response {index} with substantial detail"),
                ConversationOutcome::Completed,
                &[],
                TurnOwnership::Verified,
            ));
        }
        let (live, brief) = compact_thread(&turns, None);
        assert_eq!(live.len(), MAX_RAW_TURNS);
        let brief = brief.expect("long thread must compact");
        assert!(!brief.goal.is_empty());
        assert!(brief.source_turns.len() <= 12);
    }

    #[test]
    fn test_unresolved_followup_slots_are_preserved() {
        let state = serde_json::json!({
            "expects_followup": true,
            "pending_intent": "fast_analyse",
            "context": {"missing": ["repo"]}
        });
        assert_eq!(
            unresolved_from_dialog_state(Some(&state)),
            vec!["repo".to_string()]
        );
        assert!(unresolved_from_dialog_state(None).is_empty());
    }

    #[test]
    fn test_prompt_redacts_pii_and_enforces_budget() {
        let dir = tempdir("prompt");
        for index in 0..8 {
            record_conversation_turn(
                &dir,
                ConversationTurnInput {
                    agent: "worker".to_string(),
                    // A real intent: "unknown" turns are low-signal and deliberately kept out of the prompt.
                    intent: "search".to_string(),
                    transcript: format!("owner question {index} from jane.doe@example.com"),
                    response: format!("owner answer {index}"),
                    outcome: ConversationOutcome::Completed,
                    unresolved: Vec::new(),
                    owner: TurnOwnership::Verified,
                },
            )
            .expect("record turn");
        }
        let prompt = conversation_prompt(&dir).expect("prompt");
        assert!(!prompt.contains("jane.doe@example.com"));
        assert!(prompt.contains("[REDACTED:EMAIL]"));
        assert!(prompt.len() <= MAX_LIVE_CHARS);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_unfinished_requests_are_recent_deduped_and_owner_checked() {
        let dir = tempdir("unfinished");
        let now = chrono::Utc::now().timestamp();
        let mk = |t: &str, outcome, unresolved: &[&str]| ConversationTurnInput {
            agent: "worker".into(),
            intent: "unknown".into(),
            transcript: t.into(),
            response: "r".into(),
            outcome,
            unresolved: unresolved.iter().map(|s| s.to_string()).collect(),
            owner: TurnOwnership::Verified,
        };
        record_conversation_turn(&dir, mk("send the report to Asha", ConversationOutcome::Failed, &[]));
        record_conversation_turn(&dir, mk("open chrome", ConversationOutcome::Completed, &[]));
        record_conversation_turn(&dir, mk("Send the report to asha", ConversationOutcome::Failed, &[]));
        record_conversation_turn(&dir, mk("message mom", ConversationOutcome::Clarification, &["message"]));
        record_conversation_turn(&dir, mk("what time is it", ConversationOutcome::Clarification, &[]));
        let got = unfinished_requests(&dir, now + 5);
        assert_eq!(got, vec!["message mom".to_string(), "Send the report to asha".to_string()]);
        // Older than 48 h → gone.
        assert!(unfinished_requests(&dir, now + 49 * 3600).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
