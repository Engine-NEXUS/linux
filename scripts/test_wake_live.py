#!/usr/bin/env python3
"""
NEXUS Wake Word — Real-Time Live Microphone Tester & Performance Benchmark
Tests how reliably the model hears your call in real-time.

Usage:
  python scripts/test_wake_live.py                 # Live microphone test
  python scripts/test_wake_live.py --threshold 0.40 # Custom trigger threshold
  python scripts/test_wake_live.py --batch          # Batch test on recorded dataset
"""

import os
import sys
import time
import json
import argparse
import numpy as np
from pathlib import Path

# Fix Windows console encoding
if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

try:
    import onnxruntime as ort
except ImportError:
    print("Error: onnxruntime not installed. Run: pip install onnxruntime")
    sys.exit(1)

try:
    import sounddevice as sd
except ImportError:
    print("Error: sounddevice not installed. Run: pip install sounddevice")
    sys.exit(1)

try:
    from scipy.io import wavfile
except ImportError:
    print("Error: scipy not installed. Run: pip install scipy")
    sys.exit(1)

# Paths
ROOT = Path(__file__).resolve().parent.parent
OWW_DIR = ROOT / "src-tauri" / "resources" / "oww"
DATA_DIR = ROOT / "wake_word_data"

# DSP & Pipeline Constants (Aligned with Rust Engine wakeword_oww.rs)
SAMPLE_RATE = 16000
CHUNK_SAMPLES = 1280       # 80ms hop
LOOKBACK_SAMPLES = 480     # 30ms lookback
MEL_FRAMES_PER_CHUNK = 8
MEL_BUFFER_CHUNKS = 10     # 80 total mel frames
EMBEDDING_FRAMES = 16      # 16 embedding vectors = 1.28s context
DEFAULT_THRESHOLD = 0.50   # Raised to match model trained at 0.50 — gives 87.9% recall / 0.7% FA
TARGET_RMS = 0.04
MAX_GAIN = 30.0
COOLDOWN_SECONDS = 1.2     # Minimum time between consecutive triggers


