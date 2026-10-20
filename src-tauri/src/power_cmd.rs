//! "Nexus shut down my laptop" and friends.
//!
//! Before this existed `"shut down"` was only a *close-app* verb, so "shut down
//! my laptop" parsed as "close the app called 'my laptop'". Power commands are
//! parsed first, strictly (the target must be the computer itself), confirmed
//! by voice when they end the session, and always preceded by a session
//! snapshot so the next boot can offer to reopen what was open.
//!
//! The actual call happens after a short, cancellable countdown ("say cancel to
//! stop"), through Windows' own `shutdown` so apps with unsaved work still get
//! their "save?" prompts instead of being killed.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerKind {
    Shutdown,
    Restart,
    Sleep,
    Lock,
    SignOut,
}

impl PowerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PowerKind::Shutdown => "shutdown",
            PowerKind::Restart => "restart",
            PowerKind::Sleep => "sleep",
            PowerKind::Lock => "lock",
            PowerKind::SignOut => "signout",
        }
    }

    pub fn from_str(s: &str) -> Option<PowerKind> {
        Some(match s {
            "shutdown" => PowerKind::Shutdown,
            "restart" => PowerKind::Restart,
            "sleep" => PowerKind::Sleep,
            "lock" => PowerKind::Lock,
            "signout" => PowerKind::SignOut,
            _ => return None,
        })
    }

    /// Ends the user's session: asks first, and snapshots what is open.
    pub fn ends_session(self) -> bool {
        matches!(self, PowerKind::Shutdown | PowerKind::Restart | PowerKind::SignOut)
    }

    /// Whether the PC comes back to the same open windows (sleep/lock), so no snapshot is needed.
    pub fn needs_snapshot(self) -> bool {
        self.ends_session()
    }

    /// Seconds between the spoken announcement and the call.
    pub fn countdown_secs(self) -> u64 {
        match self {
            PowerKind::Shutdown | PowerKind::Restart | PowerKind::SignOut => 12,
            PowerKind::Sleep => 4,
            PowerKind::Lock => 1,
        }
    }

    pub fn ask(self) -> &'static str {
        match self {
            PowerKind::Shutdown => "Shut down your laptop, sir? I'll note what's open first so I can offer to reopen it.",
            PowerKind::Restart => "Restart your laptop, sir? I'll note what's open first so I can offer to reopen it.",
            PowerKind::SignOut => sign_out_ask(),
            PowerKind::Sleep => "Putting the laptop to sleep.",
            PowerKind::Lock => "Locking the screen.",
        }
    }

    pub fn verb(self) -> &'static str {
        match self {
            PowerKind::Shutdown => "powering off",
            PowerKind::Restart => "restarting",
            PowerKind::SignOut => "signing out",
            PowerKind::Sleep => "going to sleep",
            PowerKind::Lock => "locking",
        }
    }
}

/// The spoken confirmation for signing out. "Windows" only exists on
/// Windows, so the wording follows the platform.
fn sign_out_ask() -> &'static str {
    #[cfg(target_os = "linux")]
    let line = "Sign you out of this session, sir? I'll note what's open first.";
    #[cfg(not(target_os = "linux"))]
    let line = "Sign you out of Windows, sir? I'll note what's open first.";
    line
}

// ─── The command that does it ─────────────────────────────────────────

/// A command as a program plus its arguments. Pure data so the mapping from
/// a spoken power action to a command line can be unit-tested without
/// touching the machine.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerCommand {
    pub program: &'static str,
    pub args: Vec<String>,
}

