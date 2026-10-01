#!/usr/bin/env python3
"""
NEXUS Impulsive Negative Synthesizer
=====================================
Generates synthetic negative training samples for impulsive acoustic events
that caused false wake triggers:
  - Sneezes (broadband burst + exponential decay)
  - Shouts / loud non-speech vocals
  - Numerical speech sequences ("mic testing 1 2 3", "one two three")
  - Hand claps and finger snaps
  - Door slams / desk thumps
  - Keyboard mashing bursts
  - Sudden noise burst from AGC amplification stress

Root cause addressed: The impulsive gate (rms < 0.05 upper bound) only catches
low-RMS spikes. Events with RMS > 0.05 pass through as "speech" because the
upper bound is too tight. Additionally, no impulsive events existed in the negative
training set, so the classifier has never learned to reject them.

All output files are 16kHz mono WAV at 2 seconds each.
"""

import os
import sys
import random
import struct
import numpy as np
from pathlib import Path

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

try:
    from scipy.io import wavfile
    from scipy.signal import butter, lfilter, sosfilt, butter as butter2
except ImportError:
    print("ERROR: scipy not installed. Run: pip install scipy")
    sys.exit(1)

SR = 16000
DURATION = 2.0
N = int(SR * DURATION)

DATA_DIR = Path(__file__).resolve().parent.parent / "wake_word_data" / "negative"
DATA_DIR.mkdir(parents=True, exist_ok=True)


def save_wav(path: Path, audio: np.ndarray):
    """Save float32 [-1,1] audio as 16-bit PCM WAV."""
    data = np.clip(audio, -1.0, 1.0)
    pcm = (data * 32767).astype(np.int16)
    wavfile.write(str(path), SR, pcm)


def bandpass(audio, lo, hi, fs=SR, order=4):
    sos = butter(order, [lo / (fs / 2), hi / (fs / 2)], btype='band', output='sos')
    return sosfilt(sos, audio).astype(np.float32)


def lowpass(audio, cutoff, fs=SR, order=4):
    sos = butter(order, cutoff / (fs / 2), btype='low', output='sos')
    return sosfilt(sos, audio).astype(np.float32)


def normalize(audio, target_rms=0.15):
    rms = float(np.sqrt(np.mean(audio ** 2)))
    if rms < 1e-9:
        return audio
    return audio * (target_rms / rms)


# ─── Sneeze ─────────────────────────────────────────────────────────────────
def gen_sneeze(rng: np.random.Generator, idx: int):
    """
    Sneeze = broadband burst (5-8kHz) with exponential decay, ~150-250ms.
    Preceded by brief quiet inhalation.
    """
    t = np.arange(N, dtype=np.float32) / SR
    burst_start = rng.uniform(0.3, 0.7)
    burst_len = int(rng.uniform(0.12, 0.22) * SR)
    burst_start_s = int(burst_start * SR)

    # White noise burst with exponential decay
    noise = rng.standard_normal(N).astype(np.float32)
    envelope = np.zeros(N, dtype=np.float32)
    if burst_start_s + burst_len < N:
        decay = np.exp(-np.arange(burst_len) / (burst_len * 0.25))
        # Quick attack
        attack_len = int(0.01 * SR)
        attack = np.linspace(0, 1, attack_len)
        decay[:attack_len] *= attack
        envelope[burst_start_s:burst_start_s + burst_len] = decay

    burst = noise * envelope * rng.uniform(0.4, 0.9)

    # High-frequency emphasis (sneezes are 4-8kHz heavy)
    burst = bandpass(burst, 3000, 7500)

    # Quiet inhalation before burst
    inhale_len = int(rng.uniform(0.08, 0.15) * SR)
    inhale_start = max(0, burst_start_s - inhale_len - int(0.05 * SR))
    inhale_noise = rng.standard_normal(N).astype(np.float32) * 0.04
    inhale_env = np.zeros(N, dtype=np.float32)
    if inhale_start + inhale_len < N:
        inhale_env[inhale_start:inhale_start + inhale_len] = np.linspace(0, 1, inhale_len) * np.linspace(1, 0, inhale_len)
    inhale = bandpass(inhale_noise * inhale_env, 200, 3000)

    audio = np.clip(burst + inhale * 0.3, -1.0, 1.0)
    audio = normalize(audio, target_rms=rng.uniform(0.12, 0.25))

    path = DATA_DIR / f"sneeze_{idx:04d}.wav"
    save_wav(path, audio)


