#!/usr/bin/env python3
"""
NEXUS Wake Word — Continuous Live Sample Recorder
Records many voice samples quickly for wake word training.

Modes:
  1. POSITIVE — say "NEXUS" (or variant) when prompted
  2. NEGATIVE — say the displayed negative word/phrase
  3. FREE — record continuously, say "NEXUS" naturally with pauses
  4. BACKGROUND — record background noise (no speaking) for negatives

Usage:
  python scripts/record_wake_samples.py positive 200
  python scripts/record_wake_samples.py negative 100
  python scripts/record_wake_samples.py free 60
  python scripts/record_wake_samples.py background 30

Output:
  wake_word_data/positive/nexus_001.wav ... nexus_200.wav
  wake_word_data/negative/next_001.wav ... focus_001.wav ...
  wake_word_data/free/free_001.wav ...
  wake_word_data/background/bg_001.wav ...
"""
import sounddevice as sd
import numpy as np
import scipy.io.wavfile as wav
import os
import sys
import time
import random

# Fix Windows console encoding for Unicode characters
if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

# ─── Config ────────────────────────────────────────────────────────────────
SAMPLE_RATE = 16000
CLIP_DURATION = 2.0  # seconds per clip
SAMPLES_PER_CLIP = int(SAMPLE_RATE * CLIP_DURATION)

BASE_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "wake_word_data")

# Positive phrases — the wake word and its variants
POSITIVE_PHRASES = [
    "nexus",
    "hey nexus",
    "ok nexus",
    "nexus wake up",
    "nexus please",
]

# Negative phrases — soundalikes and common words that should NOT trigger
NEGATIVE_PHRASES = [
    "next", "nixis", "mexic", "necess", "lexis", "nixes", "nixus",
    "noxus", "naxus", "text", "taxes", "focus", "bonus", "census",
    "versus", "hocus", "locus", "next us", "this is", "process",
    "access", "excess", "success", "reflexes", "complex", "context",
    "index", "annex", "nervous", "precious", "delicious", "suspicious",
    "connect us", "protect us", "collect us", "expect us",
    # Common everyday words that might false-trigger
    "hello", "hey", "okay", "please", "thank you", "what",
    "computer", "assistant", "google", "alexa", "siri", "hey google",
    "hey siri", "hey alexa", "ok google", "ok siri",
    # Random speech
    "the weather is nice", "open chrome", "play music", "what time is it",
    "send a message", "close the window", "turn off the light",
]

# ─── Recording ─────────────────────────────────────────────────────────────

def record_clip(duration=CLIP_DURATION):
    """Record a single clip. Returns numpy array or None if too quiet."""
    audio = sd.rec(int(duration * SAMPLE_RATE), samplerate=SAMPLE_RATE,
                   channels=1, dtype=np.float32)
    sd.wait()
    rms = np.sqrt(np.mean(audio**2))
    return audio, rms


def save_wav(path, audio):
    """Save float32 audio as 16-bit PCM WAV."""
    audio_int16 = (audio * 32767).clip(-32768, 32767).astype(np.int16)
    wav.write(path, SAMPLE_RATE, audio_int16)


def count_existing(directory, prefix):
    """Count existing files with the given prefix."""
    if not os.path.exists(directory):
        return 0
    return len([f for f in os.listdir(directory) if f.startswith(prefix) and f.endswith(".wav")])


