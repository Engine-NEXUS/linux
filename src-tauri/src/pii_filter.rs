//! PII filtering middleware — redacts personally identifiable information
//! from transcripts before they leave the device (Groq STT, Cloudflare Worker).
//!
//! Patterns: email, phone (IN + intl), Aadhaar, PAN, credit card, IPv4.
//! All detection is local regex — no cloud dependency.

use regex::Regex;
use std::sync::OnceLock;

fn patterns() -> &'static PiiPatterns {
    static PATTERNS: OnceLock<PiiPatterns> = OnceLock::new();
    PATTERNS.get_or_init(|| PiiPatterns {
        email: Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap(),
        phone_in: Regex::new(r"\+91[\s-]?[6-9]\d{9}|\b[6-9]\d{9}\b").unwrap(),
        aadhaar: Regex::new(r"\b\d{4}[\s-]?\d{4}[\s-]?\d{4}\b").unwrap(),
        pan: Regex::new(r"\b[A-Z]{5}\d{4}[A-Z]\b").unwrap(),
        card: Regex::new(r"\d{4}[\s-]\d{4}[\s-]\d{4}[\s-]\d{4}|\d{16}").unwrap(),
        ipv4: Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b").unwrap(),
    })
}

struct PiiPatterns {
    email: Regex,
    phone_in: Regex,
    aadhaar: Regex,
    pan: Regex,
    card: Regex,
    ipv4: Regex,
}

/// Redact PII from text. Replaces sensitive spans with `[REDACTED:<type>]`.
pub fn sanitize(input: &str) -> String {
    let p = patterns();
    let mut out = input.to_string();

    out = p.email.replace_all(&out, "[REDACTED:EMAIL]").to_string();
    out = p.card.replace_all(&out, "[REDACTED:CARD]").to_string();
    out = p.aadhaar.replace_all(&out, "[REDACTED:AADHAAR]").to_string();
    out = p.pan.replace_all(&out, "[REDACTED:PAN]").to_string();
    out = p.ipv4.replace_all(&out, "[REDACTED:IP]").to_string();
    out = p.phone_in.replace_all(&out, "[REDACTED:PHONE]").to_string();

    out
}

/// Count PII hits without redacting (for logging/telemetry).
#[allow(dead_code)]
pub fn count_hits(input: &str) -> usize {
    let p = patterns();
    p.email.find_iter(input).count()
        + p.aadhaar.find_iter(input).count()
        + p.pan.find_iter(input).count()
        + p.card.find_iter(input).count()
        + p.ipv4.find_iter(input).count()
        + p.phone_in.find_iter(input).count()
}

/// Sanitize a dialog_context value before it leaves the device:
/// history turn contents + memory block. Shape preserved.
pub fn sanitize_context(ctx: &serde_json::Value) -> serde_json::Value {
    let mut out = ctx.clone();
    if let Some(obj) = out.as_object_mut() {
        if let Some(history) = obj.get_mut("history").and_then(|h| h.as_array_mut()) {
            for turn in history.iter_mut() {
                if let Some(content) = turn.get_mut("content") {
                    if let Some(s) = content.as_str() {
                        *content = serde_json::Value::String(sanitize(s));
                    }
                }
            }
        }
        if let Some(mem) = obj.get_mut("memory") {
            if let Some(s) = mem.as_str() {
                *mem = serde_json::Value::String(sanitize(s));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_redacted() {
        let out = sanitize("contact me at john.doe@example.com please");
        assert!(!out.contains("john.doe@example.com"));
        assert!(out.contains("[REDACTED:EMAIL]"));
    }

    #[test]
    fn test_indian_phone_redacted() {
        let out = sanitize("call me at 9876543210");
        assert!(!out.contains("9876543210"));
        assert!(out.contains("[REDACTED:PHONE]"));
    }

    #[test]
    fn test_aadhaar_redacted() {
        let out = sanitize("aadhaar 1234 5678 9012");
        assert!(out.contains("[REDACTED:AADHAAR]"));
    }

    #[test]
    fn test_pan_redacted() {
        let out = sanitize("PAN ABCDE1234F");
        assert!(out.contains("[REDACTED:PAN]"));
    }

    #[test]
    fn test_card_redacted() {
        let out = sanitize("card 4111 1111 1111 1111");
        assert!(out.contains("[REDACTED:CARD]"));
    }

    #[test]
    fn test_ipv4_redacted() {
        let out = sanitize("server at 192.168.1.1");
        assert!(out.contains("[REDACTED:IP]"));
    }

    #[test]
    fn test_clean_text_unchanged() {
        let input = "open chrome and search for cats";
        assert_eq!(sanitize(input), input);
    }

    #[test]
    fn test_multiple_pii_types() {
        let out = sanitize("email a@b.com phone 9876543210 card 4111 1111 1111 1111");
        assert!(out.contains("[REDACTED:EMAIL]"));
        assert!(out.contains("[REDACTED:PHONE]"));
        assert!(out.contains("[REDACTED:CARD]"));
        assert_eq!(count_hits("email a@b.com phone 9876543210"), 2);
    }

    #[test]
    fn test_false_positives_minimal() {
        // Short digit strings that are NOT card numbers should pass through
        let out = sanitize("open 3 tabs");
        assert_eq!(out, "open 3 tabs");
    }

    #[test]
    fn test_sanitize_context_history_and_memory() {
        let ctx = serde_json::json!({
            "history": [
                {"role": "user", "content": "my email is a@b.com"},
                {"role": "assistant", "content": "noted"},
            ],
            "memory": "phone: 9876543210",
        });
        let clean = sanitize_context(&ctx);
        assert!(clean["history"][0]["content"].as_str().unwrap().contains("[REDACTED:EMAIL]"));
        assert_eq!(clean["history"][1]["content"], "noted");
        assert!(clean["memory"].as_str().unwrap().contains("[REDACTED:PHONE]"));
    }

    #[test]
    fn test_sanitize_context_shape_preserved() {
        let ctx = serde_json::json!({"history": [], "other": 1});
        let clean = sanitize_context(&ctx);
        assert_eq!(clean["other"], 1);
    }
}