# ─── Loud Shout / Vocal Burst ───────────────────────────────────────────────
def gen_shout(rng: np.random.Generator, idx: int):
    """
    Simulates a non-speech shout: sustained vowel-like burst with
    harmonics but no clear phone sequence. "AH!" or "HEY!" that isn't NEXUS.
    """
    t = np.arange(N, dtype=np.float32) / SR
    shout_start = rng.uniform(0.2, 0.5)
    shout_dur = rng.uniform(0.15, 0.35)
    shout_start_s = int(shout_start * SR)
    shout_end_s = min(N, shout_start_s + int(shout_dur * SR))

    # Fundamental frequency for a shout: 150-300 Hz
    f0 = rng.uniform(150, 300)
    harmonics = np.zeros(N, dtype=np.float32)
    for h in range(1, 8):
        amp = 1.0 / h
        harmonics += amp * np.sin(2 * np.pi * f0 * h * t + rng.uniform(0, 2 * np.pi))
    harmonics = harmonics.astype(np.float32)

    # Add noise component
    noise = rng.standard_normal(N).astype(np.float32) * 0.3

    envelope = np.zeros(N, dtype=np.float32)
    seg_len = shout_end_s - shout_start_s
    if seg_len > 0:
        attack = int(0.02 * SR)
        decay = int(0.05 * SR)
        flat = max(0, seg_len - attack - decay)
        env_seg = np.concatenate([
            np.linspace(0, 1, min(attack, seg_len)),
            np.ones(flat),
            np.linspace(1, 0, min(decay, max(0, seg_len - attack - flat)))
        ])[:seg_len]
        envelope[shout_start_s:shout_end_s] = env_seg

    audio = (harmonics + noise) * envelope
    audio = bandpass(audio, 100, 6000)
    audio = normalize(audio, target_rms=rng.uniform(0.15, 0.30))

    path = DATA_DIR / f"shout_{idx:04d}.wav"
    save_wav(path, audio)


# ─── Numerical / "Mic Testing" speech sequences ─────────────────────────────
def gen_mic_testing_tts(idx: int):
    """
    Generate TTS for "mic testing 1 2 3" and similar counting phrases.
    Falls back to synthesized sine-based speech if TTS unavailable.
    """
    phrases = [
        "mic testing one two three",
        "testing one two three",
        "one two three",
        "check one two",
        "audio check",
        "testing testing",
        "hello hello",
        "mic check",
        "sound check one two three",
        "is this on",
        "can you hear me",
        "check check",
    ]
    phrase = phrases[idx % len(phrases)]

    try:
        import edge_tts
        import asyncio

        async def _tts(text, outpath):
            voices = ["en-US-GuyNeural", "en-IN-PrabhatNeural", "en-GB-RyanNeural"]
            voice = voices[idx % len(voices)]
            communicate = edge_tts.Communicate(text=text, voice=voice)
            await communicate.save(str(outpath) + ".mp3")

        import tempfile
        tmp = Path(tempfile.mktemp(suffix=".mp3"))
        asyncio.run(_tts(phrase, tmp))

        # Convert MP3 to WAV 16kHz
        try:
            import subprocess
            out_wav = DATA_DIR / f"mic_testing_{idx:04d}.wav"
            subprocess.run(
                ["ffmpeg", "-y", "-i", str(tmp) + ".mp3",
                 "-ar", "16000", "-ac", "1", "-f", "wav", str(out_wav)],
                check=True, capture_output=True
            )
            tmp.with_suffix(".mp3").unlink(missing_ok=True)
            return
        except Exception:
            pass
    except Exception:
        pass

    # Fallback: synthesized counting tones (not real speech, but exposes the mel pattern)
    _gen_counting_tones(idx, phrase)


