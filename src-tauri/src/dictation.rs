//! Dictation: safe typing of cleaned speech, with undo.
//!
//! `dictation_text::clean` decides WHAT to type; this module decides WHETHER and records HOW MUCH, so
//! that "scratch that" can take it back. Rules (each is a refusal reason that is spoken, never silent):
//! - never type into a protected window (banking / password manager / crypto wallet, the denylist in
//!   `live::safety`), into NEXUS's own windows, into an elevated window we cannot type into, or into
//!   a password field (on a native Wayland session the focused window cannot be identified, so that
//!   rule cannot be enforced there — see `foreground_target`);
//! - never press Enter on its own: utterances are joined with a space, line breaks only come from
//!   the user saying "new line" / "new paragraph";
//! - undo only when the same window is still in front, otherwise Ctrl+Z.
//!
//! On Linux there is one more case: a native Wayland window cannot be typed into at all (the
//! synthetic key events never reach it), so the text goes on the clipboard and the user pastes it
//! — see [`fallback_line`]. Each utterance replaces the previous one there, so the clipboard holds
//! the latest line only.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::dictation_text::{self, Ctx};
use crate::live::commands::keyboard::InputSession;

/// A dictation session with no accepted line for this long stops itself (a TV must not type into a
/// forgotten document).
pub const IDLE_STOP: Duration = Duration::from_secs(30);

/// What is known about the window the text would go into.
#[derive(Debug, Clone, Default)]
pub struct Target {
    pub title: String,
    pub process: String,
    pub elevated_blocks_us: bool,
    pub password_field: bool,
}

/// Why typing into `t` must not happen (None = fine). Pure.
pub fn refusal(t: &Target) -> Option<String> {
    let hay = format!("{} {}", t.title, t.process);
    if crate::live::safety::is_target_blocked(&hay) {
        return Some("That window is protected, sir. I didn't type anything.".to_string());
    }
    if t.process.eq_ignore_ascii_case("nexus.exe") {
        return Some("The focus is on NEXUS itself, sir. Click where you want the text.".to_string());
    }
    if t.elevated_blocks_us {
        return Some("That window runs as administrator, so I can't type into it, sir.".to_string());
    }
    if t.password_field {
        return Some("That's a password field, sir. I never type into those.".to_string());
    }
    None
}

/// Heuristic: targets that take one line (Enter submits). Chat boxes and search/address fields.
pub fn single_line_target(t: &Target) -> bool {
    let p = t.process.to_lowercase();
    let title = t.title.to_lowercase();
    p.contains("whatsapp")
        || p.contains("telegram")
        || p.contains("discord")
        || p.contains("slack")
        || title.contains("whatsapp")
        || title.contains("search")
}

// ── undo ────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insertion {
    pub chars: usize,
    pub title: String,
}

#[derive(Debug, Default)]
pub struct History {
    items: Vec<Insertion>,
}

pub enum Undo {
    /// Backspace this many characters (same window still in front).
    Backspace(usize),
    /// The window changed: fall back to Ctrl+Z.
    CtrlZ,
    Nothing,
}

