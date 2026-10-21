//! Smart Turn v3.2 end-of-turn detector (pipecat-ai/smart-turn, BSD-2-Clause).
//!
//! STATUS: standalone + tested, NOT wired into the capture path. The energy
//! endpointer in `wakeword_oww.rs` is untouched. Wire-in is gated on a
//! real-speech evaluation (see docs/research/jarvis-landscape/03-smart-turn-prototype-2026-10-04.md).
//!
//! Pipeline (mirrors the reference `inference.py`):
//!   last ≤8 s of 16 kHz mono f32 → zero-mean/unit-var over the real samples →
//!   right-pad zeros to 8 s → Whisper log-mel (80 × 800) → ONNX → P(turn complete).
//! The model already outputs a probability; ≥ 0.5 means "complete".

use std::path::Path;

pub const SAMPLE_RATE: usize = 16_000;
pub const WINDOW_SAMPLES: usize = 8 * SAMPLE_RATE;
const N_FFT: usize = 400;
const HOP: usize = 160;
const N_FREQ: usize = N_FFT / 2 + 1; // 201
pub const N_MELS: usize = 80;
pub const N_FRAMES: usize = WINDOW_SAMPLES / HOP; // 800 (801 STFT frames, last dropped)

// ── Slaney mel scale (matches HF `mel_filter_bank(norm="slaney", mel_scale="slaney")`) ──
fn hz_to_mel(f: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    const MIN_LOG_HZ: f64 = 1000.0;
    let min_log_mel = MIN_LOG_HZ / F_SP;
    let logstep = (6.4f64).ln() / 27.0;
    if f >= MIN_LOG_HZ {
        min_log_mel + (f / MIN_LOG_HZ).ln() / logstep
    } else {
        f / F_SP
    }
}

fn mel_to_hz(m: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    const MIN_LOG_HZ: f64 = 1000.0;
    let min_log_mel = MIN_LOG_HZ / F_SP;
    let logstep = (6.4f64).ln() / 27.0;
    if m >= min_log_mel {
        MIN_LOG_HZ * (logstep * (m - min_log_mel)).exp()
    } else {
        F_SP * m
    }
}

/// `[N_MELS][N_FREQ]` triangular filterbank, Slaney-normalised, 0–8 kHz.
fn mel_filters() -> Vec<[f32; N_FREQ]> {
    let fft_freqs: Vec<f64> = (0..N_FREQ)
        .map(|i| i as f64 * (SAMPLE_RATE as f64 / 2.0) / (N_FREQ - 1) as f64)
        .collect();
    let (mel_min, mel_max) = (hz_to_mel(0.0), hz_to_mel(SAMPLE_RATE as f64 / 2.0));
    let filter_freqs: Vec<f64> = (0..N_MELS + 2)
        .map(|i| mel_to_hz(mel_min + (mel_max - mel_min) * i as f64 / (N_MELS + 1) as f64))
        .collect();
    let diff: Vec<f64> = filter_freqs.windows(2).map(|w| w[1] - w[0]).collect();
    (0..N_MELS)
        .map(|m| {
            let enorm = 2.0 / (filter_freqs[m + 2] - filter_freqs[m]);
            let mut row = [0f32; N_FREQ];
            for (k, &f) in fft_freqs.iter().enumerate() {
                let down = -(filter_freqs[m] - f) / diff[m];
                let up = (filter_freqs[m + 2] - f) / diff[m + 1];
                row[k] = (down.min(up).max(0.0) * enorm) as f32;
            }
            row
        })
        .collect()
}

struct Tables {
    window: Vec<f64>,
    cos_t: Vec<f64>,
    sin_t: Vec<f64>,
    filters: Vec<[f32; N_FREQ]>,
}

fn tables() -> &'static Tables {
    static T: std::sync::OnceLock<Tables> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let window: Vec<f64> = (0..N_FFT)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / N_FFT as f64).cos())
            .collect();
        let (mut cos_t, mut sin_t) = (vec![0f64; N_FREQ * N_FFT], vec![0f64; N_FREQ * N_FFT]);
        for k in 0..N_FREQ {
            for t in 0..N_FFT {
                let a = 2.0 * std::f64::consts::PI * ((k * t) % N_FFT) as f64 / N_FFT as f64;
                cos_t[k * N_FFT + t] = a.cos();
                sin_t[k * N_FFT + t] = a.sin();
            }
        }
        Tables { window, cos_t, sin_t, filters: mel_filters() }
    })
}

