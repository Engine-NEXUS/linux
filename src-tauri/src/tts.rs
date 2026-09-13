//! TTS — 2-tier fallback: edge-tts (cloud) → Piper (local).
//!
//! Phase 2 architecture:
//!   Primary:   edge-tts (Microsoft Neural, cloud, $0, ~200ms, 0 MB RAM)
//!   Fallback:  Piper (local VITS ONNX, $0, ~40ms, 80 MB RAM, lazy-loaded)
//!
//! Cached acknowledgment phrases ("On it sir", etc.) are pre-synthesized at
//! boot using edge-tts and stored as f32 PCM in RAM. Playback is <5ms
//! regardless of which engine generated them.

use crate::meeting_detect::MeetingState;
use rodio::{buffer::SamplesBuffer, OutputStream, Sink};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::{Emitter, State};

/// Global Piper engine reference — set at TtsState creation.
/// Allows the network monitor to unload Piper without accessing TtsState.
static GLOBAL_PIPER_ENGINE: std::sync::OnceLock<crate::tts_piper::PiperEngine> =
    std::sync::OnceLock::new();

pub struct TtsState {
    /// Piper fallback engine (lazy-loaded only when edge-tts fails).
    pub piper_engine: crate::tts_piper::PiperEngine,
    /// Pre-synthesized short phrases for instant acknowledgment playback.
    /// Keyed by the exact phrase text. Stores f32 PCM samples + sample rate.
    pub cache: Arc<Mutex<HashMap<String, CachedAudio>>>,
}

/// Cached audio: PCM samples + sample rate for rodio playback.
#[derive(Clone)]
pub struct CachedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl TtsState {
    pub fn new() -> Self {
        let piper_engine = crate::tts_piper::new_engine();
        // Store global reference for the network monitor to access
        let _ = GLOBAL_PIPER_ENGINE.set(piper_engine.clone());
        Self {
            piper_engine,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

/// Unload the Piper engine globally (called by network monitor after 10 min).
pub async fn unload_piper_global() {
    if let Some(engine) = GLOBAL_PIPER_ENGINE.get() {
        crate::tts_piper::unload_engine(engine).await;
    }
}

/// Phrases that are pre-synthesized on first TTS load for instant playback.
/// These are the high-frequency acknowledgment/error phrases that must play
/// with zero synthesis delay to feel natural.
const CACHED_PHRASES: &[&str] = &[
    "On it sir",
    "Didn't understand that sir",
    "Didn't catch that sir",
    "Here is the analysis, sir",
    "Ok sir",
];

/// Pre-synthesize cached phrases using edge-tts at boot.
/// Falls back to Piper if edge-tts is unavailable.
/// This runs once at startup so all subsequent speak_cached() calls are instant (<5ms).
pub async fn pregenerate_cache(
    cache_arc: &Arc<Mutex<HashMap<String, CachedAudio>>>,
    voice: &str,
) {
    let cache_start = std::time::Instant::now();

    let mut cached_count = 0;

    // Always try edge-tts first (cloud, best quality).
    // If it fails, the Piper fallback below kicks in automatically.
    tracing::info!("tts: pre-generating cache with edge-tts (voice={})", voice);
    for phrase in CACHED_PHRASES {
        match crate::tts_edge::synthesize_to_pcm(phrase, voice).await {
                Ok((samples, sr)) => {
                    cache_arc.lock().await.insert(
                        phrase.to_string(),
                        CachedAudio { samples, sample_rate: sr },
                    );
                    cached_count += 1;
                }
                Err(e) => {
                    tracing::warn!("tts: edge-tts cache failed for '{}': {}", phrase, e);
                }
            }
        }

    // If edge-tts failed, try Piper for cache
    if cached_count == 0 {
        tracing::info!("tts: falling back to Piper for cache generation");
        // We need a temporary Piper engine for cache generation
        let piper_engine = crate::tts_piper::new_engine();
        for phrase in CACHED_PHRASES {
            match crate::tts_piper::synthesize(&piper_engine, phrase).await {
                Ok((samples, sr)) => {
                    cache_arc.lock().await.insert(
                        phrase.to_string(),
                        CachedAudio { samples, sample_rate: sr },
                    );
                    cached_count += 1;
                }
                Err(e) => {
                    tracing::warn!("tts: piper cache failed for '{}': {}", phrase, e);
                }
            }
        }
    }

    tracing::info!(
        "tts: cached {} phrases in {:.2}s",
        cached_count,
        cache_start.elapsed().as_secs_f32()
    );
}

/// Global generation counter: incremented by `stop_tts` to signal the playback thread
/// to stop the current audio immediately.
static TTS_GENERATION: AtomicUsize = AtomicUsize::new(0);

/// IPC: Stop any currently-playing TTS audio.
#[tauri::command]
pub fn stop_tts() -> Result<(), String> {
    TTS_GENERATION.fetch_add(1, Ordering::SeqCst);
    tracing::info!("tts: stop requested (generation {})", TTS_GENERATION.load(Ordering::SeqCst));
    Ok(())
}

/// IPC: Speak text using the 3-tier fallback chain.
///
/// Tries edge-tts (cloud) first, then Piper (local), then eSpeak (last resort).
/// For cached phrases, plays instantly from memory (<5ms).
#[tauri::command]
pub async fn speak_text(
    text: String,
    voice: Option<String>,
    _speed: Option<f32>,
    state: State<'_, TtsState>,
    meeting: State<'_, Arc<MeetingState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    tracing::info!("tts: speaking '{}'", text);

    meeting.set_tts_playing(true);
    let my_generation = TTS_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    // Read settings
    let edge_voice = crate::commands::read_edge_tts_voice(&app);
    let voice_id = voice.unwrap_or_else(|| edge_voice.clone());
    let tts_volume_pct = read_tts_volume(&app);

    // Try to synthesize using the 3-tier fallback chain
    let (audio, sample_rate) = match synthesize_with_fallback(&text, &voice_id, &state).await {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("tts: all TTS engines failed: {}", e);
            meeting.set_tts_playing(false);
            return Err(e);
        }
    };

    // Check if stop was requested during synthesis
    if TTS_GENERATION.load(Ordering::SeqCst) > my_generation {
        tracing::info!("tts: stop requested during synthesis, skipping playback");
        // Do NOT set tts_playing=false here. A newer TTS call (higher
        // generation) is already active and has set tts_playing=true.
        // If we set it false, the wake word detector becomes un-suppressed
        // during the newer TTS playback, causing false wakes from TTS echo.
        // Only the current (newest) generation should clear the flag.
        return Ok(());
    }

    // Save and set system volume
    let volume_changed = if tts_volume_pct > 0 {
        let target = tts_volume_pct as f32 / 100.0;
        crate::volume::save_and_set_volume(target)
    } else {
        false
    };

    // Emit audio-started event
    let _ = app.emit("tts:audio-started", &text);

    // Play audio
    let play_result = play_audio(audio, sample_rate, my_generation).await;

    // Restore volume
    if volume_changed {
        crate::volume::restore_volume();
    }

    // Grace period
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    meeting.set_tts_playing(false);

    play_result
}

/// IPC: Preview a specific TTS voice with a short demo phrase.
/// Used by the settings sidebar voice picker — plays a demo without
/// saving the voice as the default. Does NOT change system volume.
#[tauri::command]
pub async fn preview_voice(
    voice_id: String,
    text: Option<String>,
    state: State<'_, TtsState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let demo_text = text.unwrap_or_else(|| {
        if voice_id.starts_with("en-US-") {
            let name = voice_id
                .strip_prefix("en-US-")
                .unwrap_or("")
                .trim_end_matches("Neural");
            format!("Hello, I'm {}. This is how I sound.", name)
        } else if voice_id == "piper-amy" {
            "Hello, I'm Amy. This is the offline voice.".to_string()
        } else {
            "Hello, this is a voice preview.".to_string()
        }
    });

