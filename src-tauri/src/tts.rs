use crate::meeting_detect::MeetingState;
use piper_rs::Piper;
use rodio::{buffer::SamplesBuffer, OutputStream, Sink};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::{Emitter, State};

pub struct TtsState {
    pub engine: Arc<Mutex<Option<Piper>>>,
    /// Pre-synthesized short phrases for instant acknowledgment playback.
    /// Keyed by the exact phrase text. Stores f32 PCM samples at the model's
    /// native sample rate (typically 22050 Hz for Piper medium voices).
    pub cache: Arc<Mutex<HashMap<String, (Vec<f32>, u32)>>>,
    /// The sample rate of the loaded Piper model (e.g. 22050).
    /// Stored after engine load so playback uses the correct rate.
    pub sample_rate: Arc<Mutex<u32>>,
}

/// Phrases that are pre-synthesized on first Piper load for instant playback.
/// These are the high-frequency acknowledgment/error phrases that must play
/// with zero synthesis delay to feel natural.
const CACHED_PHRASES: &[&str] = &[
    "On it sir",
    "Didn't understand that sir",
    "Didn't catch that sir",
    "Here is the analysis, sir",
    "Ok sir",
];

/// Default Piper voice model (en_US-amy-medium ΓÇö warm, neutral American female).
/// ~63 MB on disk, ~80 MB in RAM, 22050 Hz output.
const PIPER_MODEL_NAME: &str = "en_US-amy-medium";
const PIPER_MODEL_URL: &str = "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/amy/medium/en_US-amy-medium.onnx";
const PIPER_CONFIG_URL: &str = "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/amy/medium/en_US-amy-medium.onnx.json";

/// Resolve the Piper model + config paths.
/// Checks in order:
///   1. resources/piper/ next to the executable (production bundled)
///   2. %APPDATA%/com.nexus.assistant/piper/ (cached download)
/// Returns (model_path, config_path) if both exist, None otherwise.
fn resolve_piper_paths() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    // 1. Check resources/ next to executable (production bundled)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let res_dir = exe_dir.join("resources").join("piper");
            let model = res_dir.join(format!("{}.onnx", PIPER_MODEL_NAME));
            let config = res_dir.join(format!("{}.onnx.json", PIPER_MODEL_NAME));
            if model.exists() && config.exists() {
                tracing::info!("tts: using bundled Piper model at {}", res_dir.display());
                return Some((model, config));
            }
        }
    }

    // 2. Check app data directory (cached download)
    if let Some(data_dir) = dirs_next::data_dir() {
        let piper_dir = data_dir.join("com.nexus.assistant").join("piper");
        let model = piper_dir.join(format!("{}.onnx", PIPER_MODEL_NAME));
        let config = piper_dir.join(format!("{}.onnx.json", PIPER_MODEL_NAME));
        if model.exists() && config.exists() {
            tracing::info!("tts: using cached Piper model at {}", piper_dir.display());
            return Some((model, config));
        }
    }

    None
}

/// Download the Piper voice model + config from HuggingFace.
/// Saves to %APPDATA%/com.nexus.assistant/piper/.
/// Model is ~63 MB, config is ~1 KB. One-time download.
async fn download_piper_model() -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let data_dir = dirs_next::data_dir()
        .ok_or_else(|| "Could not find data directory".to_string())?;
    let piper_dir = data_dir.join("com.nexus.assistant").join("piper");
    std::fs::create_dir_all(&piper_dir)
        .map_err(|e| format!("Failed to create piper dir: {}", e))?;

    let model_path = piper_dir.join(format!("{}.onnx", PIPER_MODEL_NAME));
    let config_path = piper_dir.join(format!("{}.onnx.json", PIPER_MODEL_NAME));

    // Download model (~63 MB)
    if !model_path.exists() {
        tracing::info!("tts: downloading Piper model ({} MB)...", 63);
        let start = std::time::Instant::now();
        let bytes = reqwest::get(PIPER_MODEL_URL)
            .await
            .map_err(|e| format!("Model download failed: {}", e))?
            .bytes()
            .await
            .map_err(|e| format!("Model download read failed: {}", e))?;
        std::fs::write(&model_path, &bytes)
            .map_err(|e| format!("Failed to write model file: {}", e))?;
        tracing::info!(
            "tts: Piper model downloaded in {:.1}s ({} bytes)",
            start.elapsed().as_secs_f32(),
            bytes.len()
        );
    }

    // Download config (~1 KB)
    if !config_path.exists() {
        let bytes = reqwest::get(PIPER_CONFIG_URL)
            .await
            .map_err(|e| format!("Config download failed: {}", e))?
            .bytes()
            .await
            .map_err(|e| format!("Config download read failed: {}", e))?;
        std::fs::write(&config_path, &bytes)
            .map_err(|e| format!("Failed to write config file: {}", e))?;
    }

    Ok((model_path, config_path))
}

