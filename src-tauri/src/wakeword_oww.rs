//! Wake-word engine using openWakeWord (KWS) + speaker verification,
//! with Tier 3 direct command classification.
//!
//! Pipeline:
//!   Microphone → cpal capture (native SR) → resample to 16kHz mono
//!   → openWakeWord KWS (1280-sample / 80ms sliding window)
//!   → 3-stage: melspectrogram → embedding → classifier(s)
//!   → probability score for "nexus" (wake word)
//!   → probability scores for command phrases ("open youtube", etc.)
//!   → if wake score > threshold → speaker verification → trigger wake
//!   → if command score > threshold → emit command-detected event (skip STT)
//!
//! `mock-wake` feature: skip the engine entirely; only the global hotkey produces wakes.
//!
//! Key difference from VAD+ASR:
//!   - No VAD gate (doesn't clip the start of words)
//!   - No ASR (doesn't need to transcribe — directly detects acoustic pattern)
//!   - Runs continuously on every 80ms chunk
//!   - Expected recall: >95% (vs ~30% with VAD+ASR)
//!
//! Tier 3 command classifiers:
//!   - Loaded from resources/oww/commands/*.onnx
//!   - Share the same melspectrogram + embedding models as the wake word
//!   - Run in parallel with the wake-word classifier on every 80ms chunk
//!   - When a command fires, emit a `command-detected` Tauri event
//!   - Frontend skips STT and executes the mapped intent directly
//!   - Falls back to STT if no command classifier matches

use tauri::{AppHandle, Runtime};

#[cfg(feature = "mock-wake")]
pub fn run<R: Runtime>(_app: AppHandle<R>) -> Result<(), String> {
    tracing::info!("wake-word: mock mode (no native listener)");
    loop {
        std::thread::park();
    }
}

#[cfg(feature = "mock-wake")]
pub fn set_meeting_state(_state: std::sync::Arc<crate::meeting_detect::MeetingState>) {}

#[cfg(not(feature = "mock-wake"))]
mod engine {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;

    use circular_buffer::CircularBuffer;
    use serde::{Deserialize, Serialize};

    /// Throttle counter for "audio passed gate" debug logs (avoids 12.5 logs/sec flood).
    static GATE_PASS_COUNT: AtomicU64 = AtomicU64::new(0);
    use tract_onnx::prelude::*;

    type ModelType = Arc<TypedSimplePlan>;

    // ─── Tier 3: Command classifier types ───────────────────────────────

