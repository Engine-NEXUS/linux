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
pub const NEXUS_VOCABULARY: &str = "Ghost mode. Ghost control. Exit ghost mode. \
Open WhatsApp. Open Chrome. Open VS Code. Open Spotify. Open Discord. \
Open Brave. Open YouTube. Open settings. Open architecture mapper. \
Open my chat with Mummy in WhatsApp. Chat with Amma. \
Analyse Servx. Analyse Zync. GitHub pull request. Merge pull request. \
List pull requests. Search for. Play pause. Next track. Previous track. \
Memory audit. Forget everything. Take the mouse. Control my cursor. \
Type this. Press enter. Click the button. Search the web. \
Remember this button as. Click learned. Forget learned. \
Ghostwriter. Take a letter. Order food. Send message. \
Deepgram Groq Supabase Eesha Prem Lakshya Congi Shopkart NEXUS \
Shut down my laptop. Restart my laptop. Lock my laptop. \
open close stop cancel navigate analyse search type click press scroll.";

/// Neutral English decoder instruction used when a turn has not been
/// owner-verified. Unlike the owner vocabulary, it does not bias Whisper
/// toward NEXUS commands or known entities.
pub const NEUTRAL_DECODER_PROMPT: &str = "Transcribe this English audio exactly as spoken.";

/// Locked English-only Groq transcription contract. Every STT upload must use
/// these decoder settings; language support is a routing concern, not a
/// per-call option.
pub const STT_LANGUAGE: &str = "en";
pub const STT_TEMPERATURE: &str = "0";
pub const STT_RESPONSE_FORMAT: &str = "json";
pub const STT_VERBOSE_RESPONSE_FORMAT: &str = "verbose_json";

/// Decoder bias selected from turn ownership. Owner-verified captures may use
/// the NEXUS vocabulary; all other captures use the neutral English prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderBias {
    OwnerCommand,
    Neutral,
}

/// Pure Groq multipart fields. Centralizing these values prevents one STT path
/// from accidentally permitting another language, temperature, or format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GroqTranscriptionFields {
    pub model: &'static str,
    pub language: &'static str,
    pub temperature: &'static str,
    pub response_format: &'static str,
    pub prompt: &'static str,
    pub file_name: String,
    pub mime: String,
}

pub(crate) fn decoder_prompt(prompt: Option<&'static str>) -> &'static str {
    prompt.unwrap_or(NEUTRAL_DECODER_PROMPT)
}

/// Which decoder bias a turn gets. The NEXUS vocabulary nudges Whisper toward NEXUS commands,
/// so it is withheld from a voice that is KNOWN not to be the owner (uncertain, rejected).
/// With nobody enrolled there is no owner to protect: `Unenrolled` gets the vocabulary too.
/// Before, every unenrolled turn got the neutral prompt, so "chat with mummy in WhatsApp" was
/// heard as "chart with mommy and what so".
pub(crate) fn decoder_bias_for(ownership: crate::voice_profile::TurnOwnership) -> DecoderBias {
    use crate::voice_profile::TurnOwnership::*;
    match ownership {
        Verified | Unenrolled => DecoderBias::OwnerCommand,
        Uncertain | Rejected => DecoderBias::Neutral,
    }
}

pub(crate) fn decoder_bias_prompt(bias: DecoderBias) -> &'static str {
    match bias {
        DecoderBias::OwnerCommand => NEXUS_VOCABULARY,
        DecoderBias::Neutral => NEUTRAL_DECODER_PROMPT,
    }
}

pub(crate) fn transcription_fields(
    file_name: &str,
    mime: &str,
    prompt: Option<&'static str>,
    verbose: bool,
) -> GroqTranscriptionFields {
    GroqTranscriptionFields {
        model: GROQ_MODEL,
        language: STT_LANGUAGE,
        temperature: STT_TEMPERATURE,
        response_format: if verbose {
            STT_VERBOSE_RESPONSE_FORMAT
        } else {
            STT_RESPONSE_FORMAT
        },
        prompt: decoder_prompt(prompt),
        file_name: file_name.to_string(),
        mime: mime.to_string(),
    }
}

