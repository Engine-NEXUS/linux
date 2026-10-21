//! Action Router, keyboard tier (shortcut brain, phase E1).
//!
//! `run` turns "perform ACTION in the app that is in front" into at most one chord sent, with the safety
//! gates the legacy PowerShell-`SendKeys` path never had:
//!
//!   1. there is a foreground window                       (else `NoForeground`)
//!   2. it is not a protected window (banking/password…)   (`Blocked`)
//!   3. it is not an elevated app we cannot type into      (`Elevated`; UIPI would silently eat the keys)
//!   4. the Shortcut Map has a chord for (exe, action)     (`NoBinding` / `BadArg`)
//!   5. no stop was requested                              (`Stopped`)
//!   6. **the foreground is still the SAME window right before sending** (`ForegroundChanged`) —
//!      keystrokes go to whoever is in front, so a window switch between decision and send must send nothing
//!   7. send one chord (one batched call), then optionally verify an observable effect.
//!
//! All OS access is behind `Env`, so the gates — above all #6 "0 wrong-window sends" — are unit-tested
//! with a fake environment.

use super::keys::Chord;
use super::map::{Action, Lookup, Scope, ShortcutMap};

/// What the router knows about the window in front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreground {
    /// Lower-case executable name, e.g. `brave.exe`.
    pub exe: String,
    pub title: String,
    /// Opaque window identity (HWND value). Two different windows of the same app differ.
    pub hwnd: isize,
    /// The window's process is elevated (running as admin).
    pub elevated: bool,
}

/// The OS surface the router needs. The real implementation is `exec::WinEnv`.
pub trait Env {
    fn foreground(&self) -> Option<Foreground>;
    fn send(&self, chord: &Chord) -> Result<(), String>;
    fn sleep_ms(&self, ms: u64);
    fn now_ms(&self) -> u64;
    /// A ghost stop (voice "stop", Esc) is pending.
    fn stop_requested(&self) -> bool;
    fn self_elevated(&self) -> bool;
    /// Banking / password-manager / crypto-wallet windows (live::safety denylist).
    fn is_protected(&self, exe: &str, title: &str) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Abort {
    NoForeground,
    Blocked,
    Elevated,
    ForegroundChanged { expected: String, actual: String },
    Stopped,
}

/// Did the action visibly take effect?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verified {
    /// No cheap observable exists for this action (copy, save, scroll…).
    NotChecked,
    /// The window title (or foreground window) changed after the keys — the action took effect.
    Yes,
    /// Keys were sent but no change was observed. NOT a failure (e.g. "tab 2" while already on tab 2).
    Unconfirmed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Sent { chord: String, scope: Scope, verified: Verified, ms: u64 },
    /// No chord for this (app, action). `known_app`: the app is in a seeded family.
    NoBinding { known_app: bool, exe: String },
    BadArg(String),
    Aborted(Abort),
    Failed(String),
}

/// Milliseconds to wait for a title change after sending, for actions whose effect shows in the title.
/// `None` = nothing cheap to observe, so no waiting at all.
pub fn verify_budget_ms(action: Action) -> Option<u64> {
    match action {
        Action::NewTab
        | Action::CloseTab
        | Action::ReopenTab
        | Action::NextTab
        | Action::PrevTab
        | Action::GotoTab
        | Action::GoBack
        | Action::GoForward => Some(700),
        _ => None,
    }
}

const VERIFY_POLL_MS: u64 = 40;

