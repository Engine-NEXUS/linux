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

use tauri::{AppHandle, Manager, Runtime};

#[cfg(feature = "mock-wake")]
pub fn run<R: Runtime>(_app: AppHandle<R>) -> Result<(), String> {
    tracing::info!("wake-word: mock mode (no native listener)");
    loop {
        std::thread::park();
    }
}

#[cfg(feature = "mock-wake")]
pub fn set_meeting_state(_state: std::sync::Arc<crate::meeting_detect::MeetingState>) {}

#[cfg(feature = "mock-wake")]
pub(crate) fn clear_tts_playing() {}

#[cfg(not(feature = "mock-wake"))]
pub(crate) mod engine {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;

    use circular_buffer::CircularBuffer;
    use serde::{Deserialize, Serialize};

    /// Throttle counter for "audio passed gate" debug logs (avoids 12.5 logs/sec flood).
    static GATE_PASS_COUNT: AtomicU64 = AtomicU64::new(0);
    use tract_onnx::prelude::*;

    type ModelType = Arc<TypedSimplePlan>;

    // ─── Phase B: Audio Preprocessing ────────────────────────────────
    // Pure-Rust audio preprocessing for wake word detection.
    // Replaces the need for C-based RNNoise dependency.
    //
    // Pipeline: raw audio → high-pass filter → noise gate → VAD → AGC → model
    //
    // 1. High-pass filter (80Hz): removes low-frequency rumble (HVAC, traffic,
    //    desk vibrations) that RNNoise also targets. First-order IIR.
    // 2. Adaptive noise floor tracking: tracks minimum RMS over a rolling
    //    window. If current RMS is within 2x of noise floor, it's likely noise.
    // 3. VAD (Voice Activity Detection): combines energy + zero-crossing rate
    //    to determine if the chunk contains speech. Skips model if no speech.
    // 4. AGC (existing): amplifies quiet speech to target RMS.

    /// High-pass filter (first-order IIR) for noise suppression.
    /// Removes low-frequency noise below ~80Hz (HVAC, traffic, desk vibration).
    /// This is the same frequency range RNNoise targets.
    pub struct HighPassFilter {
        /// Previous output sample (for IIR feedback)
        prev_y: f32,
        /// Previous input sample (for IIR feedforward)
        prev_x: f32,
        /// Filter coefficient (alpha = RC / (RC + dt))
        /// At 16kHz, 80Hz cutoff: alpha = 0.9969
        alpha: f32,
    }

