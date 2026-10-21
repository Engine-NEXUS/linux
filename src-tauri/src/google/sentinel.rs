#![allow(dead_code)]

use super::types::{AlertUrgency, CommuteDelay, DeadlineUpdate, MailMessage, ProactiveAlert};
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, Runtime};

// ─── Sentinel orb-landing payload (tracking-first contract) ───────────
// Builds the exact `orchestrator:sentinel-alert` wire shape from the Orb UI
// & Landing Specification. Pure + tested; the polling loop below only
// fills it in and emits it.

/// Deterministic brand palette for org avatars (domain-hashed fallback
/// until an organization directory exists).
const ORG_BRAND_COLORS: &[&str] = &[
    "#4285F4", "#EA4335", "#FBBC04", "#34A853", "#8E24AA", "#00ACC1", "#F4511E", "#3949AB",
];

/// Derive an organization block from a sender string
/// ("Dr. Henderson <henderson@acme.edu>" → Acme/acme.edu).
/// Fallback only — a real org directory should replace this (follow-up).
pub fn organization_from_sender(sender: &str) -> super::types::AlertOrganization {
    let email = sender
        .rfind('<')
        .and_then(|l| sender.rfind('>').map(|r| sender[l + 1..r].to_string()))
        .unwrap_or_else(|| sender.to_string());
    let domain = email
        .rfind('@')
        .map(|i| email[i + 1..].trim().to_lowercase())
        .filter(|d| !d.is_empty() && d.contains('.'))
        .unwrap_or_else(|| "unknown".to_string());
    let stem = domain.split('.').next().unwrap_or("unknown");
    let mut name_chars = stem.chars();
    let name = match name_chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + name_chars.as_str(),
        None => "Unknown".to_string(),
    };
    let initials: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase();
    let initials = if initials.len() >= 2 {
        initials
    } else {
        name.chars().take(2).collect::<String>().to_uppercase()
    };
    let hash: usize = domain.bytes().map(|b| b as usize).sum();
    super::types::AlertOrganization {
        name,
        brand_color: ORG_BRAND_COLORS[hash % ORG_BRAND_COLORS.len()].to_string(),
        avatar_fallback_initials: initials,
        domain,
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Build the spec payload. `deadline` is (previous, new) for deadline
/// motion, None for replies/attachments. `is_extended` defaults true —
/// direction needs date parsing (follow-up); urgency mirrors the existing
/// synthesis semantics (deadline High, reply Medium, attachment Low).
pub fn build_alert_payload(
    kind: &str,
    watch: &super::types::ThreadWatchTarget,
    sender: &str,
    snippet: &str,
    deadline: Option<(String, String)>,
    urgency: AlertUrgency,
    spoken: String,
) -> super::types::SentinelAlertPayload {
    let org = organization_from_sender(sender);
    let deadline_block = match deadline {
        Some((previous_deadline, new_deadline)) => {
            Some(super::types::AlertDeadline {
                is_extended: true,
                previous_deadline,
                new_deadline,
                urgency,
            })
        }
        None => None,
    };
    super::types::SentinelAlertPayload {
        alert_id: format!("sentinel_{kind}_{}", now_secs()),
        source: "Gmail".to_string(),
        account_email: watch.account_email.clone(),
        organization: org,
        context: super::types::AlertContext {
            thread_id: watch.thread_id.clone(),
            subject: watch.subject.clone(),
            sender: sender.to_string(),
            snippet: snippet.to_string(),
        },
        deadline: deadline_block,
        landing_animation: super::types::LandingAnimation {
            initial_state: "incoming_pulse".to_string(),
            docked_state: "side_pill_pill".to_string(),
            auto_collapse_after_ms: 7000,
        },
        spoken_notification: spoken,
    }
}

/// Ring buffer tracking recently alerted items to prevent repetitive interruptions.
pub struct SeenMessageRingBuffer {
    max_capacity: usize,
    items: Vec<(String, u64)>, // (id, timestamp_ms)
    set: HashSet<String>,
}

impl SeenMessageRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            max_capacity: capacity,
            items: Vec::with_capacity(capacity),
            set: HashSet::with_capacity(capacity),
        }
    }

    /// Returns true if the item was newly inserted; false if it was already seen.
    pub fn insert(&mut self, id: &str) -> bool {
        if self.set.contains(id) {
            return false;
        }

        if self.items.len() >= self.max_capacity {
            let (oldest, _) = self.items.remove(0);
            self.set.remove(&oldest);
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.items.push((id.to_string(), now));
        self.set.insert(id.to_string());
        true
    }

    pub fn contains(&self, id: &str) -> bool {
        self.set.contains(id)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Proactive Sentinel: processes background signals from Gmail, Calendar, Maps
/// into prioritized spoken/UI alerts for the user.
pub struct ProactiveSentinel {
    seen_buffer: Mutex<SeenMessageRingBuffer>,
}

impl ProactiveSentinel {
    pub fn new() -> Self {
        Self {
            seen_buffer: Mutex::new(SeenMessageRingBuffer::new(500)),
        }
    }

    /// Synthesize an alert when an email indicates a deadline or submission change.
    pub fn synthesize_mail_deadline_alert(
        &self,
        mail: &MailMessage,
        deadline: &DeadlineUpdate,
    ) -> Option<ProactiveAlert> {
        let alert_id = format!("mail_dl_{}", mail.id);

        let mut buf = self.seen_buffer.lock().unwrap();
        if !buf.insert(&alert_id) {
            return None; // Already alerted for this email
        }

        let sender_name = mail
            .from
            .split('<')
            .next()
            .unwrap_or(&mail.from)
            .trim()
            .trim_matches('"');

        let spoken_line = format!(
            "Sir, email from {} states the {} deadline has been {} to {}.",
            sender_name,
            deadline.task_or_subject.to_lowercase(),
            if deadline.is_extended { "extended" } else { "updated" },
            deadline.new_deadline_raw
        );

        let title = format!("Deadline Update: {}", deadline.task_or_subject);
        let summary = format!("From {}: {}", sender_name, deadline.context_phrase);

        Some(ProactiveAlert {
            id: alert_id,
            source: "Gmail".to_string(),
            title,
            summary,
            spoken_line,
            urgency: AlertUrgency::High,
            timestamp_ms: mail.timestamp_ms,
        })
    }

    /// Synthesize an alert for traffic delay impacting an upcoming commitment.
    pub fn synthesize_commute_alert(
        &self,
        event_title: &str,
        commute: &CommuteDelay,
    ) -> Option<ProactiveAlert> {
        if !commute.is_congested {
            return None;
        }

        let alert_key = format!("commute_{}_{}", commute.destination, commute.delay_minutes);
        let mut buf = self.seen_buffer.lock().unwrap();
        if !buf.insert(&alert_key) {
            return None;
        }

        let spoken_line = format!(
            "Sir, heavy traffic to {}. Travel time is now {} minutes, a delay of {} minutes. Suggest departing soon.",
            commute.destination, commute.current_duration_min, commute.delay_minutes
        );

        let title = format!("Commute Delay: {}", commute.destination);
        let summary = format!(
            "{} delay of {}m for {}",
            commute.destination, commute.delay_minutes, event_title
        );

        Some(ProactiveAlert {
            id: alert_key,
            source: "Google Maps".to_string(),
            title,
            summary,
            spoken_line,
            urgency: AlertUrgency::Medium,
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        })
    }

    /// Checks if the alert should be spoken immediately based on urgency threshold.
    pub fn should_speak_immediately(urgency: AlertUrgency, min_threshold: AlertUrgency) -> bool {
        Self::urgency_rank(urgency) >= Self::urgency_rank(min_threshold)
    }

    fn urgency_rank(u: AlertUrgency) -> u8 {
        match u {
            AlertUrgency::Low => 1,
            AlertUrgency::Medium => 2,
            AlertUrgency::High => 3,
            AlertUrgency::Critical => 4,
        }
    }
}

/// Triggers an immediate proactive check and ensures background sentinel polling loop is active.
pub fn trigger_proactive_check<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        poll_sentinel_targets(&app).await;
    });
}

