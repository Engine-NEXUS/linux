//! Keyboard simulation — type text, press keys, press hotkey combos.
//!
//! Uses `enigo` (cross-platform) for input simulation and `arboard` for
//! clipboard-paste (the OpenDex pattern that avoids autocomplete corruption).
//!
//! Key mapping inspired by OpenDex's `keyFromToken()` in computer/skill.ts.
//! Clipboard-paste pattern inspired by OpenDex's `pasteText()`.
//!
//! Why clipboard paste > character-by-character typing:
//!   Typing "hi what are u doing" into WhatsApp character by character can
//!   trigger autocomplete suggestions that corrupt the text (e.g., "u" →
//!   "you" or "doing" → "doing?"). Clipboard paste is instant and bypasses
//!   autocomplete entirely. We restore the previous clipboard contents
//!   after pasting.
//!
//! Why there is a session gate (Linux):
//!   `enigo` is built with the X11 backend only (`Cargo.toml` declares
//!   `enigo = "0.5"` with no features), so every key NEXUS synthesises goes
//!   through XTEST on the X server. On a native Wayland session the X server
//!   only knows XWayland clients, so a key event aimed at a native Wayland
//!   window goes nowhere — while the old code reported success. The audit of
//!   the target machine (Pop!_OS, COSMIC, Wayland) found exactly that:
//!   "type_text returns Ok even when nothing was typed".
//!
//!   So the session is asked FIRST, and "we could not tell" is treated as
//!   "no": the gate refuses with a sentence to say instead of typing into
//!   the void.

use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use std::thread;
use std::time::Duration;

#[cfg(target_os = "linux")]
mod linux;

/// What we know about the display session, for the input decision.
///
/// Plain data with no platform types in it, so [`typing_decision`] reads the
/// same on every operating system and its tests run everywhere. Linux fills it
/// in from `crate::session` and the X server; Windows and macOS fill in
/// "there is a display, it is not Wayland, so the focused window takes the
/// keys".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InputSession {
    /// There is a display (X11 or Wayland) to type into.
    pub display: bool,
    /// The session is native Wayland: XTEST reaches XWayland windows only.
    pub wayland: bool,
    /// The focused window is known to be an XWayland (X11) window, so XTEST
    /// reaches it. `None` means "we could not tell", which is not the same as
    /// "no" — and is treated as a refusal.
    pub xwayland_focus: Option<bool>,
}

/// Pure decision: may we send synthetic key events right now?
///
/// `Err` carries the sentence to say (or log). It never reports success for
/// an event that will not arrive.
pub fn typing_decision(session: InputSession) -> Result<(), String> {
    if !session.display {
        return Err("There is no display session, sir — I cannot type anything.".to_string());
    }
    if !session.wayland {
        // X11 (and an XWayland session): XTEST reaches the focused window.
        return Ok(());
    }
    match session.xwayland_focus {
        Some(true) => Ok(()),
        Some(false) => Err(
            "That window is a native Wayland app, sir — I cannot type into it. \
             Run it under XWayland, or let me copy the text to the clipboard."
                .to_string(),
        ),
        None => Err(
            "I cannot tell whether that window takes typing on Wayland, sir — \
             I will not type into it blindly."
                .to_string(),
        ),
    }
}

/// What we know about this session, for the input decision.
#[cfg(target_os = "linux")]
pub fn current_session() -> InputSession {
    linux::current_session()
}

/// Windows and macOS: there is a display, and it is not Wayland, so the
/// focused window always takes the keys.
#[cfg(not(target_os = "linux"))]
pub fn current_session() -> InputSession {
    InputSession { display: true, wayland: false, xwayland_focus: None }
}

/// Refuse synthetic input when this session cannot deliver it.
fn ensure_input_allowed() -> Result<(), String> {
    typing_decision(current_session())
}

/// Put text on the clipboard so the user can paste it.
///
/// Linux only: this is the fallback for a native Wayland window, which no
/// synthetic input can reach. `arboard` first (it is already a dependency),
/// then `wl-copy` when it is installed.
#[cfg(target_os = "linux")]
pub fn copy_to_clipboard(text: &str) -> bool {
    linux::copy_to_clipboard(text)
}

