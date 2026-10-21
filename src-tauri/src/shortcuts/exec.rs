//! Windows adapter for the Action Router (shortcut brain, phase E1): real foreground probe, batched
//! `SendInput`, ghost stop flag. Everything decision-like lives in `router.rs`; this file is deliberately
//! thin so the tested logic is the logic that ships.

use super::keys::{key_events, Chord, KeyEvent};
use super::map::{Action, SEED};
use super::router::{self, Env, Foreground, Outcome};

/// Credential managers and wallets: protected even when only the *window title* says so
/// (e.g. a browser tab "Bitwarden Web Vault"). The money terms in `live::safety::DENIED_TARGETS`
/// ("bank", "ledger", …) are far too common in ordinary page titles, so they are matched against the
/// executable name only.
const TITLE_PROTECTED: &[&str] = &[
    "1password", "bitwarden", "keychain", "lastpass", "keepass", "dashlane", "metamask", "trezor",
];

/// Pure (unit-tested): should keystrokes be refused for this window?
pub fn is_protected_window(exe: &str, title: &str) -> bool {
    if crate::live::safety::is_target_blocked(exe) {
        return true;
    }
    let t = title.to_lowercase();
    TITLE_PROTECTED.iter().any(|p| t.contains(p))
}

/// Kill switch: `NEXUS_SHORTCUTS_LEGACY=1` makes every call site use its previous implementation.
pub fn enabled() -> bool {
    !std::env::var("NEXUS_SHORTCUTS_LEGACY").map(|v| v == "1").unwrap_or(false)
}

pub struct WinEnv;

impl Env for WinEnv {
    fn foreground(&self) -> Option<Foreground> {
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
            let hwnd = unsafe { GetForegroundWindow() };
            if hwnd.0 == 0 {
                return None;
            }
            let exe = crate::browser_url::foreground_process_name()?;
            let title = crate::browser_url::foreground_window_title().unwrap_or_default();
            let elevated = crate::live::commands::window::window_process_elevated(hwnd);
            Some(Foreground { exe, title, hwnd: hwnd.0, elevated })
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    }

    fn send(&self, chord: &Chord) -> Result<(), String> {
        send_chord(chord)
    }

    fn sleep_ms(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    fn now_ms(&self) -> u64 {
        static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        T0.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
    }

    fn stop_requested(&self) -> bool {
        // Only meaningful inside a ghost session; outside one the flag is irrelevant.
        crate::ghost::session_active() && crate::ghost::stop_requested()
    }

    fn self_elevated(&self) -> bool {
        crate::live::commands::window::our_process_elevated()
    }

    fn is_protected(&self, exe: &str, title: &str) -> bool {
        is_protected_window(exe, title)
    }
}

/// One chord = ONE `SendInput` call (modifiers down, key down/up, modifiers up), by virtual key, so it is
/// atomic with respect to other input and independent of the keyboard layout / IME.
#[cfg(target_os = "windows")]
fn send_chord(chord: &Chord) -> Result<(), String> {
    send_events(&key_events(chord))
}

/// Raw key events in one `SendInput` batch (used by hand gestures to hold Alt for the window switcher).
#[cfg(target_os = "windows")]
pub fn send_events(events: &[KeyEvent]) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    let inputs: Vec<INPUT> = events
        .iter()
        .map(|e| {
            let mut flags = 0u32;
            if e.extended {
                flags |= KEYEVENTF_EXTENDEDKEY.0;
            }
            if e.up {
                flags |= KEYEVENTF_KEYUP.0;
            }
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(e.vk),
                        wScan: 0,
                        dwFlags: KEYBD_EVENT_FLAGS(flags),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            }
        })
        .collect();
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(format!(
            "SendInput delivered {sent}/{} events (blocked by UIPI or another desktop?)",
            inputs.len()
        ))
    }
}

#[cfg(not(target_os = "windows"))]
pub fn send_events(_events: &[KeyEvent]) -> Result<(), String> {
    Err("keyboard shortcuts are only implemented on Windows".into())
}

#[cfg(not(target_os = "windows"))]
fn send_chord(_chord: &Chord) -> Result<(), String> {
    Err("keyboard shortcuts are only implemented on Windows".into())
}

/// Perform `action` in the application that is in front right now. Blocking (≤ ~0.8 s when it verifies a
/// title change, a few ms otherwise): call from a blocking context, not from a latency-critical async task.
pub fn do_action(action: Action, arg: Option<u32>) -> Outcome {
    let out = router::run(&WinEnv, &SEED, action, arg);
    log_outcome(action, arg, &out);
    out
}