/// Whisper-style log-mel for up to 8 s of 16 kHz audio → row-major `[80 × 800]`.
/// Applies the reference normalisation (over the real samples only) and right-pads with zeros.
pub fn log_mel(samples: &[f32]) -> Vec<f32> {
    // Keep the last 8 s; normalise over the real samples; right-pad zeros.
    let s = if samples.len() > WINDOW_SAMPLES {
        &samples[samples.len() - WINDOW_SAMPLES..]
    } else {
        samples
    };
    let n = s.len().max(1) as f64;
    let mean = s.iter().map(|&x| x as f64).sum::<f64>() / n;
    let var = s.iter().map(|&x| (x as f64 - mean).powi(2)).sum::<f64>() / n;
    let inv = 1.0 / (var + 1e-7).sqrt();
    let mut wave = vec![0f32; WINDOW_SAMPLES];
    for (d, &x) in wave.iter_mut().zip(s.iter()) {
        *d = ((x as f64 - mean) * inv) as f32;
    }

    // Reflect-pad N_FFT/2 each side (STFT center=True).
    let pad = N_FFT / 2;
    let mut padded = Vec::with_capacity(WINDOW_SAMPLES + 2 * pad);
    for i in (1..=pad).rev() {
        padded.push(wave[i]);
    }
    padded.extend_from_slice(&wave);
    for i in 1..=pad {
        padded.push(wave[WINDOW_SAMPLES - 1 - i]);
    }

    let Tables { window, cos_t, sin_t, filters } = tables();

    let mut mel = vec![0f32; N_MELS * N_FRAMES];
    let mut power = [0f64; N_FREQ];
    let mut frame = [0f64; N_FFT];
    for f in 0..N_FRAMES {
        let off = f * HOP;
        for t in 0..N_FFT {
            frame[t] = padded[off + t] as f64 * window[t];
        }
        for k in 0..N_FREQ {
            let (c, sn) = (&cos_t[k * N_FFT..(k + 1) * N_FFT], &sin_t[k * N_FFT..(k + 1) * N_FFT]);
            let (mut re, mut im) = (0f64, 0f64);
            for t in 0..N_FFT {
                re += frame[t] * c[t];
                im -= frame[t] * sn[t];
            }
            power[k] = re * re + im * im;
        }
        for m in 0..N_MELS {
            let mut acc = 0f64;
            for k in 0..N_FREQ {
                acc += filters[m][k] as f64 * power[k];
            }
            mel[m * N_FRAMES + f] = acc.max(1e-10).log10() as f32;
        }
    }

    // Whisper dynamic-range clamp + scale.
    let max = mel.iter().cloned().fold(f32::MIN, f32::max);
    for v in mel.iter_mut() {
        *v = (v.max(max - 8.0) + 4.0) / 4.0;
    }
    mel
}

/// Loaded Smart Turn model (ONNX Runtime — tract 0.23 cannot analyse the quantized Conv1d).
/// ~tens of ms per call on CPU: NOT for the audio callback — run from a worker thread
/// once trailing silence has persisted.
pub struct TurnDetector {
    session: std::sync::Mutex<ort::session::Session>,
}

impl TurnDetector {
    pub fn load(path: &Path) -> Result<Self, String> {
        let session = ort::session::Session::builder()
            .map_err(|e| format!("smart-turn: session builder: {e}"))?
            .commit_from_file(path)
            .map_err(|e| format!("smart-turn: load failed: {e}"))?;
        Ok(Self { session: std::sync::Mutex::new(session) })
    }

    /// P(turn complete) in [0,1] for the most recent ≤8 s of 16 kHz mono audio.
    pub fn predict(&self, samples: &[f32]) -> Result<f32, String> {
        let mel = log_mel(samples);
        let input = ort::value::Tensor::<f32>::from_array(([1usize, N_MELS, N_FRAMES], mel.into_boxed_slice()))
            .map_err(|e| format!("smart-turn: tensor: {e}"))?;
        let mut session = self.session.lock().map_err(|_| "smart-turn: poisoned".to_string())?;
        let outputs = session
            .run(ort::inputs!["input_features" => input])
            .map_err(|e| format!("smart-turn: run failed: {e}"))?;
        let (_, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("smart-turn: extract: {e}"))?;
        data.first().copied().ok_or_else(|| "smart-turn: empty output".to_string())
    }
}

