//! Groq Cloud STT — Whisper Large v3 Turbo via Groq API.
//!
//! Free tier: 2,000 requests/day, 28,800 audio seconds/day per user.
//! No credit card required. Sign up at console.groq.com.
//!
//! Latency: ~247ms (batch, LPU-accelerated)
//! RAM: 0 MB (cloud, no local model)
//! Cost: $0 on free tier, $0.04/hr on paid tier

use reqwest::Client;

const GROQ_STT_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";
const GROQ_MODEL: &str = "whisper-large-v3-turbo";

/// Domain-vocabulary decoder bias for every NEXUS command transcription.
///
/// Whisper's decoder bets on words it knows: "cervix" (millions of prior
/// sightings) beats "servx" (zero) on identical acoustics. Sending this as
/// the `prompt` field seeds the decoder with our world, flipping those
/// bets — same audio, "servx" wins. A nudge, not a command: clearly-spoken
/// other words still transcribe normally.
///
/// Rules honored: command-styled sentences (the decoder continues prompt
/// style, so prompts read like NEXUS commands), entities + verbs +
/// user phrasing openers, well under Whisper's ~244-token prompt cap.
/// The alias map (`canonical_repo_name`) and heard-text NLU rows stay as
/// the downstream safety net — prompt biasing reduces errors, it never
/// eliminates them.
pub const NEXUS_VOCABULARY: &str = "Analyse the Servx repo. Show me the pull requests in Zync. Check Eesha's PR in Servx. List all open pull requests and tell me about them. Message Prem on WhatsApp. Merge the pull request. NEXUS GitHub Supabase Tailscale Ollama Qwen GLM n8n Meet Congi Shopkart Ledger Lakshya architect workflow release branch";

/// Transcribe audio using Groq's Whisper Large v3 Turbo model.
///
/// Sends WAV audio (16kHz, mono, 16-bit) to Groq's OpenAI-compatible API.
/// Returns the transcript text on success, or an error string on failure.
///
/// # Arguments
/// * `samples` - Raw i16 PCM samples at 16kHz mono
/// * `api_key` - User's Groq API key (starts with "gsk_")
/// * `client` - Reused reqwest client (avoids per-call Client::build)
/// * `prompt` - Optional decoder-bias hint (Whisper `prompt` field). Used by
///   the wake-word verifier to prefer wake-word readings of ambiguous audio.
pub async fn transcribe_with_groq(
    samples: &[i16],
    api_key: &str,
    client: &Client,
    prompt: Option<&str>,
) -> Result<String, String> {
    if api_key.is_empty() {
        return Err("No Groq API key provided".to_string());
    }

    let wav_bytes = pcm_to_wav(samples, 16000);
    tracing::info!(
        "stt-groq: sending {} bytes ({}ms audio) to Groq",
        wav_bytes.len(),
        samples.len() / 16
    );

    let part = reqwest::multipart::Part::bytes(wav_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("MIME error: {}", e))?;

    let mut form = reqwest::multipart::Form::new()
        .text("model", GROQ_MODEL.to_string())
        .text("language", "en".to_string())
        .text("response_format", "json".to_string())
        .part("file", part);
    if let Some(p) = prompt {
        form = form.text("prompt", p.to_string());
    }

    let start = std::time::Instant::now();

    let resp = client
        .post(GROQ_STT_URL)
        .bearer_auth(api_key)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Groq STT request failed: {}", e))?;

    let elapsed = start.elapsed();

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        // Check for rate limit (429) or auth error (401) for better error messages
        if status.as_u16() == 429 {
            return Err("Groq rate limit hit (2,000 req/day free tier)".to_string());
        }
        if status.as_u16() == 401 {
            return Err("Groq API key invalid — check Settings".to_string());
        }
        return Err(format!("Groq STT error {}: {}", status, body));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Groq response parse error: {}", e))?;

    let text = json
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    tracing::info!("stt-groq: transcript '{}' in {}ms", text, elapsed.as_millis());

    Ok(text)
}

/// Transcribe pre-encoded audio bytes (e.g. a Telegram voice note in Opus/OGG)
/// via Groq's OpenAI-compatible API. Same model/parse contract as
/// [`transcribe_with_groq`]; the caller supplies filename + MIME.
pub async fn transcribe_bytes_with_groq(
    audio: &[u8],
    filename: &str,
    mime: &str,
    api_key: &str,
    client: &Client,
) -> Result<String, String> {
    if api_key.is_empty() {
        return Err("No Groq API key provided".to_string());
    }
    if audio.is_empty() {
        return Err("Empty audio".to_string());
    }

    let part = reqwest::multipart::Part::bytes(audio.to_vec())
        .file_name(filename.to_string())
        .mime_str(mime)
        .map_err(|e| format!("MIME error: {}", e))?;

    let form = reqwest::multipart::Form::new()
        .text("model", GROQ_MODEL.to_string())
        .text("language", "en".to_string())
        .text("response_format", "json".to_string())
        .part("file", part);

    let start = std::time::Instant::now();

    let resp = client
        .post(GROQ_STT_URL)
        .bearer_auth(api_key)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Groq STT request failed: {}", e))?;

    let elapsed = start.elapsed();

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if status.as_u16() == 429 {
            return Err("Groq rate limit hit (2,000 req/day free tier)".to_string());
        }
        if status.as_u16() == 401 {
            return Err("Groq API key invalid — check Settings".to_string());
        }
        return Err(format!("Groq STT error {}: {}", status, body));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Groq response parse error: {}", e))?;

    let text = json
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    tracing::info!("stt-groq: transcript '{}' in {}ms", text, elapsed.as_millis());

    Ok(text)
}

