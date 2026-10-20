//! SystemCenter — Sub-Center for Windows Desktop UI Automation & Control Inspection.
//!
//! Derived from Microsoft UFO (UI-Focused Agent for Windows OS), providing
//! native Windows UI Automation (UIA) tree walking, control filtering,
//! active window management, and interactive element inspection.

use crate::center::{ConfirmKind, SubCenter, Validity};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

/// Actions owned by SystemCenter.
#[allow(dead_code)]
pub const ACTIONS: &[&str] = &[
    "system_focus_app",
    "system_minimize_window",
    "system_maximize_window",
    "system_inspect_window",
    "system_click_element",
];

pub struct SystemCenter;

impl SystemCenter {
    pub fn new() -> Self {
        SystemCenter
    }
}

impl Default for SystemCenter {
    fn default() -> Self {
        Self::new()
    }
}

fn slot_str(slots: &Value, key: &str) -> String {
    slots
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

impl SubCenter for SystemCenter {
    fn name(&self) -> &'static str {
        "SystemCenter"
    }

    fn validate(&self, action: &str, slots: &Value) -> Validity {
        match action {
            "system_focus_app" | "focus_app" => {
                let target = slot_str(slots, "target");
                let app = slot_str(slots, "app");
                let title = slot_str(slots, "title");
                if target.is_empty() && app.is_empty() && title.is_empty() {
                    Validity::NeedSlot {
                        slot: "app",
                        prompt: "Which application window should I bring to focus, sir?".to_string(),
                    }
                } else {
                    Validity::Ok
                }
            }
            "system_click_element" | "click_element" => {
                let element = slot_str(slots, "element");
                let name = slot_str(slots, "name");
                let has_x = slots.get("x").and_then(|v| v.as_f64()).is_some();
                let has_y = slots.get("y").and_then(|v| v.as_f64()).is_some();

                if element.is_empty() && name.is_empty() && !(has_x && has_y) {
                    Validity::NeedSlot {
                        slot: "element",
                        prompt: "Which UI element or button should I click, sir?".to_string(),
                    }
                } else {
                    Validity::Ok
                }
            }
            "system_inspect_window" | "system_minimize_window" | "system_maximize_window" => {
                Validity::Ok
            }
            _ => Validity::Ok,
        }
    }

    fn confirm_kind(&self, action: &str, slots: &Value) -> ConfirmKind {
        match action {
            "system_click_element" | "click_element" => {
                let elem = slot_str(slots, "element");
                let name = slot_str(slots, "name");
                let target = if !name.is_empty() {
                    name
                } else if !elem.is_empty() {
                    elem
                } else {
                    "element".to_string()
                };
                ConfirmKind::RepeatBack {
                    ack: format!("Clicking {}, sir.", target),
                }
            }
            _ => ConfirmKind::None,
        }
    }
}