fn log_outcome(action: Action, arg: Option<u32>, out: &Outcome) {
    let a = match arg {
        Some(n) => format!("{}({n})", action.id()),
        None => action.id().to_string(),
    };
    match out {
        Outcome::Sent { chord, scope, verified, ms } => tracing::info!(
            "[SHORTCUT] {a} -> {chord} ({}) verified={verified:?} in {ms}ms",
            scope.describe()
        ),
        Outcome::NoBinding { known_app, exe } => {
            tracing::info!("[SHORTCUT] {a}: no binding for {exe} (known_app={known_app})")
        }
        Outcome::BadArg(m) => tracing::info!("[SHORTCUT] {a}: bad argument - {m}"),
        Outcome::Aborted(why) => tracing::warn!("[SHORTCUT] {a}: ABORTED, nothing sent - {why:?}"),
        Outcome::Failed(e) => tracing::warn!("[SHORTCUT] {a}: send failed - {e}"),
    }
}

/// Run an action and turn the outcome into the legacy `Result<spoken line, spoken reason>` shape.
///
/// `legacy_ok_for_unknown_app` preserves the old behaviour for browser hotkeys: if the foreground app is
/// **not in any seeded family** (an unknown browser such as Arc/Zen) and has no binding, the caller should
/// run its previous implementation instead of refusing — returned as `Err(None)`-like via `Fallback`.
pub enum Spoken {
    /// Success line (legacy wording).
    Ok(String),
    /// Plain reason (no honorific) — embed it: `format!("Couldn't …, sir: {reason}")`.
    Refused(String),
    /// Unknown app, no binding: let the caller's legacy path decide.
    Fallback,
}

pub fn do_action_spoken(action: Action, arg: Option<u32>, legacy_ok_for_unknown_app: bool) -> Spoken {
    let out = do_action(action, arg);
    if legacy_ok_for_unknown_app {
        if let Outcome::NoBinding { known_app: false, .. } = out {
            return Spoken::Fallback;
        }
    }
    match router::reason(action, &out) {
        None => Spoken::Ok(router::spoken(action, arg, &out).unwrap_or_default()),
        Some(reason) => Spoken::Refused(reason),
    }
}

/// How a low-level hotkey call was carried out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sent {
    /// Through the router (gated, verified when the action allows).
    Router(router::Verified),
    /// The caller's previous implementation ran (kill switch, or an app the map knows nothing about).
    Legacy,
}

/// For low-level callers that only need "make this happen": run the action through the router, or fall back
/// to `legacy` when the shortcut brain is off or the foreground app is not in any seeded family (an unknown
/// browser, say) — exactly what those callers did before. A refusal (protected window, window changed,
/// known app without that shortcut…) is an `Err` with a plain reason and sends nothing.
pub fn run_or_legacy(
    action: Action,
    arg: Option<u32>,
    legacy: impl FnOnce() -> Result<(), String>,
) -> Result<Sent, String> {
    if !enabled() {
        return legacy().map(|_| Sent::Legacy);
    }
    let out = do_action(action, arg);
    match &out {
        Outcome::Sent { verified, .. } => Ok(Sent::Router(*verified)),
        Outcome::NoBinding { known_app: false, .. } => {
            tracing::info!("[SHORTCUT] {}: unknown app, using the legacy hotkey", action.id());
            legacy().map(|_| Sent::Legacy)
        }
        other => Err(router::reason(action, other).unwrap_or_else(|| "refused".to_string())),
    }
}

/// IPC (dev + the upcoming grammar track): `invoke("shortcut_do", { action: "refresh" })` or
/// `{ action: "goto_tab_n", arg: 3 }`. Returns the spoken line on success, a spoken reason on refusal.
/// Runs on a blocking thread (verification may wait up to ~0.8 s).
#[tauri::command]
pub async fn shortcut_do(action: String, arg: Option<u32>) -> Result<String, String> {
    let act = Action::from_id(&action).ok_or_else(|| format!("unknown shortcut action '{action}'"))?;
    if !enabled() {
        return Err("shortcut brain is disabled (NEXUS_SHORTCUTS_LEGACY=1)".into());
    }
    let out = tokio::task::spawn_blocking(move || do_action(act, arg))
        .await
        .map_err(|e| e.to_string())?;
    router::spoken(act, arg, &out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_windows_by_exe_and_by_credential_title() {
        // Executable names: the full live::safety denylist applies.
        assert!(is_protected_window("bitwarden.exe", "Vault"));
        assert!(is_protected_window("1password.exe", ""));
        assert!(is_protected_window("coinbase.exe", "Coinbase"));
        // Titles: only credential managers / wallets.
        assert!(is_protected_window("brave.exe", "Bitwarden Web Vault"));
        assert!(is_protected_window("chrome.exe", "MetaMask Notification"));
        // Ordinary pages that merely contain a money word are NOT blocked (Ctrl+W on them is harmless).
        assert!(!is_protected_window("brave.exe", "Memory Bank notes - Brave"));
        assert!(!is_protected_window("chrome.exe", "Ledger of expenses - Google Sheets"));
        assert!(!is_protected_window("notepad.exe", "Untitled - Notepad"));
    }

    #[test]
    fn kill_switch_reads_the_environment() {
        // Not mutating the process env here (tests run in parallel); just assert the default.
        if std::env::var("NEXUS_SHORTCUTS_LEGACY").is_err() {
            assert!(enabled());
        }
    }
}
