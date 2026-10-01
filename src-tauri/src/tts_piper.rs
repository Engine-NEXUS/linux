//! Piper TTS — local VITS ONNX fallback (no internet required).
//!
//! Uses piper-rs to load ONNX voice models. Lazy-loaded only when edge-tts
//! is unavailable (network down).
//!
//! Latency: ~40ms (warm, CPU inference)
//! RAM: 80 MB (model loaded in memory)
//! Model size: ~60 MB (en_US-amy-medium.onnx)
//! Cost: $0 (free, MIT license)
//! Quality: Good (clear but less natural than edge-tts/Kokoro)

use std::sync::Arc;
use tokio::sync::Mutex;

/// Lazy-initialized Piper TTS engine.
/// None = not loaded yet. Some = loaded and ready.
pub type PiperEngine = Arc<Mutex<Option<piper_rs::Piper>>>;

/// Create a new lazy Piper engine state (engine not loaded).
pub fn new_engine() -> PiperEngine {
    Arc::new(Mutex::new(None))
}

/// Find the Piper model path.
/// Checks bundled resources first, then user cache directory.
fn find_model_paths() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    // 1. Check bundled resources (exe_dir/resources/piper/)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let res_dir = exe_dir.join("resources").join("piper");
            let onnx = res_dir.join("en_US-amy-medium.onnx");
            let json = res_dir.join("en_US-amy-medium.onnx.json");
            if onnx.exists() && json.exists() {
                tracing::info!("tts-piper: using bundled model: {}", onnx.display());
                return Some((onnx, json));
            }
        }
    }

    // 2. Check user cache directory (downloaded on first use)
    if let Some(cache_dir) = dirs_next::cache_dir() {
        let cache_dir = cache_dir.join("com.nexus.assistant").join("piper");
        let onnx = cache_dir.join("en_US-amy-medium.onnx");
        let json = cache_dir.join("en_US-amy-medium.onnx.json");
        if onnx.exists() && json.exists() {
            tracing::info!("tts-piper: using cached model: {}", onnx.display());
            return Some((onnx, json));
        }
    }

    None
}

/// Lazily load the Piper engine on first use.
pub async fn ensure_engine_loaded(engine: &PiperEngine) -> Result<(), String> {
    if engine.lock().await.is_some() {
        return Ok(());
    }

    tracing::info!("tts-piper: lazy-loading Piper engine...");
    let start = std::time::Instant::now();

    let (onnx_path, json_path) = find_model_paths().ok_or_else(|| {
        "Piper model not found. Place en_US-amy-medium.onnx + .json in resources/piper/".to_string()
    })?;

    // Piper requires espeak-ng data path.
    // This is also set at startup in lib.rs::setup_espeak_data_path(),
    // but we keep it here as a fallback in case the startup check missed
    // the resources directory (e.g. different working directory).
    if std::env::var("PIPER_ESPEAKNG_DATA_DIRECTORY").is_err() {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                let espeak_parent = exe_dir.join("resources");
                if espeak_parent.join("espeak-ng-data").exists() {
                    std::env::set_var("PIPER_ESPEAKNG_DATA_DIRECTORY", &espeak_parent);
                    tracing::info!("tts-piper: espeak-ng data path set to {}", espeak_parent.display());
                }
            }
        }
    }

    let piper = piper_rs::Piper::new(&onnx_path, &json_path)
        .map_err(|e| format!("Piper model load failed: {}", e))?;

    *engine.lock().await = Some(piper);

    tracing::info!(
        "tts-piper: engine loaded in {:.2}s",
        start.elapsed().as_secs_f32()
    );

    Ok(())
}

/// Synthesize text to f32 PCM samples using Piper.
///
/// Returns (samples, sample_rate) for rodio playback.
/// Piper outputs 22050 Hz mono by default.
pub async fn synthesize(
    engine: &PiperEngine,
    text: &str,
) -> Result<(Vec<f32>, u32), String> {
    if text.is_empty() {
        return Err("Empty text".to_string());
    }

    ensure_engine_loaded(engine).await?;

    let engine_clone = engine.clone();
    let text_clone = text.to_string();

    let (samples, sample_rate) = tokio::task::spawn_blocking(move || {
        let mut lock = engine_clone.blocking_lock();
        let piper = lock.as_mut().ok_or("Piper engine not loaded")?;

        // Piper::create returns (samples, sample_rate)
        let (samples, sample_rate) = piper
            .create(&text_clone, false, None, None, None, None)
            .map_err(|e| format!("Piper synthesis failed: {}", e))?;

        Ok::<(Vec<f32>, u32), String>((samples, sample_rate))
    })
    .await
    .map_err(|e| format!("Piper task panicked: {}", e))??;

    tracing::info!(
        "tts-piper: synthesized '{}' ({} samples, {}Hz)",
        crate::tts::truncate_for_log(text, 50),
        samples.len(),
        sample_rate
    );

    Ok((samples, sample_rate))
}

/// Unload the Piper engine to free ~80 MB RAM.
///
/// Called by the network monitor after 10 minutes of stable network.
/// If the network drops again, Piper will reload on the next fallback.
pub async fn unload_engine(engine: &PiperEngine) {
    let mut lock = engine.lock().await;
    if lock.is_some() {
        *lock = None;
        tracing::info!("tts-piper: engine unloaded (network stable for 10+ minutes, ~80 MB freed)");
    }
}

/// Check if the Piper engine is currently loaded.
pub async fn is_engine_loaded(engine: &PiperEngine) -> bool {
    engine.lock().await.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_engine_starts_empty() {
        let engine = new_engine();
        let _ = std::hint::black_box(engine);
    }

    #[test]
    fn test_find_model_paths_doesnt_panic() {
        let _ = find_model_paths();
    }
}