/// The Linux command for `kind`.
///
/// * shutdown  -> `systemctl poweroff`
/// * restart   -> `systemctl reboot`
/// * sleep     -> `systemctl suspend`
/// * lock      -> `loginctl lock-session [SESSION]`
/// * sign out  -> `loginctl terminate-session [SESSION]`
///
/// `SESSION` is the logind session id (`XDG_SESSION_ID`). It is optional:
/// loginctl falls back to the caller's own session, which is the same session
/// on a normal desktop login.
#[cfg(target_os = "linux")]
pub fn linux_power_command(kind: PowerKind, session_id: Option<&str>) -> PowerCommand {
    let session = session_id.map(str::trim).filter(|s| !s.is_empty());
    let (program, mut args): (&'static str, Vec<String>) = match kind {
        PowerKind::Shutdown => ("systemctl", vec!["poweroff".into()]),
        PowerKind::Restart => ("systemctl", vec!["reboot".into()]),
        PowerKind::Sleep => ("systemctl", vec!["suspend".into()]),
        PowerKind::Lock => ("loginctl", vec!["lock-session".into()]),
        PowerKind::SignOut => ("loginctl", vec!["terminate-session".into()]),
    };
    if matches!(kind, PowerKind::Lock | PowerKind::SignOut) {
        if let Some(s) = session {
            args.push(s.to_string());
        }
    }
    PowerCommand { program, args }
}

// ─── Parsing ─────────────────────────────────────────────────────────

const POLITE_PREFIXES: &[&str] = &[
    "nexus ",
    "hey nexus ",
    "please ",
    "can you ",
    "could you ",
    "would you ",
    "go ahead and ",
    "i want you to ",
    "i need you to ",
];
const POLITE_SUFFIXES: &[&str] = &[" right now", " immediately", " now", " please", " for me", " nexus"];
/// The only things a power verb may be aimed at.
const COMPUTER_WORDS: &[&str] = &["laptop", "computer", "pc", "system", "machine", "windows", "device", "notebook"];

fn clean(text: &str) -> String {
    let mut t = text.trim().to_lowercase();
    t = t.trim_matches(|c: char| matches!(c, '.' | ',' | '!' | '?' | ' ')).to_string();
    loop {
        let before = t.clone();
        for p in POLITE_PREFIXES {
            if let Some(rest) = t.strip_prefix(p) {
                t = rest.trim().to_string();
            }
        }
        for s in POLITE_SUFFIXES {
            if let Some(rest) = t.strip_suffix(s) {
                t = rest.trim().to_string();
            }
        }
        if t == before {
            return t;
        }
    }
}

/// "my laptop", "the computer", "windows pc", "of windows" -> only computer words remain.
fn is_computer(rest: &str, allow_screen: bool) -> bool {
    let words: Vec<&str> = rest
        .split_whitespace()
        .filter(|w| !matches!(*w, "my" | "the" | "this" | "your" | "of" | "off" | "down" | "out" | "to" | "all"))
        .collect();
    if words.is_empty() {
        return false;
    }
    words
        .iter()
        .all(|w| COMPUTER_WORDS.contains(w) || (allow_screen && matches!(*w, "screen" | "workstation")))
}

/// Strict parse of a power command. `None` for everything else - in
/// particular "shut down notepad" (a close-app command) and "shut down nexus".
/// Whisper hears "shut down" as these (live log 2026-10-15: "send on my laptop", "set down my laptop",
/// "to set down my laptop" - all three fell through to the cloud, which explained how to shut down).
/// Only honoured when what follows is exactly the computer itself, so "set down my coffee" never matches.
const SHUTDOWN_MISHEARINGS: &[&str] = &["set down", "sat down", "shot down", "shut dawn", "shutdown", "send on", "shut it down", "shot dawn", "shutten", "check down", "chuck down", "shuck down", "tick on", "send down"];

fn fix_shutdown_mishearing(t: &str) -> Option<String> {
    let t = t.strip_prefix("to ").unwrap_or(t);
    for bad in SHUTDOWN_MISHEARINGS {
        if let Some(rest) = t.strip_prefix(bad) {
            if rest.starts_with(' ') && is_computer(rest.trim(), false) && rest.split_whitespace().count() <= 3 {
                return Some(format!("shut down{rest}"));
            }
        }
    }
    None
}