impl History {
    pub fn push(&mut self, ins: Insertion) {
        self.items.push(ins);
        if self.items.len() > 50 {
            self.items.remove(0);
        }
    }
    pub fn undo(&mut self, current_title: &str) -> Undo {
        match self.items.pop() {
            None => Undo::Nothing,
            Some(i) if i.title == current_title => Undo::Backspace(i.chars),
            Some(_) => Undo::CtrlZ,
        }
    }
    pub fn clear(&mut self) {
        self.items.clear();
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

static HISTORY: Mutex<History> = Mutex::new(History { items: Vec::new() });
static AT_SENTENCE_START: AtomicBool = AtomicBool::new(true);
static LAST_LINE_MS: AtomicU64 = AtomicU64::new(0);
static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn now_ms() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// A new dictation session: fresh undo history, capitalise the first word.
pub fn begin_session() {
    if let Ok(mut h) = HISTORY.lock() {
        h.clear();
    }
    AT_SENTENCE_START.store(true, Ordering::Relaxed);
    LAST_LINE_MS.store(now_ms(), Ordering::Relaxed);
}

/// True when no line was accepted for `IDLE_STOP`.
pub fn idle_expired() -> bool {
    expired(now_ms(), LAST_LINE_MS.load(Ordering::Relaxed))
}

fn expired(now: u64, last_line: u64) -> bool {
    now.saturating_sub(last_line) >= IDLE_STOP.as_millis() as u64
}

// ── stop phrases and the user dictionary ────────────────────────────────────────────────────

/// "stop typing." / "Nexus, stop dictation" / "stop": STT adds punctuation and the odd address, so
/// compare the words, not the string.
pub fn is_stop_phrase(raw: &str) -> bool {
    let words: Vec<String> = raw
        .to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect();
    let mut w: Vec<&str> = words.iter().map(|x| x.as_str()).collect();
    while w.first().map_or(false, |x| matches!(*x, "nexus" | "hey" | "okay" | "ok" | "please")) {
        w.remove(0);
    }
    while w.last().map_or(false, |x| matches!(*x, "please" | "now" | "nexus")) {
        w.pop();
    }
    matches!(
        w.join(" ").as_str(),
        "stop typing"
            | "stop dictation"
            | "done typing"
            | "end typing"
            | "finish typing"
            | "exit dictation"
            | "stop"
            | "stand down"
            | "cancel"
    )
}

/// `dictionary.json` in the app data folder: {"spoken": "Written", ...}. Missing/invalid = empty.
pub fn load_dictionary(app_data_dir: &std::path::Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(app_data_dir.join("dictionary.json")) else {
        return Vec::new();
    };
    parse_dictionary(&text)
}

pub fn parse_dictionary(json: &str) -> Vec<(String, String)> {
    serde_json::from_str::<std::collections::BTreeMap<String, String>>(json)
        .map(|m| m.into_iter().filter(|(k, v)| !k.trim().is_empty() && !v.is_empty()).collect())
        .unwrap_or_default()
}

// ── the operation ───────────────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Typed(String),
    Undone,
    /// Nothing to type (a pure command with no effect, or only fillers).
    Nothing,
    /// Not typed; say this.
    ///
    /// On Linux the text may have gone to the clipboard instead (a native
    /// Wayland window cannot be typed into), so the sentence can also be
    /// "copied to the clipboard — it holds only this last line".
    Refused(String),
}

/// The line to speak when dictation text could not be typed into the focused
/// window.
///
/// `None` means "typing was the right delivery, so report the typing error";
/// `Some(line)` is what to say instead.
///
/// Pure, so the choice can be tested without a display, and shared by every
/// platform so the decision lives in one place. `clipboard` says whether the
/// text could be put on the clipboard — the caller has already put it there
/// when this is true.
pub fn fallback_line(session: &InputSession, clipboard: bool) -> Option<String> {
    if !session.display {
        return Some("There is no display, sir — I cannot type anything.".to_string());
    }
    if !session.wayland {
        // X11: typing was expected to work and did not. The caller reports it.
        return None;
    }
    if clipboard {
        // The text is on the clipboard; the user pastes it. Each utterance
        // replaces the previous one, so the clipboard holds the latest line
        // only — say so, rather than let the user paste a fragment and think
        // the rest was lost.
        Some(
            "Copied to the clipboard, sir — it holds only this last line, so paste it with Ctrl+V."
                .to_string(),
        )
    } else {
        Some(
            "I cannot type into native Wayland windows, sir, and there is no clipboard to fall back on."
                .to_string(),
        )
    }
}

/// Dictation text that could not be typed: what to do about it.
///
/// Windows and macOS: typing was supposed to work, so the failure is the
/// news. The shared decision is still asked, so both platforms agree on what
/// "the fallback is" — on these platforms it is never the clipboard.
#[cfg(not(target_os = "linux"))]
fn typing_failed(_text: &str, err: &str) -> Outcome {
    let session = crate::live::commands::keyboard::current_session();
    match fallback_line(&session, false) {
        Some(line) => Outcome::Refused(line),
        None => Outcome::Refused(format!("Typing failed: {err}")),
    }
}

/// Dictation text that could not be typed: what to do about it.
///
/// On Linux the focused window may be a native Wayland window, which no
/// synthetic input can reach. The text goes on the clipboard instead and the
/// user pastes it — honest, where "Ok sir" over a silent no-op is not.
#[cfg(target_os = "linux")]
fn typing_failed(text: &str, err: &str) -> Outcome {
    let session = crate::live::commands::keyboard::current_session();
    // Only touch the clipboard when the clipboard is actually the fallback.
    let copied = session.wayland && crate::live::commands::keyboard::copy_to_clipboard(text);
    match fallback_line(&session, copied) {
        Some(line) => Outcome::Refused(line),
        None => Outcome::Refused(format!("Typing failed: {err}")),
    }
}

/// Continuous dictation: lines join with a space, capitalisation continues across lines.
pub fn dictate(raw: &str, dictionary: Vec<(String, String)>) -> Outcome {
    dictate_inner(raw, dictionary, false)
}

/// One-shot "type <text>": exactly the spoken text (cleaned), no leading space, capitalised only for
/// document-like targets (a search box or chat box keeps what was said).
pub fn dictate_oneshot(raw: &str, dictionary: Vec<(String, String)>) -> Outcome {
    dictate_inner(raw, dictionary, true)
}

/// Clean `raw` and type it (or undo the previous insertion). Blocking: call from `spawn_blocking`.
fn dictate_inner(raw: &str, dictionary: Vec<(String, String)>, oneshot: bool) -> Outcome {
    let target = foreground_target();
    if let Some(why) = refusal(&target) {
        return Outcome::Refused(why);
    }
    let single = single_line_target(&target);
    let ctx = Ctx {
        at_sentence_start: if oneshot { !single } else { AT_SENTENCE_START.load(Ordering::Relaxed) },
        single_line: single,
        dictionary,
    };
    let cleaned = dictation_text::clean(raw, &ctx);
    LAST_LINE_MS.store(now_ms(), Ordering::Relaxed);

    if cleaned.scratch_previous {
        let undo = HISTORY.lock().map(|mut h| h.undo(&target.title)).unwrap_or(Undo::Nothing);
        return match undo {
            Undo::Backspace(n) => {
                let mut pressed = true;
                for _ in 0..n {
                    if crate::live::commands::keyboard::press_key("backspace").is_err() {
                        pressed = false;
                        break;
                    }
                }
                // On Linux the focused window may be out of reach (a native
                // Wayland window), and saying "undone" over an insertion that
                // is still there is exactly the lie this module refuses to
                // tell.
                #[cfg(target_os = "linux")]
                {
                    if !pressed {
                        return Outcome::Refused(
                            "I could not take that text back, sir — I cannot reach that window."
                                .to_string(),
                        );
                    }
                }
                #[cfg(not(target_os = "linux"))]
                let _ = pressed;
                AT_SENTENCE_START.store(true, Ordering::Relaxed);
                AT_SENTENCE_START_AFTER_BREAK.store(false, Ordering::Relaxed);
                Outcome::Undone
            }
            Undo::CtrlZ => {
                let sent = crate::live::commands::keyboard::press_hotkey(&["ctrl", "z"]);
                #[cfg(target_os = "linux")]
                {
                    if sent.is_err() {
                        return Outcome::Refused(
                            "I could not take that text back, sir — I cannot reach that window."
                                .to_string(),
                        );
                    }
                }
                #[cfg(not(target_os = "linux"))]
                let _ = sent;
                Outcome::Undone
            }
            Undo::Nothing => Outcome::Nothing,
        };
    }
    if cleaned.text.trim().is_empty() {
        return Outcome::Nothing;
    }

    // Join to what is already there with one space, unless it ended a line.
    let joiner = if oneshot
        || HISTORY.lock().map(|h| h.is_empty()).unwrap_or(true)
        || AT_SENTENCE_START_AFTER_BREAK.load(Ordering::Relaxed)
    {
        ""
    } else {
        " "
    };
    let to_type = format!("{joiner}{}", cleaned.text);
    if let Err(e) = crate::live::commands::keyboard::type_text(&to_type) {
        return typing_failed(&to_type, &e);
    }
    AT_SENTENCE_START.store(dictation_text::ends_sentence(&cleaned.text), Ordering::Relaxed);
    AT_SENTENCE_START_AFTER_BREAK.store(cleaned.text.ends_with('\n'), Ordering::Relaxed);
    if let Ok(mut h) = HISTORY.lock() {
        h.push(Insertion { chars: to_type.chars().count(), title: target.title.clone() });
    }
    println!(
        "[DICT] raw='{}' -> typed {} chars into '{}' ({})",
        crate::tts::truncate_for_log(raw, 60),
        to_type.chars().count(),
        crate::tts::truncate_for_log(&target.title, 40),
        target.process
    );
    Outcome::Typed(cleaned.text)
}

static AT_SENTENCE_START_AFTER_BREAK: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "windows")]
fn foreground_target() -> Target {
    let title = crate::browser_url::foreground_window_title().unwrap_or_default();
    let process = crate::browser_url::foreground_process_name().unwrap_or_default();
    let mut t = Target { title, process, ..Default::default() };
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let hwnd = GetForegroundWindow();
        if hwnd.0 != 0 {
            let target_elevated = crate::live::commands::window::window_process_elevated(hwnd);
            let ours = crate::live::commands::window::our_process_elevated();
            t.elevated_blocks_us = crate::live::commands::window::elevated_block_reason(target_elevated, ours).is_some();
        }
    }
    t.password_field = focused_is_password();
    t
}