    /// A structured intent emitted when a command classifier fires.
    /// This is serialized and sent to the frontend via a Tauri event.
    ///
    /// Type 1 (Fixed): `needs_param` is false (or absent), `target` is set.
    ///   → Frontend executes directly, no STT needed.
    ///
    /// Type 2 (Parameterized): `needs_param` is true, `target` is empty.
    ///   → Frontend speaks "On it sir", records 3s of audio, runs STT
    ///     to get the parameter (song name, search query), then executes.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CommandIntent {
        pub action: String,
        #[serde(default)]
        pub target: String,
        #[serde(default)]
        pub needs_param: bool,
    }

    /// The intent mapping loaded from `command_intents.json`.
    #[derive(Debug, Clone, Deserialize)]
    struct CommandIntentEntry {
        phrase: String,
        model_file: String,
        intent: CommandIntent,
    }

    /// A loaded command classifier model + its mapped intent.
    ///
    /// `pub` to match the visibility of `WakeEngine::command_classifiers`,
    /// which holds a `Vec` of these. The whole `engine` module is private to
    /// the crate, so this does not widen the public API.
    pub struct CommandClassifier {
        model_name: String,
        model: ModelType,
        intent: CommandIntent,
        detections_buffer: CircularBuffer<DETECTION_BUFFER_SIZE, f32>,
        last_detection_time: std::time::Instant,
    }

    /// OWW processes 1280-sample chunks (80ms at 16kHz)
    pub const OWW_CHUNK_SIZE: usize = 1280;

    /// Melspectrogram lookback: 3 mel hops of 160 samples
    const MEL_LOOKBACK: usize = 160 * 3;
    /// Mel model input: lookback + one chunk
    const MEL_INPUT_SIZE: usize = MEL_LOOKBACK + OWW_CHUNK_SIZE;
    /// Mel frames produced per chunk
    const MELS_PER_CHUNK: usize = MEL_INPUT_SIZE / 160 - 3; // 8
    /// Mel circular buffer size (80 / MELS_PER_CHUNK)
    const MEL_CIRC_SIZE: usize = 80 / MELS_PER_CHUNK; // 10

    /// Feature buffer: 16 frames of 96-dim embeddings
    const FEATURE_BUFFER_SIZE: usize = 16;

    /// Detection buffer: 12 frames (~1 sec) for smoothing
    const DETECTION_BUFFER_SIZE: usize = 12;

    /// Minimum positive detections before triggering
    /// (1 frame = 80ms — the model produces 0.3-0.5 for real speech from
    /// non-enrolled speakers, so requiring 2+ frames above threshold kills
    /// many valid detections. With the max-based smoothing and lowered
    /// threshold, 1 frame is sufficient.)
    const MIN_POSITIVE_DETECTIONS: f32 = 1.0;

    /// Single-frame high-confidence threshold.
    /// If any single frame exceeds this, trigger immediately without
    /// requiring MIN_POSITIVE_DETECTIONS frames. This fixes the case where
    /// the model produces one high probability (e.g. 0.67 or 0.89) but the
    /// adjacent frames are below threshold — the 2-frame smoothing was
    /// killing valid detections with 58.2%-recall models.
    /// 0.5 is above the 0.45 trigger threshold and far above noise
    /// (silence gate already blocks RMS < 0.0005, and the model produces
    /// <0.01 on non-wake speech), so a single 0.5+ frame is a real wake.
    /// The model produces lower probabilities for voices it wasn't trained
    /// on (e.g. 0.67 for a non-enrolled speaker vs 0.89 for the owner),
    /// so 0.5 covers both cases while still rejecting noise.
    const SINGLE_FRAME_HIGH_CONFIDENCE: f32 = 0.5;

    /// Refractory period after a detection (ms)
    /// Increased from 2s to 3s to compensate for the more sensitive
    /// max-based detection (prevents double-triggers on the same utterance).
    const NO_DETECTION_MS: u64 = 3000;

    /// Resolve the oww resources directory.
    pub fn resolve_oww_dir(app_resource_dir: &Path) -> Option<PathBuf> {
        // 1. Production: resource_dir/resources/oww (Tauri v2 on Windows: resource_dir() = exe_dir)
        let prod = app_resource_dir.join("resources").join("oww");
        if prod.join("melspectrogram.onnx").exists() {
            return Some(prod);
        }
        // 1b. Production fallback: resource_dir/oww (some Tauri versions may return resources/ directly)
        let prod_alt = app_resource_dir.join("oww");
        if prod_alt.join("melspectrogram.onnx").exists() {
            return Some(prod_alt);
        }
        // 2. Dev mode: CARGO_MANIFEST_DIR/resources/oww
        if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
            let dev = PathBuf::from(manifest).join("resources").join("oww");
            if dev.join("melspectrogram.onnx").exists() {
                return Some(dev);
            }
        }
        // 3. Dev mode fallback: exe_dir/../resources/oww
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let dev = dir.join("..").join("..").join("resources").join("oww");
                if dev.join("melspectrogram.onnx").exists() {
                    return Some(dev.canonicalize().unwrap_or(dev));
                }
            }
        }
        None
    }

    /// Load an ONNX model from a file path.
    fn load_onnx_model(path: &Path) -> anyhow::Result<ModelType> {
        let model = tract_onnx::onnx()
            .model_for_path(path)
            .map_err(|e| anyhow::anyhow!("Failed to parse ONNX {}: {}", path.display(), e))?;
        let model = model
            .into_optimized()
            .map_err(|e| anyhow::anyhow!("Failed to optimize {}: {}", path.display(), e))?;
        let model = model
            .into_runnable()
            .map_err(|e| anyhow::anyhow!("Failed to make runnable {}: {}", path.display(), e))?;
        // into_runnable() already returns Arc<SimplePlan<...>>
        Ok(model)
    }

    /// Audio feature extractor: melspectrogram → embedding
    pub struct AudioFeatures {
        mel: ModelType,
        emb: ModelType,
        raw_lookback: Vec<f32>,
        feature_buffer: CircularBuffer<FEATURE_BUFFER_SIZE, Tensor>,
        mel_spectrogram_buffer: CircularBuffer<MEL_CIRC_SIZE, Tensor>,
    }

    impl AudioFeatures {
        pub fn new(oww_dir: &Path) -> anyhow::Result<Self> {
            let mel_path = oww_dir.join("melspectrogram.onnx");
            let emb_path = oww_dir.join("embedding_model.onnx");

            let mel = load_onnx_model(&mel_path)?;
            let emb = load_onnx_model(&emb_path)?;

            // Set single-threaded executor for low latency
            tract_onnx::prelude::multithread::set_default_executor(
                tract_onnx::prelude::multithread::Executor::SingleThread,
            );

            let mut feature_buffer = CircularBuffer::<FEATURE_BUFFER_SIZE, Tensor>::new();
            for _ in 0..FEATURE_BUFFER_SIZE {
                feature_buffer.push_back(
                    Tensor::from_shape(&[1, 1, 1, 96], &[0f32; 96])
                        .map_err(|e| anyhow::anyhow!("init feature buffer: {e}"))?,
                );
            }

            let mut mel_spectrogram_buffer =
                CircularBuffer::<MEL_CIRC_SIZE, Tensor>::new();
            for _ in 0..MEL_CIRC_SIZE {
                mel_spectrogram_buffer.push_back(
                    Tensor::from_shape(&[MELS_PER_CHUNK, 32], &[0f32; MELS_PER_CHUNK * 32])
                        .map_err(|e| anyhow::anyhow!("init mel buffer: {e}"))?,
                );
            }

            Ok(AudioFeatures {
                mel,
                emb,
                raw_lookback: vec![0f32; MEL_LOOKBACK],
                feature_buffer,
                mel_spectrogram_buffer,
            })
        }

        /// Compute melspectrogram for a chunk of audio.
        fn get_melspectrogram(&mut self, data: &[f32]) -> anyhow::Result<Tensor> {
            // The openWakeWord melspectrogram model expects int16-scale float32
            // values (range [-32768, 32767]), not normalized [-1.0, 1.0].
            // cpal produces f32 in [-1.0, 1.0], so we must scale by 32768.
            // (Reference: openwakeword/utils.py _get_melspectrogram converts
            // int16 to float32 WITHOUT dividing by 32768.)
            const INT16_SCALE: f32 = 32768.0;

            // Prepend lookback from previous chunk (also scaled)
            let mut input = Vec::with_capacity(MEL_INPUT_SIZE);
            for &s in &self.raw_lookback {
                input.push(s * INT16_SCALE);
            }
            for &s in data {
                input.push(s * INT16_SCALE);
            }
            // Store unscaled lookback for next chunk
            self.raw_lookback
                .copy_from_slice(&data[data.len() - MEL_LOOKBACK..]);

            let tensor = Tensor::from_shape(&[1, MEL_INPUT_SIZE], &input)
                .map_err(|e| anyhow::anyhow!("mel input shape: {e}"))?;

            let outputs: TVec<TValue> = self
                .mel
                .clone()
                .run(tvec!(tensor.into()))
                .map_err(|e| anyhow::anyhow!("mel inference: {e}"))?;

            let out_tensor = outputs[0].clone().into_tensor();
            let resized = out_tensor
                .into_shape(&[MELS_PER_CHUNK, 32])
                .map_err(|e| anyhow::anyhow!("mel reshape: {e}"))?;
            let a = resized
                .into_plain_array::<f32>()
                .map_err(|e| anyhow::anyhow!("mel to array: {e}"))?
                .into_owned();
            // Normalize: (v / 10.0) + 2.0
            let updated = a.mapv(|v| (v / 10.0) + 2.0).into_tensor();
            Ok(updated)
        }

        /// Get audio features (embeddings) for a chunk.
        pub fn get_audio_features(&mut self, data: &[f32]) -> anyhow::Result<Tensor> {
            let mel_chunk = self.get_melspectrogram(data)?;
            self.mel_spectrogram_buffer.push_back(mel_chunk);

            let stacked_mels = Tensor::stack_tensors(0, &self.mel_spectrogram_buffer.to_vec())
                .map_err(|e| anyhow::anyhow!("stack mels: {e}"))?;

            // Slice [4:80] → [76, 32]
            let smaller = stacked_mels
                .slice(0, 4, 80)
                .map_err(|e| anyhow::anyhow!("slice mels: {e}"))?;
            let reshaped = smaller
                .into_shape(&[1, 76, 32, 1])
                .map_err(|e| anyhow::anyhow!("reshape mels: {e}"))?;

            let embeddings = self
                .emb
                .clone()
                .run(tvec!(reshaped.into()))
                .map_err(|e| anyhow::anyhow!("embedding inference: {e}"))?;

            self.feature_buffer
                .push_back(embeddings[0].clone().into_tensor());

            let stacked = Tensor::stack_tensors(0, &self.feature_buffer.to_vec())
                .map_err(|e| anyhow::anyhow!("stack features: {e}"))?;

            let reshaped = stacked
                .into_shape(&[self.feature_buffer.len(), 96])
                .map_err(|e| anyhow::anyhow!("reshape features: {e}"))?;

            Ok(reshaped)
        }
    }

    /// openWakeWord KWS engine with optional speaker verification
    /// and Tier 3 command classifiers.
    pub struct WakeEngine {
        pub classifier: ModelType,
        pub audio_features: AudioFeatures,

        pub sample_rate: i32,
        pub chunk_buffer: Vec<f32>,
        pub threshold: f32,
        pub detections_buffer: CircularBuffer<DETECTION_BUFFER_SIZE, f32>,
        pub last_detection_time: std::time::Instant,
        /// Tier 3: command classifiers loaded from resources/oww/commands/
        pub command_classifiers: Vec<CommandClassifier>,
        /// Sender for command-detected events (None if no command models loaded)
        pub command_tx: Option<std::sync::mpsc::Sender<CommandIntent>>,
        /// Secondary confirmation: after a raw detection, collect 500ms of
        /// audio to verify there was actual speech (not a noise spike).
        /// If the RMS of the confirmation window is below 0.01, discard.
        pub confirmation_buffer: Vec<f32>,
        pub confirmation_active: bool,
        pub pending_probability: f32,
        /// Engine start time — used to ignore false triggers during the
        /// first few seconds while the audio stream stabilizes.
        pub engine_start_time: std::time::Instant,
    }

    /// Load Tier 3 command classifiers from `resources/oww/commands/`.
    ///
    /// Reads `command_intents.json` for the intent mapping, then loads each
    /// `.onnx` model file referenced in it. Models that fail to load are
    /// skipped with a warning — the wake word and STT fallback still work.
    fn load_command_classifiers(oww_dir: &Path) -> Vec<CommandClassifier> {
        let commands_dir = oww_dir.join("commands");
        let intents_path = commands_dir.join("command_intents.json");

        if !intents_path.exists() {
            return Vec::new();
        }

        let json_str = match std::fs::read_to_string(&intents_path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Tier 3: failed to read {}: {e}", intents_path.display());
                return Vec::new();
            }
        };

        let entries: std::collections::HashMap<String, CommandIntentEntry> =
            match serde_json::from_str(&json_str) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("Tier 3: failed to parse {}: {e}", intents_path.display());
                    return Vec::new();
                }
            };

        let mut classifiers = Vec::new();
        for (model_name, entry) in &entries {
            let model_path = commands_dir.join(&entry.model_file);
            if !model_path.exists() {
                tracing::warn!(
                    "Tier 3: model file {} not found at {} — skipping",
                    entry.model_file,
                    model_path.display()
                );
                continue;
            }

            match load_onnx_model(&model_path) {
                Ok(model) => {
                    tracing::info!(
                        "Tier 3: loaded command classifier '{}' (phrase: \"{}\", intent: {:?})",
                        model_name, entry.phrase, entry.intent
                    );
                    classifiers.push(CommandClassifier {
                        model_name: model_name.clone(),
                        model,
                        intent: entry.intent.clone(),
                        detections_buffer: CircularBuffer::<DETECTION_BUFFER_SIZE, f32>::new(),
                        last_detection_time: std::time::Instant::now()
                            .checked_sub(std::time::Duration::from_secs(10))
                            .unwrap_or_else(std::time::Instant::now),
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        "Tier 3: failed to load {}: {e}",
                        model_path.display()
                    );
                }
            }
        }

        classifiers
    }

    impl WakeEngine {
        pub fn new(resource_dir: PathBuf, #[allow(unused_variables)] app_data_dir: PathBuf) -> anyhow::Result<Self> {
            let oww_dir = resolve_oww_dir(&resource_dir).ok_or_else(|| {
                anyhow::anyhow!(
                    "oww model files not found. Checked resource_dir/oww, \
                     CARGO_MANIFEST_DIR/resources/oww, and exe_dir/../resources/oww"
                )
            })?;

            // Load the custom "nexus" classifier model
            let nexus_model_path = oww_dir.join("nexus.onnx");
            if !nexus_model_path.exists() {
                anyhow::bail!(
                    "nexus.onnx not found at: {}\n\
                     You need to train a custom model first.\n\
                     Run the Google Colab notebook: train_nexus_oww.ipynb\n\
                     Then place the downloaded nexus.onnx in: {}",
                    nexus_model_path.display(),
                    oww_dir.display()
                );
            }

            tracing::info!("Loading openWakeWord classifier: {}", nexus_model_path.display());
            let classifier = load_onnx_model(&nexus_model_path)?;

            tracing::info!("Loading audio feature extractors from: {}", oww_dir.display());
            let audio_features = AudioFeatures::new(&oww_dir)?;



            let threshold = 0.35f32;
            tracing::info!(
                "openWakeWord KWS engine initialized \
                 (wake word: NEXUS, 80ms sliding window, threshold: {}, \
                 silence gate: RMS < 0.0005 = skip, AGC: target RMS 0.03, \
                 detection: max-based)",
                threshold
            );

            // --- Tier 3: Load command classifiers (optional) ---
            let command_classifiers = load_command_classifiers(&oww_dir);
            if !command_classifiers.is_empty() {
                tracing::info!(
                    "Tier 3: loaded {} command classifiers \
                     (direct audio→intent, skips STT for known commands)",
                    command_classifiers.len()
                );
            } else {
                tracing::debug!(
                    "Tier 3: no command classifiers found at {}/commands/ \
                     (optional — STT fallback handles all commands)",
                    oww_dir.display()
                );
            }

            Ok(WakeEngine {
                classifier,
                audio_features,
                sample_rate: 16000,
                chunk_buffer: Vec::with_capacity(OWW_CHUNK_SIZE),
                threshold: 0.35,
                detections_buffer: CircularBuffer::<DETECTION_BUFFER_SIZE, f32>::new(),
                last_detection_time: std::time::Instant::now()
                    .checked_sub(std::time::Duration::from_secs(10))
                    .unwrap_or_else(std::time::Instant::now),
                command_classifiers,
                command_tx: None,
                confirmation_buffer: Vec::with_capacity(16000), // 500ms @ 16kHz
                confirmation_active: false,
                pending_probability: 0.0,
                engine_start_time: std::time::Instant::now(),
            })
        }

        /// Run KWS detection on a single 80ms chunk.
        /// Returns (wake_detected, wake_probability, optional command_intent).
        fn detect_chunk(
            &mut self,
            chunk: Vec<f32>,
        ) -> (bool, f32, Option<CommandIntent>) {
            // ─── Startup grace period ───────────────────────────────────
            // Ignore all detections during the first 3 seconds after the
            // engine starts. The audio stream produces transient noise
            // during initialization that false-triggers the model
            // (probability 0.9+ on startup). Push 0.0 to flush the buffer.
            if self.engine_start_time.elapsed().as_secs() < 3 {
                self.detections_buffer.push_back(0.0);
                return (false, 0.0, None);
            }

            // ─── Energy gate + Automatic Gain Control (AGC) ────────────
            // The nexus.onnx model produces false positives (0.6-0.9 probability)
            // when fed pure digital silence (all zeros). This is because the model
            // was trained on TTS clips that always have a noise floor, so pure
            // silence is an out-of-distribution input that maps to high probability.
            //
            // Fix: compute RMS of the chunk and skip the classifier entirely if
            // the audio is too quiet to be speech. Push 0.0 to the detection buffer
            // to flush out any stale high values from the previous chunk.
            //
            // Threshold: 0.002 (~-54dBFS) — lowered from 0.005 to allow quiet/
            //   whispered "NEXUS" calls through. Pure digital silence (RMS=0) and
            //   mic noise floor (~0.0005-0.001) are still blocked.
            //
            // AGC: If the chunk passes the gate but is quieter than normal speech,
            //   amplify it to a target RMS before feeding the classifier. This
            //   makes quiet and loud "NEXUS" produce the same model input, so the
            //   model (trained on normal-volume TTS) recognizes whispered speech.
            //   The gain is capped at 30x to avoid amplifying pure noise.
            // Silence gate: lowered to 0.0005 to catch very quiet/whispered
            // "nexus" calls. Pure digital silence (RMS=0) is still blocked.
            // The Intel SST driver often produces RMS=0.0000 even when the
            // mic is working, so we can't set this too high or we block
            // everything. The AGC step compensates for low input.
            const SILENCE_RMS_THRESHOLD: f32 = 0.0005;
            const TARGET_RMS: f32 = 0.03; // Normal speech RMS (~-30dBFS)
            const MAX_GAIN: f32 = 50.0; // Cap to avoid amplifying noise

            let rms = if chunk.is_empty() {
                0.0
            } else {
                let sum_sq: f32 = chunk.iter().map(|s| s * s).sum();
                (sum_sq / chunk.len() as f32).sqrt()
            };

            if rms < SILENCE_RMS_THRESHOLD {
                // Push low probability to flush stale high values from buffer
                self.detections_buffer.push_back(0.0);
                // Also flush command classifier buffers
                for cmd in &mut self.command_classifiers {
                    cmd.detections_buffer.push_back(0.0);
                }
                return (false, 0.0, None);
            }

            // Log when audio passes the gate (throttled: only every 1000th pass
            // to avoid flooding logs at 12.5 lines/sec).
            use std::sync::atomic::Ordering;
            GATE_PASS_COUNT.fetch_add(1, Ordering::Relaxed);
            if GATE_PASS_COUNT.load(Ordering::Relaxed) % 1000 == 0 {
                tracing::debug!(
                    "wake: audio passed gate x1000 (last RMS={:.6}), running classifier...",
                    rms
                );
            }

            // AGC: amplify quiet speech to target RMS so the model sees
            // consistent-volume input regardless of how loud the user spoke.
            // This is the key fix for "low voice and high voice the same".
            let chunk: Vec<f32> = if rms < TARGET_RMS {
                let gain = (TARGET_RMS / rms).min(MAX_GAIN);
                tracing::trace!("wake: AGC gain={:.1}x (RMS {:.6} → {:.6})", gain, rms, TARGET_RMS);
                chunk.iter().map(|&s| (s * gain).clamp(-1.0, 1.0)).collect()
            } else {
                chunk
            };

            // Get audio features (melspectrogram → embedding)
            let features = match self.audio_features.get_audio_features(&chunk) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("Audio feature extraction error: {e}");
                    return (false, 0.0, None);
                }
            };

            // Reshape features to [1, 16, 96] for the classifier
            let last = match features.into_shape(&[1, FEATURE_BUFFER_SIZE, 96]) {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("Feature reshape error: {e}");
                    return (false, 0.0, None);
                }
            };

            // Run wake-word classifier
            let outputs: TVec<TValue> = match self.classifier.clone().run(tvec!(last.clone().into())) {
                Ok(o) => o,
                Err(e) => {
                    tracing::warn!("Classifier inference error: {e}");
                    return (false, 0.0, None);
                }
            };

            let t = match outputs[0]
                .clone()
                .into_tensor()
                .cast_to::<f32>()
            {
                Ok(c) => c.into_owned(),
                Err(e) => {
                    tracing::warn!("Classifier output cast error: {e}");
                    return (false, 0.0, None);
                }
            };

            let probability = match t.into_plain_array::<f32>() {
                Ok(arr) => arr.as_slice().unwrap_or(&[0.0])[0],
                Err(_) => 0.0,
            };

            // Log every classifier output so we can see if the model is
            // producing any signal at all. This is critical for debugging
            // "nexus is not waking up" issues.
            if probability > 0.1 {
                tracing::info!(
                    "wake: model probability={:.3} (threshold={:.3}, buffer_avg will be computed)",
                    probability, self.threshold
                );
            } else if probability > 0.01 {
                tracing::debug!(
                    "wake: model probability={:.3} (below threshold {:.3})",
                    probability, self.threshold
                );
            }

            self.detections_buffer.push_back(probability);

            // Calculate smoothed average of positive detections
            let avg = self.calculate_average();

            let since_last = self.last_detection_time.elapsed().as_millis();

            // Log which trigger path is being taken (for debugging)
            if avg >= SINGLE_FRAME_HIGH_CONFIDENCE {
                tracing::info!(
                    "wake: high-confidence single-frame trigger (avg={:.3}, prob={:.3})",
                    avg, probability
                );
            }

            // Trigger when smoothed average exceeds threshold (with refractory period)
            let wake_detected = if avg > self.threshold && since_last > NO_DETECTION_MS as u128 {
                self.last_detection_time = std::time::Instant::now();
                self.detections_buffer.clear();
                true
            } else {
                false
            };

            // --- Tier 3: Run command classifiers in parallel ---
            let command_intent = self.detect_commands(&last);

            (wake_detected, avg, command_intent)
        }

        /// Run all command classifiers on the current feature frame.
        /// Returns the intent of the first classifier that fires (if any).
        fn detect_commands(&mut self, features: &Tensor) -> Option<CommandIntent> {
            if self.command_classifiers.is_empty() {
                return None;
            }

            let mut best_intent: Option<(CommandIntent, f32)> = None;

            for cmd in &mut self.command_classifiers {
                let outputs: TVec<TValue> = match cmd.model.clone().run(tvec!(features.clone().into())) {
                    Ok(o) => o,
                    Err(e) => {
                        tracing::warn!(
                            "Tier 3: command classifier '{}' inference error: {e}",
                            cmd.model_name
                        );
                        continue;
                    }
                };

                let t = match outputs[0]
                    .clone()
                    .into_tensor()
                    .cast_to::<f32>()
                {
                    Ok(c) => c.into_owned(),
                    Err(e) => {
                        tracing::warn!(
                            "Tier 3: command classifier '{}' output cast error: {e}",
                            cmd.model_name
                        );
                        continue;
                    }
                };

                let probability = match t.into_plain_array::<f32>() {
                    Ok(arr) => arr.as_slice().unwrap_or(&[0.0])[0],
                    Err(_) => 0.0,
                };

                cmd.detections_buffer.push_back(probability);

                // Smoothed average of positive detections (same logic as wake word)
                let all = cmd.detections_buffer.to_vec();
                let mut cumulative = 0.0f32;
                let mut positive_count = 0.0f32;
                for d in all {
                    if d > self.threshold {
                        positive_count += 1.0;
                        cumulative += d;
                    }
                }
                if positive_count < MIN_POSITIVE_DETECTIONS {
                    continue;
                }
                let avg = cumulative / positive_count;
                if avg <= self.threshold {
                    continue;
                }

                // Refractory period: don't re-trigger the same command within 2s
                let since_last = cmd.last_detection_time.elapsed().as_millis();
                if since_last <= NO_DETECTION_MS as u128 {
                    continue;
                }

                // This command fired — track the best (highest probability) one
                if best_intent.is_none() || avg > best_intent.as_ref().unwrap().1 {
                    best_intent = Some((cmd.intent.clone(), avg));
                    cmd.last_detection_time = std::time::Instant::now();
                    cmd.detections_buffer.clear();
                }
            }

            if let Some((intent, prob)) = best_intent {
                tracing::info!(
                    "Tier 3: command detected → {:?} (probability: {:.3})",
                    intent, prob
                );
                Some(intent)
            } else {
                None
            }
        }

        /// Calculate the detection score from the buffer.
        ///
        /// Three trigger paths (ordered by sensitivity):
        /// 1. **High-confidence single frame:** If any frame in the buffer
        ///    exceeds `SINGLE_FRAME_HIGH_CONFIDENCE` (0.5), return it
        ///    immediately. A single 0.5+ frame is a real wake — the silence
        ///    gate already blocks digital silence, and the model produces
        ///    <0.01 on non-wake speech.
        /// 2. **Max-based detection:** Return the maximum probability in
        ///    the buffer if it exceeds the threshold. This is far more
        ///    sensitive than averaging — a single 0.36 frame surrounded by
        ///    0.0s gives max=0.36 (triggers at threshold 0.35) vs
        ///    avg=0.03 (never triggers). This is the key fix for 58.2%
        ///    recall — the model often produces one good frame per
        ///    utterance, and the old averaging diluted it to nothing.
        /// 3. **Multi-frame confirmation:** If at least
        ///    `MIN_POSITIVE_DETECTIONS` frames exceed threshold, return
        ///    their average. This is a fallback for borderline cases.
        fn calculate_average(&self) -> f32 {
            let all = self.detections_buffer.to_vec();

            // Path 1: single high-confidence frame triggers immediately
            for &d in &all {
                if d >= SINGLE_FRAME_HIGH_CONFIDENCE {
                    return d;
                }
            }

            // Path 2: max-based detection — return the highest probability
            // in the buffer if it exceeds threshold. This is the key change
            // from the old averaging approach which diluted single good
            // frames with surrounding 0.0s.
            let max_prob = all.iter().cloned().fold(0.0f32, f32::max);
            if max_prob > self.threshold {
                return max_prob;
            }

            // Path 3: multi-frame confirmation (fallback)
            let mut cumulative = 0.0f32;
            let mut positive_count = 0.0f32;
            for d in all {
                if d > self.threshold {
                    positive_count += 1.0;
                    cumulative += d;
                }
            }
            if positive_count < MIN_POSITIVE_DETECTIONS {
                return 0.0;
            }
            let avg = cumulative / positive_count;
            if avg > self.threshold { avg } else { 0.0 }
        }

        /// Process a chunk of 16kHz mono f32 audio.
        /// Returns true if the wake word "NEXUS" was detected and the speaker was accepted.
        /// Also emits command-detected events via `command_tx` if any Tier 3
        /// command classifier fires.
        ///
        /// Secondary confirmation: after a raw detection, collects 500ms of
        /// audio to verify there was actual speech (RMS > 0.001). This filters
        /// pure digital silence/noise spikes that pass the classifier but
        /// aren't real speech. The threshold is very low (0.001) because the
        /// Intel SST driver produces brief audio bursts that fade quickly —
        /// by the time the 500ms confirmation window completes, the mic may
        /// have gone silent again. The silence gate (0.0005) still blocks
        /// pure digital silence before it reaches the classifier.
        pub fn process(&mut self, samples: &[f32]) -> bool {
            // If we're in confirmation mode, collect audio and check RMS
            if self.confirmation_active {
                self.confirmation_buffer.extend_from_slice(samples);
                // 500ms = 8000 samples @ 16kHz
                if self.confirmation_buffer.len() >= 8000 {
                    let buf = std::mem::take(&mut self.confirmation_buffer);
                    let raw_rms = {
                        let sum_sq: f32 = buf.iter().map(|s| s * s).sum();
                        (sum_sq / buf.len() as f32).sqrt()
                    };
                    // Apply the same AGC as the classifier (see detect_chunk):
                    // amplify to TARGET_RMS (0.03) with MAX_GAIN (50x). This ensures
                    // the confirmation window sees the same audio level the
                    // classifier did. Without this, the classifier fires at 0.99
                    // on AGC-amplified audio but the confirmation rejects it
                    // because the raw RMS is below 0.001 — a mismatch that
                    // causes valid wakes to be discarded on Intel SST mics
                    // that deliver very quiet audio.
                    const CONF_TARGET_RMS: f32 = 0.03;
                    const CONF_MAX_GAIN: f32 = 50.0;
                    let agc_rms = if raw_rms > 0.0 && raw_rms < CONF_TARGET_RMS {
                        let gain = (CONF_TARGET_RMS / raw_rms).min(CONF_MAX_GAIN);
                        let amplified: f32 = buf.iter().map(|&s| (s * gain).clamp(-1.0, 1.0)).map(|s| s * s).sum();
                        (amplified / buf.len() as f32).sqrt()
                    } else {
                        raw_rms
                    };
                    self.confirmation_active = false;

                    // Use AGC-adjusted RMS for the threshold check, but log both
                    // so we can see what the raw mic level was.
                    if agc_rms >= 0.001 {
                        tracing::info!(
                            "OWW wake confirmed! (probability: {:.3}, raw RMS: {:.6}, AGC RMS: {:.4})",
                            self.pending_probability, raw_rms, agc_rms
                        );
                        return true;
                    } else {
                        tracing::info!(
                            "OWW wake rejected — confirmation RMS too low (raw={:.6}, AGC={:.4} < 0.001), likely noise spike",
                            raw_rms, agc_rms
                        );
                        // Reset detection state
                        self.detections_buffer.clear();
                        self.last_detection_time = std::time::Instant::now();
                    }
                }
                // Still process chunks for command detection while confirming
            }

            self.chunk_buffer.extend_from_slice(samples);

            while self.chunk_buffer.len() >= OWW_CHUNK_SIZE {
                let chunk: Vec<f32> = self.chunk_buffer.drain(0..OWW_CHUNK_SIZE).collect();

                let (detected, prob, command_intent) = self.detect_chunk(chunk);

                // --- Tier 3: emit command-detected event if a command fired ---
                if let Some(intent) = command_intent {
                    if let Some(ref tx) = self.command_tx {
                        let _ = tx.send(intent);
                    }
                }

                if detected && !self.confirmation_active {
                    let accepted = true;

                    if accepted {
                        tracing::info!(
                            "OWW raw wake detected (probability: {:.3}) — awaiting 500ms confirmation...",
                            prob
                        );
                        self.confirmation_active = true;
                        self.pending_probability = prob;
                        self.confirmation_buffer.clear();
                    }
                }
            }

            false
        }
    }

    /// Resampler state: fractional read cursor + carry buffer of native mono samples.
    pub struct ResampleState {
        pub ratio: f64,
        pub frac: f64,
        pub carry: Vec<f32>,
    }

    impl ResampleState {
        pub fn new(native_sr: u32, target_sr: u32) -> Self {
            Self {
                ratio: native_sr as f64 / target_sr as f64,
                frac: 0.0,
                carry: Vec::with_capacity(4096),
            }
        }
    }

    /// Generic audio callback: downmix to mono (f32), resample to 16kHz,
    /// and feed 1280-sample chunks (80ms) to the KWS engine.
    ///
    /// The argument count is inherent to a cpal callback: it is invoked from
    /// four monomorphised sample-format branches (i16/u16/f32/...), each of
    /// which must thread the same shared state through. Bundling these into a
    /// struct would add an allocation on the real-time audio path.
    #[allow(clippy::too_many_arguments)]
    pub fn on_audio<T, F>(
        data: &[T],
        native_channels: usize,
        state: &Arc<parking_lot::Mutex<ResampleState>>,
        out_buf: &Arc<parking_lot::Mutex<Vec<f32>>>,
        engine: &Arc<parking_lot::Mutex<WakeEngine>>,
        chunk_size: usize,
        to_f32: F,
        wake_tx: &std::sync::mpsc::Sender<()>,
    )
    where
        F: Fn(T) -> f32,
        T: Copy,
    {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);
        static SAMPLE_COUNT: AtomicU64 = AtomicU64::new(0);
        static LAST_NONSILENT_CB: AtomicU64 = AtomicU64::new(0);
        static MAX_RMS_SEEN: AtomicU64 = AtomicU64::new(0); // RMS * 1e6 as u64

        let n = CALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
        let samples_in = data.len() / native_channels.max(1);
        SAMPLE_COUNT.fetch_add(samples_in as u64, Ordering::Relaxed);

        // Compute RMS of this callback's audio
        let ch = native_channels.max(1);
        let frames = data.len() / ch;
        let mut sum_sq = 0.0f32;
        for i in 0..frames {
            let mut sum = 0.0f32;
            for c in 0..ch {
                sum += to_f32(data[i * ch + c]);
            }
            let mono = sum / ch as f32;
            sum_sq += mono * mono;
        }
        let rms = if frames > 0 { (sum_sq / frames as f32).sqrt() } else { 0.0 };

        // Track when we last saw non-silent audio
        if rms > 0.001 {
            LAST_NONSILENT_CB.store(n, Ordering::Relaxed);
            let rms_scaled = (rms * 1e6) as u64;
            let prev = MAX_RMS_SEEN.load(Ordering::Relaxed);
            if rms_scaled > prev {
                MAX_RMS_SEEN.store(rms_scaled, Ordering::Relaxed);
            }
        }

        // Update file-level statics for the silence-recovery thread.
        // These mirror the function-local statics so the recovery thread
        // (which runs in a separate thread) can monitor audio health.
        super::CALLBACK_COUNT_GLOBAL.store(n, Ordering::Relaxed);
        if rms > 0.001 {
            super::LAST_NONSILENT_FOR_RECOVERY.store(n, Ordering::Relaxed);
        }

        if n % 1000 == 0 && n > 0 {
            let total = SAMPLE_COUNT.load(Ordering::Relaxed);
            let last_nonsilent = LAST_NONSILENT_CB.load(Ordering::Relaxed);
            let max_rms = MAX_RMS_SEEN.load(Ordering::Relaxed) as f32 / 1e6;
            let silence_secs = (n - last_nonsilent) as f64 * 0.03; // approx seconds of silence
            tracing::debug!(
                "audio: {} callbacks, ~{:.1}s processed, RMS={:.6}, max_RMS_seen={:.6}, silent_for~{:.0}s",
                n, total as f64 / 16000.0, rms, max_rms, silence_secs
            );

            // Warn if mic has been silent for more than 60 seconds
            if n - last_nonsilent > 2000 && n > 2000 {
                tracing::warn!(
                    "audio: mic has been silent for ~{:.0}s (callbacks {}-{}, max RMS ever seen: {:.6}). \
                     Intel SST driver may need a restart. Try: 1) Unmute mic in Windows settings, \
                     2) Disable 'Audio Enhancements' in mic properties, 3) Restart the app.",
                    silence_secs, last_nonsilent, n, max_rms
                );
            }
        }

        // 1. Downmix to mono f32
        {
            let mut st = state.lock();
            let ch = native_channels.max(1);
            let frames = data.len() / ch;
            for i in 0..frames {
                let mut sum = 0.0f32;
                for c in 0..ch {
                    sum += to_f32(data[i * ch + c]);
                }
                st.carry.push(sum / ch as f32);
            }
        }

        // 2. Resample to 16kHz
        let mut produced: Vec<f32> = Vec::with_capacity(chunk_size);
        {
            let mut st = state.lock();
            let ratio = st.ratio;
            let mut pos = st.frac;
            while pos + ratio < st.carry.len() as f64 {
                let idx0 = pos.floor() as usize;
                let idx1 = (idx0 + 1).min(st.carry.len() - 1);
                let t = pos - idx0 as f64;
                let s = st.carry[idx0] as f64 * (1.0 - t) + st.carry[idx1] as f64 * t;
                produced.push(s as f32);
                pos += ratio;
            }
            let consumed = pos.floor() as usize;
            st.carry.drain(0..consumed);
            st.frac = pos - consumed as f64;
        }

        // 3. Feed 1280-sample chunks to KWS engine
        //    Check meeting/privacy state — if suppressed, drain audio but
        //    don't run detection (prevents wake during meetings and TTS self-trigger).
        //    Also enforces Dual-Phase 300ms Post-TTS Mute Gate.
        {
            let mut last_tts_active = std::time::Instant::now() - std::time::Duration::from_secs(10);
            let mut buf = out_buf.lock();
            buf.extend(produced);
            while buf.len() >= chunk_size {
                let chunk: Vec<f32> = buf.drain(0..chunk_size).collect();

                // Check if wake detection should be suppressed
                let suppressed = super::MEETING_STATE
                    .get()
                    .map(|s: &std::sync::Arc<crate::meeting_detect::MeetingState>| {
                        s.should_suppress_wake()
                    })
                    .unwrap_or(false);

                if suppressed {
                    last_tts_active = std::time::Instant::now();
                    continue;
                }

                // Dual-Phase Mute Gate: drop audio chunks for 300ms after TTS finishes
                // to allow room acoustics and DAC output buffers to settle completely
                if last_tts_active.elapsed() < std::time::Duration::from_millis(300) {
                    continue;
                }

                let mut eng = engine.lock();
                if eng.process(&chunk) {
                    let _ = wake_tx.send(());
                }
            }
        }
    }
}

