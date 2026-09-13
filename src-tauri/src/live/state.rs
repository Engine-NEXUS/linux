//! Live mode state machine — tracks context across sequential voice commands.
//!
//! This is NEXUS's unique contribution. No open-source project we studied
//! tracks context across sequential voice commands. They either do single
//! commands (Ari, OpenDex) or full LLM plans (AnovaX). NEXUS's state machine
//! approach is simpler and more predictable.
//!
//! Example flow:
//!   User: "open whatsapp"     → state: AppOpen { app: "whatsapp" }
//!   User: "chat with mummy"   → state: ChatActive { app, contact: "mummy" }
//!   User: "type hi"           → state: TextTyped { app, contact, text: "hi" }
//!   User: "send"              → executes send, state: Idle
//!
//! Auto-resets to Idle after 30s of silence (configurable).

use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The current state of the live-mode conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum LiveState {
    /// No context — ready for a new command sequence.
    Idle,
    /// An app was just opened/focused.
    AppOpen { app: String },
    /// A chat is active (e.g., WhatsApp chat with a specific contact).
    ChatActive { app: String, contact: String },
    /// Text has been typed into a chat but not yet sent.
    TextTyped { app: String, contact: String, text: String },
    /// A browser search was performed.
    BrowserSearch { query: String },
    /// A browser tab is open and ready for navigation.
    BrowserOpen { url: Option<String> },
}

impl LiveState {
    /// Returns a human-readable description of the current state.
    pub fn describe(&self) -> String {
        match self {
            LiveState::Idle => "idle".to_string(),
            LiveState::AppOpen { app } => format!("{app} open"),
            LiveState::ChatActive { app, contact } => {
                format!("{app} chat with {contact}")
            }
            LiveState::TextTyped { app, contact, text } => {
                format!("{app} chat with {contact}, typed: {text}")
            }
            LiveState::BrowserSearch { query } => format!("searched: {query}"),
            LiveState::BrowserOpen { url } => {
                format!("browser open{}", url.as_ref().map(|u| format!(" at {u}")).unwrap_or_default())
            }
        }
    }

    /// Whether the current state implies an app is focused.
    pub fn app_context(&self) -> Option<&str> {
        match self {
            LiveState::AppOpen { app }
            | LiveState::ChatActive { app, .. }
            | LiveState::TextTyped { app, .. } => Some(app),
            _ => None,
        }
    }

    /// Whether there's unsent text that a "send" command would dispatch.
    pub fn has_pending_text(&self) -> bool {
        matches!(self, LiveState::TextTyped { .. })
    }
}

/// Thread-safe live-mode context shared across command invocations.
pub struct LiveContext {
    state: Mutex<LiveState>,
    last_action: Mutex<Instant>,
    timeout: Duration,
}

impl LiveContext {
    /// Create a new context with the default 30s timeout.
    pub fn new() -> Self {
        Self {
            state: Mutex::new(LiveState::Idle),
            last_action: Mutex::new(Instant::now()),
            timeout: Duration::from_secs(30),
        }
    }

    /// Get the current state. Resets to Idle if expired.
    pub fn state(&self) -> LiveState {
        let mut guard = self.state.lock().unwrap();
        if self.is_expired() {
            *guard = LiveState::Idle;
        }
        guard.clone()
    }

    /// Transition to a new state and update the last-action timestamp.
    pub fn transition(&self, new_state: LiveState) {
        let mut guard = self.state.lock().unwrap();
        tracing::info!("live: {} → {}", guard.describe(), new_state.describe());
        *guard = new_state;
        *self.last_action.lock().unwrap() = Instant::now();
    }

    /// Reset to Idle immediately.
    pub fn reset(&self) {
        let mut guard = self.state.lock().unwrap();
        tracing::info!("live: {} → idle (reset)", guard.describe());
        *guard = LiveState::Idle;
        *self.last_action.lock().unwrap() = Instant::now();
    }

    /// Check if the context has expired (no action for `timeout` duration).
    pub fn is_expired(&self) -> bool {
        self.last_action.lock().unwrap().elapsed() > self.timeout
    }

    /// Time since the last action.
    pub fn time_since_last_action(&self) -> Duration {
        self.last_action.lock().unwrap().elapsed()
    }
}

/// Global live-mode context (lazy-initialized, shared across Tauri commands).
static LIVE_CONTEXT: once_cell::sync::Lazy<Arc<LiveContext>> =
    once_cell::sync::Lazy::new(|| Arc::new(LiveContext::new()));

/// Get the global live-mode context.
pub fn context() -> Arc<LiveContext> {
    LIVE_CONTEXT.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_describe() {
        assert_eq!(LiveState::Idle.describe(), "idle");
        assert_eq!(
            LiveState::AppOpen { app: "whatsapp".into() }.describe(),
            "whatsapp open"
        );
        assert_eq!(
            LiveState::ChatActive { app: "whatsapp".into(), contact: "mummy".into() }.describe(),
            "whatsapp chat with mummy"
        );
    }

    #[test]
    fn test_app_context() {
        assert_eq!(LiveState::Idle.app_context(), None);
        assert_eq!(
            LiveState::AppOpen { app: "brave".into() }.app_context(),
            Some("brave")
        );
        assert_eq!(
            LiveState::ChatActive { app: "whatsapp".into(), contact: "mum".into() }.app_context(),
            Some("whatsapp")
        );
    }

    #[test]
    fn test_has_pending_text() {
        assert!(!LiveState::Idle.has_pending_text());
        assert!(
            LiveState::TextTyped {
                app: "whatsapp".into(),
                contact: "mummy".into(),
                text: "hi".into()
            }
            .has_pending_text()
        );
    }

    #[test]
    fn test_context_transition() {
        let ctx = LiveContext::new();
        assert_eq!(ctx.state(), LiveState::Idle);

        ctx.transition(LiveState::AppOpen { app: "whatsapp".into() });
        assert_eq!(
            ctx.state(),
            LiveState::AppOpen { app: "whatsapp".into() }
        );

        ctx.reset();
        assert_eq!(ctx.state(), LiveState::Idle);
    }
}
