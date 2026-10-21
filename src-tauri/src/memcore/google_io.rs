//! Gmail / Calendar HTTP for the Memory Core (plan P5). Thin wrappers over
//! `google::client::GoogleClient`; all parsing is in `mailtriage` / `agenda`
//! (pure, tested). Mail is read with metadata only (sender, subject, Gmail's
//! own snippet) — message bodies are never requested.

use serde_json::Value;

use crate::google::client::GoogleClient;
use crate::google::types::{CalendarEvent, EventReceipt, GoogleError, NewEvent};

use super::mailtriage::{self, HistoryPage, MailMeta};

const GMAIL_DEFAULT: &str = "https://gmail.googleapis.com/gmail/v1/users/me";
const CALENDAR_DEFAULT: &str = "https://www.googleapis.com/calendar/v3/calendars/primary/events";

/// Test hook: point the Gmail/Calendar calls at a local mock server.
#[cfg(test)]
pub static TEST_BASE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn gmail_base() -> String {
    #[cfg(test)]
    if let Some(b) = TEST_BASE.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return format!("{b}/gmail/v1/users/me");
    }
    GMAIL_DEFAULT.to_string()
}

fn calendar_base() -> String {
    #[cfg(test)]
    if let Some(b) = TEST_BASE.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return format!("{b}/calendar/v3/calendars/primary/events");
    }
    CALENDAR_DEFAULT.to_string()
}

/// One connected Google account as the watcher sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    /// `None` = the legacy single-token setup (no multi-account registry).
    pub email: Option<String>,
}

impl Account {
    pub fn key(&self) -> String {
        self.email.clone().unwrap_or_else(|| "default".to_string())
    }
}

/// True when the account's granted scopes can read mail. An empty scope list
/// (older profiles) is given the benefit of the doubt.
pub fn can_read_mail(scopes: &[String]) -> bool {
    scopes.is_empty() || scopes.iter().any(|s| s.contains("gmail.readonly") || s.contains("gmail.modify") || s.ends_with("/mail.google.com/"))
}

pub fn can_use_calendar(scopes: &[String]) -> bool {
    scopes.is_empty() || scopes.iter().any(|s| s.contains("calendar"))
}

/// Connected accounts that may be read. Falls back to the single legacy token.
pub fn list_accounts(need: fn(&[String]) -> bool) -> Vec<Account> {
    let accounts = crate::auth_vault::get_google_accounts();
    if accounts.is_empty() {
        return if crate::auth_vault::get_token("google").is_some() {
            vec![Account { email: None }]
        } else {
            vec![]
        };
    }
    accounts
        .into_iter()
        .filter(|a| need(&a.scopes))
        .map(|a| Account { email: Some(a.email.to_lowercase()) })
        .collect()
}

/// The primary account (or the first connected one).
pub fn primary_account(need: fn(&[String]) -> bool) -> Option<Account> {
    let accounts = crate::auth_vault::get_google_accounts();
    if accounts.is_empty() {
        return list_accounts(need).into_iter().next();
    }
    accounts
        .iter()
        .find(|a| a.is_primary && need(&a.scopes))
        .or_else(|| accounts.iter().find(|a| need(&a.scopes)))
        .map(|a| Account { email: Some(a.email.to_lowercase()) })
}

/// A usable access token: the cached one, else a refreshed one.
pub async fn token_for(email: Option<&str>) -> Result<String, GoogleError> {
    if let Some(t) = crate::auth_vault::get_token_for_google_account(email) {
        return Ok(t);
    }
    refresh_token(email).await
}

async fn refresh_token(email: Option<&str>) -> Result<String, GoogleError> {
    match email {
        Some(e) => crate::google::oauth::refresh_access_token_for(e).await.map_err(|_| GoogleError::AuthRequired),
        None => crate::google::auth::GoogleAuth::get_access_token().map_err(|_| GoogleError::AuthRequired),
    }
}

/// Run `op` with a client; on an auth failure refresh the token once and retry.
pub async fn with_client<T, F, Fut>(email: Option<&str>, op: F) -> Result<T, GoogleError>
where
    F: Fn(GoogleClient) -> Fut,
    Fut: std::future::Future<Output = Result<T, GoogleError>>,
{
    let token = token_for(email).await?;
    match op(GoogleClient::new(token)?).await {
        Err(GoogleError::AuthRequired) => {
            let fresh = refresh_token(email).await?;
            op(GoogleClient::new(fresh)?).await
        }
        other => other,
    }
}

