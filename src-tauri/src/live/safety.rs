//! Safety layer for live mode — whitelist, denylist, and confirmation gates.
//!
//! Inspired by:
//!   - AnovaX two-stage safety filter (prompt rules + code whitelist/denylist)
//!   - OpenDex permission model (sensitive/optIn flags per skill)
//!   - SynapseKit SafetyPolicy (confirm_before, forbidden_apps)
//!   - Anthropic Computer Use guidelines (confirm destructive actions)
//!
//! Every live-mode action passes through `safety_check()` before execution.
//! Actions that match the denylist are refused. Actions in the
//! `confirm_before` list require explicit user confirmation.

/// Tools that are allowed in live mode.
pub const ALLOWED_TOOLS: &[&str] = &[
    "type_text",
    "press_key",
    "press_hotkey",
    "open_app",
    "close_app",
    "focus_app",
    "open_url",
    "open_search",
    "whatsapp_open",
    "whatsapp_search",
    "whatsapp_send",
    "browser_new_tab",
    "browser_navigate",
    "browser_search",
    "confirm_send",
    "cancel_action",
];

/// Targets that are never allowed (banking, password managers, etc.).
/// Matched case-insensitively against the target string.
pub const DENIED_TARGETS: &[&str] = &[
    "1password",
    "bitwarden",
    "keychain",
    "lastpass",
    "keepass",
    "dashlane",
    "bank",
    "paypal",
    "venmo",
    "cashapp",
    "stripe",
    "coinbase",
    "metamask",
    "trezor",
    "ledger",
];

/// Actions that always require user confirmation before execution.
pub const CONFIRM_ACTIONS: &[&str] = &[
    "whatsapp_send",   // sending a message is irreversible
    "confirm_send",     // explicit send confirmation
    "close_app",       // closing an app may lose work
];

/// Check if a tool is allowed.
pub fn is_tool_allowed(tool: &str) -> bool {
    ALLOWED_TOOLS.contains(&tool)
}

/// Check if a target (app name, URL, etc.) is blocked.
pub fn is_target_blocked(target: &str) -> bool {
    let lower = target.to_lowercase();
    DENIED_TARGETS.iter().any(|d| lower.contains(d))
}

/// Check if an action requires user confirmation.
pub fn needs_confirmation(tool: &str) -> bool {
    CONFIRM_ACTIONS.contains(&tool)
}

/// Full safety check for a (tool, target) pair.
/// Returns Ok(()) if the action is safe to execute,
/// Err(message) if it should be blocked.
/// Returns Ok(ConfirmRequired) if the action needs user confirmation.
pub fn safety_check(tool: &str, target: Option<&str>) -> SafetyVerdict {
    if !is_tool_allowed(tool) {
        return SafetyVerdict::Blocked(format!("Tool '{tool}' is not in the allowed list"));
    }

    if let Some(t) = target {
        if is_target_blocked(t) {
            return SafetyVerdict::Blocked(format!(
                "Target '{t}' is blocked for your safety, sir"
            ));
        }
    }

    if needs_confirmation(tool) {
        return SafetyVerdict::NeedsConfirmation;
    }

    SafetyVerdict::Allowed
}

/// The verdict from a safety check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyVerdict {
    /// The action is safe to execute immediately.
    Allowed,
    /// The action is safe but requires user confirmation first.
    NeedsConfirmation,
    /// The action is blocked and must not execute.
    Blocked(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowed_tools() {
        assert!(is_tool_allowed("type_text"));
        assert!(is_tool_allowed("whatsapp_send"));
        assert!(is_tool_allowed("open_app"));
        assert!(!is_tool_allowed("delete_file"));
        assert!(!is_tool_allowed("rm_rf"));
    }

    #[test]
    fn test_blocked_targets() {
        assert!(is_target_blocked("1Password"));
        assert!(is_target_blocked("Bitwarden"));
        assert!(is_target_blocked("my bank account"));
        assert!(is_target_blocked("PayPal"));
        assert!(!is_target_blocked("whatsapp"));
        assert!(!is_target_blocked("brave"));
        assert!(!is_target_blocked("notepad"));
    }

    #[test]
    fn test_confirmation_required() {
        assert!(needs_confirmation("whatsapp_send"));
        assert!(needs_confirmation("confirm_send"));
        assert!(!needs_confirmation("type_text"));
        assert!(!needs_confirmation("open_app"));
    }

    #[test]
    fn test_safety_check_allowed() {
        assert_eq!(safety_check("type_text", None), SafetyVerdict::Allowed);
        assert_eq!(
            safety_check("open_app", Some("notepad")),
            SafetyVerdict::Allowed
        );
    }

    #[test]
    fn test_safety_check_blocked() {
        assert_eq!(
            safety_check("delete_file", None),
            SafetyVerdict::Blocked("Tool 'delete_file' is not in the allowed list".to_string())
        );
        assert_eq!(
            safety_check("open_app", Some("1Password")),
            SafetyVerdict::Blocked(
                "Target '1Password' is blocked for your safety, sir".to_string()
            )
        );
    }

    #[test]
    fn test_safety_check_needs_confirmation() {
        assert_eq!(
            safety_check("whatsapp_send", None),
            SafetyVerdict::NeedsConfirmation
        );
        assert_eq!(
            safety_check("confirm_send", None),
            SafetyVerdict::NeedsConfirmation
        );
    }
}