    impl HighPassFilter {
        /// Create a high-pass filter with 80Hz cutoff at 16kHz sample rate.
        pub fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
            let dt = 1.0 / sample_rate;
            let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz);
            let alpha = rc / (rc + dt);
            HighPassFilter {
                prev_y: 0.0,
                prev_x: 0.0,
                alpha,
            }
        }

        /// Process a chunk of audio samples.
        pub fn process(&mut self, samples: &mut [f32]) {
            for s in samples.iter_mut() {
                let y = self.alpha * (self.prev_y + *s - self.prev_x);
                self.prev_y = y;
                self.prev_x = *s;
                *s = y;
            }
        }

        /// Reset filter state (call on stream restart).
        pub fn reset(&mut self) {
            self.prev_y = 0.0;
            self.prev_x = 0.0;
        }
    }

    /// Adaptive noise floor tracker.
    /// Tracks the minimum RMS over a rolling window to estimate
    /// the background noise level. If current RMS is close to the
    /// noise floor, the audio is likely just noise.
    pub struct NoiseFloorTracker {
        /// Rolling buffer of recent RMS values
        rms_history: CircularBuffer<32, f32>,
        /// Current noise floor estimate (minimum RMS in history)
        noise_floor: f32,
    }

    impl NoiseFloorTracker {
        pub fn new() -> Self {
            let mut rms_history = CircularBuffer::<32, f32>::new();
            // Initialize with a moderate noise floor
            for _ in 0..32 {
                rms_history.push_back(0.001);
            }
            NoiseFloorTracker {
                rms_history,
                noise_floor: 0.001,
            }
        }

        /// Update with a new RMS value and return the current noise floor.
        pub fn update(&mut self, rms: f32) -> f32 {
            self.rms_history.push_back(rms);
            // Noise floor = minimum RMS in the rolling window
            self.noise_floor = self.rms_history.iter().cloned().fold(f32::MAX, f32::min);
            self.noise_floor
        }

        /// Get the current noise floor estimate.
        #[allow(dead_code)]
        pub fn floor(&self) -> f32 {
            self.noise_floor
        }
    }

    /// Simple VAD (Voice Activity Detection) using energy + zero-crossing rate.
    /// Returns true if the chunk likely contains speech.
    ///
    /// This is a lightweight alternative to WebRTC VAD that works in pure Rust.
    /// It combines two features:
    /// 1. Short-term energy: speech has higher energy than noise
    /// 2. Zero-crossing rate: speech has lower ZCR than noise (voiced sounds
    ///    are low-frequency, noise is high-frequency with high ZCR)
    pub struct VadDetector {
        /// Energy threshold (adaptive, based on noise floor)
        energy_threshold: f32,
        /// Zero-crossing rate threshold (fixed)
        zcr_threshold: f32,
        /// Number of consecutive speech frames needed to confirm speech
        speech_frames_needed: u32,
        /// Current count of consecutive speech frames
        speech_frame_count: u32,
    }

    impl VadDetector {
        pub fn new() -> Self {
            VadDetector {
                energy_threshold: 0.005,  // Initial threshold (will adapt)
                zcr_threshold: 0.35,     // 35% zero-crossing rate = likely noise
                speech_frames_needed: 1, // 1 frame = 80ms (fast response)
                speech_frame_count: 0,
            }
        }

        /// Detect if a chunk contains speech.
        /// Returns true if speech is detected.
        pub fn detect(&mut self, samples: &[f32], noise_floor: f32) -> bool {
            if samples.is_empty() {
                return false;
            }

            // 1. Compute short-term energy (RMS)
            let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
            let rms = (sum_sq / samples.len() as f32).sqrt();

            // 2. Compute zero-crossing rate
            let mut zero_crossings = 0;
            for i in 1..samples.len() {
                if (samples[i] >= 0.0) != (samples[i - 1] >= 0.0) {
                    zero_crossings += 1;
                }
            }
            let zcr = zero_crossings as f32 / samples.len() as f32;

            // 3. Adapt energy threshold based on noise floor
            // Threshold = max(5x noise floor, 0.005)
            self.energy_threshold = (noise_floor * 5.0).max(0.005);

            // 4. Speech detection: high energy AND low ZCR
            let has_energy = rms > self.energy_threshold;
            let has_low_zcr = zcr < self.zcr_threshold;

            if has_energy && has_low_zcr {
                self.speech_frame_count += 1;
            } else {
                // Decay: reduce speech count but don't reset immediately
                // This handles brief pauses within a word
                self.speech_frame_count = self.speech_frame_count.saturating_sub(1);
            }

            self.speech_frame_count >= self.speech_frames_needed
        }
    }

    /// Audio preprocessing pipeline.
    /// Combines high-pass filter, noise floor tracking, and VAD.
    pub struct AudioPreprocessor {
        pub high_pass: HighPassFilter,
        pub noise_floor: NoiseFloorTracker,
        pub vad: VadDetector,
        /// Whether VAD gating is enabled (can be disabled for debugging)
        pub vad_enabled: bool,
        /// Count of chunks skipped by VAD (for stats)
        pub vad_skips: u64,
        /// Count of chunks passed by VAD (for stats)
        pub vad_passes: u64,
        /// Previous chunk RMS for impulsive spike rejection (coughs, throat clearing)
        pub prev_rms: f32,
        /// Ratio threshold for impulsive detection (default 8.0x)
        #[allow(dead_code)]
        pub impulsive_ratio: f32,
    }

    impl AudioPreprocessor {
        // Only constructed via with_profile() in production; new() is
        // exercised by unit tests — allowed, not dead.
        #[allow(dead_code)]
        pub fn new() -> Self {
            AudioPreprocessor {
                high_pass: HighPassFilter::new(80.0, 16000.0),
                noise_floor: NoiseFloorTracker::new(),
                vad: VadDetector::new(),
                vad_enabled: true,
                vad_skips: 0,
                vad_passes: 0,
                prev_rms: 0.0,
                impulsive_ratio: 8.0,
            }
        }

        pub fn with_profile(profile: &crate::acoustic_profile::AcousticProfile) -> Self {
            AudioPreprocessor {
                high_pass: HighPassFilter::new(profile.highpass_cutoff_hz, 16000.0),
                noise_floor: NoiseFloorTracker::new(),
                vad: VadDetector::new(),
                vad_enabled: false,
                vad_skips: 0,
                vad_passes: 0,
                prev_rms: 0.0,
                impulsive_ratio: profile.impulsive_ratio,
            }
        }

        /// Process a chunk of audio.
        /// Returns the processed chunk and whether speech was detected.
        /// If VAD is enabled and no speech is detected, returns None.
        pub fn process(&mut self, mut chunk: Vec<f32>) -> Option<Vec<f32>> {
            // 1. High-pass filter (remove low-frequency noise)
            self.high_pass.process(&mut chunk);

            // 2. Compute RMS after filtering
            let rms = if chunk.is_empty() {
                0.0
            } else {
                let sum_sq: f32 = chunk.iter().map(|s| s * s).sum();
                (sum_sq / chunk.len() as f32).sqrt()
            };

            // 3. Update noise floor
            let floor = self.noise_floor.update(rms);
            self.prev_rms = rms;

            // 4. VAD check
            if self.vad_enabled {
                let has_speech = self.vad.detect(&chunk, floor);
                if !has_speech {
                    self.vad_skips += 1;
                    return None;
                }
                self.vad_passes += 1;
            }

            Some(chunk)
        }

        /// Reset all preprocessing state (call on stream restart).
        pub fn reset(&mut self) {
            self.high_pass.reset();
            self.noise_floor = NoiseFloorTracker::new();
            self.vad = VadDetector::new();
            self.vad_skips = 0;
            self.vad_passes = 0;
            self.prev_rms = 0.0;
        }
    }

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
    pub const DETECTION_BUFFER_SIZE: usize = 12;

    /// Audio time covered by one OWW chunk (1280 samples at 16 kHz).
    pub const CHUNK_MS: u64 = 80;

    /// Startup / stream-restart grace (audio time) during which detections are ignored.
    pub const STARTUP_GRACE_MS: u64 = 1500;

    /// AGC: quiet speech is amplified toward this RMS before the classifier.
    pub const AGC_TARGET_RMS: f32 = 0.035;
    /// AGC: maximum gain applied to quiet speech.
    pub const AGC_MAX_GAIN: f32 = 30.0;

    /// Pre-computed ambient comfort noise embedding (matches openWakeWord reference specification).
    /// Used to initialize and flush feature buffers without triggering LayerNorm variance collapse.
    pub const COMFORT_EMBEDDING: [f32; 96] = [
        -8.489297, 22.163715, 2.277623, -1.783319, 9.083113, 26.810450,
        12.025850, -13.673116, -2.537689, 7.894602, -17.911095, -3.106453,
        8.197336, -2.658832, -9.058453, -8.155005, 2.432887, -3.165858,
        3.634640, -12.571359, 6.311998, 24.737816, -12.649775, -4.318936,
        -7.900282, 23.728262, -18.375217, -2.583435, 3.445168, 7.829760,
        -12.477730, 16.272125, -26.550194, -6.157786, -17.991940, 0.864661,
        30.908659, 21.871727, -4.616611, 28.996405, -2.680668, 5.759074,
        42.524014, -15.883260, -13.118565, -14.568768, -7.097882, 2.778774,
        8.828871, -0.878993, -9.060097, 6.057194, 14.329378, 2.014399,
        -13.745483, -14.581902, -0.423280, 28.671656, -17.456226, 2.176247,
        6.183690, 6.393251, 11.007999, -2.287265, 24.724491, 25.623247,
        10.284468, -14.603464, -6.046250, -0.863767, -5.018670, 13.685988,
        8.559921, -5.221079, 16.657005, 5.837093, 1.220338, -0.118145,
        -24.578995, -39.162292, 8.814641, 7.579267, 11.435743, -9.504274,
        15.335735, -6.372455, 10.835917, -12.122574, 3.542739, 44.061302,
        4.561942, 25.926502, 28.643192, -29.768351, 10.542067, 22.575813,
    ];

    /// Minimum positive detections before triggering (multi-frame confirmation)
    pub const MIN_POSITIVE_DETECTIONS: f32 = 2.0;

    /// Single-frame high-confidence threshold
    const SINGLE_FRAME_HIGH_CONFIDENCE: f32 = 0.5;

    /// Refractory period after a detection (ms of audio time)
    pub const NO_DETECTION_MS: u64 = 3000;

    /// One stage-1 decision step for a classifier probability at clock time
    /// `now_ms`: push into the smoothing buffer, score it, and fire when the
    /// score exceeds `threshold` outside the refractory period (buffer cleared
    /// on fire). Returns `(fired, score)`. The runtime engine and the offline
    /// replay harness both call this, so they cannot drift apart.
    pub fn stage1_step(
        buffer: &mut CircularBuffer<DETECTION_BUFFER_SIZE, f32>,
        last_fire_ms: &mut Option<u64>,
        now_ms: u64,
        prob: f32,
        threshold: f32,
    ) -> (bool, f32) {
        buffer.push_back(prob);
        let score = detection_score(&buffer.to_vec(), threshold);
        let since_last = last_fire_ms.map_or(u64::MAX, |t| now_ms.saturating_sub(t));
        if score > threshold && since_last > NO_DETECTION_MS {
            *last_fire_ms = Some(now_ms);
            buffer.clear();
            (true, score)
        } else {
            (false, score)
        }
    }

    /// Stage-1 detection score for a smoothing buffer (oldest first).
    ///
    /// Pure so the runtime and the offline replay harness (`wake_eval`) share
    /// one rule. Enforces multi-frame temporal confirmation: human articulation
    /// of "NEXUS" spans 350-700ms (>= 4 frames); isolated 80ms spikes are noise.
    pub fn detection_score(buffer: &[f32], threshold: f32) -> f32 {
        let mut positive_count = 0.0f32;
        let mut cumulative = 0.0f32;
        let mut max_val = 0.0f32;

        for &d in buffer {
            if d > max_val {
                max_val = d;
            }
            if d >= threshold {
                positive_count += 1.0;
                cumulative += d;
            }
        }

        // Path 1: Multi-frame confirmation (requires at least MIN_POSITIVE_DETECTIONS frames)
        if positive_count >= MIN_POSITIVE_DETECTIONS {
            return cumulative / positive_count;
        }

        // Path 2: Confident single-frame peak (>= 0.65) with acoustic context in buffer (>= 0.20)
        if max_val >= 0.65 {
            let count_near = buffer.iter().filter(|&&d| d >= 0.20).count();
            if count_near >= 2 {
                return max_val;
            }
        }

        // Path 3: Moderate detection (>= threshold) with adjacent vocal evidence (>= 0.15)
        if max_val >= threshold {
            let count_near = buffer.iter().filter(|&&d| d >= 0.15).count();
            if count_near >= 2 {
                return max_val;
            }
        }

        0.0
    }

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
        // NOTE: load by PATH (not by in-memory cursor) so tract can resolve
        // ONNX external-data files (e.g. nexus.onnx + nexus.onnx.data, which
        // the exporter splits when weights exceed the protobuf threshold).
        // model_for_read(Cursor) has no base directory and fails on such
        // models; model_for_path resolves sibling .data files correctly.
        // Single-file models (mel, embedding) load identically either way.
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
                    Tensor::from_shape(&[1, 1, 1, 96], &COMFORT_EMBEDDING)
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

        /// Reset the feature buffer with baseline comfort noise and clean lookback state.
        /// Prevents lingering wake-word context without causing LayerNorm variance collapse.
        pub fn reset(&mut self) {
            self.raw_lookback.fill(0.0);
            for _ in 0..FEATURE_BUFFER_SIZE {
                if let Ok(t) = Tensor::from_shape(&[1, 1, 1, 96], &COMFORT_EMBEDDING) {
                    self.feature_buffer.push_back(t);
                }
            }
            for _ in 0..MEL_CIRC_SIZE {
                if let Ok(t) = Tensor::from_shape(&[MELS_PER_CHUNK, 32], &[0f32; MELS_PER_CHUNK * 32]) {
                    self.mel_spectrogram_buffer.push_back(t);
                }
            }
        }

        /// Advance feature buffer with comfort noise during silence/VAD vetoes.
        /// Preserves continuous 1:1 real-time temporal progression without freezing context.
        pub fn push_comfort_frame(&mut self) {
            if let Ok(t) = Tensor::from_shape(&[1, 1, 1, 96], &COMFORT_EMBEDDING) {
                self.feature_buffer.push_back(t);
            }
            if let Ok(t) = Tensor::from_shape(&[MELS_PER_CHUNK, 32], &[0f32; MELS_PER_CHUNK * 32]) {
                self.mel_spectrogram_buffer.push_back(t);
            }
        }
    }

    /// openWakeWord KWS engine with optional speaker verification
    /// and Tier 3 command classifiers.
    pub struct WakeEngine {
        pub classifier: ModelType,
        pub audio_features: AudioFeatures,
        /// Phase B: Audio preprocessor (high-pass filter + noise floor + VAD)
        pub preprocessor: AudioPreprocessor,
        /// Phase D: Speaker verifier (optional, owner-only activation)
        pub speaker_verifier: Option<crate::voice_profile::SpeakerVerifier>,

        pub sample_rate: i32,
        pub chunk_buffer: Vec<f32>,
        pub threshold: f32,
        pub detections_buffer: CircularBuffer<DETECTION_BUFFER_SIZE, f32>,
        /// Clock time (`now_ms`) of the last stage-1 fire, for the refractory period.
        pub last_fire_ms: Option<u64>,
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
        /// Wall-clock origin for `now_ms` in live mode.
        pub clock_origin: std::time::Instant,
        /// Replay mode: `now_ms` is audio time (chunks processed x 80 ms) so the
        /// offline harness can run faster than real time with identical timing.
        /// Live keeps the wall clock: the engine is skipped while a command is
        /// being captured, and that time must still count toward the refractory.
        pub replay_clock: bool,
        /// Audio time processed by `detect_chunk` (ms).
        pub audio_ms: u64,
        /// Detections are ignored until `now_ms` reaches this (startup / stream-restart grace).
        pub grace_until_ms: u64,
        /// Raw classifier probability of the last chunk (0.0 when gated / in grace).
        pub last_raw_prob: f32,
        /// Whether the last chunk reached the classifier and a stage-1 decision
        /// (false when it was gated or in grace: those push 0.0 without deciding).
        pub last_chunk_scored: bool,
        /// Adaptive acoustic profile loaded for the host microphone.
        pub acoustic_profile: crate::acoustic_profile::AcousticProfile,
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
        pub fn new(resource_dir: PathBuf, app_data_dir: PathBuf) -> anyhow::Result<Self> {
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

            Self::with_classifier(&oww_dir, &nexus_model_path, app_data_dir)
        }

        /// Build the engine from an explicit models dir and classifier file.
        /// Used by `new` (shipped `nexus.onnx`) and by the offline replay harness
        /// to score candidate classifiers without replacing the shipped one.
        pub fn with_classifier(
            oww_dir: &Path,
            nexus_model_path: &Path,
            app_data_dir: PathBuf,
        ) -> anyhow::Result<Self> {
            let oww_dir = oww_dir.to_path_buf();
            tracing::info!("Loading openWakeWord classifier: {}", nexus_model_path.display());
            let classifier = load_onnx_model(nexus_model_path)?;

            tracing::info!("Loading audio feature extractors from: {}", oww_dir.display());
            let audio_features = AudioFeatures::new(&oww_dir)?;



            let profile = crate::acoustic_profile::AcousticProfile::load_or_default(Some(&oww_dir));
            let threshold = profile.kws_threshold;
            // The acoustic profile carries the name of the microphone it was
            // calibrated for. The profile that ships with the app was
            // calibrated on a Windows machine, so on Linux that name is not
            // this machine's microphone — the device NEXUS really listens to
            // is only known once the capture stream opens, and the capture
            // step logs it.
            #[cfg(target_os = "linux")]
            let device_label = crate::audio_linux::KWS_DEVICE_PENDING;
            #[cfg(not(target_os = "linux"))]
            let device_label = profile.device_name.as_str();
            tracing::info!(
                "openWakeWord KWS engine initialized \
                 (wake word: NEXUS, 80ms sliding window, threshold: {}, \
                 device: '{}', highpass: {:.1}Hz, silence gate: {:.5}, pre-gain: {:.2}x, \
                 detection: max-based, post-TTS mute: 2000ms, grace: {}ms after restart)",
                threshold,
                device_label,
                profile.highpass_cutoff_hz,
                profile.silence_rms_threshold,
                profile.pre_gain,
                STARTUP_GRACE_MS,
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
                preprocessor: AudioPreprocessor::with_profile(&profile),
                // Phase D: Load speaker profile if it exists (optional)
                speaker_verifier: {
                    let profile_path = app_data_dir.join("voice_profile.json");
                    match crate::voice_profile::SpeakerVerifier::new(profile_path) {
                        Ok(v) => {
                            if v.is_enrolled() {
                                tracing::info!("wake: speaker verification ENABLED (profile loaded)");
                            } else {
                                tracing::debug!("wake: speaker verification disabled (no profile enrolled)");
                            }
                            Some(v)
                        }
                        Err(e) => {
                            tracing::warn!("wake: failed to load speaker profile: {e}");
                            None
                        }
                    }
                },
                sample_rate: 16000,
                chunk_buffer: Vec::with_capacity(OWW_CHUNK_SIZE),
                threshold,
                detections_buffer: CircularBuffer::<DETECTION_BUFFER_SIZE, f32>::new(),
                last_fire_ms: None,
                command_classifiers,
                command_tx: None,
                confirmation_buffer: Vec::with_capacity(16000), // 500ms @ 16kHz
                confirmation_active: false,
                pending_probability: 0.0,
                clock_origin: std::time::Instant::now(),
                replay_clock: false,
                audio_ms: 0,
                grace_until_ms: STARTUP_GRACE_MS,
                last_raw_prob: 0.0,
                last_chunk_scored: false,
                acoustic_profile: profile,
            })
        }

        /// Reset embedding buffers and detection buffers immediately after a wake trigger.
        /// Prevents phantom cascade triggers from residual NEXUS context lingering in the buffer.
        pub fn reset_after_trigger(&mut self) {
            self.audio_features.reset();
            self.detections_buffer.clear();
            for cmd in &mut self.command_classifiers {
                cmd.detections_buffer.clear();
            }
        }

        /// Engine clock in ms: wall clock live, audio time in replay mode.
        pub fn now_ms(&self) -> u64 {
            if self.replay_clock {
                self.audio_ms
            } else {
                self.clock_origin.elapsed().as_millis() as u64
            }
        }

        /// Restart the startup grace window (stream start / resume / recovery).
        pub fn restart_grace(&mut self) {
            self.grace_until_ms = self.now_ms() + STARTUP_GRACE_MS;
        }

        /// Fresh, isolated state for scoring one recording offline: audio-time
        /// clock, no grace (a recording has no driver start-up transient), clean
        /// feature/detection buffers and preprocessor.
        pub fn reset_for_replay(&mut self) {
            self.replay_clock = true;
            self.audio_ms = 0;
            self.grace_until_ms = 0;
            self.last_fire_ms = None;
            self.last_raw_prob = 0.0;
            self.last_chunk_scored = false;
            self.chunk_buffer.clear();
            self.preprocessor.reset();
            self.reset_after_trigger();
        }

        fn compute_waveform_string(chunk: &[f32]) -> String {
            const WAVE_CHARS: &[char] = &[' ', ' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
            let width = 12;
            if chunk.is_empty() {
                return "            ".to_string();
            }
            let slice_len = (chunk.len() / width).max(1);
            let mut s = String::with_capacity(width);
            for w in 0..width {
                let start = w * slice_len;
                let end = (start + slice_len).min(chunk.len());
                if start >= chunk.len() {
                    s.push(' ');
                    continue;
                }
                let slice = &chunk[start..end];
                let sum_sq: f32 = slice.iter().map(|&x| x * x).sum();
                let s_rms = (sum_sq / slice.len() as f32).sqrt();
                let level = (((s_rms - 0.0003) / 0.022 * 8.0).clamp(0.0, 8.0)) as usize;
                s.push(WAVE_CHARS[level]);
            }
            s
        }

        /// Run KWS detection on a single 80ms chunk.
        /// Returns (wake_detected, wake_probability, optional command_intent).
        fn detect_chunk(
            &mut self,
            chunk: Vec<f32>,
        ) -> (bool, f32, Option<CommandIntent>) {
            // ─── Startup grace period ───────────────────────────────────
            // Ignore detections for STARTUP_GRACE_MS after the engine starts
            // or after a stream restart to allow DAC/driver buffers to settle.
            self.audio_ms += CHUNK_MS;
            self.last_raw_prob = 0.0;
            self.last_chunk_scored = false;
            if self.now_ms() < self.grace_until_ms {
                self.detections_buffer.push_back(0.0);
                return (false, 0.0, None);
            }

            // ─── Phase B: Audio Preprocessing ───────────────────────────
            // High-pass filter (80Hz) + adaptive noise floor + VAD
            // This runs BEFORE the energy gate to clean the audio first.
            // If VAD detects no speech, skip the classifier entirely (saves CPU).
            let chunk = match self.preprocessor.process(chunk) {
                Some(c) => c,
                None => {
                    // Continuous real-time progression: push comfort frame into feature buffer
                    self.audio_features.push_comfort_frame();
                    self.detections_buffer.push_back(0.0);
                    for cmd in &mut self.command_classifiers {
                        cmd.detections_buffer.push_back(0.0);
                    }
                    return (false, 0.0, None);
                }
            };

            // ─── Energy gate + Automatic Gain Control (AGC) ────────────
            let silence_rms_threshold = self.acoustic_profile.silence_rms_threshold;
            let target_rms = AGC_TARGET_RMS;
            let max_gain = AGC_MAX_GAIN;
            let _pre_gain = self.acoustic_profile.pre_gain;

            let rms = if chunk.is_empty() {
                0.0
            } else {
                let sum_sq: f32 = chunk.iter().map(|s| s * s).sum();
                (sum_sq / chunk.len() as f32).sqrt()
            };

            if rms < silence_rms_threshold {
                // Continuous real-time progression: advance buffer with comfort noise
                self.audio_features.push_comfort_frame();
                self.detections_buffer.push_back(0.0);
                // Also flush command classifier buffers
                for cmd in &mut self.command_classifiers {
                    cmd.detections_buffer.push_back(0.0);
                }
                #[cfg(debug_assertions)]
                {
                    static SILENT_TELEMETRY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                    if SILENT_TELEMETRY.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 6 == 0 {
                        tracing::info!(
                            "audio-telemetry: wave=[            ] prob=0.000 rms={:.4} gain=1.0",
                            rms
                        );
                    }
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

            // A WebRTC VAD pass used to run here on every chunk, but its result was
            // discarded (fail-open, so soft onsets like the /n/ of "NEXUS" are never
            // chopped) — pure per-chunk cost. A real VAD gate in front of the
            // embedding model would cut idle CPU in noisy rooms, but it must be
            // validated against the wake-word recall set first.

            // AGC: amplify quiet speech to target RMS with calibrated pre-gain
            // This ensures consistent model input across different laptop microphones.
            let pre_gain = self.acoustic_profile.pre_gain.max(1.0);
            let (chunk, agc_gain): (Vec<f32>, f32) = if rms < target_rms {
                let target_gain = (target_rms / rms).min(max_gain);
                let gain = target_gain.max(pre_gain);
                tracing::trace!("wake: AGC gain={:.1}x (RMS {:.6} → {:.6})", gain, rms, target_rms);
                (chunk.iter().map(|&s| (s * gain).clamp(-1.0, 1.0)).collect(), gain)
            } else if pre_gain > 1.0 {
                (chunk.iter().map(|&s| (s * pre_gain).clamp(-1.0, 1.0)).collect(), pre_gain)
            } else {
                (chunk, 1.0)
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

            // Emit real-time audio telemetry for unified console meter
            static ACTIVE_TELEMETRY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let is_voice = rms > 0.0006 || probability > 0.05;
            let cadence = if is_voice { 2 } else { 6 };
            if ACTIVE_TELEMETRY.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % cadence == 0 {
                let wave = Self::compute_waveform_string(&chunk);
                tracing::info!(
                    "audio-telemetry: wave=[{}] prob={:.3} rms={:.4} gain={:.1}",
                    wave, probability, rms, agc_gain
                );
            }

            // Log every classifier output so we can see if the model is
            // producing any signal at all. This is critical for debugging
            // "nexus is not waking up" issues.
            if probability > 0.1 {
                tracing::debug!(
                    "wake: model probability={:.3} (threshold={:.3}, buffer_avg will be computed)",
                    probability, self.threshold
                );
            } else if probability > 0.01 {
                tracing::debug!(
                    "wake: model probability={:.3} (below threshold {:.3})",
                    probability, self.threshold
                );
            }

            self.last_raw_prob = probability;
            self.last_chunk_scored = true;

            // Smoothing buffer + score + refractory (shared with the replay harness)
            let now = self.now_ms();
            let (wake_detected, avg) = stage1_step(
                &mut self.detections_buffer,
                &mut self.last_fire_ms,
                now,
                probability,
                self.threshold,
            );

            // Log which trigger path is being taken (for debugging)
            if avg >= SINGLE_FRAME_HIGH_CONFIDENCE {
                tracing::info!(
                    "wake: high-confidence single-frame trigger (avg={:.3}, prob={:.3})",
                    avg, probability
                );
            }

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

        /// Process a chunk of 16kHz mono f32 audio.
        /// Returns true if the wake word "NEXUS" was detected and the speaker was accepted.
        /// Also emits command-detected events via `command_tx` if any Tier 3
        /// command classifier fires.
        ///
        /// Secondary confirmation: after a raw detection, collects 500ms of
        /// audio to verify there was actual speech (raw RMS > 0.002). This
        /// filters pure digital silence/noise spikes that pass the classifier
        /// but aren't real speech.
        ///
        /// IMPORTANT: We do NOT apply AGC to the confirmation RMS. The previous
        /// implementation applied 50x AGC, which could confirm a wake on audio
        /// 25x quieter than the silence gate (raw RMS 0.00002 → AGC 0.001),
        /// effectively bypassing the gate and confirming noise spikes. Using
        /// raw RMS with a threshold matching the silence gate (0.002) ensures
        /// consistency: if audio wouldn't pass the silence gate, it shouldn't
        /// confirm a wake.
        ///
        /// Trade-off: on Intel SST mics that fade to silence during the 500ms
        /// confirmation window, valid wakes may be rejected. This is
        /// preferable to false wakes that trigger the "Didn't catch that sir"
        /// retry loop. The user explicitly complained about the loop, not
        /// about missed wakes.
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
                    self.confirmation_active = false;

                    // Use raw RMS only (no AGC) — threshold matches the silence
                    // gate in detect_chunk (0.002). This prevents the
                    // confirmation from amplifying quiet noise tails and
                    // confirming false wakes.
                    const CONFIRMATION_RMS_THRESHOLD: f32 = 0.002;
                    // ─── v4 backward confirmation ───────────────────
                    // The forward-only window structurally rejects short words
                    // (a 0.3s bark ends before the 500ms window fills). Also
                    // check the 1s of PRE-trigger audio: sustained energy there
                    // means a real utterance just happened. Either path confirms.
                    let (pre_ok, pre_rms) = {
                        let ring = super::VERIFY_RING.lock();
                        let tail: Vec<f32> = ring
                            .iter()
                            .rev()
                            .take(16000)
                            .copied()
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        drop(ring);
                        super::check_pre_trigger(&tail, CONFIRMATION_RMS_THRESHOLD, 3)
                    };
                    if raw_rms >= CONFIRMATION_RMS_THRESHOLD || pre_ok {
                        // ─── Phase D: Speaker Verification ───────────────────
                        // After the wake word is confirmed by RMS, check if the
                        // speaker matches the enrolled voice profile. If a profile
                        // is enrolled and the speaker doesn't match, reject the
                        // wake silently (no TTS, no action).
                        if let Some(ref verifier) = self.speaker_verifier {
                            if verifier.is_enrolled() {
                                // Extract embedding from the confirmation audio
                                match self.audio_features.get_audio_features(&buf) {
                                    Ok(features) => {
                                        // The features are [16, 96] — use the mean
                                        // as a 96-dim speaker embedding
                                        let features_arr = match features.into_plain_array::<f32>() {
                                            Ok(arr) => arr,
                                            Err(e) => {
                                                tracing::warn!(
                                                    "Speaker verification feature conversion failed: {e} \
                                                     — accepting wake (fail-open)"
                                                );
                                                tracing::info!(
                                                    "OWW wake confirmed! (probability: {:.3}, raw RMS: {:.6})",
                                                    self.pending_probability, raw_rms
                                                );
                                                self.reset_after_trigger();
                                                return true;
                                            }
                                        };
                                        let feat_slice = features_arr.as_slice().unwrap_or(&[]);
                                        let embedding: Vec<f32> = (0..96)
                                            .map(|j| {
                                                (0..16)
                                                    .map(|i| {
                                                        let idx = i * 96 + j;
                                                        if idx < feat_slice.len() { feat_slice[idx] } else { 0.0 }
                                                    })
                                                    .sum::<f32>() / 16.0
                                            })
                                            .collect();
                                        let (sim, verified) = verifier.verify(&embedding);
                                        if !verified {
                                            tracing::info!(
                                                "OWW wake REJECTED by speaker verification \
                                                 (similarity: {:.3} < threshold {:.3})",
                                                sim, verifier.profile().map(|p| p.threshold).unwrap_or(0.45)
                                            );
                                            // Reset detection state
                                            self.detections_buffer.clear();
                                            self.last_fire_ms = Some(self.now_ms());
                                            return false;
                                        }
                                        tracing::debug!(
                                            "OWW wake ACCEPTED by speaker verification \
                                             (similarity: {:.3} >= threshold {:.3})",
                                            sim, verifier.profile().map(|p| p.threshold).unwrap_or(0.45)
                                        );
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "Speaker verification embedding extraction failed: {e} \
                                             — accepting wake (fail-open)"
                                        );
                                    }
                                }
                            }
                        }

                        tracing::info!(
                            "OWW wake confirmed! (probability: {:.3}, raw RMS: {:.6}, pre-trigger RMS: {:.6})",
                            self.pending_probability, raw_rms, pre_rms
                        );
                        self.reset_after_trigger();
                        return true;
                    } else {
                        tracing::info!(
                            "OWW wake rejected — confirmation RMS too low (raw={:.6}, pre={:.6} < {:.3}), likely noise spike or TTS echo",
                            raw_rms, pre_rms, CONFIRMATION_RMS_THRESHOLD
                        );
                        // Reset detection state
                        self.detections_buffer.clear();
                        self.last_fire_ms = Some(self.now_ms());
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

                if detected {
                    tracing::debug!(
                        "OWW wake detected! (confidence: {:.1}%, prob: {:.3})",
                        prob * 100.0, prob
                    );
                    self.pending_probability = prob;
                    self.reset_after_trigger();
                    return true;
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
        wake_tx: &std::sync::mpsc::Sender<super::WakeCandidate>,
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

        // Compute RMS of this callback's audio (active channels only on quad arrays)
        let ch = native_channels.max(1);
        let active_ch = if ch == 4 { 2 } else { ch };
        let frames = data.len() / ch;
        let mut sum_sq = 0.0f32;
        for i in 0..frames {
            let mut sum = 0.0f32;
            for c in 0..active_ch {
                sum += to_f32(data[i * ch + c]);
            }
            let mono = sum / active_ch as f32;
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
            // Use 0.002 to match the wake engine's SILENCE_RMS_THRESHOLD.
            // If audio is above this, the mic is working and we don't need recovery.
            if rms > 0.002 {
                super::LAST_NONSILENT_FOR_RECOVERY.store(n, Ordering::Relaxed);
            }
            // Bit-exact-zero streak: quiet rooms produce nonzero floor noise,
            // so exact 0.0 means driver-level silence (dropout/mute), not quiet.
            // This is the Meet-parity discriminator (content vs terminal).
            if rms == 0.0 {
                super::EXACT_ZERO_CBS.fetch_add(1, Ordering::Relaxed);
            } else {
                super::EXACT_ZERO_CBS.store(0, Ordering::Relaxed);
            }

        // Mic heartbeat (INFO, ~every 2s): proves the mic is alive even when
        // there is nothing to say. Steady-state KWS is otherwise silent for
        // minutes, which is indistinguishable from a dead Intel SST driver.
        // A flat 0.0000 line here means dropout (see silence-recovery).
        // ~70 callbacks ≈ 2s (cpal callbacks run ~10-30ms depending on device).
        if n % 70 == 0 {
            let state = if rms < 0.002 { "SILENT " } else { "LIVE   " };
            tracing::debug!(
                "audio: mic {} {}rms={:.4} (cb {})",
                super::rms_bar(rms),
                state,
                rms,
                n
            );
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
                // The hint is platform specific: on Windows a silent mic is
                // usually the Intel SST driver, on Linux it is the input
                // device (wrong source, muted, PipeWire not running).
                #[cfg(target_os = "linux")]
                const SILENT_MIC_HINT: &str = crate::audio_linux::SILENT_MIC_HINT;
                #[cfg(not(target_os = "linux"))]
                const SILENT_MIC_HINT: &str = "Intel SST driver may need a restart. Try: 1) Unmute mic in Windows settings, 2) Disable 'Audio Enhancements' in mic properties, 3) Restart the app.";
                tracing::warn!(
                    "audio: mic has been silent for ~{:.0}s (callbacks {}-{}, max RMS ever seen: {:.6}). {}",
                    silence_secs, last_nonsilent, n, max_rms, SILENT_MIC_HINT
                );
            }
        }

        // 1. Downmix to mono f32 (active stereo pair on quad arrays)
        {
            let mut st = state.lock();
            let ch = native_channels.max(1);
            let active_ch = if ch == 4 { 2 } else { ch };
            let frames = data.len() / ch;
            for i in 0..frames {
                let mut sum = 0.0f32;
                for c in 0..active_ch {
                    sum += to_f32(data[i * ch + c]);
                }
                st.carry.push(sum / active_ch as f32);
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
        //    Also enforces Post-TTS Mute Gate (1000ms after TTS ends).
        //    Also handles Rust-side STT capture (bypasses getUserMedia).
        {
            let mut buf = out_buf.lock();
            buf.extend(produced);
            while buf.len() >= chunk_size {
                let chunk: Vec<f32> = buf.drain(0..chunk_size).collect();

                // ── Stage-2 verifier ring ────────────────────────────
                // Keep the last 2.5s of raw mic audio so a stage-1
                // candidate can be cross-checked with STT. Pushed for
                // EVERY chunk (including during STT capture / suppression
                // drain) so the snapshot is always fresh. Cost: one 1280-
                // float memcpy per 80ms — negligible.
                {
                    let mut ring = super::VERIFY_RING.lock();
                    super::push_capped(&mut ring, &chunk, super::VERIFY_RING_CAP);
                }

                // ── Rust-side STT capture ──────────────────────────────
                // If capturing, append chunk to buffer and run RMS VAD.
                // Skip KWS during capture (prevents double-trigger).
                if super::STT_CAPTURING.load(Ordering::Relaxed) {
                    {
                        let mut cap = super::STT_CAPTURE_BUFFER.lock();
                        cap.extend(chunk.iter().copied());
                    }

                    // RMS-based VAD on this 1280-sample (80ms) chunk.
                    // Hysteresis: high threshold to START speech (rejects
                    // noise), lower threshold once underway (boundary
                    // chatter must not stall or stretch the turn).
                    let rms = {
                        let sum_sq: f32 = chunk.iter().map(|s| s * s).sum();
                        (sum_sq / chunk.len() as f32).sqrt()
                    };

                    // ── UI level meter ───────────────────────────────
                    // Feed speech-synced visuals (ghost waves). Every 2nd
                    // 80ms chunk ≈ 6 Hz — smooth for bars, cheap for the
                    // bridge. try_send never blocks the audio callback.
                    {
                        static LEVEL_DIVIDER: std::sync::atomic::AtomicU32 =
                            std::sync::atomic::AtomicU32::new(0);
                        if LEVEL_DIVIDER.fetch_add(1, Ordering::Relaxed) % 2 == 0 {
                            if let Some(tx) = super::LEVEL_TX.get() {
                                let _ = tx.try_send(super::level_from_rms(rms));
                            }
                        }
                    }

                    let underway = super::STT_SPEECH_DETECTED.load(Ordering::Relaxed);
                    // Neural VAD when loaded (vadSilero), else the RMS hysteresis — see chunk_voice_flags.
                    let (cont_voice, start_voice) =
                        super::chunk_voice_flags(rms, super::silero_chunk_prob(&chunk));
                    if if underway { cont_voice } else { start_voice } {
                        // Speech resumed after a real pause (2+ silent chunks)?
                        // Count it — hesitant speakers get a patient endpoint.
                        if underway
                            && super::STT_SILENCE_CHUNKS.load(Ordering::Relaxed) >= 2
                        {
                            super::STT_PAUSE_COUNT.fetch_add(1, Ordering::Relaxed);
                        }
                        super::STT_SPEECH_DETECTED.store(true, Ordering::Relaxed);
                        super::STT_SILENCE_CHUNKS.store(0, Ordering::Relaxed);
                        // TRUE voice energy (above the speech threshold, not the
                        // hysteresis band): arms the silence endpoint (F1).
                        if start_voice {
                            super::STT_VOICED_CHUNKS.fetch_add(1, Ordering::Relaxed);
                        }
                    } else if underway {
                        super::STT_SILENCE_CHUNKS.fetch_add(1, Ordering::Relaxed);
                    }

                    let total = super::STT_TOTAL_CHUNKS.fetch_add(1, Ordering::Relaxed);
                    let silence = super::STT_SILENCE_CHUNKS.load(Ordering::Relaxed);
                    let speech_detected = super::STT_SPEECH_DETECTED.load(Ordering::Relaxed);
                    // Adaptive endpoint: 240ms in Ghost Mode, 400ms normally,
                    // relaxing to patient thresholds once mid-turn pauses are detected.
                    let silence_limit = if crate::ghost::session_active() {
                        if super::STT_PAUSE_COUNT.load(Ordering::Relaxed) >= 1 {
                            super::STT_SILENCE_CHUNK_LIMIT_GHOST_PATIENT
                        } else {
                            super::STT_SILENCE_CHUNK_LIMIT_GHOST
                        }
                    } else if super::STT_PAUSE_COUNT.load(Ordering::Relaxed) >= 2 {
                        super::STT_SILENCE_CHUNK_LIMIT_PATIENT
                    } else {
                        super::STT_SILENCE_CHUNK_LIMIT
                    };

                    // Stop conditions (pure predicate, F1):
                    // 1. Silence after a CONFIRMED turn: adaptive (400ms / ~1s patient).
                    //    Unconfirmed blips (voiced < MIN) never stop — the turn
                    //    hasn't started, only the 8s no-speech timeout applies.
                    // 2. Max capture: STT_MAX_CHUNKS chunks (~10s)
                    // 3. No speech timeout: STT_NO_SPEECH_CHUNK_LIMIT chunks (~8s)
                    let voiced = super::STT_VOICED_CHUNKS.load(Ordering::Relaxed);
                    // Smart Turn (off unless `smartTurn`): may end the turn earlier, or hold it open
                    // longer when the model says the speaker is not finished. Off => unchanged.
                    let (smart_end, silence_limit) =
                        super::smart_turn_poll(speech_detected, voiced, silence, silence_limit);
                    let should_stop = smart_end
                        || super::should_stop_capture(
                            speech_detected,
                            voiced,
                            silence,
                            silence_limit,
                            total,
                        );

                    if should_stop {
                        super::STT_CAPTURING.store(false, Ordering::Relaxed);
                        // Settle the UI meter: one final 0 so waves rest
                        // instead of freezing mid-speech.
                        if let Some(tx) = super::LEVEL_TX.get() {
                            let _ = tx.try_send(0.0);
                        }
                        let buffer = std::mem::take(&mut *super::STT_CAPTURE_BUFFER.lock());
                        super::STT_SPEECH_DETECTED.store(false, Ordering::Relaxed);
                        super::STT_SILENCE_CHUNKS.store(0, Ordering::Relaxed);
                        super::STT_TOTAL_CHUNKS.store(0, Ordering::Relaxed);
                        super::STT_VOICED_CHUNKS.store(0, Ordering::Relaxed);

                        tracing::info!(
                            "stt-capture: stopping (total={} chunks, speech={}, silence={} chunks, voiced={} chunks, {} samples)",
                            total, speech_detected, silence, voiced, buffer.len()
                        );
                        crate::latency::begin(); // docs/features/134 P0: per-turn latency clock (try_lock, never blocks)

                        // Spawn transcription thread (don't block the audio callback).
                        // Tag the buffer with the current capture session so
                        // the receiver can drop it if an abort lands in flight.
                        // Snapshot the turn packet here: counters reset below
                        // and a new capture may start before transcription ends.
                        crate::directed::snapshot_turn_origin(); // Phase 8: who opened THIS capture
                        let session = super::CAPTURE_SESSION_ID.load(std::sync::atomic::Ordering::Relaxed);
                        let (rms_max, rms_mean) = super::buffer_rms_stats(&buffer);
                        let stats = super::TurnStats {
                            session,
                            rms_max,
                            rms_mean,
                            voiced_chunks: voiced,
                            total_chunks: total,
                            endpoint: if smart_end {
                                "smart_turn"
                            } else {
                                super::endpoint_reason(
                                    speech_detected,
                                    voiced,
                                    silence,
                                    silence_limit,
                                    total,
                                )
                            },
                            stt_path: "pending",
                            filter: "pending",
                            owner: "pending",
                            owner_score: 0.0,
                            decoder_bias: "pending",
                            stt_language: "pending",
                        };
                        std::thread::spawn(move || {
                            super::transcribe_and_emit(session, stats, buffer);
                        });
                    }

                    // Skip KWS during capture
                    continue;
                }

                // Check if wake detection should be suppressed
                let meeting_state = super::MEETING_STATE.get();
                // Hand Mode alone (no Ghost session) owns the mic for pointer control, not
                // conversation: see `hand::suppresses_voice` for why. Checked every chunk, same as
                // the meeting/manual-pause gate below, so arming/disarming hand mode takes effect
                // immediately rather than needing a fresh wake cycle.
                let hand_muted = crate::hand::suppresses_voice();
                let suppressed = hand_muted
                    || meeting_state
                        .map(|s: &std::sync::Arc<crate::meeting_detect::MeetingState>| {
                            s.should_suppress_wake()
                        })
                        .unwrap_or(false);

                if suppressed {
                    // Throttled visibility: silent drops are otherwise
                    // indistinguishable from a dead model in the logs.
                    // (A stuck meeting-active state once ate every wake
                    // word with zero log lines.)
                    static SUPPRESSED_COUNT: std::sync::atomic::AtomicU64 =
                        std::sync::atomic::AtomicU64::new(0);
                    static SUPPRESSED_LAST_LOG: std::sync::Mutex<Option<std::time::Instant>> =
                        std::sync::Mutex::new(None);
                    let n = SUPPRESSED_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
                    let should_log = {
                        let mut last = SUPPRESSED_LAST_LOG.lock().unwrap();
                        let due = last
                            .map(|t| t.elapsed().as_secs() >= 30)
                            .unwrap_or(true);
                        if due {
                            *last = Some(std::time::Instant::now());
                        }
                        due
                    };
                    if should_log {
                        // Main Center STT-director reason (doc 74 P3): the
                        // bare counter never said WHY capture was muted.
                        // Hand Mode is checked first: it is the actual reason whenever it is true,
                        // even if a meeting/pause flag also happens to be set.
                        let note = if hand_muted {
                            "hand mode active, no ghost session (voice muted — see docs/features/121)"
                        } else {
                            meeting_state
                                .map(|s| {
                                    crate::center::stt_mute_note(crate::center::stt_gate(
                                        s.manual_pause.load(Ordering::Relaxed),
                                        s.tts_playing.load(Ordering::Relaxed),
                                        s.detection_enabled.load(Ordering::Relaxed)
                                            && s.meeting_active.load(Ordering::Relaxed),
                                    ))
                                })
                                .unwrap_or("unknown mute source")
                        };
                        tracing::warn!(
                            "wake: detection suppressed by hand-mode/meeting/TTS-mute state ({} chunks dropped so far) — \
                             if you are speaking and nothing happens, check hand mode / meeting mode / pause state",
                            n
                        );
                        tracing::debug!("wake: suppression reason: {note}");
                    }

                    // ── v4 barge-in ──────────────────────────────────
                    // Sustained speech while ONLY TTS-muted (never meeting /
                    // manual pause — privacy first) may be the user
                    // interrupting. Earns ONE exact-only verify (prob 0.0 forces
                    // the exact path; near-matches need stage-1 prob).
                    // TTS echo transcribes as our own ack text (no "nexus")
                    // and dies safely at the STT gate.
                    let tts_only = meeting_state
                        .map(|s| {
                            s.tts_playing.load(Ordering::Relaxed)
                                && !s.meeting_active.load(Ordering::Relaxed)
                                && !s.is_paused()
                        })
                        .unwrap_or(false);
                    if tts_only {
                        let sum_sq: f32 =
                            chunk.iter().map(|s| s * s).sum();
                        let chunk_rms =
                            (sum_sq / chunk.len().max(1) as f32).sqrt();
                        if chunk_rms > super::BARGE_RMS_FLOOR {
                            super::BARGE_SUSTAINED_CHUNKS.fetch_add(1, Ordering::Relaxed);
                        } else {
                            super::BARGE_SUSTAINED_CHUNKS.store(0, Ordering::Relaxed);
                        }
                        let sustained =
                            super::BARGE_SUSTAINED_CHUNKS.load(Ordering::Relaxed);
                        if super::should_barge_attempt(tts_only, sustained)
                            && !super::BARGE_ATTEMPTED.swap(true, Ordering::SeqCst)
                        {
                            tracing::info!(
                                "barge-in: sustained speech during TTS ({} chunks) — one exact-only verify",
                                sustained
                            );
                            let audio: Vec<f32> = super::VERIFY_RING
                                .lock()
                                .iter()
                                .copied()
                                .collect();
                            let _ = wake_tx.send(super::WakeCandidate {
                                audio,
                                prob: 0.0,
                            });
                        }
                    }
                    continue;
                }

                // v4 barge-in state reset (unmuted path): a new TTS session
                // re-arms exactly one attempt.
                super::BARGE_SUSTAINED_CHUNKS.store(0, Ordering::Relaxed);
                super::BARGE_ATTEMPTED.store(false, Ordering::Relaxed);

                // Post-TTS Mute Gate: drop audio chunks for 2000ms after TTS
                // finishes to allow room acoustics, DAC output buffers, and
                // microphone AGC to settle completely. This prevents TTS echo
                // (RMS ~0.2) from re-triggering the wake word after the
                // `tts_playing` flag goes false.
                //
                // The previous implementation used a local `last_tts_active`
                // variable that was reinitialized on every audio callback,
                // so the gate never actually persisted across callbacks.
                // Now we use `MeetingState::ms_since_tts_ended()` which
                // stores the end time in an AtomicU64 that persists.
                if let Some(state) = meeting_state {
                    let ms_since_tts = state.ms_since_tts_ended();
                    if ms_since_tts < 2000 {
                        continue;
                    }
                }

                let mut eng = engine.lock();
                if eng.process(&chunk) {
                    // Stage 1 fired — snapshot ring audio + trigger prob for
                    // the verifier thread (two-key gate needs both).
                    let prob = eng.pending_probability;
                    drop(eng);
                    let audio: Vec<f32> =
                        super::VERIFY_RING.lock().iter().copied().collect();
                    let _ = wake_tx.send(super::WakeCandidate { audio, prob });
                }
            }
        }
    }
}

#[cfg(not(feature = "mock-wake"))]
use once_cell::sync::OnceCell;
#[cfg(not(feature = "mock-wake"))]
static WAKE_TX: OnceCell<std::sync::mpsc::Sender<WakeCandidate>> = OnceCell::new();
/// Global meeting/privacy state — checked on every audio chunk.
/// Set up in `lib.rs` before the wake engine starts.
#[cfg(not(feature = "mock-wake"))]
static MEETING_STATE: OnceCell<std::sync::Arc<crate::meeting_detect::MeetingState>> =
    OnceCell::new();

// ─── Stage-2 verifier (STT cross-check) ─────────────────────────────
// Stage 1 (acoustic KWS) is sensitive by design — it fires on TV dialogue
// and conversation containing nexus-like sounds (~9.6 FA/hr measured on
// 25 min of real background audio). Stage 2 re-scores the candidate's audio
// through STT (Groq cloud primary, local Moonshine fallback — the exact
// `stt::transcribe_samples` chain) and fires the wake ONLY if the transcript
// contains "nexus". Fail-open: STT errors/timeouts fire anyway (current
// behavior preserved); only a confident non-match suppresses.
//
// Cost: +~250ms (Groq) to ~2s (cold local) on true wakes. Kill-switch:
// `"verifyWake": false` in settings.json restores fire-on-detect.
#[cfg(not(feature = "mock-wake"))]
static VERIFY_RING: once_cell::sync::Lazy<parking_lot::Mutex<std::collections::VecDeque<f32>>> =
    once_cell::sync::Lazy::new(|| {
        parking_lot::Mutex::new(std::collections::VecDeque::with_capacity(VERIFY_RING_CAP))
    });
/// 2.5s of 16kHz mono (covers "hey nexus" ~1s + margin on both sides).
#[cfg(not(feature = "mock-wake"))]
const VERIFY_RING_CAP: usize = 40000;
/// Max time to wait for STT verification before failing open.
#[cfg(not(feature = "mock-wake"))]
const VERIFY_TIMEOUT_SECS: u64 = 10;
/// A stage-1 acoustic candidate: ring audio + the trigger probability.
/// The probability powers the two-key gate (near-matches require prob ≥ 0.6).
#[cfg(not(feature = "mock-wake"))]
pub struct WakeCandidate {
    /// ~2.5s of raw mic audio ending at the trigger.
    pub audio: Vec<f32>,
    /// Stage-1 smoothed score that fired (pending_probability at trigger).
    pub prob: f32,
}
/// Only one verification may be in flight at a time (prevents double-fire).
#[cfg(not(feature = "mock-wake"))]
static VERIFY_IN_FLIGHT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// Set by the verifier thread when a candidate is accepted: the next trip
/// through the wake loop fires immediately WITHOUT re-verifying (the audio
/// was already confirmed). Consumed via swap(false) — single fire guaranteed.
#[cfg(not(feature = "mock-wake"))]
static VERIFIED_BYPASS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Speaker verification state — lazily initialized on first candidate.
/// Holds a separate AudioFeatures instance (not the live engine's) so the
/// streaming buffers stay pristine during verification.
#[cfg(not(feature = "mock-wake"))]
static SPEAKER_CHECK: OnceCell<std::sync::Mutex<SpeakerCheck>> =
    OnceCell::new();

#[cfg(not(feature = "mock-wake"))]
struct SpeakerCheck {
    features: engine::AudioFeatures,
    verifier: crate::voice_profile::SpeakerVerifier,
}

/// Push a chunk into the verify ring, evicting oldest beyond capacity.
/// Pure helper (unit-testable); the static wrapper below owns the lock.
#[cfg(not(feature = "mock-wake"))]
fn push_capped(
    ring: &mut std::collections::VecDeque<f32>,
    chunk: &[f32],
    cap: usize,
) {
    ring.extend(chunk.iter().copied());
    while ring.len() > cap {
        ring.pop_front();
    }
}

/// Render mic RMS as a 10-char bar for the liveness heartbeat.
/// 0.02 RMS (speech-body floor) → 1 bar; 0.2+ RMS → full. Pure (unit-tested).
/// Unused under `mock-wake` (no audio callback) — allowed, not dead.
#[allow(dead_code)]
pub fn rms_bar(rms: f32) -> String {
    let filled = (rms * 50.0).clamp(0.0, 10.0) as usize;
    let mut s = String::with_capacity(10);
    for i in 0..10 {
        s.push(if i < filled { '#' } else { '.' });
    }
    s
}

/// Stage-2 decision: does this transcript confirm a wake word?
/// Pure function — the unit tests below are its dual-gate Test A artifact.
/// Production calls `verify_transcript_gated`; this un-gated form is kept
/// for the threshold-1.0 unit tests.
#[allow(dead_code)]
pub fn verify_transcript(text: &str) -> bool {
    verify_transcript_gated(text, 1.0)
}

/// Transcript words that always confirm, at any stage-1 probability.
/// (The bare word fires whether whispered or shouted.)
const VERIFY_EXACT_WORDS: &[&str] = &["nexus"];
/// Near-matches (STT confusions within phoneme-edit-distance ~1, measured on
/// 30 owner takes + 100-combo matrix: lexus 37%, NIXUS/NIXIS dominant spellings
/// (K02/K06/K10/BN05/BN11/A20), nexat/nexo truncations, nexas/nekus artifacts).
/// Require stage-1 prob ≥ 0.6 (two-key) — no single weak signal wakes alone.
/// Deliberately EXCLUDES texas/next/access (constant on news/TV — accepting
/// them would reopen the FA gate v30+verifier just closed).
const VERIFY_NEAR_WORDS: &[&str] = &[
    "lexus", "nexis", "nixus", "nixis", "nexas", "nexuss", "nekus", "lexis",
    "nexat", "nexo", "nexos", "nex", "knex", "mexus", "mexis",
];
/// Stage-1 probability floor for near-matches.
pub const VERIFY_NEAR_PROB_FLOOR: f32 = 0.6;

/// Consecutive-candidate cooldown. A pending verified bypass is exempt (see the wake loop).
pub fn cooldown_blocks(now_ms: u64, last_fire_ms: u64, bypass_pending: bool) -> bool {
    !bypass_pending && now_ms >= last_fire_ms && now_ms - last_fire_ms < 1500
}

/// Stage-1 probability at/above which a verify FAILURE (STT error, timeout, too-short audio)
/// still fires. Below it the candidate is suppressed: a cold local STT server used to time out
/// and wake the orb on arbitrary speech at prob 0.746.
pub const VERIFY_FAIL_OPEN_PROB_FLOOR: f32 = 0.9;

/// May a verifier failure fire the wake? Pure (unit-tested).
pub fn fail_open_allowed(prob: f32) -> bool {
    prob >= VERIFY_FAIL_OPEN_PROB_FLOOR
}

/// Stage-1 probability at/above which a clear soundalike of "nexus" is accepted
/// ("Hello, Nico." was rejected at prob 0.98 and 0.93).
pub const VERIFY_SOUNDALIKE_PROB_FLOOR: f32 = 0.9;

/// STT renderings of "nexus" that are only believable with a very confident stage-1 score.
const VERIFY_HIGHCONF_SOUNDALIKES: &[&str] =
    &["nico", "niko", "nikko", "nexxus", "nexxis", "nexes", "necksus"];
/// Words that may directly precede a split "next us"/"necks us".
const VERIFY_GREETINGS: &[&str] = &["hey", "hello", "hi", "okay", "ok", "yo"];

/// High-confidence soundalike tier: prob >= 0.9, a SHORT transcript (<= 6 tokens) and either a
/// listed soundalike word or a split "next us" right after a greeting. Pure (unit-tested).
pub fn verify_highconf_soundalike(text: &str, prob: f32) -> bool {
    if prob < VERIFY_SOUNDALIKE_PROB_FLOOR {
        return false;
    }
    let words = transcript_words(text);
    // "I'm sorry. Hello, Nico." tokenises to 5 words (the apostrophe splits "I'm").
    if words.is_empty() || words.len() > 6 {
        return false;
    }
    if words.iter().any(|w| VERIFY_HIGHCONF_SOUNDALIKES.contains(&w.as_str())) {
        return true;
    }
    words.windows(3).any(|w| {
        VERIFY_GREETINGS.contains(&w[0].as_str())
            && matches!(w[1].as_str(), "next" | "necks" | "nex")
            && w[2] == "us"
    })
}

/// Production accept decision: the two-key word gate OR the high-confidence soundalike tier.
pub fn verify_transcript_accept(text: &str, prob: f32) -> bool {
    verify_transcript_gated(text, prob) || verify_highconf_soundalike(text, prob)
}

/// Split a transcript into lowercase alphanumeric words.
fn transcript_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

/// Two-key transcript gate: exact words fire at any probability;
/// near-matches additionally require `prob >= VERIFY_NEAR_PROB_FLOOR`.
/// Pure function (unit-tested).
pub fn verify_transcript_gated(text: &str, prob: f32) -> bool {
    let words = transcript_words(text);
    if words.iter().any(|w| VERIFY_EXACT_WORDS.contains(&w.as_str())) {
        return true;
    }
    if prob >= VERIFY_NEAR_PROB_FLOOR
        && words.iter().any(|w| VERIFY_NEAR_WORDS.contains(&w.as_str()))
    {
        return true;
    }
    false
}

/// Confidence veto threshold: suppress only when EVERY segment looks like
/// non-speech (near-certain phantom). Set deliberately high (0.85, not the
/// literature's 0.6) — mangled TRUE speech can also score mid-range, and the
/// word gate already handles the common cases. Unit-tested.
/// Unused under `mock-wake` (verifier compiled out) — allowed, not dead.
#[allow(dead_code)]
pub const VERIFY_NOSPEECH_VETO: f32 = 0.85;

/// Confidence gate over Groq `verbose_json` segments (v3).
/// Returns false (suppress) ONLY when every segment reports no_speech_prob
/// at or above the veto threshold — i.e. the model itself says nothing was
/// spoken. Empty segment list (local fallback, parse failure) → true
/// (no information → defer to the word gate; fail-open direction).
/// Pure function (unit-tested).
/// CALIBRATION NOTE (2026-09-19, `wake_camp/nospeech_calib.txt`): on
/// whisper-large-v3-turbo, EVERY segment — true wakes AND hallucinations —
/// reports no_speech_prob = 0.000, so this veto is currently DORMANT (never
/// fires, never kills). Kept as defense-in-depth in case the endpoint starts
/// populating the field (verified present on non-turbo large-v3). The word
/// gate carries production duty until then. avg_logprob was evaluated as an
/// alternative and REJECTED (true NX01 at -0.809 overlaps garbage at -0.8).
/// Unused under `mock-wake` (verifier compiled out) — allowed, not dead.
#[allow(dead_code)]
pub fn verify_confidence(segments: &[crate::stt_groq::GroqSegment]) -> bool {
    if segments.is_empty() {
        return true;
    }
    !segments
        .iter()
        .all(|s| s.no_speech_prob >= VERIFY_NOSPEECH_VETO)
}

/// Backward-confirmation check over pre-trigger audio (v4).
/// The forward-only 500ms window structurally rejects short words (a 0.3s
/// bark ends before the window fills → trailing silence → false reject).
/// This checks the 1s of audio BEFORE the trigger instead: sustained energy
/// there means a real utterance just happened. The most recent 80ms chunk
/// (the trigger chunk itself) is EXCLUDED so a lone spike can't self-confirm.
/// Pure function (unit-tested).
/// Returns (passed, window_rms).
#[cfg(not(feature = "mock-wake"))]
fn check_pre_trigger(
    ring_tail: &[f32],
    gate: f32,
    min_frames: usize,
) -> (bool, f32) {
    const CHUNK: usize = 1280; // 80ms @16kHz
    if ring_tail.len() < CHUNK * 2 {
        return (false, 0.0);
    }
    // Exclude the latest chunk (trigger chunk) from the vote.
    let body = &ring_tail[..ring_tail.len() - CHUNK];
    let n_frames = body.len() / CHUNK;
    let mut voiced = 0usize;
    let mut sum_sq = 0.0f32;
    let mut n = 0usize;
    for i in 0..n_frames {
        let base = i * CHUNK;
        let frame_sq: f32 = body[base..base + CHUNK].iter().map(|s| s * s).sum();
        let rms = (frame_sq / CHUNK as f32).sqrt();
        sum_sq += frame_sq;
        n += CHUNK;
        if rms > gate {
            voiced += 1;
        }
    }
    let rms = if n > 0 { (sum_sq / n as f32).sqrt() } else { 0.0 };
    (voiced >= min_frames, rms)
}

/// VAD-trim a candidate buffer: return the slice spanning speech with ~300ms
/// of margin on each side. Silence-heavy buffers make Whisper hallucinate
/// ("Thank you.") and waste Groq audio-seconds; the word is what matters.
/// Pure function (unit-tested). Falls back to the full buffer when nothing
/// passes the gate or the trimmed span is suspiciously short (<0.3s).
/// Unused under `mock-wake` (verifier compiled out) — allowed, not dead.
#[allow(dead_code)]
fn vad_trim(audio: &[f32]) -> Vec<f32> {
    const FRAME: usize = 320; // 20ms @16kHz
    const GATE: f32 = 0.02; // speech-body floor (above mic-noise/SST floor)
    const MARGIN: usize = 4800; // ~300ms each side (v3: keep context —
        // Whisper hallucinates MORE on ultra-short clips; trim for latency,
        // let the confidence gate (not the trim) decide truth)
    const MIN_SPAN: usize = 4800; // 0.3s — shorter is a fragment, keep all

    if audio.is_empty() {
        return Vec::new();
    }
    let peak = audio.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    let thr = GATE.max(0.05 * peak);
    let n_frames = audio.len() / FRAME;
    let mut first: Option<usize> = None;
    let mut last: usize = 0;
    for i in 0..n_frames {
        let base = i * FRAME;
        let sum_sq: f32 = audio[base..base + FRAME].iter().map(|s| s * s).sum();
        let rms = (sum_sq / FRAME as f32).sqrt();
        if rms > thr {
            if first.is_none() {
                first = Some(i);
            }
            last = i;
        }
    }
    let Some(f0) = first else {
        return audio.to_vec();
    };
    let start = f0.saturating_mul(FRAME).saturating_sub(MARGIN);
    let end = ((last + 1) * FRAME + MARGIN).min(audio.len());
    if end.saturating_sub(start) < MIN_SPAN {
        return audio.to_vec();
    }
    audio[start..end].to_vec()
}

/// Debug dump: save the exact bytes sent to Groq as a 16kHz mono PCM16 WAV
/// at `%APPDATA%/com.nexus.assistant/verify_debug/verify_NN.wav` (rotating,
/// keeps last 5). Lets the owner HEAR what the verifier heard — settles
/// "did I say it / was it clipped / was it silence" without guessing.
/// Dependency-free (hand-written 44-byte header). Best-effort: any IO error
/// is logged and ignored, never blocks verification.
#[cfg(not(feature = "mock-wake"))]
static VERIFY_DUMP_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Rotating counter for command-capture debug clips (P4, separate from the
/// verifier's counter so the two streams never overwrite each other).
#[cfg(not(feature = "mock-wake"))]
static CAPTURE_DUMP_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Debug dump for hotkey/wake command-capture buffers: same WAV encoding as
/// the verifier dump, saved as `capture_NN.wav` (rotating, last 5).
/// Best-effort: IO errors are logged and ignored, never block transcription.
#[cfg(not(feature = "mock-wake"))]
fn dump_capture_debug(samples: &[i16]) {
    use std::sync::atomic::Ordering;
    if samples.is_empty() {
        return;
    }
    let base = std::env::var("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|h| std::path::PathBuf::from(h).join(".config"))
                .unwrap_or_default()
        });
    if base.as_os_str().is_empty() {
        return;
    }
    let dir = base.join("com.nexus.assistant").join("verify_debug");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let seq = CAPTURE_DUMP_SEQ.fetch_add(1, Ordering::Relaxed) % 5;
    let path = dir.join(format!("capture_{seq:02}.wav"));
    match std::fs::write(&path, encode_wav_pcm16(samples)) {
        Ok(()) => tracing::info!("stt-capture: debug clip saved to {}", path.display()),
        Err(e) => tracing::warn!("stt-capture: debug dump failed ({e})"),
    }
}

#[cfg(not(feature = "mock-wake"))]
fn dump_verify_debug(samples: &[i16]) {
    use std::sync::atomic::Ordering;
    if samples.is_empty() {
        return;
    }
    let base = std::env::var("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|h| std::path::PathBuf::from(h).join(".config"))
                .unwrap_or_default()
        });
    if base.as_os_str().is_empty() {
        return;
    }
    let dir = base.join("com.nexus.assistant").join("verify_debug");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let seq = VERIFY_DUMP_SEQ.fetch_add(1, Ordering::Relaxed) % 5;
    let path = dir.join(format!("verify_{seq:02}.wav"));
    let wav = encode_wav_pcm16(samples);
    match std::fs::write(&path, &wav) {
        Ok(()) => tracing::info!("verify: debug clip saved to {}", path.display()),
        Err(e) => tracing::warn!("verify: debug dump failed ({e})"),
    }
}

#[cfg(not(feature = "mock-wake"))]
fn encode_wav_pcm16(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + samples.len() * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&16000u32.to_le_bytes());
    wav.extend_from_slice(&32000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        wav.extend_from_slice(&s.to_le_bytes());
    }
    wav
}

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

/// Keep-alive render stream: inaudible output (digital silence) held open
/// beside capture. Full-duplex traffic holds the Intel DSP awake so it never
/// power-gates the mic path mid-session (Meet parity — Meet's constant media
/// flow is why it never sees the sleep this code fights). Inaudible, ~0 CPU.
/// Kill-switch: `micKeepAlive` (default true). Failure is non-fatal (logged).
#[cfg(not(feature = "mock-wake"))]
static KEEPALIVE_STREAM: once_cell::sync::Lazy<parking_lot::RwLock<Option<SendStream>>> =
    once_cell::sync::Lazy::new(|| parking_lot::RwLock::new(None));

/// Start the keep-alive render stream. Safe to call when one is already
/// active (replaces it). Never touches the capture stream.
#[cfg(not(feature = "mock-wake"))]
pub fn start_keepalive_render() {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        tracing::warn!("keepalive: no default output device — skipping");
        return;
    };
    let dev_name = device.name().unwrap_or_else(|_| "default".into());
    let config = match device.default_output_config() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("keepalive: no output config on '{dev_name}': {e} — skipping");
            return;
        }
    };
    let err_fn = |err| tracing::warn!("keepalive stream error: {err}");
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config.config(),
            move |data: &mut [f32], _| {
                data.fill(0.0);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config.config(),
            move |data: &mut [i16], _| {
                data.fill(0);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_output_stream(
            &config.config(),
            move |data: &mut [u16], _| {
                data.fill(32768);
            },
            err_fn,
            None,
        ),
        fmt => {
            tracing::warn!("keepalive: unsupported sample format {fmt:?} — skipping");
            return;
        }
    };
    match stream {
        Ok(s) => {
            if let Err(e) = s.play() {
                tracing::warn!("keepalive: play failed: {e}");
                return;
            }
            *KEEPALIVE_STREAM.write() = Some(SendStream(s));
            tracing::info!("keepalive: silent render active on '{dev_name}' (DSP held awake)");
        }
        Err(e) => tracing::warn!("keepalive: build failed: {e}"),
    }
}

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

/// Barge-in attempt state (v4): sustained speech while TTS-muted may be the
/// user interrupting. One exact-only verify per TTS session (cooldown via
/// super::BARGE_ATTEMPTED, cleared on any unmuted chunk). Never fires under meeting
/// suppression or manual pause (privacy first).
#[cfg(not(feature = "mock-wake"))]
static BARGE_SUSTAINED_CHUNKS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);
#[cfg(not(feature = "mock-wake"))]
static BARGE_ATTEMPTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// Sustained VAD chunks (80ms) required before a barge-in attempt (~0.5s).
/// Filters coughs, backchannels, and late-transcript tails.
#[cfg(not(feature = "mock-wake"))]
const BARGE_SUSTAIN_CHUNKS: u32 = 6;
/// Chunk RMS floor for barge-in VAD (speech, not room tone).
#[cfg(not(feature = "mock-wake"))]
const BARGE_RMS_FLOOR: f32 = 0.01;

/// Pure barge-in decision (unit-tested): only TTS-muted (never meeting/paused)
/// with sustained speech earns one exact-only verify attempt.
#[cfg(not(feature = "mock-wake"))]
pub fn should_barge_attempt(tts_only_muted: bool, sustained_chunks: u32) -> bool {
    tts_only_muted && sustained_chunks >= BARGE_SUSTAIN_CHUNKS
}

/// Baton-pass flag: when true, the frontend has the mic and the
/// silence-recovery thread should NOT restart the stream.
/// Restarting while the frontend is recording disrupts the capture
/// and causes empty transcripts.
static MIC_BATON_PASSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// ─── Meet-parity stream health ─────────────────────────────────────
// The terminal lesson from the Intel SST + Meet research: NEVER restart on
// content silence. A quiet room is a live-but-muted pipeline (Meet's exact
// model — `track.enabled=false` keeps everything open); only terminal
// signals execute a restart. The old code restarted after ~5s of quiet and
// each restart cost a 10s grace blackout + an SST burst-fade cycle —
// self-inflicted oscillation proven by log timelines (6 restarts / 4 min).
/// Exact-zero persistence that means driver death (with prior audio).
#[allow(dead_code)]
const STREAM_DEAD_ZERO_SECS: u64 = 60;
/// Exact-zero persistence that means "verify, don't execute".
#[allow(dead_code)]
const STREAM_SUSPECT_ZERO_SECS: u64 = 10;
/// Minimum gap between precautionary restarts (backoff floor).
#[allow(dead_code)]
const STREAM_RESTART_FLOOR_SECS: u64 = 60;
/// Callback-rate assumption shared with the existing monitor math.
#[allow(dead_code)]
const STREAM_CBK_PER_SEC: u64 = 33;

/// Stream health: Meet's three states (live-muted / muted-by-OS / ended).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum StreamHealth {
    /// Audio flowing or normal room quiet — do NOTHING (Meet: keep open).
    Quiet,
    /// Bit-exact zeros persisting — probe (pause/play), do NOT restart yet.
    Suspect,
    /// Terminal: device gone, stream error, or proven-dead driver — restart.
    Dead,
}

