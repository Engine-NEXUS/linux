#![allow(dead_code)]

use super::types::{
    DeadlineUpdate, DraftReceipt, GoogleError, MailMessage, MailThread, SendReceipt,
    ThreadUpdateEvent, ThreadWatchTarget, WatchStatus,
};
use base64::{engine::general_purpose::URL_SAFE, Engine as _};
use regex::Regex;
use serde_json::Value;

/// Gmail domain service.
pub struct MailService;

impl MailService {
    /// Extract deadline updates, submission extensions, or rescheduled times
    /// from an incoming email snippet or body text.
    pub fn parse_deadline_update(body: &str) -> Option<DeadlineUpdate> {
        let clean = body.replace("\r\n", " ").replace('\n', " ");

        // Patterns for deadline extensions, reschedules, and time updates
        // e.g.: "deadline ... extended to tomorrow at 5:00 PM"
        //       "submission due date is moved to Friday 3 PM"
        //       "meeting rescheduled to 4 PM"
        let re = Regex::new(
            r"(?i)(?:(?:assignment|project|submission|report|meeting)\s+)?(?:deadline|due\s+date|submission|assignment|report|meeting|presentation)\s+(?:is\s+|was\s+|has\s+been\s+|got\s+)?(?:moved|extended|postponed|rescheduled|changed|delayed|pushed)\s+(?:to|until)\s+([A-Za-z0-9:,\s]{3,35}?)(?:\.|\n|$)"
        ).ok()?;

        if let Some(caps) = re.captures(&clean) {
            let matched_text = caps.get(0)?.as_str().trim().to_string();
            let new_deadline_raw = caps.get(1)?.as_str().trim().to_string();
            let is_extended = clean.to_lowercase().contains("extended") || clean.to_lowercase().contains("postponed") || clean.to_lowercase().contains("pushed");

            return Some(DeadlineUpdate {
                context_phrase: matched_text,
                task_or_subject: Self::extract_subject_keyword(&clean),
                new_deadline_raw,
                is_extended,
                confidence: 0.92,
            });
        }

        // Secondary pattern: "new deadline is <time>" or "new due date: <time>"
        let re_new = Regex::new(
            r"(?i)(?:new\s+deadline|new\s+due\s+date|rescheduled\s+to)(?:\s+is|\s*:)?\s+([A-Za-z0-9:,\s]{3,35}?)(?:\.|\n|$)"
        ).ok()?;

        if let Some(caps) = re_new.captures(&clean) {
            let matched_text = caps.get(0)?.as_str().trim().to_string();
            let new_deadline_raw = caps.get(1)?.as_str().trim().to_string();

            return Some(DeadlineUpdate {
                context_phrase: matched_text,
                task_or_subject: Self::extract_subject_keyword(&clean),
                new_deadline_raw,
                is_extended: true,
                confidence: 0.88,
            });
        }

        // Tertiary pattern: initial stated deadline "due date is <time>", "deadline is/by <time>"
        let re_initial = Regex::new(
            r"(?i)(?:deadline|due\s+date|submission\s+deadline)\s+(?:is\s+|:\s*|by\s+)([A-Za-z0-9:,\s]{3,35}?)(?:\.|\n|$)"
        ).ok()?;

        if let Some(caps) = re_initial.captures(&clean) {
            let matched_text = caps.get(0)?.as_str().trim().to_string();
            let new_deadline_raw = caps.get(1)?.as_str().trim().to_string();

            return Some(DeadlineUpdate {
                context_phrase: matched_text,
                task_or_subject: Self::extract_subject_keyword(&clean),
                new_deadline_raw,
                is_extended: false,
                confidence: 0.85,
            });
        }

        None
    }

    fn extract_subject_keyword(text: &str) -> String {
        let lower = text.to_lowercase();
        if lower.contains("project") {
            "Project".to_string()
        } else if lower.contains("assignment") {
            "Assignment".to_string()
        } else if lower.contains("report") {
            "Report".to_string()
        } else if lower.contains("meeting") {
            "Meeting".to_string()
        } else if lower.contains("submission") {
            "Submission".to_string()
        } else {
            "Task".to_string()
        }
    }

