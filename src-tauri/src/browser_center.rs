//! BrowserCenter — per-action sub-center for the browser family.
//!
//! Doc 74 P4 (first family): one module per action family implementing the
//! shared `SubCenter` contract. Design rules for this migration:
//!
//! 1. ADAPT + DELEGATE, never duplicate: `(action, slots)` converts to the
//!    canonical `ParsedIntent`, and policy (validate) delegates to the Main
//!    Center gate — the single source of truth. Only family-specific
//!    confirm wording lives here.
//! 2. NO execution moves in P4: runners stay where they are
//!    (`run_browser_*` in orchestrator, hotkeys in command_executor).
//!    Execution migrates only when the trait gains an `execute` method
//!    (joint decision — not unilaterally).
//! 3. Unknown actions fail closed: `Invalid`, never `Ok`.

use crate::center::{ConfirmKind, SubCenter, Validity};
use crate::intent_parser::ParsedIntent;
use serde_json::Value;

use std::path::PathBuf;
use std::process::Command;

/// Actions this center owns. Mirrors the Browser arms of `center_for`.
#[allow(dead_code)]
pub const ACTIONS: &[&str] = &[
    "browser_tab",
    "browser_close_tab",
    "browser_search",
    "browser_search_focus",
    "browser_new_tab",
    "browser_navigate",
    "browser_read_page",
    "browser_extract_elements",
];

pub struct BrowserCenter;

impl BrowserCenter {
    pub fn new() -> Self {
        BrowserCenter
    }
}