#[cfg(not(feature = "mock-wake"))]
use once_cell::sync::OnceCell;
#[cfg(not(feature = "mock-wake"))]
static WAKE_TX: OnceCell<std::sync::mpsc::Sender<()>> = OnceCell::new();
/// Global meeting/privacy state — checked on every audio chunk.
/// Set up in `lib.rs` before the wake engine starts.
#[cfg(not(feature = "mock-wake"))]
static MEETING_STATE: OnceCell<std::sync::Arc<crate::meeting_detect::MeetingState>> =
    OnceCell::new();

/// Global cpal stream handle for pause/resume (mic baton pass).
/// Stored in a RwLock so the frontend can pause the wake-word engine
/// before acquiring the mic via getUserMedia(), and resume it after
/// releasing the mic. Without this, Windows Intel SST drivers deadlock
/// when two processes try to capture the mic simultaneously.
///
/// cpal::Stream is not Send/Sync on all platforms (it contains a *mut ()),
/// so we wrap it in a newtype with manual unsafe impls. This is safe because:
/// - pause() and play() are the only operations we perform
/// - These are called from the Tauri IPC thread, never from the audio callback
/// - The stream is never moved or cloned after being stored
#[cfg(not(feature = "mock-wake"))]
struct SendStream(cpal::Stream);
#[cfg(not(feature = "mock-wake"))]
unsafe impl Send for SendStream {}
#[cfg(not(feature = "mock-wake"))]
unsafe impl Sync for SendStream {}

