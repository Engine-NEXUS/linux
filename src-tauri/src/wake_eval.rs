//! Offline wake-word evaluation (the "replay harness").
//!
//! Drives the real runtime engine from `wakeword_oww` (preprocessor -> mel ->
//! embedding -> classifier) over recordings, then replays the stage-1 decision
//! (`stage1_step`: smoothing buffer, score, refractory) at any threshold. The
//! decision code is the same function the live engine calls, so numbers from
//! here describe what the app does, not a re-implementation of it.
//!
//! Stage 1 only: speaker check and the Stage-2 transcript verify are not
//! modelled. One approximation: after a fire the live engine also resets its
//! feature buffers; the replay does not. The refractory (3 s) is longer than
//! the feature window (16 x 80 ms), so fires outside the refractory are
//! unaffected.
//!
//! Used by `dev_tools/wake_replay.rs` (`nexus wake replay`).

use std::path::{Path, PathBuf};

use circular_buffer::CircularBuffer;
use serde::Serialize;

use crate::wakeword_oww::engine::{
    resolve_oww_dir, stage1_step, WakeEngine, CHUNK_MS, DETECTION_BUFFER_SIZE, OWW_CHUNK_SIZE,
};

/// Engine input rate.
pub const SAMPLE_RATE: u32 = 16_000;

/// Silence added before a short clip, so a word at t=0 has a quiet lead-in
/// (the live engine has always been listening to the room before you speak).
pub const CLIP_LEAD_S: f64 = 0.5;
/// Silence added after a short clip, so a word ending at the last sample can
/// still complete its multi-frame confirmation.
pub const CLIP_TAIL_S: f64 = 1.0;
/// Recordings longer than this are treated as continuous streams (no padding).
pub const CLIP_MAX_S: f64 = 30.0;

// ─── WAV input ─────────────────────────────────────────────────────

/// Mono audio decoded from a WAV file.
#[derive(Debug, Clone)]
pub struct WavAudio {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

impl WavAudio {
    pub fn duration_s(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.samples.len() as f64 / self.sample_rate as f64
        }
    }
}

/// Decode a RIFF/WAVE file: PCM 8/16/24/32-bit or IEEE float 32/64, any
/// channel count (averaged to mono). Unknown chunks (LIST, fact, ...) are skipped.
pub fn decode_wav(data: &[u8]) -> anyhow::Result<WavAudio> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        anyhow::bail!("not a RIFF/WAVE file");
    }
    let mut fmt: Option<(u16, u16, u32, u16)> = None; // (format, channels, rate, bits)
    let mut pos = 12usize;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]) as usize;
        let body_start = pos + 8;
        let body_end = (body_start + size).min(data.len());
        let body = &data[body_start..body_end];
        if id == b"fmt " {
            if body.len() < 16 {
                anyhow::bail!("fmt chunk too short");
            }
            let mut format = u16::from_le_bytes([body[0], body[1]]);
            let channels = u16::from_le_bytes([body[2], body[3]]);
            let rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            let bits = u16::from_le_bytes([body[14], body[15]]);
            // WAVE_FORMAT_EXTENSIBLE: the real format is the first 2 bytes of the sub-format GUID.
            if format == 0xFFFE && body.len() >= 26 {
                format = u16::from_le_bytes([body[24], body[25]]);
            }
            fmt = Some((format, channels, rate, bits));
        } else if id == b"data" {
            let (format, channels, rate, bits) =
                fmt.ok_or_else(|| anyhow::anyhow!("data chunk before fmt chunk"))?;
            let samples = decode_samples(body, format, channels, bits)?;
            return Ok(WavAudio { sample_rate: rate, samples });
        }
        // Chunks are word-aligned.
        pos = body_start + size + (size & 1);
    }
    anyhow::bail!("no data chunk")
}

fn decode_samples(body: &[u8], format: u16, channels: u16, bits: u16) -> anyhow::Result<Vec<f32>> {
    let ch = channels.max(1) as usize;
    let bytes = (bits as usize).div_ceil(8);
    if bytes == 0 {
        anyhow::bail!("invalid bits per sample: {bits}");
    }
    let read = |b: &[u8]| -> anyhow::Result<f32> {
        Ok(match (format, bits) {
            (1, 8) => (b[0] as f32 - 128.0) / 128.0,
            (1, 16) => i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0,
            (1, 24) => (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0,
            (1, 32) => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0,
            (3, 32) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            (3, 64) => f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f32,
            _ => anyhow::bail!("unsupported WAV encoding: format {format}, {bits} bits"),
        })
    };
    let frame = bytes * ch;
    let frames = body.len() / frame;
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut sum = 0.0f32;
        for c in 0..ch {
            let at = f * frame + c * bytes;
            sum += read(&body[at..at + bytes])?;
        }
        out.push(sum / ch as f32);
    }
    Ok(out)
}

