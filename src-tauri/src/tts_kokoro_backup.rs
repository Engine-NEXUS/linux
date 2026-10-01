use crate::meeting_detect::MeetingState;
use kokoro_micro::TtsEngine;
use rodio::{buffer::SamplesBuffer, OutputStream, Sink};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::{Emitter, State};

pub struct TtsState {
    pub engine: Arc<Mutex<Option<TtsEngine>>>,
    /// Pre-synthesized short phrases for instant acknowledgment playback.
    /// Keyed by the exact phrase text. Stores 24kHz mono f32 PCM samples.
    pub cache: Arc<Mutex<HashMap<String, Vec<f32>>>>,
}

/// Phrases that are pre-synthesized on first Kokoro load for instant playback.
/// These are the high-frequency acknowledgment/error phrases that must play
    /// with zero synthesis delay to feel natural.
const CACHED_PHRASES: &[&str] = &[
    "On it sir",
    "Didn't understand that sir",
    "Didn't catch that sir",
    "Here is the analysis, sir",
    "Ok sir",
];

/// Lazily initialize the Kokoro TTS engine on first use.
/// Called from `speak_text` when the engine is `None`.
/// Saves ~350 MB RAM at idle by not loading Kokoro at boot.
/// First TTS call takes ~1.7s extra (one-time model load); subsequent calls are instant.
/// Also pre-synthesizes cached acknowledgment phrases for instant playback.
async fn ensure_engine_loaded(
    engine_arc: &Arc<Mutex<Option<TtsEngine>>>,
    cache_arc: &Arc<Mutex<HashMap<String, Vec<f32>>>>,
) -> Result<(), String> {
    // Fast path: already loaded
    if engine_arc.lock().await.is_some() {
        return Ok(());
    }

    tracing::info!("tts: lazy-loading Kokoro engine on first speak...");
    let start_time = std::time::Instant::now();

    // Set espeak-ng data path for kokoro-micro's espeak-rs dependency.
    // This is also set at startup in lib.rs::setup_espeak_data_path(),
    // but we keep it here as a fallback.
    let mut custom_model_path: Option<(std::path::PathBuf, std::path::PathBuf)> = None;
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            if std::env::var("PIPER_ESPEAKNG_DATA_DIRECTORY").is_err() {
                let espeak_parent = exe_dir.join("resources");
                if espeak_parent.join("espeak-ng-data").exists() {
                    std::env::set_var("PIPER_ESPEAKNG_DATA_DIRECTORY", &espeak_parent);
                    tracing::info!("tts: espeak-ng data path set to {}", espeak_parent.display());
                }
            }

            let res_model = exe_dir.join("resources").join("kokoro").join("0.onnx");
            let res_voices = exe_dir.join("resources").join("kokoro").join("0.bin");
            if res_model.exists() && res_voices.exists() {
                custom_model_path = Some((res_model, res_voices));
            }
        }
    }

    let engine_result = match custom_model_path {
        Some((m, v)) => {
            tracing::info!("tts: loading Kokoro models from resources: {}", m.display());
            kokoro_micro::TtsEngine::with_paths(
                m.to_str().unwrap_or("0.onnx"),
                v.to_str().unwrap_or("0.bin"),
            )
            .await
        }
        None => kokoro_micro::TtsEngine::new().await,
    };

    match engine_result {
        Ok(engine) => {
            *engine_arc.lock().await = Some(engine);
            tracing::info!(
                "tts: Kokoro engine lazy-loaded in {:.2}s",
                start_time.elapsed().as_secs_f32()
            );

            // Pre-synthesize cached phrases for instant acknowledgment playback.
            // This runs once, right after the engine loads, so all subsequent
            // speak_cached() calls are instant (<5ms) instead of 200-1700ms.
            pregenerate_cache(engine_arc, cache_arc).await;

            Ok(())
        }
        Err(e) => {
            tracing::error!("tts: failed to lazy-init Kokoro: {}", e);
            Err(format!("TTS engine init failed: {}", e))
        }
    }
}