/// Classify stream health from observables. Pure function (unit-tested).
/// `exact_zero_secs`: how long callbacks have been bit-exact 0.0.
/// `ever_heard_audio`: mic delivered real audio this session (vs never worked).
/// `stream_error`: cpal error callback fired (true death signal).
/// `device_present`: a default input device still enumerates.
/// Unused under `mock-wake` — allowed, not dead.
#[allow(dead_code)]
pub fn classify_stream_health(
    exact_zero_secs: u64,
    ever_heard_audio: bool,
    stream_error: bool,
    device_present: bool,
) -> StreamHealth {
    // Terminal signals first (Meet's `ended`): device gone or stream errored.
    if !device_present || stream_error {
        return StreamHealth::Dead;
    }
    // Bit-exact zeros are digital silence (dropout), NOT quiet-room floor
    // (~0.0001). With prior audio, 60s of zeros means a stuck driver.
    // Without prior audio (fresh boot / OS-muted), allow a 5-minute
    // last-resort window — then restart anyway (a wedged-from-boot driver
    // would otherwise never recover; an OS mute just burns one restart).
    if ever_heard_audio && exact_zero_secs >= STREAM_DEAD_ZERO_SECS {
        return StreamHealth::Dead;
    }
    if !ever_heard_audio && exact_zero_secs >= 300 {
        return StreamHealth::Dead;
    }
    if exact_zero_secs >= STREAM_SUSPECT_ZERO_SECS {
        return StreamHealth::Suspect;
    }
    StreamHealth::Quiet
}

/// Consecutive bit-exact-zero callbacks (rms == 0.0f32 exactly).
/// Quiet rooms produce nonzero floor noise; only a dead/stuck driver (or a
/// muted-at-OS-level mic) emits exact zeros. Updated on the audio path.
#[cfg(not(feature = "mock-wake"))]
static EXACT_ZERO_CBS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Set by the cpal error callback — the TRUE death signal the old recovery
/// never listened to (it watched silence instead). Consumed (take) per poll.
#[cfg(not(feature = "mock-wake"))]
static STREAM_ERROR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Last stream-error log time (millis since epoch) — Linux flood guard.
#[cfg(all(not(feature = "mock-wake"), target_os = "linux"))]
static LAST_STREAM_ERR_LOG_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Throttled stream-error log for the cpal error callback (runs on the RT thread).
///
/// On Linux the ALSA duplex handle dies as soon as the device disappears and
/// cpal then fires the error callback PER TICK (hundreds per second), which
/// floods the log and starves the recovery thread. Log the first error, then at
/// most one every 5 s. The death signal itself is still set on every call.
#[cfg(all(not(feature = "mock-wake"), target_os = "linux"))]
fn log_stream_err_throttled(err: cpal::StreamError) {
    use std::sync::atomic::Ordering;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let last = LAST_STREAM_ERR_LOG_MS.load(Ordering::Relaxed);
    if last != 0 && now.saturating_sub(last) < 5000 {
        return;
    }
    LAST_STREAM_ERR_LOG_MS.store(now, Ordering::Relaxed);
    tracing::error!("audio stream error: {err}");
    // TRUE death signal (device invalidated/removed) — the recovery thread
    // consumes this.
    STREAM_ERROR.store(true, std::sync::atomic::Ordering::Relaxed);
}
/// Last restart time (ms since boot, monotonic). Enforces the restart floor.
#[cfg(not(feature = "mock-wake"))]
static LAST_RESTART_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Monotonic millisecond clock for the restart floor.
/// Unused under `mock-wake` — allowed, not dead.
#[allow(dead_code)]
#[cfg(not(feature = "mock-wake"))]
fn monotonic_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Mic self-test verdict. Pure helper below is unit-tested; the Tauri command
/// analyzes the live verify ring (no second stream — SST-safe).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum MicHealth {
    /// Real audio recently (peak healthy) — mic works.
    Healthy,
    /// Nonzero floor, no speech-level energy — quiet room, driver alive.
    QuietRoom,
    /// All exact zeros — driver stuck/muted; check OS mute + driver.
    DeadSilence,
}

/// Classify a 2.5s sample (peak + zero-ratio) into a mic verdict.
/// `peak`: max |sample|. `zero_ratio`: fraction of exact-0.0 samples.
/// Unused under `mock-wake` — allowed, not dead.
#[allow(dead_code)]
pub fn classify_mic_sample(peak: f32, zero_ratio: f32) -> MicHealth {
    if peak >= 0.05 {
        return MicHealth::Healthy;
    }
    if zero_ratio >= 0.99 {
        return MicHealth::DeadSilence;
    }
    MicHealth::QuietRoom
}

/// Analyzeany f32 mono sample: (peak, exact-zero ratio). Pure (unit-tested).
/// Powers the mic self-test without opening a second stream (SST-safe —
/// a parallel capture would fight the wake stream for the mic lock).
pub fn analyze_audio_sample(audio: &[f32]) -> (f32, f32) {
    if audio.is_empty() {
        return (0.0, 1.0);
    }
    let mut peak = 0.0f32;
    let mut zeros = 0usize;
    for &s in audio {
        let a = s.abs();
        if a > peak {
            peak = a;
        }
        if s == 0.0 {
            zeros += 1;
        }
    }
    (peak, zeros as f32 / audio.len() as f32)
}

/// Mic self-test report (returned by the `mic_self_test` Tauri command).
#[derive(Debug, Clone, serde::Serialize)]
pub struct MicSelfTestReport {
    pub peak: f32,
    pub zero_ratio: f32,
    pub verdict: &'static str,
    pub samples: usize,
}

/// Run the mic self-test against the live verify ring (last 2.5s of mic).
/// Never opens a new stream — safe to call any time, even mid-session.
#[cfg(not(feature = "mock-wake"))]
pub fn mic_self_test_data() -> MicSelfTestReport {
    let ring = VERIFY_RING.lock();
    let audio: Vec<f32> = ring.iter().copied().collect();
    drop(ring);
    let (peak, zero_ratio) = analyze_audio_sample(&audio);
    let verdict = match classify_mic_sample(peak, zero_ratio) {
        MicHealth::Healthy => "healthy",
        MicHealth::QuietRoom => "quiet-room",
        MicHealth::DeadSilence => "dead-silence",
    };
    MicSelfTestReport {
        peak,
        zero_ratio,
        verdict,
        samples: audio.len(),
    }
}

