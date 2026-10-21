//! Inbox watcher (plan P5): polls Gmail's `history.list` (cheap — a few quota
//! units per call, and it only returns what changed) and tells the user about
//! the emails `mailtriage` rates as important.
//!
//! Why polling and not push: Gmail push needs a Cloud Pub/Sub topic and a
//! public HTTPS endpoint, which a desktop app does not have.
//!
//! First connect is quiet: the watcher records "now" as its starting point and
//! files the last three days of important mail for the briefing, without
//! speaking any of it. After that only new mail can interrupt.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, Runtime};

use crate::google::types::{AlertUrgency, GoogleError};

use super::google_io::{self, Account};
use super::mailtriage::{self, MailMeta, Triage};
use super::store::{Observation, Store, Tier, Trust};

pub const TICK_SECS: u64 = 30;
/// Poll interval while the user is at the PC / away.
pub const ACTIVE_POLL_SECS: u64 = 90;
pub const IDLE_POLL_SECS: u64 = 8 * 60;
/// Cap on messages fetched per poll (a burst beyond this is picked up next poll).
const MAX_FETCH_PER_POLL: usize = 40;
const MAX_HISTORY_PAGES: usize = 5;
/// "That's not important" applies to an alert this recent.
const MUTE_WINDOW: Duration = Duration::from_secs(10 * 60);