pub fn run(env: &dyn Env, map: &ShortcutMap, action: Action, arg: Option<u32>) -> Outcome {
    let t0 = env.now_ms();

    // 1-3: who is in front, and may we act on it at all?
    let Some(fg) = env.foreground() else {
        return Outcome::Aborted(Abort::NoForeground);
    };
    if env.is_protected(&fg.exe, &fg.title) {
        return Outcome::Aborted(Abort::Blocked);
    }
    if fg.elevated && !env.self_elevated() {
        return Outcome::Aborted(Abort::Elevated);
    }

    // 4: is there a chord?
    let (chord, scope) = match map.lookup(&fg.exe, action, arg) {
        Lookup::Found { chord, scope, .. } => (chord, scope),
        Lookup::NoBinding { known_app, .. } => {
            return Outcome::NoBinding { known_app, exe: fg.exe.clone() };
        }
        Lookup::BadArg(msg) => return Outcome::BadArg(msg),
    };

    // 5: a pending stop wins.
    if env.stop_requested() {
        return Outcome::Aborted(Abort::Stopped);
    }

    // 6: THE gate — still the same window, immediately before the keys go out.
    match env.foreground() {
        None => return Outcome::Aborted(Abort::NoForeground),
        Some(now) => {
            if now.hwnd != fg.hwnd || !now.exe.eq_ignore_ascii_case(&fg.exe) {
                return Outcome::Aborted(Abort::ForegroundChanged { expected: fg.exe, actual: now.exe });
            }
        }
    }
    if env.stop_requested() {
        return Outcome::Aborted(Abort::Stopped);
    }

    // 7: send, then (cheaply) verify.
    if let Err(e) = env.send(&chord) {
        return Outcome::Failed(e);
    }

    let verified = match verify_budget_ms(action) {
        None => Verified::NotChecked,
        Some(budget) => {
            let start = env.now_ms();
            let mut seen_change = false;
            loop {
                env.sleep_ms(VERIFY_POLL_MS);
                match env.foreground() {
                    // Foreground vanished or moved to another window (closing the last tab closes the
                    // window): the keys definitely did something.
                    None => {
                        seen_change = true;
                        break;
                    }
                    Some(now) => {
                        if now.hwnd != fg.hwnd || now.title != fg.title {
                            seen_change = true;
                            break;
                        }
                    }
                }
                if env.now_ms().saturating_sub(start) >= budget {
                    break;
                }
            }
            if seen_change { Verified::Yes } else { Verified::Unconfirmed }
        }
    };

    Outcome::Sent {
        chord: chord.display(),
        scope,
        verified,
        ms: env.now_ms().saturating_sub(t0),
    }
}

/// "brave.exe" → "Brave".
fn app_display(exe: &str) -> String {
    let base = exe.trim_end_matches(".exe");
    let mut c = base.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => "that app".to_string(),
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn noun(action: Action) -> &'static str {
    match action {
        Action::GotoTab => "tab-switching",
        Action::NewTab => "new-tab",
        Action::CloseTab => "close-tab",
        Action::ReopenTab => "reopen-tab",
        Action::NextTab => "next-tab",
        Action::PrevTab => "previous-tab",
        Action::Refresh => "refresh",
        Action::GoBack => "back",
        Action::GoForward => "forward",
        Action::AddressBar => "address-bar",
        Action::FindInPage => "find",
        Action::Save => "save",
        Action::Copy => "copy",
        Action::Paste => "paste",
        Action::Cut => "cut",
        Action::Undo => "undo",
        Action::Redo => "redo",
        Action::SelectAll => "select-all",
        Action::ScrollDown | Action::ScrollUp => "scroll",
        Action::ZoomIn | Action::ZoomOut => "zoom",
        Action::SwitchWindow => "window-switching",
    }
}

/// Why nothing (useful) happened, as a plain clause with NO honorific and no final stop, so callers can
/// embed it in their own sentence ("Couldn't switch tabs, sir: {reason}") without doubling "sir".
/// `None` for `Sent`.
pub fn reason(action: Action, out: &Outcome) -> Option<String> {
    match out {
        Outcome::Sent { .. } => None,
        Outcome::NoBinding { known_app, exe } => Some(if *known_app {
            format!("{} has no {} shortcut", app_display(exe), noun(action))
        } else {
            format!("I don't know a {} shortcut for {}", noun(action), app_display(exe))
        }),
        Outcome::BadArg(msg) => Some(format!("which tab? {msg}")),
        Outcome::Aborted(a) => Some(match a {
            Abort::NoForeground => "I can't tell which window is in front".to_string(),
            Abort::Blocked => "that window is protected, I won't send keys to it".to_string(),
            Abort::Elevated => "that app runs as administrator, I can't send it keys".to_string(),
            Abort::ForegroundChanged { .. } => "the window changed, I pressed nothing".to_string(),
            Abort::Stopped => "stopped".to_string(),
        }),
        Outcome::Failed(e) => Some(format!("couldn't send the shortcut: {e}")),
    }
}

