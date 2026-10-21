//! Layout-independent key chords (shortcut brain, phase E1).
//!
//! A shortcut such as `Ctrl+T` is bound by applications to a **virtual key**, not to the character the
//! user's layout happens to print on that key. The legacy path sent `Key::Unicode('t')`, which on a
//! non-Latin layout or under an IME is not the key the application listens for (research doc 09 §1.4).
//! Chords here are therefore parsed to virtual-key codes, and `key_events` produces the exact
//! press/release sequence that is sent in ONE batched `SendInput` call (no inter-key sleeps).
//!
//! Pure and platform-independent: everything is unit-tested without touching the OS.

use std::fmt;

/// Modifier keys, in canonical send order (also the display order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Win,
}

impl Modifier {
    /// Virtual-key code and whether the key needs `KEYEVENTF_EXTENDEDKEY`.
    fn vk(self) -> (u16, bool) {
        match self {
            Modifier::Ctrl => (0x11, false),  // VK_CONTROL
            Modifier::Alt => (0x12, false),   // VK_MENU
            Modifier::Shift => (0x10, false), // VK_SHIFT
            Modifier::Win => (0x5B, true),    // VK_LWIN
        }
    }

    fn name(self) -> &'static str {
        match self {
            Modifier::Ctrl => "Ctrl",
            Modifier::Alt => "Alt",
            Modifier::Shift => "Shift",
            Modifier::Win => "Win",
        }
    }
}

/// One keyboard shortcut: modifiers held while a single main key is pressed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Chord {
    pub mods: Vec<Modifier>,
    /// Virtual-key code of the main key.
    pub vk: u16,
    /// Main key needs the extended-key flag (arrows, Home/End, PageUp/Down, Insert/Delete).
    pub extended: bool,
    label: String,
}

impl Chord {
    /// Human-readable form, e.g. `Ctrl+Shift+T` (used in logs and in spoken "what's the shortcut").
    pub fn display(&self) -> String {
        let mut parts: Vec<&str> = self.mods.iter().map(|m| m.name()).collect();
        parts.push(self.label.as_str());
        parts.join("+")
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display())
    }
}

/// One low-level key event of a chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub vk: u16,
    pub extended: bool,
    pub up: bool,
}

/// The exact event sequence for a chord: modifiers down (canonical order), key down, key up,
/// modifiers up (reverse order). Sent as one `SendInput` batch.
pub fn key_events(chord: &Chord) -> Vec<KeyEvent> {
    let mut ev = Vec::with_capacity(chord.mods.len() * 2 + 2);
    for m in &chord.mods {
        let (vk, extended) = m.vk();
        ev.push(KeyEvent { vk, extended, up: false });
    }
    ev.push(KeyEvent { vk: chord.vk, extended: chord.extended, up: false });
    ev.push(KeyEvent { vk: chord.vk, extended: chord.extended, up: true });
    for m in chord.mods.iter().rev() {
        let (vk, extended) = m.vk();
        ev.push(KeyEvent { vk, extended, up: true });
    }
    ev
}

/// Parse `Ctrl+Shift+T`, `Alt+Left`, `Ctrl+=`, `F5`, `Ctrl+PageDown`… (case-insensitive).
/// The **last** token is the main key; every earlier token must be a modifier.
pub fn parse_chord(text: &str) -> Result<Chord, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("empty chord".into());
    }
    // A literal '+' main key is written "Ctrl+Plus" (a bare trailing '+' would split ambiguously).
    let tokens: Vec<&str> = text.split('+').map(|t| t.trim()).collect();
    if tokens.iter().any(|t| t.is_empty()) {
        return Err(format!("malformed chord '{text}' (use 'Plus' for the + key)"));
    }
    let (key_tok, mod_toks) = tokens.split_last().ok_or_else(|| "empty chord".to_string())?;

    let mut mods: Vec<Modifier> = Vec::new();
    for t in mod_toks {
        let m = match t.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Modifier::Ctrl,
            "alt" | "option" => Modifier::Alt,
            "shift" => Modifier::Shift,
            "win" | "windows" | "meta" | "super" | "cmd" => Modifier::Win,
            other => return Err(format!("'{other}' is not a modifier in '{text}'")),
        };
        if mods.contains(&m) {
            return Err(format!("duplicate modifier '{}' in '{text}'", m.name()));
        }
        mods.push(m);
    }
    mods.sort();

    let (vk, extended, label) = parse_key(key_tok)
        .ok_or_else(|| format!("unknown key '{key_tok}' in '{text}'"))?;
    Ok(Chord { mods, vk, extended, label })
}