pub fn parse(text: &str) -> Option<PowerKind> {
    let t = clean(text);
    if t.is_empty() {
        return None;
    }
    let t = fix_shutdown_mishearing(&t).unwrap_or(t);

    // "put my laptop to sleep", "send the computer to sleep"
    for lead in ["put ", "send "] {
        if let Some(rest) = t.strip_prefix(lead) {
            if let Some(mid) = rest.strip_suffix(" to sleep").or_else(|| rest.strip_suffix(" in sleep mode")) {
                return is_computer(mid, false).then_some(PowerKind::Sleep);
            }
        }
    }

    let table: &[(&str, PowerKind)] = &[
        ("shut down", PowerKind::Shutdown),
        ("shutdown", PowerKind::Shutdown),
        ("power off", PowerKind::Shutdown),
        ("power down", PowerKind::Shutdown),
        ("turn off", PowerKind::Shutdown),
        ("switch off", PowerKind::Shutdown),
        ("restart", PowerKind::Restart),
        ("reboot", PowerKind::Restart),
        ("sleep", PowerKind::Sleep),
        ("hibernate", PowerKind::Sleep),
        ("lock", PowerKind::Lock),
        ("sign out", PowerKind::SignOut),
        ("log out", PowerKind::SignOut),
        ("log off", PowerKind::SignOut),
        ("sign off", PowerKind::SignOut),
    ];
    for (verb, kind) in table {
        if let Some(rest) = t.strip_prefix(verb) {
            // The verb must end at a word boundary ("locked" is not "lock").
            if !rest.is_empty() && !rest.starts_with(' ') {
                continue;
            }
            if is_computer(rest.trim(), *kind == PowerKind::Lock) {
                return Some(*kind);
            }
        }
    }
    None
}

// ─── Loose match (STT mishearings) + learned phrases ─────────────────

/// Words that make "<verb> down/on/off my laptop" something other than a shutdown.
const BENIGN_VERBS: &[&str] = &[
    "turn", "switch", "power", "boot", "start", "wake", "plug", "charge", "clean", "slow", "calm", "break", "bring",
    "back", "update", "write", "track", "wipe", "dust", "open", "close", "lock", "sleep", "restart", "reboot", "log",
    "sign", "hang", "hold", "keep", "leave", "look", "carry", "pick", "take", "get", "put", "tidy", "shut",
    "show", "cut", "kick", "clear", "knock", "hand", "tear",
];
/// A sentence that starts with one of these is a question, not a command.
const QUESTION_WORDS: &[&str] = &[
    "how", "what", "why", "can", "do", "does", "did", "is", "are", "where", "who", "when", "which", "will", "would",
    "could", "should", "am", "tell", "explain",
];

/// Whisper keeps mishearing "shut down my laptop" ("send on", "set down", "check down", "tick on", ...).
/// A fixed list always misses the next variant, so this is structural: a short sentence of the form
/// `<one or two words> <down|off> <my laptop>` whose first words are not an ordinary verb. ("on" is NOT
/// used: "music on my laptop" is an ordinary phrase; the two logged "on" mishearings are in the strict list.)
/// Weak evidence: the caller must ASK before powering off, never obey silently and never ask the cloud.
pub fn parse_loose(text: &str) -> Option<PowerKind> {
    let t = clean(&text.replace(',', " "));
    let t = t.strip_prefix("to ").unwrap_or(&t);
    let tokens: Vec<&str> = t.split_whitespace().collect();
    if tokens.len() < 3 || tokens.len() > 5 {
        return None;
    }
    if QUESTION_WORDS.contains(&tokens[0]) {
        return None;
    }
    let particle = tokens.iter().position(|w| matches!(*w, "down" | "off"))?;
    if particle == 0 || particle > 2 {
        return None;
    }
    // A misheard verb is a plain word; "what's"/"it's" style contractions are never one.
    if !tokens[..particle].iter().all(|w| w.chars().all(|c| c.is_ascii_alphabetic())) {
        return None;
    }
    if tokens[..particle].iter().any(|w| BENIGN_VERBS.contains(w)) {
        return None;
    }
    let rest = tokens[particle + 1..].join(" ");
    is_computer(&rest, false).then_some(PowerKind::Shutdown)
}

