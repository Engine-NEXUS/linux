#!/usr/bin/env python3
"""
NEXUS Wake Word & Live Speech Transcription Comparison System.
Simultaneously runs:
  1. Real-time openWakeWord KWS detection on 80ms sliding windows.
  2. Local Whisper ASR transcription of spoken utterances.
Cross-checks every spoken phrase against wake model activations to verify
flawless discrimination between "NEXUS" and other phrases/noises.

Usage:
  python scripts/test_wake_compare.py
  python scripts/test_wake_compare.py --threshold 0.50
  nexus wake test --compare
"""

import os
import sys
import time
import queue
import threading
import argparse
from pathlib import Path
import numpy as np

# Fix Windows console encoding
if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace", line_buffering=True)
    sys.stderr.reconfigure(encoding="utf-8", errors="replace", line_buffering=True)

try:
    import sounddevice as sd
except ImportError:
    print("Error: sounddevice is required. Install: pip install sounddevice")
    sys.exit(1)

try:
    from scipy.io import wavfile
except ImportError:
    print("Error: scipy is required. Install: pip install scipy")
    sys.exit(1)

from faster_whisper import WhisperModel

ROOT = Path(__file__).resolve().parent.parent
OWW_DIR = ROOT / "src-tauri" / "resources" / "oww"
RECORDINGS_DIR = ROOT / "wake_test_recordings"
RECORDINGS_DIR.mkdir(exist_ok=True)

# Add scripts directory to path to import WakeWordPipeline
sys.path.insert(0, str(ROOT / "scripts"))
from test_wake_live import WakeWordPipeline, SAMPLE_RATE, CHUNK_SAMPLES


