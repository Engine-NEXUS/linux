#![allow(dead_code)]

pub mod accounts;
pub mod auth;
pub mod calendar;
pub mod client;
pub mod mail;
pub mod maps;
pub mod oauth;
pub mod photos;
pub mod sentinel;
pub mod types;

use crate::center::{ConfirmKind, SubCenter, Validity};
use serde_json::Value;

/// Google Ecosystem Sub-Center: routes validated commands to Gmail, Calendar, Maps,
/// Photos, and manages background proactive intelligence via Sentinel.
pub struct GoogleCenter {
    pub sentinel: sentinel::ProactiveSentinel,
}

impl GoogleCenter {
    pub fn new() -> Self {
        Self {
            sentinel: sentinel::ProactiveSentinel::new(),
        }
    }
}

impl Default for GoogleCenter {
    fn default() -> Self {
        Self::new()
    }
}

impl SubCenter for GoogleCenter {
    fn name(&self) -> &'static str {
        "GoogleCenter"
    }

    /// Alexa Skills Kit dialog standard: validates required slots and emits
    /// exact spoken elicitation templates on missing parameters.
    fn validate(&self, action: &str, slots: &Value) -> Validity {
        match action {
            "send_email" => {
                let to = slots
                    .get("to")
                    .or_else(|| slots.get("recipient"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if to.is_empty() {
                    return Validity::NeedSlot {
                        slot: "to",
                        prompt: "Who should I send this email to, sir?".to_string(),
                    };
                }
                let body = slots
                    .get("body")
                    .or_else(|| slots.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if body.is_empty() {
                    return Validity::NeedSlot {
                        slot: "body",
                        prompt: format!("What should the email to {} say, sir?", to),
                    };
                }
                Validity::Ok
            }
            "create_event" => {
                let title = slots
                    .get("title")
                    .or_else(|| slots.get("summary"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if title.is_empty() {
                    return Validity::NeedSlot {
                        slot: "title",
                        prompt: "What is the title of the calendar event, sir?".to_string(),
                    };
                }
                let time = slots
                    .get("time")
                    .or_else(|| slots.get("start_iso"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if time.is_empty() {
                    return Validity::NeedSlot {
                        slot: "time",
                        prompt: format!("What time should I schedule {}, sir?", title),
                    };
                }
                Validity::Ok
            }
            "directions" | "navigate" => {
                let destination = slots
                    .get("destination")
                    .or_else(|| slots.get("to"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if destination.is_empty() {
                    return Validity::NeedSlot {
                        slot: "destination",
                        prompt: "Where would you like directions to, sir?".to_string(),
                    };
                }
                Validity::Ok
            }
            "search_photos" => {
                let query = slots.get("query").and_then(|v| v.as_str()).unwrap_or("").trim();
                if query.is_empty() {
                    return Validity::NeedSlot {
                        slot: "query",
                        prompt: "What photos would you like me to find, sir?".to_string(),
                    };
                }
                Validity::Ok
            }
            "unread_emails" | "today_agenda" | "search_places" | "watch_screen_email" => Validity::Ok,
            _ => Validity::Invalid {
                reason: "unknown_google_action",
                prompt: "I didn't recognize that Google command, sir.".to_string(),
            },
        }
    }

    /// Confirmation policy: destructive/external actions (sending an email) are gated;
    /// routine reads are repeat-backed or immediate.
    fn confirm_kind(&self, action: &str, slots: &Value) -> ConfirmKind {
        match action {
            "watch_screen_email" => ConfirmKind::RepeatBack {
                ack: "Scanning your screen and setting up a watch on this email, sir.".to_string(),
            },
            "send_email" => {
                let to = slots
                    .get("to")
                    .or_else(|| slots.get("recipient"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("the recipient");
                let subject = slots.get("subject").and_then(|v| v.as_str()).unwrap_or("(no subject)");
                ConfirmKind::GatePrompt {
                    prompt: format!(
                        "Ready to send email to {} with subject '{}'. Should I send it, sir?",
                        to, subject
                    ),
                    timeout_secs: 30,
                }
            }
            "create_event" => {
                let title = slots
                    .get("title")
                    .or_else(|| slots.get("summary"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("event");
                let time = slots
                    .get("time")
                    .or_else(|| slots.get("start_iso"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                ConfirmKind::RepeatBack {
                    ack: format!("Scheduling {} for {}, sir.", title, time),
                }
            }
            "directions" | "navigate" => {
                let dest = slots.get("destination").and_then(|v| v.as_str()).unwrap_or("destination");
                ConfirmKind::RepeatBack {
                    ack: format!("Finding the best route to {}, sir.", dest),
                }
            }
            "search_photos" => {
                let q = slots.get("query").and_then(|v| v.as_str()).unwrap_or("photos");
                ConfirmKind::RepeatBack {
                    ack: format!("Looking up {} in Google Photos, sir.", q),
                }
            }
            _ => ConfirmKind::None,
        }
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_google_center_validation_send_email() {
        let center = GoogleCenter::new();

        // Missing recipient
        let slots_empty = serde_json::json!({});
        let res = center.validate("send_email", &slots_empty);
        assert_eq!(
            res,
            Validity::NeedSlot {
                slot: "to",
                prompt: "Who should I send this email to, sir?".to_string(),
            }
        );

        // Missing body
        let slots_with_to = serde_json::json!({ "to": "colleague@work.com" });
        let res2 = center.validate("send_email", &slots_with_to);
        assert_eq!(
            res2,
            Validity::NeedSlot {
                slot: "body",
                prompt: "What should the email to colleague@work.com say, sir?".to_string(),
            }
        );

        // Complete slots
        let slots_valid = serde_json::json!({
            "to": "colleague@work.com",
            "body": "Meeting notes attached."
        });
        assert_eq!(center.validate("send_email", &slots_valid), Validity::Ok);
    }

    #[test]
    fn test_google_center_validation_calendar_and_maps() {
        let center = GoogleCenter::new();

        // Calendar missing time
        let slots_ev = serde_json::json!({ "title": "Design Sync" });
        assert_eq!(
            center.validate("create_event", &slots_ev),
            Validity::NeedSlot {
                slot: "time",
                prompt: "What time should I schedule Design Sync, sir?".to_string(),
            }
        );

        // Maps missing destination
        let slots_nav = serde_json::json!({});
        assert_eq!(
            center.validate("directions", &slots_nav),
            Validity::NeedSlot {
                slot: "destination",
                prompt: "Where would you like directions to, sir?".to_string(),
            }
        );

        // Valid directions
        let slots_nav_ok = serde_json::json!({ "destination": "SFO Airport" });
        assert_eq!(center.validate("directions", &slots_nav_ok), Validity::Ok);
    }

    #[test]
    fn test_google_center_confirm_kind() {
        let center = GoogleCenter::new();

        // Destructive / Outgoing: GatePrompt
        let send_slots = serde_json::json!({ "to": "investor@venture.com", "subject": "Pitch Deck" });
        match center.confirm_kind("send_email", &send_slots) {
            ConfirmKind::GatePrompt { prompt, timeout_secs } => {
                assert!(prompt.contains("investor@venture.com"));
                assert_eq!(timeout_secs, 30);
            }
            _ => panic!("Expected GatePrompt for send_email"),
        }

        // Routine: RepeatBack
        let nav_slots = serde_json::json!({ "destination": "Starbucks" });
        match center.confirm_kind("directions", &nav_slots) {
            ConfirmKind::RepeatBack { ack } => {
                assert!(ack.contains("Starbucks"));
            }
            _ => panic!("Expected RepeatBack for directions"),
        }
    }
}