def mode_positive(n_clips):
    """Record positive samples — say 'NEXUS' or a variant."""
    out_dir = os.path.join(BASE_DIR, "positive")
    os.makedirs(out_dir, exist_ok=True)

    existing = count_existing(out_dir, "nexus_")
    start = existing + 1
    end = start + n_clips - 1

    print("=" * 60)
    print(f"  POSITIVE SAMPLE RECORDING")
    print(f"  Recording {n_clips} clips ({start} to {end})")
    print(f"  Each clip: {CLIP_DURATION}s — say the phrase when prompted")
    print(f"  Output: {out_dir}")
    print("=" * 60)
    print()
    print("  INSTRUCTIONS:")
    print("  - Say the phrase CLEARLY and NATURALLY")
    print("  - Vary your distance from the mic (close, normal, far)")
    print("  - Vary your volume (normal, quiet, loud, whisper)")
    print("  - Vary your speed (normal, fast, slow)")
    print("  - Speak in different directions (facing mic, turned away)")
    print("  - Press Ctrl+C to stop early")
    print()

    SILENCE_THRESHOLD = 0.003
    saved = 0
    skipped = 0

    for i in range(start, end + 1):
        phrase = random.choice(POSITIVE_PHRASES)
        print(f"  [{i}/{end}] Say: \"{phrase}\"  ", end="", flush=True)

        # Countdown
        for c in range(3, 0, -1):
            print(f"{c}... ", end="", flush=True)
            time.sleep(0.4)

        print("REC", end="", flush=True)
        audio, rms = record_clip()

        if rms < SILENCE_THRESHOLD:
            print(f"  SKIP (RMS={rms:.5f} too quiet)")
            skipped += 1
            continue

        fname = f"nexus_{i:04d}.wav"
        save_wav(os.path.join(out_dir, fname), audio)
        saved += 1
        print(f"  OK (RMS={rms:.4f}) → {fname}")

    print(f"\n  Done: {saved} saved, {skipped} skipped")
    print(f"  Total positive samples: {count_existing(out_dir, 'nexus_')}")


def mode_negative(n_clips):
    """Record negative samples — say the displayed phrase (NOT nexus)."""
    out_dir = os.path.join(BASE_DIR, "negative")
    os.makedirs(out_dir, exist_ok=True)

    print("=" * 60)
    print(f"  NEGATIVE SAMPLE RECORDING")
    print(f"  Recording {n_clips} clips")
    print(f"  Each clip: {CLIP_DURATION}s — say the displayed phrase")
    print(f"  Output: {out_dir}")
    print("=" * 60)
    print()
    print("  INSTRUCTIONS:")
    print("  - Say the displayed phrase (it will NOT be 'nexus')")
    print("  - These teach the model what NOT to trigger on")
    print("  - Speak naturally")
    print("  - Press Ctrl+C to stop early")
    print()

    SILENCE_THRESHOLD = 0.003
    saved = 0
    skipped = 0

    for i in range(1, n_clips + 1):
        phrase = random.choice(NEGATIVE_PHRASES)
        # Create safe filename from phrase
        safe = phrase.replace(" ", "_").replace("'", "")
        existing = count_existing(out_dir, f"{safe}_")
        idx = existing + 1

        print(f"  [{i}/{n_clips}] Say: \"{phrase}\"  ", end="", flush=True)
        for c in range(3, 0, -1):
            print(f"{c}... ", end="", flush=True)
            time.sleep(0.4)

        print("REC", end="", flush=True)
        audio, rms = record_clip()

        if rms < SILENCE_THRESHOLD:
            print(f"  SKIP (RMS={rms:.5f} too quiet)")
            skipped += 1
            continue

        fname = f"{safe}_{idx:04d}.wav"
        save_wav(os.path.join(out_dir, fname), audio)
        saved += 1
        print(f"  OK (RMS={rms:.4f}) → {fname}")

    print(f"\n  Done: {saved} saved, {skipped} skipped")