class WakeWordPipeline:
    def __init__(self, model_path=None):
        if model_path is None:
            model_path = OWW_DIR / "nexus.onnx"
        else:
            model_path = Path(model_path)

        if not model_path.exists():
            raise FileNotFoundError(f"Wake word model not found at {model_path}")

        mel_path = OWW_DIR / "melspectrogram.onnx"
        emb_path = OWW_DIR / "embedding_model.onnx"

        if not mel_path.exists() or not emb_path.exists():
            raise FileNotFoundError(f"OWW feature models missing in {OWW_DIR}")

        # Load ONNX sessions
        opts = ort.SessionOptions()
        opts.inter_op_num_threads = 1
        opts.intra_op_num_threads = 2
        opts.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL

        self.mel_session = ort.InferenceSession(str(mel_path), opts)
        self.emb_session = ort.InferenceSession(str(emb_path), opts)
        self.clf_session = ort.InferenceSession(str(model_path), opts)

        self.mel_in_name = self.mel_session.get_inputs()[0].name
        self.emb_in_name = self.emb_session.get_inputs()[0].name
        self.clf_in_name = self.clf_session.get_inputs()[0].name

        # Load Acoustic Profile if available
        self.profile = {}
        profile_path = OWW_DIR / "acoustic_profile.json"
        if not profile_path.exists() and sys.platform == "win32":
            appdata = os.environ.get("APPDATA")
            if appdata:
                profile_path = Path(appdata) / "com.nexus.assistant" / "acoustic_profile.json"
        if profile_path.exists():
            try:
                with open(profile_path, "r", encoding="utf-8") as f:
                    self.profile = json.load(f)
            except Exception:
                pass

        self.highpass_cutoff = self.profile.get("highpass_cutoff_hz", 80.0)
        self.pre_gain = self.profile.get("pre_gain", 1.5)
        self.silence_threshold = self.profile.get("silence_rms_threshold", 0.0030)
        self.impulsive_ratio = self.profile.get("impulsive_ratio", 8.0)
        self.kws_threshold = self.profile.get("kws_threshold", DEFAULT_THRESHOLD)

        # Adaptive Noise Floor & Hardware Calibration
        self.noise_floor = self.profile.get("measured_noise_floor_rms", 0.001)
        self.highpass_prev_x = 0.0
        self.highpass_prev_y = 0.0
        # Dynamic cutoff at 16kHz
        dt = 1.0 / 16000.0
        rc = 1.0 / (2.0 * np.pi * self.highpass_cutoff)
        self.highpass_alpha = rc / (rc + dt)

        self.reset()

    def reset(self):
        """Reset circular buffers and filter state."""
        self.lookback = np.zeros(LOOKBACK_SAMPLES, dtype=np.float32)
        self.mel_buffer = [np.zeros((MEL_FRAMES_PER_CHUNK, 32), dtype=np.float32)] * MEL_BUFFER_CHUNKS
        self.emb_buffer = [np.zeros(96, dtype=np.float32)] * EMBEDDING_FRAMES
        self.last_trigger_time = 0.0
        self.prev_rms = 0.0  # Track previous chunk RMS for impulsive-sound detection
        self.highpass_prev_x = 0.0
        self.highpass_prev_y = 0.0

    def reset_after_trigger(self):
        """Flush embedding buffer after a trigger — prevents phantom cascade triggers
        from residual NEXUS embeddings lingering in the 16-frame context window."""
        self.emb_buffer = [np.zeros(96, dtype=np.float32)] * EMBEDDING_FRAMES
        self.mel_buffer = [np.zeros((MEL_FRAMES_PER_CHUNK, 32), dtype=np.float32)] * MEL_BUFFER_CHUNKS

    def apply_highpass(self, chunk: np.ndarray) -> np.ndarray:
        """Filter out chassis fan rumble (matches Rust HighPassFilter)."""
        out = np.empty_like(chunk)
        for i in range(len(chunk)):
            y = self.highpass_alpha * (self.highpass_prev_y + chunk[i] - self.highpass_prev_x)
            self.highpass_prev_x = chunk[i]
            self.highpass_prev_y = y
            out[i] = y
        return out

    def process_chunk(self, raw_chunk: np.ndarray) -> tuple[float, float, float]:
        """
        Process an 80ms chunk with hardware adaptation (matches Rust wakeword_oww.rs):
        1. Adaptive High-Pass fan filter
        2. Dynamic Pre-Gain + AGC
        3. Continuous Mel + Embedding sliding inference
        """
        # 1. High-pass fan noise filter
        chunk = self.apply_highpass(raw_chunk)
        rms = float(np.sqrt(np.mean(chunk ** 2)))

        # Track ambient noise floor (slow leaky minimum)
        if rms < self.noise_floor:
            self.noise_floor = 0.90 * self.noise_floor + 0.10 * rms
        else:
            self.noise_floor = 0.995 * self.noise_floor + 0.005 * rms

        # 2. Hardware Noise Gate & Impulsive Filter (matches Rust wakeword_oww.rs)
        is_speech = (rms >= self.silence_threshold)
        
        # Impulsive sound gate: coughs, throat-clears are short spikes (much louder than prev chunk)
        is_impulsive = (self.prev_rms > 0.0005 and rms > self.prev_rms * self.impulsive_ratio and rms < 0.05)
        self.prev_rms = rms
        
        if not is_speech or is_impulsive:
            self.lookback = chunk[-LOOKBACK_SAMPLES:]
            return 0.0, rms, 1.0

        # 3. Adaptive Hardware Dynamic AGC
        gain = 1.0
        if rms < TARGET_RMS:
            gain = min((TARGET_RMS / rms) * self.pre_gain, MAX_GAIN)
        elif rms > 0.15:
            gain = 0.15 / rms
        
        proc_chunk = np.clip(chunk * gain, -1.0, 1.0)

        # 4. Mel-Spectrogram Stage
        framed = np.concatenate([self.lookback, proc_chunk]) * 32768.0
        self.lookback = proc_chunk[-LOOKBACK_SAMPLES:]

        mel_out = self.mel_session.run(None, {self.mel_in_name: framed[None, :]})[0]
        mel_scaled = mel_out.reshape(MEL_FRAMES_PER_CHUNK, 32) / 10.0 + 2.0
        self.mel_buffer = (self.mel_buffer + [mel_scaled])[-MEL_BUFFER_CHUNKS:]

        # 5. Embedding Extraction (76-frame sliding window)
        window = np.stack(self.mel_buffer).reshape(80, 32)[4:80]
        emb_out = self.emb_session.run(None, {self.emb_in_name: window[None, :, :, None].astype(np.float32)})[0]
        self.emb_buffer = (self.emb_buffer + [emb_out.reshape(-1)])[-EMBEDDING_FRAMES:]

        # 6. Classifier Scoring
        clf_input = np.stack(self.emb_buffer)[None, :, :].astype(np.float32)
        score = float(self.clf_session.run(None, {self.clf_in_name: clf_input})[0].reshape(-1)[0])

        return score, rms, gain


