//! Tests for the new offline local commands (close_app, whatsapp_chat).
//! Run with: cargo test --lib offline_commands

use nexus_lib::intent_parser::{parse_deterministic, ParsedIntent};

#[test]
fn test_close_app_chrome() {
    let result = parse_deterministic("close chrome");
    assert!(result.is_some(), "should parse 'close chrome'");
    let intent = result.unwrap();
    assert!(matches!(
        &intent.intent,
        ParsedIntent::CloseApp { target } if target == "chrome"
    ));
    assert_eq!(intent.confidence, 1.0);
    assert_eq!(intent.source, "deterministic");
}

#[test]
fn test_close_app_quit_notepad() {
    let result = parse_deterministic("quit notepad");
    assert!(result.is_some());
    assert!(matches!(
        &result.unwrap().intent,
        ParsedIntent::CloseApp { target } if target == "notepad"
    ));
}

#[test]
fn test_close_app_exit_whatsapp() {
    let result = parse_deterministic("exit whatsapp");
    assert!(result.is_some());
    assert!(matches!(
        &result.unwrap().intent,
        ParsedIntent::CloseApp { target } if target == "whatsapp"
    ));
}

#[test]
fn test_close_app_does_not_close_nexus() {
    let result = parse_deterministic("close nexus");
    // Should NOT match CloseApp — "nexus" is protected
    if let Some(r) = result {
        assert!(!matches!(r.intent, ParsedIntent::CloseApp { .. }));
    }
}

#[test]
fn test_whatsapp_chat_with_lakshya() {
    let result = parse_deterministic("open chat with lakshya");
    assert!(result.is_some(), "should parse 'open chat with lakshya'");
    let intent = result.unwrap();
    assert!(matches!(
        &intent.intent,
        ParsedIntent::WhatsappChat { contact } if contact == "lakshya"
    ));
}

#[test]
fn test_whatsapp_chat_with_mom() {
    let result = parse_deterministic("chat with mom");
    assert!(result.is_some());
    assert!(matches!(
        &result.unwrap().intent,
        ParsedIntent::WhatsappChat { contact } if contact == "mom"
    ));
}

#[test]
fn test_whatsapp_message_lakshya() {
    let result = parse_deterministic("message lakshya");
    assert!(result.is_some());
    assert!(matches!(
        &result.unwrap().intent,
        ParsedIntent::WhatsappChat { contact } if contact == "lakshya"
    ));
}

#[test]
fn test_whatsapp_open_my_chat_with_lakshya() {
    let result = parse_deterministic("open my chat with lakshya");
    assert!(result.is_some());
    assert!(matches!(
        &result.unwrap().intent,
        ParsedIntent::WhatsappChat { contact } if contact == "lakshya"
    ));
}

#[test]
fn test_whatsapp_chat_with_lakshya_on_whatsapp() {
    let result = parse_deterministic("chat with lakshya on whatsapp");
    assert!(result.is_some());
    assert!(matches!(
        &result.unwrap().intent,
        ParsedIntent::WhatsappChat { contact } if contact == "lakshya"
    ));
}

#[test]
fn test_whatsapp_send_message_to_lakshya() {
    // "send message to lakshya" has no message body — the MCP send parser
    // (Pattern 3 partial-send) asks what to say instead of opening the chat.
    // Handled downstream: orchestrator speaks the prompt, command_executor
    // returns it, and the reply completes SendWhatsAppMessage.
    let result = parse_deterministic("send message to lakshya");
    assert!(result.is_some());
    assert!(matches!(
        &result.unwrap().intent,
        ParsedIntent::NeedMoreInfo { prompt } if prompt == "What should I say to lakshya?"
    ));
}
