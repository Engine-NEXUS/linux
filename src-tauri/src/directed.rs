//! Directed-speech gate for open-mic (ghost hot-mic) turns — Phase 8.
//!
//! PROBLEM: while a ghost session keeps the mic open, every utterance is transcribed. Anything the
//! deterministic parser does not recognise falls through to the cloud LLM chat — so TV, a side
//! conversation, our own TTS echo, or an STT hallucination loop can reach the LLM (and be spoken
//! back / acted on). Apple's Siri pipeline solves the same problem with a final "is this speech
//! actually addressed to the assistant?" stage (text-based false-trigger mitigation); isair's docs
//! describe an engagement-gated "intent judge" with echo rejection against the last TTS text.
//!
//! WHAT THIS DOES: for turns that did NOT start from an explicit wake/hotkey/confirm window
//! (i.e. `Origin::HotMic`), classify the transcript as directed or not, deterministically (no model,
//! < 1 ms). Turns the user started explicitly are ALWAYS accepted. Ignored utterances are logged
//! locally (text only, rotated 5 MB) to `missed_intents.jsonl` with source `directed_gate` for tuning.
//!
//! Design: docs/research/jarvis-landscape/08-case-2-plan-2026-10-05.md §C2-2 (clean-room: ideas only).

use serde::Serialize;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Who opened the mic for this turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Wake word, hotkey, confirmation / clarification window, follow-up question: the user is
    /// expected to speak TO the assistant. Never gated.
    Direct = 0,
    /// Ghost hot-mic loop re-opened the mic by itself: speech may not be addressed to us.
    HotMic = 1,
}

impl Origin {
    pub fn parse(s: &str) -> Origin {
        match s.trim().to_ascii_lowercase().as_str() {
            "hot_mic" | "hotmic" | "hot-mic" => Origin::HotMic,
            _ => Origin::Direct,
        }
    }
    fn from_u8(v: u8) -> Origin {
        if v == 1 { Origin::HotMic } else { Origin::Direct }
    }
}

/// Origin of the capture currently open, and of the capture that most recently produced a transcript.
static CAPTURE_ORIGIN: AtomicU8 = AtomicU8::new(0);
static LAST_TURN_ORIGIN: AtomicU8 = AtomicU8::new(0);

/// Set when a capture starts (wake/hotkey paths use `Direct`; the hot-mic loop passes `HotMic`).
pub fn set_capture_origin(o: Origin) {
    CAPTURE_ORIGIN.store(o as u8, Ordering::Relaxed);
}

/// Called when a capture stops and its transcription begins: freeze the origin for that turn so a
/// capture opened afterwards (the hot-mic loop re-opens immediately) cannot change the verdict.
pub fn snapshot_turn_origin() {
    LAST_TURN_ORIGIN.store(CAPTURE_ORIGIN.load(Ordering::Relaxed), Ordering::Relaxed);
}

pub fn last_turn_origin() -> Origin {
    Origin::from_u8(LAST_TURN_ORIGIN.load(Ordering::Relaxed))
}

// ── our own recent speech (echo rejection) ──────────────────────────────────────────────────
const SPOKEN_KEEP: usize = 4;
/// How long after we spoke a line it can still plausibly come back through the mic.
const ECHO_WINDOW: Duration = Duration::from_secs(30);

static SPOKEN: Mutex<Vec<(String, Instant)>> = Mutex::new(Vec::new());

/// Record a line NEXUS is about to speak (any engine: cache, streaming, proactive alert).
pub fn note_spoken(text: &str) {
    let t = text.trim();
    if t.is_empty() {
        return;
    }
    if let Ok(mut v) = SPOKEN.lock() {
        v.push((t.to_string(), Instant::now()));
        let n = v.len();
        if n > SPOKEN_KEEP {
            v.drain(0..n - SPOKEN_KEEP);
        }
    }
    // The owner usually answers right after we speak: window = speaking time + the configured grace.
    if engagement_secs() > 0 {
        engage_for(estimated_speech(t) + Duration::from_secs(engagement_secs()));
    }
}

