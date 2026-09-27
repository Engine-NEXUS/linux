//! 9Router — Local AI gateway that routes reasoning requests directly
//! to free cloud providers (Groq, Gemini, Cerebras), bypassing the Worker
//! for general AI questions. The Worker still handles identity, OAuth,
//! PR analysis, and deep analysis — but general Q&A goes through 9Router
//! for 3-7x lower latency.
//!
//! ## Architecture
//!
//! ```text
//! Current (Worker path):
//!   Device → Worker (50ms) → Workers AI (500-2000ms) → Worker (50ms) → Device
//!   Total: 600-2100ms
//!
//! With 9Router:
//!   Device → localhost (1ms) → Groq (120ms) → localhost (1ms) → Device
//!   Total: ~242ms  (3-7x faster)
//! ```
//!
//! ## Provider Cascade
//!
//! ```text
//! Groq (14,400 req/day free, ~120ms, Llama 3.3 70B)
//!   → Gemini (1,500 req/day free, ~400ms, flash-lite)
//!     → Cerebras (1M tokens/day free, ~80ms, Llama 3.3 70B)
//!       → Worker (fallback, uses neurons)
//! ```
//!
//! 9Router tries the fastest free provider first. If it fails (rate
//! limit, network, auth), it falls back to the next. If all providers
//! fail, the caller falls back to the Worker (old path).
//!
//! ## What 9Router Handles
//!
//! - General questions ("what's the capital of France?")
//! - Factual queries ("how tall is the Eiffel Tower?")
//! - Simple reasoning ("is 42 divisible by 7?")
//! - Conversational responses (when NLU confidence is low)
//!
//! ## What Still Goes Through the Worker
//!
//! - PR analysis (needs GitHub token + GLM models on Worker)
//! - Deep analysis (needs GLM-5.3 on Worker)
//! - Research (needs Wikipedia/Wikidata retrieval on Worker)
//! - Any task requiring the user's session/OAuth tokens

use std::time::Duration;

use serde::Serialize;
use tauri::Manager;

// ─── Provider Configuration ────────────────────────────────────────────

/// Free cloud AI providers that 9Router can route to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// Groq — fastest free provider. gpt-oss-120b, ~120ms, 1K req/day/model.
    Groq,
    /// Google Gemini — flash-lite, ~400ms, 1,500 req/day.
    Gemini,
    /// Cerebras — fastest inference when keyed, but free is now a
    /// card-gated 30-day trial (demoted to last resort, key-only).
    Cerebras,
    /// Worker fallback — uses Workers AI neurons (~500ms, ~100 neurons).
    Worker,
}

impl Provider {
    /// Human-readable name for logging and UI.
    pub fn name(&self) -> &'static str {
        match self {
            Provider::Groq => "Groq",
            Provider::Gemini => "Gemini",
            Provider::Cerebras => "Cerebras",
            Provider::Worker => "Worker",
        }
    }
}

/// Model IDs for each provider (OpenAI-compatible endpoints use model
/// names; Gemini uses its own naming scheme).
///
/// Sept 2026 refresh: Groq retired `llama-3.3-70b-versatile` (HTTP 404) and
/// dropped Llama from the free plan — free tier is now gpt-oss + Qwen.
/// Cerebras narrowed to gpt-oss-120b/GLM and moved free to a card trial.
/// Multiple Groq IDs are tried in order (quota multiplies across models;
/// 404s fall through harmlessly).
const GROQ_MODELS: &[&str] = &[
    "openai/gpt-oss-120b",
    "openai/gpt-oss-20b",
    "qwen/qwen3-32b",
];
const GEMINI_MODEL: &str = "gemini-flash-lite-latest";
const CEREBRAS_MODEL: &str = "gpt-oss-120b";

/// API endpoints for each provider.
const GROQ_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const GEMINI_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-flash-lite-latest:generateContent";
const CEREBRAS_URL: &str = "https://api.cerebras.ai/v1/chat/completions";

// ─── Response Types ────────────────────────────────────────────────────

/// The result of a 9Router AI request.
#[derive(Debug, Clone, Serialize)]
pub struct RouterResponse {
    /// The AI-generated text response.
    pub text: String,
    /// Which provider handled the request.
    pub provider: Provider,
    /// Model name used by the provider.
    pub model: String,
    /// Round-trip latency in milliseconds.
    pub latency_ms: u64,
}

