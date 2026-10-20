//! Last-heard cache + corrected-command resubmit for the sidebar row.
//!
//! The sidebar's editable "last heard" row needs two things from Rust:
//! - the most recent final transcript, even if the sidebar window was
//!   created long after the words were spoken (`last_stt_transcript`);
//! - a way to run a corrected line as a normal turn (`stt_resubmit_transcript`).
//!
//! The resubmit deliberately does NOT execute anything here. It emits the
//! same `stt:transcript` event the wake pipeline emits, so the stage runs
//! the turn with its own ordering (`runInTurnOrder`) and ghost state — a
//! sidebar-local `processTranscript` call would run in the wrong WebView,
//! with an empty assistant store and no stage ordering.
//!
//! This module is always compiled (unlike `wakeword_oww`, which is feature
//! gated), and it never touches `stage.rs`'s destructive pending-transcript
//! stash: it only calls the shared `stash_transcript_if_not_ready` so a
//! stage that is still loading replays the turn on mount.

use tauri::{AppHandle, Emitter};

/// The most recent final transcript, kept for the sidebar's editable "last
/// heard" row. Unlike the stage's pending transcript this is NOT consumed:
/// the sidebar window is created on demand and may mount long after the
/// words were spoken, and re-opening it must still show the last command.
static LAST_TRANSCRIPT: once_cell::sync::Lazy<parking_lot::Mutex<Option<String>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

/// Longest transcript kept for the correction row. Real STT output is short;
/// this only guards against a runaway hallucination holding memory.
const LAST_TRANSCRIPT_MAX_CHARS: usize = 2000;

/// Pure: the text worth caching for the correction row, or `None` when the
/// transcript carries nothing a human could edit (empty/whitespace).
fn transcript_for_cache(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let total = trimmed.chars().count();
    if total <= LAST_TRANSCRIPT_MAX_CHARS {
        return Some(trimmed.to_string());
    }
    // Keep the tail: the end of an utterance is the actionable part.
    let tail: String = trimmed
        .chars()
        .skip(total - LAST_TRANSCRIPT_MAX_CHARS)
        .collect();
    Some(format!("…{tail}"))
}

/// Pure: validate a corrected command before it becomes a turn. Empty input
/// is a miss, not a command; an over-limit line is a runaway/hallucination,
/// not something the user meant to send.
fn resubmit_text(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("nothing to resubmit".to_string());
    }
    if trimmed.chars().count() > LAST_TRANSCRIPT_MAX_CHARS {
        return Err(format!(
            "transcript too long (max {LAST_TRANSCRIPT_MAX_CHARS} characters)"
        ));
    }
    Ok(trimmed.to_string())
}

/// Remember the final transcript of the last turn for the sidebar's editable
/// "last heard" row. Empty transcripts (a silent miss) never clear it.
pub fn remember_transcript_text(text: &str) {
    if let Some(line) = transcript_for_cache(text) {
        *LAST_TRANSCRIPT.lock() = Some(line);
    }
}

/// IPC: the sidebar's last-heard row seeds itself from this on mount. Returns
/// `None` until something has actually been heard, and never clears the cache.
#[tauri::command]
pub fn last_stt_transcript() -> Option<String> {
    LAST_TRANSCRIPT.lock().clone()
}

/// IPC: run a corrected command exactly like a spoken one. The stage owns the
/// turn (ordering, ghost state, TTS/captions); this only validates the line,
/// remembers it for the row, stashes it for a loading stage, and broadcasts
/// the standard `stt:transcript` payload.
#[tauri::command]
pub fn stt_resubmit_transcript(app: AppHandle, text: String) -> Result<(), String> {
    let line = resubmit_text(&text)?;
    remember_transcript_text(&line);

    // Same shape the wake pipeline emits; the stage fills the optional
    // fields' defaults (owner score/bias/language) when they are absent.
    let payload = serde_json::json!({
        "text": line,
        "ownership": "unenrolled",
    });
    crate::stage::stash_transcript_if_not_ready(payload.clone());
    app.emit("stt:transcript", payload)
        .map_err(|e| format!("emit stt:transcript: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resubmit_text_trims_surrounding_whitespace() {
        assert_eq!(
            resubmit_text("  open chrome  \n"),
            Ok("open chrome".to_string())
        );
    }

    #[test]
    fn resubmit_text_rejects_empty_input() {
        assert!(resubmit_text("").is_err());
    }

    #[test]
    fn resubmit_text_rejects_whitespace_only_input() {
        assert!(resubmit_text("  \n\t ").is_err());
    }

    #[test]
    fn resubmit_text_rejects_more_than_the_limit_and_accepts_the_limit() {
        let too_long = "a".repeat(LAST_TRANSCRIPT_MAX_CHARS + 1);
        assert!(resubmit_text(&too_long).is_err());

        let at_limit = "a".repeat(LAST_TRANSCRIPT_MAX_CHARS);
        let accepted = resubmit_text(&at_limit).expect("the limit itself is allowed");
        assert_eq!(accepted, at_limit);
    }

    // One test: LAST_TRANSCRIPT is process-global and must not race with
    // another test touching the same cache (same reason the stage's
    // heartbeat/stash assertions share a single test).
    #[test]
    fn transcript_cache_ignores_empty_keeps_the_tail_and_is_not_consumed() {
        assert_eq!(transcript_for_cache(""), None);
        assert_eq!(transcript_for_cache("  \n\t "), None);
        assert_eq!(
            transcript_for_cache("  open chrome  "),
            Some("open chrome".into())
        );

        let long = format!(
            "{}SEND-IT-TO-MOM",
            "a".repeat(LAST_TRANSCRIPT_MAX_CHARS * 2)
        );
        let kept = transcript_for_cache(&long).expect("long line still cached");
        assert!(kept.starts_with('…'));
        assert!(kept.ends_with("SEND-IT-TO-MOM"));
        assert_eq!(kept.chars().count(), LAST_TRANSCRIPT_MAX_CHARS + 1);

        remember_transcript_text("open chrome");
        assert_eq!(last_stt_transcript().as_deref(), Some("open chrome"));
        remember_transcript_text("   ");
        assert_eq!(
            last_stt_transcript().as_deref(),
            Some("open chrome"),
            "a silent miss must not erase the last real command"
        );
        assert_eq!(
            last_stt_transcript().as_deref(),
            Some("open chrome"),
            "reading the cache is non-destructive"
        );

        remember_transcript_text("open youtube");
        assert_eq!(last_stt_transcript().as_deref(), Some("open youtube"));
    }
}
