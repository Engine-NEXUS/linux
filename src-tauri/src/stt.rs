//! STT proxy — routes to Groq cloud STT (primary) or local Moonshine (fallback).
//!
//! Primary: Groq Whisper Large v3 Turbo (cloud, ~247ms, $0 free tier)
//! Fallback: Moonshine Small Streaming (local Python sidecar, ~165ms, 7.84% WER)
//!
//! The fallback is used when:
//! - No Groq API key is set in settings
//! - Groq API is unreachable (network error)
//! - Groq rate limit is hit (429)
//! - Groq returns an error
//! - localSttOnly is true (privacy mode — audio never leaves the device)

use std::sync::Arc;
use tauri::State;

pub struct SttState {
    /// Reused HTTP client — avoids building a new reqwest::Client per transcription.
    pub client: Arc<reqwest::Client>,
}

impl SttState {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build STT HTTP client");
        Self { client: Arc::new(client) }
    }
}

const STT_URL: &str = "http://127.0.0.1:39217/transcribe";

/// Transcribe audio — tries Groq cloud first, falls back to local faster-whisper.
///
/// The frontend sends Int16 PCM samples at 16kHz. We route to Groq if an API
/// key is configured, otherwise use the local Python sidecar.
#[tauri::command]
pub async fn transcribe_audio(
    samples: Vec<i16>,
    state: State<'_, SttState>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    tracing::info!("stt: received {} samples for transcription", samples.len());

    // Read Groq API key from settings
    let groq_key = crate::commands::read_groq_api_key(&app);

    // Check if user has enabled "Local STT only" (privacy mode).
    // If true, audio never leaves the device — skip Groq cloud entirely.
    let local_only = crate::commands::read_local_stt_only(&app);
    if local_only {
        tracing::info!("stt: localSttOnly=true, using local whisper (privacy mode)");
    } else if !groq_key.is_empty() {
        // Primary: Groq cloud STT (~247ms, free)
        match crate::stt_groq::transcribe_with_groq(&samples, &groq_key, &state.client).await {
            Ok(text) => {
                let filtered = apply_hallucination_filter(&text);
                if filtered != text {
                    tracing::info!("stt: filtered hallucination '{}' -> '{}'", text, filtered);
                } else {
                    tracing::info!("stt: groq transcript: '{}'", filtered);
                }
                return Ok(filtered);
            }
            Err(e) => {
                tracing::warn!("stt: groq failed ({}), falling back to local whisper", e);
                // Fall through to local STT
            }
        }
    } else {
        tracing::info!("stt: no groq key, using local whisper directly");
    }

    // Fallback: local faster-whisper Python sidecar
    transcribe_local(&samples, &state.client).await
}

/// Transcribe audio samples using the full STT pipeline (Groq → local fallback).
/// This is the same logic as `transcribe_audio` but can be called from
/// the Rust-side STT capture thread (no Tauri command context needed).
///
/// # Arguments
/// * `samples` - Raw i16 PCM samples at 16kHz mono
/// * `client` - Reused reqwest client
/// * `app` - Optional AppHandle for reading Groq API key and local_stt_only setting
pub async fn transcribe_samples<R: tauri::Runtime>(
    samples: &[i16],
    client: &reqwest::Client,
    app: Option<&tauri::AppHandle<R>>,
) -> Result<String, String> {
    tracing::info!("stt: transcribing {} samples (Rust-side capture)", samples.len());

    // Read Groq API key from settings (if app handle is available)
    if let Some(app) = app {
        let groq_key = crate::commands::read_groq_api_key(app);
        let local_only = crate::commands::read_local_stt_only(app);

        if local_only {
            tracing::info!("stt: localSttOnly=true, using local whisper (privacy mode)");
        } else if !groq_key.is_empty() {
            match crate::stt_groq::transcribe_with_groq(samples, &groq_key, client).await {
                Ok(text) => {
                    let filtered = apply_hallucination_filter(&text);
                    if filtered != text {
                        tracing::info!("stt: filtered hallucination '{}' -> '{}'", text, filtered);
                    } else {
                        tracing::info!("stt: groq transcript: '{}'", filtered);
                    }
                    return Ok(filtered);
                }
                Err(e) => {
                    tracing::warn!("stt: groq failed ({}), falling back to local whisper", e);
                }
            }
        } else {
            tracing::info!("stt: no groq key, using local whisper directly");
        }
    }

    // Fallback: local faster-whisper Python sidecar
    transcribe_local(samples, client).await
}