/// Spoken result for direct speech. `Ok` = success line (identical wording to the legacy executor so nothing
/// the user hears changes); `Err` = a clear spoken reason, never silence.
pub fn spoken(action: Action, arg: Option<u32>, out: &Outcome) -> Result<String, String> {
    match reason(action, out) {
        None => Ok(match (action, arg) {
            (Action::GotoTab, Some(n)) => format!("Switched to tab {n}, sir."),
            _ => format!("{} sir.", capitalize(action.label())),
        }),
        Some(r) => Err(format!("{}, sir.", capitalize(&r))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::map::SEED;
    use std::cell::{Cell, RefCell};

    /// Scriptable environment: `fg` is consumed one answer per `foreground()` call (the last repeats).
    struct FakeEnv {
        fg: RefCell<Vec<Option<Foreground>>>,
        sent: RefCell<Vec<String>>,
        send_err: Option<String>,
        stop: Cell<bool>,
        self_elev: bool,
        protected_exe: Option<&'static str>,
        clock: Cell<u64>,
        fg_calls: Cell<usize>,
    }

    fn win(exe: &str, title: &str, hwnd: isize) -> Foreground {
        Foreground { exe: exe.into(), title: title.into(), hwnd, elevated: false }
    }

    impl FakeEnv {
        fn new(seq: Vec<Option<Foreground>>) -> Self {
            Self {
                fg: RefCell::new(seq),
                sent: RefCell::new(vec![]),
                send_err: None,
                stop: Cell::new(false),
                self_elev: false,
                protected_exe: None,
                clock: Cell::new(0),
                fg_calls: Cell::new(0),
            }
        }
        /// The same window forever.
        fn steady(f: Foreground) -> Self {
            Self::new(vec![Some(f)])
        }
        fn sent(&self) -> Vec<String> {
            self.sent.borrow().clone()
        }
    }

    impl Env for FakeEnv {
        fn foreground(&self) -> Option<Foreground> {
            self.fg_calls.set(self.fg_calls.get() + 1);
            let mut q = self.fg.borrow_mut();
            if q.len() > 1 { q.remove(0) } else { q[0].clone() }
        }
        fn send(&self, chord: &Chord) -> Result<(), String> {
            if let Some(e) = &self.send_err {
                return Err(e.clone());
            }
            self.sent.borrow_mut().push(chord.display());
            Ok(())
        }
        fn sleep_ms(&self, ms: u64) {
            self.clock.set(self.clock.get() + ms);
        }
        fn now_ms(&self) -> u64 {
            self.clock.get()
        }
        fn stop_requested(&self) -> bool {
            self.stop.get()
        }
        fn self_elevated(&self) -> bool {
            self.self_elev
        }
        fn is_protected(&self, exe: &str, _title: &str) -> bool {
            self.protected_exe.map(|p| p == exe).unwrap_or(false)
        }
    }

    #[test]
    fn happy_path_sends_exactly_one_chord_and_verifies_a_title_change() {
        // decision, pre-send gate, then the title changes on the first poll.
        let env = FakeEnv::new(vec![
            Some(win("brave.exe", "Inbox - Brave", 7)),
            Some(win("brave.exe", "Inbox - Brave", 7)),
            Some(win("brave.exe", "New Tab - Brave", 7)),
        ]);
        let out = run(&env, &SEED, Action::NewTab, None);
        assert_eq!(env.sent(), vec!["Ctrl+T"]);
        match out {
            Outcome::Sent { chord, verified, scope, .. } => {
                assert_eq!(chord, "Ctrl+T");
                assert_eq!(verified, Verified::Yes);
                assert_eq!(scope, Scope::Family("browser".into()));
            }
            other => panic!("expected Sent, got {other:?}"),
        }
    }

    #[test]
    fn unchanged_title_is_unconfirmed_not_a_failure() {
        let env = FakeEnv::steady(win("brave.exe", "Same - Brave", 7));
        let out = run(&env, &SEED, Action::GotoTab, Some(2)); // already on tab 2: title cannot change
        assert_eq!(env.sent(), vec!["Ctrl+2"]);
        assert!(matches!(out, Outcome::Sent { verified: Verified::Unconfirmed, .. }), "{out:?}");
        // It waited its budget, and no longer.
        assert!(env.now_ms() >= 700 && env.now_ms() < 700 + 2 * VERIFY_POLL_MS + 1, "{}", env.now_ms());
    }

    /// THE GATE: the window changes between decision and send => nothing is sent.
    #[test]
    fn foreground_change_before_send_sends_nothing() {
        let env = FakeEnv::new(vec![
            Some(win("brave.exe", "Inbox", 7)),   // decision
            Some(win("slack.exe", "General", 9)), // user alt-tabbed before the keys went out
        ]);
        let out = run(&env, &SEED, Action::NewTab, None);
        assert_eq!(
            out,
            Outcome::Aborted(Abort::ForegroundChanged { expected: "brave.exe".into(), actual: "slack.exe".into() })
        );
        assert!(env.sent().is_empty(), "0 wrong-window sends");
    }

    #[test]
    fn a_different_window_of_the_same_app_also_aborts() {
        let env = FakeEnv::new(vec![Some(win("brave.exe", "A", 7)), Some(win("brave.exe", "B", 8))]);
        assert!(matches!(run(&env, &SEED, Action::Refresh, None), Outcome::Aborted(Abort::ForegroundChanged { .. })));
        assert!(env.sent().is_empty());
    }

    #[test]
    fn losing_the_foreground_entirely_sends_nothing() {
        let env = FakeEnv::new(vec![Some(win("brave.exe", "A", 7)), None]);
        assert_eq!(run(&env, &SEED, Action::Refresh, None), Outcome::Aborted(Abort::NoForeground));
        assert!(env.sent().is_empty());
        let none = FakeEnv::new(vec![None]);
        assert_eq!(run(&none, &SEED, Action::Refresh, None), Outcome::Aborted(Abort::NoForeground));
    }

    #[test]
    fn protected_windows_get_no_keys_and_are_not_even_looked_up() {
        let mut env = FakeEnv::steady(win("1password.exe", "Vault", 3));
        env.protected_exe = Some("1password.exe");
        assert_eq!(run(&env, &SEED, Action::Paste, None), Outcome::Aborted(Abort::Blocked));
        assert!(env.sent().is_empty());
    }

    #[test]
    fn elevated_targets_are_refused_unless_we_are_elevated_too() {
        let mut f = win("taskmgr.exe", "Task Manager", 4);
        f.elevated = true;
        let env = FakeEnv::steady(f.clone());
        assert_eq!(run(&env, &SEED, Action::Copy, None), Outcome::Aborted(Abort::Elevated));
        assert!(env.sent().is_empty());

        let mut both = FakeEnv::steady(f);
        both.self_elev = true;
        assert!(matches!(run(&both, &SEED, Action::Copy, None), Outcome::Sent { .. }));
    }

    #[test]
    fn stop_requested_wins_at_either_checkpoint() {
        let env = FakeEnv::steady(win("brave.exe", "A", 7));
        env.stop.set(true);
        assert_eq!(run(&env, &SEED, Action::NewTab, None), Outcome::Aborted(Abort::Stopped));
        assert!(env.sent().is_empty());
    }

    #[test]
    fn no_binding_reports_known_vs_unknown_apps_and_sends_nothing() {
        let env = FakeEnv::steady(win("notepad.exe", "Untitled", 5));
        assert_eq!(
            run(&env, &SEED, Action::Refresh, None),
            Outcome::NoBinding { known_app: true, exe: "notepad.exe".into() }
        );
        let env = FakeEnv::steady(win("randomapp.exe", "x", 6));
        assert_eq!(
            run(&env, &SEED, Action::NewTab, None),
            Outcome::NoBinding { known_app: false, exe: "randomapp.exe".into() }
        );
        assert!(env.sent().is_empty());
    }

    #[test]
    fn consoles_never_receive_global_edit_shortcuts() {
        // Ctrl+C in a console is an interrupt: must be NoBinding, never a send.
        for exe in ["cmd.exe", "powershell.exe", "pwsh.exe", "windowsterminal.exe"] {
            for a in [Action::Cut, Action::Undo, Action::Save, Action::SelectAll] {
                let env = FakeEnv::steady(win(exe, "term", 2));
                assert!(matches!(run(&env, &SEED, a, None), Outcome::NoBinding { known_app: true, .. }), "{exe} {a:?}");
                assert!(env.sent().is_empty());
            }
        }
        let env = FakeEnv::steady(win("cmd.exe", "cmd", 2));
        assert!(matches!(run(&env, &SEED, Action::Copy, None), Outcome::NoBinding { .. }));
        assert!(env.sent().is_empty());
    }

    #[test]
    fn bad_tab_number_is_rejected_before_anything_is_sent() {
        let env = FakeEnv::steady(win("brave.exe", "A", 7));
        assert!(matches!(run(&env, &SEED, Action::GotoTab, Some(12)), Outcome::BadArg(_)));
        assert!(matches!(run(&env, &SEED, Action::GotoTab, None), Outcome::BadArg(_)));
        assert!(env.sent().is_empty());
    }

    #[test]
    fn send_failure_is_reported_and_not_verified() {
        let mut env = FakeEnv::steady(win("brave.exe", "A", 7));
        env.send_err = Some("blocked by UIPI".into());
        assert_eq!(run(&env, &SEED, Action::NewTab, None), Outcome::Failed("blocked by UIPI".into()));
        assert_eq!(env.now_ms(), 0, "no verify wait after a failed send");
    }

    #[test]
    fn actions_without_an_observable_effect_do_not_wait() {
        let env = FakeEnv::steady(win("notepad.exe", "Untitled", 5));
        let out = run(&env, &SEED, Action::Copy, None);
        assert!(matches!(out, Outcome::Sent { verified: Verified::NotChecked, .. }));
        assert_eq!(env.now_ms(), 0);
        assert_eq!(env.fg_calls.get(), 2, "decision + pre-send gate only");
    }

    #[test]
    fn closing_the_last_tab_moves_the_foreground_and_still_counts_as_done() {
        let env = FakeEnv::new(vec![
            Some(win("brave.exe", "Only tab", 7)),
            Some(win("brave.exe", "Only tab", 7)),
            Some(win("explorer.exe", "Desktop", 1)),
        ]);
        assert!(matches!(run(&env, &SEED, Action::CloseTab, None), Outcome::Sent { verified: Verified::Yes, .. }));
    }

    /// RELEASE GATE (docs/features/103 E1): over the whole seeded browser table, every action sends the
    /// documented chord exactly once, and no run ever sends to a window that changed.
    #[test]
    fn corpus_gate_every_browser_action_sends_once_and_never_to_the_wrong_window() {
        let expect = [
            (Action::NewTab, None, "Ctrl+T"),
            (Action::CloseTab, None, "Ctrl+W"),
            (Action::ReopenTab, None, "Ctrl+Shift+T"),
            (Action::NextTab, None, "Ctrl+Tab"),
            (Action::PrevTab, None, "Ctrl+Shift+Tab"),
            (Action::GotoTab, Some(1), "Ctrl+1"),
            (Action::GotoTab, Some(8), "Ctrl+8"),
            (Action::GotoTab, Some(9), "Ctrl+9"),
            (Action::Refresh, None, "F5"),
            (Action::GoBack, None, "Alt+Left"),
            (Action::GoForward, None, "Alt+Right"),
            (Action::AddressBar, None, "Ctrl+L"),
            (Action::FindInPage, None, "Ctrl+F"),
            (Action::Save, None, "Ctrl+S"),
            (Action::Copy, None, "Ctrl+C"),
            (Action::Paste, None, "Ctrl+V"),
            (Action::Cut, None, "Ctrl+X"),
            (Action::Undo, None, "Ctrl+Z"),
            (Action::Redo, None, "Ctrl+Y"),
            (Action::SelectAll, None, "Ctrl+A"),
            (Action::ScrollDown, None, "PageDown"),
            (Action::ScrollUp, None, "PageUp"),
            (Action::ZoomIn, None, "Ctrl+="),
            (Action::ZoomOut, None, "Ctrl+-"),
            (Action::SwitchWindow, None, "Alt+Tab"),
        ];
        let mut pass = 0;
        for exe in ["brave.exe", "chrome.exe", "msedge.exe", "firefox.exe"] {
            for (action, arg, want) in expect {
                // Same window throughout => exactly one send of the documented chord.
                let env = FakeEnv::steady(win(exe, "Page", 7));
                let out = run(&env, &SEED, action, arg);
                assert!(matches!(out, Outcome::Sent { .. }), "{exe} {action:?}: {out:?}");
                assert_eq!(env.sent(), vec![want.to_string()], "{exe} {action:?}");
                // Window swapped before the send => nothing sent.
                let swapped = FakeEnv::new(vec![Some(win(exe, "Page", 7)), Some(win("other.exe", "x", 9))]);
                let out = run(&swapped, &SEED, action, arg);
                assert!(matches!(out, Outcome::Aborted(Abort::ForegroundChanged { .. })), "{out:?}");
                assert!(swapped.sent().is_empty(), "wrong-window send for {exe} {action:?}");
                pass += 1;
            }
        }
        assert_eq!(pass, 4 * expect.len(), "100% of the corpus");
    }

    #[test]
    fn spoken_lines_keep_the_legacy_wording_on_success() {
        let sent = |chord: &str| Outcome::Sent {
            chord: chord.into(),
            scope: Scope::Global,
            verified: Verified::Unconfirmed,
            ms: 1,
        };
        assert_eq!(spoken(Action::NewTab, None, &sent("Ctrl+T")).unwrap(), "New tab sir.");
        assert_eq!(spoken(Action::CloseTab, None, &sent("Ctrl+W")).unwrap(), "Tab closed sir.");
        assert_eq!(spoken(Action::NextTab, None, &sent("Ctrl+Tab")).unwrap(), "Next tab sir.");
        assert_eq!(spoken(Action::GoBack, None, &sent("Alt+Left")).unwrap(), "Back sir.");
        assert_eq!(spoken(Action::GotoTab, Some(3), &sent("Ctrl+3")).unwrap(), "Switched to tab 3, sir.");
    }

    #[test]
    fn reasons_have_no_honorific_so_callers_can_embed_them() {
        let r = reason(Action::GotoTab, &Outcome::NoBinding { known_app: true, exe: "notepad.exe".into() }).unwrap();
        assert_eq!(r, "Notepad has no tab-switching shortcut");
        assert!(!r.contains("sir"));
        let r = reason(Action::Refresh, &Outcome::Aborted(Abort::ForegroundChanged { expected: "a".into(), actual: "b".into() })).unwrap();
        assert_eq!(r, "the window changed, I pressed nothing");
        // Sent has no reason; direct speech capitalises and adds the honorific exactly once.
        let sent = Outcome::Sent { chord: "F5".into(), scope: Scope::Global, verified: Verified::NotChecked, ms: 0 };
        assert_eq!(reason(Action::Refresh, &sent), None);
        assert_eq!(
            spoken(Action::Refresh, None, &Outcome::Aborted(Abort::Stopped)).unwrap_err(),
            "Stopped, sir."
        );
    }

    #[test]
    fn every_failure_has_a_spoken_reason() {
        let cases = [
            Outcome::NoBinding { known_app: true, exe: "notepad.exe".into() },
            Outcome::NoBinding { known_app: false, exe: "x.exe".into() },
            Outcome::BadArg("tab 12 is out of range".into()),
            Outcome::Aborted(Abort::NoForeground),
            Outcome::Aborted(Abort::Blocked),
            Outcome::Aborted(Abort::Elevated),
            Outcome::Aborted(Abort::ForegroundChanged { expected: "a".into(), actual: "b".into() }),
            Outcome::Aborted(Abort::Stopped),
            Outcome::Failed("boom".into()),
        ];
        for c in cases {
            let msg = spoken(Action::Refresh, None, &c).expect_err("failures are Err");
            assert!(msg.ends_with("sir.") || msg.contains("sir"), "{msg}");
        }
        assert_eq!(
            spoken(Action::Refresh, None, &Outcome::NoBinding { known_app: true, exe: "notepad.exe".into() }).unwrap_err(),
            "Notepad has no refresh shortcut, sir."
        );
    }
}