/// The mishearing the user was just asked about (so a "yes" can teach it). Expires with the question.
static PENDING_LOOSE: std::sync::Mutex<Option<(String, std::time::Instant)>> = std::sync::Mutex::new(None);
const PENDING_LOOSE_TTL: std::time::Duration = std::time::Duration::from_secs(45);
const MAX_ALIASES: usize = 50;

fn alias_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("power_aliases.json")
}

/// Normalised form used to store and compare a heard phrase.
pub fn alias_key(text: &str) -> String {
    clean(&text.replace(',', " ")).split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn load_aliases(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(alias_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

/// Has the user confirmed this exact (normalised) phrase as "shut down" before?
pub fn is_learned_shutdown(dir: &std::path::Path, text: &str) -> bool {
    let key = alias_key(text);
    !key.is_empty() && load_aliases(dir).iter().any(|a| *a == key)
}

/// Remember a confirmed mishearing (local only, capped, newest kept). Returns whether it was stored.
pub fn add_alias(dir: &std::path::Path, text: &str) -> bool {
    let key = alias_key(text);
    // Only things the loose rule would have caught are worth storing (never an arbitrary sentence).
    if key.is_empty() || parse_loose(&key).is_none() {
        return false;
    }
    let mut all = load_aliases(dir);
    if all.contains(&key) {
        return true;
    }
    all.push(key);
    if all.len() > MAX_ALIASES {
        let drop = all.len() - MAX_ALIASES;
        all.drain(..drop);
    }
    let Ok(json) = serde_json::to_string_pretty(&all) else { return false };
    let _ = std::fs::create_dir_all(dir);
    let tmp = dir.join("power_aliases.json.tmp");
    std::fs::write(&tmp, json).is_ok() && std::fs::rename(&tmp, alias_path(dir)).is_ok()
}

/// Remember which heard phrase the "Shut down your laptop?" question is about.
pub fn note_loose_question(text: &str) {
    if let Ok(mut g) = PENDING_LOOSE.lock() {
        *g = Some((text.to_string(), std::time::Instant::now() + PENDING_LOOSE_TTL));
    }
}

/// The user said yes: teach the phrase that triggered the question (if it is still fresh).
pub fn learn_pending_alias(dir: &std::path::Path) -> bool {
    let taken = PENDING_LOOSE.lock().ok().and_then(|mut g| g.take());
    match taken {
        Some((phrase, until)) if until > std::time::Instant::now() => add_alias(dir, &phrase),
        _ => false,
    }
}

// ─── Countdown / execution ───────────────────────────────────────────

/// Bumped on every new countdown and on cancel; a countdown only fires if its
/// generation is still current.
static GENERATION: AtomicU64 = AtomicU64::new(0);
static ACTIVE: AtomicU64 = AtomicU64::new(0);

/// True while a power command is counting down and can still be cancelled.
pub fn countdown_active() -> bool {
    ACTIVE.load(Ordering::SeqCst) != 0
}

/// Abort a pending power command. Returns whether one was pending.
pub fn cancel() -> bool {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    ACTIVE.swap(0, Ordering::SeqCst) != 0
}

/// Whether a reply means "stop the power command".
pub fn is_cancel_phrase(text: &str) -> bool {
    let t = text.trim().to_lowercase().replace(',', "");
    let t = t.trim_matches(|c: char| matches!(c, '.' | '!' | '?' | ' '));
    let t = t.strip_prefix("nexus ").unwrap_or(t);
    matches!(
        t,
        "cancel"
            | "stop"
            | "abort"
            | "wait"
            | "no"
            | "don't"
            | "do not"
            | "never mind"
            | "cancel that"
            | "stop that"
            | "cancel shutdown"
            | "cancel the shutdown"
            | "don't shut down"
            | "do not shut down"
            | "stop shutdown"
            | "cancel restart"
            | "abort shutdown"
    )
}

/// Start a power sequence that runs its own steps (say goodbye, close apps, ...) and needs the same
/// cancel flag as the countdown. Returns its generation; `cancel()` (or a newer command) invalidates it.
pub fn begin() -> u64 {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    ACTIVE.store(generation, Ordering::SeqCst);
    generation
}

/// Is this still the live power command (not cancelled, not superseded)?
pub fn is_current(generation: u64) -> bool {
    GENERATION.load(Ordering::SeqCst) == generation
}

/// The sequence reached its end (or gave up): it is no longer cancellable.
pub fn finish(generation: u64) {
    if is_current(generation) {
        ACTIVE.store(0, Ordering::SeqCst);
    }
}

/// The last window in which "cancel" still stops a shutdown after the apps have been closed.
pub const FINAL_CANCEL_WINDOW_SECS: u64 = 3;

/// Call Windows now (no countdown). Used at the end of a sequence that already waited and checked.
pub fn execute_now(kind: PowerKind) -> Result<(), String> {
    println!("[POWER] executing {}", kind.as_str());
    execute(kind)
}

/// What NEXUS says as the shutdown starts: "Good night" late, a plain goodbye otherwise
/// (both are pre-rendered fixed lines). `noted` = a session snapshot was saved for tomorrow's offer.
pub fn goodbye_line(hour: u32, name: Option<&str>, friend: bool, noted: bool) -> String {
    let addr = crate::persona::greet_address(name, friend);
    let base = match hour {
        20..=23 | 0..=4 => format!("Good night{addr}."),
        _ => format!("Shutting down{addr}. See you soon."),
    };
    if noted {
        format!("I've noted what's open. {base}")
    } else {
        base
    }
}

/// Start the countdown, then call Windows. Returns immediately.
pub fn schedule(kind: PowerKind) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    ACTIVE.store(generation, Ordering::SeqCst);
    let secs = kind.countdown_secs();
    std::thread::Builder::new()
        .name("power-countdown".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(secs));
            if GENERATION.load(Ordering::SeqCst) != generation {
                println!("[POWER] {} cancelled", kind.as_str());
                return;
            }
            ACTIVE.store(0, Ordering::SeqCst);
            println!("[POWER] executing {}", kind.as_str());
            if let Err(e) = execute(kind) {
                tracing::error!("power command {} failed: {e}", kind.as_str());
            }
        })
        .ok();
}

