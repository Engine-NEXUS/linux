//! Ghostwriter session — a persistent dictation room, not a transaction.
//!
//! Entered explicitly ("ghostwriter", "take a letter"), stays open across
//! turns: dictation appends ink, allowlisted commands act
//! (`send it`, `scratch that`, `change X to Y`, `read it back`, `discard`).
//! Everything else is text — even "can u send it to me".
//!
//! Mic rule: the mic is always hot; this flag only changes ROUTING.
//! While a session is active, every transcript goes to `handle_turn`
//! instead of the normal parse pipeline. If the session lapses (timeout),
//! the user re-calls NEXUS and the draft is still here for resume.
//!
//! No auto-execution, no countdowns: the draft sits visible in the sidebar
//! until an explicit send. The draft IS the undo state.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Idle expiry: 10 minutes of silence ends the session (draft kept in
/// memory for resume via re-entry).
const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(600);

/// A Ghostwriter session: who we're writing to, on which channel, and the ink.
#[derive(Debug, Clone)]
pub struct GhostSession {
    pub contact: Option<String>,
    pub channel: String,
    pub draft: String,
    pub last_active: Instant,
}

impl GhostSession {
    fn touch(&mut self) {
        self.last_active = Instant::now();
    }

    fn expired(&self) -> bool {
        self.last_active.elapsed() > SESSION_IDLE_TIMEOUT
    }
}

static SESSION: Mutex<Option<GhostSession>> = Mutex::new(None);

/// Is a live (unexpired) session active? Expired sessions are reaped here.
pub fn is_active() -> bool {
    let mut guard = SESSION.lock().unwrap();
    if let Some(s) = guard.as_ref() {
        if s.expired() {
            *guard = None;
            return false;
        }
        return true;
    }
    false
}

/// Enter (or re-enter) the room. Keeps an existing draft on resume.
pub fn enter(contact: Option<String>) -> String {
    let mut guard = SESSION.lock().unwrap();
    if let Some(s) = guard.as_mut() {
        // Resume: keep the ink, refresh the clock, adopt a new target.
        if contact.is_some() {
            s.contact = contact.clone();
        }
        s.touch();
        let draft_note = if s.draft.trim().is_empty() {
            "blank page"
        } else {
            "your draft is still here"
        };
        match &s.contact {
            Some(c) => format!("Ghostwriter open for {c}, sir — {draft_note}."),
            None => format!("Ghostwriter open, sir — {draft_note}. Who is this for?"),
        }
    } else {
        *guard = Some(GhostSession {
            contact: contact.clone(),
            channel: "whatsapp".to_string(),
            draft: String::new(),
            last_active: Instant::now(),
        });
        match contact {
            Some(c) => format!("Ghostwriter open for {c}, sir — speak, and I'll write."),
            None => "Ghostwriter open, sir — who is this for?".to_string(),
        }
    }
}

/// Leave the room. Draft is dropped on explicit discard/exit.
pub fn exit() -> String {
    *SESSION.lock().unwrap() = None;
    "Ghostwriter closed, sir.".to_string()
}

/// Clear the ink but stay in the room (after a send — ready for the
/// next dictation). Contact and clock are preserved.
pub fn clear_draft() {
    if let Some(s) = SESSION.lock().unwrap().as_mut() {
        s.draft.clear();
        s.touch();
    }
}

/// Outcome of one in-session turn.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnOutcome {
    /// Ink appended; string is the full draft (for the sidebar card).
    Dictated(String),
    /// A command was handled; string is the spoken reply.
    Replied(String),
    /// Draft + contact complete — ready for the gated send path.
    SendReady { contact: String, message: String },
    /// Session ended; string is the spoken reply.
    Exited(String),
}

