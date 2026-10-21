//! Personality tone (F0): butler (default, formal "sir") vs friend opt-in.
//!
//! Pure helpers — no I/O. Settings live in `commands.rs` (`persona_mode`).

/// True when the stored mode string selects friend tone. Unknown → butler.
pub fn is_friend(mode: &str) -> bool {
    mode.trim().eq_ignore_ascii_case("friend")
}

/// How to address the user: butler → always "sir"; friend → first name
/// when known, otherwise no address (callers skip the comma phrase).
/// Returns None only for friend-without-name.
pub fn address(name: Option<&str>, friend: bool) -> Option<String> {
    if !friend {
        return Some("sir".to_string());
    }
    match name.map(|n| n.trim()).filter(|n| !n.is_empty()) {
        Some(n) => Some(n.to_string()),
        None => None,
    }
}

/// "Hello, sir." vs "Hey, Lakshya." vs "Hey." — comma phrase included.
pub fn greet_address(name: Option<&str>, friend: bool) -> String {
    match address(name, friend) {
        Some(a) => format!(", {a}"),
        None => String::new(),
    }
}

/// Restyle a canned butler greeting for friend mode: ", sir" becomes
/// ", {name}" (or vanishes when the name is unknown). Pure + tested —
/// applied once at the orchestrator hook so every pick-list is covered.
pub fn restyle_greeting(reply: &str, name: Option<&str>) -> String {
    let addr = greet_address(name, true);
    reply.replace(", sir", &addr).replace("  ", " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_butler_always_sir() {
        assert_eq!(address(None, false).as_deref(), Some("sir"));
        assert_eq!(address(Some("Lakshya"), false).as_deref(), Some("sir"));
        assert_eq!(address(Some("Lakshya"), true).as_deref(), Some("Lakshya"));
        assert_eq!(address(None, true), None);
        assert!(!is_friend("butler"));
        assert!(!is_friend("FRIENDLY"));
        assert!(!is_friend(""));
        assert!(is_friend("friend"));
        assert!(is_friend("Friend"));
    }

    #[test]
    fn test_greet_address_shapes() {
        assert_eq!(greet_address(None, false), ", sir");
        assert_eq!(greet_address(Some("Lakshya"), true), ", Lakshya");
        assert_eq!(greet_address(None, true), "");
    }

    #[test]
    fn test_restyle_greeting_canned_lists() {
        // With a name.
        assert_eq!(
            restyle_greeting("Hello, sir.", Some("Lakshya")),
            "Hello, Lakshya."
        );
        assert_eq!(
            restyle_greeting("Hi, sir. How can I help?", Some("Lakshya")),
            "Hi, Lakshya. How can I help?"
        );
        assert_eq!(
            restyle_greeting("At your service, sir.", Some("Lakshya")),
            "At your service, Lakshya."
        );
        // Without a name the address vanishes cleanly (no double space).
        assert_eq!(restyle_greeting("Hello, sir.", None), "Hello.");
        assert_eq!(
            restyle_greeting("Hey, sir. What can I do for you?", None),
            "Hey. What can I do for you?"
        );
        // Replies without ", sir" pass through untouched.
        assert_eq!(restyle_greeting("All systems green. Ready?", Some("Lakshya")), "All systems green. Ready?");
    }
}