/// Main-key table → (virtual-key code, extended flag, canonical label).
fn parse_key(tok: &str) -> Option<(u16, bool, String)> {
    let t = tok.trim();
    let lower = t.to_ascii_lowercase();

    // Single letter / digit.
    if t.chars().count() == 1 {
        let c = t.chars().next().unwrap();
        if c.is_ascii_alphabetic() {
            let up = c.to_ascii_uppercase();
            return Some((up as u16, false, up.to_string())); // VK_A..VK_Z == ASCII
        }
        if c.is_ascii_digit() {
            return Some((c as u16, false, c.to_string())); // VK_0..VK_9 == ASCII
        }
        // Punctuation keys by their unshifted glyph (US-layout label; the VK is what matters).
        let punct = match c {
            '=' => Some(0xBB), // VK_OEM_PLUS (the "=/+" key)
            '-' => Some(0xBD), // VK_OEM_MINUS
            ',' => Some(0xBC), // VK_OEM_COMMA
            '.' => Some(0xBE), // VK_OEM_PERIOD
            '/' => Some(0xBF), // VK_OEM_2
            ';' => Some(0xBA), // VK_OEM_1
            '`' => Some(0xC0), // VK_OEM_3
            '[' => Some(0xDB), // VK_OEM_4
            '\\' => Some(0xDC), // VK_OEM_5
            ']' => Some(0xDD), // VK_OEM_6
            '\'' => Some(0xDE), // VK_OEM_7
            _ => None,
        };
        if let Some(vk) = punct {
            return Some((vk, false, c.to_string()));
        }
    }

    // Function keys F1..F24.
    if let Some(n) = lower.strip_prefix('f').and_then(|r| r.parse::<u16>().ok()) {
        if (1..=24).contains(&n) {
            return Some((0x6F + n, false, format!("F{n}"))); // VK_F1 = 0x70
        }
    }

    // Named keys. (vk, extended, label)
    let named: Option<(u16, bool, &str)> = match lower.as_str() {
        "enter" | "return" => Some((0x0D, false, "Enter")),
        "esc" | "escape" => Some((0x1B, false, "Esc")),
        "tab" => Some((0x09, false, "Tab")),
        "space" | "spacebar" => Some((0x20, false, "Space")),
        "backspace" => Some((0x08, false, "Backspace")),
        "delete" | "del" => Some((0x2E, true, "Delete")),
        "insert" | "ins" => Some((0x2D, true, "Insert")),
        "home" => Some((0x24, true, "Home")),
        "end" => Some((0x23, true, "End")),
        "pageup" | "pgup" => Some((0x21, true, "PageUp")),
        "pagedown" | "pgdn" => Some((0x22, true, "PageDown")),
        "left" => Some((0x25, true, "Left")),
        "up" => Some((0x26, true, "Up")),
        "right" => Some((0x27, true, "Right")),
        "down" => Some((0x28, true, "Down")),
        "plus" => Some((0xBB, false, "Plus")),
        "minus" => Some((0xBD, false, "Minus")),
        "numadd" => Some((0x6B, false, "NumAdd")),
        "numsubtract" | "numsub" => Some((0x6D, false, "NumSubtract")),
        _ => None,
    };
    named.map(|(vk, ext, label)| (vk, ext, label.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_chords_to_virtual_keys() {
        let c = parse_chord("Ctrl+T").unwrap();
        assert_eq!((c.vk, c.extended), (0x54, false));
        assert_eq!(c.display(), "Ctrl+T");

        let c = parse_chord("ctrl+shift+t").unwrap();
        assert_eq!(c.mods, vec![Modifier::Ctrl, Modifier::Shift]);
        assert_eq!(c.display(), "Ctrl+Shift+T");

        let c = parse_chord("Alt+Left").unwrap();
        assert_eq!((c.vk, c.extended), (0x25, true), "arrows are extended keys");

        let c = parse_chord("F5").unwrap();
        assert_eq!((c.vk, c.mods.len()), (0x74, 0));

        let c = parse_chord("Ctrl+PageDown").unwrap();
        assert_eq!((c.vk, c.extended), (0x22, true));

        let c = parse_chord("Ctrl+=").unwrap();
        assert_eq!(c.vk, 0xBB, "zoom-in key is VK_OEM_PLUS, not the '=' character");
        assert_eq!(parse_chord("Ctrl+Plus").unwrap().vk, 0xBB);
        assert_eq!(parse_chord("Ctrl+-").unwrap().vk, 0xBD);
        assert_eq!(parse_chord("Ctrl+9").unwrap().vk, 0x39);
    }

    #[test]
    fn modifier_order_is_canonical_regardless_of_input_order() {
        let a = parse_chord("Shift+Ctrl+T").unwrap();
        let b = parse_chord("Ctrl+Shift+T").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.display(), "Ctrl+Shift+T");
    }

    #[test]
    fn rejects_malformed_chords() {
        for bad in ["", "Ctrl+", "+T", "Ctrl+Ctrl+T", "Foo+T", "Ctrl+Nonsense", "Ctrl++"] {
            assert!(parse_chord(bad).is_err(), "should reject '{bad}'");
        }
        // A modifier is never a main key.
        assert!(parse_chord("Ctrl+Shift").is_err());
    }

    #[test]
    fn event_sequence_presses_mods_then_key_then_releases_in_reverse() {
        let ev = key_events(&parse_chord("Ctrl+Shift+T").unwrap());
        let seq: Vec<(u16, bool)> = ev.iter().map(|e| (e.vk, e.up)).collect();
        assert_eq!(
            seq,
            vec![
                (0x11, false), // Ctrl down
                (0x10, false), // Shift down
                (0x54, false), // T down
                (0x54, true),  // T up
                (0x10, true),  // Shift up
                (0x11, true),  // Ctrl up
            ]
        );
        // A bare key is just down/up.
        assert_eq!(key_events(&parse_chord("F5").unwrap()).len(), 2);
    }

    #[test]
    fn extended_flag_follows_the_main_key_only() {
        let ev = key_events(&parse_chord("Alt+Left").unwrap());
        let by_vk = |vk| ev.iter().find(|e| e.vk == vk).unwrap().extended;
        assert!(by_vk(0x25), "Left is extended");
        assert!(!by_vk(0x12), "Alt is not");
    }

    #[test]
    fn all_f_keys_and_digits_map() {
        for n in 1..=24u16 {
            assert_eq!(parse_chord(&format!("F{n}")).unwrap().vk, 0x6F + n);
        }
        for d in 0..=9u16 {
            assert_eq!(parse_chord(&format!("Ctrl+{d}")).unwrap().vk, 0x30 + d);
        }
    }
}
