//! Brain client — calls the Qwen brain server for advanced intent classification.
#![allow(dead_code)]
//!
//! The brain server is a Python sidecar (like STT 39217 and NLU 39218) on port 39219.
//! It runs a Qwen2.5-0.5B-Instruct model that can:
//!   - Classify transcripts the deterministic parser + BERT-Mini miss
//!   - Generate alternative phrasings for training BERT-Mini
//!   - Cross-validate mispronunciations (e.g. "zys" → "zync" via PR number lookup)
//!   - Build a personal pronunciation map that grows over time
//!
//! Admin-only: the brain server is only started when is_admin is true.
//! Always loaded (no idle timeout) — stays in memory for instant responses.

use crate::intent_parser::{ParseResult, ParsedIntent};
use serde::Deserialize;
use std::time::Duration;

/// Brain server port (separate from STT 39217 and NLU 39218).
const BRAIN_PORT: u16 = 39219;

/// Brain server response format.
#[derive(Debug, Deserialize)]
struct BrainResponse {
    intent: String,
    slots: serde_json::Value,
    confidence: f32,
    corrected_repo: Option<String>,
    #[serde(rename = "original_text")]
    _original_text: String,
    #[serde(rename = "corrected_text")]
    _corrected_text: String,
    #[serde(rename = "latency_ms")]
    _latency_ms: f32,
}

/// Phrasing generation response.
#[derive(Debug, Deserialize)]
struct PhrasingResponse {
    phrasings: Vec<String>,
    #[serde(rename = "latency_ms")]
    _latency_ms: f32,
}

/// Pronunciation map response.
#[derive(Debug, Deserialize)]
pub struct PronunciationMapResponse {
    pub map: std::collections::HashMap<String, String>,
    pub count: usize,
}

/// Classify a transcript via the brain server.
///
/// Returns None if the server is not running or the request fails.
/// Returns Some(ParseResult) if the brain returns a valid classification.
/// Returns None if not admin or the brain server is not running.
pub async fn brain_classify(transcript: &str) -> Option<ParseResult> {
    // Runtime admin check — no-op if not admin
    if !crate::admin_config::is_admin() {
        return None;
    }

    // Ensure the brain server is running (lazy-start, admin-only)
    tokio::task::spawn_blocking(|| {
        crate::lazy_brain::ensure_brain_running();
    })
    .await
    .ok()?;

    let port = crate::admin_config::brain_port();
    let url = format!("http://127.0.0.1:{}/classify", port);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10)) // brain is slower than NLU (LLM inference)
        .build()
        .ok()?;

    let response = client
        .post(&url)
        .json(&serde_json::json!({ "text": transcript }))
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        tracing::debug!("[brain_client] server returned non-success status");
        return None;
    }

    let brain: BrainResponse = response.json().await.ok()?;

    // Convert brain response to ParsedIntent (reuse the same mapping as nlu_client)
    let intent = brain_to_parsed_intent(&brain.intent, &brain.slots)?;

    Some(ParseResult {
        intent,
        confidence: brain.confidence,
        source: "brain".to_string(),
    })
}

/// Generate alternative phrasings for an intent (for training BERT-Mini).
pub async fn brain_generate_phrasings(
    intent: &str,
    slots: &serde_json::Value,
    count: u32,
) -> Option<Vec<String>> {
    let url = format!("http://127.0.0.1:{}/generate_phrasings", BRAIN_PORT);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .ok()?;

    let response = client
        .post(&url)
        .json(&serde_json::json!({
            "intent": intent,
            "slots": slots,
            "count": count,
        }))
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let result: PhrasingResponse = response.json().await.ok()?;
    Some(result.phrasings)
}

/// Get the current pronunciation map.
pub async fn brain_get_pronunciation_map() -> Option<PronunciationMapResponse> {
    let url = format!("http://127.0.0.1:{}/pronunciation_map", BRAIN_PORT);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .ok()?;

    let response = client.get(&url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }

    response.json().await.ok()
}

/// Check if the brain server is running.
pub async fn is_brain_available() -> bool {
    let url = format!("http://127.0.0.1:{}/health", BRAIN_PORT);
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    match client.get(&url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Convert brain server response to ParsedIntent.
/// The brain uses the same 46-intent schema as the NLU server, so we
/// delegate to nlu_client::nlu_to_parsed_intent for the full mapping.
/// This ensures all 46 intents are handled, including GitHub commands
/// that the previous incomplete mapping was dropping.
fn brain_to_parsed_intent(intent: &str, slots: &serde_json::Value) -> Option<ParsedIntent> {
    // The brain server returns pr_number and other numeric slots as
    // strings sometimes (Qwen generates JSON with string values).
    // nlu_to_parsed_intent expects u64 for some fields, so we normalize
    // the slots to ensure numeric fields are numbers, not strings.
    let normalized_slots = normalize_slots(slots);
    crate::nlu_client::nlu_to_parsed_intent(intent, &normalized_slots)
}

/// Normalize slot values: convert string-encoded numbers to actual numbers
/// so that nlu_to_parsed_intent's as_u64() calls succeed.
fn normalize_slots(slots: &serde_json::Value) -> serde_json::Value {
    if let Some(obj) = slots.as_object() {
        let mut normalized = serde_json::Map::new();
        for (key, value) in obj {
            // Try to convert string numbers to actual numbers
            if let Some(s) = value.as_str() {
                if let Ok(n) = s.parse::<u64>() {
                    normalized.insert(key.clone(), serde_json::Value::Number(n.into()));
                    continue;
                }
                if let Ok(f) = s.parse::<f64>() {
                    if let Some(num) = serde_json::Number::from_f64(f) {
                        normalized.insert(key.clone(), serde_json::Value::Number(num));
                        continue;
                    }
                }
            }
            normalized.insert(key.clone(), value.clone());
        }
        serde_json::Value::Object(normalized)
    } else {
        slots.clone()
    }
}
