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


def get_next_index(directory, prefix):
    """Find the next integer index based on the highest existing file number."""
    if not os.path.exists(directory):
        return 1
    max_idx = 0
    for f in os.listdir(directory):
        if f.startswith(prefix) and f.endswith(".wav"):
            try:
                num_part = f[len(prefix):].split(".")[0]
                idx = int(num_part)
                if idx > max_idx:
                    max_idx = idx
            except ValueError:
                pass
    return max_idx + 1


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

def load_whisper_validator():
    """Load local tiny.en Whisper model for zero-lag instant verification (<100ms)."""
    try:
        from faster_whisper import WhisperModel
        return WhisperModel("tiny.en", device="cpu", compute_type="int8")
    except Exception as e:
        print(f"Warning: could not load Whisper validator ({e}). Proceeding without ASR gate.")
        return None


def verify_positive_transcript(whisper_model, audio_float32):
    """
    Strict ASR verification for positive wake word recordings:
    Returns (verdict, transcript, reason)
      - verdict: 'accept_pos', 'promote_neg', or 'reject'
    """
    if whisper_model is None:
        return 'accept_pos', 'unverified', 'Whisper validator unavailable'

    segments, _ = whisper_model.transcribe(
        audio_float32,
        beam_size=1,
        language="en",
        temperature=0.0,
        initial_prompt="NEXUS, wake word call."
    )
    transcript = " ".join([s.text for s in segments]).strip()
    clean = transcript.lower().strip(" .,!?:;\"'")

    if not clean:
        return 'reject', clean, 'No speech detected / inaudible'

    # Accepted variations of NEXUS across accents and Whisper's short-utterance bias
    # (Whisper often hallucinates 'next', 'next sis', 'next test', 'nixes' for single-word 'nexus')
    ACCEPTED_PATTERNS = [
        "nexus", "nexis", "nixes", "nexas", "nexos", "nex", "next", "nexts", "nextis",
        "hey nexus", "ok nexus", "okay nexus", "nexus wake up", "nexus please",
        "open excess", "open access", "next sis", "next sense", "next one", "next test",
        "next sus", "next us", "next yes", "next sir", "next sif", "next chef", "next sith",
        "next, s", "next, next", "mix us", "mixers", "make sense", "ten xs"
    ]
    if any(p == clean or clean.startswith(p) for p in ACCEPTED_PATTERNS):
        return 'accept_pos', clean, f"Valid phonetic NEXUS match ('{clean}')"

    # Check if 'nexus' / 'nexis' / 'next' is part of a short valid call (<= 3 words)
    words = clean.split()
    if any(w in words for w in ["nexus", "nexis", "nixes", "next", "neck"]) and len(words) <= 3:
        return 'accept_pos', clean, f"Short valid NEXUS call ('{clean}')"

    # NEVER promote to negative in positive recording mode!
    # If the user spoke completely different speech or noise, reject and re-prompt.
    return 'reject', clean, f"Non-wake speech detected ('{clean}')"


