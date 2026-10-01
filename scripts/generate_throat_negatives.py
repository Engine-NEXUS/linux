#!/usr/bin/env python3
"""
NEXUS Throat-Clearing, Gargle & Vocal Friction Negative Generator
================================================================
Generates high-fidelity acoustic negative samples for the specific physical sounds
that cause false wake triggers:
  1. Throat clearing (velar friction [x] + glottal scraping [ʔ] + phlegm flutter)
  2. Gargling (saliva/phlegm oscillation at 20-35 Hz AM + guttural friction)
  3. Dry coughing (explosive glottal burst + broadband lung wheeze)
  4. Vocal fry / creaky voice scraping (irregular subharmonic F0 with high jitter)
  5. Sputtering / lip trills / raspberry vocalizations

All samples are 16kHz mono WAV at 2.0 seconds, saved to wake_word_data/negative/
"""

import sys
import numpy as np
from pathlib import Path

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

try:
    from scipy.io import wavfile
    from scipy.signal import butter, sosfilt
except ImportError:
    print("ERROR: scipy required. Run: pip install scipy")
    sys.exit(1)

SR = 16000
DURATION = 2.0
N = int(SR * DURATION)

DATA_DIR = Path(__file__).resolve().parent.parent / "wake_word_data" / "negative"
DATA_DIR.mkdir(parents=True, exist_ok=True)


def save_wav(path: Path, audio: np.ndarray):
    """Save normalized float32 [-1, 1] as 16-bit PCM WAV."""
    data = np.clip(audio, -1.0, 1.0)
    pcm = (data * 32767).astype(np.int16)
    wavfile.write(str(path), SR, pcm)


def bandpass(audio, lo, hi, fs=SR, order=4):
    sos = butter(order, [lo / (fs / 2), min(hi, fs / 2 - 10) / (fs / 2)], btype='band', output='sos')
    return sosfilt(sos, audio).astype(np.float32)


def normalize(audio, target_rms=0.15):
    rms = float(np.sqrt(np.mean(audio ** 2)))
    if rms < 1e-9:
        return audio
    return audio * (target_rms / rms)