/// Pure endpoint policy: combine the model with the existing silence counter.
/// `silence_chunks` are 80 ms chunks of trailing silence (same unit as `STT_SILENCE_CHUNKS`).
/// * Below `min_silence_chunks` never ask the model (avoid cutting mid-word).
/// * At/above `hard_limit_chunks` always stop (fallback = today's behaviour).
/// * In between, stop only when the model says the turn is complete.
pub fn should_end_turn(
    silence_chunks: u32,
    min_silence_chunks: u32,
    hard_limit_chunks: u32,
    prob_complete: Option<f32>,
    threshold: f32,
) -> bool {
    if silence_chunks >= hard_limit_chunks {
        return true;
    }
    if silence_chunks < min_silence_chunks {
        return false;
    }
    matches!(prob_complete, Some(p) if p >= threshold)
}

/// How eagerly the assistant ends the user's turn (mirrors the OpenAI Realtime `semantic_vad`
/// `eagerness` concept: low = let the user take their time, high = answer as soon as possible).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eagerness {
    Low,
    Medium,
    High,
}

impl Eagerness {
    /// Unknown / empty strings fall back to `Medium` (never fail on a typo in settings.json).
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" => Eagerness::Low,
            "high" => Eagerness::High,
            _ => Eagerness::Medium,
        }
    }
}

/// Smart-Turn endpoint policy in 80 ms capture chunks. Smart Turn can only end a turn EARLIER than
/// the existing silence rule (never before `min_silence_chunks`) or hold it open LONGER when the
/// model says the speaker is not finished (`patient_chunks`). All values are conservative defaults
/// pending the real-speech evaluation in docs/research/jarvis-landscape/03-smart-turn-prototype-2026-10-04.md.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TurnPolicy {
    pub min_silence_chunks: u32,
    pub threshold: f32,
    pub patient_chunks: u32,
}

pub fn policy_for(e: Eagerness) -> TurnPolicy {
    match e {
        // 240 ms earliest stop, permissive threshold, never extends the 800 ms rule.
        Eagerness::High => TurnPolicy { min_silence_chunks: 3, threshold: 0.4, patient_chunks: 10 },
        // 400 ms earliest stop; holds open to ~1.2 s when the model says "not done".
        Eagerness::Medium => TurnPolicy { min_silence_chunks: 5, threshold: 0.5, patient_chunks: 15 },
        // 560 ms earliest stop, needs high confidence; holds open to ~1.8 s.
        Eagerness::Low => TurnPolicy { min_silence_chunks: 7, threshold: 0.7, patient_chunks: 22 },
    }
}

/// End the turn now? (model verdict for the CURRENT silence run + enough trailing silence)
pub fn smart_end(silence_chunks: u32, pol: &TurnPolicy, prob_complete: Option<f32>) -> bool {
    silence_chunks >= pol.min_silence_chunks
        && matches!(prob_complete, Some(p) if p >= pol.threshold)
}

