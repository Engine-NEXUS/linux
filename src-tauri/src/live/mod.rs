//! NEXUS Live Mode — always-listening, STT-only, full-laptop control.
//!
//! Architecture (inspired by AnovaX, OpenDex, SAM ScreenParser, Ari):
//!
//!   Continuous VAD → STT → Intent Parser →
//!   ┌─────────────────────────────────┐
//!   │ Simple command → direct execute │
//!   │ Multi-step → state machine       │
//!   │ Novel → LLM plan → executor      │
//!   └─────────────────────────────────┘
//!   → Safety check → Execute → Resume VAD
//!
//! TTS is disabled by default. Only spoken when:
//!   - User asks a question
//!   - Confirmation needed
//!   - Error occurred
//!   - User explicitly requests speech
//!
//! Key design principles (from research):
//!   1. "The model is the dispatcher. The skills are the workers."
//!      — Agent-Sam. Deterministic parser first, LLM only when needed.
//!   2. "LLM plans once, orchestrator executes." — AnovaX. The LLM
//!      never touches the keyboard directly.
//!   3. Two-table pattern from SAM ScreenParser: LLM picks element IDs,
//!      Rust resolves to coordinates. LLM can't hallucinate clicks.
//!   4. Clipboard paste > character typing. — OpenDex. Avoids
//!      autocomplete corruption in WhatsApp/search boxes.
//!   5. AttachThreadInput trick for window focus. — ghost-hands.
//!      Without it, SetForegroundWindow silently fails from background.

pub mod commands;
pub mod state;
pub mod safety;

use serde::{Deserialize, Serialize};

/// Result of a live-mode command execution.
#[derive(Debug, Clone, Serialize)]
pub struct LiveResult {
    pub success: bool,
    /// Human-readable message (spoken only if TTS is needed).
    pub message: String,
    /// New state after this command (for the state machine).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_state: Option<state::LiveState>,
    /// Whether this action requires user confirmation before executing.
    #[serde(default)]
    pub requires_confirmation: bool,
}

impl LiveResult {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            new_state: None,
            requires_confirmation: false,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            new_state: None,
            requires_confirmation: false,
        }
    }

    pub fn with_state(mut self, state: state::LiveState) -> Self {
        self.new_state = Some(state);
        self
    }

    pub fn needs_confirmation(mut self) -> Self {
        self.requires_confirmation = true;
        self
    }
}

/// Parsed live-mode intent — extends the existing ParsedIntent with
/// actions that only make sense in live mode (type, press, send).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "action")]
pub enum LiveIntent {
    #[serde(rename = "type_text")]
    TypeText { text: String },
    #[serde(rename = "press_key")]
    PressKey { key: String },
    #[serde(rename = "press_hotkey")]
    PressHotkey { keys: Vec<String> },
    #[serde(rename = "send_message_live")]
    SendMessageLive {
        contact: Option<String>,
        text: String,
    },
    #[serde(rename = "confirm_send")]
    ConfirmSend,
    #[serde(rename = "cancel_action")]
    CancelAction,
    #[serde(rename = "whatsapp_open")]
    WhatsappOpen,
    #[serde(rename = "whatsapp_search")]
    WhatsappSearch { contact: String },
    #[serde(rename = "browser_new_tab")]
    BrowserNewTab,
    #[serde(rename = "browser_navigate")]
    BrowserNavigate { url: String },
    #[serde(rename = "browser_search")]
    BrowserSearch { query: String },
    #[serde(rename = "focus_app")]
    FocusApp { target: String },
    #[serde(rename = "unknown")]
    Unknown { raw: String },
}

// ─── Tauri commands ──────────────────────────────────────────────────────

/// Type text into the currently focused input field.
#[tauri::command]
pub async fn live_type_text(text: String) -> Result<LiveResult, String> {
    let verdict = safety::safety_check("type_text", None);
    if let safety::SafetyVerdict::Blocked(msg) = verdict {
        return Ok(LiveResult::err(msg));
    }

    match commands::keyboard::type_text(&text) {
        Ok(()) => {
            let ctx = state::context();
            let current = ctx.state();
            // If we're in ChatActive, transition to TextTyped
            if let state::LiveState::ChatActive { app, contact } = current {
                let new_state = state::LiveState::TextTyped {
                    app,
                    contact,
                    text: text.clone(),
                };
                ctx.transition(new_state);
                Ok(LiveResult::ok(format!("Typed: {text}"))
                    .with_state(ctx.state()))
            } else {
                Ok(LiveResult::ok(format!("Typed: {text}")))
            }
        }
        Err(e) => Ok(LiveResult::err(e)),
    }
}

