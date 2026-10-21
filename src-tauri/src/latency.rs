//! Per-turn latency clock (docs/features/134 P0): where do the ~2.6 s between "the user stopped talking"
//! and "NEXUS makes a sound" go? One `[LAT]` line per spoken turn:
//! `[LAT] stt 322ms | parse 289ms | answer 614ms | tts 1278ms | total 2503ms`.
//!
//! Hooks (all one-liners): capture stop -> `begin()`, transcript ready -> `mark(Transcript)`, intent known ->
//! `mark(Parsed)`, a Result event leaves the orchestrator -> `mark(Answer)`, the first audio is queued for
//! playback -> `first_audio()`. The audio callback calls `begin()`, so nothing here may block: every access
//! uses `try_lock` and silently skips a sample instead of waiting.

use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Transcript,
    Parsed,
    Answer,
}

#[derive(Default, Clone, Debug)]
pub(crate) struct Turn {
    pub speech_end: Option<Instant>,
    pub transcript: Option<Instant>,
    pub parsed: Option<Instant>,
    pub answer: Option<Instant>,
}

static TURN: Mutex<Option<Turn>> = Mutex::new(None);

/// A turn older than this when the first audio arrives is not a reply to it (proactive speech, a stale mark).
const MAX_TURN_AGE: Duration = Duration::from_secs(60);

/// The user stopped speaking (capture ended).
pub fn begin() {
    if let Ok(mut g) = TURN.try_lock() {
        *g = Some(Turn { speech_end: Some(Instant::now()), ..Default::default() });
    }
}

/// Record the first time `stage` is reached in the current turn.
pub fn mark(stage: Stage) {
    if let Ok(mut g) = TURN.try_lock() {
        if let Some(t) = g.as_mut() {
            let slot = match stage {
                Stage::Transcript => &mut t.transcript,
                Stage::Parsed => &mut t.parsed,
                Stage::Answer => &mut t.answer,
            };
            if slot.is_none() {
                *slot = Some(Instant::now());
            }
        }
    }
}

/// The first audio of the reply is being played: print the line and close the turn (later chunks are no-ops).
pub fn first_audio() {
    let taken = TURN.try_lock().ok().and_then(|mut g| g.take());
    if let Some(t) = taken {
        if let Some(line) = format_line(&t, Instant::now()) {
            println!("{line}"); // a leading [LAT] tag is whitelisted in scripts/run.ps1
        }
    }
}

fn ms(a: Instant, b: Instant) -> u128 {
    b.saturating_duration_since(a).as_millis()
}

/// Build the `[LAT]` line. Each stage shows the time since the previous stage that was reached; stages that
/// never happened (a local command has no cloud answer) are skipped. `None` for a stale or empty turn.
pub(crate) fn format_line(t: &Turn, now: Instant) -> Option<String> {
    let start = t.speech_end?;
    if now.saturating_duration_since(start) > MAX_TURN_AGE {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    let mut prev = start;
    for (label, at) in [("stt", t.transcript), ("parse", t.parsed), ("answer", t.answer)] {
        if let Some(at) = at {
            parts.push(format!("{label} {}ms", ms(prev, at)));
            prev = at;
        }
    }
    parts.push(format!("tts {}ms", ms(prev, now)));
    parts.push(format!("total {}ms", ms(start, now)));
    Some(format!("[LAT] {}", parts.join(" | ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, millis: u64) -> Instant {
        base + Duration::from_millis(millis)
    }

    #[test]
    fn a_cloud_turn_shows_every_stage_as_a_delta_and_the_total() {
        let b = Instant::now();
        let t = Turn {
            speech_end: Some(b),
            transcript: Some(at(b, 322)),
            parsed: Some(at(b, 611)),
            answer: Some(at(b, 1225)),
        };
        let line = format_line(&t, at(b, 2503)).unwrap();
        assert_eq!(line, "[LAT] stt 322ms | parse 289ms | answer 614ms | tts 1278ms | total 2503ms");
    }

    #[test]
    fn a_local_command_without_a_cloud_answer_skips_that_stage() {
        let b = Instant::now();
        let t = Turn { speech_end: Some(b), transcript: Some(at(b, 300)), parsed: Some(at(b, 340)), answer: None };
        let line = format_line(&t, at(b, 380)).unwrap();
        assert_eq!(line, "[LAT] stt 300ms | parse 40ms | tts 40ms | total 380ms");
    }

    #[test]
    fn a_stale_or_empty_turn_prints_nothing() {
        let b = Instant::now();
        assert!(format_line(&Turn::default(), at(b, 10)).is_none());
        let t = Turn { speech_end: Some(b), ..Default::default() };
        assert!(format_line(&t, at(b, 61_000)).is_none(), "proactive speech long after the last turn is not a reply");
    }
}