    tracing::info!("tts: previewing voice '{}' with text '{}'", voice_id, demo_text);

    let my_generation = TTS_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    let (audio, sample_rate) = match synthesize_with_fallback(&demo_text, &voice_id, &state).await {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("tts: voice preview failed for '{}': {}", voice_id, e);
            return Err(e);
        }
    };

    if TTS_GENERATION.load(Ordering::SeqCst) > my_generation {
        return Ok(());
    }

    let _ = app.emit("tts:audio-started", &demo_text);
    play_audio(audio, sample_rate, my_generation).await
}
///
/// Falls back to `speak_text` if the phrase is not in the cache.
#[tauri::command]
pub async fn speak_cached(
    text: String,
    state: State<'_, TtsState>,
    meeting: State<'_, Arc<MeetingState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    tracing::info!("tts: speaking cached '{}'", text);

    meeting.set_tts_playing(true);
    let my_generation = TTS_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    // Try to get cached audio
    let cache_arc = state.cache.clone();
    let cached_audio = {
        let cache = cache_arc.lock().await;
        cache.get(&text).cloned()
    };

    let (audio, sample_rate) = match cached_audio {
        Some(ca) => {
            tracing::info!("tts: cache hit for '{}'", text);
            (ca.samples, ca.sample_rate)
        }
        None => {
            // Cache miss — synthesize on demand
            tracing::info!("tts: cache miss for '{}', synthesizing on demand", text);
            let edge_voice = crate::commands::read_edge_tts_voice(&app);
            match synthesize_with_fallback(&text, &edge_voice, &state).await {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!("tts: synthesis failed for cached phrase: {}", e);
                    meeting.set_tts_playing(false);
                    return Err(e);
                }
            }
        }
    };

    // Check if stop was requested
    if TTS_GENERATION.load(Ordering::SeqCst) > my_generation {
        tracing::info!("tts: stop requested before cached playback, skipping");
        // Do NOT set tts_playing=false — a newer generation is active.
        return Ok(());
    }

    // Read TTS volume
    let tts_volume_pct = read_tts_volume(&app);
    let volume_changed = if tts_volume_pct > 0 {
        let target = tts_volume_pct as f32 / 100.0;
        crate::volume::save_and_set_volume(target)
    } else {
        false
    };

    // Emit event
    let _ = app.emit("tts:audio-started", &text);

    // Play
    let play_result = play_audio(audio, sample_rate, my_generation).await;

    // Restore volume
    if volume_changed {
        crate::volume::restore_volume();
    }

    // Grace period
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    meeting.set_tts_playing(false);

    play_result
}