/// Start background sentinel polling loop.
pub fn start_sentinel_poller<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        tracing::info!("sentinel: background proactive polling loop started");
        println!("[SENTINEL] Background proactive sentinel poller online (interval: 15s)");
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
            poll_sentinel_targets(&app).await;
        }
    });
}

/// Polls active targets from memory and checks for updates.
pub async fn poll_sentinel_targets<R: Runtime>(app: &AppHandle<R>) {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("com.nexus.assistant"));

    let watches = crate::memory::load_mail_watches(&app_data_dir);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let active_watches: Vec<_> = watches
        .into_iter()
        .filter(|w| {
            if w.status != crate::google::types::WatchStatus::Active {
                return false;
            }
            // Synthetic local screen thread targets expire after 2 hours
            if w.thread_id.starts_with("screen_thread_") && now_ms.saturating_sub(w.created_at_ms) > 2 * 3600 * 1000 {
                return false;
            }
            true
        })
        .collect();

    if active_watches.is_empty() {
        return;
    }

    // If no Google auth is available, don't spam poll cycle lines every 15 seconds
    let has_any_token = crate::auth_vault::get_token_for_google_account(None).is_some()
        || crate::google::auth::GoogleAuth::get_access_token().is_ok()
        || !crate::auth_vault::get_google_accounts().is_empty();

    if !has_any_token {
        static NOTED_NO_AUTH: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !NOTED_NO_AUTH.swap(true, std::sync::atomic::Ordering::Relaxed) {
            println!("[SENTINEL] Note: Google account not authenticated yet. Add token in Settings or set GOOGLE_ACCESS_TOKEN. (Poller idle until connected.)");
        }
        return;
    }

    // Per-cycle header (1 line): proves the loop is alive and states the
    // workload. Per-watch stage lines below trace token → fetch → diff →
    // emit/speak so any failure is locatable from the console alone.
    println!(
        "[SENTINEL] poll cycle: {} active watch(es)",
        active_watches.len()
    );
    for watch in active_watches {
        println!(
            "[SENTINEL] watch '{}' ('{}'): checking…",
            watch.thread_id, watch.subject
        );

        let account_email_ref = watch.account_email.as_deref();
        let token_res = match crate::auth_vault::get_token_for_google_account(account_email_ref) {
            Some(t) => Ok(t),
            None => {
                if let Some(email) = account_email_ref {
                    crate::google::oauth::refresh_access_token_for(email).await
                } else {
                    crate::google::auth::GoogleAuth::get_access_token().map_err(|e| e.to_string())
                }
            }
        };

        if let Ok(token) = token_res {
            println!(
                "[SENTINEL] watch '{}': token ok → fetching Gmail thread…",
                watch.thread_id
            );
            if let Ok(client) = crate::google::client::GoogleClient::new(token) {
                let url = format!(
                    "https://gmail.googleapis.com/gmail/v1/users/me/threads/{}?format=full",
                    watch.thread_id
                );
                match client.get_json(&url).await {
                    Ok(thread_json) => {
                        println!(
                            "[SENTINEL] watch '{}': fetched thread, diffing state…",
                            watch.thread_id
                        );
                        let messages: Vec<crate::google::types::MailMessage> = thread_json
                            .get("messages")
                            .and_then(|m| m.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|msg_val| crate::google::mail::MailService::parse_message_json(msg_val).ok())
                                    .collect()
                            })
                            .unwrap_or_default();

                        let diff = crate::google::mail::MailService::diff_thread_state(&watch, &messages);
                        match diff {
                            Some(crate::google::types::ThreadUpdateEvent::DeadlineChanged { old_deadline, new_deadline, snippet }) => {
                                println!("[SENTINEL-ALERT] DEADLINE CHANGE DETECTED!");
                                println!("[SENTINEL-ALERT] Context: '{}'", snippet);
                                println!(
                                    "[SENTINEL-ALERT] New Deadline: '{}'",
                                    new_deadline
                                );

                                let spoken = format!(
                                    "Sir, update on {}: the deadline has been updated to {}.",
                                    watch.subject,
                                    new_deadline
                                );
                                // Tracking-first contract: emit the spec payload so the
                                // orb can observe/track the alert before the landing
                                // UI exists. Speech path unchanged below.
                                let payload = build_alert_payload(
                                    "deadline",
                                    &watch,
                                    &watch.sender,
                                    &snippet,
                                    Some((old_deadline.clone(), new_deadline.clone())),
                                    AlertUrgency::High,
                                    spoken.clone(),
                                );
                                let _ = app.emit("orchestrator:sentinel-alert", &payload);
                                println!("[SENTINEL-ALERT] emitted event id='{}' (deadline motion, will speak)", payload.alert_id);
                                crate::orchestrator::speak_proactive_alert(app, spoken, AlertUrgency::High, payload.alert_id.clone());
                            }
                            Some(crate::google::types::ThreadUpdateEvent::NewReply {
                                sender,
                                snippet,
                            }) => {
                                println!(
                                    "[SENTINEL] New reply in thread from '{}': '{}'",
                                    sender, snippet
                                );
                                // Tracked, not spoken (matches current behavior —
                                // only deadline changes interrupt by voice today).
                                let payload = build_alert_payload(
                                    "reply",
                                    &watch,
                                    &sender,
                                    &snippet,
                                    None,
                                    AlertUrgency::Medium,
                                    format!(
                                        "Sir, new reply from {} on {}.",
                                        sender, watch.subject
                                    ),
                                );
                                let _ = app.emit("orchestrator:sentinel-alert", &payload);
                                println!("[SENTINEL-ALERT] emitted event id='{}' (reply tracked, silent)", payload.alert_id);
                            }
                            Some(crate::google::types::ThreadUpdateEvent::AttachmentAdded { filenames }) => {
                                println!(
                                    "[SENTINEL] New attachment(s) added: {:?}",
                                    filenames
                                );
                                let payload = build_alert_payload(
                                    "attachment",
                                    &watch,
                                    &watch.sender,
                                    &filenames.join(", "),
                                    None,
                                    AlertUrgency::Low,
                                    format!(
                                        "Sir, new attachments in {}.",
                                        watch.subject
                                    ),
                                );
                                let _ = app.emit("orchestrator:sentinel-alert", &payload);
                                println!("[SENTINEL-ALERT] emitted event id='{}' (attachment tracked, silent)", payload.alert_id);
                            }
                            None => {
                                println!(
                                    "[SENTINEL] watch '{}': no changes.",
                                    watch.thread_id
                                );
                            }
                        }
                    }
                    Err(e) => {
                        println!(
                            "[SENTINEL] Gmail API query failed for thread '{}': {:?}",
                            watch.thread_id, e
                        );
                    }
                }
            }
        } else {
            // No-token skip note prints ONCE per process — the poller would
            // otherwise repeat it every 15s forever (the dominant console
            // noise when Google OAuth isn't connected yet).
            static NOTED_NO_TOKEN: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !NOTED_NO_TOKEN.swap(true, std::sync::atomic::Ordering::Relaxed) {
                println!("[SENTINEL] Note: Google account not authenticated yet. Add token in Settings or set GOOGLE_ACCESS_TOKEN. (Further skips are debug-only.)");
            } else {
                tracing::debug!("[SENTINEL] Skipping poll: no Google token.");
            }
        }
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deduplication_buffer() {
        let mut buf = SeenMessageRingBuffer::new(3);
        assert!(buf.insert("msg_1"));
        assert!(!buf.insert("msg_1")); // duplicate rejected
        assert!(buf.contains("msg_1"));
        assert_eq!(buf.len(), 1);

        assert!(buf.insert("msg_2"));
        assert!(buf.insert("msg_3"));
        assert_eq!(buf.len(), 3);

        // Fourth insert should evict oldest (msg_1)
        assert!(buf.insert("msg_4"));
        assert!(!buf.contains("msg_1"));
        assert!(buf.contains("msg_2"));
        assert!(buf.contains("msg_3"));
        assert!(buf.contains("msg_4"));
    }

    #[test]
    fn test_mail_deadline_alert_synthesis() {
        let sentinel = ProactiveSentinel::new();
        let mail = MailMessage {
            id: "msg_901".into(),
            thread_id: "th_901".into(),
            from: "Dr. Henderson <henderson@university.edu>".into(),
            to: "student@nexus.internal".into(),
            subject: "Final Project Extension".into(),
            body_text: "Due date extended to Monday 9 AM".into(),
            timestamp_ms: 1727690000000,
        };

        let deadline = DeadlineUpdate {
            context_phrase: "Due date extended to Monday 9 AM".into(),
            task_or_subject: "Project".into(),
            new_deadline_raw: "Monday 9 AM".into(),
            is_extended: true,
            confidence: 0.95,
        };

        let alert = sentinel.synthesize_mail_deadline_alert(&mail, &deadline);
        assert!(alert.is_some());
        let a = alert.unwrap();
        assert_eq!(a.source, "Gmail");
        assert_eq!(a.urgency, AlertUrgency::High);
        assert!(a.spoken_line.contains("Dr. Henderson"));
        assert!(a.spoken_line.contains("Monday 9 AM"));

        // Second time should be de-duplicated
        let dupe = sentinel.synthesize_mail_deadline_alert(&mail, &deadline);
        assert!(dupe.is_none());
    }

    #[test]
    fn test_commute_alert_synthesis() {
        let sentinel = ProactiveSentinel::new();
        let commute = CommuteDelay {
            destination: "Downtown Office".into(),
            normal_duration_min: 20,
            current_duration_min: 40,
            delay_minutes: 20,
            is_congested: true,
            recommended_departure_iso: "2026-09-30T09:15:00Z".into(),
        };

        let alert = sentinel.synthesize_commute_alert("Product Review", &commute);
        assert!(alert.is_some());
        let a = alert.unwrap();
        assert_eq!(a.source, "Google Maps");
        assert!(a.spoken_line.contains("Downtown Office"));
        assert!(a.spoken_line.contains("40 minutes"));
    }

    #[test]
    fn test_urgency_filtering() {
        assert!(ProactiveSentinel::should_speak_immediately(
            AlertUrgency::Critical,
            AlertUrgency::High
        ));
        assert!(ProactiveSentinel::should_speak_immediately(
            AlertUrgency::High,
            AlertUrgency::High
        ));
        assert!(!ProactiveSentinel::should_speak_immediately(
            AlertUrgency::Medium,
            AlertUrgency::High
        ));
        assert!(!ProactiveSentinel::should_speak_immediately(
            AlertUrgency::Low,
            AlertUrgency::Medium
        ));
    }

    fn watch_fixture() -> crate::google::types::ThreadWatchTarget {
        crate::google::types::ThreadWatchTarget {
            watch_id: "w1".into(),
            thread_id: "screen_thread_1790701281".into(),
            account_email: Some("official.lakshya.chitkul@gmail.com".into()),
            initial_history_id: None,
            sender: "Dr. Henderson <henderson@acme.edu>".into(),
            subject: "Final Submission Guidelines & Deadline Extension".into(),
            initial_deadline_raw: Some("Friday 5:00 PM".into()),
            message_count: 3,
            created_at_ms: 1790701281000,
            last_checked_ms: 1790701281000,
            status: crate::google::types::WatchStatus::Active,
        }
    }

    /// Organization fallback derivation from the sender string.
    #[test]
    fn test_organization_from_sender() {
        let org = organization_from_sender("Dr. Henderson <henderson@acme.edu>");
        assert_eq!(org.domain, "acme.edu");
        assert_eq!(org.name, "Acme");
        assert_eq!(org.avatar_fallback_initials.len(), 2);
        assert!(org.brand_color.starts_with('#'));
        // Plain sender without email → unknown fallback, never panics.
        let org = organization_from_sender("Dr. Henderson");
        assert_eq!(org.domain, "unknown");
    }

    /// Wire shape matches the Orb UI & Landing Specification exactly —
    /// the TS interface mirrors these keys 1:1.
    #[test]
    fn test_alert_payload_wire_shape() {
        let watch = watch_fixture();
        let p = build_alert_payload(
            "deadline",
            &watch,
            &watch.sender,
            "The submission portal will remain open until Monday 9:00 AM.",
            Some(("Friday 5:00 PM".into(), "Monday 9:00 AM".into())),
            AlertUrgency::High,
            "Sir, update on Final Submission Guidelines: the deadline has been updated to Monday 9:00 AM.".into(),
        );
        assert!(p.alert_id.starts_with("sentinel_deadline_"));
        assert_eq!(p.source, "Gmail");
        assert_eq!(
            p.account_email.as_deref(),
            Some("official.lakshya.chitkul@gmail.com")
        );
        let v = serde_json::to_value(&p).unwrap();
        for key in [
            "alert_id", "source", "account_email", "organization", "context",
            "deadline", "landing_animation", "spoken_notification",
        ] {
            assert!(v.get(key).is_some(), "missing top-level key {key}");
        }
        assert_eq!(v["organization"]["domain"], "acme.edu");
        assert_eq!(v["context"]["thread_id"], "screen_thread_1790701281");
        assert_eq!(v["deadline"]["new_deadline"], "Monday 9:00 AM");
        assert_eq!(v["deadline"]["is_extended"], true);
        assert_eq!(v["landing_animation"]["auto_collapse_after_ms"], 7000);
        assert_eq!(v["landing_animation"]["initial_state"], "incoming_pulse");
    }

    /// Urgency mapping mirrors synthesis semantics: deadline High,
    /// reply Medium (tracked, not spoken), attachment Low.
    #[test]
    fn test_alert_urgency_mapping() {
        let watch = watch_fixture();
        let d = build_alert_payload(
            "deadline", &watch, &watch.sender, "s",
            Some(("a".into(), "b".into())), AlertUrgency::High, "s".into(),
        );
        assert_eq!(d.deadline.as_ref().unwrap().urgency, AlertUrgency::High);
        let r = build_alert_payload(
            "reply", &watch, "mom", "s", None, AlertUrgency::Medium, "s".into(),
        );
        assert!(r.deadline.is_none());
        let a = build_alert_payload(
            "attachment", &watch, &watch.sender, "f.pdf", None,
            AlertUrgency::Low, "s".into(),
        );
        assert!(a.deadline.is_none());
    }
}