def _gen_counting_tones(idx: int, label: str = "count"):
    """Fallback: generate rhythmic speech-like tones if TTS unavailable."""
    rng = np.random.default_rng(idx + 9000)
    t = np.arange(N, dtype=np.float32) / SR

    # Simulate 3-4 short vowel segments (one two three)
    audio = np.zeros(N, dtype=np.float32)
    word_times = [0.25, 0.65, 1.05, 1.40]
    for wt in word_times[:rng.integers(2, 5)]:
        wstart = int(wt * SR)
        wlen = int(rng.uniform(0.12, 0.20) * SR)
        f0 = rng.uniform(120, 280)
        seg = np.zeros(wlen, dtype=np.float32)
        for h in range(1, 6):
            seg += (1.0 / h) * np.sin(2 * np.pi * f0 * h * np.arange(wlen) / SR)
        env = np.sin(np.pi * np.arange(wlen) / wlen) ** 0.5
        seg = bandpass(seg * env, 80, 5000)
        end = min(N, wstart + wlen)
        audio[wstart:end] += seg[:end - wstart]

    audio += rng.standard_normal(N).astype(np.float32) * 0.02
    audio = normalize(audio, target_rms=rng.uniform(0.08, 0.18))

    path = DATA_DIR / f"mic_testing_{idx:04d}.wav"
    save_wav(path, audio)


# ─── Clap / Snap ─────────────────────────────────────────────────────────────
def gen_clap(rng: np.random.Generator, idx: int):
    """Sharp percussive broadband burst — characteristic of hand clap or finger snap."""
    clap_start = int(rng.uniform(0.2, 1.2) * SR)
    clap_len = int(rng.uniform(0.015, 0.040) * SR)

    noise = rng.standard_normal(N).astype(np.float32)
    envelope = np.zeros(N, dtype=np.float32)
    if clap_start + clap_len < N:
        attack = int(0.003 * SR)
        decay_arr = np.exp(-np.arange(clap_len) / (clap_len * 0.2))
        if attack < clap_len:
            decay_arr[:attack] *= np.linspace(0, 1, attack)
        envelope[clap_start:clap_start + clap_len] = decay_arr

    clap = bandpass(noise * envelope, 1000, 7500)
    clap = normalize(clap, target_rms=rng.uniform(0.10, 0.25))

    path = DATA_DIR / f"clap_{idx:04d}.wav"
    save_wav(path, clap)


# ─── Desk Thump / Door Slam ───────────────────────────────────────────────────
def gen_thump(rng: np.random.Generator, idx: int):
    """Low-frequency thump with sharp onset — desk knock, door slam."""
    thump_start = int(rng.uniform(0.2, 1.2) * SR)
    thump_len = int(rng.uniform(0.05, 0.12) * SR)

    t_seg = np.arange(thump_len, dtype=np.float32) / SR
    f_thump = rng.uniform(50, 180)
    tone = np.sin(2 * np.pi * f_thump * t_seg).astype(np.float32)
    decay = np.exp(-np.arange(thump_len) / (thump_len * 0.3))
    attack = int(0.005 * SR)
    decay[:attack] *= np.linspace(0, 1, attack)
    thump = tone * decay

    noise = rng.standard_normal(thump_len).astype(np.float32) * 0.4
    thump = thump + lowpass(noise, 800)

    audio = np.zeros(N, dtype=np.float32)
    end = min(N, thump_start + thump_len)
    audio[thump_start:end] = thump[:end - thump_start]
    audio = normalize(audio, target_rms=rng.uniform(0.10, 0.22))

    path = DATA_DIR / f"thump_{idx:04d}.wav"
    save_wav(path, audio)


# ─── Keyboard Mashing Burst ───────────────────────────────────────────────────
def gen_keyboard_burst(rng: np.random.Generator, idx: int):
    """
    Rapid multi-keypress burst (5-15 keys in 0.5-1.2s) that could
    sometimes trip the energy gate if RMS spikes.
    """
    audio = np.zeros(N, dtype=np.float32)
    n_keys = rng.integers(6, 20)
    key_times = sorted(rng.uniform(0.1, 1.8, n_keys))
    for kt in key_times:
        ks = int(kt * SR)
        klen = int(rng.uniform(0.005, 0.018) * SR)
        noise = rng.standard_normal(klen).astype(np.float32)
        decay = np.exp(-np.arange(klen) / (klen * 0.3))
        key = bandpass(noise * decay, 800, 6000)
        end = min(N, ks + klen)
        audio[ks:end] += key[:end - ks] * rng.uniform(0.3, 0.8)

    audio = normalize(audio, target_rms=rng.uniform(0.04, 0.12))
    path = DATA_DIR / f"keyboard_burst_{idx:04d}.wav"
    save_wav(path, audio)