// ─── Gmail ──────────────────────────────────────────────────────────

pub async fn gmail_history_id(c: &GoogleClient) -> Result<String, GoogleError> {
    let v = c.get_json(&format!("{}/profile", gmail_base())).await?;
    v.get("historyId")
        .and_then(|h| h.as_str())
        .map(String::from)
        .ok_or_else(|| GoogleError::SerializationError("profile has no historyId".into()))
}

/// New inbox messages since `start`. A 404 means the history id is too old.
pub async fn gmail_history(c: &GoogleClient, start: &str, page: Option<&str>) -> Result<HistoryPage, GoogleError> {
    let mut url = format!(
        "{}/history?startHistoryId={}&historyTypes=messageAdded&labelId=INBOX&maxResults=100",
        gmail_base(),
        mailtriage::pct_encode(start)
    );
    if let Some(p) = page {
        url.push_str(&format!("&pageToken={}", mailtriage::pct_encode(p)));
    }
    Ok(mailtriage::parse_history(&c.get_json(&url).await?))
}

pub async fn gmail_meta(c: &GoogleClient, id: &str) -> Result<MailMeta, GoogleError> {
    let url = format!(
        "{}/messages/{}?format=metadata&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date",
        gmail_base(),
        mailtriage::pct_encode(id)
    );
    let v: Value = c.get_json(&url).await?;
    mailtriage::parse_message_meta(&v).ok_or_else(|| GoogleError::SerializationError("bad message metadata".into()))
}

/// Ids of recent inbox mail, for the quiet catch-up on first connect.
pub async fn gmail_recent_ids(c: &GoogleClient) -> Result<Vec<String>, GoogleError> {
    let url = format!(
        "{}/messages?q={}&maxResults=30",
        gmail_base(),
        mailtriage::pct_encode("in:inbox newer_than:3d")
    );
    let v = c.get_json(&url).await?;
    Ok(v.get("messages")
        .and_then(|m| m.as_array())
        .map(|a| a.iter().filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(String::from)).collect())
        .unwrap_or_default())
}

// ─── Calendar ───────────────────────────────────────────────────────

pub async fn calendar_events(c: &GoogleClient, from_rfc3339: &str, to_rfc3339: &str) -> Result<Vec<CalendarEvent>, GoogleError> {
    let url = format!(
        "{}?timeMin={}&timeMax={}&singleEvents=true&orderBy=startTime&maxResults=25",
        calendar_base(),
        mailtriage::pct_encode(from_rfc3339),
        mailtriage::pct_encode(to_rfc3339)
    );
    crate::google::calendar::CalendarService::parse_events_json(&c.get_json(&url).await?)
}

pub async fn calendar_insert(c: &GoogleClient, event: &NewEvent) -> Result<EventReceipt, GoogleError> {
    let payload = crate::google::calendar::CalendarService::create_event_payload(event);
    let v = c.post_json(&calendar_base(), &payload).await?;
    crate::google::calendar::CalendarService::parse_event_receipt_json(&v)
}

/// Calendar events for today + `day_offset` (primary account), as display items.
pub async fn agenda_for(day_offset: i64) -> Result<Vec<super::agenda::AgendaItem>, GoogleError> {
    let acct = primary_account(can_use_calendar).ok_or(GoogleError::AuthRequired)?;
    let (from, to) = super::agenda::day_bounds(chrono::Local::now(), day_offset);
    let events = with_client(acct.email.as_deref(), |c| {
        let (from, to) = (from.clone(), to.clone());
        async move { calendar_events(&c, &from, &to).await }
    })
    .await?;
    Ok(super::agenda::items_from_events(&events))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scopes(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn scope_checks() {
        assert!(can_read_mail(&scopes(&["https://www.googleapis.com/auth/gmail.readonly"])));
        assert!(can_read_mail(&scopes(&["https://www.googleapis.com/auth/gmail.modify"])));
        assert!(!can_read_mail(&scopes(&["https://www.googleapis.com/auth/calendar", "openid"])));
        assert!(can_read_mail(&[]), "older profiles without a scope list are not locked out");
        assert!(can_use_calendar(&scopes(&["https://www.googleapis.com/auth/calendar"])));
        assert!(!can_use_calendar(&scopes(&["https://www.googleapis.com/auth/gmail.readonly"])));
    }

    #[test]
    fn account_keys() {
        assert_eq!(Account { email: Some("a@b.com".into()) }.key(), "a@b.com");
        assert_eq!(Account { email: None }.key(), "default");
    }
}