/// Locate Python bridge script `server/ufo/uia_engine.py`.
pub fn resolve_ufo_script_path() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest_dir.join("..").join("server").join("ufo").join("uia_engine.py"),
        manifest_dir.join("server").join("ufo").join("uia_engine.py"),
        PathBuf::from("server").join("ufo").join("uia_engine.py"),
    ];

    for candidate in candidates {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Inspect window interactive controls via Python UFO engine bridge.
#[allow(dead_code)]
pub fn inspect_window_via_bridge(title: Option<&str>) -> Result<Value, String> {
    let script_path = resolve_ufo_script_path().ok_or_else(|| {
        "UFO UI Automation bridge script not found at server/ufo/uia_engine.py".to_string()
    })?;

    let mut cmd = Command::new("python");
    cmd.arg(&script_path).arg("inspect");

    if let Some(t) = title {
        if !t.is_empty() {
            cmd.arg("--title").arg(t);
        }
    }

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to spawn UFO bridge: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("UFO bridge exited with error: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str::<Value>(&stdout)
        .map_err(|e| format!("Failed to parse UFO JSON output: {} | raw: {}", e, stdout))
}

/// Native Windows UI Automation control inspection using `uiautomation` crate.
#[cfg(target_os = "windows")]
#[allow(dead_code)]
pub fn inspect_window_native(title_query: Option<&str>) -> Value {
    use uiautomation::controls::ControlType;
    use uiautomation::UIAutomation;

    let automation = match UIAutomation::new() {
        Ok(a) => a,
        Err(e) => return json!({ "status": "error", "message": format!("UIAutomation init error: {}", e) }),
    };

    let root = match automation.get_root_element() {
        Ok(r) => r,
        Err(e) => return json!({ "status": "error", "message": format!("Failed to get root element: {}", e) }),
    };

    let target_window = if let Some(query) = title_query {
        automation
            .create_matcher()
            .from(root.clone())
            .timeout(500)
            .control_type(ControlType::Window)
            .contains_name(query)
            .find_first()
            .ok()
    } else {
        automation.get_focused_element().ok()
    };

    let window_elem = match target_window {
        Some(w) => w,
        None => return json!({ "status": "error", "message": "Window not found" }),
    };

    let win_name = window_elem.get_name().unwrap_or_default();
    let win_rect = window_elem.get_bounding_rectangle().ok();

    json!({
        "status": "ok",
        "window": {
            "name": win_name,
            "rect": win_rect.map(|r| json!({
                "left": r.get_left(),
                "top": r.get_top(),
                "right": r.get_right(),
                "bottom": r.get_bottom(),
            })).unwrap_or(Value::Null)
        }
    })
}

#[cfg(target_os = "linux")]
pub fn inspect_window_native(_title_query: Option<&str>) -> Value {
    inspect_window_semantic()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn inspect_window_native(_title_query: Option<&str>) -> Value {
    json!({ "status": "unsupported", "message": "Native UIA requires Windows" })
}

/// Inspect the foreground window's interactive elements on Linux.
///
/// The Windows implementation walks the UI Automation tree. On Linux the
/// equivalent tree is the accessibility bus (AT-SPI), reached through the
/// semantic tier, and it answers the same question: what can the user click
/// here, and what is it called?
///
/// The operation reads every accessible name in the window, so it goes through
/// the policy gate exactly like every other semantic read. An unidentified
/// window is refused rather than reported as "no controls" — those are very
/// different answers, and only one of them is safe to act on.
#[cfg(target_os = "linux")]
pub fn inspect_window_semantic() -> Value {
    match crate::semantic::list_actionables() {
        Ok(hits) => {
            let elements: Vec<Value> = hits
                .iter()
                .map(|h| {
                    json!({
                        "name": h.name,
                        "role": h.role,
                        "tier": h.tier.as_str(),
                        "actions": h.actions,
                        "bounds": h.bounds.map(|b| json!({
                            "left": b.x, "top": b.y, "width": b.w, "height": b.h
                        })).unwrap_or(Value::Null),
                        "center": h.center().map(|(x, y)| json!({"x": x, "y": y})).unwrap_or(Value::Null),
                    })
                })
                .collect();
            let fg = crate::semantic::observed_foreground();
            let (app, title) = match &fg {
                crate::atspi::Foreground::Known { app, title } => (app.clone(), title.clone()),
                crate::atspi::Foreground::Unknown { .. } => (String::new(), String::new()),
            };
            json!({
                "status": "ok",
                "window": { "app": app, "name": title },
                "elements": elements,
            })
        }
        Err(e) => json!({
            "status": "refused",
            "message": e.spoken(),
        }),
    }
}

/// Click (activate) a named element on Linux, through the semantic tiers.
///
/// This is the Wayland-safe path: the *application* performs the action, so
/// no synthetic input is needed — no portal, no `/dev/uinput`, no consent
/// dialog. That is the whole reason the semantic tier exists.
#[cfg(target_os = "linux")]
pub fn click_element_by_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("no element name given".to_string());
    }
    match crate::semantic::activate(name, None) {
        Ok(outcome) if outcome.ok => Ok(format!(
            "Clicked {} ({}), sir.",
            outcome.name, outcome.tier.as_str()
        )),
        Ok(outcome) => Err(format!(
            "{} did not act on {name}, sir.",
            outcome.tier.as_str()
        )),
        Err(e) => Err(e.spoken()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::center::SubCenter;

    #[test]
    fn test_system_center_metadata() {
        let center = SystemCenter::new();
        assert_eq!(center.name(), "SystemCenter");
        assert_eq!(center.confirm_kind("system_focus_app", &json!({})), ConfirmKind::None);
        assert_eq!(
            center.confirm_kind("system_click_element", &json!({ "name": "OK" })),
            ConfirmKind::RepeatBack {
                ack: "Clicking OK, sir.".to_string()
            }
        );
    }

    #[test]
    fn test_system_center_validate_focus() {
        let center = SystemCenter::new();
        // Missing target
        let v1 = center.validate("system_focus_app", &json!({}));
        assert!(matches!(v1, Validity::NeedSlot { slot: "app", .. }));

        // With target
        let v2 = center.validate("system_focus_app", &json!({ "app": "notepad" }));
        assert_eq!(v2, Validity::Ok);
    }

    #[test]
    fn test_system_center_validate_click() {
        let center = SystemCenter::new();
        // Missing element
        let v1 = center.validate("system_click_element", &json!({}));
        assert!(matches!(v1, Validity::NeedSlot { slot: "element", .. }));

        // Named element
        let v2 = center.validate("system_click_element", &json!({ "name": "Submit" }));
        assert_eq!(v2, Validity::Ok);

        // Coordinates
        let v3 = center.validate("system_click_element", &json!({ "x": 100.0, "y": 200.0 }));
        assert_eq!(v3, Validity::Ok);
    }

    #[test]
    fn test_system_center_resolve_script_path() {
        let path = resolve_ufo_script_path();
        assert!(path.is_some(), "UFO script path should resolve in workspace");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_inspection_never_claims_an_empty_window_is_fine() {
        // Without an accessibility bus the inspection must REFUSE, not report
        // "status: ok, no elements" — an empty list reads as "this window has
        // no controls", which is a different claim from "I could not look".
        let v = inspect_window_native(None);
        let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
        assert!(
            status == "ok" || status == "refused",
            "unexpected status {status}"
        );
        if status == "refused" {
            assert!(
                v.get("message").and_then(|m| m.as_str()).is_some(),
                "a refusal must explain itself"
            );
        }
    }
}
