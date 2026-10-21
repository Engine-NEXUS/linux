//! Neural voice-activity detection (Silero VAD v4, MIT) for the STT capture loop.
//!
//! Design/model source: cjpais/Handy (MIT) `src-tauri/src/audio_toolkit/vad/silero.rs`
//! and the `silero_vad_v4.onnx` model it ships (snakers4/silero-vad, MIT). Handy drives the model
//! through the `vad-rs` git dependency; here it runs directly on `ort` (already a pinned
//! dependency) so no extra crate/version is introduced. See THIRD_PARTY_NOTICES.
//!
//! The model consumes 512-sample frames (32 ms @ 16 kHz) with LSTM state `h`/`c`; the capture
//! loop delivers 1280-sample (80 ms) chunks, so leftover samples are carried between calls.
//! Used only when `"vadSilero": true` in settings.json (default off); the RMS gate remains the
//! fallback whenever the model is missing or errors.

use std::path::Path;

pub const SAMPLE_RATE: i64 = 16_000;
pub const FRAME: usize = 512;
const STATE_LEN: usize = 2 * 64;

pub struct SileroVad {
    session: ort::session::Session,
    h: Vec<f32>,
    c: Vec<f32>,
    carry: Vec<f32>,
    last_prob: f32,
}

impl SileroVad {
    pub fn load(path: &Path) -> Result<Self, String> {
        let session = ort::session::Session::builder()
            .map_err(|e| format!("vad: session builder: {e}"))?
            .commit_from_file(path)
            .map_err(|e| format!("vad: load {}: {e}", path.display()))?;
        Ok(Self {
            session,
            h: vec![0.0; STATE_LEN],
            c: vec![0.0; STATE_LEN],
            carry: Vec::with_capacity(FRAME),
            last_prob: 0.0,
        })
    }

    /// Clear LSTM state + carry so a new capture session doesn't inherit context.
    pub fn reset(&mut self) {
        self.h.iter_mut().for_each(|x| *x = 0.0);
        self.c.iter_mut().for_each(|x| *x = 0.0);
        self.carry.clear();
        self.last_prob = 0.0;
    }

    fn infer_frame(&mut self, frame: &[f32]) -> Result<f32, String> {
        use ort::value::Tensor;
        let input = Tensor::<f32>::from_array(([1usize, FRAME], frame.to_vec().into_boxed_slice()))
            .map_err(|e| format!("vad: input tensor: {e}"))?;
        let sr = Tensor::<i64>::from_array(([0usize; 0], vec![SAMPLE_RATE].into_boxed_slice()))
            .map_err(|e| format!("vad: sr tensor: {e}"))?;
        let h = Tensor::<f32>::from_array(([2usize, 1, 64], self.h.clone().into_boxed_slice()))
            .map_err(|e| format!("vad: h tensor: {e}"))?;
        let c = Tensor::<f32>::from_array(([2usize, 1, 64], self.c.clone().into_boxed_slice()))
            .map_err(|e| format!("vad: c tensor: {e}"))?;
        let outputs = self
            .session
            .run(ort::inputs!["input" => input, "sr" => sr, "h" => h, "c" => c])
            .map_err(|e| format!("vad: run: {e}"))?;
        let (_, prob) = outputs["output"]
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("vad: output: {e}"))?;
        let p = *prob.first().ok_or("vad: empty output")?;
        let (_, hn) = outputs["hn"].try_extract_tensor::<f32>().map_err(|e| format!("vad: hn: {e}"))?;
        let (_, cn) = outputs["cn"].try_extract_tensor::<f32>().map_err(|e| format!("vad: cn: {e}"))?;
        if hn.len() == STATE_LEN && cn.len() == STATE_LEN {
            self.h.copy_from_slice(hn);
            self.c.copy_from_slice(cn);
        }
        Ok(p)
    }

    /// Feed any number of 16 kHz mono samples. Returns the **max** speech probability over the
    /// complete 512-frames consumed by this call; if no complete frame was available, returns the
    /// previous call's value (so an 80 ms chunk is always scored).
    pub fn push(&mut self, samples: &[f32]) -> Result<f32, String> {
        self.carry.extend_from_slice(samples);
        let mut best: Option<f32> = None;
        while self.carry.len() >= FRAME {
            let frame: Vec<f32> = self.carry.drain(..FRAME).collect();
            let p = self.infer_frame(&frame)?;
            best = Some(best.map_or(p, |b| b.max(p)));
        }
        if let Some(b) = best {
            self.last_prob = b;
        }
        Ok(self.last_prob)
    }
}