def run_live_test(threshold=None, model_path=None):
    pipeline = WakeWordPipeline(model_path)
    if threshold is None:
        threshold = pipeline.kws_threshold
    
    print("\n" + "═" * 65)
    print("  🎙️  NEXUS WAKE WORD — LIVE REAL-TIME AUDIT")
    print("═" * 65)
    print(f"  • Trigger Threshold : {threshold:.2f} (Confidence)")
    print(f"  • Audio Stream      : 16 kHz Mono, 80ms chunks (1280 samples)")
    print(f"  • Adaptive DSP      : {pipeline.highpass_cutoff:.1f}Hz HPF + {pipeline.pre_gain:.2f}x Pre-Gain + AGC (30x)")
    if pipeline.profile.get("device_name"):
        print(f"  • Calibrated Mic    : {pipeline.profile.get('device_name')}")
    print(f"  • Instructions      : Speak 'NEXUS' or 'Hey NEXUS' naturally.")
    print(f"  • Press Ctrl+C to finish and view session metrics.")
    print("═" * 65 + "\n")

    trigger_count = 0
    scores_history = []
    start_time = time.time()
    last_trigger_ts = 0.0

    def make_meter(score, length=20):
        filled = int(score * length)
        bar = "█" * filled + "░" * (length - filled)
        return f"[{bar}] {score:5.1%}"

    print(f"  Listening... (Speak 'NEXUS' to test)\n")

    try:
        with sd.InputStream(samplerate=SAMPLE_RATE, channels=1, dtype="float32",
                            blocksize=CHUNK_SAMPLES) as stream:
            while True:
                chunk, _ = stream.read(CHUNK_SAMPLES)
                audio_data = chunk.flatten()
                
                score, rms, gain = pipeline.process_chunk(audio_data)
                now = time.time()

                # Visual meter in console
                if score >= threshold and (now - last_trigger_ts) > COOLDOWN_SECONDS:
                    trigger_count += 1
                    last_trigger_ts = now
                    scores_history.append(score)
                    ts = time.strftime("%H:%M:%S")
                    print(f"\r  🔔 \033[1;32m[TRIGGER #{trigger_count:02d}]\033[0m {ts} — Confidence: \033[1;32m{score:6.1%}\033[0m | RMS: {rms:.4f} (AGC {gain:.1f}x)  ")
                    print(f"     Status: \033[32m● WAKE WORD HEARD SIR!\033[0m\n")
                    # CRITICAL: flush embedding buffer so residual NEXUS context
                    # doesn't cascade into phantom false triggers on next quiet chunks
                    pipeline.reset_after_trigger()
                else:
                    # Live score line
                    meter = make_meter(score)
                    rms_bar = "·" * int(min(rms * 500, 15))
                    status = "\033[33m👂 Listening\033[0m" if rms > 0.0005 else "\033[90m💤 Quiet\033[0m"
                    print(f"\r  {status} {meter} | Gain: {gain:4.1f}x | Energy: {rms_bar:<15s}", end="", flush=True)

    except KeyboardInterrupt:
        pass

    elapsed = time.time() - start_time
    print("\n\n" + "═" * 65)
    print("  📊 SESSION PERFORMANCE SUMMARY")
    print("═" * 65)
    print(f"  • Total Time Monitored   : {elapsed:.1f} seconds")
    print(f"  • Total Wake Triggers    : {trigger_count} times heard")
    if scores_history:
        print(f"  • Peak Confidence Score  : {max(scores_history):.1%}")
        print(f"  • Mean Trigger Confidence: {np.mean(scores_history):.1%}")
        print(f"  • Min Trigger Confidence : {min(scores_history):.1%}")
    else:
        print("  • No triggers recorded (try speaking closer or increasing mic gain)")
    print("═" * 65 + "\n")