#[cfg(target_os = "linux")]
fn foreground_target() -> Target {
    // The foreground window title is knowable on X11 only (xdotool). On a
    // native Wayland session there is no portable way to ask the compositor
    // which window has the focus, so the title stays empty.
    //
    // Consequence, stated plainly: on native Wayland the protected-window
    // gate does NOT work. `refusal()` has no title to match against
    // `live::safety::DENIED_TARGETS`, so a dictated password into a native
    // Wayland password manager is copied to the clipboard unrefused. The gate
    // is real on X11; on Wayland it is an open limitation of this port, not a
    // solved problem.
    let title = crate::browser_url::foreground_window_title().unwrap_or_default();
    Target { title, ..Default::default() }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn foreground_target() -> Target {
    Target::default()
}

/// UIA: is the focused control a password box? Unknown (API error) counts as no.
#[cfg(target_os = "windows")]
fn focused_is_password() -> bool {
    use uiautomation::UIAutomation;
    UIAutomation::new()
        .ok()
        .and_then(|a| a.get_focused_element().ok())
        .and_then(|e| e.is_password().ok())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tgt(title: &str, process: &str) -> Target {
        Target { title: title.into(), process: process.into(), ..Default::default() }
    }

    #[test]
    fn protected_own_elevated_and_password_targets_are_refused() {
        assert!(refusal(&tgt("Untitled - Notepad", "notepad.exe")).is_none());
        assert!(refusal(&tgt("Inbox - Gmail - Brave", "brave.exe")).is_none());
        assert!(refusal(&tgt("1Password", "1password.exe")).is_some());
        assert!(refusal(&tgt("Online Banking", "brave.exe")).is_some());
        assert!(refusal(&tgt("NEXUS", "nexus.exe")).is_some());
        assert!(refusal(&Target { elevated_blocks_us: true, ..tgt("Admin shell", "cmd.exe") }).is_some());
        assert!(refusal(&Target { password_field: true, ..tgt("Sign in - Brave", "brave.exe") }).is_some());
    }

    #[test]
    fn chat_and_search_targets_are_single_line() {
        assert!(single_line_target(&tgt("WhatsApp", "whatsapp.root.exe")));
        assert!(single_line_target(&tgt("Search - Brave", "brave.exe")));
        assert!(!single_line_target(&tgt("Untitled - Notepad", "notepad.exe")));
    }

    #[test]
    fn undo_backspaces_in_the_same_window_and_falls_back_to_ctrl_z_elsewhere() {
        let mut h = History::default();
        h.push(Insertion { chars: 12, title: "Notes".into() });
        h.push(Insertion { chars: 5, title: "Notes".into() });
        assert!(matches!(h.undo("Notes"), Undo::Backspace(5)));
        assert!(matches!(h.undo("Other window"), Undo::CtrlZ));
        assert!(matches!(h.undo("Notes"), Undo::Nothing));
    }

    #[test]
    fn history_is_bounded() {
        let mut h = History::default();
        for i in 0..80 {
            h.push(Insertion { chars: i, title: "t".into() });
        }
        assert_eq!(h.items.len(), 50);
    }

    #[test]
    fn a_session_starts_fresh_and_times_out() {
        begin_session();
        assert!(!idle_expired());
        assert!(HISTORY.lock().unwrap().is_empty());
        let stop = IDLE_STOP.as_millis() as u64;
        assert!(!expired(stop - 1, 0));
        assert!(expired(stop, 0));
        assert!(expired(stop * 2, stop / 2));
    }

    #[test]
    fn stop_phrases_survive_stt_punctuation_and_an_address() {
        for p in ["Stop typing.", "stop typing", "Nexus, stop dictation.", "Okay, stop.", "Done typing!", "stand down please"] {
            assert!(is_stop_phrase(p), "{p}");
        }
        for p in ["stop the music", "typing is fun", "please stop typing so loudly", "cancel my order"] {
            assert!(!is_stop_phrase(p), "{p}");
        }
    }

    #[test]
    fn the_dictionary_file_is_parsed_defensively() {
        assert_eq!(
            parse_dictionary(r#"{"zinc":"Zync","":"x","empty":""}"#),
            vec![("zinc".to_string(), "Zync".to_string())]
        );
        assert!(parse_dictionary("not json").is_empty());
        assert!(load_dictionary(std::path::Path::new("Z:/definitely/not/here")).is_empty());
    }

    #[test]
    fn an_x11_session_reports_the_typing_failure() {
        // Typing was supposed to work, so there is no fallback line: the
        // caller reports the typing error itself.
        let s = InputSession { display: true, wayland: false, xwayland_focus: None };
        assert!(fallback_line(&s, false).is_none());
        assert!(fallback_line(&s, true).is_none());
    }

    #[test]
    fn a_wayland_session_falls_back_to_the_clipboard() {
        let s = InputSession { display: true, wayland: true, xwayland_focus: Some(false) };
        let line = fallback_line(&s, true).expect("a spoken line");
        assert!(line.to_lowercase().contains("clipboard"), "{line}");
        assert!(line.to_lowercase().contains("paste"), "{line}");
        // Each utterance replaces the previous one on the clipboard, so the
        // line has to say so — otherwise the user pastes a fragment and
        // thinks the rest was lost.
        assert!(line.to_lowercase().contains("last line"), "{line}");

        // No clipboard: a clear error, never a silent no-op.
        let line = fallback_line(&s, false).expect("a spoken line");
        assert!(line.to_lowercase().contains("wayland"), "{line}");
    }

    #[test]
    fn no_display_says_so_instead_of_typing() {
        let s = InputSession { display: false, wayland: false, xwayland_focus: None };
        let line = fallback_line(&s, false).expect("a spoken line");
        assert!(line.to_lowercase().contains("display"), "{line}");
    }
}