/// Press a single key (e.g., "enter", "escape", "tab").
#[tauri::command]
pub async fn live_press_key(key: String) -> Result<LiveResult, String> {
    let verdict = safety::safety_check("press_key", None);
    if let safety::SafetyVerdict::Blocked(msg) = verdict {
        return Ok(LiveResult::err(msg));
    }

    match commands::keyboard::press_key(&key) {
        Ok(()) => Ok(LiveResult::ok(format!("Pressed {key}"))),
        Err(e) => Ok(LiveResult::err(e)),
    }
}

/// Press a key combination (e.g., ["ctrl", "a"], ["ctrl", "shift", "tab"]).
#[tauri::command]
pub async fn live_press_hotkey(keys: Vec<String>) -> Result<LiveResult, String> {
    let verdict = safety::safety_check("press_hotkey", None);
    if let safety::SafetyVerdict::Blocked(msg) = verdict {
        return Ok(LiveResult::err(msg));
    }

    let keys_ref: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
    match commands::keyboard::press_hotkey(&keys_ref) {
        Ok(()) => Ok(LiveResult::ok(format!("Pressed {}", keys.join("+")))),
        Err(e) => Ok(LiveResult::err(e)),
    }
}

/// Open WhatsApp and optionally search for a contact.
#[tauri::command]
pub async fn live_whatsapp_open() -> Result<LiveResult, String> {
    let verdict = safety::safety_check("whatsapp_open", None);
    if let safety::SafetyVerdict::Blocked(msg) = verdict {
        return Ok(LiveResult::err(msg));
    }

    match commands::whatsapp::open_whatsapp() {
        Ok(()) => {
            let ctx = state::context();
            ctx.transition(state::LiveState::AppOpen {
                app: "whatsapp".to_string(),
            });
            Ok(LiveResult::ok("WhatsApp open, sir.")
                .with_state(ctx.state()))
        }
        Err(e) => Ok(LiveResult::err(e)),
    }
}

/// Search for a contact in WhatsApp and open their chat.
#[tauri::command]
pub async fn live_whatsapp_search(contact: String) -> Result<LiveResult, String> {
    let verdict = safety::safety_check("whatsapp_search", Some(&contact));
    if let safety::SafetyVerdict::Blocked(msg) = verdict {
        return Ok(LiveResult::err(msg));
    }

    match commands::whatsapp::search_contact(&contact) {
        Ok(()) => {
            let ctx = state::context();
            ctx.transition(state::LiveState::ChatActive {
                app: "whatsapp".to_string(),
                contact: contact.clone(),
            });
            Ok(LiveResult::ok(format!("Opened chat with {contact}, sir."))
                .with_state(ctx.state()))
        }
        Err(e) => Ok(LiveResult::err(e)),
    }
}

/// Send the currently typed message in WhatsApp (presses Enter).
/// ALWAYS requires confirmation — this is an irreversible action.
#[tauri::command]
pub async fn live_whatsapp_send() -> Result<LiveResult, String> {
    let verdict = safety::safety_check("whatsapp_send", None);
    match verdict {
        safety::SafetyVerdict::Blocked(msg) => Ok(LiveResult::err(msg)),
        safety::SafetyVerdict::NeedsConfirmation => {
            // Return a result that tells the frontend to ask for confirmation
            Ok(LiveResult::ok("Ready to send. Please confirm.")
                .needs_confirmation())
        }
        safety::SafetyVerdict::Allowed => {
            // This path is reached after the user confirms
            match commands::whatsapp::send_message() {
                Ok(()) => {
                    let ctx = state::context();
                    ctx.reset();
                    Ok(LiveResult::ok("Message sent, sir."))
                }
                Err(e) => Ok(LiveResult::err(e)),
            }
        }
    }
}

/// Full WhatsApp flow: open → search contact → type message.
/// The send step is NOT included — user must call live_whatsapp_send separately.
#[tauri::command]
pub async fn live_whatsapp_type_message(
    contact: Option<String>,
    text: String,
) -> Result<LiveResult, String> {
    let ctx = state::context();
    let current = ctx.state();

    // If a contact is specified, search for them first
    if let Some(contact) = &contact {
        let verdict = safety::safety_check("whatsapp_search", Some(contact));
        if let safety::SafetyVerdict::Blocked(msg) = verdict {
            return Ok(LiveResult::err(msg));
        }

        // If WhatsApp isn't open yet, open it first
        if !matches!(current, state::LiveState::AppOpen { .. } | state::LiveState::ChatActive { .. } | state::LiveState::TextTyped { .. }) {
            if let Err(e) = commands::whatsapp::open_whatsapp() {
                return Ok(LiveResult::err(e));
            }
        }

        match commands::whatsapp::search_contact(contact) {
            Ok(()) => {
                ctx.transition(state::LiveState::ChatActive {
                    app: "whatsapp".to_string(),
                    contact: contact.clone(),
                });
            }
            Err(e) => return Ok(LiveResult::err(e)),
        }
    }

    // Type the message
    match commands::whatsapp::type_message(&text) {
        Ok(()) => {
            let new_state = match ctx.state() {
                state::LiveState::ChatActive { app, contact } => state::LiveState::TextTyped {
                    app,
                    contact,
                    text: text.clone(),
                },
                _ => state::LiveState::TextTyped {
                    app: "whatsapp".to_string(),
                    contact: String::new(),
                    text: text.clone(),
                },
            };
            ctx.transition(new_state);
            Ok(LiveResult::ok(format!("Typed: {text}. Say 'send' when ready."))
                .with_state(ctx.state()))
        }
        Err(e) => Ok(LiveResult::err(e)),
    }
}