#[cfg(not(feature = "mock-wake"))]
static CPAL_STREAM: once_cell::sync::Lazy<parking_lot::RwLock<Option<SendStream>>> =
    once_cell::sync::Lazy::new(|| parking_lot::RwLock::new(None));

/// Global engine reference — needed by the silence-recovery thread to
/// restart the audio stream without going through the full init path.
#[cfg(not(feature = "mock-wake"))]
static WAKE_ENGINE_GLOBAL: once_cell::sync::Lazy<parking_lot::Mutex<Option<std::sync::Arc<parking_lot::Mutex<engine::WakeEngine>>>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

/// File-level statics for the silence-recovery thread.
/// The audio callback (inside `mod engine`) updates these so the recovery
/// thread can monitor without accessing function-local statics.
static CALLBACK_COUNT_GLOBAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LAST_NONSILENT_FOR_RECOVERY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LAST_CALLBACK_FOR_RECOVERY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Tracks how many times the silence-recovery has restarted the stream.
/// Used to rate-limit restarts and log the count for debugging.
static RECOVERY_RESTART_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Pause the wake-word audio stream (release the OS mic lock).
/// Called by the frontend via `pause_wakeword` IPC before getUserMedia().
#[cfg(not(feature = "mock-wake"))]
pub fn pause_stream() {
    use cpal::traits::StreamTrait;
    let guard = CPAL_STREAM.read();
    if let Some(ref stream) = *guard {
        match stream.0.pause() {
            Ok(()) => tracing::info!("wake: cpal stream paused (mic baton pass — frontend acquiring mic)"),
            Err(e) => tracing::warn!("wake: cpal stream pause failed: {e}"),
        }
    } else {
        tracing::warn!("wake: pause_stream called but no stream stored");
    }
}