/// A single transcription segment from `verbose_json` responses.
/// Carries the model-native confidence signals used by the wake-word
/// verifier's confidence gate (v3): `no_speech_prob` flags phantom
/// readings on mangled/dropout-torn audio; `avg_logprob` flags garbage.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct GroqSegment {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub avg_logprob: f32,
    #[serde(default)]
    pub no_speech_prob: f32,
}

/// Transcribe with structured segment metadata (`verbose_json`).
/// Used ONLY by the wake-word verifier (v3 confidence gate). The normal
/// transcription path keeps plain `json` (smaller, faster to parse).
///
/// Requests pinned `temperature=0` (deterministic greedy; the documented
/// recommendation for STT accuracy) and English language.
pub async fn transcribe_with_groq_verbose(
    samples: &[i16],
    api_key: &str,
    client: &Client,
    prompt: Option<&str>,
) -> Result<(String, Vec<GroqSegment>), String> {
    if api_key.is_empty() {
        return Err("No Groq API key provided".to_string());
    }

    let wav_bytes = pcm_to_wav(samples, 16000);

    let part = reqwest::multipart::Part::bytes(wav_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("MIME error: {}", e))?;

    let mut form = reqwest::multipart::Form::new()
        .text("model", GROQ_MODEL.to_string())
        .text("language", "en".to_string())
        .text("response_format", "verbose_json".to_string())
        .text("temperature", "0".to_string())
        .part("file", part);
    if let Some(p) = prompt {
        form = form.text("prompt", p.to_string());
    }

    let resp = client
        .post(GROQ_STT_URL)
        .bearer_auth(api_key)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Groq STT request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if status.as_u16() == 429 {
            return Err("Groq rate limit hit (2,000 req/day free tier)".to_string());
        }
        if status.as_u16() == 401 {
            return Err("Groq API key invalid — check Settings".to_string());
        }
        return Err(format!("Groq STT error {}: {}", status, body));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Groq response parse error: {}", e))?;

    let text = json
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let mut segments = Vec::new();
    if let Some(arr) = json.get("segments").and_then(|s| s.as_array()) {
        for seg in arr {
            segments.push(GroqSegment {
                text: seg
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
                avg_logprob: seg
                    .get("avg_logprob")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32,
                no_speech_prob: seg
                    .get("no_speech_prob")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32,
            });
        }
    }

    Ok((text, segments))
}

/// Convert raw i16 PCM samples to a WAV file (16-bit, mono, given sample rate).
fn pcm_to_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let num_samples = samples.len();
    let data_size = num_samples * 2;
    let mut wav = Vec::with_capacity(44 + data_size);

    // RIFF header
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_size as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");

    // fmt chunk
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    // data chunk
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_size as u32).to_le_bytes());

    for &s in samples {
        wav.extend_from_slice(&s.to_le_bytes());
    }

    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pcm_to_wav_header() {
        let samples = vec![0i16; 160]; // 10ms of audio at 16kHz
        let wav = pcm_to_wav(&samples, 16000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + 320); // 44 header + 160 samples * 2 bytes
    }

    #[test]
    fn test_empty_samples() {
        let samples: Vec<i16> = vec![];
        let wav = pcm_to_wav(&samples, 16000);
        assert_eq!(wav.len(), 44); // just the header
    }

    #[test]
    fn test_groq_url_constant() {
        assert!(GROQ_STT_URL.starts_with("https://api.groq.com"));
        assert!(GROQ_STT_URL.contains("/audio/transcriptions"));
    }

    #[test]
    fn test_groq_model_constant() {
        assert_eq!(GROQ_MODEL, "whisper-large-v3-turbo");
    }

    #[test]
    fn test_nexus_vocabulary_budget_and_coverage() {
        // Whisper prompt cap is ~244 tokens; English averages ~1.3
        // tokens/word, so cap words at 140 for a safe margin. If this
        // fails, trim terms — never raise the cap.
        let words = NEXUS_VOCABULARY.split_whitespace().count();
        assert!(words <= 140, "vocabulary too long: {words} words");
        // Every entity the alias map rescues must be seeded upstream.
        for term in [
            "servx", "zync", "eesha", "nexus", "github", "whatsapp",
            "prem", "lakshya", "congi", "shopkart",
        ] {
            assert!(
                NEXUS_VOCABULARY.to_lowercase().contains(term),
                "vocabulary missing: {term}"
            );
        }
    }

    #[test]
    fn test_groq_segment_defaults_on_missing_fields() {
        // Groq may add/drop segment fields over time — parsing must never fail.
        let v: serde_json::Value = serde_json::json!({ "text": "Nexus." });
        let seg: GroqSegment = serde_json::from_value(v).unwrap();
        assert_eq!(seg.text, "Nexus.");
        assert_eq!(seg.avg_logprob, 0.0);
        assert_eq!(seg.no_speech_prob, 0.0);
    }

    #[test]
    fn test_groq_segment_full_parse() {
        let v: serde_json::Value = serde_json::json!({
            "text": " Lots of children.",
            "avg_logprob": -0.85,
            "no_speech_prob": 0.91
        });
        let seg: GroqSegment = serde_json::from_value(v).unwrap();
        assert_eq!(seg.text, " Lots of children.");
        assert!((seg.no_speech_prob - 0.91).abs() < 1e-6);
    }
}