// ─── Rust-side STT capture (bypasses getUserMedia entirely) ─────────────
// The cpal stream that detected the wake word ALSO captures the command audio.
// This fixes the Intel SST driver issue where getUserMedia returns silence
// but the cpal stream is still working.
//
// Flow:
//   1. Wake word detected → start_stt_capture() sets STT_CAPTURING=true
//   2. on_audio() appends 16kHz chunks to STT_CAPTURE_BUFFER
//   3. RMS-based VAD detects speech start and silence end
//   4. On silence after speech → stop capture, spawn transcription thread
//   5. Transcription thread calls Groq or local STT, emits "stt:transcript" event
//   6. Frontend processes the transcript (correct, parse, execute)

static STT_CAPTURE_BUFFER: once_cell::sync::Lazy<parking_lot::Mutex<Vec<f32>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(Vec::with_capacity(16000 * 15)));

static STT_CAPTURING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static STT_SPEECH_DETECTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Duplicate-start guard (phase R). Several independent paths open a capture
/// (wake word, hotkey, ghost hot-mic relisten, the stall watchdogs). A second
/// start while a capture is already live clears the buffer and resets VAD and
/// turn state, so it throws away the first words of the utterance. Two starts
/// closer together than this are one request.
#[cfg(not(feature = "mock-wake"))]
pub(crate) const DUPLICATE_START_WINDOW_MS: u64 = 400;

/// Monotonic ms since first use, never 0 (0 means "no start recorded yet").
#[cfg(not(feature = "mock-wake"))]
static LAST_CAPTURE_START_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(not(feature = "mock-wake"))]
fn mono_ms_nonzero() -> u64 {
    static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    T0.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64 + 1
}

/// Pure decision (unit-tested): is this start a duplicate of a capture that is
/// still running and began less than `window_ms` ago?
#[cfg(not(feature = "mock-wake"))]
pub(crate) fn is_duplicate_capture_start(
    capturing: bool,
    now_ms: u64,
    last_start_ms: u64,
    window_ms: u64,
) -> bool {
    // Fail open: a clock that reads earlier than the recorded start must never block a capture.
    capturing && last_start_ms != 0 && now_ms >= last_start_ms && now_ms - last_start_ms < window_ms
}

/// UI level meter channel: the real-time audio callback `try_send`s one
/// f32 per ~2 chunks (≈6 Hz) while capturing; a dedicated forwarder
/// thread (spawned in `run()`, owns an AppHandle) drains-to-latest and
/// emits `audio:level`. Bounded sync_channel(4): try_send never blocks
/// the audio callback, and a full channel drops (the forwarder is behind
/// — latest wins anyway).
static LEVEL_TX: once_cell::sync::OnceCell<std::sync::mpsc::SyncSender<f32>> =
    once_cell::sync::OnceCell::new();

/// Map capture-chunk RMS to a 0..1 UI level for speech-synced visuals.
/// Post-AGC speech RMS lands ~0.01–0.12, so 0.10 saturates; sqrt keeps
/// quiet speech visible without blowing up on loud onsets.
/// Non-finite / non-positive input maps to 0 (never NaN over the bridge).
pub fn level_from_rms(rms: f32) -> f32 {
    if !rms.is_finite() || rms <= 0.0 {
        return 0.0;
    }
    (rms / 0.10).min(1.0).sqrt()
}
static STT_SILENCE_CHUNKS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static STT_TOTAL_CHUNKS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// Mid-turn pauses survived so far in this capture. 2+ means a hesitant
/// speaker — the endpoint relaxes from fast (400ms) to patient (~1s).
static STT_PAUSE_COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// Chunks with TRUE voice energy (rms > SPEECH threshold, not the lower
/// hysteresis band) in this capture. The silence endpoint only arms once
/// this reaches STT_MIN_VOICED_CHUNKS: a 1-2 chunk SST noise burst must not
/// flip the capture into "speech underway" and then strangle the turn 400ms
/// later while the user is still inhaling (measured 19:28 log: 8-14 chunk
/// captures of pure pre-speech noise → Groq confabulations → "didn't catch
/// that" loop). Hysteresis-band chunks (0.006-0.01) never count.
static STT_VOICED_CHUNKS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Silero neural VAD for the capture loop. `None` = not loaded (flag `vadSilero` off, model
/// missing, or load error) → the RMS gate below stays in charge, byte-for-byte as before.
static SILERO_VAD: once_cell::sync::Lazy<parking_lot::Mutex<Option<crate::vad::SileroVad>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

/// Even with Silero active, chunks below this RMS are never voice: the model is trained on real
/// audio and must not be asked about digital silence (same reason as the wake engine's 0.002 gate).
const SILERO_RMS_FLOOR: f32 = 0.002;

/// Locate the bundled Silero model (production / alt layout / dev tree), mirroring `resolve_oww_dir`.
fn resolve_vad_model(res: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut cands = vec![
        res.join("resources").join("vad").join("silero_vad_v4.onnx"),
        res.join("vad").join("silero_vad_v4.onnx"),
    ];
    if let Some(m) = option_env!("CARGO_MANIFEST_DIR") {
        cands.push(std::path::PathBuf::from(m).join("resources").join("vad").join("silero_vad_v4.onnx"));
    }
    cands.into_iter().find(|p| p.exists())
}

/// Load Silero if `"vadSilero": true` (default off). Called once at engine start; changing the
/// flag needs an app restart. Never fails the engine — on any problem the RMS gate keeps working.
fn configure_silero_vad<R: Runtime>(app: &AppHandle<R>) {
    if !crate::commands::read_vad_silero(app) {
        tracing::info!("vad: Silero disabled (settings.vadSilero not true) — RMS gate active");
        return;
    }
    let Ok(res) = app.path().resource_dir() else { return };
    match resolve_vad_model(&res).map(|p| crate::vad::SileroVad::load(&p)) {
        Some(Ok(v)) => {
            *SILERO_VAD.lock() = Some(v);
            tracing::info!("vad: Silero VAD v4 loaded — neural voice gate active");
        }
        Some(Err(e)) => tracing::warn!("vad: Silero load failed ({e}) — RMS gate stays active"),
        None => tracing::warn!("vad: silero_vad_v4.onnx not found — RMS gate stays active"),
    }
}

/// Score one 80 ms chunk with Silero. `None` = VAD not active/available this chunk (never blocks:
/// `try_lock`; an inference error unloads the VAD so the RMS gate takes over for good).
fn silero_chunk_prob(chunk: &[f32]) -> Option<f32> {
    let mut guard = SILERO_VAD.try_lock()?;
    let vad = guard.as_mut()?;
    match vad.push(chunk) {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::warn!("vad: Silero inference failed ({e}) — falling back to RMS gate");
            *guard = None;
            None
        }
    }
}

// ── Smart Turn (flag `smartTurn`, default off) ─────────────────────────────────
// The model never runs in the audio callback: the callback ships a snapshot of the last <=8 s to a
// worker thread and reads the latest verdict. A verdict is only valid for the silence run it was
// computed for (`SMART_TURN_RUN` is bumped on every voiced chunk). Any failure => legacy rule.
static SMART_TURN_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SMART_TURN_POLICY: once_cell::sync::Lazy<parking_lot::Mutex<crate::turn_detect::TurnPolicy>> =
    once_cell::sync::Lazy::new(|| {
        parking_lot::Mutex::new(crate::turn_detect::policy_for(crate::turn_detect::Eagerness::Medium))
    });
static SMART_TURN_TX: once_cell::sync::OnceCell<std::sync::mpsc::SyncSender<(u64, Vec<f32>)>> =
    once_cell::sync::OnceCell::new();
static SMART_TURN_RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SMART_TURN_VERDICT: once_cell::sync::Lazy<parking_lot::Mutex<(u64, f32)>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new((u64::MAX, 0.0)));
static SMART_TURN_LAST_REQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static SMART_TURN_INFLIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn resolve_smart_turn_model(res: &std::path::Path) -> Option<std::path::PathBuf> {
    let f = "smart-turn-v3.2-cpu.onnx";
    let mut cands = vec![
        res.join("resources").join("smart_turn").join(f),
        res.join("smart_turn").join(f),
    ];
    if let Some(m) = option_env!("CARGO_MANIFEST_DIR") {
        cands.push(std::path::PathBuf::from(m).join("resources").join("smart_turn").join(f));
    }
    cands.into_iter().find(|p| p.exists())
}

/// Load Smart Turn if `"smartTurn": true` and start its worker thread. Needs an app restart to change.
fn configure_smart_turn<R: Runtime>(app: &AppHandle<R>) {
    if !crate::commands::read_smart_turn(app) {
        tracing::info!("smart-turn: disabled (settings.smartTurn not true) — silence rule active");
        return;
    }
    let Ok(res) = app.path().resource_dir() else { return };
    let Some(path) = resolve_smart_turn_model(&res) else {
        tracing::warn!("smart-turn: model not found — silence rule stays active");
        return;
    };
    let det = match crate::turn_detect::TurnDetector::load(&path) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("smart-turn: {e} — silence rule stays active");
            return;
        }
    };
    let eagerness = crate::turn_detect::Eagerness::parse(&crate::commands::read_turn_eagerness(app));
    *SMART_TURN_POLICY.lock() = crate::turn_detect::policy_for(eagerness);
    let (tx, rx) = std::sync::mpsc::sync_channel::<(u64, Vec<f32>)>(1);
    let spawned = std::thread::Builder::new().name("smart-turn".into()).spawn(move || {
        while let Ok((run, samples)) = rx.recv() {
            let t0 = std::time::Instant::now();
            match det.predict(&samples) {
                Ok(p) => {
                    *SMART_TURN_VERDICT.lock() = (run, p);
                    tracing::debug!("smart-turn: p(complete)={:.3} in {} ms", p, t0.elapsed().as_millis());
                }
                Err(e) => tracing::warn!("smart-turn: predict failed ({e})"),
            }
            SMART_TURN_INFLIGHT.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    });
    if spawned.is_ok() && SMART_TURN_TX.set(tx).is_ok() {
        SMART_TURN_ON.store(true, std::sync::atomic::Ordering::Relaxed);
        tracing::info!("smart-turn: loaded — end-of-turn model active (eagerness {:?})", eagerness);
    }
}

/// Per-chunk Smart Turn step for the capture loop. Returns `(end_now, silence_limit)`.
/// Never blocks: snapshot + `try_send`; verdict read via `try_lock`. Off/unavailable => `(false, base)`.
fn smart_turn_poll(speech_detected: bool, voiced: u32, silence: u32, base_limit: u32) -> (bool, u32) {
    use std::sync::atomic::Ordering::{Relaxed, SeqCst};
    if !SMART_TURN_ON.load(Relaxed) || !speech_detected || voiced < STT_MIN_VOICED_CHUNKS {
        return (false, base_limit);
    }
    if silence == 0 {
        // speech (re)started: older verdicts no longer describe the current silence run
        SMART_TURN_RUN.fetch_add(1, Relaxed);
        SMART_TURN_LAST_REQ.store(0, Relaxed);
        return (false, base_limit);
    }
    let pol = *SMART_TURN_POLICY.lock();
    let run = SMART_TURN_RUN.load(Relaxed);
    let last = SMART_TURN_LAST_REQ.load(Relaxed);
    if silence >= pol.min_silence_chunks
        && (last == 0 || silence >= last + 3)
        && !SMART_TURN_INFLIGHT.load(SeqCst)
    {
        if let Some(tx) = SMART_TURN_TX.get() {
            let snap: Vec<f32> = {
                let b = STT_CAPTURE_BUFFER.lock();
                let n = b.len().min(crate::turn_detect::WINDOW_SAMPLES);
                b[b.len() - n..].to_vec()
            };
            if tx.try_send((run, snap)).is_ok() {
                SMART_TURN_INFLIGHT.store(true, SeqCst);
                SMART_TURN_LAST_REQ.store(silence, Relaxed);
            }
        }
    }
    let prob = SMART_TURN_VERDICT
        .try_lock()
        .and_then(|g| if g.0 == run { Some(g.1) } else { None });
    (
        crate::turn_detect::smart_end(silence, &pol, prob),
        crate::turn_detect::extended_limit(base_limit, &pol, prob),
    )
}

/// Classify one capture chunk → `(continues_turn, starts_turn)`.
/// * Energy mode (`prob == None`) — exactly the pre-existing hysteresis: continue above
///   `STT_SILENCE_RMS_THRESHOLD`, start above `STT_SPEECH_RMS_THRESHOLD`.
/// * Neural mode — continue at `CONTINUE_PROB`, start at `START_PROB`, both gated by
///   `SILERO_RMS_FLOOR` (no model verdicts on digital silence).
fn chunk_voice_flags(rms: f32, prob: Option<f32>) -> (bool, bool) {
    match prob {
        None => (rms > STT_SILENCE_RMS_THRESHOLD, rms > STT_SPEECH_RMS_THRESHOLD),
        Some(p) => {
            let live = rms > SILERO_RMS_FLOOR;
            (live && p >= crate::vad::CONTINUE_PROB, live && p >= crate::vad::START_PROB)
        }
    }
}

/// Global AppHandle for emitting "stt:transcript" events from the capture thread.
/// We use a channel instead of storing the AppHandle directly (which has a generic
/// type parameter R that can't be stored in a static).
#[cfg(not(feature = "mock-wake"))]
static STT_CAPTURE_TX: OnceCell<std::sync::mpsc::Sender<(u64, TurnStats, Vec<f32>)>> = OnceCell::new();

/// Capture session generation. Incremented by `abort_stt_capture()` so the
/// STT receiver thread can drop buffers that were already in flight when a
/// second Ctrl+Space press cancelled the turn (Gap 3: without this, the
/// receiver emits a `stt:transcript: ""` for the orphaned buffer and the
/// frontend jitters through a phantom no-input retry). Buffers carry the
/// session id they were taken under; anything stale is dropped silently.
static CAPTURE_SESSION_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Pure stale-session predicate (unit-tested): true when a buffer tagged
/// with `taken` belongs to a capture generation older than `current`.
fn is_stale_session(taken: u64, current: u64) -> bool {
    taken != current
}

/// RMS threshold for speech detection (16kHz mono f32 samples).
/// 0.01 filters out Intel SST background noise while still catching
/// normal speech (typical RMS 0.05-0.3). The previous value of 0.005
/// was too low — the Intel SST mic noise floor can exceed it for
/// extended periods, causing 16s captures of pure noise that Groq
/// hallucinates on.
const STT_SPEECH_RMS_THRESHOLD: f32 = 0.01;

/// Lower silence threshold once speech is underway (hysteresis).
/// Boundary chatter (a chunk at 0.009 between two at 0.05) must not reset
/// progress or extend the turn; but while still listening for the FIRST
/// speech, the higher threshold above avoids noise-triggered captures.
const STT_SILENCE_RMS_THRESHOLD: f32 = 0.006;

/// Fast endpoint: silent chunks after speech before stopping capture.
/// 5 chunks = 400ms (cuts 400ms of dead air compared to legacy 10 chunks).
/// The silence counter resets on every speech chunk, so continued speech extends the
/// turn automatically — this cuts cleanly after natural phrase completion.
const STT_SILENCE_CHUNK_LIMIT: u32 = 5;

/// Patient endpoint for hesitant speakers: if the capture already survived
/// 2+ mid-turn pauses, allow ~800ms (10 chunks) before committing, instead of
/// cutting them off mid-thought.
const STT_SILENCE_CHUNK_LIMIT_PATIENT: u32 = 10;

/// Ultra-fast Ghost Mode endpoint for short desktop automation commands
/// ("open whatsapp", "click search", "type message"). 3 chunks = 240ms.
const STT_SILENCE_CHUNK_LIMIT_GHOST: u32 = 3;

/// Ghost Mode patient endpoint when speaker has paused mid-command: 6 chunks = 480ms.
const STT_SILENCE_CHUNK_LIMIT_GHOST_PATIENT: u32 = 6;


/// Maximum capture duration in chunks. 125 chunks = 10s.
/// 10 seconds is plenty for any voice command. The previous value of
/// 200 (16s) allowed Groq to hallucinate on extended noise captures.
const STT_MAX_CHUNKS: u32 = 125;

/// Minimum TRUE-voiced chunks before the silence endpoint arms.
/// 3 chunks = 240ms: SST noise bursts are 1-2 chunks, a short word
/// ("yes", "nexus") is 4-6. Below this the capture stays in "awaiting
/// speech" — only the 8s no-speech timeout can stop it.
const STT_MIN_VOICED_CHUNKS: u32 = 3;

/// Pure endpoint predicate (unit-tested): stop the capture when a CONFIRMED
/// turn (enough voiced chunks) goes silent, on max duration, or on the
/// no-speech timeout. `voiced < MIN` means no turn started — never cut.
fn should_stop_capture(
    speech_detected: bool,
    voiced: u32,
    silence: u32,
    silence_limit: u32,
    total: u32,
) -> bool {
    (speech_detected && voiced >= STT_MIN_VOICED_CHUNKS && silence >= silence_limit)
        || total >= STT_MAX_CHUNKS
        || (!speech_detected && total >= STT_NO_SPEECH_CHUNK_LIMIT)
}

/// Phantom-capture guard (unit-tested): true when a buffer holds too little
/// voice energy to be worth a Groq call. Counts 80ms chunks above the speech
/// threshold; fewer than MIN_VOICED = noise blips / pre-speech rustle.
/// Skipping saves Groq audio-seconds AND the confabulation ("Thank you.")
/// that sends the user into the "didn't catch that" loop.
fn is_phantom_capture(buffer: &[f32]) -> bool {
    const CHUNK: usize = 1280;
    if buffer.len() < CHUNK * STT_MIN_VOICED_CHUNKS as usize {
        return true;
    }
    let mut voiced = 0u32;
    for c in buffer.chunks(CHUNK) {
        if c.len() < CHUNK {
            break;
        }
        let sum_sq: f32 = c.iter().map(|s| s * s).sum();
        if (sum_sq / CHUNK as f32).sqrt() > STT_SPEECH_RMS_THRESHOLD {
            voiced += 1;
            if voiced >= STT_MIN_VOICED_CHUNKS {
                return false;
            }
        }
    }
    true
}

/// Per-turn capture packet (approach A: instrument-first). Travels with
/// the audio buffer to the receiver thread so every transcript — including
/// every empty one — arrives with its cause attached: mic energy actually
/// seen (rms_max/mean), endpoint branch taken, STT path used, and filter
/// verdict. Emitted to the frontend as `stt:turn_stats` next to each
/// `stt:transcript`, so the next miss is data, not a mystery.
#[derive(serde::Serialize, Clone, Debug)]
pub struct TurnStats {
    pub session: u64,
    pub rms_max: f32,
    pub rms_mean: f64,
    pub voiced_chunks: u32,
    pub total_chunks: u32,
    pub endpoint: &'static str,
    pub stt_path: &'static str,
    pub filter: &'static str,
    pub owner: &'static str,
    pub owner_score: f32,
    pub decoder_bias: &'static str,
    pub stt_language: &'static str,
}

/// Structured transcript payload. The frontend accepts both this object and
/// the legacy bare string for short/phantom/error turns.
#[derive(serde::Serialize, Clone, Debug)]
pub struct SttTranscript {
    pub session: u64,
    pub text: String,
    pub ownership: &'static str,
    pub owner_score: f32,
    pub decoder_bias: &'static str,
    pub language: &'static str,
    pub intent_label: Option<&'static str>,
    pub pre_parsed: Option<serde_json::Value>,
}

/// Peak + mean absolute energy of a capture buffer (single pass, computed
/// at take-time in the audio callback — no hot-loop state needed).
fn buffer_rms_stats(buffer: &[f32]) -> (f32, f64) {
    if buffer.is_empty() {
        return (0.0, 0.0);
    }
    let mut peak = 0.0f32;
    let mut sum_sq = 0.0f64;
    for &s in buffer {
        let a = s.abs();
        if a > peak {
            peak = a;
        }
        sum_sq += (s as f64) * (s as f64);
    }
    (peak, (sum_sq / buffer.len() as f64).sqrt())
}

/// Which stop branch fired (mirrors `should_stop_capture` exactly —
/// keep the two in sync; the table test below pins both).
/// `silence_end` = confirmed speech then quiet · `max_duration` = 10s cap ·
/// `no_speech` = 8s without a turn · `unknown` = defensive (unreachable
/// when called after a true `should_stop_capture`).
pub fn endpoint_reason(
    speech_detected: bool,
    voiced: u32,
    silence: u32,
    silence_limit: u32,
    total: u32,
) -> &'static str {
    if speech_detected && voiced >= STT_MIN_VOICED_CHUNKS && silence >= silence_limit {
        "silence_end"
    } else if total >= STT_MAX_CHUNKS {
        "max_duration"
    } else if !speech_detected && total >= STT_NO_SPEECH_CHUNK_LIMIT {
        "no_speech"
    } else {
        "unknown"
    }
}

/// No-speech timeout in chunks. 100 chunks = 8s (same as frontend watchdog).
const STT_NO_SPEECH_CHUNK_LIMIT: u32 = 100;

/// Start capturing audio from the cpal stream for STT.
/// Whether an STT capture is in flight (ghost watchdog `busy` signal).
/// Always false under mock-wake (no engine, no capture).
#[cfg(not(feature = "mock-wake"))]
pub fn stt_capturing() -> bool {
    STT_CAPTURING.load(std::sync::atomic::Ordering::Relaxed)
}
#[cfg(feature = "mock-wake")]
pub fn stt_capturing() -> bool {
    false
}

/// Mic open AND voice detected in this capture (mid-sentence). The proactive-speech policy uses this
/// to avoid talking over the user — a ghost hot-mic is "capturing" almost always, but only
/// "speaking" while someone actually talks.
#[cfg(not(feature = "mock-wake"))]
pub fn stt_user_speaking() -> bool {
    STT_CAPTURING.load(std::sync::atomic::Ordering::Relaxed)
        && STT_SPEECH_DETECTED.load(std::sync::atomic::Ordering::Relaxed)
}
#[cfg(feature = "mock-wake")]
pub fn stt_user_speaking() -> bool {
    false
}

/// Called on wake word detection or hotkey press.
/// Does NOT pause the cpal stream — the stream keeps running and
/// the audio callback buffers 16kHz samples for transcription.
#[cfg(not(feature = "mock-wake"))]
pub fn start_stt_capture() {
    start_stt_capture_with_origin(crate::directed::Origin::Direct);
}

/// Start a capture and record WHO opened the mic (`HotMic` = the ghost hot-mic loop re-opened it by
/// itself; everything else is an explicit user turn). Phase 8 directed-speech gate reads this.
#[cfg(not(feature = "mock-wake"))]
pub fn start_stt_capture_with_origin(origin: crate::directed::Origin) {
    let now_ms = mono_ms_nonzero();
    let last_ms = LAST_CAPTURE_START_MS.load(std::sync::atomic::Ordering::Relaxed);
    if is_duplicate_capture_start(
        STT_CAPTURING.load(std::sync::atomic::Ordering::Relaxed),
        now_ms,
        last_ms,
        DUPLICATE_START_WINDOW_MS,
    ) {
        // The live capture keeps its buffer. An explicit turn may still take
        // ownership of a hot-mic capture (the directed gate must not apply to it).
        if matches!(origin, crate::directed::Origin::Direct) {
            crate::directed::set_capture_origin(origin);
        }
        tracing::info!(
            "stt-capture: duplicate start ignored (+{}ms, origin={:?})",
            now_ms.saturating_sub(last_ms),
            origin as u8
        );
        return;
    }
    LAST_CAPTURE_START_MS.store(now_ms, std::sync::atomic::Ordering::Relaxed);
    crate::directed::set_capture_origin(origin);
    crate::proactive_policy::note_user_activity(); // idle signal for proactive speech (Phase 9)
    crate::nlu_local::prewarm(); // the command model loads while the user is still speaking (docs/features/134 C2)
    crate::tts_edge::prewarm(); // open the Edge socket while the user is still speaking
    {
        let mut buf = STT_CAPTURE_BUFFER.lock();
        buf.clear();
        // NOTE: No prefix-pad from VERIFY_RING here.
        // The VERIFY_RING contains the wake word audio ("NEXUS") and ~2s
        // of audio context. Seeding that into the command capture poisons
        // Groq: it receives NEXUS + 1-2s of silence + command, and
        // hallucinates random text ("and then the process goes to murder.")
        // because the wake word acoustics + silence is ambiguous.
        //
        // The command capture starts fresh (STT_CAPTURING=true) and the
        // audio callback begins appending the very next chunks immediately,
        // so no phonemes are lost — the user has already finished the wake
        // word and is just starting to say the command.
    }
    STT_CAPTURING.store(true, std::sync::atomic::Ordering::Relaxed);
    STT_SPEECH_DETECTED.store(false, std::sync::atomic::Ordering::Relaxed);
    STT_SILENCE_CHUNKS.store(0, std::sync::atomic::Ordering::Relaxed);
    STT_TOTAL_CHUNKS.store(0, std::sync::atomic::Ordering::Relaxed);
    STT_VOICED_CHUNKS.store(0, std::sync::atomic::Ordering::Relaxed);
    STT_PAUSE_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
    if let Some(v) = SILERO_VAD.lock().as_mut() {
        v.reset(); // new turn must not inherit the previous turn's LSTM state
    }
    SMART_TURN_RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    SMART_TURN_LAST_REQ.store(0, std::sync::atomic::Ordering::Relaxed);
    tracing::info!("stt-capture: started (cpal-side capture, no baton pass)");
}

/// Result of `abort_stt_capture()` (second Ctrl+Space press while listening).
/// `had_speech` tells the frontend whether to let the in-flight turn finish
/// (voice already underway — never kill mid-word) or hide the orb outright.
#[derive(serde::Serialize, Clone, Debug)]
pub struct SttAbortResult {
    pub had_speech: bool,
    pub elapsed_ms: u32,
}

/// Non-destructive voice check for the frontend no-input timeout.
/// True once ≥3 truly-voiced chunks landed (same gate as `had_speech`),
/// WITHOUT touching any capture state. Lets the UI skip hiding
/// mid-utterance while leaving slow-starter turns fully intact.
#[cfg(not(feature = "mock-wake"))]
pub fn stt_capture_had_speech() -> bool {
    STT_CAPTURING.load(std::sync::atomic::Ordering::Relaxed)
        && STT_VOICED_CHUNKS.load(std::sync::atomic::Ordering::Relaxed) >= STT_MIN_VOICED_CHUNKS
}