/// Allowlisted commands. Everything else is ink — including sentences
/// containing "send".
fn match_command(text: &str) -> Option<TurnOutcome> {
    let t = text.trim().to_lowercase();

    // Exit doors — every sayable form of leaving the room.
    if ["discard", "forget it", "command mode", "close ghostwriter", "exit ghostwriter",
        "exit the mode", "exit mode", "exit ghost mode", "stop ghost mode",
        "stop ghostwriter", "quit ghostwriter", "turn off ghostwriter",
        "close ghost mode", "leave ghostwriter"]
        .iter()
        .any(|p| t == *p || t.ends_with(&format!(" {p}")))
    {
        return Some(TurnOutcome::Exited(exit()));
    }
    // Send doors (filler-tolerant): "send it", "ok send it", "shoot it".
    let stripped = t
        .strip_prefix("ok ")
        .or_else(|| t.strip_prefix("yeah "))
        .or_else(|| t.strip_prefix("perfect "))
        .or_else(|| t.strip_prefix("so "))
        .unwrap_or(&t);
    if ["send it", "shoot it", "fire it off", "send the message"].contains(&stripped) {
        let guard = SESSION.lock().unwrap();
        let s = guard.as_ref()?;
        if s.draft.trim().is_empty() {
            return Some(TurnOutcome::Replied(
                "Nothing written yet, sir — dictate first.".to_string(),
            ));
        }
        match &s.contact {
            Some(c) => {
                return Some(TurnOutcome::SendReady {
                    contact: c.clone(),
                    message: s.draft.trim().to_string(),
                })
            }
            None => {
                return Some(TurnOutcome::Replied(
                    "Who should I send this to, sir?".to_string(),
                ))
            }
        }
    }
    // Read-back borrows Echo's voice.
    if t == "read it back" || t == "echo" || t == "read that back" {
        let guard = SESSION.lock().unwrap();
        let draft = guard.as_ref().map(|s| s.draft.trim().to_string()).unwrap_or_default();
        if draft.is_empty() {
            return Some(TurnOutcome::Replied("The page is blank, sir.".to_string()));
        }
        return Some(TurnOutcome::Replied(draft));
    }
    // Scratch last chunk.
    if t == "scratch that" || t == "delete that" || t == "undo that" {
        let mut guard = SESSION.lock().unwrap();
        if let Some(s) = guard.as_mut() {
            s.draft = pop_last_sentence(&s.draft);
            s.touch();
            if s.draft.trim().is_empty() {
                return Some(TurnOutcome::Replied("Scratched, sir — blank page.".to_string()));
            }
            return Some(TurnOutcome::Dictated(s.draft.clone()));
        }
    }
    // In-place edit: "change X to Y".
    if let Some(rest) = t.strip_prefix("change ") {
        if let Some(pos) = rest.find(" to ") {
            let (from, to) = (rest[..pos].trim(), rest[pos + 4..].trim());
            if !from.is_empty() && !to.is_empty() {
                let mut guard = SESSION.lock().unwrap();
                if let Some(s) = guard.as_mut() {
                    if s.draft.contains(from) {
                        s.draft = s.draft.replacen(from, to, 1);
                        s.touch();
                        return Some(TurnOutcome::Dictated(s.draft.clone()));
                    }
                    return Some(TurnOutcome::Replied(format!(
                        "I don't see '{from}' on the page, sir."
                    )));
                }
            }
        }
    }
    // Retarget mid-session: "now one to dad", "switch to mom", "this is for X".
    for prefix in ["now one to ", "switch to ", "this is for ", "write to "] {
        if let Some(who) = t.strip_prefix(prefix) {
            let who = who.trim();
            if !who.is_empty() {
                let mut guard = SESSION.lock().unwrap();
                if let Some(s) = guard.as_mut() {
                    s.contact = Some(who.to_string());
                    s.touch();
                    return Some(TurnOutcome::Replied(format!(
                        "Switched to {who}, sir — draft kept."
                    )));
                }
            }
        }
    }
    None
}

/// Handle one transcript while a session is active.
/// Commands act; everything else becomes ink.
pub fn handle_turn(text: &str) -> TurnOutcome {
    if !is_active() {
        return TurnOutcome::Replied("Ghostwriter isn't open, sir.".to_string());
    }
    if let Some(outcome) = match_command(text) {
        return outcome;
    }
    // Ink: append with spacing + sentence capitalization.
    let mut guard = SESSION.lock().unwrap();
    let s = guard.as_mut().expect("session checked active");
    let piece = text.trim();
    if !piece.is_empty() {
        if s.draft.trim().is_empty() {
            s.draft = capitalize_first(piece);
        } else {
            s.draft.push(' ');
            s.draft.push_str(&capitalize_first(piece));
        }
    }
    s.touch();
    TurnOutcome::Dictated(s.draft.clone())
}

/// Current draft + contact for the sidebar card. None when no session.
pub fn card_state() -> Option<(Option<String>, String)> {
    SESSION
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| (s.contact.clone(), s.draft.clone()))
}