pub fn read_wav(path: &Path) -> anyhow::Result<WavAudio> {
    let data = std::fs::read(path).map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    decode_wav(&data).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

/// Linear-interpolation resampler, the same arithmetic as the live callback
/// (`engine::on_audio`), applied to a whole recording.
pub fn resample_linear(samples: &[f32], from_sr: u32, to_sr: u32) -> Vec<f32> {
    if from_sr == to_sr || samples.is_empty() || from_sr == 0 || to_sr == 0 {
        return samples.to_vec();
    }
    let ratio = from_sr as f64 / to_sr as f64;
    let mut out = Vec::with_capacity((samples.len() as f64 / ratio) as usize + 1);
    let mut pos = 0.0f64;
    while pos + ratio < samples.len() as f64 {
        let idx0 = pos.floor() as usize;
        let idx1 = (idx0 + 1).min(samples.len() - 1);
        let t = pos - idx0 as f64;
        out.push((samples[idx0] as f64 * (1.0 - t) + samples[idx1] as f64 * t) as f32);
        pos += ratio;
    }
    out
}

/// Engine-ready 16 kHz mono samples for a recording, with clip padding for
/// short clips. Returns (samples, real duration in seconds without padding).
pub fn prepare(audio: &WavAudio) -> (Vec<f32>, f64) {
    let duration = audio.duration_s();
    let mut x = resample_linear(&audio.samples, audio.sample_rate, SAMPLE_RATE);
    if duration <= CLIP_MAX_S {
        let lead = (CLIP_LEAD_S * SAMPLE_RATE as f64) as usize;
        let tail = (CLIP_TAIL_S * SAMPLE_RATE as f64) as usize;
        let mut padded = Vec::with_capacity(lead + x.len() + tail);
        padded.resize(lead, 0.0);
        padded.extend_from_slice(&x);
        padded.resize(padded.len() + tail, 0.0);
        x = padded;
    }
    (x, duration)
}

// ─── Engine pass ───────────────────────────────────────────────────

/// One 80 ms chunk as the engine saw it: `Some(prob)` when it reached the
/// classifier and a stage-1 decision, `None` when it was gated as silence.
pub type Frame = Option<f32>;

/// The real wake engine in replay mode.
pub struct ReplayEngine {
    engine: WakeEngine,
    threshold: f32,
}

impl ReplayEngine {
    /// Load the shipped models (`resources/oww`) with `classifier` in place of
    /// `nexus.onnx` when given. The acoustic profile (threshold, high-pass,
    /// gate, gain) is resolved exactly as the app resolves it.
    pub fn load(classifier: Option<&Path>) -> anyhow::Result<Self> {
        let resources = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        let oww_dir = resolve_oww_dir(&resources)
            .ok_or_else(|| anyhow::anyhow!("oww models not found under {}", resources.display()))?;
        let classifier = classifier
            .map(Path::to_path_buf)
            .unwrap_or_else(|| oww_dir.join("nexus.onnx"));
        // Empty data dir: no enrolled voice profile, so nothing speaker-specific leaks in.
        let app_data = std::env::temp_dir().join("nexus_wake_replay");
        let engine = WakeEngine::with_classifier(&oww_dir, &classifier, app_data)?;
        let threshold = engine.threshold;
        Ok(Self { engine, threshold })
    }

    /// Stage-1 threshold from the acoustic profile (what the app uses live).
    pub fn threshold(&self) -> f32 {
        self.threshold
    }

    pub fn profile(&self) -> &crate::acoustic_profile::AcousticProfile {
        &self.engine.acoustic_profile
    }

    /// Per-chunk frames for 16 kHz mono audio, from fresh engine state. Firing
    /// is disabled during the pass, so frames are not disturbed by a
    /// post-fire reset and can be replayed at any threshold.
    pub fn frames(&mut self, samples: &[f32]) -> Vec<Frame> {
        self.engine.reset_for_replay();
        let saved = self.engine.threshold;
        self.engine.threshold = f32::INFINITY;
        let mut out = Vec::with_capacity(samples.len() / OWW_CHUNK_SIZE + 1);
        for chunk in samples.chunks_exact(OWW_CHUNK_SIZE) {
            self.engine.process(chunk);
            out.push(self.engine.last_chunk_scored.then_some(self.engine.last_raw_prob));
        }
        self.engine.threshold = saved;
        out
    }

    /// Live-equivalent pass at `threshold` with firing enabled: returns the
    /// chunk index of every fire. Used to prove `simulate_fires` matches.
    pub fn live_fires(&mut self, samples: &[f32], threshold: f32) -> Vec<usize> {
        self.engine.reset_for_replay();
        let saved = self.engine.threshold;
        self.engine.threshold = threshold;
        let mut fires = Vec::new();
        for (i, chunk) in samples.chunks_exact(OWW_CHUNK_SIZE).enumerate() {
            if self.engine.process(chunk) {
                fires.push(i);
            }
        }
        self.engine.threshold = saved;
        fires
    }
}

// ─── Stage-1 replay ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Fire {
    pub chunk: usize,
    pub time_ms: u64,
    pub score: f32,
}