#[derive(Debug)]
pub enum PollError {
    Auth,
    RateLimited,
    Other(#[allow(dead_code)] String),
}

impl From<GoogleError> for PollError {
    fn from(e: GoogleError) -> Self {
        match e {
            GoogleError::AuthRequired => PollError::Auth,
            GoogleError::RateLimited => PollError::RateLimited,
            other => PollError::Other(other.to_string()),
        }
    }
}

/// Seconds until the next poll after `outcome`. Pure.
pub fn next_delay_secs(outcome: &Result<(), PollErrorKind>, user_active: bool, consecutive_failures: u32) -> u64 {
    match outcome {
        Ok(()) => {
            if user_active { ACTIVE_POLL_SECS } else { IDLE_POLL_SECS }
        }
        Err(PollErrorKind::Auth) => 30 * 60,
        Err(PollErrorKind::RateLimited) => 10 * 60,
        Err(PollErrorKind::Other) => (60u64 << consecutive_failures.min(4)).min(15 * 60),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollErrorKind {
    Auth,
    RateLimited,
    Other,
}

impl PollError {
    fn kind(&self) -> PollErrorKind {
        match self {
            PollError::Auth => PollErrorKind::Auth,
            PollError::RateLimited => PollErrorKind::RateLimited,
            PollError::Other(_) => PollErrorKind::Other,
        }
    }
}

// ─── Mute list ("that's not important") ─────────────────────────────

const MUTE_PREFIX: &str = "mail_mute_";

pub fn muted_senders(store: &Store) -> Vec<String> {
    store
        .with_prefix(Tier::Fact, MUTE_PREFIX)
        .into_iter()
        .filter_map(|(k, _)| k.strip_prefix(MUTE_PREFIX).map(|s| s.to_lowercase()))
        .collect()
}

pub fn mute_sender(store: &Store, sender: &str, now: i64) -> bool {
    let sender = sender.trim().to_lowercase();
    if sender.is_empty() {
        return false;
    }
    store
        .observe(
            &Observation {
                tier: Tier::Fact,
                key: format!("{MUTE_PREFIX}{sender}"),
                value: "muted".into(),
                source: "user:mute".into(),
                trust: Trust::UserSaid,
                pinned: true,
            },
            now,
        )
        .is_ok()
}

// ─── Last alert (so "that's not important" knows what "that" is) ─────

static LAST_ALERT: Mutex<Option<(String, String, Instant)>> = Mutex::new(None);

fn note_alert(email: &str, label: &str) {
    *LAST_ALERT.lock().unwrap_or_else(|e| e.into_inner()) = Some((email.to_string(), label.to_string(), Instant::now()));
}

/// How long ago the last mail alert was spoken (within the mute window).
pub fn last_alert_age() -> Option<Duration> {
    LAST_ALERT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|(_, _, at)| at.elapsed())
        .filter(|age| *age < MUTE_WINDOW)
}

/// The sender of the most recent alert, if it was spoken within the last 10 min.
pub fn last_alert_sender() -> Option<(String, String)> {
    LAST_ALERT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .filter(|(_, _, at)| at.elapsed() < MUTE_WINDOW)
        .map(|(e, l, _)| (e.clone(), l.clone()))
}

// ─── Storing + alerting ─────────────────────────────────────────────

/// Store a classified email. `true` = it is new (not seen before).
fn store_mail(store: &Store, t: &Triage, m: &MailMeta, account: &str, now: i64) -> bool {
    let stored = mailtriage::to_stored(t, m, account);
    let value = serde_json::to_string(&stored).unwrap_or_default();
    matches!(
        store.observe(
            &Observation {
                tier: Tier::Mail,
                key: format!("mail:{}", m.id),
                value,
                source: format!("gmail:{account}"),
                trust: Trust::Untrusted,
                pinned: false,
            },
            now,
        ),
        Ok(super::store::Admit::Inserted)
    )
}

/// Everything worth surfacing from a set of messages: classify, drop Low,
/// store, and return the new ones in urgency order. Pure over the store.
pub fn triage_and_store(store: &Store, metas: &[MailMeta], account: &str, now: i64) -> Vec<(Triage, MailMeta)> {
    let muted = muted_senders(store);
    let mut fresh: Vec<(Triage, MailMeta)> = vec![];
    for m in metas {
        let Some(t) = mailtriage::classify(m, &muted) else { continue };
        if mailtriage::urgency_rank(t.urgency) < mailtriage::urgency_rank(AlertUrgency::Medium) {
            continue;
        }
        if store_mail(store, &t, m, account, now) {
            fresh.push((t, m.clone()));
        }
    }
    fresh.sort_by(|a, b| {
        mailtriage::urgency_rank(b.0.urgency)
            .cmp(&mailtriage::urgency_rank(a.0.urgency))
            .then(b.1.date_ms.cmp(&a.1.date_ms))
    });
    fresh
}

fn speak_address<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let friend = crate::persona::is_friend(&crate::commands::read_persona_mode(app));
    let name = app
        .path()
        .app_data_dir()
        .ok()
        .and_then(|d| crate::memory::read_user_profile(&d))
        .and_then(|p| p.name);
    crate::persona::address(name.as_deref(), friend)
}

/// One alert per email, or a single summary when several arrive together.
fn fire_alerts<R: Runtime>(app: &AppHandle<R>, items: &[(Triage, MailMeta)]) {
    if items.is_empty() {
        return;
    }
    let address = speak_address(app);
    note_alert(&items[0].1.from_email, &mailtriage::sender_label(&items[0].1));
    let top = items.iter().map(|(t, _)| t.urgency).max_by_key(|u| mailtriage::urgency_rank(*u)).unwrap_or(AlertUrgency::Medium);
    if items.len() >= 3 {
        let text = mailtriage::batch_text(items, address.as_deref());
        crate::proactive_policy::submit(app, format!("mail_batch_{}", items[0].1.id), text, top);
        return;
    }
    for (t, m) in items {
        let text = mailtriage::alert_text(t, m, address.as_deref());
        crate::proactive_policy::submit(app, format!("mail_{}", m.id), text, t.urgency);
    }
}

// ─── Polling one account ────────────────────────────────────────────

async fn fetch_metas(client: &crate::google::client::GoogleClient, ids: &[String]) -> Vec<MailMeta> {
    let mut out = vec![];
    for id in ids.iter().take(MAX_FETCH_PER_POLL) {
        match google_io::gmail_meta(client, id).await {
            Ok(m) => out.push(m),
            Err(e) => tracing::debug!("mailwatch: could not read message {id}: {e}"),
        }
    }
    out
}

/// What one poll found.
pub enum PollOutcome {
    /// First contact (or an expired history id): starting point recorded,
    /// recent important mail filed quietly — nothing to say.
    Baselined(#[allow(dead_code)] usize),
    /// New important mail since the last poll, most urgent first.
    Polled(Vec<(Triage, MailMeta)>),
}

/// Record "now" as the starting point and quietly file recent important mail.
async fn baseline(
    dir: &std::path::Path,
    account_key: &str,
    client: &crate::google::client::GoogleClient,
) -> Result<usize, PollError> {
    let history_id = google_io::gmail_history_id(client).await?;
    let ids = google_io::gmail_recent_ids(client).await.unwrap_or_default();
    let metas = fetch_metas(client, &ids).await;
    let now = chrono::Utc::now().timestamp();
    let key = format!("gmail_hist:{account_key}");
    let n = super::with_store(dir, |s| {
        s.batch(|s| {
            let n = triage_and_store(s, &metas, account_key, now).len();
            s.meta_set(&key, &history_id);
            n
        })
    })
    .unwrap_or(0);
    tracing::info!("mailwatch: baseline for {account_key} ({n} important in the last 3 days, filed quietly)");
    Ok(n)
}

/// One poll for one account, without any speaking (so it is testable
/// against a mock server): history since the stored id → metadata →
/// classify → store.
pub async fn poll_core(
    dir: &std::path::Path,
    account_key: &str,
    client: &crate::google::client::GoogleClient,
) -> Result<PollOutcome, PollError> {
    let key = format!("gmail_hist:{account_key}");
    let Some(start) = super::with_store(dir, |s| s.meta_get(&key)).flatten() else {
        return Ok(PollOutcome::Baselined(baseline(dir, account_key, client).await?));
    };

    let mut ids: Vec<String> = vec![];
    let mut newest = start.clone();
    let mut page_token: Option<String> = None;
    for _ in 0..MAX_HISTORY_PAGES {
        let page = match google_io::gmail_history(client, &start, page_token.as_deref()).await {
            Ok(p) => p,
            // History id too old (Gmail only keeps ~a week): start over quietly.
            Err(GoogleError::NotFound(_)) => {
                tracing::info!("mailwatch: history id expired for {account_key}; re-baselining");
                return Ok(PollOutcome::Baselined(baseline(dir, account_key, client).await?));
            }
            Err(e) => return Err(e.into()),
        };
        for (id, labels) in page.added {
            if !ids.contains(&id) && (labels.is_empty() || labels.iter().any(|l| l == "INBOX")) {
                ids.push(id);
            }
        }
        if let Some(h) = page.history_id {
            newest = h;
        }
        match page.next_page {
            Some(t) => page_token = Some(t),
            None => break,
        }
    }

    let metas = fetch_metas(client, &ids).await;
    let now = chrono::Utc::now().timestamp();
    let fresh = super::with_store(dir, |s| {
        s.batch(|s| {
            let fresh = triage_and_store(s, &metas, account_key, now);
            s.meta_set(&key, &newest);
            fresh
        })
    })
    .unwrap_or_default();
    Ok(PollOutcome::Polled(fresh))
}

pub async fn poll_account<R: Runtime>(app: &AppHandle<R>, dir: &std::path::Path, acct: &Account) -> Result<usize, PollError> {
    let token = google_io::token_for(acct.email.as_deref()).await?;
    let client = crate::google::client::GoogleClient::new(token)?;
    match poll_core(dir, &acct.key(), &client).await? {
        PollOutcome::Baselined(_) => Ok(0),
        PollOutcome::Polled(fresh) => {
            let n = fresh.len();
            if n > 0 {
                tracing::info!("mailwatch: {n} important new email(s) for {}", acct.key());
                fire_alerts(app, &fresh);
            }
            Ok(n)
        }
    }
}

/// Remember (and, once a day, say) that Google needs a fresh sign-in. While an
/// OAuth consent screen is in Testing mode Google expires refresh tokens after
/// 7 days, so without this the inbox watcher would just stop, silently.
fn note_auth_state<R: Runtime>(app: &AppHandle<R>, dir: &std::path::Path, account_key: &str, needs_signin: bool) {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let say = super::with_store(dir, |s| {
        let was = s.meta_get("mail_needs_signin").map(|v| !v.is_empty()).unwrap_or(false);
        if !needs_signin {
            if was {
                s.meta_set("mail_needs_signin", "");
            }
            return false;
        }
        s.meta_set("mail_needs_signin", account_key);
        let told = s.meta_get("mail_signin_alert_day").as_deref() == Some(today.as_str());
        if !told {
            s.meta_set("mail_signin_alert_day", &today);
        }
        !told
    })
    .unwrap_or(false);
    if say {
        let addr = speak_address(app).map(|a| format!(", {a}")).unwrap_or_default();
        crate::proactive_policy::submit(
            app,
            "mail_signin".to_string(),
            format!("Google needs you to sign in again{addr}, so I can keep watching your email. You can do it under Add Google Account in the Command Hub."),
            AlertUrgency::Medium,
        );
    }
}

/// Start the watcher. Idle (no work, no network) until a Google account with
/// mail access is connected.
pub fn start<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let mut next_due: HashMap<String, Instant> = HashMap::new();
        let mut failures: HashMap<String, u32> = HashMap::new();
        let mut noted_auth: HashMap<String, bool> = HashMap::new();
        loop {
            tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;
            let Ok(dir) = app.path().app_data_dir() else { continue };
            if !super::enabled(&dir) || !super::flag(&dir, "memcoreMail", true) {
                continue;
            }
            let accounts = google_io::list_accounts(google_io::can_read_mail);
            if accounts.is_empty() {
                continue;
            }
            let active = super::resume::user_idle_secs().map(|s| s < 300).unwrap_or(true);
            for acct in accounts {
                let key = acct.key();
                if next_due.get(&key).map(|t| Instant::now() < *t).unwrap_or(false) {
                    continue;
                }
                let result = poll_account(&app, &dir, &acct).await;
                let kind = match &result {
                    Ok(_) => Ok(()),
                    Err(e) => Err(e.kind()),
                };
                let fails = failures.entry(key.clone()).or_insert(0);
                note_auth_state(&app, &dir, &key, matches!(&result, Err(PollError::Auth)));
                match &result {
                    Ok(_) => {
                        *fails = 0;
                        noted_auth.remove(&key);
                    }
                    Err(e) => {
                        *fails += 1;
                        if matches!(e, PollError::Auth) {
                            if !noted_auth.contains_key(&key) {
                                noted_auth.insert(key.clone(), true);
                                tracing::warn!("mailwatch: {key} needs to sign in to Google again; pausing checks for 30 min");
                            }
                        } else {
                            tracing::debug!("mailwatch: {key}: {e:?}");
                        }
                    }
                }
                let delay = next_delay_secs(&kind, active, *fails);
                next_due.insert(key, Instant::now() + Duration::from_secs(delay));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(id: &str, from: &str, subject: &str, labels: &[&str], date: i64) -> MailMeta {
        let (from_name, from_email) = mailtriage::parse_from(from);
        MailMeta {
            id: id.into(),
            thread_id: "t".into(),
            from_name,
            from_email,
            subject: subject.into(),
            snippet: String::new(),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            date_ms: date,
        }
    }

    #[test]
    fn triage_stores_medium_and_above_orders_by_urgency_and_never_repeats() {
        let s = Store::open_in_memory().unwrap();
        let metas = vec![
            mail("1", "GitHub <notifications@github.com>", "[r] Run failed: CI", &["INBOX"], 100),
            mail("2", "Supabase <noreply@supabase.io>", "Your project will be paused", &["INBOX"], 50),
            mail("3", "GitHub <notifications@github.com>", "Re: [r] Fix typo", &["INBOX"], 200), // Low: dropped
            mail("4", "Friend <f@gmail.com>", "lunch?", &["INBOX"], 300),                       // unclassified
        ];
        let fresh = triage_and_store(&s, &metas, "me@gmail.com", 1_000);
        assert_eq!(fresh.iter().map(|(_, m)| m.id.as_str()).collect::<Vec<_>>(), vec!["2", "1"], "High before Medium");
        assert_eq!(s.count(Tier::Mail), 2);
        // The same messages again (e.g. overlapping history pages) are not new.
        assert!(triage_and_store(&s, &metas, "me@gmail.com", 1_001).is_empty());
        assert_eq!(s.count(Tier::Mail), 2);
    }

    #[test]
    fn muting_a_sender_silences_future_mail_and_is_visible_as_a_fact() {
        let s = Store::open_in_memory().unwrap();
        assert!(mute_sender(&s, " Notifications@GitHub.com ", 10));
        assert_eq!(muted_senders(&s), vec!["notifications@github.com"]);
        let metas = vec![mail("1", "GitHub <notifications@github.com>", "[r] Run failed", &["INBOX"], 1)];
        assert!(triage_and_store(&s, &metas, "me", 11).is_empty());
        assert!(!mute_sender(&s, "  ", 12));
        // Forgetting the fact un-mutes (it is a normal, user-visible memory).
        assert_eq!(s.forget_key("mail_mute_notifications@github.com", "user", 13), 1);
        assert_eq!(triage_and_store(&s, &metas, "me", 14).len(), 1);
    }

    #[test]
    fn stored_mail_is_untrusted_and_never_a_fact() {
        let s = Store::open_in_memory().unwrap();
        triage_and_store(&s, &[mail("1", "Supabase <noreply@supabase.io>", "Project will be deleted", &["INBOX"], 1)], "me", 5);
        let rows = s.recent(Tier::Mail, 5);
        assert_eq!(rows.len(), 1);
        assert_eq!(s.count(Tier::Fact), 0);
        let stored: mailtriage::StoredMail = serde_json::from_str(&rows[0].value).unwrap();
        assert_eq!((stored.category.as_str(), stored.urgency.as_str()), ("database", "high"));
        assert!(s.list_rows(10).is_empty(), "mail summaries stay out of the memory list");
    }

    #[test]
    fn a_github_token_notice_is_still_stored_despite_the_secret_screen() {
        let s = Store::open_in_memory().unwrap();
        let m = mail("9", "GitHub <noreply@github.com>", "Your personal access token has expired", &["INBOX"], 1);
        assert_eq!(triage_and_store(&s, &[m], "me", 5).len(), 1);
    }

    #[test]
    fn poll_delays() {
        assert_eq!(next_delay_secs(&Ok(()), true, 0), 90);
        assert_eq!(next_delay_secs(&Ok(()), false, 0), 480);
        assert_eq!(next_delay_secs(&Err(PollErrorKind::Auth), true, 3), 1800);
        assert_eq!(next_delay_secs(&Err(PollErrorKind::RateLimited), true, 1), 600);
        assert_eq!(next_delay_secs(&Err(PollErrorKind::Other), true, 1), 120);
        assert_eq!(next_delay_secs(&Err(PollErrorKind::Other), true, 9), 900, "backoff is capped");
    }

    #[test]
    fn last_alert_window() {
        note_alert("a@b.com", "A");
        assert_eq!(last_alert_sender(), Some(("a@b.com".into(), "A".into())));
        *LAST_ALERT.lock().unwrap() = Some(("old@b.com".into(), "Old".into(), Instant::now() - Duration::from_secs(11 * 60)));
        assert_eq!(last_alert_sender(), None);
    }

    // ─── Mock Gmail server (real HTTP, no Google) ───────────────────

    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    static MOCK_GUARD: Mutex<()> = Mutex::new(());

    struct MockGmail {
        log: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
    }

    impl Drop for MockGmail {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            *google_io::TEST_BASE.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }

    /// `routes`: (substring of the request target, responses served in order;
    /// the last one repeats). First matching route wins.
    fn mock_gmail(routes: Vec<(&'static str, Vec<(u16, String)>)>) -> MockGmail {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (log2, stop2) = (log.clone(), stop.clone());
        let mut state: Vec<(&'static str, Vec<(u16, String)>, usize)> =
            routes.into_iter().map(|(k, v)| (k, v, 0)).collect();
        std::thread::spawn(move || {
            while !stop2.load(Ordering::Relaxed) {
                let Ok((mut sock, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(3));
                    continue;
                };
                let _ = sock.set_nonblocking(false);
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let target = req.split_whitespace().nth(1).unwrap_or("").to_string();
                log2.lock().unwrap().push(target.clone());
                let (code, body) = match state.iter_mut().find(|(k, _, _)| target.contains(*k)) {
                    Some((_, resp, i)) => {
                        let r = resp[(*i).min(resp.len() - 1)].clone();
                        *i += 1;
                        r
                    }
                    None => (404, "{}".to_string()),
                };
                let reply = format!(
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(reply.as_bytes());
            }
        });
        *google_io::TEST_BASE.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("http://{addr}"));
        MockGmail { log, stop }
    }

    fn msg_json(id: &str, from: &str, subject: &str, labels: &[&str]) -> (u16, String) {
        let l: Vec<String> = labels.iter().map(|x| format!("\"{x}\"")).collect();
        (
            200,
            format!(
                r#"{{"id":"{id}","threadId":"t{id}","snippet":"","labelIds":[{}],"internalDate":"1700000000000",
                    "payload":{{"headers":[{{"name":"From","value":"{from}"}},{{"name":"Subject","value":"{subject}"}}]}}}}"#,
                l.join(",")
            ),
        )
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nexus_mailwatch_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn client() -> crate::google::client::GoogleClient {
        crate::google::client::GoogleClient::new("test-token".into()).unwrap()
    }

    #[test]
    fn mock_baseline_then_paginated_poll_then_stale_history() {
        let _g = MOCK_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tmp("flow");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mock = mock_gmail(vec![
            ("/profile", vec![(200, r#"{"historyId":"100"}"#.into()), (200, r#"{"historyId":"300"}"#.into())]),
            ("/messages?q=", vec![(200, r#"{"messages":[{"id":"a"},{"id":"b"}]}"#.into()), (200, r#"{"messages":[]}"#.into())]),
            ("/messages/a?", vec![msg_json("a", "Supabase <noreply@supabase.io>", "Your project will be paused", &["INBOX"])]),
            ("/messages/b?", vec![msg_json("b", "Friend <f@gmail.com>", "lunch", &["INBOX"])]),
            ("/messages/c?", vec![msg_json("c", "GitHub <notifications@github.com>", "[r] Run failed: CI", &["INBOX"])]),
            ("/messages/e?", vec![msg_json("e", "Exam Cell <exams@vit.edu>", "Hall ticket for End-Sem", &["INBOX"])]),
            (
                "/history",
                vec![
                    (200, r#"{"history":[{"messagesAdded":[{"message":{"id":"c","labelIds":["INBOX"]}}]}],"nextPageToken":"p2","historyId":"110"}"#.into()),
                    (200, r#"{"history":[{"messagesAdded":[{"message":{"id":"d","labelIds":["SENT"]}},{"message":{"id":"e","labelIds":["INBOX"]}}]}],"historyId":"120"}"#.into()),
                    (404, r#"{"error":{"code":404,"message":"Requested entity was not found."}}"#.into()),
                ],
            ),
        ]);
        rt.block_on(async {
            // 1. First contact: quiet baseline, important mail filed, nothing to say.
            match poll_core(&dir, "me@gmail.com", &client()).await.unwrap() {
                PollOutcome::Baselined(n) => assert_eq!(n, 1, "only the Supabase warning is important"),
                PollOutcome::Polled(_) => panic!("first contact must not speak"),
            }
            let (hist, filed) =
                super::super::with_store(&dir, |s| (s.meta_get("gmail_hist:me@gmail.com"), s.count(Tier::Mail))).unwrap();
            assert_eq!((hist.as_deref(), filed), (Some("100"), 1));

            // 2. Next poll: two history pages, SENT mail ignored, High before Medium.
            match poll_core(&dir, "me@gmail.com", &client()).await.unwrap() {
                PollOutcome::Polled(fresh) => {
                    let ids: Vec<&str> = fresh.iter().map(|(_, m)| m.id.as_str()).collect();
                    assert_eq!(ids, vec!["e", "c"], "exam (High) before CI failure (Medium); SENT message d dropped");
                }
                PollOutcome::Baselined(_) => panic!("expected a normal poll"),
            }
            let hist = super::super::with_store(&dir, |s| s.meta_get("gmail_hist:me@gmail.com")).unwrap();
            assert_eq!(hist.as_deref(), Some("120"));

            // 3. History id expired (404): quiet re-baseline from the profile.
            match poll_core(&dir, "me@gmail.com", &client()).await.unwrap() {
                PollOutcome::Baselined(_) => {}
                PollOutcome::Polled(_) => panic!("a stale history id must re-baseline, not alert"),
            }
            let hist = super::super::with_store(&dir, |s| s.meta_get("gmail_hist:me@gmail.com")).unwrap();
            assert_eq!(hist.as_deref(), Some("300"));
        });
        let log = mock.log.lock().unwrap().clone();
        let hist_calls: Vec<&String> = log.iter().filter(|t| t.contains("/history")).collect();
        assert!(
            hist_calls[0].contains("startHistoryId=100")
                && hist_calls[0].contains("labelId=INBOX")
                && hist_calls[0].contains("historyTypes=messageAdded"),
            "{hist_calls:?}"
        );
        assert!(hist_calls[1].contains("pageToken=p2"), "{hist_calls:?}");
        assert!(log.iter().all(|t| !t.contains("format=full")), "message bodies must never be requested: {log:?}");
        assert!(log.iter().filter(|t| t.contains("/messages/")).all(|t| t.contains("format=metadata")));
    }

    #[test]
    fn mock_auth_and_rate_limit_errors_map_to_poll_errors() {
        let _g = MOCK_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let dir = tmp("errors");
        let _mock = mock_gmail(vec![("/profile", vec![(401, "{}".into()), (429, "{}".into()), (500, "oops".into())])]);
        rt.block_on(async {
            assert!(matches!(poll_core(&dir, "a", &client()).await, Err(PollError::Auth)));
            assert!(matches!(poll_core(&dir, "a", &client()).await, Err(PollError::RateLimited)));
            assert!(matches!(poll_core(&dir, "a", &client()).await, Err(PollError::Other(_))));
        });
    }

    #[test]
    fn mock_calendar_list_and_insert() {
        let _g = MOCK_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mock = mock_gmail(vec![(
            "/calendar/v3/calendars/primary/events",
            vec![
                (200, r#"{"items":[{"id":"1","summary":"Dentist","start":{"dateTime":"2026-10-08T15:00:00+05:30"},"end":{"dateTime":"2026-10-08T16:00:00+05:30"}}]}"#.into()),
                (200, r#"{"id":"new1","summary":"Gym","start":{"dateTime":"2026-10-08T17:00:00+05:30"}}"#.into()),
            ],
        )]);
        rt.block_on(async {
            let evs = google_io::calendar_events(&client(), "2026-10-08T00:00:00+05:30", "2026-10-09T00:00:00+05:30").await.unwrap();
            assert_eq!(evs.len(), 1);
            assert_eq!(evs[0].summary, "Dentist");
            let receipt = google_io::calendar_insert(
                &client(),
                &crate::google::types::NewEvent {
                    summary: "Gym".into(),
                    description: None,
                    start_iso: "2026-10-08T17:00:00+05:30".into(),
                    end_iso: "2026-10-08T18:00:00+05:30".into(),
                    location: None,
                },
            )
            .await
            .unwrap();
            assert_eq!(receipt.event_id, "new1");
        });
        let log = mock.log.lock().unwrap().clone();
        assert!(log[0].contains("timeMin=2026-10-08T00%3A00%3A00%2B05%3A30") && log[0].contains("singleEvents=true"), "{log:?}");
    }

    /// Live probe against the real Google account(s). Prints counts and
    /// categories only — never subjects, senders or snippets.
    /// cargo test --lib live_mail_probe -- --ignored --nocapture
    #[test]
    #[ignore = "uses the real Google account"]
    fn live_mail_probe() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mail_accounts = google_io::list_accounts(google_io::can_read_mail);
            let cal = google_io::primary_account(google_io::can_use_calendar);
            println!("mail-capable accounts: {}, calendar account: {}", mail_accounts.len(), cal.is_some());
            for acct in &mail_accounts {
                let tok = google_io::token_for(acct.email.as_deref()).await;
                println!("token: {}", if tok.is_ok() { "ok" } else { "MISSING/expired and refresh failed" });
                let Ok(token) = tok else { continue };
                let client = crate::google::client::GoogleClient::new(token).unwrap();
                match google_io::gmail_history_id(&client).await {
                    Ok(h) => println!("gmail profile historyId: ok ({} chars)", h.len()),
                    Err(e) => println!("gmail profile: {e}"),
                }
                match google_io::gmail_recent_ids(&client).await {
                    Ok(ids) => {
                        println!("recent inbox ids (3d): {}", ids.len());
                        let metas = fetch_metas(&client, &ids).await;
                        let mut by: std::collections::BTreeMap<String, usize> = Default::default();
                        for m in &metas {
                            if let Some(t) = mailtriage::classify(m, &[]) {
                                *by.entry(format!("{}/{}", t.category.as_str(), mailtriage::urgency_str(t.urgency))).or_default() += 1;
                            }
                        }
                        println!("fetched metadata: {} | classified: {:?}", metas.len(), by);
                    }
                    Err(e) => println!("messages.list: {e}"),
                }
            }
            match google_io::agenda_for(0).await {
                Ok(items) => println!("calendar today: {} event(s)", items.len()),
                Err(e) => println!("calendar: {e}"),
            }
        });
    }
}
