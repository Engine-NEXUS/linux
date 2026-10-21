#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GoogleError {
    AuthRequired,
    TokenExpired,
    RateLimited,
    NetworkError(String),
    ApiError { code: u16, message: String },
    NotFound(String),
    SerializationError(String),
    InvalidInput(String),
}

impl std::fmt::Display for GoogleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthRequired => write!(f, "Google authentication required. Please sign in via Settings."),
            Self::TokenExpired => write!(f, "Google OAuth token expired. Re-authenticating."),
            Self::RateLimited => write!(f, "Google API rate limit exceeded."),
            Self::NetworkError(msg) => write!(f, "Network error: {}", msg),
            Self::ApiError { code, message } => write!(f, "Google API error ({}): {}", code, message),
            Self::NotFound(item) => write!(f, "Item not found: {}", item),
            Self::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            Self::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
        }
    }
}

impl std::error::Error for GoogleError {}

// ─── Gmail Models ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MailThread {
    pub id: String,
    pub snippet: String,
    pub history_id: Option<String>,
    pub sender: String,
    pub subject: String,
    pub timestamp_ms: u64,
    pub is_unread: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MailMessage {
    pub id: String,
    pub thread_id: String,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub body_text: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SendReceipt {
    pub message_id: String,
    pub thread_id: String,
    pub recipient: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DraftReceipt {
    pub draft_id: String,
    pub thread_id: Option<String>,
    pub recipient: String,
}

/// Extracted deadline or schedule change from an email body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeadlineUpdate {
    pub context_phrase: String,
    pub task_or_subject: String,
    pub new_deadline_raw: String,
    pub is_extended: bool,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WatchStatus {
    Active,
    Triggered,
    Dismissed,
}

/// A connected Google account profile in the Multi-Email registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoogleAccountProfile {
    pub email: String,
    pub name: String,
    pub picture: Option<String>,
    pub is_primary: bool,
    pub added_at_ms: u64,
    pub scopes: Vec<String>,
    /// Phone number from People API (optional progressive scope
    /// `user.phonenumbers.read`). None = not granted or not saved in the
    /// Google Account (the common case — never an error).
    /// `#[serde(default)]` keeps pre-field profiles readable after upgrade
    /// (without it a missing key fails the whole vec parse and ALL
    /// accounts vanish from the UI).
    #[serde(default)]
    pub phone: Option<String>,
    /// Street address from People API (optional progressive scope
    /// `user.addresses.read`). Same absence semantics as phone.
    #[serde(default)]
    pub address: Option<String>,
}

/// A target email thread being actively watched by the Gmail Engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThreadWatchTarget {
    pub watch_id: String,
    pub thread_id: String,
    pub account_email: Option<String>,
    pub initial_history_id: Option<String>,
    pub sender: String,
    pub subject: String,
    pub initial_deadline_raw: Option<String>,
    pub message_count: usize,
    pub created_at_ms: u64,
    pub last_checked_ms: u64,
    pub status: WatchStatus,
}

/// Events emitted when a watched thread changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ThreadUpdateEvent {
    DeadlineChanged {
        old_deadline: String,
        new_deadline: String,
        snippet: String,
    },
    NewReply {
        sender: String,
        snippet: String,
    },
    AttachmentAdded {
        filenames: Vec<String>,
    },
}

// ─── Calendar Models ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalendarEvent {
    pub id: String,
    pub summary: String,
    pub description: Option<String>,
    pub start_iso: String,
    pub end_iso: String,
    pub location: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewEvent {
    pub summary: String,
    pub description: Option<String>,
    pub start_iso: String,
    pub end_iso: String,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventReceipt {
    pub event_id: String,
    pub summary: String,
    pub start_iso: String,
}

// ─── Maps Models ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum TravelMode {
    Driving,
    Walking,
    Bicycling,
    Transit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteSummary {
    pub origin: String,
    pub destination: String,
    pub distance_text: String,
    pub duration_text: String,
    pub duration_seconds: u32,
    pub traffic_delay_minutes: u32,
    pub summary_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommuteDelay {
    pub destination: String,
    pub normal_duration_min: u32,
    pub current_duration_min: u32,
    pub delay_minutes: u32,
    pub is_congested: bool,
    pub recommended_departure_iso: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlaceItem {
    pub name: String,
    pub address: String,
    pub rating: Option<f32>,
    pub open_now: Option<bool>,
}

// ─── Photos Models ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SemanticPhotoQuery {
    pub query: String,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PhotoItem {
    pub id: String,
    pub filename: String,
    pub product_url: String,
    pub mime_type: String,
    pub creation_time_iso: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlbumItem {
    pub id: String,
    pub title: String,
    pub total_media_items: u64,
}

// ─── Proactive Sentinel Models ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum AlertUrgency {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProactiveAlert {
    pub id: String,
    pub source: String,
    pub title: String,
    pub summary: String,
    pub spoken_line: String,
    pub urgency: AlertUrgency,
    pub timestamp_ms: u64,
}

// ─── Sentinel orb-landing payload (-tracking-first contract) ──────────
// Exact wire shape of the `orchestrator:sentinel-alert` event (see the Orb
// UI & Landing Specification). The frontend tracks these in the sentinel
// store; the landing animation (incoming_pulse → side_pill, 7s collapse)
// is a later phase. Field names are the spec's — do not rename casually:
// the TS interface mirrors them 1:1.

/// Organization derived from the sender domain (fallback until an org
/// directory exists — see `organization_from_sender`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlertOrganization {
    pub name: String,
    pub domain: String,
    pub avatar_fallback_initials: String,
    pub brand_color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlertContext {
    pub thread_id: String,
    pub subject: String,
    pub sender: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlertDeadline {
    pub is_extended: bool,
    pub previous_deadline: String,
    pub new_deadline: String,
    pub urgency: AlertUrgency,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LandingAnimation {
    pub initial_state: String,
    pub docked_state: String,
    pub auto_collapse_after_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SentinelAlertPayload {
    pub alert_id: String,
    pub source: String,
    pub account_email: Option<String>,
    pub organization: AlertOrganization,
    pub context: AlertContext,
    /// None for reply/attachment alerts (no deadline motion).
    pub deadline: Option<AlertDeadline>,
    pub landing_animation: LandingAnimation,
    pub spoken_notification: String,
}