/// Lazily initialize the Piper TTS engine on first use.
/// Called from `speak_text` when the engine is `None`.
/// Saves ~80 MB RAM at idle by not loading Piper at boot.
/// First TTS call takes ~85ms extra (one-time model load); subsequent calls are instant.
/// Also pre-synthesizes cached acknowledgment phrases for instant playback.
async fn ensure_engine_loaded(
    engine_arc: &Arc<Mutex<Option<Piper>>>,
    cache_arc: &Arc<Mutex<HashMap<String, (Vec<f32>, u32)>>>,
    sample_rate_arc: &Arc<Mutex<u32>>,
) -> Result<(), String> {
    // Fast path: already loaded
    if engine_arc.lock().await.is_some() {
        return Ok(());
    }

    tracing::info!("tts: lazy-loading Piper engine on first speak...");
    let start_time = std::time::Instant::now();

    // 1. Resolve model paths (bundled or cached)
    let (model_path, config_path) = match resolve_piper_paths() {
        Some(paths) => paths,
        None => {
            // 2. Download on first use (one-time, ~63 MB)
            tracing::info!("tts: Piper model not found, downloading...");
            download_piper_model().await?
        }
    };

    // 3. Load Piper model (sync ΓÇö wrap in spawn_blocking)
    let model_path_clone = model_path.clone();
    let config_path_clone = config_path.clone();
    let load_result = tokio::task::spawn_blocking(move || {
        Piper::new(&model_path_clone, &config_path_clone)
    })
    .await
    .map_err(|e| format!("Piper load task panicked: {}", e))?;

    match load_result {
        Ok(piper) => {
            // Get sample rate from the model config
            // Piper stores it in the config, but we can't access it directly.
            // We'll get it from the first synthesis call. For now, default to 22050.
            *sample_rate_arc.lock().await = 22050;
            *engine_arc.lock().await = Some(piper);
            tracing::info!(
                "tts: Piper engine lazy-loaded in {:.2}s",
                start_time.elapsed().as_secs_f32()
            );

            // Pre-synthesize cached phrases for instant acknowledgment playback.
            pregenerate_cache(engine_arc, cache_arc, sample_rate_arc).await;

            Ok(())
        }
        Err(e) => {
            tracing::error!("tts: failed to lazy-init Piper: {}", e);
            Err(format!("TTS engine init failed: {}", e))
        }
    }
}