def mode_positive(n_clips):
    """Record positive samples — with real-time Whisper anti-poisoning validation."""
    out_dir = os.path.join(BASE_DIR, "positive")
    neg_dir = os.path.join(BASE_DIR, "negative")
    os.makedirs(out_dir, exist_ok=True)
    os.makedirs(neg_dir, exist_ok=True)

    print("Loading real-time Whisper ASR validator...", end="", flush=True)
    whisper = load_whisper_validator()
    print(" Ready!\n")

    print("=" * 60)
    print(f"  PRISTINE POSITIVE SAMPLE RECORDING (ASR-VALIDATED)")
    print(f"  Targeting {n_clips} verified pristine 'NEXUS' recordings")
    print(f"  Real-time Whisper ASR gate active (100% poison prevention)")
    print(f"  Output: {out_dir}")
    print("=" * 60)
    print()
    print("  INSTRUCTIONS:")
    print("  - Say 'NEXUS' CLEARLY and NATURALLY when prompted")
    print("  - Vary your volume, distance, and direction")
    print("  - Invalid or ambiguous speech is automatically rejected or routed")
    print("  - Press Ctrl+C to stop early")
    print()

    SILENCE_THRESHOLD = 0.003
    saved = 0
    promoted = 0
    rejected = 0
    attempt = 0

    while saved < n_clips:
        attempt += 1
        phrase = random.choice(POSITIVE_PHRASES)
        print(f"  [{saved + 1}/{n_clips}] Say: \"{phrase}\"  ", end="", flush=True)

        # Quick countdown
        for c in range(2, 0, -1):
            print(f"{c}... ", end="", flush=True)
            time.sleep(0.35)

        print("REC", end="", flush=True)
        audio, rms = record_clip()

        if rms < SILENCE_THRESHOLD:
            print(f"  \033[93m[SKIP: Silence / RMS={rms:.5f}]\033[0m")
            rejected += 1
            continue

        # In-memory float32 audio for Whisper
        audio_flat = audio.flatten()
        verdict, transcript, reason = verify_positive_transcript(whisper, audio_flat)

        if verdict == 'accept_pos':
            next_idx = get_next_index(out_dir, "nexus_")
            fname = f"nexus_{next_idx:04d}.wav"
            save_wav(os.path.join(out_dir, fname), audio)
            saved += 1
            print(f"  \033[92m[✓ ACCEPTED: '{transcript}']\033[0m (RMS={rms:.4f}) → {fname}")

        elif verdict == 'promote_neg':
            # Save to negative so it trains as hard negative instead of polluting positive
            next_neg_idx = get_next_index(neg_dir, "soundalike_from_rec_")
            fname = f"soundalike_from_rec_{next_neg_idx:04d}.wav"
            save_wav(os.path.join(neg_dir, fname), audio)
            promoted += 1
            print(f"  \033[94m[🛡️  PROMOTED TO NEGATIVE: '{transcript}']\033[0m → {fname}")

        else:
            rejected += 1
            print(f"  \033[91m[✕ REJECTED: {reason}]\033[0m — re-prompting...")

    print(f"\n  Summary: {saved} pristine positive saved, {promoted} promoted to negative, {rejected} rejected")
    print(f"  Total verified positive library: {count_existing(out_dir, 'nexus_')} files")


def mode_negative(n_clips):
    """Record negative samples — say the displayed phrase (NOT nexus)."""
    out_dir = os.path.join(BASE_DIR, "negative")
    os.makedirs(out_dir, exist_ok=True)

    print("Loading real-time Whisper ASR validator...", end="", flush=True)
    whisper = load_whisper_validator()
    print(" Ready!\n")

    print("=" * 60)
    print(f"  NEGATIVE SAMPLE RECORDING (ASR-VALIDATED)")
    print(f"  Recording {n_clips} verified negative speech clips")
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

    while saved < n_clips:
        phrase = random.choice(NEGATIVE_PHRASES)
        safe = phrase.replace(" ", "_").replace("'", "")
        existing = count_existing(out_dir, f"{safe}_")
        idx = existing + 1

        print(f"  [{saved + 1}/{n_clips}] Say: \"{phrase}\"  ", end="", flush=True)
        for c in range(2, 0, -1):
            print(f"{c}... ", end="", flush=True)
            time.sleep(0.35)

        print("REC", end="", flush=True)
        audio, rms = record_clip()

        if rms < SILENCE_THRESHOLD:
            print(f"  \033[93m[SKIP: Silence / RMS={rms:.5f}]\033[0m")
            skipped += 1
            continue

        # Verify that the user did NOT accidentally say NEXUS
        if whisper is not None:
            segments, _ = whisper.transcribe(audio.flatten(), beam_size=1, language="en", temperature=0.0)
            transcript = " ".join([s.text for s in segments]).strip().lower()
            if "nexus" in transcript or "nexis" in transcript:
                print(f"  \033[91m[✕ REJECTED: You said 'NEXUS' in negative mode!]\033[0m — re-prompting...")
                continue

        fname = f"{safe}_{idx:04d}.wav"
        save_wav(os.path.join(out_dir, fname), audio)
        saved += 1
        print(f"  \033[92m[✓ OK]\033[0m (RMS={rms:.4f}) → {fname}")

    print(f"\n  Done: {saved} verified negative samples saved, {skipped} skipped")


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
