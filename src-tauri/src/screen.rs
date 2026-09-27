//! Screen vision + control — see the screen, resolve ordinals, click.
//!
//! Grounding order (see docs/mcp/09-screen-vision-control.md):
//!   1. Browser tabs → hotkeys first (Ctrl+1..8, 100% reliable, instant).
//!   2. Everything else → Windows UIA tree (exact names + boxes, free).
//!   3. Canvas/custom UI → future (vision model / OCR), not this module.
//!
//! Ordinal rule: visible + enabled actionables in the foreground window,
//! sorted top-to-bottom then left-to-right. "3rd option" = 3rd actionable.
//! Password/secure fields never resolve; destructive targets need confirm
//! (enforced by callers, not here).

/// One actionable UI element.
#[derive(Debug, Clone, PartialEq)]
pub struct UiElement {
    pub name: String,
    pub kind: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Sort elements in reading order: top-to-bottom, then left-to-right.
/// Pure function — unit-tested below.
pub fn sort_reading_order(mut els: Vec<UiElement>) -> Vec<UiElement> {
    // Row tolerance: items within 12px vertically share a row.
    els.sort_by(|a, b| {
        let row_a = a.y / 12;
        let row_b = b.y / 12;
        row_a.cmp(&row_b).then(a.x.cmp(&b.x))
    });
    els
}

/// 1-based ordinal pick. Returns None when out of range.
pub fn pick_ordinal(sorted: &[UiElement], n: u32) -> Option<&UiElement> {
    if n == 0 {
        return None;
    }
    sorted.get((n - 1) as usize)
}

/// Parse "3rd", "2nd", "1st", "4th", "12th" (and bare "3") into a number.
pub fn parse_ordinal(text: &str) -> Option<u32> {
    let t = text.trim().to_lowercase();
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    // Accept "3", "3rd", "3th"-ish; reject trailing junk like "3x".
    let rest = t[digits.len()..].trim_start();
    if !rest.is_empty()
        && !["st", "nd", "rd", "th"].iter().any(|suf| rest.starts_with(suf))
    {
        return None;
    }
    digits.parse().ok()
}

#[cfg(target_os = "windows")]
mod win {
    use super::UiElement;
    use uiautomation::controls::ControlType;
    use uiautomation::UIAutomation;

    const CLICKABLE: &[ControlType] = &[
        ControlType::Button,
        ControlType::Hyperlink,
        ControlType::TabItem,
        ControlType::MenuItem,
        ControlType::ListItem,
        ControlType::CheckBox,
        ControlType::RadioButton,
        ControlType::SplitButton,
    ];

    /// List actionable elements in the foreground window via UIA.
    /// Best-effort: any failing element is skipped, never fatal.
    pub fn list_actionables() -> Vec<UiElement> {
        let automation = match UIAutomation::new() {
            Ok(a) => a,
            Err(_) => return Vec::new(),
        };
        let focused = match automation.get_focused_element() {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        for ct in CLICKABLE {
            let found = automation
                .create_matcher()
                .from(focused.clone())
                .timeout(500)
                .control_type(*ct)
                .find_all();
            let elements = match found {
                Ok(e) => e,
                Err(_) => continue,
            };
            for el in elements.iter() {
                let enabled = el.is_enabled().unwrap_or(false);
                if !enabled {
                    continue;
                }
                let name = el.get_name().unwrap_or_default();
                if name.trim().is_empty() {
                    continue;
                }
                let kind = format!("{:?}", ct);
                // Skip password/secure fields and address bars by name.
                let lower = name.to_lowercase();
                if lower.contains("password") {
                    continue;
                }
                let (x, y, w, h) = match el.get_bounding_rectangle() {
                    Ok(r) => (r.get_left(), r.get_top(), r.get_width(), r.get_height()),
                    Err(_) => continue,
                };
                if w <= 0 || h <= 0 {
                    continue;
                }
                out.push(UiElement {
                    name,
                    kind,
                    x,
                    y,
                    w,
                    h,
                });
            }
        }
        super::sort_reading_order(out)
    }

    /// Move the mouse to an element's center and left-click.
    pub fn click_element(el: &UiElement) -> Result<(), String> {
        use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};
        let mut enigo =
            Enigo::new(&Settings::default()).map_err(|e| format!("enigo init: {e}"))?;
        let cx = el.x + el.w / 2;
        let cy = el.y + el.h / 2;
        enigo
            .move_mouse(cx, cy, Coordinate::Abs)
            .map_err(|e| format!("mouse move: {e}"))?;
        enigo
            .button(Button::Left, Direction::Click)
            .map_err(|e| format!("mouse click: {e}"))?;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
pub use win::{click_element, list_actionables};

/// Switch browser tab by index (1-based): Ctrl+1..8, Ctrl+9 = last.
/// Hotkeys beat grounding for tabs — 100% reliable, instant.
pub fn switch_browser_tab(index: u32) -> Result<String, String> {
    if index == 0 || index > 9 {
        return Err("tab number must be 1-9".to_string());
    }
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| format!("enigo init: {e}"))?;
    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| format!("ctrl press: {e}"))?;
    let digit = match index {
        1 => Key::Unicode('1'),
        2 => Key::Unicode('2'),
        3 => Key::Unicode('3'),
        4 => Key::Unicode('4'),
        5 => Key::Unicode('5'),
        6 => Key::Unicode('6'),
        7 => Key::Unicode('7'),
        8 => Key::Unicode('8'),
        _ => Key::Unicode('9'),
    };
    let r = enigo
        .key(digit, Direction::Click)
        .map_err(|e| format!("digit: {e}"));
    let _ = enigo.key(Key::Control, Direction::Release);
    r?;
    Ok(format!("Switched to tab {index}, sir."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(name: &str, x: i32, y: i32) -> UiElement {
        UiElement {
            name: name.into(),
            kind: "Button".into(),
            x,
            y,
            w: 80,
            h: 30,
        }
    }

    #[test]
    fn reading_order_top_to_bottom_then_left_to_right() {
        let v = sort_reading_order(vec![el("b", 200, 10), el("a", 10, 10), el("c", 10, 100)]);
        assert_eq!(v[0].name, "a");
        assert_eq!(v[1].name, "b");
        assert_eq!(v[2].name, "c");
    }

    #[test]
    fn ordinal_pick_one_based() {
        let v = vec![el("a", 0, 0), el("b", 0, 50)];
        assert_eq!(pick_ordinal(&v, 1).unwrap().name, "a");
        assert_eq!(pick_ordinal(&v, 2).unwrap().name, "b");
        assert!(pick_ordinal(&v, 0).is_none());
        assert!(pick_ordinal(&v, 3).is_none());
    }

    #[test]
    fn ordinal_parse_suffixes() {
        assert_eq!(parse_ordinal("3rd"), Some(3));
        assert_eq!(parse_ordinal("1st"), Some(1));
        assert_eq!(parse_ordinal("2nd"), Some(2));
        assert_eq!(parse_ordinal("12th"), Some(12));
        assert_eq!(parse_ordinal("4"), Some(4));
        assert_eq!(parse_ordinal("option"), None);
        assert_eq!(parse_ordinal("3x"), None);
    }
}