    /// Encodes to RFC 2822 base64url for Gmail API `raw` field.
    pub fn encode_rfc2822(to: &str, subject: &str, body: &str) -> String {
        let rfc = format!(
            "To: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=\"UTF-8\"\r\n\r\n{}",
            to, subject, body
        );
        URL_SAFE.encode(rfc)
    }

    /// Parse Gmail thread list response into structured `MailThread` objects.
    pub fn parse_threads_json(json: &Value) -> Result<Vec<MailThread>, GoogleError> {
        let threads = json
            .get("threads")
            .and_then(|t| t.as_array())
            .ok_or_else(|| GoogleError::SerializationError("Missing threads array in response".into()))?;

        let mut list = Vec::new();
        for t in threads {
            let id = t.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let snippet = t.get("snippet").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let history_id = t.get("historyId").and_then(|v| v.as_str()).map(|s| s.to_string());

            list.push(MailThread {
                id,
                snippet,
                history_id,
                sender: String::new(),
                subject: String::new(),
                timestamp_ms: 0,
                is_unread: false,
            });
        }
        Ok(list)
    }

    /// Parse Gmail message payload JSON into structured `MailMessage`.
    pub fn parse_message_json(json: &Value) -> Result<MailMessage, GoogleError> {
        let id = json.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let thread_id = json.get("threadId").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let timestamp_ms = json.get("internalDate")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let mut from = String::new();
        let mut to = String::new();
        let mut subject = String::new();

        if let Some(headers) = json.pointer("/payload/headers").and_then(|h| h.as_array()) {
            for h in headers {
                let name = h.get("name").and_then(|v| v.as_str()).unwrap_or_default();
                let value = h.get("value").and_then(|v| v.as_str()).unwrap_or_default();
                if name.eq_ignore_ascii_case("from") {
                    from = value.to_string();
                } else if name.eq_ignore_ascii_case("to") {
                    to = value.to_string();
                } else if name.eq_ignore_ascii_case("subject") {
                    subject = value.to_string();
                }
            }
        }

        let body_text = json.get("snippet").and_then(|v| v.as_str()).unwrap_or_default().to_string();

        Ok(MailMessage {
            id,
            thread_id,
            from,
            to,
            subject,
            body_text,
            timestamp_ms,
        })
    }

    /// Parse send receipt JSON response from Gmail API
    pub fn parse_send_receipt_json(json: &Value, recipient: &str, subject: &str) -> Result<SendReceipt, GoogleError> {
        let message_id = json.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let thread_id = json.get("threadId").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        Ok(SendReceipt {
            message_id,
            thread_id,
            recipient: recipient.to_string(),
            subject: subject.to_string(),
        })
    }

    /// Parse draft receipt JSON response from Gmail API
    pub fn parse_draft_receipt_json(json: &Value, recipient: &str) -> Result<DraftReceipt, GoogleError> {
        let draft_id = json.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let thread_id = json.pointer("/message/threadId").and_then(|v| v.as_str()).map(|s| s.to_string());
        Ok(DraftReceipt {
            draft_id,
            thread_id,
            recipient: recipient.to_string(),
        })
    }

    /// Construct a new `ThreadWatchTarget` recording initial thread state and any existing deadline.
    pub fn create_thread_watch(
        watch_id: String,
        thread_id: String,
        account_email: Option<String>,
        sender: String,
        subject: String,
        initial_messages: &[MailMessage],
        initial_history_id: Option<String>,
    ) -> ThreadWatchTarget {
        // Inspect the latest message to see if there is already an active deadline
        let initial_deadline_raw = initial_messages
            .last()
            .and_then(|m| Self::parse_deadline_update(&m.body_text))
            .map(|dl| dl.new_deadline_raw);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        ThreadWatchTarget {
            watch_id,
            thread_id,
            account_email,
            initial_history_id,
            sender,
            subject,
            initial_deadline_raw,
            message_count: initial_messages.len(),
            created_at_ms: now,
            last_checked_ms: now,
            status: WatchStatus::Active,
        }
    }