/// Drop the last sentence-ish chunk (split on . ! ? or fallback: last 8 words).
fn pop_last_sentence(draft: &str) -> String {
    let t = draft.trim();
    for sep in ['.', '!', '?'] {
        if let Some(pos) = t.rfind(sep) {
            let kept = t[..=pos].trim().to_string();
            // If the separator is the very end, drop that whole sentence:
            // find the previous one.
            if kept.len() == t.len() {
                let before = t[..pos].trim();
                if let Some(p2) = before.rfind(['.', '!', '?']) {
                    return t[..=p2].trim().to_string();
                }
                return String::new();
            }
            return kept;
        }
    }
    // No sentence punctuation: drop the last 8 words.
    let words: Vec<&str> = t.split_whitespace().collect();
    if words.len() <= 8 {
        return String::new();
    }
    words[..words.len() - 8].join(" ")
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests share the process-wide SESSION static and run in parallel
    /// threads — serialize them or they poison each other's mutex.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn fresh(contact: Option<&str>) {
        *SESSION.lock().unwrap() = None;
        enter(contact.map(|s| s.to_string()));
    }

    #[test]
    fn dictate_appends_ink() {
        let _g = TEST_LOCK.lock().unwrap();
        fresh(Some("mom"));
        assert!(matches!(handle_turn("I miss you"), TurnOutcome::Dictated(_)));
        match handle_turn("hope you are well") {
            TurnOutcome::Dictated(d) => {
                assert!(d.contains("I miss you") && d.contains("Hope you are well"));
            }
            other => panic!("expected Dictated, got {other:?}"),
        }
        *SESSION.lock().unwrap() = None;
    }

    #[test]
    fn send_phrase_inside_dictation_is_ink() {
        let _g = TEST_LOCK.lock().unwrap();
        fresh(Some("mom"));
        match handle_turn("can u send it to me tomorrow") {
            TurnOutcome::Dictated(d) => assert!(d.contains("send it to me")),
            other => panic!("must be ink, got {other:?}"),
        }
        *SESSION.lock().unwrap() = None;
    }

    #[test]
    fn send_it_fires_only_as_command() {
        let _g = TEST_LOCK.lock().unwrap();
        fresh(Some("mom"));
        handle_turn("see you soon");
        match handle_turn("ok send it") {
            TurnOutcome::SendReady { contact, message } => {
                assert_eq!(contact, "mom");
                assert!(message.contains("See you soon"));
            }
            other => panic!("expected SendReady, got {other:?}"),
        }
        *SESSION.lock().unwrap() = None;
    }

    #[test]
    fn send_without_draft_or_contact_asks() {
        let _g = TEST_LOCK.lock().unwrap();
        fresh(None);
        // Empty page → asked to dictate, not to send.
        match handle_turn("send it") {
            TurnOutcome::Replied(r) => assert!(r.contains("Nothing written yet")),
            other => panic!("expected dictate prompt, got {other:?}"),
        }
        // Ink but no contact → asked who it's for.
        handle_turn("hello there");
        match handle_turn("send it") {
            TurnOutcome::Replied(r) => assert!(r.contains("Who should")),
            other => panic!("expected ask-who, got {other:?}"),
        }
    }

    #[test]
    fn scratch_and_change_and_exit() {
        let _g = TEST_LOCK.lock().unwrap();
        fresh(Some("mom"));
        handle_turn("I miss you. Take care.");
        match handle_turn("scratch that") {
            TurnOutcome::Dictated(d) => assert!(!d.contains("Take care")),
            other => panic!("expected Dictated, got {other:?}"),
        }
        match handle_turn("change miss to love") {
            TurnOutcome::Dictated(d) => assert!(d.contains("love")),
            other => panic!("expected Dictated, got {other:?}"),
        }
        match handle_turn("discard") {
            TurnOutcome::Exited(_) => {}
            other => panic!("expected Exited, got {other:?}"),
        }
        assert!(!is_active());
    }

    #[test]
    fn resume_keeps_draft() {
        let _g = TEST_LOCK.lock().unwrap();
        fresh(Some("mom"));
        handle_turn("hello there");
        enter(Some("dad".to_string()));
        match card_state() {
            Some((c, d)) => {
                assert_eq!(c.as_deref(), Some("dad"));
                assert!(d.contains("Hello there"));
            }
            None => panic!("expected session"),
        }
        *SESSION.lock().unwrap() = None;
    }
}