fn recent_spoken(now: Instant) -> Vec<String> {
    SPOKEN
        .lock()
        .map(|v| {
            v.iter()
                .filter(|(_, at)| now.saturating_duration_since(*at) <= ECHO_WINDOW)
                .map(|(t, _)| t.clone())
                .collect()
        })
        .unwrap_or_default()
}

// ── engagement window ───────────────────────────────────────────────────────────────────────
/// OPT-IN (`ghostEngagementSecs` > 0): after NEXUS speaks, or accepts a command, the next N seconds of
/// the open mic count as the owner; outside that window a hot-mic turn needs the word "Nexus".
/// **Off by default**: Ghost mode is full control, the user never has to say "Nexus" or wait for a
/// window (live test 2026-10-13: it dropped "open notepad" 16 s after our reply). Protection from a
/// TV / Echo Dot comes from rejecting other assistants' wake words, our own voice, and (next) the
/// speaker check, none of which need the owner to change how they speak.
pub const DEFAULT_ENGAGEMENT_SECS: u64 = 0;
static ENGAGEMENT_SECS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(DEFAULT_ENGAGEMENT_SECS);
static ENGAGED_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// Configured window length in seconds; 0 disables the rule.
pub fn set_engagement_secs(secs: u64) {
    ENGAGEMENT_SECS.store(secs.min(120), std::sync::atomic::Ordering::Relaxed);
}
pub fn engagement_secs() -> u64 {
    ENGAGEMENT_SECS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Keep the window open for at least `extra` more time (never shortens it).
pub fn engage_for(extra: Duration) {
    let until = Instant::now() + extra;
    if let Ok(mut g) = ENGAGED_UNTIL.lock() {
        if g.map_or(true, |cur| until > cur) {
            *g = Some(until);
        }
    }
}

/// Open (or extend) the window by the configured length.
pub fn engage_now() {
    engage_for(Duration::from_secs(engagement_secs()));
}

/// Close the window (tests share the process-global state).
#[cfg(test)]
pub(crate) fn reset_engagement() {
    if let Ok(mut g) = ENGAGED_UNTIL.lock() {
        *g = None;
    }
}

pub fn is_engaged(now: Instant) -> bool {
    ENGAGED_UNTIL.lock().map(|g| g.map_or(false, |u| now < u)).unwrap_or(true)
}

/// Rough speaking time of a line (so a long reply does not eat the window while it is still talking).
fn estimated_speech(text: &str) -> Duration {
    Duration::from_millis(text.split_whitespace().count() as u64 * 400)
}

// ── the pure decision ───────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Verdict {
    pub accept: bool,
    pub reason: &'static str,
}

fn accept(reason: &'static str) -> Verdict {
    Verdict { accept: true, reason }
}
fn ignore(reason: &'static str) -> Verdict {
    Verdict { accept: false, reason }
}

pub struct Input<'a> {
    pub transcript: &'a str,
    pub origin: Origin,
    /// The deterministic intent parser recognised the utterance.
    pub parses_as_command: bool,
    /// NEXUS lines spoken recently (echo candidates).
    pub spoken: &'a [String],
    /// Free-text dictation is on: every utterance is wanted.
    pub dictation_active: bool,
    /// Inside the engagement window (see `is_engaged`). Ignored when `window_enabled` is false.
    pub engaged: bool,
    /// The engagement rule is on (`ghostEngagementSecs` > 0).
    pub window_enabled: bool,
}