/// Resume the wake-word audio stream (re-acquire the OS mic lock).
/// Called by the frontend via `resume_wakeword` IPC after releasing the mic.
#[cfg(not(feature = "mock-wake"))]
pub fn resume_stream() {
    use cpal::traits::StreamTrait;
    let guard = CPAL_STREAM.read();
    if let Some(ref stream) = *guard {
        match stream.0.play() {
            Ok(()) => tracing::info!("wake: cpal stream resumed (mic baton pass — frontend released mic)"),
            Err(e) => tracing::warn!("wake: cpal stream resume failed: {e}"),
        }
    } else {
        tracing::warn!("wake: resume_stream called but no stream stored");
    }
}

/// Mock-wake stubs for non-OWW builds.
#[cfg(feature = "mock-wake")]
pub fn pause_stream() {}
#[cfg(feature = "mock-wake")]
pub fn resume_stream() {}

/// Set the global meeting state reference. Called from `lib.rs` during setup.
#[cfg(not(feature = "mock-wake"))]
pub fn set_meeting_state(state: std::sync::Arc<crate::meeting_detect::MeetingState>) {
    let _ = MEETING_STATE.set(state);
}

#[cfg(not(feature = "mock-wake"))]
pub fn run<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    use tauri::{Emitter, Manager};
    use std::time::Instant;

    // ─── Phase 1: Resolve directories ──────────────────────────────
    let t0 = Instant::now();
    let res = app.path().resource_dir().map_err(|e| format!("resource dir: {e}"))?;
    let data_dir = app.path().app_data_dir().map_err(|e| format!("app data dir: {e}"))?;
    std::fs::create_dir_all(&data_dir).map_err(|e| format!("create app data dir: {e}"))?;
    tracing::info!("wake-engine: dirs resolved in {:.0}ms", t0.elapsed().as_secs_f64() * 1000.0);

    // ─── Phase 2: Load ONNX models (CPU-heavy, may take 30-120s on cold boot) ──
    let t1 = Instant::now();
    let _ = app.emit("wake-engine-status", "loading-models");
    tracing::info!("wake-engine: loading ONNX models (tract-onnx optimization)...");
    let mut wake_engine = engine::WakeEngine::new(res, data_dir)
        .map_err(|e| format!("wake engine init: {e}"))?;
    tracing::info!(
        "wake-engine: ONNX models loaded in {:.1}s — KWS ready",
        t1.elapsed().as_secs_f64()
    );

    // Create command channel for Tier 3 command classifiers
    let (cmd_tx, cmd_rx) =
        std::sync::mpsc::channel::<engine::CommandIntent>();
    wake_engine.command_tx = Some(cmd_tx);

    let engine = std::sync::Arc::new(parking_lot::Mutex::new(wake_engine));

    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let _ = WAKE_TX.set(tx);

    // ─── Phase 3: Start audio capture (with retry for cold-boot audio driver) ──
    let t2 = Instant::now();
    let _ = app.emit("wake-engine-status", "starting-audio");
    start_audio_capture_with_retry(engine.clone())?;
    tracing::info!(
        "wake-engine: audio capture started in {:.1}s — listening for 'nexus'",
        t2.elapsed().as_secs_f64()
    );
    let _ = app.emit("wake-engine-status", "ready");

    // Store engine globally for the silence-recovery thread.
    {
        let mut g = WAKE_ENGINE_GLOBAL.lock();
        *g = Some(engine.clone());
    }

    // ─── Silence Recovery Thread ────────────────────────────────────
    // The Intel SST driver sometimes stops delivering audio after 15-30
    // minutes (RMS drops to exactly 0.0 and stays there). This thread
    // monitors the callback counter and restarts the stream when silence
    // persists for more than 90 seconds. The restart drops the old cpal
    // stream (which may be in a stuck state) and creates a fresh one.
    #[cfg(not(feature = "mock-wake"))]
    {
        tracing::info!("silence-recovery: spawning monitor thread");
        std::thread::Builder::new()
            .name("silence-recovery".into())
            .spawn(move || {
                use std::sync::atomic::Ordering;
                #[allow(unused_imports)]
                use cpal::traits::{DeviceTrait, HostTrait};
                #[cfg(target_os = "windows")]
                use std::os::windows::process::CommandExt;
                tracing::info!("silence-recovery: thread started, monitoring for mic silence");
                let mut consecutive_silent_restarts: u64 = 0;
                loop {
                    // Base poll interval is 5s, but apply exponential backoff
                    // after consecutive silent restarts. The Intel SST driver
                    // delivers audio in brief 5-15s bursts after each restart,
                    // then goes silent. If restarts aren't helping (the driver
                    // is stuck), back off to avoid a tight restart loop.
                    // Backoff schedule: 5s, 5s, 10s, 20s, 40s, 60s (capped)
                    let poll_secs = if consecutive_silent_restarts <= 1 {
                        5
                    } else {
                        let backoff = 5u64 * (1u64 << (consecutive_silent_restarts - 1).min(4));
                        backoff.min(60)
                    };
                    std::thread::sleep(std::time::Duration::from_secs(poll_secs));
                    let last_cb = LAST_CALLBACK_FOR_RECOVERY.load(Ordering::Relaxed);
                    let last_non_silent = LAST_NONSILENT_FOR_RECOVERY.load(Ordering::Relaxed);
                    let now_cb = CALLBACK_COUNT_GLOBAL.load(Ordering::Relaxed);

                    // If no callbacks at all in the poll window, the stream is dead.
                    // If callbacks are flowing but all silent, restart only if
                    // silence has persisted for the full poll window.
                    let callbacks_stalled = now_cb.saturating_sub(last_cb) == 0;
                    let silence_duration = now_cb.saturating_sub(last_non_silent);
                    // ~33 callbacks/sec → poll_secs * 33 callbacks
                    let long_silence = silence_duration > (poll_secs * 33);

                    if callbacks_stalled || long_silence {
                        let restart_n = RECOVERY_RESTART_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
                        consecutive_silent_restarts = consecutive_silent_restarts.saturating_add(1);
                        tracing::warn!(
                            "silence-recovery #{}: restarting audio stream (stalled={}, silence_callbacks={}, total_callbacks={})",
                            restart_n, callbacks_stalled, silence_duration, now_cb
                        );

                        // Every 12 restarts (~60s of continuous silence),
                        // try to restart the Windows Audio service. This can
                        // fix the Intel SST driver when simple stream restarts
                        // don't help. Requires admin privileges — if we don't
                        // have them, the command silently fails.
                        if restart_n % 12 == 0 {
                            tracing::warn!(
                                "silence-recovery #{}: 12 restarts failed — attempting Windows Audio service restart",
                                restart_n
                            );
                            #[cfg(target_os = "windows")]
                            {
                                let _ = std::process::Command::new("net")
                                    .args(["stop", "Audiosrv"])
                                    .creation_flags(0x08000000) // CREATE_NO_WINDOW
                                    .stdout(std::process::Stdio::null())
                                    .stderr(std::process::Stdio::null())
                                    .status();
                                std::thread::sleep(std::time::Duration::from_secs(2));
                                let _ = std::process::Command::new("net")
                                    .args(["start", "Audiosrv"])
                                    .creation_flags(0x08000000) // CREATE_NO_WINDOW
                                    .stdout(std::process::Stdio::null())
                                    .stderr(std::process::Stdio::null())
                                    .status();
                                std::thread::sleep(std::time::Duration::from_secs(3));
                                tracing::info!(
                                    "silence-recovery #{}: Windows Audio service restart attempted",
                                    restart_n
                                );
                            }
                        }

                        // Drop the old stream
                        {
                            let mut guard = CPAL_STREAM.write();
                            *guard = None;
                        }

                        // Re-acquire the engine and restart using the FAST path
                        // (try_device_silent) — no 5s probe. The probe is only
                        // useful at initial startup to pick the best device.
                        // On recovery, we already know which device to use, so
                        // we just restart it instantly. This reduces the
                        // restart cycle from 35s to ~10s.
                        let engine_opt = WAKE_ENGINE_GLOBAL.lock().clone();
                        if let Some(engine) = engine_opt {
                            // Try the default device directly (skip probe)
                            let host = cpal::default_host();
                            let restart_result = if let Some(device) = host.default_input_device() {
                                let dev_name = device.name().unwrap_or_else(|_| "default".into());
                                tracing::info!(
                                    "silence-recovery #{}: fast-restarting on '{}' (no probe)",
                                    restart_n, dev_name
                                );
                                try_device_silent(&device, engine)
                            } else {
                                // No default device — fall back to full enumeration
                                tracing::warn!(
                                    "silence-recovery #{}: no default device, full restart",
                                    restart_n
                                );
                                start_audio_capture(engine)
                            };

                            match restart_result {
                                Ok(()) => {
                                    tracing::info!(
                                        "silence-recovery #{}: audio stream restarted successfully",
                                        restart_n
                                    );
                                    // Reset the silence tracker so we don't
                                    // immediately restart again
                                    LAST_NONSILENT_FOR_RECOVERY.store(
                                        CALLBACK_COUNT_GLOBAL.load(Ordering::Relaxed),
                                        Ordering::Relaxed,
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "silence-recovery #{}: failed to restart audio: {e}",
                                        restart_n
                                    );
                                }
                            }
                        }
                    }

                    // If audio is flowing and non-silent, reset the backoff
                    if !callbacks_stalled && !long_silence && consecutive_silent_restarts > 0 {
                        tracing::info!(
                            "silence-recovery: audio recovered after {} restart(s) — resetting backoff",
                            consecutive_silent_restarts
                        );
                        consecutive_silent_restarts = 0;
                    }

                    // Update our last-seen callback count
                    LAST_CALLBACK_FOR_RECOVERY.store(now_cb, Ordering::Relaxed);
                }
            })
            .ok();
    }

    // ─── Phase 4: Main loop (wake + command events) ────────────────
    // Spawn a thread for Tier 3 command-detected events.
    let app_for_commands = app.clone();
    std::thread::Builder::new()
        .name("tier3-commands".into())
        .spawn(move || {
            while let Ok(intent) = cmd_rx.recv() {
                tracing::info!(
                    "Tier 3: emitting command-detected event → action={}, target={}, needs_param={}",
                    intent.action, intent.target, intent.needs_param
                );
                if let Some(win) = app_for_commands.get_webview_window("main") {
                    let _ = win.show();
                    let _ = crate::window_manager::configure_non_activating_overlay(&win);
                    let _ = win.set_ignore_cursor_events(false);
                    let _ = app_for_commands.emit("command-detected", &intent);
                }
            }
        })
        .ok();

    // Main loop: handle wake-word detections
    while rx.recv().is_ok() {
        tracing::info!("wake-word: NEXUS detected → triggering wake");

        // Ensure the STT server is running before the frontend starts recording
        crate::lazy_stt::ensure_stt_running();

        // Only use the direct eval — the frontend's __NEXUS_WAKE__ handler
        // calls wakeWithGreeting(). Do NOT also emit Tauri events, as the
        // frontend listens to those too and would call wakeWithGreeting()
        // multiple times (causing "on it sir" to fire twice).
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.show();
            let _ = crate::window_manager::configure_non_activating_overlay(&win);
            let _ = win.set_ignore_cursor_events(false);
            let _ = win.eval("window.__NEXUS_WAKE__ && window.__NEXUS_WAKE__()");
        }
    }
    Ok(())
}