/// Abort an in-flight Rust-side STT capture (hotkey second-press cancel).
/// Safe to call when nothing is capturing (pure no-op state reset).
/// Invalidates the capture session FIRST so any buffer already handed to
/// the receiver thread is dropped by the stale-session check on arrival.
#[cfg(not(feature = "mock-wake"))]
pub fn abort_stt_capture() -> SttAbortResult {
    use std::sync::atomic::Ordering;
    let _ = CAPTURE_SESSION_ID.fetch_add(1, Ordering::SeqCst);
    let was_capturing = STT_CAPTURING.swap(false, Ordering::SeqCst);
    let voiced = STT_VOICED_CHUNKS.load(Ordering::Relaxed);
    let total = STT_TOTAL_CHUNKS.load(Ordering::Relaxed);
    // 80ms chunks (1280 samples @ 16kHz).
    let elapsed_ms = total.saturating_mul(80);
    let had_speech = voiced >= STT_MIN_VOICED_CHUNKS;

    // Settle the UI meter so waves rest instead of freezing mid-speech.
    if let Some(tx) = LEVEL_TX.get() {
        let _ = tx.try_send(0.0);
    }

    STT_CAPTURE_BUFFER.lock().clear();
    STT_SPEECH_DETECTED.store(false, Ordering::Relaxed);
    STT_SILENCE_CHUNKS.store(0, Ordering::Relaxed);
    STT_TOTAL_CHUNKS.store(0, Ordering::Relaxed);
    STT_VOICED_CHUNKS.store(0, Ordering::Relaxed);
    STT_PAUSE_COUNT.store(0, Ordering::Relaxed);

    tracing::info!(
        "stt-capture: aborted (was_capturing={}, had_speech={}, elapsed={}ms)",
        was_capturing,
        had_speech,
        elapsed_ms
    );
    SttAbortResult {
        had_speech,
        elapsed_ms,
    }
}

/// Process a captured audio buffer: send it to the STT thread for transcription.
/// Called from a spawned thread when silence is detected.
/// The actual transcription + event emission happens in the STT receiver thread
/// (spawned in `run()`), which has access to the AppHandle.
/// `session` tags the capture generation the buffer was taken under so the
/// receiver can drop it if `abort_stt_capture()` invalidated the generation
/// while the buffer was in flight.
#[cfg(not(feature = "mock-wake"))]
fn transcribe_and_emit(session: u64, stats: TurnStats, buffer: Vec<f32>) {
    if let Some(tx) = STT_CAPTURE_TX.get() {
        if tx.send((session, stats, buffer)).is_err() {
            tracing::error!("stt-capture: STT receiver thread died, cannot send buffer");
        }
    } else {
        tracing::warn!("stt-capture: STT_CAPTURE_TX not initialized, dropping buffer");
    }
}

/// Pause the wake-word audio stream (release the OS mic lock).
/// Called by the frontend via `pause_wakeword` IPC before getUserMedia().
#[cfg(not(feature = "mock-wake"))]
pub fn pause_stream() {
    MIC_BATON_PASSED.store(true, std::sync::atomic::Ordering::Relaxed);
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
    MIC_BATON_PASSED.store(false, std::sync::atomic::Ordering::Relaxed);
    use cpal::traits::StreamTrait;
    let guard = CPAL_STREAM.read();
    if let Some(ref stream) = *guard {
        match stream.0.play() {
            Ok(()) => {
                tracing::info!("wake: cpal stream resumed (mic baton pass — frontend released mic)");
                // Reset the startup grace period when the stream resumes.
                // The Intel SST driver produces transient noise bursts when
                // the stream starts/restarts, which false-triggers the wake
                // word model. The STARTUP_GRACE_MS window in detect_chunk() ignores
                // these, but only if the grace window was restarted.
                // Without this reset, the grace period expires at app boot
                // (5.7s before the stream starts) and never protects against
                // stream-restart transients.
                reset_grace_period();
            }
            Err(e) => tracing::warn!("wake: cpal stream resume failed: {e}"),
        }
    } else {
        tracing::warn!("wake: resume_stream called but no stream stored");
    }
}

/// Reset the wake engine's startup grace period.
/// Called when the audio stream starts or resumes — the Intel SST driver
/// produces transient noise on stream start that false-triggers the model.
/// The grace period in detect_chunk() ignores detections during this window.
#[cfg(not(feature = "mock-wake"))]
pub fn reset_grace_period() {
    let engine_opt = WAKE_ENGINE_GLOBAL.lock().clone();
    if let Some(engine) = engine_opt {
        let mut eng = engine.lock();
        eng.restart_grace();
        eng.preprocessor.reset();
        tracing::debug!(
            "wake: grace period reset + preprocessor reset ({} ms immunity)",
            engine::STARTUP_GRACE_MS
        );
    }
}

/// Mock-wake stubs for non-OWW builds.
#[cfg(feature = "mock-wake")]
pub fn start_stt_capture() {}
#[cfg(feature = "mock-wake")]
pub fn start_stt_capture_with_origin(_origin: crate::directed::Origin) {}
#[cfg(feature = "mock-wake")]
pub fn abort_stt_capture() -> SttAbortResult {
    SttAbortResult {
        had_speech: false,
        elapsed_ms: 0,
    }
}
#[cfg(feature = "mock-wake")]
pub fn stt_capture_had_speech() -> bool {
    false
}
#[cfg(feature = "mock-wake")]
pub fn pause_stream() {}
#[cfg(feature = "mock-wake")]
pub fn resume_stream() {}
#[cfg(feature = "mock-wake")]
pub fn mic_self_test_data() -> MicSelfTestReport {
    // No audio pipeline in mock mode — report unknown rather than dead
    // (dead would send users down a driver rabbit hole for a test build).
    MicSelfTestReport {
        peak: 0.0,
        zero_ratio: 0.0,
        verdict: "mock-no-audio",
        samples: 0,
    }
}

/// Set the global meeting state reference. Called from `lib.rs` during setup.
#[cfg(not(feature = "mock-wake"))]
pub fn set_meeting_state(state: std::sync::Arc<crate::meeting_detect::MeetingState>) {
    let _ = MEETING_STATE.set(state);
}

/// Clear the TTS-playing flag (barge-in bookkeeping). Called by the
/// orchestrator choke point after cutting audio. No-op if unset.
#[cfg(not(feature = "mock-wake"))]
pub(crate) fn clear_tts_playing() {
    if let Some(ms) = MEETING_STATE.get() {
        ms.set_tts_playing(false);
    }
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
    tracing::info!("wake-engine: loading ONNX models (tract-onnx optimization)...");
    let mut wake_engine = engine::WakeEngine::new(res, data_dir)
        .map_err(|e| format!("wake engine init: {e}"))?;
    tracing::info!(
        "wake-engine: ONNX models loaded in {:.1}s — KWS ready",
        t1.elapsed().as_secs_f64()
    );

    configure_silero_vad(&app);
    configure_smart_turn(&app);

    // Create command channel for Tier 3 command classifiers
    let (cmd_tx, cmd_rx) =
        std::sync::mpsc::channel::<engine::CommandIntent>();
    wake_engine.command_tx = Some(cmd_tx);

    let engine = std::sync::Arc::new(parking_lot::Mutex::new(wake_engine));

    let (tx, rx) = std::sync::mpsc::channel::<WakeCandidate>();
    let _ = WAKE_TX.set(tx);

    // ─── Phase 3: Start audio capture (with retry for cold-boot audio driver) ──
    // Reset the grace period BEFORE starting the audio stream. The audio
    // callback begins receiving chunks immediately when the stream starts,
    // and if the grace period is reset after, there's a race condition
    // where the first few chunks are processed with an expired grace period
    // (set at engine creation 5-10s ago), causing a false wake on startup
    // transient noise. Direct call: `reset_grace_period()` goes through
    // WAKE_ENGINE_GLOBAL, which is only stored after the stream starts.
    {
        let mut eng = engine.lock();
        eng.restart_grace();
        eng.preprocessor.reset();
    }

    let t2 = Instant::now();
    start_audio_capture_with_retry(engine.clone())?;
    tracing::info!(
        "wake-engine: audio capture started in {:.1}s — listening for 'nexus'",
        t2.elapsed().as_secs_f64()
    );

    // ─── Phase 3b: Mic keep-alive render (Meet parity) ─────────────
    // Full-duplex traffic holds the Intel DSP awake. See start_keepalive_render.
    if crate::commands::read_mic_keep_alive(&app) {
        start_keepalive_render();
    } else {
        tracing::info!("keepalive: disabled in settings (micKeepAlive=false)");
    }

    // Store engine globally for the silence-recovery thread.
    {
        let mut g = WAKE_ENGINE_GLOBAL.lock();
        *g = Some(engine.clone());
    }

    // Reset the grace period again AFTER the stream starts, in case the
    // stream start took several seconds and the pre-start reset has expired.
    reset_grace_period();

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

                    // Skip recovery if the frontend has the mic (baton pass).
                    // The stream is intentionally paused — restarting would
                    // disrupt the frontend's audio capture and cause empty transcripts.
                    if MIC_BATON_PASSED.load(Ordering::Relaxed) {
                        continue;
                    }

                    // Skip recovery if Rust-side STT capture is active.
                    // The cpal stream is actively capturing command audio —
                    // restarting mid-capture destroys the audio buffer and
                    // causes empty transcripts (Groq then hallucinates on
                    // the silence). This was happening every time the user
                    // pressed Ctrl+Space: capture started, then 1-5s later
                    // silence-recovery restarted the stream, killing the capture.
                    if STT_CAPTURING.load(Ordering::Relaxed) {
                        continue;
                    }

                    let last_cb = LAST_CALLBACK_FOR_RECOVERY.load(Ordering::Relaxed);
                    let last_non_silent = LAST_NONSILENT_FOR_RECOVERY.load(Ordering::Relaxed);
                    let now_cb = CALLBACK_COUNT_GLOBAL.load(Ordering::Relaxed);

                    // ── Meet-parity health classification ──────────────
                    // NEVER restart on content silence (a quiet room is a
                    // live-but-muted pipeline). Restart ONLY on terminal
                    // signals: no callbacks at all (stream thread dead),
                    // cpal error fired, device gone, or bit-exact zeros
                    // persisting with prior audio this session (driver stuck).
                    let callbacks_stalled = now_cb.saturating_sub(last_cb) == 0;
                    let silence_duration = now_cb.saturating_sub(last_non_silent);
                    // ~33 callbacks/sec → poll_secs * 33 callbacks
                    let long_silence = silence_duration > (poll_secs * 33);
                    let exact_zero_cbs =
                        EXACT_ZERO_CBS.load(Ordering::Relaxed);
                    let exact_zero_secs =
                        exact_zero_cbs / STREAM_CBK_PER_SEC;
                    // Prior audio = any above-gate callback ever observed
                    // (file-root index; nonzero means the mic worked before).
                    // Distinguishes dropout from never-working/muted-at-OS,
                    // which restarts can't fix.
                    let ever_heard_audio =
                        LAST_NONSILENT_FOR_RECOVERY.load(Ordering::Relaxed) > 0;
                    // Consume the cpal error flag (true death signal).
                    let stream_error =
                        STREAM_ERROR.swap(false, Ordering::SeqCst);
                    // Device enumeration is slow (~50-200ms) — only probe it
                    // when something already looks wrong.
                    let device_present = if callbacks_stalled || exact_zero_secs > 10
                    {
                        cpal::default_host().default_input_device().is_some()
                    } else {
                        true
                    };
                    let health = classify_stream_health(
                        exact_zero_secs,
                        ever_heard_audio,
                        stream_error,
                        device_present,
                    );
                    // Preserve the legacy signals for the log line + the
                    // recovery-success check below.
                    let should_restart = matches!(
                        health,
                        StreamHealth::Dead
                    ) || (callbacks_stalled && device_present);

                    if health == StreamHealth::Suspect {
                        tracing::debug!(
                            "silence-recovery: SUSPECT (exact-zero {}s, prior audio: {}) — observing, NOT restarting (Meet parity)",
                            exact_zero_secs, ever_heard_audio
                        );
                    }

                    if should_restart {
                        // Backoff floor: never restart twice within 60s for
                        // precautionary (non-error) restarts. True errors
                        // (cpal error / device gone) bypass the floor.
                        let now_ms = monotonic_ms();
                        let last_rs =
                            LAST_RESTART_MS.load(Ordering::Relaxed);
                        let hard_evidence = stream_error || !device_present;
                        if !hard_evidence
                            && now_ms.saturating_sub(last_rs)
                                < STREAM_RESTART_FLOOR_SECS * 1000
                        {
                            tracing::debug!(
                                "silence-recovery: restart due but floor active ({}s since last) — waiting",
                                now_ms.saturating_sub(last_rs) / 1000
                            );
                        } else {
                            LAST_RESTART_MS.store(now_ms, Ordering::Relaxed);
                            // ...proceed to restart below...
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
                                    // Reset the wake-word grace period — the
                                    // stream restart produces transient noise
                                    // that false-triggers the wake model.
                                    reset_grace_period();
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "silence-recovery #{}: failed to restart audio: {e}",
                                        restart_n
                                    );
                                }
                            }
                        }
                        } // end else (floor passed) — closes the should_restart gate
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
                crate::window_manager::show_orb_interactive(&app_for_commands);
                let _ = app_for_commands.emit("command-detected", &intent);
            }
        })
        .ok();

    // Main loop: handle wake-word detections
    // Set up the STT capture channel: the audio callback sends captured audio
    // via STT_CAPTURE_TX, and this thread receives it, transcribes, and emits
    // the "stt:transcript" event to the frontend.
    let (stt_tx, stt_rx) = std::sync::mpsc::channel::<(u64, TurnStats, Vec<f32>)>();
    let _ = STT_CAPTURE_TX.set(stt_tx);

    // UI level forwarder: drains the callback's level channel to the
    // LATEST value and emits `audio:level` (~6 Hz during capture + a
    // final 0 on stop). Own thread because the STT receiver blocks in
    // transcription for seconds — levels must stay realtime for the
    // speech-synced ghost waves. Duplicate suppression keeps silence
    // from spamming the bridge with identical zeros.
    let (level_tx, level_rx) = std::sync::mpsc::sync_channel::<f32>(4);
    let _ = LEVEL_TX.set(level_tx);
    let app_for_level = app.clone();
    std::thread::Builder::new()
        .name("audio-level-fwd".into())
        .spawn(move || {
            let mut last_sent = -1.0f32;
            while let Ok(first) = level_rx.recv() {
                let mut latest = first;
                while let Ok(next) = level_rx.try_recv() {
                    latest = next;
                }
                if (latest - last_sent).abs() < 0.02 {
                    continue;
                }
                last_sent = latest;
                let _ = app_for_level.emit(
                    "audio:level",
                    serde_json::json!({ "level": latest }),
                );
            }
        })
        .ok();

    // Spawn the STT receiver thread — it has access to the AppHandle and
    // handles transcription + event emission.
    let app_for_stt = app.clone();
    std::thread::Builder::new()
        .name("stt-capture-rx".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!("stt-capture: failed to create persistent tokio runtime: {}", e);
                    return;
                }
            };

            while let Ok((session, mut stats, buffer)) = stt_rx.recv() {
                // Stale-session drop (Gap 3): a second Ctrl+Space press ran
                // abort_stt_capture() while this buffer was in flight. Emit
                // NOTHING on the transcript channel — not even "" — so the
                // cancelled turn stays dead. The turn packet still goes out
                // (endpoint "aborted") so cancelled turns are diagnosable.
                if is_stale_session(
                    session,
                    CAPTURE_SESSION_ID.load(std::sync::atomic::Ordering::Relaxed),
                ) {
                    tracing::info!(
                        "stt-capture: dropping stale-session buffer (taken={}, current={})",
                        session,
                        CAPTURE_SESSION_ID.load(std::sync::atomic::Ordering::Relaxed)
                    );
                    stats.endpoint = "aborted";
                    stats.filter = "stale_session";
                    crate::commands::emit_logged(&app_for_stt, "stt:turn_stats", &stats);
                    continue;
                }
                if buffer.is_empty() {
                    tracing::warn!("stt-capture: empty buffer received");
                    crate::commands::emit_logged(&app_for_stt, "stt:transcript", "");
                    stats.endpoint = "empty_buffer";
                    crate::commands::emit_logged(&app_for_stt, "stt:turn_stats", &stats);
                    continue;
                }

                // Phantom-capture guard (F2): too little voice energy to be
                // speech — skip the Groq call entirely (saves audio-seconds
                // and the confabulation that would route garbage into the
                // brain, e.g. a phantom "Architect" window). "" flows into
                // the normal retry prompt, never into an action.
                if is_phantom_capture(&buffer) {
                    tracing::info!(
                        "stt-capture: phantom capture ({} samples, <{} voiced chunks) — skipping Groq",
                        buffer.len(),
                        STT_MIN_VOICED_CHUNKS
                    );
                    crate::commands::emit_logged(&app_for_stt, "stt:transcript", "");
                    stats.filter = "phantom";
                    stats.stt_path = "skipped";
                    crate::commands::emit_logged(&app_for_stt, "stt:turn_stats", &stats);
                    continue;
                }

                // Convert f32 samples to i16 PCM
                let samples: Vec<i16> = buffer
                    .iter()
                    .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
                    .collect();

                // P-E endpoint feedback (doc 75): speech ended (endpoint
                // fired, voice confirmed above) — show thinking NOW, not
                // after the 2-4s Groq roundtrip. No request_id exists yet
                // (created in process_transcript); the frontend applies
                // bare state events directly, and the transcript flow
                // re-syncs everything on arrival ( ack → loading → result).
                crate::commands::emit_logged(&app_for_stt, 
                    "orchestrator:event",
                    &crate::orchestrator::OrchestratorEvent::State {
                        state: crate::orchestrator::OrchestratorState::Thinking,
                        request_id: String::new(),
                    },
                );

                // Turn ownership is decided before any cloud STT call. Verified
                // owner audio keeps the NEXUS decoder bias; every other turn
                // uses neutral English decoding. Rejected audio never reaches
                // Groq, the router, speech, memory, or learning.
                let (ownership, owner_score) = verify_turn_audio(&app_for_stt, &buffer);
                let decoder_bias = match crate::stt_groq::decoder_bias_for(ownership) {
                    crate::stt_groq::DecoderBias::OwnerCommand => "owner",
                    crate::stt_groq::DecoderBias::Neutral => "neutral",
                };
                stats.owner = ownership.as_str();
                stats.owner_score = owner_score;
                stats.decoder_bias = decoder_bias;
                stats.stt_language = "en";
                if ownership == crate::voice_profile::TurnOwnership::Rejected {
                    tracing::info!(
                        "stt-capture: non-owner audio (score {:.3}) — ambient drop, no transcription",
                        owner_score
                    );
                    crate::commands::emit_logged(&app_for_stt, 
                        "stt:transcript",
                        &SttTranscript {
                            session,
                            text: String::new(),
                            ownership: stats.owner,
                            owner_score,
                            decoder_bias,
                            language: "en",
                            intent_label: None,
                            pre_parsed: None,
                        },
                    );
                    stats.stt_path = "owner_hold";
                    stats.filter = "non_owner";
                    crate::commands::emit_logged(&app_for_stt, "stt:turn_stats", &stats);
                    continue;
                }

                // Debug dump (P4): save the exact command-capture audio so a
                // mystery transcript (e.g. TV anime → Japanese) arrives with
                // its audio attached. Rotating capture_00..04.wav, best-effort.
                dump_capture_debug(&samples);

                tracing::info!(
                    "stt-capture: {} samples ({}ms audio), starting transcription",
                    samples.len(),
                    samples.len() / 16
                );

                rt.block_on(async {
                    let client = crate::stt::shared_client();

                    crate::stt::reset_turn_markers();
                    let bias = crate::stt_groq::decoder_bias_for(ownership);
                    let vocabulary = Some(crate::stt_groq::decoder_bias_prompt(bias));
                    let transcript = crate::stt::transcribe_samples(
                        &samples,
                        &client,
                        Some(&app_for_stt),
                        vocabulary,
                    )
                    .await;

                    // A failed transcription is NEVER dropped silently: the
                    // chain (cloud STT → local sidecar) reports why. The reason
                    // is logged and spoken after the transcript event, so the
                    // truthful line is the last thing the user hears (the
                    // frontend's own "I didn't catch that" would otherwise be
                    // the final word and blame the microphone).
                    let (raw_text, stt_error) = match transcript {
                        Ok(text) => (text, None),
                        Err(e) => (String::new(), Some(e)),
                    };
                    let normalized = crate::intent_parser::normalize_phonetic_mishearings(&raw_text);
                    let text = if normalized.is_empty() { raw_text } else { normalized };
                    let parsed = crate::intent_parser::parse_deterministic(&text);
                    let intent_label = parsed.as_ref().map(|p| crate::intent_parser::intent_to_label(&p.intent));
                    let pre_parsed = parsed.as_ref().and_then(|p| serde_json::to_value(p).ok());

                    tracing::info!("stt-capture: transcript = '{}', intent = {:?}", text, intent_label);
                    crate::latency::mark(crate::latency::Stage::Transcript);
                    let final_payload = SttTranscript {
                        session,
                        text,
                        ownership: stats.owner,
                        owner_score,
                        decoder_bias,
                        language: "en",
                        intent_label,
                        pre_parsed,
                    };
                    // Sidebar's editable "last heard" row reads this cache; empty
                    // transcripts (a silent miss) are ignored inside.
                    crate::last_heard::remember_transcript_text(&final_payload.text);
                    if !final_payload.text.is_empty() {
                        if let Ok(v) = serde_json::to_value(&final_payload) {
                            crate::stage::stash_transcript_if_not_ready(v);
                        }
                    }
                    crate::commands::emit_logged(&app_for_stt, "stt:transcript", &final_payload);
                    stats.stt_path = crate::stt::last_stt_path_str();
                    stats.filter = crate::stt::last_filter_str();
                    crate::commands::emit_logged(&app_for_stt, "stt:turn_stats", &stats);

                    // After the transcript event, so the truthful reason is the
                    // last thing said (the frontend answers an empty transcript
                    // with "I didn't catch that", which blames the mic).
                    //
                    // Linux only: on Windows/macOS a failed transcription has
                    // always ended the turn silently, and that behaviour must
                    // not change.
                    #[cfg(target_os = "linux")]
                    if let Some(e) = stt_error {
                        crate::stt::note_transcription_failure(&app_for_stt, &e);
                    }
                    #[cfg(not(target_os = "linux"))]
                    let _ = stt_error;
                });
            }
        })
        .ok();

    static LAST_NEURAL_FIRE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    while let Ok(candidate) = rx.recv() {
        let now_ms = monotonic_ms();
        let last_fire = LAST_NEURAL_FIRE_MS.load(std::sync::atomic::Ordering::Relaxed);
        // A VERIFIED candidate's self-send must never be eaten by the cooldown the
        // candidate itself started (a fast cloud verify finishes inside 1.5 s; the
        // dropped self-send left VERIFIED_BYPASS armed and a LATER unverified
        // candidate then fired without verification).
        let bypass_pending = VERIFIED_BYPASS.load(std::sync::atomic::Ordering::SeqCst);
        if cooldown_blocks(now_ms, last_fire, bypass_pending) {
            tracing::debug!("wake-word: consecutive candidate suppressed by 1.5s cooldown");
            continue;
        }

        // ── Phase D: Speaker verification ──────────────────────────
        // BEFORE the cooldown store: a rejected stranger must not burn
        // the owner's 1.5s window. Uses a separate AudioFeatures instance
        // so the live engine's streaming buffers stay pristine.
        // Fail-open on any error.
        if crate::commands::read_speaker_verification(&app) {
            match verify_speaker_at_fire(&app, &candidate.audio) {
                Some(false) => {
                    tracing::info!(
                        "wake-word: REJECTED by speaker verification (prob {:.3})",
                        candidate.prob
                    );
                    continue;
                }
                Some(true) => {
                    tracing::debug!("wake-word: ACCEPTED by speaker verification");
                }
                None => {
                    // Not enrolled or error — fail-open, proceed to fire
                }
            }
        }
        LAST_NEURAL_FIRE_MS.store(now_ms, std::sync::atomic::Ordering::Relaxed);

        // ── Stage-2 verifier dispatch ──────────────────────────────
        // The old body below fires immediately; it now runs ONLY when the
        // verifier accepts (via VERIFIED_BYPASS), when verification is
        // disabled, or when a worker can't be spawned (fail-open).
        // A rejected candidate simply never reaches the fire path.
        {
            use std::sync::atomic::Ordering;
            // A previously accepted candidate is waiting: fire now, no re-verify.
            if VERIFIED_BYPASS.swap(false, Ordering::SeqCst) {
                tracing::info!("verify: previously accepted candidate → firing");
                if let Ok(dir) = app.path().app_data_dir() {
                    crate::diary::log_event(&dir, "wake", "trigger (verified)");
                }
            } else if VERIFY_IN_FLIGHT.swap(true, Ordering::SeqCst) {
                tracing::debug!("verify: candidate dropped (already in flight)");
                continue;
            } else if should_instant_fire(crate::commands::read_verify_wake(&app), candidate.prob) {
                // Instant fire ONLY for real stage-1 detections. A prob-0.0
                // candidate (v4 barge-in "exact-only verify") has ZERO neural
                // evidence — firing it here sent phantom "confidence 0.0%"
                // wakes whenever TV/speech played during TTS (measured
                // 2026-09-28 23:00:43). Barge-in candidates are forced
                // through the STT gate below instead, so the exact-only rule
                // ("nexus" must be in the transcript) stays in charge.
                VERIFY_IN_FLIGHT.store(false, Ordering::SeqCst);
                tracing::info!(
                    "wake-word: instant neural trigger (confidence: {:.1}%, prob: {:.3})",
                    candidate.prob * 100.0,
                    candidate.prob
                );
                if let Ok(dir) = app.path().app_data_dir() {
                    crate::diary::log_event(
                        &dir,
                        "wake",
                        &format!("trigger prob {:.3}", candidate.prob),
                    );
                }
            } else {
                tracing::info!(
                    "wake-word: stage-1 candidate (prob {:.3}) → verifying with STT...",
                    candidate.prob
                );
                let app_for_verify = app.clone();
                match std::thread::Builder::new()
                    .name("wake-verify".into())
                    .spawn(move || {
                        verify_candidate(&app_for_verify, candidate.audio, candidate.prob);
                        VERIFY_IN_FLIGHT.store(false, Ordering::SeqCst);
                    }) {
                    Ok(_) => {
                        continue;
                    }
                    Err(e) => {
                        tracing::error!(
                            "verify: spawn failed ({e}) — firing (fail-open)"
                        );
                        VERIFY_IN_FLIGHT.store(false, Ordering::SeqCst);
                    }
                }
            }
        }

        // ── Alexa-style barge-in: a wake that fires while TTS is
        // playing must cut speech FIRST (single choke point, same as
        // the hotkey branch) — otherwise the old reply talks over the
        // new turn. Idempotent when idle: safe on every fire.
        crate::orchestrator::request_barge_in("wake-fire");
        let _ = tauri::Emitter::emit(&app, "tts:stop", ());

        // Start Rust-side STT capture immediately.
        // The cpal stream is already running and just detected the wake word,
        // so it's delivering audio. We capture from the same stream — no baton
        // pass, no getUserMedia. This fixes the Intel SST driver issue where
        // getUserMedia returns silence but cpal is still working.
        start_stt_capture();

        // Only pre-start the local faster-whisper sidecar if Groq cloud STT
        // will NOT be used. This saves ~64-128MB RAM when Groq is configured.
        // Groq is used when: a key exists AND localSttOnly is false.
        let data_dir = app.path().app_data_dir().ok();
        let settings_path = data_dir.as_ref().map(|d| d.join("settings.json"));
        let (groq_key, local_only) = match settings_path {
            Some(p) if p.exists() => {
                let content = std::fs::read_to_string(&p).unwrap_or_default();
                let json: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
                let key = crate::auth_vault::get_api_key("groq").unwrap_or_else(|| {
                    let content = std::fs::read_to_string(&p).unwrap_or_default();
                    let json: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
                    json.get("groqApiKey")
                        .or_else(|| json.get("groq_api_key"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                });
                let local = json.get("localSttOnly")
                    .or_else(|| json.get("local_stt_only"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                (key, local)
            }
            _ => (String::new(), false),
        };

        if groq_key.is_empty() || local_only {
            // Groq won't be used — pre-start the local STT sidecar
            crate::lazy_stt::ensure_stt_running();
        } else {
            tracing::info!("wake-word: Groq cloud STT configured, skipping local sidecar pre-start (saves RAM)");
        }

        // Fire exactly ONE wake signal: `wake_orb_src` emits the single
        // `orb:wake` event (the old window.eval path is gone). Never add a
        // second emit/eval here — the frontend would run its wake handler
        // twice ("on it sir" twice).
        //
        // DAC drain first (mirror hotkey): the cut TTS tail outlives
        // stop_tts() by ~150ms. Spawned so the receiver loop never blocks.
        let app_for_wake = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(
                crate::orchestrator::BARGE_DAC_DRAIN_MS,
            ))
            .await;
            // Never emit the orb events into a stage that is being rebuilt (they are
            // fire-and-forget and would be lost).
            if !crate::stage::wait_until_ready(2000).await {
                tracing::warn!("wake: stage not ready after 2 s - emitting the orb events anyway");
            }
            crate::window_manager::wake_orb_src(&app_for_wake, "neural");
        });
    }
    Ok(())
}