/// Replay the live stage-1 decision over recorded frames at `threshold`.
/// Gated chunks push 0.0 without a decision, exactly like the engine.
pub fn simulate_fires(frames: &[Frame], threshold: f32) -> Vec<Fire> {
    let mut buffer = CircularBuffer::<DETECTION_BUFFER_SIZE, f32>::new();
    let mut last_fire = None;
    let mut fires = Vec::new();
    for (i, frame) in frames.iter().enumerate() {
        let now = (i as u64 + 1) * CHUNK_MS;
        match frame {
            Some(prob) => {
                let (fired, score) = stage1_step(&mut buffer, &mut last_fire, now, *prob, threshold);
                if fired {
                    fires.push(Fire { chunk: i, time_ms: now, score });
                }
            }
            None => {
                buffer.push_back(0.0);
            }
        }
    }
    fires
}

/// Highest raw classifier probability in a recording.
pub fn peak_prob(frames: &[Frame]) -> f32 {
    frames.iter().flatten().fold(0.0f32, |a, &b| a.max(b))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Label {
    Positive,
    Negative,
}

/// One scored recording.
#[derive(Debug, Clone)]
pub struct Scored {
    pub path: String,
    pub label: Label,
    /// Condition / category for per-group reporting (e.g. "far", "tv").
    pub group: String,
    /// Real audio duration, excluding clip padding.
    pub duration_s: f64,
    pub frames: Vec<Frame>,
}

/// One threshold on the detection-error trade-off curve.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Point {
    pub threshold: f32,
    pub pos_hits: usize,
    pub pos_total: usize,
    pub recall: f64,
    /// Short negative clips (<= CLIP_MAX_S, e.g. one spoken soundalike) that fired.
    /// A clip is too short to express a per-hour rate, so it is counted, not timed.
    pub neg_clips_fired: usize,
    pub neg_clips: usize,
    /// False fires and hours over negative STREAMS (> CLIP_MAX_S: TV, conversation, room).
    pub neg_fires: usize,
    pub neg_hours: f64,
    /// Stage-1 false fires per hour of negative streams (None with no stream audio).
    pub fa_per_hour: Option<f64>,
}

pub fn evaluate_at(set: &[Scored], threshold: f32) -> Point {
    let mut p = Point {
        threshold,
        pos_hits: 0,
        pos_total: 0,
        recall: 0.0,
        neg_clips_fired: 0,
        neg_clips: 0,
        neg_fires: 0,
        neg_hours: 0.0,
        fa_per_hour: None,
    };
    for s in set {
        let fires = simulate_fires(&s.frames, threshold).len();
        match s.label {
            Label::Positive => {
                p.pos_total += 1;
                if fires > 0 {
                    p.pos_hits += 1;
                }
            }
            Label::Negative if s.duration_s > CLIP_MAX_S => {
                p.neg_fires += fires;
                p.neg_hours += s.duration_s / 3600.0;
            }
            Label::Negative => {
                p.neg_clips += 1;
                if fires > 0 {
                    p.neg_clips_fired += 1;
                }
            }
        }
    }
    if p.pos_total > 0 {
        p.recall = p.pos_hits as f64 / p.pos_total as f64;
    }
    if p.neg_hours > 0.0 {
        p.fa_per_hour = Some(p.neg_fires as f64 / p.neg_hours);
    }
    p
}

pub fn sweep(set: &[Scored], thresholds: &[f32]) -> Vec<Point> {
    thresholds.iter().map(|&t| evaluate_at(set, t)).collect()
}