/// 2-tier synthesis fallback: edge-tts → Piper.
///
/// Returns (f32 PCM samples, sample_rate) on success.
async fn synthesize_with_fallback(
    text: &str,
    voice: &str,
    state: &TtsState,
) -> Result<(Vec<f32>, u32), String> {
    // If the voice is explicitly the local Piper voice, skip Edge TTS.
    if voice == "piper-amy" {
        tracing::info!("tts: using Piper directly (piper-amy voice selected)");
        crate::tts_network::mark_piper_loaded();
        return crate::tts_piper::synthesize(&state.piper_engine, text).await;
    }

    // Check network state — if network is down, skip Edge TTS entirely
    // and go straight to Piper (saves ~1-2s of waiting for Edge to fail).
    let network_up = crate::tts_network::check_network_now().await;

    if network_up {
        // Tier 1: edge-tts (cloud, ~200ms, best quality, 0 MB RAM)
        match crate::tts_edge::synthesize_to_pcm(text, voice).await {
            Ok((samples, sr)) => {
                tracing::info!("tts: edge-tts synthesis OK (cloud)");
                return Ok((samples, sr));
            }
            Err(e) => {
                tracing::warn!("tts: edge-tts failed ({}), trying Piper fallback", e);
                // Edge TTS failed even though network check passed —
                // could be a transient error. Mark network as down.
                crate::tts_network::set_network_down();
            }
        }
    } else {
        tracing::info!("tts: network down — using Piper directly (skipping Edge TTS)");
    }

    // Tier 2: Piper (local, ~40ms, good quality, ~80 MB RAM)
    crate::tts_network::mark_piper_loaded();
    match crate::tts_piper::synthesize(&state.piper_engine, text).await {
        Ok((samples, sr)) => {
            tracing::info!("tts: piper fallback synthesis OK (local)");
            return Ok((samples, sr));
        }
        Err(e) => {
            tracing::error!("tts: piper fallback also failed: {}", e);
            Err(format!("All TTS engines failed. Last error: {}", e))
        }
    }
}

/// Play f32 PCM audio through rodio with barge-in support.
async fn play_audio(
    audio: Vec<f32>,
    sample_rate: u32,
    my_generation: usize,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        match OutputStream::try_default() {
            Ok((_stream, handle)) => {
                match Sink::try_new(&handle) {
                    Ok(sink) => {
                        let source = SamplesBuffer::new(1, sample_rate, audio);
                        sink.append(source);

                        // Poll for stop request (barge-in)
                        while !sink.empty() {
                            if TTS_GENERATION.load(Ordering::SeqCst) > my_generation {
                                sink.stop();
                                tracing::info!("tts: playback stopped by user (barge-in)");
                                return Ok(());
                            }
                            std::thread::sleep(std::time::Duration::from_millis(20));
                        }
                        tracing::info!("tts: audio playback completed");
                        Ok(())
                    }
                    Err(e) => {
                        tracing::error!("tts: failed to create audio sink: {}", e);
                        Err(format!("Failed to create audio sink: {}", e))
                    }
                }
            }
            Err(e) => {
                tracing::error!("tts: failed to open audio output: {}", e);
                Err(format!("Failed to get audio output stream: {}", e))
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        tracing::error!("tts: audio thread panicked");
        Err("Audio thread panicked".to_string())
    })
}

/// Read the TTS volume setting from settings.json.
/// Returns 0-100. 0 means "disabled" (don't adjust system volume).
fn read_tts_volume(app: &tauri::AppHandle) -> u8 {
    use tauri::Manager;
    let dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(_) => return 75,
    };
    let path = dir.join("settings.json");
    if !path.exists() {
        return 75;
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return 75,
    };
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
        if let Some(vol) = json.get("ttsVolume").and_then(|v| v.as_u64()) {
            return vol as u8;
        }
        if let Some(vol) = json.get("tts_volume").and_then(|v| v.as_u64()) {
            return vol as u8;
        }
    }
    75
}