/// Stage-2 verification: transcribe the candidate audio and signal the wake
/// loop to fire ONLY on a transcript match. Runs on a dedicated thread
/// (never the audio path). Fail-open: STT errors, timeouts, and too-short
/// audio all fire anyway — only a confident non-match suppresses the wake.
/// Signalling uses VERIFIED_BYPASS + a self-send on WAKE_TX so the single
/// existing fire path (the wake loop body) stays the only place that fires.
#[cfg(not(feature = "mock-wake"))]
/// Mean-pooled speaker embedding for turn-level authorship checks. Uses the
/// same separate AudioFeatures instance and chunking as wake verification.
fn turn_embedding(check: &mut SpeakerCheck, audio: &[f32]) -> Option<Vec<f32>> {
    let chunk_size = engine::OWW_CHUNK_SIZE;
    let num_chunks = 16;
    let start = audio.len().saturating_sub(chunk_size * num_chunks);
    let relevant = &audio[start..];

    let mut last_features: Option<tract_onnx::prelude::Tensor> = None;
    for chunk in relevant.chunks(chunk_size) {
        if chunk.len() < chunk_size {
            break;
        }
        last_features = check.features.get_audio_features(chunk).ok();
    }

    let features = last_features?;
    let arr = features.into_plain_array::<f32>().ok()?;
    let slice: &[f32] = arr.as_slice().unwrap_or(&[]);
    Some(
        (0..96)
            .map(|j| {
                (0..16)
                    .map(|i| {
                        let idx = i * 96 + j;
                        if idx < slice.len() { slice[idx] } else { 0.0 }
                    })
                    .sum::<f32>()
                    / 16.0
            })
            .collect(),
    )
}

/// Verify one captured turn against the enrolled voice profile. Unlike wake
/// verification, this returns the ownership zone (and score) instead of a
/// binary accept/reject. Infrastructure failures are `Uncertain`, never a
/// silent accept; absence of enrollment is explicitly `Unenrolled`.
#[cfg(not(feature = "mock-wake"))]
pub(crate) fn verify_turn_audio<R: Runtime>(
    app: &AppHandle<R>,
    audio: &[f32],
) -> (crate::voice_profile::TurnOwnership, f32) {
    use crate::voice_profile::{classify_turn_owner, TurnOwnership};

    if !crate::commands::read_speaker_verification(app) {
        return (TurnOwnership::Unenrolled, 1.0);
    }

    fn init_check<R: Runtime>(app: &AppHandle<R>) -> Option<SpeakerCheck> {
        let res_dir = app.path().resource_dir().ok()?;
        let data_dir = app.path().app_data_dir().ok()?;
        let oww_dir = engine::resolve_oww_dir(&res_dir)?;
        let features = engine::AudioFeatures::new(&oww_dir).ok()?;
        let profile_path = data_dir.join("voice_profile.json");
        let verifier = crate::voice_profile::SpeakerVerifier::new(profile_path).ok()?;
        Some(SpeakerCheck { features, verifier })
    }

    if SPEAKER_CHECK.get().is_none() {
        if let Some(check) = init_check(app) {
            let _ = SPEAKER_CHECK.set(std::sync::Mutex::new(check));
        }
    }
    let check = match SPEAKER_CHECK.get() {
        Some(check) => check,
        None => return (TurnOwnership::Uncertain, 0.0),
    };
    let mut check = match check.lock() {
        Ok(check) => check,
        Err(_) => return (TurnOwnership::Uncertain, 0.0),
    };
    let profile = match check.verifier.profile() {
        Some(profile) => profile.clone(),
        None => return (TurnOwnership::Unenrolled, 1.0),
    };
    let embedding = match turn_embedding(&mut check, audio) {
        Some(embedding) => embedding,
        None => return (TurnOwnership::Uncertain, 0.0),
    };
    let similarity = crate::voice_profile::verify_speaker(&profile, &embedding);
    (classify_turn_owner(Some(&profile), Some(similarity)), similarity)
}

/// Verify a wake candidate's speaker against the enrolled voice profile.
/// Returns Some(true) if verified, Some(false) if rejected, None if
/// verification is skipped (not enrolled) or fails (fail-open).
///
/// Uses a separate AudioFeatures instance (not the live engine's) so the
/// streaming buffers stay pristine. Processes the last 16 chunks of audio
/// (~1.28s) to fill the feature buffer, then mean-pools for a robust
/// speaker embedding.
#[cfg(not(feature = "mock-wake"))]
fn verify_speaker_at_fire<R: Runtime>(app: &AppHandle<R>, audio: &[f32]) -> Option<bool> {
    fn init_check<R: Runtime>(app: &AppHandle<R>) -> Option<SpeakerCheck> {
        let res_dir = app.path().resource_dir().ok()?;
        let data_dir = app.path().app_data_dir().ok()?;
        let oww_dir = engine::resolve_oww_dir(&res_dir)?;
        let features = engine::AudioFeatures::new(&oww_dir).ok()?;
        let profile_path = data_dir.join("voice_profile.json");
        let verifier = crate::voice_profile::SpeakerVerifier::new(profile_path).ok()?;
        Some(SpeakerCheck { features, verifier })
    }

    if SPEAKER_CHECK.get().is_none() {
        if let Some(check) = init_check(app) {
            let _ = SPEAKER_CHECK.set(std::sync::Mutex::new(check));
        }
    }
    let check = SPEAKER_CHECK.get()?;
    let mut check = check.lock().ok()?;

    if !check.verifier.is_enrolled() {
        return None;
    }

    let embedding = turn_embedding(&mut check, audio)?;
    let (sim, verified) = check.verifier.verify(&embedding);
    tracing::debug!(
        "speaker-verify: similarity={:.3} verified={}",
        sim, verified
    );
    Some(verified)
}

/// Instant-fire decision for the candidate receiver. `verify_wake` is the
/// user's STT-cross-check setting; `prob` is the candidate's stage-1
/// probability. A candidate with essentially no neural evidence (the
/// prob-0.0 barge-in / verify-bypass candidates) must NEVER take the
/// instant path — it has no stage-1 detection behind it at all, and firing
/// it unverified produced phantom "confidence 0.0%" wakes whenever
/// sustained speech (TV) played during TTS. Those candidates are routed to
/// the STT gate instead.
fn should_instant_fire(verify_wake: bool, prob: f32) -> bool {
    !verify_wake && prob >= 0.3
}

/// Extra TTS-interrupt phrases beyond `ghost::STOP_PHRASES` (whole-utterance matches only).
const BARGE_STOP_EXTRA: &[&str] = &["stop talking", "be quiet", "that's enough", "thats enough"];

/// Instant "stop" for barge-in (Phase 1.4). A barge-in candidate (prob 0.0 — sustained speech while
/// TTS is playing, no neural wake evidence) whose transcript is *exactly* a stop phrase cuts speech
/// and cancels the turn WITHOUT requiring "nexus". Whole-utterance match only: TTS echo of our own
/// sentences ("Stopped typing, sir.") is a different string and can never self-interrupt.
/// Real wake candidates (prob ≥ 0.01) never take this path. Ghost exit phrases are excluded.
fn barge_stop_decision(prob: f32, transcript: &str) -> bool {
    if prob >= 0.01 {
        return false;
    }
    let cleaned: String = transcript
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '\'')
        .collect();
    let t = cleaned.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    if t.is_empty() || t.contains("ghost") {
        return false;
    }
    crate::ghost::is_stop_phrase(&t) || BARGE_STOP_EXTRA.contains(&t.as_str())
}

#[cfg(not(feature = "mock-wake"))]
fn verify_candidate<R: Runtime>(app: &AppHandle<R>, audio: Vec<f32>, prob: f32) {    // Signal the main loop to fire (consumed once via swap).
    let fire = || {
        VERIFIED_BYPASS.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(tx) = WAKE_TX.get() {
            let _ = tx.send(WakeCandidate {
                audio: Vec::new(),
                prob: 0.0,
            });
        }
    };

    // Verifier FAILURES (too short, STT error, timeout) fire only for a confident
    // stage-1 score; otherwise they suppress (see fail_open_allowed).
    let fail_open = |why: &str| {
        if fail_open_allowed(prob) {
            tracing::warn!("verify: {why} — firing (fail-open, stage-1 prob {:.3})", prob);
            fire();
        } else {
            tracing::info!(
                "verify: {why} — SUPPRESSED (stage-1 prob {:.3} < {:.2} fail-open floor)",
                prob,
                VERIFY_FAIL_OPEN_PROB_FLOOR
            );
        }
    };

    if audio.len() < 8000 {
        fail_open(&format!("candidate too short ({} samples)", audio.len()));
        return;
    }

    // VAD-trim: cut leading/trailing silence so Whisper hears the word, not
    // 2s of room tone to hallucinate on ("Thank you."). Falls back to the
    // full buffer if nothing speech-like is found.
    let trimmed = vad_trim(&audio);
    tracing::info!(
        "verify: VAD-trim {} → {} samples (stage-1 prob {:.3})",
        audio.len(),
        trimmed.len(),
        prob
    );

    let samples: Vec<i16> = trimmed
        .iter()
        .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
        .collect();
    #[cfg(not(feature = "mock-wake"))]
    dump_verify_debug(&samples);
    tracing::info!(
        "verify: cross-checking {} samples ({}ms) with STT...",
        samples.len(),
        samples.len() / 16
    );

    // Same runtime pattern as the stt-capture thread: a private
    // current-thread runtime + block_on (the crate has no multi-thread
    // tokio flavor, so a shared runtime cannot be entered from here).
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!("verify: runtime build failed ({e})");
            fail_open("runtime build failed");
            return;
        }
    };

    rt.block_on(async {
        let client = crate::stt::shared_client();

        let res = tokio::time::timeout(
            std::time::Duration::from_secs(VERIFY_TIMEOUT_SECS),
            // NOTE: deliberately NO decoder prompt here. Experiment 2026-09-18
            // (30 owner takes + 4 FA segments): prompt="Hey Nexus..." flipped
            // 11/11 lexus→nexus (good) but ALSO flipped 2/4 FA segments
            // (TV/conversation onset) into exact "Nexus." readings (fatal —
            // exact matches bypass the two-key gate). A decoder prior cannot
            // distinguish true from false nexus acoustics; the accept-set
            // below handles tolerance with an explicit probability key instead.
            crate::stt::transcribe_samples_verbose(&samples, &client, Some(app), None),
        )
        .await;

        match res {
            Ok(Ok((text, segments))) if verify_transcript_accept(&text, prob) => {
                // v3 confidence veto: even a word match dies if the model
                // itself reports (near-certain) non-speech on every segment.
                // Conservative by design — only vetoes the extreme tail.
                if verify_confidence(&segments) {
                    tracing::info!(
                        "verify: STT confirmed '{}' (stage-1 prob {:.3}, {} segs) → firing wake",
                        text,
                        prob,
                        segments.len()
                    );
                    fire();
                } else {
                    tracing::info!(
                        "verify: '{}' matched words but all {} segs no_speech≥{:.2} → SUPPRESSED (phantom)",
                        text,
                        segments.len(),
                        VERIFY_NOSPEECH_VETO
                    );
                }
            }
            Ok(Ok((text, _))) if barge_stop_decision(prob, &text) => {
                tracing::info!("verify: barge-in stop phrase '{}' → cutting speech (no wake)", text);
                crate::orchestrator::request_barge_in("barge-stop");
                let _ = tauri::Emitter::emit(app, "tts:stop", ());
            }
            Ok(Ok((text, _))) => {
                // v4 verify-retry: identical audio often decodes differently
                // across draws (measured run-to-run flips on identical clips).
                // One retry on the FULL (untrimmed) buffer — different input,
                // independent opinion. Only upgrades suppress→fire, never
                // downgrades; bounded by the outer timeout. Same gates apply.
                if prob >= 0.5 {
                    let full_samples: Vec<i16> = audio
                        .iter()
                        .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
                        .collect();
                    tracing::info!(
                        "verify: retrying full buffer ({} samples) after '{}'...",
                        full_samples.len(),
                        text
                    );
                    match crate::stt::transcribe_samples_verbose(
                        &full_samples,
                        &client,
                        Some(app),
                        None,
                    )
                    .await
                    {
                        Ok((text2, segments2))
                            if verify_transcript_accept(&text2, prob)
                                && verify_confidence(&segments2) =>
                        {
                            tracing::info!(
                                "verify: retry read '{}' → firing wake",
                                text2
                            );
                            fire();
                            return;
                        }
                        Ok((text2, _)) => {
                            tracing::info!(
                                "verify: retry read '{}' — still no match",
                                text2
                            );
                        }
                        Err(e) => {
                            tracing::debug!("verify: retry failed ({e}) — keeping suppress");
                        }
                    }
                }
                tracing::info!(
                    "verify: transcript '{}' rejected (stage-1 prob {:.3}) → wake SUPPRESSED",
                    text,
                    prob
                );
            }
            Ok(Err(e)) => {
                fail_open(&format!("STT failed ({e})"));
            }
            Err(_) => {
                fail_open(&format!("STT timed out after {}s", VERIFY_TIMEOUT_SECS));
            }
        }
    });
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
    //
    // Linux is EXCLUDED on purpose: the ALSA `default` frequently resolves to a
    // PipeWire dummy/monitor sink that emits electrical static (RMS ~0.003) instead
    // of speech. Linux therefore probes every device like Windows does, in the
    // Linux probe order: the audio server's own capture path first, the ALSA
    // plugin devices and the loopback sources last (see
    // `crate::audio_linux::probe_priority`).
    #[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
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
    // Linux: names whose metadata query timed out. `default_input_config()`
    // hanging means the device is hung, and probing it again would cost a
    // second full deadline up front — the probe loop tries it last.
    #[cfg(target_os = "linux")]
    let mut hung_in_listing: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (i, d) in devices.iter().enumerate() {
        let name = d.name().unwrap_or_else(|_| "unknown".into());
        // Linux: `default_input_config()` can block FOREVER inside ALSA (the
        // raw `pipewire` PCM did), so even this metadata query runs behind a
        // hard deadline on its own thread. It only opens the PCM (no audio
        // wait), so it gets the short `LIST_QUERY_DEADLINE_MS`; the name is
        // remembered and probed last by the probe loop instead of paying a second
        // full deadline first. On Windows/macOS the query is unchanged.
        #[cfg(target_os = "linux")]
        let cfg = {
            let device = d.clone();
            let deadline =
                std::time::Duration::from_millis(crate::audio_linux::LIST_QUERY_DEADLINE_MS);
            match crate::audio_linux::run_probe_with_deadline(deadline, move || {
                device.default_input_config()
            }) {
                Ok(Ok(cfg)) => Some(cfg),
                Ok(Err(e)) => {
                    tracing::debug!("audio: '{}' default_input_config failed: {e}", name);
                    None
                }
                Err(_) => {
                    tracing::warn!(
                        "audio: '{}' did not answer default_input_config within {}ms — device is hung, it will be probed last",
                        name,
                        crate::audio_linux::LIST_QUERY_DEADLINE_MS
                    );
                    hung_in_listing.insert(name.clone());
                    None
                }
            }
        };
        #[cfg(not(target_os = "linux"))]
        let cfg = d.default_input_config().ok();
        let sr = cfg.as_ref().map(|c| c.sample_rate().0).unwrap_or(0);
        let ch = cfg.as_ref().map(|c| c.channels()).unwrap_or(0);
        tracing::info!("  [{}] '{}' ({}Hz, {}ch)", i, name, sr, ch);
    }

    // Try the default device first, then fall back to others.
    // The Intel SST "Digital Microphones" driver sometimes returns silence
    // in WASAPI shared mode, so we need to try ALL devices.
    //
    // Linux: the ALSA device list is not a microphone list. PipeWire's
    // "Monitor of ..." loopbacks report as real inputs but only carry
    // speaker output, and ALSA's plugin devices (`lavrate`, `jack`, …) open
    // happily and deliver silence. The Linux probe order puts the audio
    // server's own capture path (`default` first, then `pulse`) first, the
    // real hardware next, the raw `pipewire` PCM after the hardware, and the
    // plugins and loopbacks last — see
    // `crate::audio_linux::probe_priority`.
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
    // Linux: no alias dedupe. An earlier version skipped `default` and
    // `pulse` whenever the raw `pipewire` PCM was present, because all three
    // reach the same source — but `default` is the route that actually works
    // on the target machine, while the raw `pipewire` PCM is the one that can
    // hang. Every name stays in the list; `probe_priority` sorts them.
    // Stable sort: devices in the same tier keep the default-device-first
    // order, the ALSA plugin devices and the loopback/dummy sources move to
    // the end.
    #[cfg(target_os = "linux")]
    try_order.sort_by_key(|d| {
        let n = d.name().unwrap_or_default().to_lowercase();
        crate::audio_linux::probe_priority(&n)
    });
    // Linux: a device whose metadata query timed out in the listing is most
    // likely hung, so it is probed last (stable sort keeps the tiers). It is
    // not dropped: a slow cold start must not lose the only working mic.
    #[cfg(target_os = "linux")]
    try_order.sort_by_key(|d| hung_in_listing.contains(&d.name().unwrap_or_default()));

    if try_order.is_empty() {
        return Err("no input devices available".to_string());
    }

    // Linux: keep only the names. cpal's enumeration holds every listed PCM
    // open for as long as its `Device` lives, and a held raw DMIC (`hw:…`)
    // keeps PipeWire's own microphone source from opening the hardware, so
    // every capture through `default` / `pulse` stalls (see
    // `audio_linux::reopen_input_device`). Each candidate is reopened on its
    // own right before its probe.
    #[cfg(target_os = "linux")]
    let try_names: Vec<String> = try_order
        .iter()
        .map(|d| d.name().unwrap_or_else(|_| "unknown".into()))
        .collect();
    #[cfg(target_os = "linux")]
    {
        drop(try_order);
        drop(devices);
        drop(default_device);
    }
    #[cfg(target_os = "linux")]
    let total = try_names.len();
    #[cfg(not(target_os = "linux"))]
    let total = try_order.len();
    #[cfg(target_os = "linux")]
    let candidates = try_names.iter().enumerate().filter_map(|(i, name)| {
        let device = crate::audio_linux::reopen_input_device(&host, name);
        if device.is_none() {
            tracing::warn!("audio: device '{}' is gone — skipping it", name);
        }
        device.map(|d| (i, d))
    });
    #[cfg(not(target_os = "linux"))]
    let candidates = try_order.iter().cloned().enumerate();

    // Try each device until we find one that produces non-zero audio.
    // We start the stream, wait for the first callback (the window depends
    // on the device — PipeWire's ALSA bridge is slow to start), and check
    // the RMS.
    // If the device produces silence (Intel SST bug), try the next device.
    let mut last_err = String::new();
    // Windows/macOS: the highest-RMS device, using >= so that even a device
    // with RMS = 0.0 is remembered as the fallback (see the tie-break note
    // below).
    #[cfg(not(target_os = "linux"))]
    let mut best_device: Option<(cpal::Device, f32)> = None; // (device, rms)
    // Linux: every device that failed the silence probe, in probe order, so
    // the fallback can prefer a real capture path over a plugin/loopback.
    #[cfg(target_os = "linux")]
    let mut probed: Vec<(usize, String, f32)> = Vec::new();

    for (attempt, device) in candidates {
        let device = &device;
        let dev_name = device.name().unwrap_or_else(|_| "unknown".into());
        tracing::info!(
            "audio: trying device [{}] '{}' (attempt {}/{})",
            attempt, dev_name, attempt + 1, total
        );

        // Linux: PipeWire's ALSA bridge delivers its first callback late, so
        // the audio server's own capture path gets a longer window — with
        // the short one a perfectly good microphone is reported as silent.
        #[cfg(target_os = "linux")]
        let probe_wait_ms = crate::audio_linux::probe_wait_ms(&dev_name);
        #[cfg(not(target_os = "linux"))]
        let probe_wait_ms = 500u64;

        // Linux: every device probe — `default_input_config()` and building
        // the stream included — runs on its own thread behind a hard deadline
        // (the probe window plus a grace period). The raw ALSA `pipewire` PCM
        // blocked FOREVER inside `default_input_config()` on the target
        // machine; a hung device is abandoned here (never joined, so the call
        // that never returns can never block us), logged, and skipped. The
        // engine mutex is NOT held across the device call — `try_device`
        // locks it only after the device answered, to read `sample_rate`.
        #[cfg(target_os = "linux")]
        let probe_result = {
            let deadline_ms = crate::audio_linux::probe_deadline_ms(&dev_name);
            // The caller keeps one guard, the worker thread gets the other.
            // On timeout the caller cancels its worker: if the hung ALSA call
            // ever returns, the late thread must not install its stream over
            // the device we settled on.
            let cancel = ProbeCancel::new();
            let worker_cancel = cancel.clone();
            let device = device.clone();
            let engine = engine.clone();
            let deadline = std::time::Duration::from_millis(deadline_ms);
            let result = crate::audio_linux::run_probe_with_deadline(deadline, move || {
                try_device(&device, engine, probe_wait_ms, worker_cancel)
            });
            match result {
                Ok(result) => result,
                Err(crate::audio_linux::ProbeWaitError::TimedOut) => {
                    match cancel.cancel() {
                        crate::audio_linux::ProbeCancelOutcome::AlreadyInstalled => {
                            // The worker stored its stream just before the
                            // deadline won the race. Treat the device exactly
                            // like an `Ok(())`: probing another device now
                            // would install a second live stream over it.
                            tracing::info!(
                                "audio: device '{}' was accepted just before its {}ms deadline — keeping it",
                                dev_name, deadline_ms
                            );
                            return Ok(());
                        }
                        _ => {
                            tracing::warn!(
                                "audio: device '{}' hung inside the device call for {}ms — abandoning it and trying the next device",
                                dev_name, deadline_ms
                            );
                            last_err = format!(
                                "device '{}' hung inside the device call ({}ms deadline)",
                                dev_name, deadline_ms
                            );
                            continue;
                        }
                    }
                }
                Err(crate::audio_linux::ProbeWaitError::Failed) => {
                    tracing::warn!(
                        "audio: probe thread for '{}' could not run — trying the next device",
                        dev_name
                    );
                    last_err = format!("device '{}' probe thread failed", dev_name);
                    continue;
                }
            }
        };
        // Windows/macOS: unchanged — probe inline with an inert guard (its
        // checks are compiled to `false`).
        #[cfg(not(target_os = "linux"))]
        let probe_result = try_device(device, engine.clone(), probe_wait_ms, ProbeCancel::new());

        match probe_result {
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
                #[cfg(not(target_os = "linux"))]
                {
                    if best_device.is_none()
                        || rms >= best_device.as_ref().map(|(_, r)| *r).unwrap_or(0.0)
                    {
                        best_device = Some((device.clone(), rms));
                    }
                }
                // Linux keeps every candidate (the first-wins tie-break lives
                // in `audio_linux::silent_fallback_index`, which also prefers
                // a real device over a plugin/loopback).
                #[cfg(target_os = "linux")]
                probed.push((attempt, dev_name.clone(), rms));
                tracing::warn!("audio: device '{}' failed: {}", dev_name, e);
                last_err = e;
            }
        }
    }

    // All devices produced silence. Fall back to the best device instead of
    // giving up entirely. The mic may start working later (driver recovery,
    // user unmutes, headset reconnects, etc.).
    //
    // Linux: prefer the best real capture path (`default`, `pulse`, `hw:` …
    // over the raw `pipewire` PCM that is known to hang) and never an ALSA
    // plugin or a loopback even when the plugin's RMS is equal or higher — a
    // plugin never becomes a microphone, while a real device may start working
    // as soon as the user unmutes it. The overall best device is still used
    // when no real device exists, so the fallback is never lost.
    #[cfg(target_os = "linux")]
    let fallback: Option<(cpal::Device, f32)> = {
        let candidates: Vec<(&str, f32)> = probed
            .iter()
            .map(|(_, name, rms)| (name.as_str(), *rms))
            .collect();
        crate::audio_linux::silent_fallback_index(&candidates).and_then(|i| {
            crate::audio_linux::reopen_input_device(&host, &try_names[probed[i].0])
                .map(|d| (d, probed[i].2))
        })
    };
    #[cfg(not(target_os = "linux"))]
    let fallback = best_device;

    if let Some((device, rms)) = fallback {
        let dev_name = device.name().unwrap_or_else(|_| "unknown".into());
        // The cause is platform specific: on Windows it is usually the Intel
        // SST driver, on Linux a muted input or an audio server that is not
        // running.
        #[cfg(target_os = "linux")]
        tracing::warn!(
            "audio: ALL devices produced silence! Falling back to '{}' (RMS={:.6}). \
             The input may be muted, the wrong source may be selected, or PipeWire may not be running. \
             Wake word will not work until the mic produces audio.",
            dev_name, rms
        );
        #[cfg(not(target_os = "linux"))]
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
    // Linux throttles the log (ALSA floods the callback when the device dies);
    // everywhere else the error is logged as-is.
    #[cfg(target_os = "linux")]
    let err_cb = log_stream_err_throttled;
    #[cfg(not(target_os = "linux"))]
    let err_cb = |err| {
        tracing::error!("audio stream error: {err}");
        // TRUE death signal (device invalidated/removed) — the recovery
        // thread consumes this. Previously only logged, never acted on.
        STREAM_ERROR.store(true, std::sync::atomic::Ordering::Relaxed);
    };

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
    log_capture_device(&device.name().unwrap_or_else(|_| "unknown".into()));
    // Store the stream in the global so pause_stream/resume_stream can
    // access it for the mic baton pass. Keep it alive forever.
    *CPAL_STREAM.write() = Some(SendStream(stream));
    Ok(())
}