/// Pre-synthesize cached phrases using the loaded Kokoro engine.
/// Runs once after engine load. Each phrase is synthesized at the default
/// speed and stored as f32 PCM samples. Total memory: ~528 KB for 5 phrases.
async fn pregenerate_cache(
    engine_arc: &Arc<Mutex<Option<TtsEngine>>>,
    cache_arc: &Arc<Mutex<HashMap<String, Vec<f32>>>>,
) {
    let cache_start = std::time::Instant::now();
    const KOKORO_INTERNAL_SPEED_SCALE: f32 = 0.65;
    let engine_spd = (1.15_f32 / KOKORO_INTERNAL_SPEED_SCALE).clamp(0.5, 3.0);

    let mut cached_count = 0;
    for phrase in CACHED_PHRASES {
        let p = phrase.to_string();
        let ea = engine_arc.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut lock = ea.blocking_lock();
            if let Some(engine) = lock.as_mut() {
                engine.synthesize_with_options(&p, Some("af_sky"), engine_spd, 1.0, Some("en"))
            } else {
                Err("TTS Engine not initialized".to_string())
            }
        })
        .await;

        match result {
            Ok(Ok(audio)) => {
                cache_arc.lock().await.insert(phrase.to_string(), audio);
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
/// to stop the current audio immediately. Using a generation counter prevents race
/// conditions where a new speech request clears a boolean flag before the previous
/// playback thread has polled it.
static TTS_GENERATION: AtomicUsize = AtomicUsize::new(0);

/// IPC: Stop any currently-playing TTS audio.
///
/// Increments a global generation counter that the playback thread in `speak_text` polls.
/// The playback thread calls `sink.stop()` and exits as soon as it sees a newer generation.
/// This provides near-instant barge-in when the user presses Ctrl+Space while NEXUS is speaking.
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

    // Capture the current TTS generation. If stop_tts is called after this,
    // the global generation will increment and we will abort playback.
    let my_generation = TTS_GENERATION.load(Ordering::SeqCst);

    // 2. Lazy-load Kokoro engine on first speak (saves ~350 MB at idle).
    //    First call: ~1.7s model load. Subsequent calls: instant (fast path).
    let engine_arc = state.engine.clone();
    let cache_arc = state.cache.clone();
    ensure_engine_loaded(&engine_arc, &cache_arc).await?;
    let voice_id = match voice.as_deref() {
        Some("default") | None => "af_sky".to_string(),
        Some(v) => v.to_string(),
    };
    let spd = speed.unwrap_or(1.15);
    const KOKORO_INTERNAL_SPEED_SCALE: f32 = 0.65;
    let engine_spd = (spd / KOKORO_INTERNAL_SPEED_SCALE).clamp(0.5, 3.0);

    // Read TTS volume setting (0-100, default 75).
    // 0 means "disabled" ΓÇö don't adjust system volume.
    let tts_volume_pct = read_tts_volume(&app);

    let text_for_event = text.clone();
    let audio = tokio::task::spawn_blocking(move || {
        let mut lock = engine_arc.blocking_lock();
        if let Some(engine) = lock.as_mut() {
            engine
                .synthesize_with_options(&text, Some(&voice_id), engine_spd, 1.0, Some("en"))
                .map_err(|e| format!("TTS synthesis error: {}", e))
        } else {
            Err("TTS Engine not initialized".to_string())
        }
    })
    .await
    .map_err(|e| format!("TTS task panicked: {}", e))??;

    // Check if stop was requested DURING synthesis — if so, skip playback
    if TTS_GENERATION.load(Ordering::SeqCst) > my_generation {
        tracing::info!("tts: stop requested during synthesis, skipping playback");
        // Do NOT set tts_playing=false — a newer generation is active.
        return Ok(());
    }

    // 3. Save current system volume and set to TTS volume (after synthesis,
    //    before playback ΓÇö so if synthesis fails, volume is never changed).
    let volume_changed = if tts_volume_pct > 0 {
        let target = tts_volume_pct as f32 / 100.0;
        crate::volume::save_and_set_volume(target)
    } else {
        false
    };

    // 4. Emit tts:audio-started event so the frontend can sync the orb
    //    hide + loading indicator show with actual audio playback.
    let _ = app.emit("tts:audio-started", &text_for_event);

    // 5. Play audio using spawn_blocking so the tokio runtime can still
    //    process other commands (like stop_tts) while audio plays.
    //    The old code used std::thread::spawn().join() which BLOCKED the
    //    tokio worker thread, preventing stop_tts from executing.
    let play_result = tokio::task::spawn_blocking(move || {
        match OutputStream::try_default() {
            Ok((_stream, handle)) => {
                match Sink::try_new(&handle) {
                    Ok(sink) => {
                        // Kokoro output is standard 24kHz mono PCM (f32)
                        let source = SamplesBuffer::new(1, 24000, audio);
                        sink.append(source);

                        // Poll for stop request instead of sink.sleep_until_end()
                        // so the user can barge-in with Ctrl+Space.
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

    // 6. Restore system volume to the saved value (in ALL exit paths:
    //    normal completion, barge-in, rodio error, thread panic).
    if volume_changed {
        crate::volume::restore_volume();
    }

    // 7. Grace period for acoustic settling before resuming wake word detection
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    meeting.set_tts_playing(false);

    play_result
}

/// IPC: Play a pre-cached TTS phrase instantly from memory.
///
/// Falls back to `speak_text` if the phrase is not in the cache (e.g. if
/// Kokoro failed to load or the cache hasn't been generated yet).
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

    // 2. Try to get the cached audio. If not in cache, fall back to speak_text.
    let cache_arc = state.cache.clone();
    let cached_audio = {
        let cache = cache_arc.lock().await;
        cache.get(&text).cloned()
    };

    let audio = match cached_audio {
        Some(a) => {
            tracing::info!("tts: cache hit for '{}'", text);
            a
        }
        None => {
            // Cache miss ΓÇö ensure engine is loaded, then synthesize on the fly.
            // This is the slow path (~200-1700ms) but it's correct.
            tracing::info!("tts: cache miss for '{}', synthesizing on demand", text);
            let engine_arc = state.engine.clone();
            ensure_engine_loaded(&engine_arc, &cache_arc).await?;

            const KOKORO_INTERNAL_SPEED_SCALE: f32 = 0.65;
            let engine_spd = (1.15_f32 / KOKORO_INTERNAL_SPEED_SCALE).clamp(0.5, 3.0);
            let text_clone = text.clone();
            let ea = engine_arc.clone();
            tokio::task::spawn_blocking(move || {
                let mut lock = ea.blocking_lock();
                if let Some(engine) = lock.as_mut() {
                    engine.synthesize_with_options(&text_clone, Some("af_sky"), engine_spd, 1.0, Some("en"))
                        .map_err(|e| format!("TTS synthesis error: {}", e))
                } else {
                    Err("TTS Engine not initialized".to_string())
                }
            })
            .await
            .map_err(|e| format!("TTS task panicked: {}", e))??
        }
    };

    // Check if stop was requested during synthesis
    if TTS_GENERATION.load(Ordering::SeqCst) > my_generation {
        tracing::info!("tts: stop requested before cached playback, skipping");
        // Do NOT set tts_playing=false — a newer generation is active.
        return Ok(());
    }

    // 3. Read TTS volume setting
    let tts_volume_pct = read_tts_volume(&app);
    let volume_changed = if tts_volume_pct > 0 {
        let target = tts_volume_pct as f32 / 100.0;
        crate::volume::save_and_set_volume(target)
    } else {
        false
    };

    // 4. Emit tts:audio-started event so the frontend can sync animations.
    let _ = app.emit("tts:audio-started", &text);

    // 5. Play audio from cached samples
    let play_result = tokio::task::spawn_blocking(move || {
        match OutputStream::try_default() {
            Ok((_stream, handle)) => {
                match Sink::try_new(&handle) {
                    Ok(sink) => {
                        let source = SamplesBuffer::new(1, 24000, audio);
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

    // 6. Restore volume
    if volume_changed {
        crate::volume::restore_volume();
    }

    // 7. Grace period
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    meeting.set_tts_playing(false);

    play_result
}

/// Read the TTS volume setting from settings.json.
/// Returns 0-100. 0 means "disabled" (don't adjust system volume).
fn read_tts_volume(app: &tauri::AppHandle) -> u8 {
    use tauri::Manager;
    let dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(_) => return 75, // default
    };
    let path = dir.join("settings.json");
    if !path.exists() {
        return 75; // default
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return 75,
    };
    // Parse just the tts_volume field ΓÇö don't deserialize the whole struct
    // to avoid coupling to the NexusSettings struct in the tts module.
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
        if let Some(vol) = json.get("ttsVolume").and_then(|v| v.as_u64()) {
            return vol as u8;
        }
        // Also check snake_case in case of older format
        if let Some(vol) = json.get("tts_volume").and_then(|v| v.as_u64()) {
            return vol as u8;
        }
    }
    75 // default
}