/// API keys for all providers. Read from NEXUS settings.
#[derive(Debug, Clone, Default)]
pub struct ProviderKeys {
    pub groq: String,
    pub gemini: String,
    pub cerebras: String,
}

// ─── System Prompt ─────────────────────────────────────────────────────

/// The system prompt for 9Router responses. Keeps answers concise and
/// conversational — NEXUS speaks these aloud via TTS.
const SYSTEM_PROMPT: &str = concat!(
    "You are NEXUS, a concise voice assistant. ",
    "Answer in 1-3 sentences. Be direct and helpful. ",
    "Do not use markdown, headers, or bullet points — the response is spoken aloud. ",
    "If the user asks a follow-up, maintain conversational context."
);

// ─── Public API ─────────────────────────────────────────────────────────

/// Try to answer a general question using 9Router (local → free cloud
/// providers). Returns `None` if all providers fail — the caller should
/// fall back to the Worker in that case.
///
/// # Arguments
/// * `transcript` - The user's question or request
/// * `dialog_context` - Optional prior conversation turns (JSON)
/// * `keys` - API keys for each provider
/// * `client` - Reused reqwest client (avoids per-call Client::build)
///
/// # Cascade (Sept 2026 order — speed × free quota, demote card-gated)
/// Groq (3 model IDs) → Gemini → Cerebras (key-only, trial) → None
/// (caller falls back to Worker)
pub async fn route_question(
    transcript: &str,
    dialog_context: Option<&serde_json::Value>,
    keys: &ProviderKeys,
    client: &reqwest::Client,
) -> Option<RouterResponse> {
    // Build the prompt with optional dialog context
    let prompt = build_prompt(transcript, dialog_context);

    // Try each provider in order of speed/reliability
    let providers: &[(Provider, &str)] = &[
        (Provider::Groq, &keys.groq),
        (Provider::Gemini, &keys.gemini),
        (Provider::Cerebras, &keys.cerebras),
    ];
    debug_assert_eq!(
        providers.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
        provider_order()
    );

    for (provider, key) in providers {
        if key.is_empty() {
            tracing::debug!("9router: skipping {} (no API key)", provider.name());
            continue;
        }

        tracing::info!("9router: trying {} for: {:?}...", provider.name(), truncate(transcript, 60));

        match call_provider(*provider, &prompt, key, client).await {
            Ok(resp) => {
                tracing::info!(
                    "9router: {} answered in {}ms",
                    provider.name(),
                    resp.latency_ms
                );
                return Some(resp);
            }
            Err(e) => {
                tracing::warn!("9router: {} failed: {}", provider.name(), e);
                // Try next provider
            }
        }
    }

    tracing::warn!("9router: all providers failed, caller should fall back to Worker");
    None
}

// ─── Provider Calls ─────────────────────────────────────────────────────

/// Call a specific provider's API. Each provider has a slightly different
/// request format, but they all return text.
async fn call_provider(
    provider: Provider,
    prompt: &str,
    api_key: &str,
    client: &reqwest::Client,
) -> Result<RouterResponse, String> {
    let start = std::time::Instant::now();

    let (text, model) = match provider {
        Provider::Groq => {
            // Rotate across free model IDs: multiplies the per-model daily
            // quota and survives silent model retirements (404s fall through).
            let mut last_err = "no Groq models configured".to_string();
            let mut result: Option<(String, String)> = None;
            for model in GROQ_MODELS {
                match call_openai_compatible(GROQ_URL, model, prompt, api_key, client).await {
                    Ok(ok) => {
                        result = Some(ok);
                        break;
                    }
                    Err(e) => {
                        tracing::warn!("9router: Groq model {} failed: {}", model, e);
                        last_err = e;
                    }
                }
            }
            result.ok_or(last_err)?
        }
        Provider::Cerebras => {
            call_openai_compatible(CEREBRAS_URL, CEREBRAS_MODEL, prompt, api_key, client).await?
        }
        Provider::Gemini => call_gemini(prompt, api_key, client).await?,
        Provider::Worker => {
            // Worker is not called directly by 9Router — it's the fallback
            // handled by the caller. This branch should never execute.
            return Err("Worker is a fallback, not a 9Router provider".into());
        }
    };

    let latency_ms = start.elapsed().as_millis() as u64;

    Ok(RouterResponse {
        text,
        provider,
        model,
        latency_ms,
    })
}