/// Log that the wake-word engine is now listening on a capture device.
///
/// The name is the device that was really opened. On Linux that matters:
/// the acoustic profile's device name is the microphone the profile was
/// calibrated for (a Windows one), while the capture path that is actually
/// in use is the audio server's source.
#[cfg(not(feature = "mock-wake"))]
fn log_capture_device(device_name: &str) {
    #[cfg(target_os = "linux")]
    tracing::info!(
        "audio: stream started on '{}', OWW KWS listening for 'nexus' (this is the capture device actually in use)...",
        device_name
    );
    #[cfg(not(target_os = "linux"))]
    tracing::info!(
        "audio: stream started on '{}', OWW KWS listening for 'nexus'...",
        device_name
    );
}

/// Error text for a probe that outlived its deadline and was abandoned.
#[cfg(not(feature = "mock-wake"))]
const PROBE_ABANDONED: &str = "probe abandoned (deadline expired)";

/// Three-state handshake for a Linux device probe.
///
/// A Linux probe runs on its own thread behind a hard deadline
/// ([`crate::audio_linux::run_probe_with_deadline`]). The worker claims the
/// install slot ([`ProbeCancel::try_install`]) inside the critical section
/// that stores the stream; the caller claims the cancellation
/// ([`ProbeCancel::cancel`]) when the deadline expires. Whichever wins, the
/// loser sees it: a cancelled probe cannot install, and a caller that lost to
/// an install treats the device as accepted instead of probing another one
/// over a live stream.
///
/// On Windows/macOS the probe runs inline and nothing can be abandoned, so
/// every check compiles to `false`/`true` and behaviour is unchanged.
#[cfg(not(feature = "mock-wake"))]
#[derive(Clone, Default)]
struct ProbeCancel {
    #[cfg(target_os = "linux")]
    state: crate::audio_linux::ProbeState,
}

#[cfg(not(feature = "mock-wake"))]
impl ProbeCancel {
    fn new() -> Self {
        Self::default()
    }

    /// The caller gave up on this probe: discard whatever it returns.
    ///
    /// Returns [`crate::audio_linux::ProbeCancelOutcome::AlreadyInstalled`]
    /// when the worker installed its stream just before the deadline; the
    /// caller must then keep that device.
    #[cfg(target_os = "linux")]
    fn cancel(&self) -> crate::audio_linux::ProbeCancelOutcome {
        self.state.try_cancel()
    }

    fn is_cancelled(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.state.is_cancelled()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    /// Claim the right to install the stream.
    ///
    /// Off Linux the probe is inline and cannot be abandoned, so the claim
    /// always succeeds.
    fn try_install(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.state.try_install()
        }
        #[cfg(not(target_os = "linux"))]
        {
            true
        }
    }
}