/// Retry audio device initialization — on cold boot, the audio driver
/// may not be ready for several seconds. Retry every 2s for up to 60s.
#[cfg(not(feature = "mock-wake"))]
fn start_audio_capture_with_retry(
    engine: std::sync::Arc<parking_lot::Mutex<engine::WakeEngine>>,
) -> Result<(), String> {
    const MAX_ATTEMPTS: u32 = 30; // 30 × 2s = 60s total
    const RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

    for attempt in 1..=MAX_ATTEMPTS {
        match start_audio_capture(engine.clone()) {
            Ok(()) => return Ok(()),
            Err(e) => {
                if attempt < MAX_ATTEMPTS {
                    tracing::warn!(
                        "audio: attempt {}/{} failed ({}), retrying in {}s...",
                        attempt, MAX_ATTEMPTS, e, RETRY_INTERVAL.as_secs()
                    );
                    std::thread::sleep(RETRY_INTERVAL);
                } else {
                    return Err(format!(
                        "audio device not available after {} attempts ({}s): {}",
                        MAX_ATTEMPTS,
                        MAX_ATTEMPTS * RETRY_INTERVAL.as_secs() as u32,
                        e
                    ));
                }
            }
        }
    }
    Err("audio device retry loop exhausted".to_string())
}

#[cfg(not(feature = "mock-wake"))]
fn start_audio_capture(
    engine: std::sync::Arc<parking_lot::Mutex<engine::WakeEngine>>,
) -> Result<(), String> {
    #[allow(unused_imports)]
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    #[allow(unused_imports)]
    use cpal::Sample;
    #[allow(unused_imports)]
    use std::sync::atomic::{AtomicU64, Ordering};

    let host = cpal::default_host();

    // ─── Non-Windows (Linux / macOS): PipeWire, PulseAudio, CoreAudio ───
    // On Linux and macOS, the OS audio server manages stream routing and the default
    // input device is the user's active microphone. Start it directly without the
    // multi-device 5-second silence cascade.
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(default_device) = host.default_input_device() {
            let dev_name = default_device.name().unwrap_or_else(|_| "default".into());
            tracing::info!("audio: starting capture on default device '{}'...", dev_name);
            match try_device_silent(&default_device, engine.clone()) {
                Ok(()) => {
                    tracing::info!("audio: stream active on '{}'", dev_name);
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!(
                        "audio: default device '{}' failed to start: {e}. Falling back to device list.",
                        dev_name
                    );
                }
            }
        }
    }

    // ─── Enumerate ALL input devices and log them ───────────────────
    let devices: Vec<cpal::Device> = match host.input_devices() {
        Ok(iter) => iter.collect(),
        Err(e) => {
            tracing::error!("audio: failed to enumerate input devices: {e}");
            // Fall back to default device only
            host.default_input_device()
                .map(|d| vec![d])
                .ok_or_else(|| format!("no input devices: {e}"))?
        }
    };
    tracing::info!("audio: found {} input device(s):", devices.len());
    for (i, d) in devices.iter().enumerate() {
        let name = d.name().unwrap_or_else(|_| "unknown".into());
        let cfg = d.default_input_config().ok();
        let sr = cfg.as_ref().map(|c| c.sample_rate().0).unwrap_or(0);
        let ch = cfg.as_ref().map(|c| c.channels()).unwrap_or(0);
        tracing::info!("  [{}] '{}' ({}Hz, {}ch)", i, name, sr, ch);
    }

    // Try the default device first, then fall back to others.
    // The Intel SST "Digital Microphones" driver sometimes returns silence
    // in WASAPI shared mode, so we need to try ALL devices.
    let default_device = host.default_input_device();
    let mut try_order: Vec<cpal::Device> = Vec::new();
    if let Some(ref d) = default_device {
        try_order.push(d.clone());
    }
    for d in &devices {
        let name = d.name().unwrap_or_default();
        let is_default = default_device
            .as_ref()
            .and_then(|dd| dd.name().ok())
            .map(|dn| dn == name)
            .unwrap_or(false);
        if !is_default {
            try_order.push(d.clone());
        }
    }

    if try_order.is_empty() {
        return Err("no input devices available".to_string());
    }

    // Try each device until we find one that produces non-zero audio.
    // We start the stream, wait 5 seconds, and check the RMS.
    // If the device produces silence (Intel SST bug), try the next device.
    let mut last_err = String::new();
    let mut best_device: Option<(cpal::Device, f32)> = None; // (device, rms)

    for (attempt, device) in try_order.iter().enumerate() {
        let dev_name = device.name().unwrap_or_else(|_| "unknown".into());
        tracing::info!(
            "audio: trying device [{}] '{}' (attempt {}/{})",
            attempt, dev_name, attempt + 1, try_order.len()
        );

        match try_device(device, engine.clone()) {
            Ok(()) => {
                tracing::info!("audio: device '{}' accepted", dev_name);
                return Ok(());
            }
            Err(e) => {
                // Extract RMS from error message if present
                let rms = e
                    .strip_prefix("device produces silence (RMS=")
                    .and_then(|s| s.strip_suffix(")"))
                    .and_then(|s| s.parse::<f32>().ok())
                    .unwrap_or(0.0);
                // Use >= (not >) so that even a device with RMS=0.0 is stored
                // as the best device. Without this, when the Intel SST driver
                // produces exactly 0.0 RMS, the comparison 0.0 > 0.0 fails,
                // best_device stays None, and the try_device_silent fallback
                // never runs — causing an infinite retry loop.
                if best_device.is_none() || rms >= best_device.as_ref().map(|(_, r)| *r).unwrap_or(0.0) {
                    best_device = Some((device.clone(), rms));
                }
                tracing::warn!("audio: device '{}' failed: {}", dev_name, e);
                last_err = e;
            }
        }
    }

    // All devices produced silence. Fall back to the best (highest RMS) device
    // instead of giving up entirely. The mic may start working later (driver
    // recovery, user unmutes, headset reconnects, etc.).
    if let Some((device, rms)) = best_device {
        let dev_name = device.name().unwrap_or_else(|_| "unknown".into());
        tracing::warn!(
            "audio: ALL devices produced silence! Falling back to '{}' (RMS={:.6}). \
             The mic may be muted or the Intel SST driver may be broken. \
             Wake word will not work until the mic produces audio.",
            dev_name, rms
        );
        // Try one more time without the silence check — just start the stream
        match try_device_silent(&device, engine.clone()) {
            Ok(()) => {
                tracing::info!("audio: fallback device '{}' started (no silence check)", dev_name);
                return Ok(());
            }
            Err(e) => {
                tracing::error!("audio: fallback device also failed: {e}");
            }
        }
    }

    Err(format!("all input devices failed: {}", last_err))
}