#[cfg(target_os = "windows")]
fn execute(kind: PowerKind) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const NO_WINDOW: u32 = 0x0800_0000;
    let run = |prog: &str, args: &[&str]| -> Result<(), String> {
        std::process::Command::new(prog)
            .args(args)
            .creation_flags(NO_WINDOW)
            .status()
            .map_err(|e| e.to_string())
            .and_then(|s| if s.success() { Ok(()) } else { Err(format!("{prog} exited with {s}")) })
    };
    match kind {
        // No /f: apps with unsaved work get to ask instead of being killed.
        PowerKind::Shutdown => run("shutdown", &["/s", "/t", "0"]),
        PowerKind::Restart => run("shutdown", &["/r", "/t", "0"]),
        PowerKind::SignOut => run("shutdown", &["/l"]),
        PowerKind::Lock => run("rundll32.exe", &["user32.dll,LockWorkStation"]),
        PowerKind::Sleep => {
            use windows::Win32::Foundation::BOOLEAN;
            use windows::Win32::System::Power::SetSuspendState;
            // SAFETY: plain Win32 call with no pointers.
            let ok = unsafe { SetSuspendState(BOOLEAN(0), BOOLEAN(0), BOOLEAN(0)) };
            if ok.0 != 0 { Ok(()) } else { Err("SetSuspendState failed".into()) }
        }
    }
}