/// Build an `Enigo` handle, failing with an actionable message.
///
/// `Cargo.toml` declares `enigo = "0.5"` with no features, so only the `x11rb`
/// backend is compiled. On native Wayland there is no `$DISPLAY` and
/// `Enigo::new` cannot connect. On Linux the error names the session type and
/// the real remedy, because "couldn't type" with no explanation is the failure
/// mode that made this look like a working feature.
///
/// Longer-term fix is the semantic tiers in
/// `docs/features/research/20-…`: `AT-SPI EditableText` and CDP
/// `Runtime.evaluate` both set field contents with no input synthesis at all,
/// which sidesteps the Wayland input lockdown rather than fighting it.
fn init_enigo() -> Result<enigo::Enigo, String> {
    #[cfg(target_os = "linux")]
    {
        if !crate::session::has_x11_input() {
            tracing::warn!(
                "live: keyboard synthesis attempted on a session without XTEST — {}",
                crate::session::diagnostic_line()
            );
        }
    }
    Enigo::new(&Settings::default()).map_err(|e| {
        #[cfg(target_os = "linux")]
        let msg = format!(
            "keyboard input unavailable: {e} ({}). On Wayland this needs the \
             semantic input path or libei via the RemoteDesktop portal.",
            crate::session::kind().input_capability()
        );
        #[cfg(not(target_os = "linux"))]
        let msg = format!("enigo init: {e}");
        msg
    })
}

/// Type text into the currently focused input field.
///
/// For short text (< 50 chars), uses enigo's `text()` which simulates
/// individual key presses. For longer text, uses clipboard paste
/// (Ctrl+V) to avoid autocomplete corruption and improve speed.
pub fn type_text(text: &str) -> Result<(), String> {
    ensure_input_allowed()?;
    tracing::info!("live: typing {} chars", text.len());
    let mut enigo = init_enigo()?;

    if text.len() > 50 {
        // Use clipboard paste for longer text (OpenDex pattern)
        paste_text(text)?;
    } else {
        enigo
            .text(text)
            .map_err(|e| format!("type_text failed: {e}"))?;
    }

    Ok(())
}

/// Paste text via clipboard (Ctrl+V / Cmd+V on macOS).
/// Restores the previous clipboard contents after pasting.
fn paste_text(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| format!("clipboard init: {e}"))?;

    // Save previous clipboard contents
    let prev = clipboard.get_text().unwrap_or_default();

    // Set new text
    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("clipboard set: {e}"))?;

    // Small delay to ensure clipboard is ready
    thread::sleep(Duration::from_millis(50));

    // Press Ctrl+V (or Cmd+V on macOS)
    let mut enigo = init_enigo()?;

    let modifier = if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    };

    enigo
        .key(modifier, Press)
        .map_err(|e| format!("press modifier: {e}"))?;
    enigo
        .key(Key::Unicode('v'), Click)
        .map_err(|e| format!("press V: {e}"))?;
    enigo
        .key(modifier, Release)
        .map_err(|e| format!("release modifier: {e}"))?;

    // Wait for paste to complete
    thread::sleep(Duration::from_millis(120));

    // Restore previous clipboard
    let _ = clipboard.set_text(prev);

    tracing::info!("live: pasted {} chars via clipboard", text.len());
    Ok(())
}

/// Press a single key (e.g., "enter", "escape", "tab").
pub fn press_key(key: &str) -> Result<(), String> {
    ensure_input_allowed()?;
    tracing::info!("live: pressing key: {key}");
    let mut enigo = init_enigo()?;

    let k = parse_key(key).ok_or_else(|| format!("unknown key: {key}"))?;
    enigo
        .key(k, Click)
        .map_err(|e| format!("press_key failed: {e}"))?;

    Ok(())
}

/// Press a key combination (e.g., Ctrl+A, Ctrl+Shift+Tab).
/// The last key in the slice is the "main" key; all others are modifiers
/// that are held down during the press.
pub fn press_hotkey(keys: &[&str]) -> Result<(), String> {
    if keys.is_empty() {
        return Err("press_hotkey: no keys provided".to_string());
    }

    tracing::info!("live: pressing hotkey: {:?}", keys);

    ensure_input_allowed()?;
    let mut enigo = init_enigo()?;

    // Parse all keys
    let parsed: Vec<Key> = keys
        .iter()
        .map(|k| parse_key(k))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| format!("unknown key in hotkey: {:?}", keys))?;

    // Press all modifier keys (all except the last)
    for k in &parsed[..parsed.len() - 1] {
        enigo
            .key(k.clone(), Press)
            .map_err(|e| format!("press modifier: {e}"))?;
    }

    // Click the last key
    enigo
        .key(parsed[parsed.len() - 1].clone(), Click)
        .map_err(|e| format!("click main key: {e}"))?;

    // Release all modifiers in reverse order
    for k in parsed[..parsed.len() - 1].iter().rev() {
        enigo
            .key(k.clone(), Release)
            .map_err(|e| format!("release modifier: {e}"))?;
    }

    Ok(())
}