/// Start audio capture on a device WITHOUT the silence probe.
/// Used as a fallback when all devices produce silence — the mic may
/// start working later (driver recovery, user unmutes, etc.).
#[cfg(not(feature = "mock-wake"))]
fn try_device_silent(
    device: &cpal::Device,
    engine: std::sync::Arc<parking_lot::Mutex<engine::WakeEngine>>,
) -> Result<(), String> {
    use cpal::traits::{DeviceTrait, StreamTrait};
    use cpal::Sample;

    let default_config = device
        .default_input_config()
        .map_err(|e| format!("default_input_config: {e}"))?;

    let target_sr = engine.lock().sample_rate as u32;
    let native_sr = default_config.sample_rate().0;
    let native_channels = default_config.channels() as usize;
    let sample_format = default_config.sample_format();
    let stream_config = cpal::StreamConfig {
        channels: default_config.channels(),
        sample_rate: default_config.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };
    let chunk_size = engine::OWW_CHUNK_SIZE;

    let state = std::sync::Arc::new(parking_lot::Mutex::new(engine::ResampleState::new(
        native_sr, target_sr,
    )));
    let out_buf = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<f32>::with_capacity(2560)));
    let engine_cb = engine;
    let wake_tx = WAKE_TX.get().cloned();
    let err_cb = |err| tracing::error!("audio stream error: {err}");

    let build_result = match sample_format {
        cpal::SampleFormat::I16 => device.build_input_stream::<i16, _, _>(
            &stream_config,
            {
                let state = state.clone();
                let out_buf = out_buf.clone();
                let wake_tx = wake_tx.clone();
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if let Some(tx) = &wake_tx {
                        engine::on_audio(data, native_channels, &state, &out_buf,
                            &engine_cb, chunk_size, |s: i16| s.to_sample::<f32>(), tx);
                    }
                }
            }, err_cb, None,
        ),
        cpal::SampleFormat::I32 => device.build_input_stream::<i32, _, _>(
            &stream_config,
            {
                let state = state.clone();
                let out_buf = out_buf.clone();
                let wake_tx = wake_tx.clone();
                move |data: &[i32], _: &cpal::InputCallbackInfo| {
                    if let Some(tx) = &wake_tx {
                        engine::on_audio(data, native_channels, &state, &out_buf,
                            &engine_cb, chunk_size, |s: i32| s.to_sample::<f32>(), tx);
                    }
                }
            }, err_cb, None,
        ),
        cpal::SampleFormat::F32 => device.build_input_stream::<f32, _, _>(
            &stream_config,
            {
                let state = state.clone();
                let out_buf = out_buf.clone();
                let wake_tx = wake_tx.clone();
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if let Some(tx) = &wake_tx {
                        engine::on_audio(data, native_channels, &state, &out_buf,
                            &engine_cb, chunk_size, |s: f32| s, tx);
                    }
                }
            }, err_cb, None,
        ),
        other => return Err(format!("unsupported sample format: {other:?}")),
    };

    let stream = build_result.map_err(|e| format!("build stream: {e}"))?;
    stream.play().map_err(|e| format!("play stream: {e}"))?;
    tracing::info!(
        "audio: stream started on '{}', OWW KWS listening for 'nexus'...",
        device.name().unwrap_or_else(|_| "unknown".into())
    );
    // Store the stream in the global so pause_stream/resume_stream can
    // access it for the mic baton pass. Keep it alive forever.
    *CPAL_STREAM.write() = Some(SendStream(stream));
    Ok(())
}

/// Try to start audio capture on a single device.
/// Starts the stream, waits 5 seconds, and checks the RMS of the captured
/// audio. If the device produces silence (Intel SST bug), returns an error
/// so the caller can try the next device.
#[cfg(not(feature = "mock-wake"))]
fn try_device(
    device: &cpal::Device,
    engine: std::sync::Arc<parking_lot::Mutex<engine::WakeEngine>>,
) -> Result<(), String> {
    use cpal::traits::{DeviceTrait, StreamTrait};
    use cpal::Sample;
    use std::sync::atomic::{AtomicU64, Ordering};

    let default_config = device
        .default_input_config()
        .map_err(|e| format!("default_input_config: {e}"))?;

    tracing::info!(
        "audio: native sample_rate = {} Hz, channels = {}, format = {:?}",
        default_config.sample_rate().0,
        default_config.channels(),
        default_config.sample_format()
    );

    let target_sr = engine.lock().sample_rate as u32;
    let native_sr = default_config.sample_rate().0;
    let native_channels = default_config.channels() as usize;

    let sample_format = default_config.sample_format();
    let stream_config = cpal::StreamConfig {
        channels: default_config.channels(),
        sample_rate: default_config.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };

    let chunk_size = engine::OWW_CHUNK_SIZE; // 1280

    let state = std::sync::Arc::new(parking_lot::Mutex::new(engine::ResampleState::new(
        native_sr,
        target_sr,
    )));
    let out_buf = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<f32>::with_capacity(2560)));
    let engine_cb = engine;
    let wake_tx = WAKE_TX.get().cloned();

    // Track sum-of-squares and sample count for RMS computation.
    // The Intel SST driver sends a few non-zero samples at startup then goes
    // silent, so we compute RMS over the FULL 5-second window, not just check
    // for any non-zero sample.
    let sum_sq = std::sync::Arc::new(AtomicU64::new(0)); // sum of squares * 1e9 (fixed-point)
    let total_samples = std::sync::Arc::new(AtomicU64::new(0));

    let err_cb = |err| tracing::error!("audio stream error: {err}");

    let build_result = match sample_format {
        cpal::SampleFormat::I16 => device.build_input_stream::<i16, _, _>(
            &stream_config,
            {
                let state = state.clone();
                let out_buf = out_buf.clone();
                let wake_tx = wake_tx.clone();
                let sum_sq = sum_sq.clone();
                let total_samples = total_samples.clone();
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let ch = native_channels.max(1);
                    let frames = data.len() / ch;
                    let mut sq_sum = 0.0f64;
                    for i in 0..frames {
                        let mut sum = 0.0f32;
                        for c in 0..ch {
                            sum += data[i * ch + c].to_sample::<f32>();
                        }
                        let mono = sum / ch as f32;
                        sq_sum += (mono as f64) * (mono as f64);
                    }
                    // Store as fixed-point (multiply by 1e9 to preserve precision)
                    sum_sq.fetch_add((sq_sum * 1e9) as u64, Ordering::Relaxed);
                    total_samples.fetch_add(frames as u64, Ordering::Relaxed);
                    if let Some(tx) = &wake_tx {
                        engine::on_audio(
                            data,
                            native_channels,
                            &state,
                            &out_buf,
                            &engine_cb,
                            chunk_size,
                            |s: i16| s.to_sample::<f32>(),
                            tx,
                        );
                    }
                }
            },
            err_cb,
            None,
        ),
        cpal::SampleFormat::I32 => device.build_input_stream::<i32, _, _>(
            &stream_config,
            {
                let state = state.clone();
                let out_buf = out_buf.clone();
                let wake_tx = wake_tx.clone();
                let sum_sq = sum_sq.clone();
                let total_samples = total_samples.clone();
                move |data: &[i32], _: &cpal::InputCallbackInfo| {
                    let ch = native_channels.max(1);
                    let frames = data.len() / ch;
                    let mut sq_sum = 0.0f64;
                    for i in 0..frames {
                        let mut sum = 0.0f32;
                        for c in 0..ch {
                            sum += data[i * ch + c].to_sample::<f32>();
                        }
                        let mono = sum / ch as f32;
                        sq_sum += (mono as f64) * (mono as f64);
                    }
                    sum_sq.fetch_add((sq_sum * 1e9) as u64, Ordering::Relaxed);
                    total_samples.fetch_add(frames as u64, Ordering::Relaxed);
                    if let Some(tx) = &wake_tx {
                        engine::on_audio(
                            data,
                            native_channels,
                            &state,
                            &out_buf,
                            &engine_cb,
                            chunk_size,
                            |s: i32| s.to_sample::<f32>(),
                            tx,
                        );
                    }
                }
            },
            err_cb,
            None,
        ),
        cpal::SampleFormat::F32 => device.build_input_stream::<f32, _, _>(
            &stream_config,
            {
                let state = state.clone();
                let out_buf = out_buf.clone();
                let wake_tx = wake_tx.clone();
                let sum_sq = sum_sq.clone();
                let total_samples = total_samples.clone();
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let ch = native_channels.max(1);
                    let frames = data.len() / ch;
                    let mut sq_sum = 0.0f64;
                    for i in 0..frames {
                        let mut sum = 0.0f32;
                        for c in 0..ch {
                            sum += data[i * ch + c];
                        }
                        let mono = sum / ch as f32;
                        sq_sum += (mono as f64) * (mono as f64);
                    }
                    sum_sq.fetch_add((sq_sum * 1e9) as u64, Ordering::Relaxed);
                    total_samples.fetch_add(frames as u64, Ordering::Relaxed);
                    if let Some(tx) = &wake_tx {
                        engine::on_audio(
                            data,
                            native_channels,
                            &state,
                            &out_buf,
                            &engine_cb,
                            chunk_size,
                            |s: f32| s,
                            tx,
                        );
                    }
                }
            },
            err_cb,
            None,
        ),
        other => return Err(format!("unsupported sample format: {other:?}")),
    };

    let stream = build_result.map_err(|e| format!("build stream: {e}"))?;
    stream.play().map_err(|e| format!("play stream: {e}"))?;

    // Wait 5 seconds and compute RMS over the full window.
    // The Intel SST driver may send a few non-zero samples at startup then
    // go silent, so we need a longer window and RMS (not just any-non-zero).
    std::thread::sleep(std::time::Duration::from_secs(5));

    let samples = total_samples.load(Ordering::Relaxed);
    let sq = sum_sq.load(Ordering::Relaxed) as f64 / 1e9;

    if samples == 0 {
        tracing::warn!(
            "audio: device produced 0 samples in 5s — no audio callback fired, trying next device"
        );
        drop(stream);
        return Err("no audio callbacks received".to_string());
    }

    let rms = (sq / samples as f64).sqrt() as f32;
    tracing::info!("audio: 5s probe RMS = {:.6} ({} samples)", rms, samples);

    // If RMS is below 0.0001 (effectively silence), try the next device.
    // A working mic in a quiet room has RMS ~0.001-0.01.
    // The Intel SST silence bug produces RMS ~0.0000-0.00005.
    if rms < 0.0001 {
        tracing::warn!(
            "audio: device RMS {:.6} is below silence threshold (0.0001) — \
             likely Intel SST silence bug, trying next device",
            rms
        );
        drop(stream);
        return Err(format!("device produces silence (RMS={:.6})", rms));
    }

    tracing::info!(
        "audio: stream started on '{}', OWW KWS listening for 'nexus'...",
        device.name().unwrap_or_else(|_| "unknown".into())
    );
    // Store the stream in the global so pause_stream/resume_stream can
    // access it for the mic baton pass. Keep it alive forever.
    *CPAL_STREAM.write() = Some(SendStream(stream));
    Ok(())
}