#[cfg(target_os = "linux")]
fn execute(kind: PowerKind) -> Result<(), String> {
    // systemd owns the machine's power state on Linux; logind owns the
    // session (lock / sign out). Both are what GNOME and COSMIC honour.
    let session = crate::power::session_id();
    let cmd = linux_power_command(kind, session.as_deref());
    let status = std::process::Command::new(cmd.program)
        .args(&cmd.args)
        .status()
        .map_err(|e| format!("{}: {e}", cmd.program))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} {} exited with {status}", cmd.program, cmd.args.join(" ")))
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn execute(_kind: PowerKind) -> Result<(), String> {
    Err("power commands are only supported on Windows and Linux".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_phrases() {
        for t in [
            "shut down my laptop",
            "Shut down the laptop.",
            "nexus shut down my laptop",
            "please shut down my computer",
            "shutdown my pc",
            "power off my laptop",
            "power down the system",
            "turn off my laptop",
            "switch off the computer",
            "can you shut down my laptop now",
            "shut down windows",
            "shut down the laptop please",
        ] {
            assert_eq!(parse(t), Some(PowerKind::Shutdown), "{t}");
        }
    }

    #[test]
    fn other_power_kinds() {
        assert_eq!(parse("restart my laptop"), Some(PowerKind::Restart));
        assert_eq!(parse("reboot the computer"), Some(PowerKind::Restart));
        assert_eq!(parse("put my laptop to sleep"), Some(PowerKind::Sleep));
        assert_eq!(parse("sleep my laptop"), Some(PowerKind::Sleep));
        assert_eq!(parse("lock my laptop"), Some(PowerKind::Lock));
        assert_eq!(parse("lock the screen"), Some(PowerKind::Lock));
        assert_eq!(parse("sign out of windows"), Some(PowerKind::SignOut));
        assert_eq!(parse("log off my computer"), Some(PowerKind::SignOut));
    }

    #[test]
    fn app_and_unrelated_commands_are_not_power_commands() {
        for t in [
            "shut down notepad",
            "shut down nexus",
            "shut down the music",
            "close notepad",
            "turn off the lights",
            "turn off bluetooth",
            "restart nexus",
            "restart the download",
            "lock the door",
            "sleep",
            "shut down",
            "reboot",
            "log out",
            "open my laptop settings",
            "locked my laptop yesterday",
            "",
        ] {
            assert_eq!(parse(t), None, "{t:?}");
        }
        // The screen is only a lock target.
        assert_eq!(parse("shut down the screen"), None);
    }

    #[test]
    fn only_session_ending_commands_ask_and_snapshot() {
        assert!(PowerKind::Shutdown.ends_session() && PowerKind::Restart.ends_session() && PowerKind::SignOut.ends_session());
        assert!(!PowerKind::Sleep.ends_session() && !PowerKind::Lock.ends_session());
        assert!(PowerKind::Shutdown.needs_snapshot() && !PowerKind::Lock.needs_snapshot());
    }

    #[test]
    fn cancel_phrases_are_strict() {
        for t in ["cancel", "stop", "Nexus, stop.", "don't shut down", "never mind", "abort shutdown"] {
            assert!(is_cancel_phrase(t), "{t}");
        }
        for t in ["open chrome", "yes", "cancel my meeting tomorrow", "stop the music please"] {
            assert!(!is_cancel_phrase(t), "{t}");
        }
    }

    #[test]
    fn a_countdown_can_be_cancelled_and_a_new_one_supersedes_it() {
        // Uses a kind whose execution is never reached: cancel before the delay elapses.
        let _ = cancel();
        assert!(!countdown_active());
        // Simulate the bookkeeping without spawning a real power call.
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        ACTIVE.store(generation, Ordering::SeqCst);
        assert!(countdown_active());
        assert!(cancel());
        assert!(!countdown_active());
        assert!(GENERATION.load(Ordering::SeqCst) != generation, "a cancelled countdown must not fire");
        assert!(!cancel(), "nothing left to cancel");
    }

    #[test]
    fn whisper_mishearings_of_shut_down_still_shut_down_but_only_for_the_computer() {
        for t in ["send on my laptop.", "set down my laptop.", "to set down my laptop.", "shot down the computer", "Nexus set down my pc"] {
            assert_eq!(parse(t), Some(PowerKind::Shutdown), "{t}");
        }
        for t in ["set down my coffee", "send on my email", "set down the phone", "send on the file to my laptop", "set down"] {
            assert_eq!(parse(t), None, "{t}");
        }
    }

    #[test]
    fn goodbye_is_good_night_late_and_a_plain_goodbye_by_day() {
        assert_eq!(goodbye_line(23, Some("Lakshya"), false, false), "Good night, sir.");
        assert_eq!(goodbye_line(2, None, false, false), "Good night, sir.");
        assert_eq!(goodbye_line(14, None, false, false), "Shutting down, sir. See you soon.");
        assert_eq!(goodbye_line(21, Some("Lakshya"), true, false), "Good night, Lakshya.");
        assert_eq!(goodbye_line(21, None, false, true), "I've noted what's open. Good night, sir.");
    }

    #[test]
    fn kind_round_trips() {
        for k in [PowerKind::Shutdown, PowerKind::Restart, PowerKind::Sleep, PowerKind::Lock, PowerKind::SignOut] {
            assert_eq!(PowerKind::from_str(k.as_str()), Some(k));
        }
        assert_eq!(PowerKind::from_str("nope"), None);
    }

    #[test]
    fn the_sign_out_question_names_the_platform() {
        // "Windows" only exists on Windows.
        #[cfg(target_os = "linux")]
        assert!(PowerKind::SignOut.ask().contains("session"), "{}", PowerKind::SignOut.ask());
        #[cfg(not(target_os = "linux"))]
        assert!(PowerKind::SignOut.ask().contains("Windows"), "{}", PowerKind::SignOut.ask());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_power_commands_map_to_systemd_and_logind() {
        // Shutdown / restart / sleep go to systemd.
        assert_eq!(
            linux_power_command(PowerKind::Shutdown, None),
            PowerCommand { program: "systemctl", args: vec!["poweroff".to_string()] }
        );
        assert_eq!(
            linux_power_command(PowerKind::Restart, None),
            PowerCommand { program: "systemctl", args: vec!["reboot".to_string()] }
        );
        assert_eq!(
            linux_power_command(PowerKind::Sleep, None),
            PowerCommand { program: "systemctl", args: vec!["suspend".to_string()] }
        );
        // Lock / sign out act on the logind session.
        assert_eq!(
            linux_power_command(PowerKind::Lock, Some("3")),
            PowerCommand { program: "loginctl", args: vec!["lock-session".to_string(), "3".to_string()] }
        );
        assert_eq!(
            linux_power_command(PowerKind::SignOut, Some("3")),
            PowerCommand { program: "loginctl", args: vec!["terminate-session".to_string(), "3".to_string()] }
        );
        // Without a session id loginctl acts on the caller's own session.
        assert_eq!(
            linux_power_command(PowerKind::Lock, None),
            PowerCommand { program: "loginctl", args: vec!["lock-session".to_string()] }
        );
        assert_eq!(
            linux_power_command(PowerKind::SignOut, Some("  ")),
            PowerCommand { program: "loginctl", args: vec!["terminate-session".to_string()] }
        );
    }
}


#[cfg(test)]
mod loose_tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nexus_power_alias_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// Every real mishearing of "shut down my laptop" seen in missed_intents.jsonl.
    const REAL_MISHEARINGS: &[&str] = &[
        "send on my laptop.",
        "set down my laptop.",
        "to set down my laptop.",
        "check down my laptop.",
        "tick on my laptop.",
        "send down my laptop.",
    ];

    #[test]
    fn the_real_mishearings_are_shutdowns() {
        for t in REAL_MISHEARINGS {
            assert_eq!(parse(t).or_else(|| parse_loose(t)), Some(PowerKind::Shutdown), "{t}");
        }
        // the ones with "down"/"off" are also caught structurally (no list needed)
        for t in ["set down my laptop.", "to set down my laptop.", "check down my laptop.", "send down my laptop."] {
            assert_eq!(parse_loose(t), Some(PowerKind::Shutdown), "{t}");
        }
        // a variant nobody has logged yet still matches (the point of a structural rule)
        assert_eq!(parse_loose("Nexus, shock down my computer"), Some(PowerKind::Shutdown));
    }

    #[test]
    fn ordinary_sentences_never_match() {
        for t in [
            "turn on my laptop",
            "slow down my laptop",
            "calm down my computer",
            "bring down my laptop",
            "charge my laptop",
            "how do i shut down my laptop",
            "what is a laptop",
            "can you shut down my laptop",
            "open my laptop",
            "close my laptop",
            "restart my laptop",
            "lock my laptop",
            "put my laptop down",
            "write down my laptop",
            "wake up my computer",
            "down my laptop",
            "shut down notepad",
            "set down the phone",
            "send on the file to my laptop",
            "my laptop is slow",
            "set down my coffee",
            "send on my email",
            // ordinary "<noun> on/off my laptop" phrases
            "music on my laptop",
            "files on my laptop",
            "play music on my laptop",
            "what's on my laptop",
            "show off my laptop",
            "get off my laptop",
            // only the question-word / contraction guards stop these
            "what down my laptop",
            "is down my computer",
            "what's down my laptop",
            // particle too far from the start: a longer sentence, not a misheard two-word verb
            "pour hot water down laptop",
            "",
        ] {
            assert_eq!(parse_loose(t), None, "{t:?} must not be a loose shutdown");
        }
    }

    #[test]
    fn logged_mishearings_are_obeyed_but_unseen_variants_are_only_loose() {
        assert_eq!(parse("shut down my laptop"), Some(PowerKind::Shutdown));
        // all six the owner actually produced are strict (obeyed at once)
        for t in REAL_MISHEARINGS {
            assert_eq!(parse(t), Some(PowerKind::Shutdown), "{t}");
        }
        // a variant nobody has logged yet is NOT a strict match: it is asked about first
        assert_eq!(parse("shock down my laptop."), None);
        assert_eq!(parse_loose("shock down my laptop."), Some(PowerKind::Shutdown));
    }

    #[test]
    fn a_yes_teaches_the_phrase_and_nothing_else() {
        let d = tmp("learn");
        assert!(!is_learned_shutdown(&d, "shock down my laptop."));
        note_loose_question("shock down my laptop.");
        assert!(learn_pending_alias(&d));
        assert!(is_learned_shutdown(&d, "Shock down my laptop"), "case/punctuation-insensitive");
        // consumed: a second yes (e.g. the strict 'shut down anyway?' flow) teaches nothing
        assert!(!learn_pending_alias(&d));
        // an arbitrary sentence can never be stored
        assert!(!add_alias(&d, "open the pod bay doors"));
        assert!(!is_learned_shutdown(&d, "open the pod bay doors"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn aliases_are_capped_newest_kept() {
        let d = tmp("cap");
        // distinct all-letter "verbs" (digits are not a plain word): zza, zzb, ...
        let word = |i: usize| format!("zz{}{}", (b'a' + (i / 26) as u8) as char, (b'a' + (i % 26) as u8) as char);
        for i in 0..(MAX_ALIASES + 10) {
            assert!(add_alias(&d, &format!("{} down my laptop", word(i))));
        }
        let all = load_aliases(&d);
        assert_eq!(all.len(), MAX_ALIASES);
        assert!(all.last().unwrap().starts_with(&word(MAX_ALIASES + 9)));
        let _ = std::fs::remove_dir_all(&d);
    }
}