/// Transcribe using the local faster-whisper Python sidecar (port 39217).
pub async fn transcribe_local(samples: &[i16], client: &reqwest::Client) -> Result<String, String> {
    // Ensure the STT server is running (lazy start)
    crate::lazy_stt::ensure_stt_running();
    crate::lazy_stt::mark_stt_request();

    let mut ready = false;
    for attempt in 0..40 {
        let health = client
            .get("http://127.0.0.1:39217/health")
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await;

        if let Ok(resp) = health {
            if resp.status().is_success() {
                ready = true;
                break;
            }
        }

        if attempt == 0 {
            tracing::info!("stt: waiting for local STT server to be ready...");
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    if !ready {
        return Err("STT server did not become ready in 20s".to_string());
    }

    let wav_bytes = pcm_to_wav(samples, 16000);
    tracing::info!("stt: sending {} bytes WAV to {}", wav_bytes.len(), STT_URL);

    let part = reqwest::multipart::Part::bytes(wav_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("MIME error: {}", e))?;
    let form = reqwest::multipart::Form::new().part("audio", part);

    let resp = client
        .post(STT_URL)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("STT server request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        tracing::error!("stt: server returned {} : {}", status, body);
        return Err(format!("STT server error {}: {}", status, body));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("STT response parse error: {}", e))?;

    let text = json
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let filtered = apply_hallucination_filter(&text);

    if filtered != text {
        tracing::info!("stt: filtered hallucination '{}' -> '{}'", text, filtered);
    } else {
        tracing::info!("stt: local transcript: '{}'", filtered);
    }

    Ok(filtered)
}

/// Check if the STT server is reachable (local fallback status).
#[tauri::command]
pub async fn stt_status(state: State<'_, SttState>) -> Result<bool, String> {
    let resp = state.client
        .get("http://127.0.0.1:39217/health")
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await;

    Ok(resp.map(|r| r.status().is_success()).unwrap_or(false))
}

/// Convert raw i16 PCM samples to a WAV file (16-bit, mono, given sample rate).
fn pcm_to_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let num_samples = samples.len();
    let data_size = num_samples * 2; // 16-bit = 2 bytes per sample
    let mut wav = Vec::with_capacity(44 + data_size);

    // RIFF header
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_size as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");

    // fmt chunk
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    wav.extend_from_slice(&1u16.to_le_bytes());  // audio format = PCM
    wav.extend_from_slice(&1u16.to_le_bytes());  // num channels = mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes());  // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    // data chunk
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_size as u32).to_le_bytes());

    // PCM samples (little-endian i16)
    for &s in samples {
        wav.extend_from_slice(&s.to_le_bytes());
    }

    wav
}

/// Filter common Whisper hallucinations on noisy/silent audio.
fn apply_hallucination_filter(text: &str) -> String {
    let lower = text.to_lowercase();
    let hallucinations = [
        "thank you for watching", "thanks for watching", "thank you.", "you.", "bye.",
        "please subscribe", "subscribe to my channel",
    ];

    for h in hallucinations.iter() {
        if lower.contains(h) {
            return "".to_string();
        }
    }

    let alpha_count = text.chars().filter(|c| c.is_alphabetic()).count();
    if alpha_count < 2 {
        return "".to_string();
    }

    text.to_string()
}