fn slot_str(slots: &Value, key: &str) -> String {
    slots
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn slot_u32(slots: &Value, key: &str) -> u32 {
    slots.get(key).and_then(|v| v.as_u64()).unwrap_or(0) as u32
}

/// Locate Python bridge script `server/crawler/engine.py`.
pub fn resolve_crawler_script_path() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest_dir.join("..").join("server").join("crawler").join("engine.py"),
        manifest_dir.join("server").join("crawler").join("engine.py"),
        PathBuf::from("server").join("crawler").join("engine.py"),
    ];

    for candidate in candidates {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Read URL content as clean markdown via the browser-use crawler engine.
#[allow(dead_code)]
pub fn read_page_markdown(url: &str) -> Result<String, String> {
    let script_path = resolve_crawler_script_path().ok_or_else(|| {
        "Crawler bridge script not found at server/crawler/engine.py".to_string()
    })?;

    let output = Command::new("python")
        .arg(&script_path)
        .arg("read-page")
        .arg(url)
        .output()
        .map_err(|e| format!("Failed to spawn crawler engine: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Crawler failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout)
        .map_err(|e| format!("Failed to parse crawler output: {}", e))?;

    if parsed.get("status").and_then(|s| s.as_str()) == Some("error") {
        let msg = parsed.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown crawler error");
        return Err(msg.to_string());
    }

    Ok(parsed.get("markdown").and_then(|m| m.as_str()).unwrap_or("").to_string())
}

/// Canonical adapter: family (action, slots) → `ParsedIntent`. `None` =
/// not a browser action (caller fails closed).
#[allow(dead_code)]
fn to_intent(action: &str, slots: &Value) -> Option<ParsedIntent> {
    match action {
        "browser_tab" => Some(ParsedIntent::BrowserTab {
            index: slot_u32(slots, "index"),
        }),
        "browser_close_tab" => {
            let n = slot_u32(slots, "index");
            Some(ParsedIntent::BrowserCloseTab {
                index: if n == 0 { None } else { Some(n) },
            })
        }
        "browser_search" => Some(ParsedIntent::BrowserSearch {
            query: slot_str(slots, "query"),
        }),
        "browser_search_focus" => Some(ParsedIntent::BrowserSearchFocus),
        // Pass through live verbs & browser-use crawler intents
        "browser_new_tab" | "browser_navigate" | "browser_read_page" | "browser_extract_elements" => {
            Some(ParsedIntent::NluResult {
                intent: action.to_string(),
                slots: slots.clone(),
                confidence: 1.0,
            })
        }
        _ => None,
    }
}

impl SubCenter for BrowserCenter {
    fn name(&self) -> &'static str {
        "BrowserCenter"
    }

    fn validate(&self, action: &str, slots: &Value) -> Validity {
        // Family value rules the generic gate doesn't cover: tab indices
        // live in 1..=9 (Ctrl+1..9). Out-of-range names the problem.
        if action == "browser_tab" {
            let n = slot_u32(slots, "index");
            if n == 0 || n > 9 {
                return Validity::Invalid {
                    reason: "tab index out of range",
                    prompt: "I can only switch to tabs 1 to 9, sir.".to_string(),
                };
            }
            return Validity::Ok;
        }
        if action == "browser_close_tab" {
            if let Some(n) = slots.get("index").and_then(|v| v.as_u64()) {
                if n == 0 || n > 9 {
                    return Validity::Invalid {
                        reason: "tab index out of range",
                        prompt: "I can only close tabs 1 to 9, sir.".to_string(),
                    };
                }
            }
            return Validity::Ok;
        }
        if action == "browser_search" {
            let query = slot_str(slots, "query");
            if query.trim().is_empty() {
                return Validity::NeedSlot {
                    slot: "query",
                    prompt: "What should I search for in the browser, sir?".to_string(),
                };
            }
            return Validity::Ok;
        }
        if action == "browser_search_focus" || action == "browser_new_tab" {
            return Validity::Ok;
        }
        if action == "browser_navigate" || action == "browser_read_page" || action == "browser_extract_elements" {
            let url = slot_str(slots, "url");
            if url.trim().is_empty() {
                return Validity::NeedSlot {
                    slot: "url",
                    prompt: "Which website or URL should I inspect, sir?".to_string(),
                };
            }
            return Validity::Ok;
        }
        Validity::Invalid {
            reason: "not a browser action",
            prompt: "That's not a browser action, sir.".to_string(),
        }
    }

    fn confirm_kind(&self, action: &str, slots: &Value) -> ConfirmKind {
        // Routine, reversible locally: repeat-back, never a gate.
        let ack = match action {
            "browser_tab" => format!("Switching to tab {}, sir.", slot_u32(slots, "index")),
            "browser_close_tab" => match slot_u32(slots, "index") {
                0 => "Closing this tab, sir.".to_string(),
                n => format!("Closing tab {n}, sir."),
            },
            "browser_search" => format!("Searching for {}, sir.", slot_str(slots, "query")),
            "browser_search_focus" => "Ready to search, sir.".to_string(),
            "browser_new_tab" => "Opening a new tab, sir.".to_string(),
            "browser_navigate" => format!("Opening {}, sir.", slot_str(slots, "url")),
            "browser_read_page" => format!("Reading {}, sir.", slot_str(slots, "url")),
            "browser_extract_elements" => {
                format!("Inspecting interactive elements on {}, sir.", slot_str(slots, "url"))
            }
            _ => return ConfirmKind::None,
        };
        ConfirmKind::RepeatBack { ack }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v() -> BrowserCenter {
        BrowserCenter::new()
    }

    #[test]
    fn test_name_and_actions() {
        assert_eq!(v().name(), "BrowserCenter");
        assert!(ACTIONS.contains(&"browser_tab"));
        assert!(ACTIONS.contains(&"browser_close_tab"));
        assert!(ACTIONS.contains(&"browser_new_tab"));
        assert!(ACTIONS.contains(&"browser_read_page"));
        assert!(ACTIONS.contains(&"browser_extract_elements"));
    }

    /// Policy delegates to the Main Center gate: valid passes, out-of-range
    /// and empty slots fail with spoken prompts, foreign actions fail closed.
    #[test]
    fn test_validate_matrix() {
        assert_eq!(
            v().validate("browser_tab", &json!({ "index": 3 })),
            Validity::Ok
        );
        assert!(matches!(
            v().validate("browser_tab", &json!({ "index": 15 })),
            Validity::Invalid { .. }
        ));
        assert!(matches!(
            v().validate("browser_close_tab", &json!({})),
            Validity::Ok
        ));
        assert!(matches!(
            v().validate("browser_search", &json!({ "query": " " })),
            Validity::NeedSlot { .. }
        ));
        assert!(matches!(
            v().validate("browser_new_tab", &json!({})),
            Validity::Ok
        ));
        assert!(matches!(
            v().validate("browser_read_page", &json!({})),
            Validity::NeedSlot { slot: "url", .. }
        ));
        assert_eq!(
            v().validate("browser_read_page", &json!({ "url": "https://example.com" })),
            Validity::Ok
        );
        assert!(matches!(
            v().validate("send_email", &json!({})),
            Validity::Invalid { .. }
        ));
    }

    /// Every owned action gets a repeat-back; foreign actions get None.
    #[test]
    fn test_confirm_repeat_backs() {
        for action in ACTIONS {
            let slots = match *action {
                "browser_tab" | "browser_close_tab" => json!({ "index": 2 }),
                "browser_search" => json!({ "query": "cats" }),
                "browser_navigate" | "browser_read_page" | "browser_extract_elements" => {
                    json!({ "url": "example.com" })
                }
                _ => json!({}),
            };
            match v().confirm_kind(action, &slots) {
                ConfirmKind::RepeatBack { ack } => {
                    assert!(ack.ends_with("sir."), "{action}: {ack}")
                }
                other => panic!("{action} must repeat back, got {other:?}"),
            }
        }
        assert_eq!(
            v().confirm_kind("send_email", &json!({})),
            ConfirmKind::None
        );
    }

    #[test]
    fn test_resolve_crawler_path() {
        assert!(resolve_crawler_script_path().is_some());
    }
}