/// Map a string key name to an enigo Key.
/// Inspired by OpenDex's `keyFromToken()` in computer/skill.ts.
fn parse_key(token: &str) -> Option<Key> {
    let t = token.trim().to_lowercase();

    // Special keys
    let special: &[(&str, Key)] = &[
        ("enter", Key::Return),
        ("return", Key::Return),
        ("tab", Key::Tab),
        ("escape", Key::Escape),
        ("esc", Key::Escape),
        ("space", Key::Space),
        ("spacebar", Key::Space),
        ("backspace", Key::Backspace),
        ("delete", Key::Delete),
        ("del", Key::Delete),
        ("up", Key::UpArrow),
        ("arrowup", Key::UpArrow),
        ("down", Key::DownArrow),
        ("arrowdown", Key::DownArrow),
        ("left", Key::LeftArrow),
        ("arrowleft", Key::LeftArrow),
        ("right", Key::RightArrow),
        ("arrowright", Key::RightArrow),
        ("home", Key::Home),
        ("end", Key::End),
        ("pageup", Key::PageUp),
        ("pgup", Key::PageUp),
        ("pagedown", Key::PageDown),
        ("pgdn", Key::PageDown),
        ("ctrl", Key::Control),
        ("control", Key::Control),
        ("alt", Key::Alt),
        ("option", Key::Alt),
        ("shift", Key::Shift),
        ("cmd", Key::Meta),
        ("command", Key::Meta),
        ("meta", Key::Meta),
        ("win", Key::Meta),
        ("windows", Key::Meta),
        ("super", Key::Meta),
    ];

    for (name, key) in special {
        if t == *name {
            return Some(key.clone());
        }
    }

    // Single character (letters, digits, symbols)
    if t.len() == 1 {
        if let Some(c) = t.chars().next() {
            return Some(Key::Unicode(c));
        }
    }

    // Function keys F1-F12
    if let Some(num) = t.strip_prefix('f') {
        if let Ok(n) = num.parse::<u8>() {
            if (1..=12).contains(&n) {
                return Some(match n {
                    1 => Key::F1,
                    2 => Key::F2,
                    3 => Key::F3,
                    4 => Key::F4,
                    5 => Key::F5,
                    6 => Key::F6,
                    7 => Key::F7,
                    8 => Key::F8,
                    9 => Key::F9,
                    10 => Key::F10,
                    11 => Key::F11,
                    12 => Key::F12,
                    _ => return None,
                });
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_key_special() {
        assert_eq!(parse_key("enter"), Some(Key::Return));
        assert_eq!(parse_key("Enter"), Some(Key::Return));
        assert_eq!(parse_key("RETURN"), Some(Key::Return));
        assert_eq!(parse_key("esc"), Some(Key::Escape));
        assert_eq!(parse_key("escape"), Some(Key::Escape));
        assert_eq!(parse_key("tab"), Some(Key::Tab));
        assert_eq!(parse_key("space"), Some(Key::Space));
        assert_eq!(parse_key("ctrl"), Some(Key::Control));
        assert_eq!(parse_key("shift"), Some(Key::Shift));
        assert_eq!(parse_key("alt"), Some(Key::Alt));
        assert_eq!(parse_key("cmd"), Some(Key::Meta));
        assert_eq!(parse_key("win"), Some(Key::Meta));
    }

    #[test]
    fn test_parse_key_single_char() {
        assert_eq!(parse_key("a"), Some(Key::Unicode('a')));
        assert_eq!(parse_key("A"), Some(Key::Unicode('a')));
        assert_eq!(parse_key("v"), Some(Key::Unicode('v')));
        assert_eq!(parse_key("1"), Some(Key::Unicode('1')));
    }

    #[test]
    fn test_parse_key_function_keys() {
        assert_eq!(parse_key("f1"), Some(Key::F1));
        assert_eq!(parse_key("f12"), Some(Key::F12));
        // F13-F24 exist in enigo 0.5 but we only map F1-F12
    }

    #[test]
    fn test_parse_key_unknown() {
        assert_eq!(parse_key("unknown"), None);
        assert_eq!(parse_key("xyz"), None);
        assert_eq!(parse_key(""), None);
    }

    #[test]
    fn an_x11_session_can_type() {
        // XTEST reaches whatever has the focus, so nothing is refused.
        let s = InputSession { display: true, wayland: false, xwayland_focus: None };
        assert!(typing_decision(s).is_ok());
    }

    #[test]
    fn wayland_only_types_into_a_window_that_is_known_to_be_xwayland() {
        // Known XWayland window: the X server delivers the key.
        let s = InputSession { display: true, wayland: true, xwayland_focus: Some(true) };
        assert!(typing_decision(s).is_ok());

        // Known native Wayland window: refuse instead of typing into the void.
        let s = InputSession { display: true, wayland: true, xwayland_focus: Some(false) };
        let err = typing_decision(s).unwrap_err();
        assert!(err.contains("Wayland"), "{err}");

        // Unknown: "we could not tell" is not "yes".
        let s = InputSession { display: true, wayland: true, xwayland_focus: None };
        let err = typing_decision(s).unwrap_err();
        assert!(err.contains("Wayland"), "{err}");
    }

    #[test]
    fn no_display_means_no_typing() {
        let s = InputSession { display: false, wayland: false, xwayland_focus: None };
        let err = typing_decision(s).unwrap_err();
        assert!(err.contains("display"), "{err}");
    }
}