/// Silence limit to use: if the model says the speaker is NOT finished, hold the turn open to at
/// least `patient_chunks`; otherwise leave the existing limit untouched.
pub fn extended_limit(base_limit: u32, pol: &TurnPolicy, prob_complete: Option<f32>) -> u32 {
    match prob_complete {
        Some(p) if p < pol.threshold => base_limit.max(pol.patient_chunks),
        _ => base_limit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic wave also generated by the Python reference (HF WhisperFeatureExtractor).
    fn synth() -> Vec<f32> {
        (0..40_000)
            .map(|i| {
                let t = i as f64;
                let a = 0.3 * (2.0 * std::f64::consts::PI * 440.0 * t / 16000.0).sin();
                let b = 0.2
                    * (2.0 * std::f64::consts::PI * 1234.5 * t / 16000.0).sin()
                    * (1.0 + (2.0 * std::f64::consts::PI * 3.0 * t / 16000.0).sin())
                    * 0.5;
                (a + b) as f32
            })
            .collect()
    }

    fn at(m: &[f32], mel: usize, frame: usize) -> f32 {
        m[mel * N_FRAMES + frame]
    }

    #[test]
    fn log_mel_matches_hf_reference() {
        let m = log_mel(&synth());
        assert_eq!(m.len(), N_MELS * N_FRAMES);
        let tol = 2e-3;
        for &(mel, fr, want) in &[
            (0usize, 0usize, 1.215064f32),
            (12, 120, 1.501683),
            (25, 199, -0.249969),
            (30, 225, 0.016987),
            (60, 249, -0.167714),
            (79, 250, 0.203149),
            (10, 251, 0.573238),
            (5, 10, -0.353635),
        ] {
            let got = at(&m, mel, fr);
            assert!((got - want).abs() < tol, "({mel},{fr}) got {got} want {want}");
        }
        let early: f64 = (0..N_MELS)
            .flat_map(|mm| (0..250).map(move |f| (mm, f)))
            .map(|(mm, f)| at(&m, mm, f) as f64)
            .sum();
        assert!((early - (-2728.546585)).abs() < 2.0, "early sum {early}");
        let all: f64 = m.iter().map(|&x| x as f64).sum();
        assert!((all - (-18163.902603)).abs() < 5.0, "total sum {all}");
    }

    /// Sanity + latency. NOTE: probabilities are NOT pinned to a reference value — this quantized
    /// model is unstable on out-of-distribution input (a pure tone gave 0.118 in Python onnxruntime
    /// vs 0.171 in Rust `ort` from byte-identical features). On real speech clips the two runtimes
    /// agreed to ~0.01 (see docs/research/jarvis-landscape/03-smart-turn-prototype-2026-10-04.md).
    /// Feature parity is pinned by `log_mel_matches_hf_reference` instead.
    #[test]
    fn model_loads_and_runs() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources/smart_turn/smart-turn-v3.2-cpu.onnx");
        let det = TurnDetector::load(&path).expect("model must load");
        let wave = synth();
        let t0 = std::time::Instant::now();
        let p = det.predict(&wave).unwrap();
        let ms = t0.elapsed().as_millis();
        println!("smart-turn predict: {ms} ms, p={p}");
        assert!(p.is_finite() && (0.0..=1.0).contains(&p), "prob out of range: {p}");
        assert!(ms < 1500, "inference too slow for an endpoint check: {ms} ms");
    }

    #[test]
    fn policy_table() {
        // never ask before the minimum silence
        assert!(!should_end_turn(1, 3, 15, Some(0.99), 0.5));
        // model says done after minimum silence
        assert!(should_end_turn(3, 3, 15, Some(0.9), 0.5));
        // model says "still talking" → keep waiting
        assert!(!should_end_turn(8, 3, 15, Some(0.1), 0.5));
        // model unavailable → wait for hard limit (today's behaviour)
        assert!(!should_end_turn(8, 3, 15, None, 0.5));
        assert!(should_end_turn(15, 3, 15, None, 0.5));
    }

    #[test]
    fn eagerness_parse_and_policies() {
        assert_eq!(Eagerness::parse("LOW"), Eagerness::Low);
        assert_eq!(Eagerness::parse(" high "), Eagerness::High);
        assert_eq!(Eagerness::parse("medium"), Eagerness::Medium);
        assert_eq!(Eagerness::parse("banana"), Eagerness::Medium);
        assert_eq!(Eagerness::parse(""), Eagerness::Medium);
        let (l, m, h) = (
            policy_for(Eagerness::Low),
            policy_for(Eagerness::Medium),
            policy_for(Eagerness::High),
        );
        // eager ends sooner, patient waits longer: strictly ordered
        assert!(h.min_silence_chunks < m.min_silence_chunks && m.min_silence_chunks < l.min_silence_chunks);
        assert!(h.threshold < m.threshold && m.threshold < l.threshold);
        assert!(h.patient_chunks <= m.patient_chunks && m.patient_chunks < l.patient_chunks);
        // nothing may end a turn before 240 ms of silence
        assert!(h.min_silence_chunks >= 3);
    }

    #[test]
    fn smart_end_and_extension_table() {
        let m = policy_for(Eagerness::Medium); // min 5, thr 0.5, patient 15
        // too little silence: never ends, whatever the model says
        assert!(!smart_end(4, &m, Some(0.99)));
        // enough silence + model confident: end early
        assert!(smart_end(5, &m, Some(0.5)));
        // enough silence but model says not done: keep listening
        assert!(!smart_end(9, &m, Some(0.49)));
        // no verdict (model missing / still computing): never ends via Smart Turn
        assert!(!smart_end(9, &m, None));
        // extension only when the model says "not done"
        assert_eq!(extended_limit(10, &m, Some(0.1)), 15);
        assert_eq!(extended_limit(10, &m, Some(0.9)), 10);
        assert_eq!(extended_limit(10, &m, None), 10);
        // an already-patient base is never shortened
        assert_eq!(extended_limit(20, &m, Some(0.1)), 20);
        // high eagerness never extends the standard 800 ms rule
        assert_eq!(extended_limit(10, &policy_for(Eagerness::High), Some(0.0)), 10);
    }
}