/// Open a new browser tab.
#[tauri::command]
pub async fn live_browser_new_tab() -> Result<LiveResult, String> {
    let verdict = safety::safety_check("browser_new_tab", None);
    if let safety::SafetyVerdict::Blocked(msg) = verdict {
        return Ok(LiveResult::err(msg));
    }

    match commands::browser::new_tab() {
        Ok(()) => Ok(LiveResult::ok("New tab opened, sir.")),
        Err(e) => Ok(LiveResult::err(e)),
    }
}

/// Navigate to a URL in the current browser tab.
#[tauri::command]
pub async fn live_browser_navigate(url: String) -> Result<LiveResult, String> {
    let verdict = safety::safety_check("browser_navigate", Some(&url));
    if let safety::SafetyVerdict::Blocked(msg) = verdict {
        return Ok(LiveResult::err(msg));
    }

    match commands::browser::navigate(&url) {
        Ok(()) => Ok(LiveResult::ok(format!("Navigating to {url}"))),
        Err(e) => Ok(LiveResult::err(e)),
    }
}

/// Search in the current browser tab.
#[tauri::command]
pub async fn live_browser_search(query: String) -> Result<LiveResult, String> {
    let verdict = safety::safety_check("browser_search", Some(&query));
    if let safety::SafetyVerdict::Blocked(msg) = verdict {
        return Ok(LiveResult::err(msg));
    }

    match commands::browser::search(&query) {
        Ok(()) => {
            let ctx = state::context();
            ctx.transition(state::LiveState::BrowserSearch {
                query: query.clone(),
            });
            Ok(LiveResult::ok(format!("Searching for {query}"))
                .with_state(ctx.state()))
        }
        Err(e) => Ok(LiveResult::err(e)),
    }
}

/// Open a website by name (e.g., "wikipedia", "github").
#[tauri::command]
pub async fn live_open_site(site: String) -> Result<LiveResult, String> {
    let verdict = safety::safety_check("open_url", Some(&site));
    if let safety::SafetyVerdict::Blocked(msg) = verdict {
        return Ok(LiveResult::err(msg));
    }

    match commands::browser::open_site(&site) {
        Ok(()) => {
            let ctx = state::context();
            ctx.transition(state::LiveState::BrowserOpen { url: None });
            Ok(LiveResult::ok(format!("Opening {site}"))
                .with_state(ctx.state()))
        }
        Err(e) => Ok(LiveResult::err(e)),
    }
}

/// Focus an app by title (bring it to the foreground).
#[tauri::command]
pub async fn live_focus_app(target: String) -> Result<LiveResult, String> {
    let verdict = safety::safety_check("focus_app", Some(&target));
    if let safety::SafetyVerdict::Blocked(msg) = verdict {
        return Ok(LiveResult::err(msg));
    }

    #[cfg(target_os = "windows")]
    {
        if commands::window::focus_app_by_title(&target) {
            let ctx = state::context();
            ctx.transition(state::LiveState::AppOpen {
                app: target.clone(),
            });
            Ok(LiveResult::ok(format!("Focused {target}"))
                .with_state(ctx.state()))
        } else {
            Ok(LiveResult::err(format!(
                "Couldn't find {target} running, sir."
            )))
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(LiveResult::err("Window focus not supported on this platform"))
    }
}

/// Cancel the current live-mode action and reset state.
#[tauri::command]
pub async fn live_cancel() -> Result<LiveResult, String> {
    let ctx = state::context();
    ctx.reset();
    Ok(LiveResult::ok("Cancelled, sir."))
}

/// Get the current live-mode state.
#[tauri::command]
pub async fn live_get_state() -> Result<serde_json::Value, String> {
    let ctx = state::context();
    let state = ctx.state();
    Ok(serde_json::json!({
        "state": state.describe(),
        "expired": ctx.is_expired(),
        "seconds_since_last_action": ctx.time_since_last_action().as_secs(),
    }))
}
