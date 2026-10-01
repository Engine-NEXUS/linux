//! Screen vision Q&A — Phase 1 of the clicky-style screen agent (Route C).
//!
//! Pipeline: screenshot JPEG (`screen.rs`) → Gemini free (per-user key,
//! thinking off) → Groq vision fallback → structured `{speak, point?}`
//! answer. `point` coordinates (0-1000 normalized) are parsed and
//! validated now; the Phase-2 overlay window will render them.

use std::time::Duration;

use crate::router::ProviderKeys;

const GEMINI_VISION_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models/gemini-flash-lite-latest:generateContent";
const GROQ_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
/// Vision-capable Groq IDs, tried in order (quota multiplies across IDs).
const GROQ_VISION_MODELS: &[&str] = &[
    "qwen/qwen3.6-27b",
    "meta-llama/llama-4-scout-17b-16e-instruct",
];

/// What the user asked about the screen.
#[derive(Debug, Clone)]
pub enum ScreenVisionKind {
    Describe,
    Locate(String),
    ReadText,
}

/// Point in 0-1000 normalized image coordinates (Phase-2 overlay input).
#[derive(Debug, Clone, PartialEq)]
pub struct PointNorm {
    pub x: u16,
    pub y: u16,
    pub label: String,
}

/// Structured model answer: spoken text + optional point target.
#[derive(Debug, Clone, PartialEq)]
pub struct VisionAnswer {
    pub speak: String,
    pub point: Option<PointNorm>,
}

const SYSTEM: &str = concat!(
    "You are NEXUS screen vision. Look at the attached screenshot and answer ",
    "the user's question in 1-3 short spoken sentences. No markdown, no bullet ",
    "points — the reply is spoken aloud. Reply with ONLY a JSON object: ",
    "{\"speak\": \"...\", \"point\": null}. When the user asks where something ",
    "is, set \"point\" to {\"x\": 0-1000, \"y\": 0-1000, \"label\": \"short name\"} ",
    "with the target's center in 0-1000 normalized image coordinates; otherwise ",
    "leave \"point\" null."
);

/// User question per vision kind.
pub fn question_for(kind: &ScreenVisionKind) -> String {
    match kind {
        ScreenVisionKind::Describe => "Describe what is visible on this screen.".to_string(),
        ScreenVisionKind::ReadText => {
            "Read aloud the important visible text on this screen, top to bottom.".to_string()
        }
        ScreenVisionKind::Locate(target) => {
            format!("Where is '{target}' on this screen? Name it in speak and give its center coordinates in point.")
        }
    }
}

/// Lenient parse: strip code fences, accept the `{speak, point?}` contract,
/// fall back to the raw text as speech when the model ignores the schema.
pub fn parse_answer(text: &str) -> VisionAnswer {
    let t = strip_fences(text);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
        let speak = v
            .get("speak")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let point = v.get("point").and_then(|p| {
            let x = p.get("x")?.as_u64()?;
            let y = p.get("y")?.as_u64()?;
            if x > 1000 || y > 1000 {
                return None;
            }
            Some(PointNorm {
                x: x as u16,
                y: y as u16,
                label: p
                    .get("label")
                    .and_then(|l| l.as_str())
                    .unwrap_or("")
                    .chars()
                    .take(80)
                    .collect(),
            })
        });
        if !speak.is_empty() {
            return VisionAnswer { speak, point };
        }
    }
    VisionAnswer {
        speak: text.trim().chars().take(600).collect(),
        point: None,
    }
}

fn strip_fences(text: &str) -> &str {
    let t = text.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t);
    t.trim()
}