/// The operating point with the best recall whose false-fire rate is within
/// `target_fa_per_hour` (ties go to the higher, safer threshold).
pub fn best_for_target(points: &[Point], target_fa_per_hour: f64) -> Option<Point> {
    points
        .iter()
        .filter(|p| p.fa_per_hour.is_some_and(|f| f <= target_fa_per_hour))
        .copied()
        .max_by(|a, b| {
            a.recall
                .partial_cmp(&b.recall)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.threshold.partial_cmp(&b.threshold).unwrap_or(std::cmp::Ordering::Equal))
        })
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupRecall {
    pub group: String,
    pub hits: usize,
    pub total: usize,
}

/// Recall per positive group at `threshold`, sorted by group name.
pub fn group_recall(set: &[Scored], threshold: f32) -> Vec<GroupRecall> {
    let mut map: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
    for s in set.iter().filter(|s| s.label == Label::Positive) {
        let e = map.entry(s.group.as_str()).or_default();
        e.1 += 1;
        if !simulate_fires(&s.frames, threshold).is_empty() {
            e.0 += 1;
        }
    }
    map.into_iter()
        .map(|(g, (hits, total))| GroupRecall { group: g.to_string(), hits, total })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct Offender {
    pub path: String,
    pub group: String,
    pub time_s: f64,
    pub score: f32,
}

/// False fires on negatives at `threshold`, highest score first: the
/// hard-negative mining list. Times exclude clip padding.
pub fn offenders(set: &[Scored], threshold: f32, limit: usize) -> Vec<Offender> {
    let mut out = Vec::new();
    for s in set.iter().filter(|s| s.label == Label::Negative) {
        let lead = if s.duration_s <= CLIP_MAX_S { CLIP_LEAD_S } else { 0.0 };
        for f in simulate_fires(&s.frames, threshold) {
            out.push(Offender {
                path: s.path.clone(),
                group: s.group.clone(),
                time_s: (f.time_ms as f64 / 1000.0 - lead).max(0.0),
                score: f.score,
            });
        }
    }
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(limit);
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct Miss {
    pub path: String,
    pub group: String,
    pub peak_prob: f32,
}

/// Positives with no fire at `threshold`, lowest peak first.
pub fn misses(set: &[Scored], threshold: f32) -> Vec<Miss> {
    let mut out: Vec<Miss> = set
        .iter()
        .filter(|s| s.label == Label::Positive && simulate_fires(&s.frames, threshold).is_empty())
        .map(|s| Miss { path: s.path.clone(), group: s.group.clone(), peak_prob: peak_prob(&s.frames) })
        .collect();
    out.sort_by(|a, b| a.peak_prob.partial_cmp(&b.peak_prob).unwrap_or(std::cmp::Ordering::Equal));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav_pcm16(rate: u32, channels: u16, frames: &[Vec<i16>]) -> Vec<u8> {
        let data_len = frames.len() * channels as usize * 2;
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data_len as u32 + 12).to_le_bytes());
        v.extend_from_slice(b"WAVE");
        // An unknown chunk before fmt must be skipped.
        v.extend_from_slice(b"LIST");
        v.extend_from_slice(&4u32.to_le_bytes());
        v.extend_from_slice(b"INFO");
        v.extend_from_slice(b"fmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&channels.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        v.extend_from_slice(&(channels * 2).to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&(data_len as u32).to_le_bytes());
        for f in frames {
            for s in f {
                v.extend_from_slice(&s.to_le_bytes());
            }
        }
        v
    }

    #[test]
    fn decode_pcm16_stereo_averages_and_skips_unknown_chunks() {
        let bytes = wav_pcm16(22_050, 2, &[vec![16384, 0], vec![-32768, -32768]]);
        let w = decode_wav(&bytes).unwrap();
        assert_eq!(w.sample_rate, 22_050);
        assert_eq!(w.samples.len(), 2);
        assert!((w.samples[0] - 0.25).abs() < 1e-6);
        assert!((w.samples[1] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn decode_float32() {
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&3u16.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&16_000u32.to_le_bytes());
        v.extend_from_slice(&64_000u32.to_le_bytes());
        v.extend_from_slice(&4u16.to_le_bytes());
        v.extend_from_slice(&32u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&8u32.to_le_bytes());
        v.extend_from_slice(&0.5f32.to_le_bytes());
        v.extend_from_slice(&(-0.25f32).to_le_bytes());
        let w = decode_wav(&v).unwrap();
        assert_eq!(w.samples, vec![0.5, -0.25]);
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode_wav(b"not a wav file at all").is_err());
    }

    #[test]
    fn resample_keeps_duration_and_dc() {
        let x = vec![0.3f32; 48_000];
        let y = resample_linear(&x, 48_000, 16_000);
        assert!((y.len() as i64 - 16_000).abs() <= 1, "len {}", y.len());
        assert!(y.iter().all(|&s| (s - 0.3).abs() < 1e-6));
        assert_eq!(resample_linear(&x[..10], 16_000, 16_000).len(), 10);
    }

    #[test]
    fn prepare_pads_clips_but_not_streams() {
        let clip = WavAudio { sample_rate: 16_000, samples: vec![0.1; 16_000] };
        let (x, d) = prepare(&clip);
        assert_eq!(d, 1.0);
        assert_eq!(x.len(), 16_000 + 8_000 + 16_000);
        let stream = WavAudio { sample_rate: 16_000, samples: vec![0.0; 16_000 * 31] };
        let (y, _) = prepare(&stream);
        assert_eq!(y.len(), 16_000 * 31);
    }

    #[test]
    fn simulate_fires_two_confident_frames_fire_once() {
        let mut f: Vec<Frame> = vec![Some(0.01); 10];
        f.push(Some(0.9));
        f.push(Some(0.9));
        f.extend(vec![Some(0.9); 5]); // still inside the refractory
        let fires = simulate_fires(&f, 0.5);
        assert_eq!(fires.len(), 1);
        assert_eq!(fires[0].chunk, 11);
        assert_eq!(fires[0].time_ms, 12 * CHUNK_MS);
    }

    #[test]
    fn simulate_fires_refractory_then_second_fire() {
        let mut f: Vec<Frame> = vec![Some(0.9), Some(0.9)];
        // 3 s of quiet = 37.5 chunks; 38 keeps us exactly past NO_DETECTION_MS.
        f.extend(vec![Some(0.0); 38]);
        f.push(Some(0.9));
        f.push(Some(0.9));
        let fires = simulate_fires(&f, 0.5);
        assert_eq!(fires.len(), 2, "{fires:?}");
        // A burst inside the refractory does not fire.
        let mut g: Vec<Frame> = vec![Some(0.9), Some(0.9)];
        g.extend(vec![Some(0.0); 20]);
        g.push(Some(0.9));
        g.push(Some(0.9));
        assert_eq!(simulate_fires(&g, 0.5).len(), 1);
    }

    #[test]
    fn gated_frames_never_fire_and_dilute() {
        assert!(simulate_fires(&vec![None; 100], 0.01).is_empty());
        // One confident frame + gated neighbours: path 2 needs two frames >= 0.20.
        let f: Vec<Frame> = vec![None, Some(0.95), None, None];
        assert!(simulate_fires(&f, 0.5).is_empty());
    }

    fn scored(label: Label, group: &str, dur: f64, frames: Vec<Frame>) -> Scored {
        Scored { path: format!("{group}.wav"), label, group: group.into(), duration_s: dur, frames }
    }

    #[test]
    fn evaluate_counts_recall_and_false_fires_per_hour() {
        let hit = vec![Some(0.8), Some(0.8)];
        let weak = vec![Some(0.3), Some(0.3)];
        let set = vec![
            scored(Label::Positive, "near", 2.0, hit.clone()),
            scored(Label::Positive, "far", 2.0, weak.clone()),
            scored(Label::Negative, "tv", 1800.0, hit.clone()),
            scored(Label::Negative, "music", 1800.0, weak),
            scored(Label::Negative, "lexus", 2.0, hit.clone()),
            scored(Label::Negative, "access", 2.0, vec![Some(0.1), Some(0.1)]),
        ];
        let p = evaluate_at(&set, 0.5);
        assert_eq!((p.pos_hits, p.pos_total), (1, 2));
        assert!((p.recall - 0.5).abs() < 1e-9);
        // Short clips are counted; only streams contribute to fires per hour.
        assert_eq!((p.neg_clips_fired, p.neg_clips), (1, 2));
        assert_eq!(p.neg_fires, 1);
        assert!((p.neg_hours - 1.0).abs() < 1e-9);
        assert!((p.fa_per_hour.unwrap() - 1.0).abs() < 1e-9);
        let low = evaluate_at(&set, 0.25);
        assert_eq!(low.pos_hits, 2);
        assert_eq!(low.neg_fires, 2);
        let gr = group_recall(&set, 0.5);
        assert_eq!(gr.len(), 2);
        assert_eq!((gr[0].group.as_str(), gr[0].hits), ("far", 0));
        assert_eq!(misses(&set, 0.5).len(), 1);
        assert_eq!(offenders(&set, 0.5, 10).len(), 2);
    }

    #[test]
    fn best_for_target_prefers_recall_within_budget() {
        let mk = |t: f32, r: f64, fa: f64| Point {
            threshold: t,
            pos_hits: 0,
            pos_total: 0,
            recall: r,
            neg_clips_fired: 0,
            neg_clips: 0,
            neg_fires: 0,
            neg_hours: 1.0,
            fa_per_hour: Some(fa),
        };
        let pts = vec![mk(0.3, 0.99, 4.0), mk(0.5, 0.95, 0.4), mk(0.6, 0.95, 0.1), mk(0.8, 0.80, 0.0)];
        let best = best_for_target(&pts, 0.5).unwrap();
        assert_eq!(best.threshold, 0.6);
        assert!(best_for_target(&pts, -1.0).is_none());
    }

    /// `model_manifest.json` documents the engine's runtime constants; it drifted
    /// once (said 10 s grace / gain 20 / threshold 0.35 while the code did
    /// 1.5 s / 30 / profile threshold). Keep them pinned together.
    #[test]
    fn model_manifest_runtime_matches_engine_constants() {
        use crate::wakeword_oww::engine::{
            AGC_MAX_GAIN, AGC_TARGET_RMS, NO_DETECTION_MS, STARTUP_GRACE_MS,
        };
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/oww/model_manifest.json");
        let m: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let rt = &m["runtime"];
        assert_eq!(rt["startup_grace_milliseconds"].as_u64(), Some(STARTUP_GRACE_MS));
        assert_eq!(rt["refractory_milliseconds"].as_u64(), Some(NO_DETECTION_MS));
        assert_eq!(rt["smoothing_frames"].as_u64(), Some(DETECTION_BUFFER_SIZE as u64));
        assert_eq!(rt["agc_target_rms"].as_f64().map(|v| v as f32), Some(AGC_TARGET_RMS));
        assert_eq!(rt["agc_max_gain"].as_f64().map(|v| v as f32), Some(AGC_MAX_GAIN));
        assert!(rt.get("wake_threshold").is_none(), "threshold belongs to the acoustic profile");
    }

    /// The replay must make the same decisions as the live engine. Uses the
    /// shipped models; skipped when they are absent.
    #[test]
    fn replay_matches_live_engine_decisions() {
        let mut eng = match ReplayEngine::load(None) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("SKIP: wake models unavailable: {e}");
                return;
            }
        };
        // Speech-like synthetic signal: AM-modulated harmonics with pauses.
        let n = SAMPLE_RATE as usize * 12;
        let samples: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                let gate = if (t * 1.3).fract() < 0.55 { 1.0 } else { 0.0 };
                let am = 0.5 + 0.5 * (t * 2.0 * std::f32::consts::PI * 4.0).sin();
                let v = (t * 2.0 * std::f32::consts::PI * 180.0).sin()
                    + 0.5 * (t * 2.0 * std::f32::consts::PI * 360.0).sin()
                    + 0.25 * (t * 2.0 * std::f32::consts::PI * 1100.0).sin();
                0.08 * gate * am * v
            })
            .collect();
        let frames = eng.frames(&samples);
        assert_eq!(frames.len(), n / OWW_CHUNK_SIZE);
        assert!(frames.iter().any(|f| f.is_some()), "signal should pass the gate");
        // Pick a threshold that is crossed by this signal so the comparison is not vacuous.
        let peak = peak_prob(&frames);
        let thr = (peak * 0.5).max(1e-6);
        let simulated: Vec<usize> = simulate_fires(&frames, thr).into_iter().map(|f| f.chunk).collect();
        let live = eng.live_fires(&samples, thr);
        assert!(!simulated.is_empty(), "comparison would be vacuous: peak={peak} thr={thr}");
        // Identical up to and including the first fire (after it the live engine
        // also resets its feature buffers, which the replay does not model).
        assert_eq!(simulated.first(), live.first(), "peak={peak} thr={thr}");
        // Silence is gated end to end.
        let silent = eng.frames(&vec![0.0; SAMPLE_RATE as usize * 3]);
        assert!(silent.iter().all(|f| f.is_none()));
    }
}