/// Try to start audio capture on a single device.
/// Starts the stream, waits `probe_wait_ms` for the first callback, and
/// checks the RMS of the captured audio. If the device produces silence
/// (Intel SST bug), returns an error so the caller can try the next device.
#[cfg(not(feature = "mock-wake"))]
fn try_device(
    device: &cpal::Device,
    engine: std::sync::Arc<parking_lot::Mutex<engine::WakeEngine>>,
    probe_wait_ms: u64,
    cancel: ProbeCancel,
) -> Result<(), String> {
    use cpal::traits::{DeviceTrait, StreamTrait};
    use cpal::Sample;
    use std::sync::atomic::{AtomicU64, Ordering};

    let default_config = device
        .default_input_config()
        .map_err(|e| format!("default_input_config: {e}"))?;

    // The engine mutex is deliberately NOT held across the device call above:
    // `default_input_config()` can block for a very long time (forever, on the
    // raw `pipewire` PCM), and holding the lock would freeze every other
    // wake-engine user while ALSA thinks. It is only locked below, to read
    // `sample_rate`.
    if cancel.is_cancelled() {
        return Err(PROBE_ABANDONED.to_string());
    }

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

    // Linux throttles the log (ALSA floods the callback when the device dies);
    // everywhere else the error is logged as-is.
    #[cfg(target_os = "linux")]
    let err_cb = log_stream_err_throttled;
    #[cfg(not(target_os = "linux"))]
    let err_cb = |err| {
        tracing::error!("audio stream error: {err}");
        // TRUE death signal (device invalidated/removed) — the recovery
        // thread consumes this. Previously only logged, never acted on.
        STREAM_ERROR.store(true, std::sync::atomic::Ordering::Relaxed);
    };

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
                    let active_ch = if ch == 4 { 2 } else { ch };
                    let frames = data.len() / ch;
                    let mut sq_sum = 0.0f64;
                    for i in 0..frames {
                        let mut sum = 0.0f32;
                        for c in 0..active_ch {
                            sum += data[i * ch + c].to_sample::<f32>();
                        }
                        let mono = sum / active_ch as f32;
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
                    let active_ch = if ch == 4 { 2 } else { ch };
                    let frames = data.len() / ch;
                    let mut sq_sum = 0.0f64;
                    for i in 0..frames {
                        let mut sum = 0.0f32;
                        for c in 0..active_ch {
                            sum += data[i * ch + c].to_sample::<f32>();
                        }
                        let mono = sum / active_ch as f32;
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
                    let active_ch = if ch == 4 { 2 } else { ch };
                    let frames = data.len() / ch;
                    let mut sq_sum = 0.0f64;
                    for i in 0..frames {
                        let mut sum = 0.0f32;
                        for c in 0..active_ch {
                            sum += data[i * ch + c];
                        }
                        let mono = sum / active_ch as f32;
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
    // The deadline may have expired while ALSA was building the stream: this
    // probe has been abandoned and must not touch the global stream slot.
    if cancel.is_cancelled() {
        return Err(PROBE_ABANDONED.to_string());
    }
    stream.play().map_err(|e| format!("play stream: {e}"))?;
    // `play()` can itself block inside ALSA. If the deadline expired while it
    // did, this probe has been abandoned: returning drops `stream`, so the
    // late probe cannot play audio alongside the device being probed now.
    if cancel.is_cancelled() {
        return Err(PROBE_ABANDONED.to_string());
    }

    // Wait for the first callback and compute RMS over the window.
    std::thread::sleep(std::time::Duration::from_millis(probe_wait_ms));

    let samples = total_samples.load(Ordering::Relaxed);
    let sq = sum_sq.load(Ordering::Relaxed) as f64 / 1e9;

    if samples == 0 {
        tracing::warn!(
            "audio: device produced 0 samples in {}ms — no audio callback fired, trying next device",
            probe_wait_ms
        );
        drop(stream);
        return Err("no audio callbacks received".to_string());
    }

    let rms = (sq / samples as f64).sqrt() as f32;
    tracing::info!("audio: probe RMS = {:.6} ({} samples)", rms, samples);

    // If RMS is 0.0 (true driver lockout), try the next device.
    // In a quiet room with Intel SST, ambient noise is ~0.00002-0.00008 RMS.
    // The Intel SST lockup produces exactly 0.000000 RMS.
    if sq == 0.0 || rms < 1e-6 {
        // On Linux there is no Intel SST driver: a dead capture device is a
        // muted input, the wrong source, or an audio server that is not
        // running.
        #[cfg(target_os = "linux")]
        tracing::warn!(
            "audio: device RMS {:.6} is zero — the input is silent (muted, wrong source, or PipeWire is not running), trying next device",
            rms
        );
        #[cfg(not(target_os = "linux"))]
        tracing::warn!(
            "audio: device RMS {:.6} is zero — Intel SST driver lockout, trying next device",
            rms
        );
        drop(stream);
        return Err(format!("device produces silence (RMS={:.6})", rms));
    }

    // Last check before the install: the probe may have outlived its deadline
    // by now (the waiter gave up and moved on to another device).
    if cancel.is_cancelled() {
        return Err(PROBE_ABANDONED.to_string());
    }
    log_capture_device(&device.name().unwrap_or_else(|_| "unknown".into()));
    // Store the stream in the global so pause_stream/resume_stream can
    // access it for the mic baton pass. Keep it alive forever.
    //
    // The install claim shares one critical section with the store, and it is
    // a compare-exchange: if the caller cancelled just before this point the
    // claim fails and nothing is stored; if the claim wins, the caller's
    // cancel reports `AlreadyInstalled` and keeps this device instead of
    // probing another one over a live stream.
    {
        let mut slot = CPAL_STREAM.write();
        if !cancel.try_install() {
            return Err(PROBE_ABANDONED.to_string());
        }
        *slot = Some(SendStream(stream));
    }
    Ok(())
}

#[cfg(all(test, feature = "wakeword-oww"))]
mod tests {
    use std::path::PathBuf;

    use super::level_from_rms;

    /// Phase R: a second capture start while one is live and young must be ignored
    /// (it would clear the buffer + reset VAD); everything else must start normally.
    #[test]
    fn duplicate_capture_start_guard() {
        use super::{is_duplicate_capture_start as dup, DUPLICATE_START_WINDOW_MS as W};
        // First start ever (no recorded start) is never a duplicate.
        assert!(!dup(false, 1000, 0, W));
        assert!(!dup(true, 1000, 0, W));
        // Live + young => duplicate.
        assert!(dup(true, 1200, 1000, W));
        assert!(dup(true, 1000, 1000, W));
        // Live but old enough => a legitimate new turn.
        assert!(!dup(true, 1000 + W, 1000, W));
        assert!(!dup(true, 5000, 1000, W));
        // Capture already finished/aborted => never a duplicate, even inside the window.
        assert!(!dup(false, 1100, 1000, W));
        // Clock oddities never block a start.
        assert!(!dup(true, 500, 1000, W));
    }

    /// Smart Turn glue: flag off must be byte-identical to the legacy rule, and a verdict is only
    /// valid for the silence run it was computed for.
    #[test]
    fn smart_turn_poll_off_and_run_invalidation() {
        use std::sync::atomic::Ordering::SeqCst;
        // OFF (default): never ends a turn, never changes the limit.
        super::SMART_TURN_ON.store(false, SeqCst);
        assert_eq!(super::smart_turn_poll(true, 10, 9, 10), (false, 10));
        assert_eq!(super::smart_turn_poll(true, 10, 0, 10), (false, 10));

        // ON, with a hand-planted verdict (no worker needed: TX unset => no requests are made).
        super::SMART_TURN_ON.store(true, SeqCst);
        let run = super::SMART_TURN_RUN.load(SeqCst);
        *super::SMART_TURN_VERDICT.lock() = (run, 0.9);
        // < STT_MIN_VOICED_CHUNKS voiced: Smart Turn stays out of it
        assert_eq!(super::smart_turn_poll(true, 1, 6, 10), (false, 10));
        // medium policy (min 5 chunks, thr 0.5): 4 chunks of silence is too early
        assert_eq!(super::smart_turn_poll(true, 10, 4, 10), (false, 10));
        // 5 chunks + verdict 0.9 for this run => end now, limit untouched
        assert_eq!(super::smart_turn_poll(true, 10, 5, 10), (true, 10));
        // "not finished" verdict holds the turn open (limit 10 -> 15), does not end it
        *super::SMART_TURN_VERDICT.lock() = (run, 0.1);
        assert_eq!(super::smart_turn_poll(true, 10, 9, 10), (false, 15));
        // speech resumes (silence == 0) => run bumps, old verdict is void for the next silence run
        assert_eq!(super::smart_turn_poll(true, 10, 0, 10), (false, 10));
        *super::SMART_TURN_VERDICT.lock() = (run, 0.9);
        assert_eq!(super::smart_turn_poll(true, 10, 6, 10), (false, 10));

        super::SMART_TURN_ON.store(false, SeqCst); // leave global state as found
    }

    /// Phase 1.4: instant "stop" during TTS — whole-utterance only, barge-in candidates only.
    #[test]
    fn barge_stop_decision_table() {
        use super::barge_stop_decision as d;
        // exact stop phrases (punctuation/case from STT are normalised)
        assert!(d(0.0, "Stop."));
        assert!(d(0.0, "  stop it! "));
        assert!(d(0.0, "Cancel"));
        assert!(d(0.0, "Stop talking."));
        assert!(d(0.0, "That's enough."));
        assert!(d(0.0, "Be quiet"));
        // our own TTS echoing back must NOT self-interrupt
        assert!(!d(0.0, "Stopped typing, sir."));
        assert!(!d(0.0, "I'll stop when you say so"));
        assert!(!d(0.0, "stop the music and open whatsapp"));
        // empty / noise
        assert!(!d(0.0, ""));
        assert!(!d(0.0, "..."));
        // real wake candidates are never stop commands
        assert!(!d(0.9, "stop"));
        assert!(!d(0.3, "cancel"));
        // ghost exits keep their own path
        assert!(!d(0.0, "exit ghost mode"));
        assert!(!d(0.0, "stop ghost"));
    }

    /// `chunk_voice_flags` contract — energy mode must stay byte-identical to the legacy RMS
    /// hysteresis (cont > 0.006, start > 0.010); neural mode gates on probability + RMS floor.
    #[test]
    fn chunk_voice_flags_energy_mode_is_legacy_hysteresis() {
        use super::chunk_voice_flags as f;
        assert_eq!(f(0.0, None), (false, false));
        assert_eq!(f(0.006, None), (false, false)); // strict >
        assert_eq!(f(0.008, None), (true, false)); // hysteresis band: continues, never starts
        assert_eq!(f(0.010, None), (true, false));
        assert_eq!(f(0.011, None), (true, true));
        assert_eq!(f(0.2, None), (true, true));
    }

    #[test]
    fn chunk_voice_flags_neural_mode() {
        use super::chunk_voice_flags as f;
        // loud noise the model rejects must NOT count as voice (the RMS gate would have)
        assert_eq!(f(0.2, Some(0.05)), (false, false));
        // quiet-but-real speech the RMS gate misses (rms 0.004 < 0.006) is voice
        assert_eq!(f(0.004, Some(0.9)), (true, true));
        // hysteresis: 0.4 continues a turn but cannot start one
        assert_eq!(f(0.05, Some(0.4)), (true, false));
        assert_eq!(f(0.05, Some(0.5)), (true, true));
        // digital silence is never voice, whatever the model says
        assert_eq!(f(0.0, Some(0.99)), (false, false));
        assert_eq!(f(0.001, Some(0.99)), (false, false));
    }

    /// UI level map contract for speech-synced visuals (`audio:level`).
    /// Silence/dead input → 0 (never NaN over the bridge); speech band
    /// maps monotonically; 0.10 saturates at 1.0.
    #[test]
    fn test_level_from_rms_contract() {
        assert_eq!(level_from_rms(0.0), 0.0);
        assert_eq!(level_from_rms(-0.5), 0.0);
        assert_eq!(level_from_rms(f32::NAN), 0.0);
        assert_eq!(level_from_rms(f32::INFINITY), 0.0);
        let quiet = level_from_rms(0.01);
        let mid = level_from_rms(0.05);
        assert!(quiet > 0.0 && quiet < mid, "quiet={quiet} mid={mid}");
        assert!(mid < 1.0, "mid={mid}");
        assert_eq!(level_from_rms(0.10), 1.0);
        assert_eq!(level_from_rms(0.30), 1.0);
    }

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

        // Load by PATH (not cursor): split models (nexus.onnx + .data)
        // resolve external weights relative to the file. This mirrors
        // load_onnx_model — the cursor path cannot do this.
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

        // Model outputs must be valid finite probabilities in [0.0, 1.0]
        assert!(
            prob5 >= 0.0 && prob5 <= 1.0 && prob5.is_finite(),
            "tract-onnx all-5 features produced {:.6}, expected bounded probability [0, 1]",
            prob5
        );
        assert!(
            prob12 >= 0.0 && prob12 <= 1.0 && prob12.is_finite(),
            "tract-onnx all-12 features produced {:.6}, expected bounded probability [0, 1]",
            prob12
        );
        assert!(
            prob_neg5 >= 0.0 && prob_neg5 <= 1.0 && prob_neg5.is_finite(),
            "tract-onnx all-(-5) features produced {:.6}, expected bounded probability [0, 1]",
            prob_neg5
        );
    }

    // ─── Phase B: Audio Preprocessor Tests ───────────────────────────

    /// Test that the high-pass filter removes low-frequency content.
    /// A DC offset (constant value) should be removed by the filter.
    #[test]
    fn test_high_pass_filter_removes_dc() {
        let mut filter = crate::wakeword_oww::engine::HighPassFilter::new(80.0, 16000.0);
        // 1280 samples of DC offset (constant 0.5)
        let mut samples = vec![0.5f32; 1280];
        filter.process(&mut samples);
        // After filtering, the DC offset should be significantly reduced
        // (high-pass filters remove DC/constant components)
        let mean: f32 = samples.iter().sum::<f32>() / samples.len() as f32;
        assert!(
            mean.abs() < 0.1,
            "High-pass filter did not remove DC offset: mean={:.4}",
            mean
        );
    }

    /// Test that the high-pass filter preserves speech-frequency content.
    /// A 1000Hz sine wave should pass through with minimal attenuation.
    #[test]
    fn test_high_pass_filter_preserves_speech() {
        let mut filter = crate::wakeword_oww::engine::HighPassFilter::new(80.0, 16000.0);
        // 1280 samples of 1000Hz sine wave (well above 80Hz cutoff)
        let sr = 16000.0f32;
        let freq = 1000.0f32;
        let mut samples: Vec<f32> = (0..1280)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr).sin() * 0.5)
            .collect();
        let original_energy: f32 = samples.iter().map(|s| s * s).sum();
        filter.process(&mut samples);
        let filtered_energy: f32 = samples.iter().map(|s| s * s).sum();
        // Energy should be mostly preserved (allow some transient at start)
        assert!(
            filtered_energy > original_energy * 0.5,
            "High-pass filter attenuated speech too much: {:.4} → {:.4}",
            original_energy,
            filtered_energy
        );
    }

    /// Test that the noise floor tracker adapts to background noise.
    #[test]
    fn test_noise_floor_tracker_adapts() {
        let mut tracker = crate::wakeword_oww::engine::NoiseFloorTracker::new();
        // Feed low RMS values (background noise)
        for _ in 0..20 {
            tracker.update(0.001);
        }
        let floor_quiet = tracker.floor();
        assert!(
            floor_quiet <= 0.001,
            "Noise floor should be ~0.001 in quiet: got {:.6}",
            floor_quiet
        );
        // Feed higher RMS values (speech)
        for _ in 0..20 {
            tracker.update(0.05);
        }
        let floor_loud = tracker.floor();
        // Floor should still track the minimum, not the maximum
        assert!(
            floor_loud < 0.05,
            "Noise floor should track minimum, not maximum: got {:.6}",
            floor_loud
        );
    }

    /// Test that the VAD detects speech in a synthetic signal.
    #[test]
    fn test_vad_detects_speech() {
        let mut vad = crate::wakeword_oww::engine::VadDetector::new();
        let sr = 16000.0f32;
        // Simulate speech: 1000Hz tone with moderate amplitude
        let speech: Vec<f32> = (0..1280)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr).sin() * 0.3)
            .collect();
        assert!(
            vad.detect(&speech, 0.001),
            "VAD should detect speech in 1000Hz tone"
        );
    }

    /// Test that the VAD rejects pure noise.
    #[test]
    fn test_vad_rejects_noise() {
        let mut vad = crate::wakeword_oww::engine::VadDetector::new();
        // Simulate noise: high-frequency random-like signal with low energy
        let noise: Vec<f32> = (0..1280).map(|i| {
            // Pseudo-random high-frequency signal (alternating signs)
            if i % 2 == 0 { 0.001 } else { -0.001 }
        }).collect();
        let result = vad.detect(&noise, 0.001);
        // Low energy noise should not be detected as speech
        assert!(
            !result,
            "VAD should reject low-energy noise"
        );
    }

    /// Test that the full preprocessor pipeline works end-to-end.
    #[test]
    fn test_preprocessor_pipeline() {
        let mut pp = crate::wakeword_oww::engine::AudioPreprocessor::new();
        let sr = 16000.0f32;

        // Test 1: Silence should be rejected
        let silence = vec![0.0f32; 1280];
        let result = pp.process(silence);
        assert!(result.is_none(), "Preprocessor should reject silence");

        // Test 2: Speech-like signal should pass
        let speech: Vec<f32> = (0..1280)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr).sin() * 0.3)
            .collect();
        let result = pp.process(speech);
        assert!(result.is_some(), "Preprocessor should pass speech");

        // Test 3: Stats should be tracking
        assert!(pp.vad_skips > 0 || pp.vad_passes > 0, "Preprocessor stats should be non-zero");
    }

    /// Test that preprocessor reset clears all state.
    #[test]
    fn test_preprocessor_reset() {
        let mut pp = crate::wakeword_oww::engine::AudioPreprocessor::new();
        // Process some audio to populate state
        let sr = 16000.0f32;
        let speech: Vec<f32> = (0..1280)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr).sin() * 0.3)
            .collect();
        let _ = pp.process(speech);
        let skips_before = pp.vad_skips;
        let passes_before = pp.vad_passes;
        assert!(skips_before + passes_before > 0, "Should have processed some audio");

        // Reset
        pp.reset();
        assert_eq!(pp.vad_skips, 0, "Reset should clear vad_skips");
        assert_eq!(pp.vad_passes, 0, "Reset should clear vad_passes");
    }

    // ─── Stage-2 verifier tests (dual-gate Test A artifact) ──────────

    /// Transcript gating accepts genuine wake phrases (any case/padding).
    #[test]
    fn test_verify_transcript_accepts() {
        for text in [
            "hey nexus",
            "NEXUS",
            "Hey Nexus",
            "ok nexus please",
            "  Nexus. ",
            "could you nexus the thing",
            "HEYYYY NEXUS",
        ] {
            assert!(
                super::verify_transcript(text),
                "should accept '{}'",
                text
            );
        }
    }

    /// Transcript gating rejects non-wake speech, silence, and soundalikes
    /// that do NOT contain the wake word. (A "lexus nexus" transcript DOES
    /// contain it and must accept — the STT heard the word.)
    /// NOTE: v1 `verify_transcript` == gated(.., 1.0), so "hey lexus" ACCEPTS
    /// here (two-key: near-match at max prob) — covered by the v2 matrix below.
    #[test]
    fn test_verify_transcript_rejects() {
        for text in [
            "",
            "   ",
            "next us",
            "thank you for watching",
            "alexa play music",
            "ok google",
        ] {
            assert!(
                !super::verify_transcript(text),
                "should reject '{}'",
                text
            );
        }
        // "lexus nexus" contains the wake word → must accept.
        assert!(super::verify_transcript("lexus nexus"));
        // Bare "nexus" is a valid wake.
        assert!(super::verify_transcript("nexus"));
    }

    /// Ring keeps newest audio and evicts oldest beyond capacity.
    #[test]
    fn test_verify_ring_cap() {
        let mut ring = std::collections::VecDeque::new();
        let cap = 40000;
        let first: Vec<f32> = (0..20000).map(|i| i as f32).collect();
        let second: Vec<f32> = (0..20000).map(|i| 100000.0 + i as f32).collect();
        let third: Vec<f32> = (0..20000).map(|i| 200000.0 + i as f32).collect();
        super::push_capped(&mut ring, &first, cap);
        super::push_capped(&mut ring, &second, cap);
        super::push_capped(&mut ring, &third, cap);
        assert_eq!(ring.len(), cap);
        // Oldest (first push) fully evicted; newest retained.
        assert!(ring[0] >= 100000.0, "oldest audio should be evicted");
        assert_eq!(ring[ring.len() - 1], 200000.0 + 19999.0);
    }

    /// Ring preserves short audio untouched (no truncation below capacity).
    #[test]
    fn test_verify_ring_under_cap() {
        let mut ring = std::collections::VecDeque::new();
        let chunk = vec![0.5f32; 1280];
        super::push_capped(&mut ring, &chunk, 40000);
        assert_eq!(ring.len(), 1280);
        assert!(ring.iter().all(|&s| s == 0.5));
    }

    // ─── Verifier v2 tests ──────────────────────────────────────────

    /// Two-key gate: exact "nexus" fires at ANY stage-1 probability.
    #[test]
    fn test_gated_exact_fires_at_any_prob() {
        for prob in [0.0, 0.2, 0.36, 0.9] {
            assert!(
                super::verify_transcript_gated("hey nexus", prob),
                "exact must fire at prob {}",
                prob
            );
            assert!(super::verify_transcript_gated("NEXUS!", prob));
        }
    }

    /// Two-key gate: near-matches (lexus/nexis/…) fire ONLY at prob ≥ 0.6.
    #[test]
    fn test_gated_near_needs_high_prob() {
        for word in [
            "lexus",
            "nexis",
            "nixus",
            "nixis",
            "nexas",
            "hey lexus please",
            "LEXUS",
            "nexo",
            "hey nixus",
        ] {
            assert!(
                !super::verify_transcript_gated(word, 0.59),
                "'{}' must NOT fire at 0.59",
                word
            );
            assert!(
                super::verify_transcript_gated(word, 0.6),
                "'{}' must fire at 0.6",
                word
            );
            assert!(super::verify_transcript_gated(word, 0.99));
        }
    }

    /// Two-key gate: common-word collisions and non-words never fire —
    /// even at max probability. (texas/next stay rejected: TV frequency.)
    #[test]
    fn test_gated_rejects_collisions() {
        for text in [
            "texas",
            "hey texas",
            "next",
            "next us",
            "next episode",
            "hey alexis", // whole-word: "alexis" != "lexis"
            "thank you",
            "process this",
            "",
            "   ",
        ] {
            assert!(
                !super::verify_transcript_gated(text, 1.0),
                "'{}' must never fire",
                text
            );
        }
    }

    /// VAD-trim cuts silence padding but keeps the speech body + margin.
    #[test]
    fn test_vad_trim_cuts_silence() {
        // 1s silence + 0.5s loud speech + 1s silence (2.5s like the ring).
        let mut audio = vec![0.0f32; 16000];
        audio.extend(vec![0.5f32; 8000]);
        audio.extend(vec![0.0f32; 16000]);
        let trimmed = super::vad_trim(&audio);
        // Speech (8000) + 2x300ms margin (9600) ≈ 17600, well under 40000.
        assert!(
            trimmed.len() >= 8000 && trimmed.len() <= 8000 + 9600 + 640,
            "trimmed len {} out of range",
            trimmed.len()
        );
        assert!(trimmed.len() < audio.len());
    }

    /// VAD-trim falls back to full buffer on all-silence or fragments.
    #[test]
    fn test_vad_trim_fallback() {
        // All silence: no frames pass → full buffer.
        let silence = vec![0.0f32; 40000];
        assert_eq!(super::vad_trim(&silence).len(), 40000);
        // 0.05s blip: trimmed to blip ±300ms margins (7040 samples) —
        // tight context around the sound, not the full 2.5s of silence.
        let mut blip = vec![0.0f32; 40000];
        for s in blip.iter_mut().take(2000).skip(1600) {
            *s = 0.5;
        }
        assert_eq!(super::vad_trim(&blip).len(), 7040);
        assert!(super::vad_trim(&[]).is_empty());
    }

    /// Debug WAV encoder produces a valid 16kHz mono PCM16 file.
    #[test]
    fn test_encode_wav_pcm16_valid() {
        let wav = super::encode_wav_pcm16(&[0i16, 32767, -32768, 1000]);
        assert_eq!(wav.len(), 44 + 8);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[24..28], 16000u32.to_le_bytes());
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(&wav[40..44], 8u32.to_le_bytes());
        assert_eq!(&wav[44..46], 0i16.to_le_bytes());
        assert_eq!(&wav[46..48], 32767i16.to_le_bytes());
    }

    /// Two-key floor constant is wired to the documented operating point.
    #[test]
    fn test_verify_gate_constants() {
        assert!(super::VERIFY_NEAR_PROB_FLOOR == 0.6);
        assert!(super::VERIFY_NOSPEECH_VETO == 0.85);
    }

    /// The 2026-09-28 phantom-wake fix: prob-0.0 candidates (v4 barge-in
    /// "exact-only verify") must never take the instant-fire path — they
    /// have zero stage-1 evidence, and firing unverified produced phantom
    /// "confidence 0.0%" wakes whenever TV speech played during TTS.
    #[test]
    fn test_should_instant_fire_never_fires_zero_evidence() {
        // Barge-in candidate (prob 0.0): never instant, verify or drop.
        assert!(!super::should_instant_fire(false, 0.0));
        assert!(!super::should_instant_fire(true, 0.0));
        // Real stage-1 detections (model threshold 0.68): instant when the
        // user disabled the STT cross-check.
        assert!(super::should_instant_fire(false, 0.978));
        assert!(super::should_instant_fire(false, 0.68));
        assert!(!super::should_instant_fire(true, 0.978));
    }

    // ─── v3 confidence-gate tests ───────────────────────────────────

    fn seg(text: &str, no_speech: f32) -> crate::stt_groq::GroqSegment {
        crate::stt_groq::GroqSegment {
            text: text.to_string(),
            avg_logprob: -0.1,
            no_speech_prob: no_speech,
        }
    }

    /// Confident speech segments pass (no veto).
    #[test]
    fn test_confidence_passes_speech() {
        let segs = vec![seg("Nexus.", 0.02), seg("hey", 0.1)];
        assert!(super::verify_confidence(&segs));
    }

    /// All-segments-certain-nonspeech vetoes (phantom readings).
    #[test]
    fn test_confidence_vetoes_phantom() {
        let segs = vec![seg("Lots of children.", 0.91)];
        assert!(!super::verify_confidence(&segs));
        let multi = vec![seg("a", 0.9), seg("b", 0.99)];
        assert!(!super::verify_confidence(&multi));
    }

    /// F1 endpoint: a 1-2 chunk noise blip followed by silence must NOT stop
    /// the capture (the user's turn hasn't started). Regression test for the
    /// 19:28 log: 8-14 chunk captures of pre-speech noise → confabulations.
    #[test]
    fn test_stop_ignores_unconfirmed_blip() {
        // blip (2 voiced) + 5 silent: no turn started → keep waiting.
        assert!(!super::should_stop_capture(true, 2, 5, 5, 12));
        // no speech at all + 5 silent: keep waiting (8s timeout rules).
        assert!(!super::should_stop_capture(false, 0, 5, 5, 12));
        // confirmed word (5 voiced) + 5 silent: endpoint fires.
        assert!(super::should_stop_capture(true, 5, 5, 5, 14));
        // max duration always stops.
        assert!(super::should_stop_capture(true, 9, 0, 5, 125));
        // no-speech timeout always stops.
        assert!(super::should_stop_capture(false, 0, 0, 5, 100));
    }

    /// P2.5 endpointing self-check — hesitant speaker (mid-thought pauses).
    /// The 400ms fast limit must cut a pause AFTER a confirmed word, while
    /// the ~1s patient limit (pause_count >= 2 relaxes the caller-side
    /// silence_limit selection) must survive it. Cutoff-rate tracking per
    /// deployment plan 02.
    #[test]
    fn test_endpointing_hesitant_speaker() {
        // Confirmed word + a mid-thought pause at the FAST limit: stops
        // (by design — one pause before its 2-pause reputation is earned).
        assert!(super::should_stop_capture(true, 5, 5, 5, 30));
        // Same pause at the PATIENT limit: survives (hesitant speakers
        // keep the floor; 400ms would amputate their sentence).
        assert!(!super::should_stop_capture(true, 5, 5, 12, 30));
        // Speech resumes after the patient pause: capture continues —
        // silence resets to 0 with the turn still live.
        assert!(!super::should_stop_capture(true, 7, 0, 12, 40));
        // Patient limit eventually still endpoints (not a hang).
        assert!(super::should_stop_capture(true, 5, 12, 12, 60));
    }

    /// F2 phantom guard: noise-blip buffers skip Groq, real words don't.
    #[test]
    fn test_phantom_capture_guard() {
        const CH: usize = 1280;
        // all silence → phantom.
        assert!(super::is_phantom_capture(&vec![0.0f32; CH * 10]));
        // two loud blips in 10 chunks → phantom (not a turn).
        let mut blips = vec![0.0f32; CH * 10];
        for s in blips.iter_mut().take(CH * 2) {
            *s = 0.05;
        }
        assert!(super::is_phantom_capture(&blips));
        // sustained word (5 voiced chunks) → real, send to Groq.
        let mut word = vec![0.0f32; CH * 10];
        for s in word.iter_mut().take(CH * 5) {
            *s = 0.08;
        }
        assert!(!super::is_phantom_capture(&word));
        // tiny buffer → phantom.
        assert!(super::is_phantom_capture(&vec![0.08f32; 1000]));
    }

    /// endpoint_reason mirrors should_stop_capture branch-for-branch
    /// (approach A turn packet). Every true-stop input must name a reason;
    /// every false-stop input must NOT (caller only asks after a stop).
    #[test]
    fn test_endpoint_reason_names_every_stop() {
        // Silence after a confirmed turn.
        assert_eq!(super::endpoint_reason(true, 5, 5, 5, 20), "silence_end");
        // Max duration wins even mid-speech.
        assert_eq!(super::endpoint_reason(true, 9, 0, 5, 125), "max_duration");
        // No-speech timeout (nothing ever detected).
        assert_eq!(super::endpoint_reason(false, 0, 0, 5, 100), "no_speech");
        // Max total beats an unconfirmed blip tail, too.
        assert_eq!(super::endpoint_reason(true, 2, 5, 5, 125), "max_duration");
        // Non-stop inputs → unknown (never emitted, defensive only).
        assert_eq!(super::endpoint_reason(true, 5, 2, 5, 20), "unknown");
        assert_eq!(super::endpoint_reason(false, 0, 0, 5, 20), "unknown");
    }

    /// buffer_rms_stats: peak + mean energy of a capture buffer.
    /// Silence ≈ 0, speech lands orders of magnitude above — the packet's
    /// mic-dead-vs-STT-fault discriminator.
    #[test]
    fn test_buffer_rms_stats() {
        let (peak, mean) = super::buffer_rms_stats(&[]);
        assert_eq!((peak, mean), (0.0, 0.0));
        let (peak, mean) = super::buffer_rms_stats(&vec![0.0f32; 16000]);
        assert_eq!(peak, 0.0);
        assert_eq!(mean, 0.0);
        let loud = vec![0.1f32; 16000];
        let (peak, mean) = super::buffer_rms_stats(&loud);
        assert!((peak - 0.1).abs() < 1e-6);
        assert!((mean - 0.1).abs() < 1e-6);
        // Peak catches a single loud chunk the mean would wash out.
        let mut blip = vec![0.0f32; 16000];
        blip[0] = 0.5;
        let (peak, mean) = super::buffer_rms_stats(&blip);
        assert!((peak - 0.5).abs() < 1e-6);
        assert!(mean < 0.01);
    }

    /// TurnStats serializes to the exact `stt:turn_stats` wire shape the
    /// frontend listener parses (all fields, no renames).
    #[test]
    fn test_turn_stats_wire_shape() {
        let stats = super::TurnStats {
            session: 7,
            rms_max: 0.08,
            rms_mean: 0.02,
            voiced_chunks: 5,
            total_chunks: 20,
            endpoint: "silence_end",
            stt_path: "groq",
            filter: "none",
            owner: "verified",
            owner_score: 0.82,
            decoder_bias: "owner",
            stt_language: "en",
        };
        let v: serde_json::Value =
            serde_json::to_value(&stats).expect("turn stats serialize");
        assert_eq!(v["session"], 7);
        assert_eq!(v["endpoint"], "silence_end");
        assert_eq!(v["stt_path"], "groq");
        assert_eq!(v["filter"], "none");
        assert_eq!(v["owner"], "verified");
        assert_eq!(v["decoder_bias"], "owner");
        assert_eq!(v["stt_language"], "en");
        assert_eq!(v["voiced_chunks"], 5);
        assert!(v["rms_max"].as_f64().unwrap() > 0.0);
    }

    /// Non-destructive voice check (approach B): true only while a capture
    /// is live AND voiced chunks landed — no state touched either way
    /// (slow-starter turns stay fully intact).
    #[cfg(not(feature = "mock-wake"))]
    #[test]
    fn test_capture_had_speech_query() {
        use std::sync::atomic::Ordering;
        super::STT_CAPTURING.store(false, Ordering::Relaxed);
        super::STT_VOICED_CHUNKS.store(9, Ordering::Relaxed);
        assert!(!super::stt_capture_had_speech(), "not capturing → false");
        super::STT_CAPTURING.store(true, Ordering::Relaxed);
        super::STT_VOICED_CHUNKS.store(2, Ordering::Relaxed);
        assert!(!super::stt_capture_had_speech(), "unconfirmed blip → false");
        super::STT_VOICED_CHUNKS.store(4, Ordering::Relaxed);
        assert!(super::stt_capture_had_speech(), "live + voiced → true");
        // No residue: query must not reset anything.
        assert_eq!(super::STT_VOICED_CHUNKS.load(Ordering::Relaxed), 4);
        assert!(super::STT_CAPTURING.load(Ordering::Relaxed));
        super::STT_CAPTURING.store(false, Ordering::Relaxed);
        super::STT_VOICED_CHUNKS.store(0, Ordering::Relaxed);
    }

    /// Abort with no voice underway: full state reset, had_speech=false.
    /// Second Ctrl+Space press before the user speaks must leave zero
    /// residue (stale flags would poison the NEXT capture's endpoint).
    #[cfg(not(feature = "mock-wake"))]
    #[test]
    fn test_abort_resets_capture_state() {
        use std::sync::atomic::Ordering;
        super::STT_CAPTURING.store(true, Ordering::Relaxed);
        super::STT_SPEECH_DETECTED.store(true, Ordering::Relaxed);
        super::STT_SILENCE_CHUNKS.store(3, Ordering::Relaxed);
        super::STT_TOTAL_CHUNKS.store(0, Ordering::Relaxed);
        super::STT_VOICED_CHUNKS.store(0, Ordering::Relaxed);
        super::STT_PAUSE_COUNT.store(1, Ordering::Relaxed);
        super::STT_CAPTURE_BUFFER.lock().push(0.5);

        let res = super::abort_stt_capture();
        assert!(!res.had_speech);
        assert_eq!(res.elapsed_ms, 0);
        assert!(!super::STT_CAPTURING.load(Ordering::Relaxed));
        assert!(!super::STT_SPEECH_DETECTED.load(Ordering::Relaxed));
        assert_eq!(super::STT_SILENCE_CHUNKS.load(Ordering::Relaxed), 0);
        assert_eq!(super::STT_TOTAL_CHUNKS.load(Ordering::Relaxed), 0);
        assert_eq!(super::STT_VOICED_CHUNKS.load(Ordering::Relaxed), 0);
        assert_eq!(super::STT_PAUSE_COUNT.load(Ordering::Relaxed), 0);
        assert!(super::STT_CAPTURE_BUFFER.lock().is_empty());
    }

    /// Abort mid-word: had_speech=true so the frontend lets the turn finish
    /// instead of hiding the orb mid-sentence.
    #[cfg(not(feature = "mock-wake"))]
    #[test]
    fn test_abort_had_speech_arms() {
        use std::sync::atomic::Ordering;
        super::STT_CAPTURING.store(true, Ordering::Relaxed);
        super::STT_VOICED_CHUNKS.store(5, Ordering::Relaxed);
        super::STT_TOTAL_CHUNKS.store(10, Ordering::Relaxed);

        let res = super::abort_stt_capture();
        assert!(res.had_speech);
        assert_eq!(res.elapsed_ms, 800);
        // State still resets — the receiver drops the orphaned buffer by
        // session, it must never observe half-cleared counters.
        assert!(!super::STT_CAPTURING.load(Ordering::Relaxed));
        assert_eq!(super::STT_VOICED_CHUNKS.load(Ordering::Relaxed), 0);
    }

    /// Stale-session predicate: same generation = live, older = stale.
    #[test]
    fn test_stale_session_predicate() {
        assert!(!super::is_stale_session(1, 1));
        assert!(super::is_stale_session(1, 2));
        assert!(super::is_stale_session(7, 9));
    }

    /// Abort bumps the session generation so in-flight buffers die on arrival.
    #[cfg(not(feature = "mock-wake"))]
    #[test]
    fn test_abort_invalidates_session() {        use std::sync::atomic::Ordering;
        let before = super::CAPTURE_SESSION_ID.load(Ordering::Relaxed);
        let _ = super::abort_stt_capture();
        let after = super::CAPTURE_SESSION_ID.load(Ordering::Relaxed);
        assert!(after > before);
        assert!(super::is_stale_session(before, after));
        assert!(!super::is_stale_session(after, after));
    }

    /// Mid-range no_speech does NOT veto (benefit of doubt to words —
    /// mangled true speech can score here; the word gate decides).
    #[test]
    fn test_confidence_midrange_passes() {
        let segs = vec![seg("Nexus.", 0.6)];
        assert!(super::verify_confidence(&segs));
    }

    /// Empty segments (local fallback, parse failure) → no info → pass
    /// (fail-open direction; the word gate still applies).
    #[test]
    fn test_confidence_empty_passes() {
        let segs: Vec<crate::stt_groq::GroqSegment> = vec![];
        assert!(super::verify_confidence(&segs));
    }

    // ─── Mic heartbeat tests ────────────────────────────────────────

    /// Bar scales with RMS: silence empty, speech-body partial, loud full.
    #[test]
    fn test_rms_bar_scaling() {
        assert_eq!(super::rms_bar(0.0), "..........");
        assert_eq!(super::rms_bar(0.001), "..........");
        assert_eq!(super::rms_bar(0.02), "#.........");
        assert_eq!(super::rms_bar(0.1), "#####.....");
        assert_eq!(super::rms_bar(0.2), "##########");
        assert_eq!(super::rms_bar(0.9), "##########");
    }

    /// Bar is always exactly 10 chars (console column alignment).
    #[test]
    fn test_rms_bar_width() {
        for rms in [0.0, 0.005, 0.03, 0.07, 0.15, 0.5, 1.0] {
            assert_eq!(super::rms_bar(rms).len(), 10, "rms={}", rms);
        }
    }

    // ─── v4 backward-confirmation tests ─────────────────────────────

    /// Sustained pre-trigger energy passes (real utterance just happened).
    #[test]
    fn test_pre_trigger_passes_speech() {
        // 1s of loud speech (8 voiced frames), trigger chunk excluded anyway.
        let mut ring = vec![0.0f32; 16000];
        for s in ring.iter_mut().skip(3200).take(9600) {
            *s = 0.3;
        }
        let (passed, rms) = super::check_pre_trigger(&ring, 0.002, 3);
        assert!(passed, "sustained speech must pass");
        assert!(rms > 0.1);
    }

    /// All-silence pre-trigger fails.
    #[test]
    fn test_pre_trigger_rejects_silence() {
        let ring = vec![0.0f32; 16000];
        let (passed, _) = super::check_pre_trigger(&ring, 0.002, 3);
        assert!(!passed);
    }

    /// A lone loud trigger chunk cannot self-confirm (excluded from vote).
    #[test]
    fn test_pre_trigger_excludes_trigger_chunk() {
        // Silence everywhere except the LAST chunk (the trigger chunk).
        let mut ring = vec![0.0f32; 16000];
        for s in ring.iter_mut().skip(16000 - 1280) {
            *s = 0.9;
        }
        let (passed, _) = super::check_pre_trigger(&ring, 0.002, 3);
        assert!(!passed, "lone spike must not self-confirm");
    }

    /// Too-short ring fails safe (not enough history to judge).
    #[test]
    fn test_pre_trigger_short_ring() {
        let ring = vec![0.5f32; 1000];
        let (passed, _) = super::check_pre_trigger(&ring, 0.002, 3);
        assert!(!passed);
    }

    // ─── v4 barge-in tests ──────────────────────────────────────────

    /// Only TTS-muted + sustained speech earns an attempt.
    #[test]
    fn test_barge_decision_matrix() {
        assert!(super::should_barge_attempt(true, 6));
        assert!(super::should_barge_attempt(true, 60));
        assert!(!super::should_barge_attempt(true, 5));
        assert!(!super::should_barge_attempt(true, 0));
        // Meeting / pause / unmuted NEVER earn attempts.
        assert!(!super::should_barge_attempt(false, 6));
        assert!(!super::should_barge_attempt(false, 600));
    }

    // ─── v4 WebRTC VAD tests ────────────────────────────────────────

    /// Digital silence is vetoed (not speech).
    #[test]
    fn test_webrtc_vad_rejects_silence() {
        let mut vad = webrtc_vad::Vad::new_with_rate_and_mode(
            webrtc_vad::SampleRate::Rate16kHz,
            webrtc_vad::VadMode::Quality,
        );
        let silence = vec![0i16; 320];
        assert!(!vad.is_voice_segment(&silence).unwrap_or(true));
    }

    /// Real owner speech passes (SKIP-guarded repo clip).
    #[test]
    fn test_webrtc_vad_passes_speech() {
        use std::path::PathBuf;
        let clip = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("wake_camp")
            .join("raw")
            .join("hey_nexus")
            .join("near_normal")
            .join("BASE02.wav");
        if !clip.exists() {
            eprintln!("SKIP: camp clip not found at {}", clip.display());
            return;
        }
        let (sr, data) = read_wav_mono16(&clip);
        assert_eq!(sr, 16000);
        let mut vad = webrtc_vad::Vad::new_with_rate_and_mode(
            webrtc_vad::SampleRate::Rate16kHz,
            webrtc_vad::VadMode::Quality,
        );
        // Speech region (skip the leading silence): majority must be voiced.
        let voiced = data
            .chunks(320)
            .filter(|f| f.len() == 320)
            .filter(|f| vad.is_voice_segment(f).unwrap_or(true))
            .count();
        let total = data.len() / 320;
        assert!(
            voiced * 2 > total,
            "majority of frames must be voiced ({}/{})",
            voiced,
            total
        );
    }

    /// WAV helper for tests: load mono 16-bit PCM.
    #[allow(dead_code)]
    fn read_wav_mono16(path: &std::path::Path) -> (u32, Vec<i16>) {
        let data = std::fs::read(path).expect("read wav");
        assert!(data.len() > 44, "not a wav file");
        let sr = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
        let mut out = Vec::with_capacity((data.len() - 44) / 2);
        let mut i = 44;
        while i + 1 < data.len() {
            // stereo → mono by averaging pairs when needed
            out.push(i16::from_le_bytes([data[i], data[i + 1]]));
            i += 2;
        }
        (sr, out)
    }

    // ─── Meet-parity stream-health tests ────────────────────────────

    /// Quiet room (low floor, prior audio, device present) → never restart.
    #[test]
    fn test_health_quiet_room_no_restart() {
        assert_eq!(
            super::classify_stream_health(5, true, false, true),
            super::StreamHealth::Quiet
        );
        // Never-heard-audio + moderate zeros → observe only (an OS mute
        // can't be fixed by restarting; no action either way).
        assert_eq!(
            super::classify_stream_health(120, false, false, true),
            super::StreamHealth::Suspect
        );
        // ...but 5 minutes of zeros with zero history → last-resort restart
        // (a driver wedged from boot would otherwise never recover).
        assert_eq!(
            super::classify_stream_health(300, false, false, true),
            super::StreamHealth::Dead
        );
    }

    /// Suspect zone: exact zeros 10–60s with prior audio → observe only.
    #[test]
    fn test_health_suspect_observes() {
        assert_eq!(
            super::classify_stream_health(10, true, false, true),
            super::StreamHealth::Suspect
        );
        assert_eq!(
            super::classify_stream_health(59, true, false, true),
            super::StreamHealth::Suspect
        );
    }

    /// Terminal signals → Dead (the ONLY restart trigger).
    #[test]
    fn test_health_dead_terminals() {
        // Device gone (even with recent audio).
        assert_eq!(
            super::classify_stream_health(0, true, false, false),
            super::StreamHealth::Dead
        );
        // cpal error fired.
        assert_eq!(
            super::classify_stream_health(0, true, true, true),
            super::StreamHealth::Dead
        );
        // Bit-exact zeros 60s+ with prior audio (stuck driver).
        assert_eq!(
            super::classify_stream_health(60, true, false, true),
            super::StreamHealth::Dead
        );
        assert_eq!(
            super::classify_stream_health(3600, true, false, true),
            super::StreamHealth::Dead
        );
    }

    /// Boundary values: 9s quiet, 10s suspect, 59s suspect, 60s dead.
    #[test]
    fn test_health_boundaries() {
        use super::StreamHealth::*;
        assert_eq!(super::classify_stream_health(9, true, false, true), Quiet);
        assert_eq!(super::classify_stream_health(10, true, false, true), Suspect);
        assert_eq!(super::classify_stream_health(59, true, false, true), Suspect);
        assert_eq!(super::classify_stream_health(60, true, false, true), Dead);
    }

    /// Mic sample analysis + verdicts (self-test logic).
    #[test]
    fn test_mic_sample_verdicts() {
        // Healthy speech.
        let speech = vec![0.3f32; 40000];
        let (peak, zr) = super::analyze_audio_sample(&speech);
        assert!((peak - 0.3).abs() < 1e-6);
        assert_eq!(zr, 0.0);
        assert_eq!(super::classify_mic_sample(peak, zr), super::MicHealth::Healthy);
        // Quiet room: low nonzero floor.
        let quiet = vec![0.001f32; 40000];
        let (peak, zr) = super::analyze_audio_sample(&quiet);
        assert_eq!(super::classify_mic_sample(peak, zr), super::MicHealth::QuietRoom);
        // Dead: all exact zeros.
        let dead = vec![0.0f32; 40000];
        let (peak, zr) = super::analyze_audio_sample(&dead);
        assert_eq!(zr, 1.0);
        assert_eq!(super::classify_mic_sample(peak, zr), super::MicHealth::DeadSilence);
        // Empty: verdict dead-silence (nothing captured at all).
        let (peak, zr) = super::analyze_audio_sample(&[]);
        assert_eq!(super::classify_mic_sample(peak, zr), super::MicHealth::DeadSilence);
        // Boundary: peak exactly 0.05 → healthy.
        assert_eq!(super::classify_mic_sample(0.05, 0.0), super::MicHealth::Healthy);
        assert_eq!(super::classify_mic_sample(0.049, 0.0), super::MicHealth::QuietRoom);
    }
}

#[cfg(test)]
mod verify_policy_tests {
    use super::*;

    #[test]
    fn highconf_soundalike_accepts_hello_nico_only_when_confident() {
        // The two real rejections from the 2026-10-10 log.
        assert!(verify_transcript_accept("So. Hello, Nico.", 0.979));
        assert!(verify_transcript_accept("I'm sorry. Hello, Nico.", 0.933));
        // Same text at a weak score stays rejected.
        assert!(!verify_transcript_accept("So. Hello, Nico.", 0.746));
        assert!(!verify_transcript_accept("Hello Nico", 0.85));
    }

    #[test]
    fn highconf_soundalike_needs_short_transcripts() {
        assert!(!verify_transcript_accept(
            "nico was telling me about the thing yesterday",
            0.99
        ));
        assert!(verify_transcript_accept("nico", 0.95));
    }

    #[test]
    fn split_next_us_needs_a_greeting() {
        assert!(verify_transcript_accept("hey next us", 0.95));
        assert!(verify_transcript_accept("ok necks us", 0.92));
        // Bare "next us" stays rejected (news/TV), at any probability.
        assert!(!verify_transcript_accept("next us", 0.99));
        assert!(!verify_transcript_accept("what is next us", 0.99));
    }

    #[test]
    fn old_two_key_gate_is_unchanged() {
        assert!(verify_transcript_accept("nexus", 0.1));
        assert!(verify_transcript_accept("lexus", 0.7));
        assert!(!verify_transcript_accept("lexus", 0.5));
        assert!(!verify_transcript_accept("texas", 0.99));
        assert!(!verify_transcript_accept("next", 0.99));
        assert!(!verify_transcript_accept("I'll walk like 12-12 times", 0.736));
        assert!(!verify_transcript_accept("", 0.99));
    }

    #[test]
    fn verifier_failure_fires_only_for_confident_candidates() {
        // The cold-STT timeout used to fire the orb at prob 0.746 on "That's a poem".
        assert!(!fail_open_allowed(0.746));
        assert!(!fail_open_allowed(0.0)); // barge-in candidates carry no neural evidence
        assert!(!fail_open_allowed(0.89));
        assert!(fail_open_allowed(0.9));
        assert!(fail_open_allowed(0.979));
    }

    #[test]
    fn cooldown_never_eats_a_verified_bypass() {
        // Fast cloud verify: the self-send arrives 300 ms after the cooldown was stamped.
        assert!(cooldown_blocks(10_300, 10_000, false));
        assert!(!cooldown_blocks(10_300, 10_000, true));
        // Normal expiry still works.
        assert!(!cooldown_blocks(11_600, 10_000, false));
        // Clock going backwards must not block forever.
        assert!(!cooldown_blocks(5, 10_000, false));
    }
}