    /// Native Gmail Engine diffing: compares a watched thread against updated messages.
    /// Detects deadline modifications, new replies, or no changes.
    pub fn diff_thread_state(
        watch: &ThreadWatchTarget,
        current_messages: &[MailMessage],
    ) -> Option<ThreadUpdateEvent> {
        if current_messages.len() <= watch.message_count {
            return None;
        }

        // Inspect each new message that arrived since the watch was created
        for msg in &current_messages[watch.message_count..] {
            if let Some(dl) = Self::parse_deadline_update(&msg.body_text) {
                if Some(&dl.new_deadline_raw) != watch.initial_deadline_raw.as_ref() {
                    return Some(ThreadUpdateEvent::DeadlineChanged {
                        old_deadline: watch.initial_deadline_raw.clone().unwrap_or_else(|| "none".into()),
                        new_deadline: dl.new_deadline_raw,
                        snippet: dl.context_phrase,
                    });
                }
            }

            return Some(ThreadUpdateEvent::NewReply {
                sender: msg.from.clone(),
                snippet: msg.body_text.chars().take(120).collect(),
            });
        }

        None
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_thread_watch_and_diff_deadline() {
        let msg1 = MailMessage {
            id: "m1".into(),
            thread_id: "t1".into(),
            from: "prof@university.edu".into(),
            to: "student@nexus.internal".into(),
            subject: "Final Project Guidelines".into(),
            body_text: "Due date is Friday 3 PM.".into(),
            timestamp_ms: 1000,
        };

        let watch = MailService::create_thread_watch(
            "w_001".into(),
            "t1".into(),
            None,
            "prof@university.edu".into(),
            "Final Project Guidelines".into(),
            &[msg1.clone()],
            Some("10099".into()),
        );

        assert_eq!(watch.message_count, 1);
        assert_eq!(watch.initial_deadline_raw, Some("Friday 3 PM".into()));
        assert_eq!(watch.status, WatchStatus::Active);

        // Same messages: no change
        assert!(MailService::diff_thread_state(&watch, &[msg1.clone()]).is_none());

        // New message arrives extending deadline
        let msg2 = MailMessage {
            id: "m2".into(),
            thread_id: "t1".into(),
            from: "prof@university.edu".into(),
            to: "student@nexus.internal".into(),
            subject: "Re: Final Project Guidelines".into(),
            body_text: "Due to server issues, deadline is extended to Monday 9 AM.".into(),
            timestamp_ms: 2000,
        };

        let event = MailService::diff_thread_state(&watch, &[msg1.clone(), msg2.clone()]).unwrap();
        match event {
            ThreadUpdateEvent::DeadlineChanged { old_deadline, new_deadline, .. } => {
                assert_eq!(old_deadline, "Friday 3 PM");
                assert_eq!(new_deadline, "Monday 9 AM");
            }
            _ => panic!("Expected DeadlineChanged event"),
        }

        // New message without deadline: emits NewReply
        let msg3 = MailMessage {
            id: "m3".into(),
            thread_id: "t1".into(),
            from: "ta@university.edu".into(),
            to: "student@nexus.internal".into(),
            subject: "Re: Final Project Guidelines".into(),
            body_text: "Office hours are open right now if anyone has questions.".into(),
            timestamp_ms: 3000,
        };

        let event_reply = MailService::diff_thread_state(&watch, &[msg1, msg3]).unwrap();
        match event_reply {
            ThreadUpdateEvent::NewReply { sender, snippet } => {
                assert_eq!(sender, "ta@university.edu");
                assert!(snippet.contains("Office hours"));
            }
            _ => panic!("Expected NewReply event"),
        }
    }

    #[test]
    fn test_receipt_parsing() {
        let send_raw = serde_json::json!({
            "id": "18ac5d7e3",
            "threadId": "18ac5d7e3"
        });
        let send = MailService::parse_send_receipt_json(&send_raw, "boss@company.com", "Status Update").unwrap();
        assert_eq!(send.message_id, "18ac5d7e3");
        assert_eq!(send.recipient, "boss@company.com");

        let draft_raw = serde_json::json!({
            "id": "draft_9921",
            "message": { "id": "msg_8842", "threadId": "th_123" }
        });
        let draft = MailService::parse_draft_receipt_json(&draft_raw, "team@nexus.internal").unwrap();
        assert_eq!(draft.draft_id, "draft_9921");
        assert_eq!(draft.recipient, "team@nexus.internal");
    }

    #[test]
    fn test_deadline_extension_parsing() {
        let email = "Dear students, the project submission deadline has been extended to tomorrow at 5:00 PM. Please make sure your code is pushed.";
        let res = MailService::parse_deadline_update(email);
        assert!(res.is_some());
        let update = res.unwrap();
        assert!(update.is_extended);
        assert_eq!(update.new_deadline_raw, "tomorrow at 5:00 PM");
        assert_eq!(update.task_or_subject, "Project");
    }

    #[test]
    fn test_assignment_due_date_moved() {
        let email = "Hello, assignment due date is moved to Friday 3 PM.";
        let res = MailService::parse_deadline_update(email);
        assert!(res.is_some());
        let update = res.unwrap();
        assert_eq!(update.new_deadline_raw, "Friday 3 PM");
        assert_eq!(update.task_or_subject, "Assignment");
    }

    #[test]
    fn test_meeting_rescheduled() {
        let email = "The sprint planning meeting has been rescheduled to Thursday at 11 AM.";
        let res = MailService::parse_deadline_update(email);
        assert!(res.is_some());
        let update = res.unwrap();
        assert_eq!(update.new_deadline_raw, "Thursday at 11 AM");
        assert_eq!(update.task_or_subject, "Meeting");
    }

    #[test]
    fn test_secondary_pattern_new_deadline_is() {
        let email = "Urgent: the new deadline is next Monday at 9 AM.";
        let res = MailService::parse_deadline_update(email);
        assert!(res.is_some());
        let update = res.unwrap();
        assert_eq!(update.new_deadline_raw, "next Monday at 9 AM");
    }

    #[test]
    fn test_reject_routine_email() {
        let email = "Hey, are we still meeting for lunch today? Let me know!";
        assert!(MailService::parse_deadline_update(email).is_none());
    }

    #[test]
    fn test_encode_rfc2822() {
        let encoded = MailService::encode_rfc2822("test@example.com", "Hello", "How are you?");
        assert!(!encoded.is_empty());
        let decoded_bytes = URL_SAFE.decode(&encoded).unwrap();
        let decoded = String::from_utf8(decoded_bytes).unwrap();
        assert!(decoded.contains("To: test@example.com"));
        assert!(decoded.contains("Subject: Hello"));
        assert!(decoded.contains("How are you?"));
    }

    #[test]
    fn test_parse_message_json() {
        let raw = serde_json::json!({
            "id": "msg_123",
            "threadId": "th_456",
            "snippet": "Project deadline moved to Friday 5 PM.",
            "internalDate": "1727600000000",
            "payload": {
                "headers": [
                    { "name": "From", "value": "prof@university.edu" },
                    { "name": "To", "value": "student@university.edu" },
                    { "name": "Subject", "value": "CS101 Final Project Deadline" }
                ]
            }
        });

        let msg = MailService::parse_message_json(&raw).unwrap();
        assert_eq!(msg.id, "msg_123");
        assert_eq!(msg.thread_id, "th_456");
        assert_eq!(msg.from, "prof@university.edu");
        assert_eq!(msg.subject, "CS101 Final Project Deadline");
        assert_eq!(msg.timestamp_ms, 1727600000000);
    }
}