class DualStreamMonitor:
    def __init__(self, threshold=0.50, whisper_model_size="tiny.en"):
        self.threshold = threshold
        self.pipeline = WakeWordPipeline()
        self.pipeline.kws_threshold = threshold

        print(f"Loading local Whisper model ({whisper_model_size}) on CPU...", flush=True)
        self.whisper = WhisperModel(whisper_model_size, device="cpu", compute_type="int8")
        print("Whisper model loaded and ready.\n", flush=True)

        self.audio_queue = queue.Queue()
        self.speech_buffer = []
        self.current_window_scores = []
        self.is_speaking = False
        self.silence_chunks = 0
        self.running = True

        # Session metrics
        self.stats = {
            "total_utterances": 0,
            "true_positives": 0,
            "true_negatives": 0,
            "false_alarms": 0,
            "missed": 0,
        }

        # Query default input device configuration
        try:
            dev = sd.default.device
            if hasattr(dev, "__getitem__"):
                dev_idx = dev[0]
            elif isinstance(dev, int):
                dev_idx = dev
            else:
                dev_idx = int(dev)
            if dev_idx is None or dev_idx < 0:
                dev_idx = 0
            dev_info = sd.query_devices(dev_idx)
            native_ch = max(1, int(dev_info.get("max_input_channels", 1)))
            native_sr = int(dev_info.get("default_samplerate", 44100))
            dev_name = dev_info.get("name", "Default Microphone")
        except Exception:
            dev_idx = 0
            native_ch = 1
            native_sr = SAMPLE_RATE
            dev_name = "Default Microphone"

        print(f"  Calibrated Mic  : {dev_name} ({native_ch}ch @ {native_sr}Hz -> 16kHz)")
        print(f"{'TIME':<8} | {'SPOKEN TRANSCRIPT':<32} | {'CONFIDENCE':<14} | {'VERDICT'}")
        print("-" * 78)

        resample_carry = np.array([], dtype=np.float32)

        def audio_cb(indata, frames, time_info, status):
            nonlocal resample_carry
            # Downmix active channels (ignore silent auxiliary channels on quad arrays)
            if native_ch >= 2:
                mono = (indata[:, 0] + indata[:, 1]) / 2.0
            else:
                mono = indata[:, 0]

            # Resample to 16kHz
            if native_sr != SAMPLE_RATE:
                total_samples = len(mono)
                num_16k = int(total_samples * SAMPLE_RATE / native_sr)
                indices = np.linspace(0, total_samples - 1, num_16k)
                resampled = np.interp(indices, np.arange(total_samples), mono).astype(np.float32)
            else:
                resampled = mono

            resample_carry = np.concatenate([resample_carry, resampled])
            while len(resample_carry) >= CHUNK_SAMPLES:
                chunk = resample_carry[:CHUNK_SAMPLES]
                resample_carry = resample_carry[CHUNK_SAMPLES:]
                self.audio_queue.put(chunk.copy())

        # Start native microphone input stream
        native_block = int(native_sr * 0.08)  # 80ms chunks
        with sd.InputStream(
            channels=native_ch,
            samplerate=native_sr,
            blocksize=native_block,
            dtype="float32",
            callback=audio_cb,
        ):
            start_time = time.time()
            accumulated_samples = np.array([], dtype=np.float32)

            try:
                while self.running:
                    try:
                        chunk = self.audio_queue.get(timeout=0.1)
                    except queue.Empty:
                        continue

                    chunk = chunk.flatten()
                    prob, rms, gain = self.pipeline.process_chunk(chunk)
                    elapsed = time.time() - start_time
                    timestamp_str = f"{int(elapsed // 60):02d}:{elapsed % 60:04.1f}"

                    is_speech = rms >= self.pipeline.silence_threshold

                    if is_speech:
                        self.is_speaking = True
                        self.silence_chunks = 0
                        self.speech_buffer.append(chunk)
                        self.current_window_scores.append(prob)
                    else:
                        if self.is_speaking:
                            self.silence_chunks += 1
                            self.speech_buffer.append(chunk)
                            self.current_window_scores.append(prob)

                            # If 400ms of silence follows speech (5 chunks), process the utterance
                            if self.silence_chunks >= 5:
                                self.process_utterance(timestamp_str)
                                self.is_speaking = False
                                self.silence_chunks = 0
                                self.speech_buffer = []
                                self.current_window_scores = []

                    # Real-time meter in status line
                    level = int(min(rms / 0.15, 1.0) * 15)
                    bar = "█" * level + "·" * (15 - level)
                    conf_display = f"{prob * 100:5.1f}%"
                    status_line = (
                        f"\r  [{timestamp_str}] [RMS: {bar} {rms:.4f}] [Live Prob: {conf_display}] "
                    )
                    sys.stdout.write(status_line)
                    sys.stdout.flush()

            except KeyboardInterrupt:
                print("\n\nStopping live comparison...")
                self.print_summary()

    def process_utterance(self, timestamp_str):
        if not self.speech_buffer:
            return

        full_audio = np.concatenate(self.speech_buffer)
        peak_score = max(self.current_window_scores) if self.current_window_scores else 0.0

        # Don't transcribe tiny blips (< 0.25s)
        if len(full_audio) < SAMPLE_RATE * 0.25:
            return

        # Fast local Whisper transcription
        segments, _ = self.whisper.transcribe(
            full_audio,
            beam_size=1,
            language="en",
            temperature=0.0,
            initial_prompt="NEXUS, wake word, documentation created and synchronized.",
        )
        transcript = " ".join([s.text for s in segments]).strip()

        # Handle non-verbal audio (empty text)
        if not transcript:
            transcript = "[Vocal noise / throat / click]"

        # Check for NEXUS in transcript
        transcript_lower = transcript.lower()
        has_nexus = "nexus" in transcript_lower or "nexis" in transcript_lower

        triggered = peak_score >= self.threshold
        self.stats["total_utterances"] += 1

        if has_nexus and triggered:
            verdict = "🔔 \033[92m[✓ TRUE POSITIVE]\033[0m"
            self.stats["true_positives"] += 1
            self.pipeline.reset_after_trigger()
        elif not has_nexus and not triggered:
            verdict = "🛡️  \033[94m[✓ REJECTED (TRUE NEGATIVE)]\033[0m"
            self.stats["true_negatives"] += 1
        elif not has_nexus and triggered:
            verdict = "⚠️  \033[91m[⚠ FALSE ALARM]\033[0m"
            self.stats["false_alarms"] += 1
            # Save audio for forensic review
            wav_path = RECORDINGS_DIR / f"false_alarm_{int(time.time())}.wav"
            wavfile.write(str(wav_path), SAMPLE_RATE, (full_audio * 32767.0).astype(np.int16))
            self.pipeline.reset_after_trigger()
        else:  # has_nexus and not triggered
            verdict = "❌ \033[93m[✕ MISSED WAKE]\033[0m"
            self.stats["missed"] += 1
            wav_path = RECORDINGS_DIR / f"missed_{int(time.time())}.wav"
            wavfile.write(str(wav_path), SAMPLE_RATE, (full_audio * 32767.0).astype(np.int16))

        # Clear the current status line and print the log row
        sys.stdout.write("\r" + " " * 85 + "\r")
        short_transcript = (transcript[:29] + "...") if len(transcript) > 32 else transcript
        print(
            f"  {timestamp_str:<6} | {short_transcript:<32} | Peak: {peak_score * 100:5.1f}%     | {verdict}"
        )
        sys.stdout.flush()

    def print_summary(self):
        print("=" * 78)
        print("  LIVE COMPARISON VERIFICATION SUMMARY")
        print("=" * 78)
        tot = self.stats["total_utterances"]
        tp = self.stats["true_positives"]
        tn = self.stats["true_negatives"]
        fa = self.stats["false_alarms"]
        fn = self.stats["missed"]

        acc = ((tp + tn) / tot * 100) if tot > 0 else 100.0

        print(f"  Total Utterances Spoken     : {tot}")
        print(f"  True Positives (NEXUS Wakes): {tp}")
        print(f"  True Negatives (Rejected)   : {tn}")
        print(f"  False Alarms (Wrong Wakes)  : {fa}")
        print(f"  Missed Calls (Failed Wakes) : {fn}")
        print(f"  Overall Verification Score  : {acc:.1f}%")
        print("=" * 78)


def main():
    parser = argparse.ArgumentParser(description="Simultaneous Wake Word & ASR Comparison")
    parser.add_argument("--threshold", type=float, default=0.50, help="KWS Trigger threshold (default: 0.50)")
    parser.add_argument("--model", type=str, default="tiny.en", help="Whisper model size (default: tiny.en)")
    args = parser.parse_args()

    monitor = DualStreamMonitor(threshold=args.threshold, whisper_model_size=args.model)
    monitor.run()


if __name__ == "__main__":
    main()