fn groq_transcription_form(
    file_bytes: Vec<u8>,
    fields: &GroqTranscriptionFields,
) -> Result<reqwest::multipart::Form, String> {
    let part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(fields.file_name.clone())
        .mime_str(&fields.mime)
        .map_err(|e| format!("MIME error: {}", e))?;

    Ok(reqwest::multipart::Form::new()
        .text("model", fields.model.to_string())
        .text("language", fields.language.to_string())
        .text("temperature", fields.temperature.to_string())
        .text("response_format", fields.response_format.to_string())
        .text("prompt", fields.prompt.to_string())
        .part("file", part))
}

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
    prompt: Option<&'static str>,
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

    let fields = transcription_fields("audio.wav", "audio/wav", prompt, false);
    let form = groq_transcription_form(wav_bytes, &fields)?;

    let start = std::time::Instant::now();

    crate::usage_counter::bump("stt");
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
    prompt: Option<&'static str>,
) -> Result<String, String> {
    if api_key.is_empty() {
        return Err("No Groq API key provided".to_string());
    }
    if audio.is_empty() {
        return Err("Empty audio".to_string());
    }

    let fields = transcription_fields(filename, mime, prompt, false);
    let form = groq_transcription_form(audio.to_vec(), &fields)?;

    let start = std::time::Instant::now();

    crate::usage_counter::bump("stt");
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
    prompt: Option<&'static str>,
) -> Result<(String, Vec<GroqSegment>), String> {
    if api_key.is_empty() {
        return Err("No Groq API key provided".to_string());
    }

    let wav_bytes = pcm_to_wav(samples, 16000);

    let fields = transcription_fields("audio.wav", "audio/wav", prompt, true);
    let form = groq_transcription_form(wav_bytes, &fields)?;

    crate::usage_counter::bump("stt");
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
    fn decoder_bias_follows_ownership() {
        use crate::voice_profile::TurnOwnership::*;
        // Nobody enrolled: there is no owner to protect, so the NEXUS vocabulary applies.
        assert_eq!(decoder_bias_for(Unenrolled), DecoderBias::OwnerCommand);
        assert_eq!(decoder_bias_for(Verified), DecoderBias::OwnerCommand);
        // A voice that is (or may be) someone else never gets command biasing.
        assert_eq!(decoder_bias_for(Uncertain), DecoderBias::Neutral);
        assert_eq!(decoder_bias_for(Rejected), DecoderBias::Neutral);
    }

    #[test]
    fn vocabulary_teaches_the_chat_command_phrasing() {
        let v = NEXUS_VOCABULARY.to_lowercase();
        assert!(v.contains("chat with"), "STT heard \"chat with\" as \"chart with\"/\"start with\"");
        assert!(v.contains("in whatsapp"), "STT heard \"in WhatsApp\" as \"and what so\"");
    }

    #[test]
    fn test_english_transcription_contract_is_locked() {
        for verbose in [false, true] {
            let fields = transcription_fields("audio.wav", "audio/wav", None, verbose);
            assert_eq!(fields.model, GROQ_MODEL);
            assert_eq!(fields.language, "en");
            assert_eq!(fields.temperature, "0");
            assert_eq!(fields.prompt, NEUTRAL_DECODER_PROMPT);
            assert_eq!(
                fields.response_format,
                if verbose {
                    STT_VERBOSE_RESPONSE_FORMAT
                } else {
                    STT_RESPONSE_FORMAT
                }
            );
        }

        let owner = transcription_fields("audio.wav", "audio/wav", Some(NEXUS_VOCABULARY), false);
        assert_eq!(owner.prompt, NEXUS_VOCABULARY);
        assert_eq!(decoder_bias_prompt(DecoderBias::OwnerCommand), NEXUS_VOCABULARY);
        assert_eq!(decoder_bias_prompt(DecoderBias::Neutral), NEUTRAL_DECODER_PROMPT);
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