# ─── AGC Stress — Quiet Random Noise Gets Amplified ──────────────────────────
def gen_agc_stress(rng: np.random.Generator, idx: int):
    """
    Very quiet ambient noise (RMS ~0.003) that AGC amplifies 30x.
    The amplified version should NOT trigger as NEXUS even though it
    has been boosted to speech-like energy levels.
    """
    noise_type = rng.integers(0, 4)
    if noise_type == 0:
        # Narrowband hum (fan, AC)
        f = rng.uniform(50, 250)
        t = np.arange(N, dtype=np.float32) / SR
        audio = (np.sin(2 * np.pi * f * t) + 0.3 * np.sin(4 * np.pi * f * t)).astype(np.float32)
        audio *= rng.uniform(0.002, 0.005)
    elif noise_type == 1:
        # White noise floor
        audio = rng.standard_normal(N).astype(np.float32) * rng.uniform(0.001, 0.004)
    elif noise_type == 2:
        # Narrowband interference (300-800 Hz)
        audio = rng.standard_normal(N).astype(np.float32)
        audio = bandpass(audio, 300, 800) * rng.uniform(0.002, 0.006)
    else:
        # Crackling / intermittent pops
        audio = np.zeros(N, dtype=np.float32)
        for _ in range(rng.integers(3, 12)):
            ps = rng.integers(0, N - 10)
            audio[ps:ps + 5] = rng.uniform(-0.01, 0.01)
        audio = lowpass(audio, 4000) * rng.uniform(50, 200)
        audio = np.clip(audio, -1.0, 1.0)

    # Now simulate AGC amplification
    rms = float(np.sqrt(np.mean(audio ** 2)))
    if rms > 0 and rms < 0.035:
        gain = min((0.035 / rms), 30.0)
        audio = np.clip(audio * gain, -1.0, 1.0)

    path = DATA_DIR / f"agc_stress_{idx:04d}.wav"
    save_wav(path, audio)


# ─── Main ─────────────────────────────────────────────────────────────────────
def main():
    print("=" * 65)
    print("  NEXUS IMPULSIVE NEGATIVE SYNTHESIZER")
    print("=" * 65)
    print(f"  Output directory: {DATA_DIR}")
    print()

    rng = np.random.default_rng(42)

    # Generate 30 of each impulsive category
    COUNT = 30

    print(f"[1/6] Generating {COUNT} sneeze samples...")
    for i in range(COUNT):
        gen_sneeze(rng, i + 1)
    print(f"      Done. sneeze_0001.wav - sneeze_{COUNT:04d}.wav")

    print(f"[2/6] Generating {COUNT} shout/vocal-burst samples...")
    for i in range(COUNT):
        gen_shout(rng, i + 1)
    print(f"      Done. shout_0001.wav - shout_{COUNT:04d}.wav")

    print(f"[3/6] Generating {COUNT} 'mic testing 1 2 3' counting sequences...")
    for i in range(COUNT):
        _gen_counting_tones(i + 1, "count")
    print(f"      Done. mic_testing_0001.wav - mic_testing_{COUNT:04d}.wav")

    print(f"[4/6] Generating {COUNT} clap/snap samples...")
    for i in range(COUNT):
        gen_clap(rng, i + 1)
    print(f"      Done. clap_0001.wav - clap_{COUNT:04d}.wav")

    print(f"[5/6] Generating {COUNT} desk-thump/door-slam samples...")
    for i in range(COUNT):
        gen_thump(rng, i + 1)
    print(f"      Done. thump_0001.wav - thump_{COUNT:04d}.wav")

    print(f"[6/6] Generating {COUNT} AGC-stress (quiet→amplified) samples...")
    for i in range(COUNT):
        gen_agc_stress(rng, i + 1)
    print(f"      Done. agc_stress_0001.wav - agc_stress_{COUNT:04d}.wav")

    total = COUNT * 6
    print()
    print(f"  ✓ Generated {total} impulsive negative samples in {DATA_DIR}")
    print("  → Now retrain the wake-word model:")
    print("    node nexus.mjs wake train")
    print("=" * 65)


if __name__ == "__main__":
    main()