/// Ask about a screenshot: Gemini first, Groq vision on failure.
pub async fn ask_about_screen(
    jpeg: &[u8],
    kind: &ScreenVisionKind,
    keys: &ProviderKeys,
) -> Result<VisionAnswer, String> {
    if jpeg.is_empty() {
        return Err("empty screenshot".into());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    if !keys.gemini.is_empty() {
        match call_gemini_vision(&client, &keys.gemini, jpeg, kind).await {
            Ok(a) => return Ok(a),
            Err(e) => tracing::warn!("screen vision: gemini failed: {e}"),
        }
    }
    if !keys.groq.is_empty() {
        match call_groq_vision(&client, &keys.groq, jpeg, kind).await {
            Ok(a) => return Ok(a),
            Err(e) => tracing::warn!("screen vision: groq failed: {e}"),
        }
    }
    Err("no vision provider available — add a Gemini API key in Settings".into())
}

async fn call_gemini_vision(
    client: &reqwest::Client,
    api_key: &str,
    jpeg: &[u8],
    kind: &ScreenVisionKind,
) -> Result<VisionAnswer, String> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(jpeg);
    let question = question_for(kind);
    let body = serde_json::json!({
        "contents": [{
            "role": "user",
            "parts": [
                { "text": format!("{SYSTEM}\n\n{question}") },
                { "inline_data": { "mime_type": "image/jpeg", "data": b64 } },
            ],
        }],
        "generationConfig": {
            "maxOutputTokens": 500,
            "temperature": 0.2,
            "responseMimeType": "application/json",
        },
    });
    let resp = client
        .post(format!("{GEMINI_VISION_URL}?key={api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
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
        .map_err(|e| format!("response parse error: {e}"))?;
    // Concatenate all text parts (some models split the JSON across parts).
    let mut text = String::new();
    if let Some(parts) = json["candidates"][0]["content"]["parts"].as_array() {
        for p in parts {
            if let Some(s) = p.get("text").and_then(|t| t.as_str()) {
                text.push_str(s);
            }
        }
    }
    let text = text.trim();
    if text.is_empty() {
        return Err("empty response".into());
    }
    Ok(parse_answer(text))
}

async fn call_groq_vision(
    client: &reqwest::Client,
    api_key: &str,
    jpeg: &[u8],
    kind: &ScreenVisionKind,
) -> Result<VisionAnswer, String> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(jpeg);
    let question = question_for(kind);
    let mut last_err = "no vision model answered".to_string();
    for model in GROQ_VISION_MODELS {
        let body = serde_json::json!({
            "model": model,
            "messages": [
                { "role": "system", "content": SYSTEM },
                { "role": "user", "content": [
                    { "type": "text", "text": question },
                    { "type": "image_url", "image_url": { "url": format!("data:image/jpeg;base64,{b64}") } },
                ]},
            ],
            "max_tokens": 500,
            "temperature": 0.2,
            "response_format": { "type": "json_object" },
        });
        let resp = match client
            .post(GROQ_URL)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("request failed: {e}");
                continue;
            }
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            last_err = if status.as_u16() == 429 {
                "rate limit hit".into()
            } else if status.as_u16() == 401 {
                "invalid API key".into()
            } else {
                format!("HTTP {}: {}", status, truncate(&body, 200))
            };
            continue;
        }
        let json: serde_json::Value = match resp.json().await {
            Ok(j) => j,
            Err(e) => {
                last_err = format!("response parse error: {e}");
                continue;
            }
        };
        let text = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();
        if text.is_empty() {
            last_err = "empty response".into();
            continue;
        }
        return Ok(parse_answer(&text));
    }
    Err(last_err)
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    format!("{}...", &s[..n])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_contract_with_point() {
        let a = parse_answer(
            r#"{"speak": "The Save button is top right.", "point": {"x": 850, "y": 120, "label": "Save"}}"#,
        );
        assert_eq!(a.speak, "The Save button is top right.");
        assert_eq!(
            a.point,
            Some(PointNorm { x: 850, y: 120, label: "Save".into() })
        );
    }

    #[test]
    fn parse_strips_code_fences() {
        let a = parse_answer("```json\n{\"speak\": \"Hi.\", \"point\": null}\n```");
        assert_eq!(a.speak, "Hi.");
        assert!(a.point.is_none());
    }

    #[test]
    fn parse_drops_out_of_range_point() {
        let a = parse_answer(r#"{"speak": "There.", "point": {"x": 5000, "y": 10, "label": "x"}}"#);
        assert_eq!(a.speak, "There.");
        assert!(a.point.is_none());
    }

    #[test]
    fn parse_falls_back_to_raw_text() {
        let a = parse_answer("I see a browser window with three tabs.");
        assert_eq!(a.speak, "I see a browser window with three tabs.");
        assert!(a.point.is_none());
    }

    #[test]
    fn question_for_locate_names_target() {
        let q = question_for(&ScreenVisionKind::Locate("save button".into()));
        assert!(q.contains("save button"));
        assert!(q.contains("point"));
    }

    #[test]
    fn groq_vision_models_not_empty() {
        assert!(!GROQ_VISION_MODELS.is_empty());
    }
}