def run_batch_test(threshold=DEFAULT_THRESHOLD, model_path=None):
    pipeline = WakeWordPipeline(model_path)
    
    print("\n" + "═" * 65)
    print("  📂 BATCH DATASET RECALL & DISCRIMINATION TEST")
    print("═" * 65)

    categories = ["positive", "negative", "background"]
    results = {}

    for cat in categories:
        cat_dir = DATA_DIR / cat
        if not cat_dir.exists():
            continue

        wav_files = list(cat_dir.glob("*.wav"))
        if not wav_files:
            continue

        hits = 0
        scores = []

        for wav_p in wav_files:
            sr, audio = wavfile.read(str(wav_p))
            if sr != 16000:
                continue
            if audio.ndim > 1:
                audio = audio.mean(axis=1)
            audio_f = audio.astype(np.float32) / 32768.0

            pipeline.reset()
            max_score = 0.0
            for i in range(0, len(audio_f) - CHUNK_SAMPLES + 1, CHUNK_SAMPLES):
                chunk = audio_f[i:i + CHUNK_SAMPLES]
                score, _, _ = pipeline.process_chunk(chunk)
                if score > max_score:
                    max_score = score

            scores.append(max_score)
            if max_score >= threshold:
                hits += 1

        total = len(wav_files)
        rate = hits / total if total > 0 else 0.0
        results[cat] = {
            "total": total,
            "hits": hits,
            "rate": rate,
            "avg_score": float(np.mean(scores)) if scores else 0.0,
            "max_score": float(np.max(scores)) if scores else 0.0,
        }

    print(f"\n  Threshold: {threshold:.2f}")
    print(f"  {'Category':<15s} {'Clips':<8s} {'Triggered':<12s} {'Rate':<10s} {'Avg Score':<10s} {'Result'}")
    print("  " + "─" * 60)

    if "positive" in results:
        r = results["positive"]
        status = "★ Flawless" if r["rate"] >= 0.90 else "● Strong" if r["rate"] >= 0.75 else "▲ Needs Work"
        print(f"  {'Positive (Recall)':<15s} {r['total']:<8d} {r['hits']:<12d} {r['rate']:6.1%}     {r['avg_score']:6.1%}     {status}")

    if "negative" in results:
        r = results["negative"]
        status = "★ Flawless" if r["rate"] <= 0.05 else "● Good" if r["rate"] <= 0.15 else "▲ High False Positive"
        print(f"  {'Negative (Reject)':<15s} {r['total']:<8d} {r['hits']:<12d} {r['rate']:6.1%}     {r['avg_score']:6.1%}     {status}")

    if "background" in results:
        r = results["background"]
        status = "★ Flawless" if r["rate"] == 0 else "● Good" if r["rate"] <= 0.05 else "▲ Noise Sensitive"
        print(f"  {'Background Noise':<15s} {r['total']:<8d} {r['hits']:<12d} {r['rate']:6.1%}     {r['avg_score']:6.1%}     {status}")

    print("═" * 65 + "\n")


def main():
    parser = argparse.ArgumentParser(description="NEXUS Wake Word Live & Batch Performance Tester")
    parser.add_argument("--batch", action="store_true", help="Run batch evaluation on dataset files")
    parser.add_argument("--threshold", type=float, default=DEFAULT_THRESHOLD, help="Trigger threshold (default: 0.35)")
    parser.add_argument("--model", type=str, default=None, help="Custom .onnx model path")

    args = parser.parse_args()

    if args.batch:
        run_batch_test(threshold=args.threshold, model_path=args.model)
    else:
        run_live_test(threshold=args.threshold, model_path=args.model)


if __name__ == "__main__":
    main()