/// Call an OpenAI-compatible chat completions endpoint (Groq, Cerebras).
/// Both use the same request/response format.
async fn call_openai_compatible(
    url: &str,
    model: &str,
    prompt: &str,
    api_key: &str,
    client: &reqwest::Client,
) -> Result<(String, String), String> {
    let messages = serde_json::json!([
        { "role": "system", "content": SYSTEM_PROMPT },
        { "role": "user", "content": prompt },
    ]);

    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "max_tokens": 500,
        "temperature": 0.3,
    });

    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if status.as_u16() == 429 {
            return Err("rate limit hit".into());
        }
        if status.as_u16() == 401 {
            return Err("invalid API key".into());
        }
        return Err(format!("HTTP {}: {}", status, truncate(&body, 200)));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("response parse error: {}", e))?;

    let text = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    if text.is_empty() {
        return Err("empty response".into());
    }

    Ok((text, model.to_string()))
}

/// Call Google Gemini's generateContent endpoint. Gemini uses a different
/// request format than OpenAI-compatible APIs.
async fn call_gemini(
    prompt: &str,
    api_key: &str,
    client: &reqwest::Client,
) -> Result<(String, String), String> {
    let url = format!("{}?key={}", GEMINI_URL, api_key);

    let body = serde_json::json!({
        "contents": [
            {
                "role": "user",
                "parts": [{ "text": format!("{}\n\n{}", SYSTEM_PROMPT, prompt) }],
            }
        ],
        "generationConfig": {
            "maxOutputTokens": 700,
            "temperature": 0.3,
        },
    });

    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if status.as_u16() == 429 {
            return Err("rate limit hit".into());
        }
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err("invalid API key".into());
        }
        return Err(format!("HTTP {}: {}", status, truncate(&body, 200)));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("response parse error: {}", e))?;

    let text = json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    if text.is_empty() {
        return Err("empty response".into());
    }

    Ok((text, GEMINI_MODEL.to_string()))
}

// ─── Helpers ────────────────────────────────────────────────────────────

/// Build the prompt for the AI provider. If dialog context exists,
/// prepend prior turns so the model has conversational continuity.
fn build_prompt(transcript: &str, dialog_context: Option<&serde_json::Value>) -> String {
    let Some(ctx) = dialog_context else {
        return transcript.to_string();
    };

    // Extract prior user/assistant turns from the dialog context.
    // The Worker expects: { "history": [{ "role": "user", "content": "..." }, ...] }
    let history = ctx
        .get("history")
        .and_then(|h| h.as_array())
        .cloned()
        .unwrap_or_default();

    if history.is_empty() {
        return transcript.to_string();
    }

    // Build a simple conversation transcript for the model.
    // We keep this short — most providers have token limits and TTS
    // needs concise responses. Take the LAST 6 turns (most recent context).
    let mut parts = Vec::new();
    let start = if history.len() > 6 { history.len() - 6 } else { 0 };
    for turn in history.iter().skip(start) {
        let role = turn.get("role").and_then(|r| r.as_str()).unwrap_or("");
        let content = turn.get("content").and_then(|c| c.as_str()).unwrap_or("");
        if !content.is_empty() {
            match role {
                "user" => parts.push(format!("User: {}", content)),
                "assistant" | "model" => parts.push(format!("NEXUS: {}", content)),
                _ => {}
            }
        }
    }
    parts.push(format!("User: {}", transcript));

    parts.join("\n")
}

/// Truncate a string to `max` chars, appending "..." if truncated.
/// Used for logging — keeps log lines readable.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

/// Public version of `truncate` for use by other modules (e.g. orchestrator
/// logging). See `truncate` for behavior.
pub fn truncate_pub(s: &str, max: usize) -> String {
    truncate(s, max)
}

/// Cascade order, fastest-free-first (Sept 2026: Groq → Gemini → Cerebras).
/// Split out for unit-testing (route_question asserts it stays in sync).
pub fn provider_order() -> [Provider; 3] {
    [Provider::Groq, Provider::Gemini, Provider::Cerebras]
}

/// Which of `wanted` model IDs are MISSING from a provider's `/models` menu
/// response body. Pure function (unit-tested) — the core of the health probe.
pub fn missing_models(menu_json: &str, wanted: &[&str]) -> Vec<String> {
    wanted
        .iter()
        .filter(|m| !menu_json.contains(**m))
        .map(|m| m.to_string())
        .collect()
}