def mode_free(duration_sec):
    """Record continuously — say 'NEXUS' naturally with pauses."""
    out_dir = os.path.join(BASE_DIR, "free")
    os.makedirs(out_dir, exist_ok=True)

    existing = count_existing(out_dir, "free_")
    start = existing + 1

    print("=" * 60)
    print(f"  FREE RECORDING MODE")
    print(f"  Recording for {duration_sec} seconds")
    print(f"  Say 'NEXUS' naturally, with pauses between")
    print(f"  Output: {out_dir}")
    print("=" * 60)
    print()
    print("  INSTRUCTIONS:")
    print("  - Say 'NEXUS' whenever you want")
    print("  - Pause 2-3 seconds between each 'NEXUS'")
    print("  - Talk normally in between (the gaps become negative data)")
    print("  - Vary your distance and volume")
    print("  - Press Ctrl+C to stop early")
    print()

    # Record in 10-second chunks
    CHUNK_SEC = 10
    chunk_samples = int(SAMPLE_RATE * CHUNK_SEC)
    n_chunks = max(1, duration_sec // CHUNK_SEC)

    for chunk_idx in range(n_chunks):
        print(f"  Chunk {chunk_idx+1}/{n_chunks} ({CHUNK_SEC}s)... ", end="", flush=True)
        audio = sd.rec(chunk_samples, samplerate=SAMPLE_RATE, channels=1, dtype=np.float32)
        sd.wait()

        rms = np.sqrt(np.mean(audio**2))
        if rms < 0.001:
            print(f"SKIP (RMS={rms:.5f})")
            continue

        fname = f"free_{start + chunk_idx:04d}.wav"
        save_wav(os.path.join(out_dir, fname), audio)
        print(f"OK (RMS={rms:.4f}) → {fname}")

    print(f"\n  Done. Free samples: {count_existing(out_dir, 'free_')}")


def mode_background(n_clips):
    """Record background noise — no speaking."""
    out_dir = os.path.join(BASE_DIR, "background")
    os.makedirs(out_dir, exist_ok=True)

    existing = count_existing(out_dir, "bg_")
    start = existing + 1

    print("=" * 60)
    print(f"  BACKGROUND NOISE RECORDING")
    print(f"  Recording {n_clips} clips of {CLIP_DURATION}s each")
    print(f"  DO NOT SPEAK — just let background noise record")
    print(f"  Output: {out_dir}")
    print("=" * 60)
    print()
    print("  Play music, have TV on, or just record room noise.")
    print("  These become negative samples for noise robustness.")
    print()

    for i in range(start, start + n_clips):
        print(f"  [{i}/{start + n_clips - 1}] Recording background... ", end="", flush=True)
        audio, rms = record_clip()
        fname = f"bg_{i:04d}.wav"
        save_wav(os.path.join(out_dir, fname), audio)
        print(f"OK (RMS={rms:.5f}) → {fname}")

    print(f"\n  Done. Background samples: {count_existing(out_dir, 'bg_')}")


def mode_stats():
    """Show statistics of all recorded samples."""
    print("=" * 60)
    print("  WAKE WORD DATA STATISTICS")
    print("=" * 60)
    print()

    for subdir in ["positive", "negative", "free", "background"]:
        d = os.path.join(BASE_DIR, subdir)
        if not os.path.exists(d):
            print(f"  {subdir:15s}: 0 files (not created yet)")
            continue
        files = [f for f in os.listdir(d) if f.endswith(".wav")]
        total_size = sum(os.path.getsize(os.path.join(d, f)) for f in files)
        print(f"  {subdir:15s}: {len(files):4d} files ({total_size/1024/1024:.1f} MB)")

    print()


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        print("\nAlso: python record_wake_samples.py stats")
        sys.exit(1)

    mode = sys.argv[1].lower()

    if mode == "stats":
        mode_stats()
    elif mode == "positive":
        n = int(sys.argv[2]) if len(sys.argv) > 2 else 50
        mode_positive(n)
    elif mode == "negative":
        n = int(sys.argv[2]) if len(sys.argv) > 2 else 50
        mode_negative(n)
    elif mode == "free":
        duration = int(sys.argv[2]) if len(sys.argv) > 2 else 60
        mode_free(duration)
    elif mode == "background":
        n = int(sys.argv[2]) if len(sys.argv) > 2 else 30
        mode_background(n)
    else:
        print(f"Unknown mode: {mode}")
        print(__doc__)
        sys.exit(1)


if __name__ == "__main__":
    main()