/// Pre-synthesize cached phrases using the loaded Piper engine.
/// Runs once after engine load. Each phrase is synthesized at the default
/// speed and stored as (f32 PCM samples, sample_rate).
async fn pregenerate_cache(
    engine_arc: &Arc<Mutex<Option<Piper>>>,
    cache_arc: &Arc<Mutex<HashMap<String, (Vec<f32>, u32)>>>,
    sample_rate_arc: &Arc<Mutex<u32>>,
) {
    let cache_start = std::time::Instant::now();
    // Piper length_scale: 1.0 = normal speed. Lower = faster.
    // Match Kokoro's 1.15x speed: length_scale = 1.0 / 1.15 Γëê 0.87
    let length_scale = 0.87_f32;

    let mut cached_count = 0;
    for phrase in CACHED_PHRASES {
        let p = phrase.to_string();
        let ea = engine_arc.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut lock = ea.blocking_lock();
            if let Some(engine) = lock.as_mut() {
                engine.create(&p, false, None, Some(length_scale), None, None)
            } else {
                Err(piper_rs::PiperError::InferenceError(
                    "TTS Engine not initialized".to_string(),
                ))
            }
        })
        .await;

        match result {
            Ok(Ok((audio, sr))) => {
                // Update sample rate from the first successful synthesis
                *sample_rate_arc.lock().await = sr;
                cache_arc.lock().await.insert(phrase.to_string(), (audio, sr));
                cached_count += 1;
            }
            Ok(Err(e)) => {
                tracing::warn!("tts: cache pre-gen failed for '{}': {}", phrase, e);
            }
            Err(e) => {
                tracing::warn!("tts: cache pre-gen task panicked for '{}': {}", phrase, e);
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

/// Single shared output handle, opened once for the life of the app.
/// Opening + dropping OutputStream per speak tears down the shared ALSA
/// duplex handle on Linux and kills the wake-word input stream with
/// `alsa::poll() returned POLLERR`. Leaked stream lives as long as process.
fn tts_output_handle() -> Result<rodio::OutputStreamHandle, String> {
    static HANDLE: std::sync::OnceLock<rodio::OutputStreamHandle> =
        std::sync::OnceLock::new();
    if let Some(h) = HANDLE.get() {
        return Ok(h.clone());
    }
    let (stream, handle) = OutputStream::try_default()
        .map_err(|e| format!("Failed to get audio output stream: {e}"))?;
    std::mem::forget(stream);
    let _ = HANDLE.set(handle.clone());
    Ok(handle)
}

/// IPC: Stop any currently-playing TTS audio.
#[tauri::command]
pub fn stop_tts() -> Result<(), String> {
    TTS_GENERATION.fetch_add(1, Ordering::SeqCst);
    tracing::info!("tts: stop requested (generation {})", TTS_GENERATION.load(Ordering::SeqCst));
    Ok(())
}

#[tauri::command]
pub async fn speak_text(
    text: String,
    voice: Option<String>,
    speed: Option<f32>,
    state: State<'_, TtsState>,
    meeting: State<'_, Arc<MeetingState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    tracing::info!("tts: speaking '{}'", text);

    // 1. Mark TTS as playing to suppress wake word self-trigger
    meeting.set_tts_playing(true);

    let my_generation = TTS_GENERATION.load(Ordering::SeqCst);

    // 2. Lazy-load Piper engine on first speak (saves ~80 MB at idle).
    //    First call: ~85ms model load. Subsequent calls: instant (fast path).
    let engine_arc = state.engine.clone();
    let cache_arc = state.cache.clone();
    let sample_rate_arc = state.sample_rate.clone();
    ensure_engine_loaded(&engine_arc, &cache_arc, &sample_rate_arc).await?;

    // Piper length_scale: 1.0 = normal, <1.0 = faster, >1.0 = slower
    let spd = speed.unwrap_or(1.15);
    let length_scale = (1.0 / spd).clamp(0.5, 3.0);

    let text_for_event = text.clone();
    let audio_result = tokio::task::spawn_blocking(move || {
        let mut lock = engine_arc.blocking_lock();
        if let Some(engine) = lock.as_mut() {
            engine
                .create(&text, false, None, Some(length_scale), None, None)
                .map_err(|e| format!("TTS synthesis error: {}", e))
        } else {
            Err("TTS Engine not initialized".to_string())
        }
    })
    .await
    .map_err(|e| format!("TTS task panicked: {}", e))?;

    let (audio, sample_rate) = audio_result?;

    // Check if stop was requested DURING synthesis
    if TTS_GENERATION.load(Ordering::SeqCst) > my_generation {
        tracing::info!("tts: stop requested during synthesis, skipping playback");
        meeting.set_tts_playing(false);
        return Ok(());
    }

    // 3. Emit tts:audio-started event so the frontend can sync the orb
    //    hide + loading indicator show with actual audio playback.
    let _ = app.emit("tts:audio-started", &text_for_event);

    // 5. Play audio using spawn_blocking so the tokio runtime can still
    //    process other commands (like stop_tts) while audio plays.
    let play_result = tokio::task::spawn_blocking(move || {
        match tts_output_handle() {
            Ok(handle) => {
                match Sink::try_new(&handle) {
                    Ok(sink) => {
                        // Piper output sample rate is dynamic (typically 22050 Hz)
                        let source = SamplesBuffer::new(1, sample_rate, audio);
                        sink.append(source);

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
                tracing::error!("tts: failed to open default audio output: {}", e);
                Err(format!("Failed to get audio output stream: {}", e))
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        tracing::error!("tts: audio thread panicked");
        Err("Audio thread panicked".to_string())
    });

    // 4. Grace period for acoustic settling before resuming wake word detection
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    meeting.set_tts_playing(false);

    play_result
}

/// IPC: Play a pre-cached TTS phrase instantly from memory.
///
/// Falls back to `speak_text` if the phrase is not in the cache.
/// Emits `tts:audio-started` event before playback starts, same as `speak_text`.
#[tauri::command]
pub async fn speak_cached(
    text: String,
    state: State<'_, TtsState>,
    meeting: State<'_, Arc<MeetingState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    tracing::info!("tts: speaking cached '{}'", text);

    // 1. Mark TTS as playing to suppress wake word self-trigger
    meeting.set_tts_playing(true);

    let my_generation = TTS_GENERATION.load(Ordering::SeqCst);

    // 2. Try to get the cached audio. If not in cache, fall back to synthesis.
    let cache_arc = state.cache.clone();
    let cached_audio = {
        let cache = cache_arc.lock().await;
        cache.get(&text).cloned()
    };

    let (audio, sample_rate) = match cached_audio {
        Some((a, sr)) => {
            tracing::info!("tts: cache hit for '{}'", text);
            (a, sr)
        }
        None => {
            // Cache miss ΓÇö ensure engine is loaded, then synthesize on the fly.
            tracing::info!("tts: cache miss for '{}', synthesizing on demand", text);
            let engine_arc = state.engine.clone();
            let sample_rate_arc = state.sample_rate.clone();
            ensure_engine_loaded(&engine_arc, &cache_arc, &sample_rate_arc).await?;

            let length_scale = 0.87_f32; // match 1.15x speed
            let text_clone = text.clone();
            let ea = engine_arc.clone();
            let result = tokio::task::spawn_blocking(move || {
                let mut lock = ea.blocking_lock();
                if let Some(engine) = lock.as_mut() {
                    engine.create(&text_clone, false, None, Some(length_scale), None, None)
                        .map_err(|e| format!("TTS synthesis error: {}", e))
                } else {
                    Err("TTS Engine not initialized".to_string())
                }
            })
            .await
            .map_err(|e| format!("TTS task panicked: {}", e))??;

            // Update stored sample rate
            *sample_rate_arc.lock().await = result.1;
            result
        }
    };

    // Check if stop was requested during synthesis
    if TTS_GENERATION.load(Ordering::SeqCst) > my_generation {
        tracing::info!("tts: stop requested before cached playback, skipping");
        meeting.set_tts_playing(false);
        return Ok(());
    }

    // 3. Emit tts:audio-started event so the frontend can sync animations.
    let _ = app.emit("tts:audio-started", &text);

    // 4. Play audio from cached samples
    let play_result = tokio::task::spawn_blocking(move || {
        match tts_output_handle() {
            Ok(handle) => {
                match Sink::try_new(&handle) {
                    Ok(sink) => {
                        let source = SamplesBuffer::new(1, sample_rate, audio);
                        sink.append(source);
                        while !sink.empty() {
                            if TTS_GENERATION.load(Ordering::SeqCst) > my_generation {
                                sink.stop();
                                tracing::info!("tts: cached playback stopped by user (barge-in)");
                                return Ok(());
                            }
                            std::thread::sleep(std::time::Duration::from_millis(20));
                        }
                        tracing::info!("tts: cached audio playback completed");
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
        tracing::error!("tts: cached audio thread panicked");
        Err("Audio thread panicked".to_string())
    });

    // 5. Grace period
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    meeting.set_tts_playing(false);

    play_result
}