# ─── 1. Throat Clearing ───────────────────────────────────────────────────────
def gen_throat_clear(rng: np.random.Generator, idx: int):
    """
    Simulates clearing the throat:
    - Velar/pharyngeal friction in 1.5 - 4.5 kHz band
    - Subharmonic vocal fry (60-120 Hz) with heavy jitter
    - Phlegm flutter (22-38 Hz amplitude modulation)
    - 2-3 repeated clearing pulses (e.g. "ahem-ahem", "khh-khh")
    """
    t = (np.arange(N) / SR).astype(np.float32)
    audio = np.zeros(N, dtype=np.float32)

    num_pulses = rng.integers(1, 4)
    pulse_starts = np.sort(rng.uniform(0.2, 1.3, num_pulses))

    for p_start in pulse_starts:
        p_dur = rng.uniform(0.18, 0.40)
        p_start_s = int(p_start * SR)
        p_len = int(p_dur * SR)
        if p_start_s + p_len > N:
            p_len = N - p_start_s

        seg_t = (np.arange(p_len) / SR).astype(np.float32)

        # Phlegm flutter (20 - 36 Hz)
        flutter_hz = rng.uniform(20.0, 36.0)
        flutter = (0.5 + 0.5 * np.sin(2 * np.pi * flutter_hz * seg_t + rng.uniform(0, 2*np.pi))).astype(np.float32)

        # Vocal fry (subharmonic pulses)
        f0 = rng.uniform(70.0, 130.0)
        fry = np.sin(2 * np.pi * f0 * seg_t).astype(np.float32)
        for h in range(2, 6):
            fry += (1.0 / h) * np.sin(2 * np.pi * f0 * h * seg_t + rng.uniform(0, 2*np.pi)).astype(np.float32)

        # Velar turbulence (white noise through 1.2kHz - 4.5kHz bandpass)
        noise = rng.standard_normal(p_len).astype(np.float32)
        turbulence = bandpass(noise, rng.uniform(1200, 1800), rng.uniform(3800, 5000))

        # Combine
        mix = (fry * 0.35 + turbulence * 0.65) * flutter

        # Envelope: fast attack (20ms), sustains with flutter, then decays
        env = np.ones(p_len, dtype=np.float32)
        attack_len = min(int(0.03 * SR), p_len // 4)
        decay_len = min(int(0.08 * SR), p_len // 3)
        if attack_len > 0:
            env[:attack_len] = np.linspace(0, 1, attack_len)
        if decay_len > 0:
            env[-decay_len:] = np.linspace(1, 0, decay_len)

        audio[p_start_s:p_start_s + p_len] += (mix * env) * rng.uniform(0.6, 1.0)

    audio = normalize(audio, target_rms=rng.uniform(0.12, 0.28))
    save_wav(DATA_DIR / f"throat_clear_{idx:04d}.wav", audio)


# ─── 2. Gargling ─────────────────────────────────────────────────────────────
def gen_gargle(rng: np.random.Generator, idx: int):
    """
    Simulates gargling / saliva bubbling:
    - Continuous 24-34 Hz droplet fluttering
    - Low-frequency vocal resonance (120-250 Hz)
    - High-frequency liquid splashing/sloshing noise (2 - 6 kHz)
    - Sustained for 0.8 - 1.5 seconds
    """
    t = (np.arange(N) / SR).astype(np.float32)
    start_s = int(rng.uniform(0.15, 0.4) * SR)
    dur_s = int(rng.uniform(0.8, 1.4) * SR)
    dur_s = min(dur_s, N - start_s)

    seg_t = (np.arange(dur_s) / SR).astype(np.float32)

    # Bubbling flutter: 2-3 interacting low frequencies (e.g. 24 Hz + 31 Hz)
    f1 = rng.uniform(22.0, 28.0)
    f2 = rng.uniform(30.0, 36.0)
    bubble_am = (0.4 + 0.3 * np.sin(2 * np.pi * f1 * seg_t) + 0.3 * np.sin(2 * np.pi * f2 * seg_t)).astype(np.float32)

    # Liquid resonance
    vocal = np.sin(2 * np.pi * rng.uniform(130, 220) * seg_t).astype(np.float32)
    noise = rng.standard_normal(dur_s).astype(np.float32)
    splash = bandpass(noise, 2200, 6200)

    gargle_body = (vocal * 0.4 + splash * 0.6) * bubble_am

    # Smooth fade-in and fade-out
    fade = int(0.08 * SR)
    env = np.ones(dur_s, dtype=np.float32)
    if fade * 2 < dur_s:
        env[:fade] = np.linspace(0, 1, fade)
        env[-fade:] = np.linspace(1, 0, fade)

    audio = np.zeros(N, dtype=np.float32)
    audio[start_s:start_s + dur_s] = gargle_body * env
    audio = normalize(audio, target_rms=rng.uniform(0.10, 0.22))
    save_wav(DATA_DIR / f"gargle_{idx:04d}.wav", audio)


# ─── 3. Coughing & Wheezing ──────────────────────────────────────────────────
def gen_cough(rng: np.random.Generator, idx: int):
    """
    Simulates a dry or chest cough:
    - Sharp explosive initial pop (< 15ms)
    - Followed by turbulent lung-wheeze decay (100 - 300ms)
    """
    t = (np.arange(N) / SR).astype(np.float32)
    audio = np.zeros(N, dtype=np.float32)

    num_hacks = rng.integers(1, 3)
    hack_starts = np.sort(rng.uniform(0.2, 1.1, num_hacks))

    for h_start in hack_starts:
        h_start_s = int(h_start * SR)
        h_len = int(rng.uniform(0.15, 0.30) * SR)
        if h_start_s + h_len > N:
            h_len = N - h_start_s

        seg_t = (np.arange(h_len) / SR).astype(np.float32)

        # Explosive burst
        noise = rng.standard_normal(h_len).astype(np.float32)
        burst = bandpass(noise, 400, 4500)
        decay = np.exp(-seg_t / rng.uniform(0.04, 0.09)).astype(np.float32)

        audio[h_start_s:h_start_s + h_len] += burst * decay

    audio = normalize(audio, target_rms=rng.uniform(0.14, 0.30))
    save_wav(DATA_DIR / f"cough_{idx:04d}.wav", audio)


# ─── 4. Vocal Fry / Creaky Throat Scraping ───────────────────────────────────
def gen_vocal_fry(rng: np.random.Generator, idx: int):
    """
    Simulates creaky voice / vocal fry scraping:
    - Highly aperiodic pulse train at 50-90 Hz
    - High cycle-to-cycle jitter (timing variation) and shimmer (amplitude variation)
    - Often produced during sighs, hesitation, or morning voice
    """
    audio = np.zeros(N, dtype=np.float32)
    start_s = int(rng.uniform(0.2, 0.5) * SR)
    dur_s = int(rng.uniform(0.5, 1.2) * SR)
    dur_s = min(dur_s, N - start_s)

    # Generate irregular pulse train
    cur = 0
    while cur < dur_s:
        period = int(rng.uniform(SR / 110.0, SR / 55.0))  # 55 - 110 Hz
        pulse_len = min(int(0.005 * SR), dur_s - cur)
        if pulse_len > 0:
            amp = rng.uniform(0.4, 1.0)
            pulse = np.sin(np.pi * np.arange(pulse_len) / pulse_len).astype(np.float32) * amp
            audio[start_s + cur:start_s + cur + pulse_len] += pulse
        cur += period

    # Add pharyngeal color
    audio = bandpass(audio, 80, 2800)
    audio = normalize(audio, target_rms=rng.uniform(0.08, 0.18))
    save_wav(DATA_DIR / f"vocal_fry_{idx:04d}.wav", audio)


# ─── Main ─────────────────────────────────────────────────────────────────────
def main():
    print("=" * 65)
    print("  NEXUS THROAT-CLEARING & VOCAL FRICTION NEGATIVE GENERATOR")
    print("=" * 65)
    print(f"  Target directory: {DATA_DIR}")
    print()

    rng = np.random.default_rng(999)
    COUNT = 30

    print(f"[1/4] Generating {COUNT} realistic throat-clearing samples...")
    for i in range(COUNT):
        gen_throat_clear(rng, i + 1)
    print(f"      Done: throat_clear_0001.wav - throat_clear_{COUNT:04d}.wav")

    print(f"[2/4] Generating {COUNT} gargling & saliva flutter samples...")
    for i in range(COUNT):
        gen_gargle(rng, i + 1)
    print(f"      Done: gargle_0001.wav - gargle_{COUNT:04d}.wav")

    print(f"[3/4] Generating {COUNT} coughing & lung wheeze samples...")
    for i in range(COUNT):
        gen_cough(rng, i + 1)
    print(f"      Done: cough_0001.wav - cough_{COUNT:04d}.wav")

    print(f"[4/4] Generating {COUNT} vocal fry & throat scraping samples...")
    for i in range(COUNT):
        gen_vocal_fry(rng, i + 1)
    print(f"      Done: vocal_fry_0001.wav - vocal_fry_{COUNT:04d}.wav")

    total = COUNT * 4
    print()
    print(f"  ✓ Total generated: {total} specialized vocal friction negatives")
    print("=" * 65)


if __name__ == "__main__":
    main()
