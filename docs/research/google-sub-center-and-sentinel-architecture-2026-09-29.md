# GoogleCenter Hierarchical Domain Architecture & Proactive Sentinel — Research & Contract Specification

**Date**: 2026-09-29  
**Status**: APPROVED SPECIFICATION  
**Target Feature**: [Feature 75 (`docs/features/75-google-center-hierarchical-domain-architecture-and-proactive-sentinel.md`)](file:///C:/PROJECTS/ULTRON/docs/features/75-google-center-hierarchical-domain-architecture-and-proactive-sentinel.md)  
**Parent Architecture**: [Feature 74 Main Center + Per-Action Sub-Centers](file:///C:/PROJECTS/ULTRON/docs/features/74-main-center-with-per-action-sub-centers.md)

---

## 1. Context & Motivation

In current voice assistants (Siri, Google Assistant, Alexa), integration with Google services is often treated as a set of fragmented, reactive intents ("Read my unread emails", "Navigate to home"). 
This approach fails in two fundamental ways:
1. **Reactive Only**: The user has to remember to ask. If an email arrives at 2:00 PM saying *"The submission deadline is moved to 4:00 PM today"*, a reactive assistant does nothing until the user asks, by which point it's too late.
2. **Monolithic or Fragmented**: Either all Google actions are crammed into one unmaintainable handler (causing dialog state confusion), or they are scattered across generic handlers (duplicating OAuth token refresh loops and session management).

This specification resolves both issues by implementing a **Hierarchical Domain Architecture with an Autonomous Proactive Sentinel**.

---

## 2. Component Specifications & Contracts

### 2.1 Sub-Command Services (Domain Engines)

Each domain engine is an isolated Rust struct with zero Tauri/UI dependencies, allowing 100% unit-testability using mock HTTP responders.

```
                    ┌──────────────────────────────┐
                    │      GoogleCenter Hub        │
                    │   (SubCenter Trait Impl)     │
                    └──────────────┬───────────────┘
                                   │
         ┌──────────────┬──────────┴───┬──────────────┬──────────────┐
         ▼              ▼              ▼              ▼              ▼
   ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐
   │  mail.rs  │  │calendar.rs│  │  maps.rs  │  │ photos.rs │  │sentinel.rs│
   └───────────┘  └───────────┘  └───────────┘  └───────────┘  └───────────┘
```

#### Module 1: `mail.rs` (Gmail Engine)
* **APIs Used**: Gmail REST API v1 (`/gmail/v1/users/me/messages`, `/threads`, `/drafts`).
* **OAuth Scopes**: `https://www.googleapis.com/auth/gmail.modify`
* **Core Functions**:
  ```rust
  pub async fn search_threads(client: &GoogleClient, query: &str) -> Result<Vec<MailThread>, GoogleError>;
  pub async fn draft_reply(client: &GoogleClient, thread_id: &str, body: &str) -> Result<DraftReceipt, GoogleError>;
  pub async fn send_email(client: &GoogleClient, to: &str, subject: &str, body: &str) -> Result<SendReceipt, GoogleError>;
  pub fn parse_deadline_update(body: &str) -> Option<DeadlineUpdate>;
  ```
* **Deadline Detection Heuristics**:
  - Matches patterns like `(?i)(?:deadline|submission|due date|scheduled for)\s+(?:has been\s+)?(?:moved|extended|postponed|rescheduled|changed)\s+to\s+([A-Za-z0-9:,\s]+)`.

#### Module 2: `calendar.rs` (Google Calendar Engine)
* **APIs Used**: Google Calendar API v3 (`/calendar/v3/calendars/primary/events`).
* **OAuth Scopes**: `https://www.googleapis.com/auth/calendar.events`
* **Core Functions**:
  ```rust
  pub async fn get_agenda(client: &GoogleClient, date: NaiveDate) -> Result<Vec<CalendarEvent>, GoogleError>;
  pub async fn create_event(client: &GoogleClient, event: NewEvent) -> Result<EventReceipt, GoogleError>;
  pub async fn reschedule_event(client: &GoogleClient, event_id: &str, new_time: DateTime<Utc>) -> Result<EventReceipt, GoogleError>;
  pub async fn find_free_buffers(client: &GoogleClient, duration_min: u32) -> Result<Vec<TimeRange>, GoogleError>;
  ```

#### Module 3: `maps.rs` (Google Maps Engine)
* **APIs Used**: Google Maps Directions API, Distance Matrix API, Places API.
* **Core Functions**:
  ```rust
  pub async fn get_route(client: &GoogleClient, origin: &str, destination: &str, mode: TravelMode) -> Result<RouteSummary, GoogleError>;
  pub async fn check_commute_delay(client: &GoogleClient, destination: &str, target_arrival: DateTime<Utc>) -> Result<CommuteDelay, GoogleError>;
  ```

#### Module 4: `photos.rs` (Google Photos Engine)
* **APIs Used**: Google Photos Library API v1 (`/v1/mediaItems:search`).
* **OAuth Scopes**: `https://www.googleapis.com/auth/photoslibrary.readonly`
* **Core Functions**:
  ```rust
  pub async fn search_photos(client: &GoogleClient, query: SemanticPhotoQuery) -> Result<Vec<PhotoItem>, GoogleError>;
  ```

#### Module 5: `sentinel.rs` (Proactive Background Sentinel)
* **Architecture**:
  - Runs in a detached background Tokio task spawned during app initialization.
  - Sleep cadence: Checks Gmail history (`/history?startHistoryId=...`) every 60s (or listens via Webhook/Push if configured).
  - Maintains `SeenMessageRingBuffer` (size 256) to ensure no message triggers more than once.
  - When an urgent deadline/schedule change is detected:
    1. Synthesizes a proactive notification payload.
    2. Consults the Main Center state: If user is actively speaking or in a meeting (`meeting_active`), holds the alert until idle.
    3. Triggers voice alert via `direct_ui(app, UiDirective::Speak { ... })`.

---

## 3. Strict Dialog Contract (Feature 74 Compliance)

Under Feature 74, `GoogleCenter` implements:
```rust
impl SubCenter for GoogleCenter {
    fn name(&self) -> &'static str { "GoogleCenter" }
    fn validate(&self, intent: &ParsedIntent) -> Validity;
    fn execute(&self, app: &AppHandle, intent: &ParsedIntent) -> ReceiptFuture;
    fn confirm_kind(&self, intent: &ParsedIntent) -> ConfirmKind;
}
```

### Slot Validation & Failure Elicitation Matrix

| Intent | Required Slots | Missing Slot Elicitation Template | Failure / Invalid Template |
| :--- | :--- | :--- | :--- |
| `GoogleMailSend` | `recipient`, `body` | *"Who should I send this email to, sir?"* | *"I couldn't find an email address for {recipient}, sir."* |
| `GoogleMailSearch` | `query` | *"What emails are you looking for, sir?"* | *"No emails found matching {query}, sir."* |
| `GoogleCalendarSchedule`| `title`, `start_time` | *"What time would you like to schedule {title}, sir?"* | *"That time conflicts with your 3 PM meeting, sir."* |
| `GoogleMapsDirections` | `destination` | *"Where would you like directions to, sir?"* | *"I couldn't locate {destination} on the map, sir."* |
| `GooglePhotosSearch` | `query` | *"Which photos or memories should I search for, sir?"* | *"No photos found for {query}, sir."* |

---

## 4. Testing & Verification Strategy

Following the user's principle: **"Sub command center then each being created and tested perfectly, then moving to the next after all done, connecting it to the Google center... if perfect connect it to the Main center."**

### Stage 1: Individual Sub-Command Test Suite
* `mail_tests.rs`: Mock Gmail API server responses (threads list, draft create, send) + 15 regex deadline test cases.
* `calendar_tests.rs`: Mock Calendar API responses (agenda fetch, event insert, free-busy overlap logic).
* `maps_tests.rs`: Mock Directions API responses (poly line parse, traffic delay computation).
* `photos_tests.rs`: Mock Photos Library search responses (date filters, keyword queries).
* `sentinel_tests.rs`: Simulated incoming email history stream verifying alert emission & de-duplication.

### Stage 2: GoogleCenter Hub Integration Suite
* Verify that `GoogleCenter::validate()` accurately catches missing slots and produces exact Alexa-standard elicitation phrases.
* Verify that destructive actions (`GoogleMailSend`, `GoogleCalendarDelete`) enforce `ConfirmKind::Gate`.

### Stage 3: Main Center Routing & End-to-End
* Connect to `center_registry.rs`.
* Verify that spoken voice commands route cleanly to `GoogleCenter` and trigger appropriate repeat-backs ("Sending email to Professor Sharma, sir.").
* Verify that proactive alerts fire smoothly without visual glitched or audio clipping.