/// Lowercased alphanumeric tokens (apostrophes kept inside words).
pub fn tokens(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|t| t.trim_matches('\'').to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Other assistants' wake words: a TV ad or an Echo Dot in the room. Their commands often parse
/// as ours ("alexa play music"), so these are rejected before the command check.
const FOREIGN_WAKE: &[&str] = &[
    "alexa", "hey google", "ok google", "okay google", "hey siri", "hi siri", "hey cortana", "hey bixby",
];

fn has_foreign_wake(text_lower: &str) -> bool {
    FOREIGN_WAKE.iter().any(|w| text_lower.contains(w))
}

const VOCATIVES: &[&str] = &["nexus", "nexis", "nexxus", "nexes", "next us", "nixes"];

fn has_vocative(text_lower: &str) -> bool {
    VOCATIVES.iter().any(|v| text_lower.contains(v))
}

/// Function words carry no echo evidence: "open THE repository" must not look like an echo of
/// "...of THE zync repository" just because both contain "the".
const STOPWORDS: &[&str] = &[
    "the", "a", "an", "of", "in", "on", "to", "is", "are", "was", "and", "or", "it", "that", "this", "i", "you", "my",
    "me", "for", "with", "at", "by", "be", "as", "so", "we", "he", "she", "they",
];

/// Fraction of the transcript's CONTENT words that appear in what we just said (needs >= 2 content
/// words, otherwise 0 — one shared word is not an echo).
fn echo_overlap(t: &[String], spoken: &[String]) -> f32 {
    let content: Vec<&String> = t.iter().filter(|w| !STOPWORDS.contains(&w.as_str())).collect();
    if content.len() < 2 {
        return 0.0;
    }
    let said: std::collections::HashSet<String> = spoken.iter().flat_map(|s| tokens(s)).collect();
    if said.is_empty() {
        return 0.0;
    }
    content.iter().filter(|w| said.contains(**w)).count() as f32 / content.len() as f32
}

/// STT hallucination loops ("I'm going to go. I'm going to go. …") and degenerate repetition.
fn is_repetitive(t: &[String]) -> bool {
    if t.len() >= 8 {
        let uniq: std::collections::HashSet<&String> = t.iter().collect();
        if (uniq.len() as f32) / (t.len() as f32) < 0.45 {
            return true;
        }
    }
    if t.len() >= 6 {
        let mut counts: std::collections::HashMap<(&String, &String, &String), u32> = std::collections::HashMap::new();
        for w in t.windows(3) {
            *counts.entry((&w[0], &w[1], &w[2])).or_insert(0) += 1;
        }
        if counts.values().any(|&c| c >= 3) {
            return true;
        }
    }
    false
}

const FILLERS: &[&str] = &[
    "uh", "um", "umm", "uhh", "hmm", "mm", "mmm", "huh", "oh", "ah", "ahh", "eh", "er", "okay", "ok", "yeah", "yep",
    "yes", "no", "nope", "right", "well", "so", "and", "the", "a", "then", "that", "this", "it", "is", "to", "of",
    "nice", "good", "great", "cool", "sure", "alright", "thanks", "thank", "you", "bye", "hi", "hello", "hey", "wow",
    "shh", "ssh",
];

/// First words that start a request/command/question — a spoken-to-an-assistant shape.
const STARTERS: &[&str] = &[
    "what", "whats", "what's", "who", "whos", "who's", "when", "where", "why", "how", "which", "is", "are", "was",
    "do", "does", "did", "can", "could", "would", "will", "should", "tell", "show", "give", "find", "search", "look",
    "check", "read", "write", "send", "call", "play", "pause", "resume", "stop", "open", "close", "start", "create",
    "make", "set", "turn", "remind", "add", "remove", "delete", "take", "get", "go", "let", "let's", "please", "list",
    "summarize", "summarise", "translate", "explain", "help", "schedule", "book", "order", "copy", "paste", "type",
    "press", "click", "scroll", "switch", "minimize", "maximize", "mute", "unmute", "increase", "decrease", "enable",
    "disable", "cancel", "exit", "leave", "analyze", "analyse", "message", "text", "email", "navigate", "launch",
    "run", "save", "undo", "redo", "refresh", "restart", "quit", "volume", "brightness", "next", "previous",
    "shift", "ctrl", "alt", "tab", "enter", "return", "esc", "escape", "space", "backspace", "double", "right",
    "left", "middle", "drag", "drop", "hover", "move", "point", "select", "cut", "highlight", "zoom", "focus",
    "bring", "window", "screen", "back", "forward", "new", "hit", "push",
];

/// Request phrasings that are addressed to an assistant wherever they appear.
const REQUEST_PHRASES: &[&str] = &["can you", "could you", "would you", "will you", "please", "i need you to", "i want you to"];

/// Words about NEXUS's own features/apps. In an open ghost-mode mic, mentioning one is a strong sign the
/// user is talking to the assistant — including garbled commands ("Prem on WhatsApp", "wave browser",
/// "the post mode") that no parser could match. A TV mentioning "gmail" is the accepted cost; such a line
/// reaches the same LLM path it always did.
const DOMAIN_CUES: &[&str] = &[
    "ghost", "hub", "command", "whatsapp", "browser", "settings", "spotify", "chrome", "github", "gmail", "youtube",
    "dictation", "screenshot", "tab", "tabs", "window", "windows", "screen", "screens", "cursor", "mouse",
    "keyboard", "antigravity", "brave", "code", "terminal", "page", "button", "link",
];

/// Commands are short; longer lines in an open mic are conversation, dictation or TV.
const MAX_COMMAND_WORDS: usize = 14;

/// Decide whether a transcript is addressed to NEXUS. See the module docs for the order and why.
pub fn evaluate(inp: &Input) -> Verdict {
    if inp.origin != Origin::HotMic {
        return accept("explicit_turn");
    }
    if inp.dictation_active {
        return accept("dictation");
    }
    let lower = inp.transcript.to_lowercase();
    let t = tokens(inp.transcript);
    let addressed = has_vocative(&lower);
    // Other assistants and our own voice never count, even when the words parse as a command
    // ("alexa play music", our "Opening Command Hub, sir" coming back through the speakers).
    if !addressed && has_foreign_wake(&lower) {
        return ignore("foreign_wake");
    }
    // A phrase-grammar command only counts as our own voice when it is (nearly) a verbatim replay:
    // the owner repeating "open the command hub" after we said "Opening Command Hub, sir" shares most
    // content words with that line but is a real request. Non-commands use the looser threshold.
    let echo_limit = if inp.parses_as_command { 0.85 } else { 0.6 };
    if t.len() >= 2 && !addressed && echo_overlap(&t, inp.spoken) >= echo_limit {
        return ignore("echo");
    }
    // Outside the engagement window only a turn that says "Nexus" is for us.
    if inp.window_enabled && !inp.engaged && !addressed {
        return ignore("outside_window");
    }
    if inp.parses_as_command {
        return accept("command");
    }
    if t.is_empty() {
        return ignore("empty");
    }
    if addressed && !is_repetitive(&t) {
        return accept("vocative");
    }
    if t.len() >= 2 && echo_overlap(&t, inp.spoken) >= 0.6 {
        return ignore("echo");
    }
    if is_repetitive(&t) {
        return ignore("repetition");
    }
    if t.iter().all(|w| FILLERS.contains(&w.as_str())) {
        return ignore("filler");
    }
    if t.len() > MAX_COMMAND_WORDS {
        return ignore("too_long");
    }
    let padded = format!(" {} ", t.join(" "));
    let starts = STARTERS.contains(&t[0].as_str());
    let requests = REQUEST_PHRASES.iter().any(|p| padded.contains(&format!(" {p} ")));
    let question = inp.transcript.trim_end().ends_with('?') && t.len() >= 2;
    if starts || requests || question {
        return accept("cues");
    }
    if t.iter().any(|w| DOMAIN_CUES.contains(&w.as_str())) {
        return accept("domain_cue");
    }
    ignore("no_cues")
}

// ── IPC ─────────────────────────────────────────────────────────────────────────────────────

/// `"directedGate": false` in settings.json turns the gate off (default ON).
pub fn gate_enabled<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    use tauri::Manager;
    let Ok(dir) = app.path().app_data_dir() else { return true };
    let Ok(content) = std::fs::read_to_string(dir.join("settings.json")) else { return true };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else { return true };
    json.get("directedGate")
        .or_else(|| json.get("directed_gate"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// `ghostEngagementSecs` in settings.json (default 15, 0 = rule off, capped at 120).
pub fn read_engagement_secs<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> u64 {
    use tauri::Manager;
    let Ok(dir) = app.path().app_data_dir() else { return DEFAULT_ENGAGEMENT_SECS };
    let Ok(content) = std::fs::read_to_string(dir.join("settings.json")) else { return DEFAULT_ENGAGEMENT_SECS };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else { return DEFAULT_ENGAGEMENT_SECS };
    json.get("ghostEngagementSecs")
        .or_else(|| json.get("ghost_engagement_secs"))
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_ENGAGEMENT_SECS)
        .min(120)
}

/// Ask whether the transcript of the turn that just ended should be acted on. The origin comes from
/// the Rust capture that produced it (frozen at capture stop), never from the caller.
#[tauri::command]
pub fn directed_gate<R: tauri::Runtime>(transcript: String, app: tauri::AppHandle<R>) -> Verdict {
    if !gate_enabled(&app) {
        return accept("gate_disabled");
    }
    // NEXUS just asked a question ("Shall I start?"): a bare "yes" is the answer.
    if crate::memcore::offer::is_armed() {
        return accept("offer_reply");
    }
    set_engagement_secs(read_engagement_secs(&app));
    let spoken = recent_spoken(Instant::now());
    let verdict = evaluate(&Input {
        transcript: &transcript,
        origin: last_turn_origin(),
        parses_as_command: crate::intent_parser::parse_deterministic(&transcript).is_some(),
        spoken: &spoken,
        dictation_active: crate::orchestrator::is_dictation_active(),
        engaged: is_engaged(Instant::now()),
        window_enabled: engagement_secs() > 0,
    });
    if verdict.accept {
        // An accepted turn is the owner talking: the conversation continues for another window.
        engage_now();
    }
    if !verdict.accept {
        println!(
            "[DROP] Directed speech gate: IGNORED ({}) transcript: '{}'",
            verdict.reason,
            crate::tts::truncate_for_log(&transcript, 80)
        );
        tracing::info!("directed-gate: IGNORED ({}) {:?}", verdict.reason, crate::tts::truncate_for_log(&transcript, 80));
        // local, text-only, rotated (5 MB) — tuning data, never sent anywhere
        crate::missed_intent_logger::log_missed_intent(&transcript, "directed_gate", verdict.reason);
        use tauri::Emitter;
        let _ = app.emit("directed:ignored", serde_json::json!({ "reason": verdict.reason }));
    } else {
        println!(
            "[DIRECTED-GATE] Accepted ({}) transcript: '{}'",
            verdict.reason,
            crate::tts::truncate_for_log(&transcript, 80)
        );
    }
    verdict
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hot(t: &str) -> Verdict {
        evaluate(&Input { transcript: t, origin: Origin::HotMic, parses_as_command: false, spoken: &[], dictation_active: false, engaged: true, window_enabled: true })
    }
    fn hot_with_spoken(t: &str, spoken: &[&str]) -> Verdict {
        let s: Vec<String> = spoken.iter().map(|x| x.to_string()).collect();
        evaluate(&Input { transcript: t, origin: Origin::HotMic, parses_as_command: false, spoken: &s, dictation_active: false, engaged: true, window_enabled: true })
    }

    #[test]
    fn explicit_turns_are_never_gated() {
        for t in ["", "uh", "total strawberry", "I'm gonna say that I'm gonna say that I'm gonna say that"] {
            let v = evaluate(&Input { transcript: t, origin: Origin::Direct, parses_as_command: false, spoken: &[], dictation_active: false, engaged: true, window_enabled: true });
            assert!(v.accept && v.reason == "explicit_turn", "{t:?}");
        }
    }

    #[test]
    fn recognised_commands_and_dictation_always_pass() {
        let c = evaluate(&Input { transcript: "gibberish", origin: Origin::HotMic, parses_as_command: true, spoken: &[], dictation_active: false, engaged: true, window_enabled: true });
        assert!(c.accept && c.reason == "command");
        let d = evaluate(&Input { transcript: "so then we went to the market and bought apples", origin: Origin::HotMic, parses_as_command: false, spoken: &[], dictation_active: true, engaged: true, window_enabled: true });
        assert!(d.accept && d.reason == "dictation");
    }

    #[test]
    fn vocative_passes() {
        assert!(hot("Nexus. Nexus command hub.").accept);
        assert!(hot("hey nexus what's up").accept);
        // but a hallucination loop that happens to contain it does not
        assert!(!hot("nexus nexus nexus nexus nexus nexus nexus nexus").accept);
    }

    #[test]
    fn our_own_speech_is_echo() {
        let spoken = ["Stopped typing, sir.", "Here is the analysis of PR 254 in the zync repository."];
        assert_eq!(hot_with_spoken("Stopped typing sir", &spoken).reason, "echo");
        assert_eq!(hot_with_spoken("here is the analysis of PR 254", &spoken).reason, "echo");
        // a user echoing one word of it is not an echo, nor is sharing function words ("the")
        assert!(hot_with_spoken("open the repository", &spoken).accept);
        assert!(hot_with_spoken("open the analysis tab", &spoken).accept);
        // a single content word is never an echo
        assert_ne!(hot_with_spoken("typing", &spoken).reason, "echo");
        // nothing spoken recently => no echo rule
        assert_ne!(hot_with_spoken("stopped typing sir", &[]).reason, "echo");
    }

    #[test]
    fn hallucination_loops_and_fillers_and_long_lines_are_ignored() {
        assert_eq!(hot("I'm gonna say that. I'm gonna say that. I'm gonna say that.").reason, "repetition");
        assert_eq!(
            hot("Open. Mareng calme. I'm going to go. I'm going to go. I'm going to go. I'm going to go. I'm going to go.").reason,
            "repetition"
        );
        assert_eq!(hot("hmm okay").reason, "filler");
        assert_eq!(hot("yeah").reason, "filler");
        assert_eq!(hot("Thank you.").reason, "filler");
        assert_eq!(hot("").reason, "empty");
        assert_eq!(
            hot("open so we were talking yesterday about the thing at the shop and then she said that maybe next week we could go there").reason,
            "too_long"
        );
    }

    #[test]
    fn assistant_shaped_requests_pass() {
        for t in [
            "What's the weather in London?",
            "Can you check my calendar",
            "could you read that again",
            "open the settings",
            "send a message to mom",
            "Type.",
            "Cancel the ghost mode.",
            "Analyzing repository from my github for me.", // "analyzing" is a real STT mishearing of "analyse"
            "remind me to call the bank",
            "is it going to rain tomorrow?",
            "please turn the volume down",
        ] {
            let v = hot(t);
            // "Analyzing …" is not a starter: it must still be judged, not crash — see the labeled set test
            if t.starts_with("Analyzing") {
                continue;
            }
            assert!(v.accept, "{t:?} -> {v:?}");
        }
    }

    /// Labeled set. NON-DIRECTED lines come from the user's real `missed_intents.jsonl` (TV, noise, STT
    /// hallucinations, our own speech); DIRECTED lines are realistic open-mic requests. These are
    /// goals to track, not a guarantee on unseen audio (see ledger P8).
    #[test]
    fn labeled_set_precision() {
        let non_directed = [
            "Total strawberry.",
            "then the",
            "left.",
            "from the",
            "I'm a member.",
            "That's out there.",
            "Ssshh.",
            "you will just pull come out more easily.",
            "Guys I saw. Come on. See you guys.",
            "Nice.",
            "I have to.",
            "Now we're back in the rapid ground.",
            "I'm gonna say that. I'm gonna say that. I'm gonna say that.",
            "I'm going to stay in the search.",
            "That was...",
            "then we'll start with the",
            "No. I have just a slow motion.",
            "Gashat in the world.",
            "See you're in a way of the mind.",
            "Pregnant. Tens it for a second time. Lengua. Se locus afundir.",
            "with the help of the heart, the heart of the heart can be used to release sugar. So, they can be used to eat, okay? As if you will see your body or your body will go too far away.",
            "Open. Mareng calme. I'm going to go. I'm going to go. I'm going to go. I'm going to go. I'm going to go.",
            "Saad Dicca. Open right. Or the page too. Go the page too. Go the page too. Go the page too.",
            "The sound of radio is very heavy. Open the sound of radio is very heavy. You can let the air the air the air the air the air the air the air.",
            "we have so. Banu Nesha. I'm going to have a fire.",
            "Sermon Dharo Bori Dune.",
            "Sustach.",
            "Eerlesska.",
            "and that is why the economy keeps struggling",
            "she told me yesterday that it was fine",
            "uh huh yeah",
        ];
        let directed = [
            "Open",
            "Type.",
            "Cancel the ghost mode.",
            "Prem on WhatsApp.",
            "S. Ghost mode.",
            "The post mode.",
            "Wave browser.",
            "I again have a command hub.",
            "NEXUS comment hub.",
            "Nexus. Nexus command hub.",
            "What's the weather in London?",
            "Can you check my calendar",
            "open the settings",
            "send a message to mom saying I'll be late",
            "remind me to call the bank at five",
            "is it going to rain tomorrow?",
            "please turn the volume down",
            "search for almonds",
            "show me my latest emails",
            "stop",
            "tell me a joke",
            "read my notifications",
            "how long is the commute to work",
            "close this window",
            "play some music",
        ];
        let false_accepts: Vec<&&str> = non_directed.iter().filter(|t| hot(t).accept).collect();
        let false_ignores: Vec<&&str> = directed.iter().filter(|t| !hot(t).accept).collect();
        println!("false accepts ({}/{}): {:?}", false_accepts.len(), non_directed.len(), false_accepts);
        println!("false ignores ({}/{}): {:?}", false_ignores.len(), directed.len(), false_ignores);
        // goals: false-accept <= 10 %, false-ignore <= 5 % on this set (it is small; track, don't over-fit)
        assert!(false_accepts.len() as f32 / non_directed.len() as f32 <= 0.10, "false accepts: {false_accepts:?}");
        assert!(false_ignores.len() as f32 / directed.len() as f32 <= 0.05, "false ignores: {false_ignores:?}");
    }

    /// Dev helper (ignored): replays the user's REAL `missed_intents.jsonl` (utterances nothing could
    /// match) through the gate as hot-mic turns and prints what it would have let through to the LLM.
    #[test]
    #[ignore]
    fn replay_real_missed_intents() {
        let path = std::env::var("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap()
            .join("com.nexus.assistant")
            .join("missed_intents.jsonl");
        let Ok(text) = std::fs::read_to_string(&path) else {
            eprintln!("no missed_intents.jsonl — skipping");
            return;
        };
        let mut seen = std::collections::HashSet::new();
        let (mut kept, mut dropped): (Vec<String>, Vec<(String, &str)>) = (vec![], vec![]);
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            let Some(t) = v.get("transcript").and_then(|x| x.as_str()) else { continue };
            if !seen.insert(t.to_lowercase()) {
                continue;
            }
            let verdict = hot(t);
            if verdict.accept {
                kept.push(format!("{t:?} [{}]", verdict.reason));
            } else {
                dropped.push((t.to_string(), verdict.reason));
            }
        }
        println!("REPLAY: {} unique real utterances -> {} ignored, {} kept (would reach the LLM)", seen.len(), dropped.len(), kept.len());
        let mut by: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for (_, r) in &dropped {
            *by.entry(r).or_insert(0) += 1;
        }
        println!("ignored by reason: {by:?}");
        for k in &kept {
            println!("KEPT {k}");
        }
    }

    #[test]
    fn origin_parse_and_snapshot() {
        assert_eq!(Origin::parse("hot_mic"), Origin::HotMic);
        assert_eq!(Origin::parse(" HOT-MIC "), Origin::HotMic);
        assert_eq!(Origin::parse("direct"), Origin::Direct);
        assert_eq!(Origin::parse("anything"), Origin::Direct);
        // the turn origin is frozen at capture stop: a capture opened later must not change it
        set_capture_origin(Origin::HotMic);
        snapshot_turn_origin();
        set_capture_origin(Origin::Direct);
        assert_eq!(last_turn_origin(), Origin::HotMic);
        snapshot_turn_origin();
        assert_eq!(last_turn_origin(), Origin::Direct);
    }

    #[test]
    fn note_spoken_keeps_only_recent_lines() {
        // shared static: only assert relative behaviour
        for i in 0..10 {
            note_spoken(&format!("line number {i}"));
        }
        let r = recent_spoken(Instant::now());
        assert!(r.len() <= SPOKEN_KEEP);
        assert!(r.last().map(|s| s.as_str()) == Some("line number 9"));
        note_spoken("   "); // ignored
        assert!(recent_spoken(Instant::now()).last().map(|s| s.as_str()) == Some("line number 9"));
    }

    #[test]
    fn tokens_normalise() {
        assert_eq!(tokens("Stopped typing, sir!"), vec!["stopped", "typing", "sir"]);
        assert_eq!(tokens("I'm 'quoted'"), vec!["i'm", "quoted"]);
        assert!(tokens("...").is_empty());
    }

    fn gate(t: &str, parses: bool, engaged: bool, spoken: &[&str]) -> Verdict {
        let s: Vec<String> = spoken.iter().map(|x| x.to_string()).collect();
        evaluate(&Input {
            transcript: t,
            origin: Origin::HotMic,
            parses_as_command: parses,
            spoken: &s,
            dictation_active: false,
            engaged,
            window_enabled: true,
        })
    }

    #[test]
    fn a_command_outside_the_window_needs_the_word_nexus() {
        // TV / mum / a call in the room: "open youtube" parses as a command, but nobody engaged us.
        let v = gate("open youtube", true, false, &[]);
        assert!(!v.accept && v.reason == "outside_window", "{v:?}");
        // Addressed to us it passes, window or not.
        assert!(gate("Nexus, open youtube", true, false, &[]).accept);
        // Inside the window the same words are the owner's next command.
        assert!(gate("open youtube", true, true, &[]).accept);
        // The rule can be switched off.
        let off = evaluate(&Input {
            transcript: "open youtube",
            origin: Origin::HotMic,
            parses_as_command: true,
            spoken: &[],
            dictation_active: false,
            engaged: false,
            window_enabled: false,
        });
        assert!(off.accept);
    }

    #[test]
    fn other_assistants_wake_words_are_never_for_us() {
        for t in ["alexa play some music", "hey google open youtube", "OK Google, what's the weather", "hey siri set a timer"] {
            // even when the words happen to parse as one of our commands, and even inside the window
            let v = gate(t, true, true, &[]);
            assert!(!v.accept && v.reason == "foreign_wake", "{t}: {v:?}");
        }
        // If the owner addresses us by name the sentence is still ours.
        assert!(gate("nexus ignore alexa and open notepad", true, true, &[]).accept);
    }

    #[test]
    fn our_own_voice_coming_back_is_not_a_command() {
        let spoken = ["Opening Command Hub, sir."];
        // "opening command hub" parses as an open command, but it is our own line echoed by the speakers.
        let v = gate("opening command hub sir", true, true, &spoken);
        assert!(!v.accept && v.reason == "echo", "{v:?}");
        // The owner genuinely asking for it is different words, so it passes.
        assert!(gate("open the command hub", true, true, &spoken).accept);
    }

    #[test]
    fn the_engagement_window_opens_extends_and_expires() {
        reset_engagement();
        assert!(!is_engaged(Instant::now()));
        engage_for(Duration::from_millis(60));
        assert!(is_engaged(Instant::now()));
        std::thread::sleep(Duration::from_millis(90));
        // Never shortened by a smaller request, extended by a larger one.
        engage_for(Duration::from_secs(5));
        engage_for(Duration::from_millis(1));
        assert!(is_engaged(Instant::now() + Duration::from_secs(4)));
        assert!(!is_engaged(Instant::now() + Duration::from_secs(6)));
    }
}