/// Probe configured provider `/models` menus at startup (non-blocking).
/// Free-tier model IDs die silently (Groq llama 404, Sept 2026) — this logs
/// which lanes are actually alive so ops notices before users do.
/// Pure HTTP menu checks; costs no inference quota.
pub async fn probe_provider_health<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let keys = read_provider_keys(app);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    if !keys.groq.is_empty() {
        match client
            .get("https://api.groq.com/openai/v1/models")
            .bearer_auth(&keys.groq)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.text().await.unwrap_or_default();
                let missing = missing_models(&body, GROQ_MODELS);
                if missing.is_empty() {
                    tracing::info!("9router health: groq OK (all {} models listed)", GROQ_MODELS.len());
                } else {
                    tracing::warn!(
                        "9router health: groq alive but models missing from menu: {:?} — cascade will skip them",
                        missing
                    );
                }
            }
            Ok(resp) => tracing::warn!("9router health: groq menu HTTP {}", resp.status()),
            Err(e) => tracing::warn!("9router health: groq unreachable: {}", e),
        }
    }

    if !keys.gemini.is_empty() {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models?key={}",
            keys.gemini
        );
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                tracing::info!("9router health: gemini OK");
            }
            Ok(resp) => tracing::warn!("9router health: gemini HTTP {}", resp.status()),
            Err(e) => tracing::warn!("9router health: gemini unreachable: {}", e),
        }
    }

    if !keys.cerebras.is_empty() {
        match client
            .get("https://api.cerebras.ai/v1/models")
            .bearer_auth(&keys.cerebras)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.text().await.unwrap_or_default();
                if body.contains(CEREBRAS_MODEL) {
                    tracing::info!("9router health: cerebras OK ({})", CEREBRAS_MODEL);
                } else {
                    tracing::warn!(
                        "9router health: cerebras alive but {} not in menu — will fall through",
                        CEREBRAS_MODEL
                    );
                }
            }
            Ok(resp) => tracing::warn!("9router health: cerebras HTTP {}", resp.status()),
            Err(e) => tracing::warn!("9router health: cerebras unreachable: {}", e),
        }
    }
}

/// Read all provider API keys from NEXUS settings.
/// Returns a `ProviderKeys` struct with all available keys.
pub fn read_provider_keys<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> ProviderKeys {
    let dir = app.path().app_data_dir();
    let Ok(dir) = dir else {
        return ProviderKeys::default();
    };
    let path: std::path::PathBuf = dir.join("settings.json");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return ProviderKeys::default();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return ProviderKeys::default();
    };

    // Read each key with camelCase → snake_case fallback (same pattern as
    // read_groq_api_key in commands.rs).
    let read_key = |camel: &str, snake: &str| -> String {
        json.get(camel)
            .or_else(|| json.get(snake))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };

    ProviderKeys {
        groq: read_key("groqApiKey", "groq_api_key"),
        gemini: read_key("geminiApiKey", "gemini_api_key"),
        cerebras: read_key("cerebrasApiKey", "cerebras_api_key"),
    }
}