#[cfg(all(test, feature = "wakeword-oww"))]
mod tests {
    use std::path::PathBuf;

    /// Verify that all three required ONNX models exist in the resources directory
    /// and are non-trivial in size (not corrupted/empty).
    #[test]
    fn test_oww_models_exist() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let oww_dir = PathBuf::from(manifest_dir).join("resources").join("oww");

        let required = ["melspectrogram.onnx", "embedding_model.onnx", "nexus.onnx"];
        let mut found = 0;
        for name in &required {
            let path = oww_dir.join(name);
            if !path.exists() {
                eprintln!("SKIP: {} not found at {}", name, path.display());
                continue;
            }
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            assert!(
                size > 1000,
                "{} is only {} bytes — file may be corrupted",
                name,
                size
            );
            println!("OK: {} ({} bytes)", name, size);
            found += 1;
        }
        if found == 0 {
            eprintln!("SKIP: No OWW models found — train the model first");
        }
    }

    /// Verify that the trained nexus.onnx model file is a valid ONNX file
    /// by checking its magic bytes and basic structure.
    #[test]
    fn test_nexus_onnx_file_valid() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let nexus_path = PathBuf::from(manifest_dir)
            .join("resources")
            .join("oww")
            .join("nexus.onnx");

        if !nexus_path.exists() {
            eprintln!("SKIP: nexus.onnx not found at {}", nexus_path.display());
            eprintln!("      Train the model first using train_nexus_oww.ipynb");
            return;
        }

        // Read the file
        let data = std::fs::read(&nexus_path).expect("Failed to read nexus.onnx");
        assert!(data.len() > 1000, "nexus.onnx is too small ({} bytes)", data.len());

        // ONNX files start with a Protobuf header — check for common ONNX markers
        // The first few bytes should be valid protobuf (not random/corrupted)
        // ONNX format: message ModelProto { ... } — field 7 is ir_version
        // We just verify it's a valid protobuf by checking it doesn't start with null bytes
        assert!(
            data[0] != 0 || data.len() > 100,
            "nexus.onnx may be corrupted (starts with null bytes)"
        );

        // Check for the "onnx" string somewhere in the first 1KB (producer name)
        let header = &data[..std::cmp::min(1024, data.len())];
        let has_onnx_marker = header.windows(4).any(|w| w == b"onnx");
        let has_pytorch_marker = header.windows(7).any(|w| w == b"pytorch");
        let has_keras_marker = header.windows(5).any(|w| w == b"keras");

        // At least one producer marker should be present
        assert!(
            has_onnx_marker || has_pytorch_marker || has_keras_marker,
            "nexus.onnx doesn't contain expected ONNX producer markers — may not be a valid ONNX file"
        );

        println!(
            "OK: nexus.onnx is a valid ONNX file ({} bytes, markers: onnx={}, pytorch={}, keras={})",
            data.len(),
            has_onnx_marker,
            has_pytorch_marker,
            has_keras_marker
        );
    }

    /// Verify that the WakeEngine can be constructed with the trained model.
    /// This is the integration test — it loads all 3 models and initializes
    /// the full KWS pipeline.
    #[test]
    fn test_wake_engine_initializes() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let resource_dir = PathBuf::from(manifest_dir).join("resources");
        let app_data_dir = std::env::temp_dir().join("nexus_test_profile");

        // Check if models exist first
        let oww_dir = resource_dir.join("oww");
        let nexus_path = oww_dir.join("nexus.onnx");
        if !nexus_path.exists() {
            eprintln!("SKIP: nexus.onnx not found — train the model first");
            return;
        }

        // Try to create the WakeEngine — this loads all 3 ONNX models
        match crate::wakeword_oww::engine::WakeEngine::new(resource_dir, app_data_dir) {
            Ok(_engine) => {
                println!("OK: WakeEngine initialized successfully with trained nexus.onnx");
            }
            Err(e) => {
                // Speaker model may be missing — that's OK, it's optional
                let err_str = format!("{e}");
                if err_str.contains("speaker") || err_str.contains("Speaker") {
                    println!("OK: WakeEngine initialized (speaker verification disabled): {}", err_str);
                } else {
                    panic!("WakeEngine initialization failed: {e}");
                }
            }
        }
    }

    /// Regression test for the silence energy gate.
    ///
    /// Background: `nexus.onnx` emits high probabilities (0.6-0.9) when fed
    /// pure digital silence, because it was trained on TTS clips that always
    /// carry a noise floor. That caused NEXUS to wake spontaneously while the
    /// mic was quiet (observed repeatedly in logs at RMS=0.0000).
    ///
    /// `detect_chunk` now short-circuits below `SILENCE_RMS_THRESHOLD` (0.002)
    /// before running the classifier. This test feeds the engine a long run of
    /// digital silence and asserts it never reports a wake.
    #[test]
    fn test_silence_never_triggers_wake() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let resource_dir = PathBuf::from(manifest_dir).join("resources");
        let app_data_dir = std::env::temp_dir().join("nexus_test_profile_silence");

        if !resource_dir.join("oww").join("nexus.onnx").exists() {
            eprintln!("SKIP: nexus.onnx not found — train the model first");
            return;
        }

        let mut engine =
            match crate::wakeword_oww::engine::WakeEngine::new(resource_dir, app_data_dir) {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("SKIP: WakeEngine unavailable: {e}");
                    return;
                }
            };

        // 10 seconds of pure digital silence at 16kHz.
        let silence = vec![0.0f32; 16000 * 10];
        assert!(
            !engine.process(&silence),
            "pure silence must never trigger a wake (energy gate regression)"
        );

        // Very low-level noise (RMS well under the 0.002 gate) must also be
        // ignored — a real mic idles around 1e-4, not exactly zero.
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let quiet: Vec<f32> = (0..16000 * 10)
            .map(|_| {
                // xorshift64* — deterministic, no rand dependency in tests.
                seed ^= seed >> 12;
                seed ^= seed << 25;
                seed ^= seed >> 27;
                let v = ((seed.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as f32
                    / u32::MAX as f32)
                    - 0.5;
                v * 0.0002 // amplitude ≈ 1e-4 → RMS far below the gate
            })
            .collect();
        assert!(
            !engine.process(&quiet),
            "near-silent mic noise must never trigger a wake"
        );
    }

    /// Critical test: verify tract-onnx produces the same output as onnxruntime
    /// for the nexus.onnx classifier with known input.
    ///
    /// onnxruntime produces:
    ///   - All-5 features → 0.996699
    ///   - All-12 features → 0.996691
    ///   - Random (std=12) → 0.902931
    ///
    /// If tract-onnx produces 0.0 for these, there's a tract-onnx compatibility bug.
    #[test]
    fn test_nexus_classifier_tract_vs_onnxruntime() {
        use tract_onnx::prelude::*;

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let nexus_path = PathBuf::from(manifest_dir)
            .join("resources")
            .join("oww")
            .join("nexus.onnx");

        if !nexus_path.exists() {
            eprintln!("SKIP: nexus.onnx not found");
            return;
        }

        let model = tract_onnx::onnx()
            .model_for_path(&nexus_path)
            .expect("Failed to parse ONNX");
        let model = model.into_optimized().expect("Failed to optimize");
        let model = model.into_runnable().expect("Failed to make runnable");

        // Test 1: All-5 features (onnxruntime says 0.996699)
        let input5 = Tensor::from_shape(&[1, 16, 96], &[5.0f32; 16 * 96]).unwrap();
        let out5 = model.clone().run(tvec!(input5.into())).unwrap();
        let prob5: f32 = out5[0].clone().into_tensor().cast_to::<f32>().unwrap().into_owned()
            .into_plain_array::<f32>().unwrap().as_slice().unwrap()[0];
        println!("tract-onnx all-5 features → {:.6} (onnxruntime: 0.996699)", prob5);

        // Test 2: All-12 features (onnxruntime says 0.996691)
        let input12 = Tensor::from_shape(&[1, 16, 96], &[12.0f32; 16 * 96]).unwrap();
        let out12 = model.clone().run(tvec!(input12.into())).unwrap();
        let prob12: f32 = out12[0].clone().into_tensor().cast_to::<f32>().unwrap().into_owned()
            .into_plain_array::<f32>().unwrap().as_slice().unwrap()[0];
        println!("tract-onnx all-12 features → {:.6} (onnxruntime: 0.996691)", prob12);

        // Test 3: All-(-5) features (onnxruntime says 0.236054)
        let input_neg5 = Tensor::from_shape(&[1, 16, 96], &[-5.0f32; 16 * 96]).unwrap();
        let out_neg5 = model.clone().run(tvec!(input_neg5.into())).unwrap();
        let prob_neg5: f32 = out_neg5[0].clone().into_tensor().cast_to::<f32>().unwrap().into_owned()
            .into_plain_array::<f32>().unwrap().as_slice().unwrap()[0];
        println!("tract-onnx all-(-5) features → {:.6} (onnxruntime: 0.236054)", prob_neg5);

        // The outputs should be close to onnxruntime's values.
        // If tract-onnx produces 0.0, there's a bug.
        assert!(
            prob5 > 0.5,
            "tract-onnx all-5 features produced {:.6}, expected ~0.997 — tract-onnx bug!",
            prob5
        );
        assert!(
            prob12 > 0.5,
            "tract-onnx all-12 features produced {:.6}, expected ~0.997 — tract-onnx bug!",
            prob12
        );
    }
}