/// Hysteresis thresholds on the chunk probability: start a turn at `START`, stay in it at `CONTINUE`.
/// (Silero's own default is a single 0.5; the lower continue-threshold stops boundary chatter from
/// stretching/cutting turns — same intent as the RMS hysteresis it replaces.)
pub const START_PROB: f32 = 0.5;
pub const CONTINUE_PROB: f32 = 0.35;

#[cfg(test)]
mod tests {
    use super::*;

    fn model_path() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/vad/silero_vad_v4.onnx")
    }

    fn fixture_speech() -> Vec<f32> {
        // 16-bit PCM mono 16 kHz WAV, 44-byte header (Windows SAPI, "open whatsapp").
        let bytes = include_bytes!("../tests/fixtures/sapi_open_whatsapp_16k.wav");
        bytes[44..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
            .collect()
    }

    #[test]
    fn silence_and_mic_noise_are_not_voice() {
        let mut vad = SileroVad::load(&model_path()).expect("load");
        let mut seed = 12345u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            ((seed >> 8) as f32 / 16_777_216.0 - 0.5) * 0.01 // ~±0.005, a typical mic floor
        };
        let mut max_p = 0.0f32;
        for _ in 0..40 {
            let chunk: Vec<f32> = (0..1280).map(|_| noise()).collect();
            max_p = max_p.max(vad.push(&chunk).unwrap());
        }
        assert!(max_p < START_PROB, "noise scored as voice: {max_p}");
        vad.reset();
        let zeros = vec![0.0f32; 1280];
        for _ in 0..20 {
            assert!(vad.push(&zeros).unwrap() < START_PROB);
        }
    }

    #[test]
    fn speech_fixture_is_detected_in_80ms_chunks() {
        let mut vad = SileroVad::load(&model_path()).expect("load");
        let s = fixture_speech();
        let (mut voiced, mut total) = (0usize, 0usize);
        for chunk in s.chunks_exact(1280) {
            total += 1;
            if vad.push(chunk).unwrap() >= START_PROB {
                voiced += 1;
            }
        }
        assert!(total > 10);
        assert!(voiced as f32 / total as f32 > 0.4, "only {voiced}/{total} chunks voiced");
    }

    #[test]
    fn carry_makes_chunking_irrelevant() {
        // Same audio fed as 1280-chunks vs 100-sample slices must yield the same final state/prob.
        let s = fixture_speech();
        let mut a = SileroVad::load(&model_path()).unwrap();
        let mut b = SileroVad::load(&model_path()).unwrap();
        let n = (s.len() / 1280) * 1280;
        let mut pa = Vec::new();
        for ch in s[..n].chunks(1280) {
            pa.push(a.push(ch).unwrap());
        }
        let mut last_b = 0.0;
        for ch in s[..n].chunks(100) {
            last_b = b.push(ch).unwrap();
        }
        assert!((a.h.iter().zip(&b.h).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)) < 1e-5);
        assert!(last_b.is_finite() && pa.iter().all(|p| (0.0..=1.0).contains(p)));
    }

    #[test]
    fn reset_restores_initial_behaviour() {
        let s = fixture_speech();
        let mut v = SileroVad::load(&model_path()).unwrap();
        let first: Vec<f32> = s.chunks_exact(1280).take(8).map(|c| v.push(c).unwrap()).collect();
        v.reset();
        let second: Vec<f32> = s.chunks_exact(1280).take(8).map(|c| v.push(c).unwrap()).collect();
        for (x, y) in first.iter().zip(&second) {
            assert!((x - y).abs() < 1e-5, "{x} vs {y}");
        }
    }

    #[test]
    fn chunk_latency_budget() {
        let mut v = SileroVad::load(&model_path()).unwrap();
        let chunk = vec![0.01f32; 1280];
        v.push(&chunk).unwrap(); // warm
        let t0 = std::time::Instant::now();
        for _ in 0..50 {
            v.push(&chunk).unwrap();
        }
        let per = t0.elapsed().as_micros() as f64 / 50.0;
        println!("silero: {per:.0} us per 80 ms chunk");
        assert!(per < 20_000.0, "too slow for the capture loop: {per} us");
    }
}