/// Check if 9Router can handle a given request. Some task types must
/// still go through the Worker (PR analysis, deep analysis, research).
///
/// Returns `true` if 9Router should try, `false` if the Worker must
/// handle it.
pub fn can_route(transcript: &str) -> bool {
    let lower = transcript.to_lowercase();

    // Tasks that MUST go through the Worker (need GitHub token, GLM
    // models, or search infrastructure).
    let worker_only_patterns = [
        "analyse pr", "analyze pr", "analyse latest pr", "analyze latest pr",
        "analyse repo", "analyze repo", "architect",
        "check branch", "merge pr", "approve pr", "close pr",
        "github", "pull request", "pullrequest",
    ];

    for pattern in &worker_only_patterns {
        if lower.contains(pattern) {
            tracing::debug!("9router: routing to Worker (matched '{}')", pattern);
            return false;
        }
    }

    // Everything else can try 9Router first.
    true
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_route_general_question() {
        assert!(can_route("what is the capital of france"));
        assert!(can_route("how tall is the eiffel tower"));
        assert!(can_route("tell me a joke"));
    }

    #[test]
    fn test_cannot_route_pr_analysis() {
        assert!(!can_route("analyse pr 42"));
        assert!(!can_route("analyze pr 42"));
        assert!(!can_route("analyse latest pr"));
        assert!(!can_route("merge pr 42"));
    }

    #[test]
    fn test_cannot_route_github_commands() {
        assert!(!can_route("check branch main"));
        assert!(!can_route("open architect"));
        assert!(!can_route("github list prs"));
    }

    #[test]
    fn test_can_route_local_commands_pass_through() {
        // These won't actually reach 9Router (they're handled by the
        // deterministic parser first), but can_route should not block them.
        assert!(can_route("open chrome"));
        assert!(can_route("pause music"));
    }

    #[test]
    fn test_build_prompt_no_context() {
        let prompt = build_prompt("what is rust", None);
        assert_eq!(prompt, "what is rust");
    }

    #[test]
    fn test_build_prompt_with_context() {
        let ctx = serde_json::json!({
            "history": [
                { "role": "user", "content": "what is rust" },
                { "role": "assistant", "content": "Rust is a systems language." },
            ]
        });
        let prompt = build_prompt("tell me more", Some(&ctx));
        assert!(prompt.contains("User: what is rust"));
        assert!(prompt.contains("NEXUS: Rust is a systems language."));
        assert!(prompt.contains("User: tell me more"));
    }

    #[test]
    fn test_build_prompt_empty_history() {
        let ctx = serde_json::json!({ "history": [] });
        let prompt = build_prompt("hello", Some(&ctx));
        assert_eq!(prompt, "hello");
    }

    #[test]
    fn test_build_prompt_truncates_long_history() {
        // Build a history with 10 turns — only 6 should be included
        let mut history = Vec::new();
        for i in 0..10 {
            history.push(serde_json::json!({
                "role": "user",
                "content": format!("message {}", i),
            }));
        }
        let ctx = serde_json::json!({ "history": history });
        let prompt = build_prompt("latest", Some(&ctx));
        // Should contain messages 4-9 (last 6) plus "latest"
        assert!(prompt.contains("message 4"));
        assert!(prompt.contains("message 9"));
        assert!(!prompt.contains("message 3"));
        assert!(prompt.contains("User: latest"));
    }

    #[test]
    fn test_truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long_string() {
        let result = truncate("this is a very long string", 10);
        assert_eq!(result, "this is a ...");
    }

    #[test]
    fn test_provider_name() {
        assert_eq!(Provider::Groq.name(), "Groq");
        assert_eq!(Provider::Gemini.name(), "Gemini");
        assert_eq!(Provider::Cerebras.name(), "Cerebras");
        assert_eq!(Provider::Worker.name(), "Worker");
    }

    #[test]
    fn test_provider_keys_default_empty() {
        let keys = ProviderKeys::default();
        assert!(keys.groq.is_empty());
        assert!(keys.gemini.is_empty());
        assert!(keys.cerebras.is_empty());
    }

    // ─── Sept 2026 cascade refresh tests ─────────────────────────────

    #[test]
    fn test_provider_order_groq_first_cerebras_last() {
        // Groq (free, high quota) → Gemini → Cerebras (card trial, last resort).
        assert_eq!(
            provider_order(),
            [Provider::Groq, Provider::Gemini, Provider::Cerebras]
        );
    }

    #[test]
    fn test_groq_models_no_dead_llama() {
        // llama-3.3-70b-versatile 404'd in Sept 2026 — must never return.
        assert!(!GROQ_MODELS.contains(&"llama-3.3-70b-versatile"));
        assert!(GROQ_MODELS.contains(&"openai/gpt-oss-120b"));
        assert!(GROQ_MODELS.len() >= 2, "need fallback IDs for quota rotation");
    }

    #[test]
    fn test_cerebras_model_current() {
        assert_eq!(CEREBRAS_MODEL, "gpt-oss-120b");
    }

    #[test]
    fn test_missing_models_detects_dead_ids() {
        let menu = r#"{"data": [{"id": "openai/gpt-oss-120b"}, {"id": "qwen/qwen3-32b"}]}"#;
        let missing = missing_models(menu, GROQ_MODELS);
        assert_eq!(missing, vec!["openai/gpt-oss-20b".to_string()]);
        let full = r#"{"data": [{"id": "openai/gpt-oss-120b"}, {"id": "openai/gpt-oss-20b"}, {"id": "qwen/qwen3-32b"}]}"#;
        assert!(missing_models(full, GROQ_MODELS).is_empty());
    }

    // Note: route_question() requires network access and API keys, so it
    // is not unit-tested here. Integration testing is done manually by
    // running NEXUS with real API keys and asking a general question.
}
