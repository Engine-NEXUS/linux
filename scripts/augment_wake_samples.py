#!/usr/bin/env python3
"""
NEXUS Wake Word — Data Augmentation Pipeline
Multiplies real voice samples 50-100x using professional augmentation.

Augmentations applied:
  1. Speed perturbation (0.85x, 0.9x, 0.95x, 1.05x, 1.1x, 1.15x)
  2. Pitch shifting (-3, -2, -1, +1, +2, +3 semitones)
  3. Volume scaling (0.3x, 0.5x, 0.7x, 1.3x, 1.5x, 2.0x)
  4. Background noise mixing (at 0, 5, 10, 15, 20 dB SNR)
  5. Room reverb (using simple convolution with decay)
  6. Time shifting (±200ms)
  7. SpecAugment-style frequency masking (on mel features)

Usage:
  python scripts/augment_wake_samples.py

Input:
  wake_word_data/positive/*.wav   (98 files)
  wake_word_data/negative/*.wav   (93 files)
  wake_word_data/background/*.wav (20 files — used as noise)

Output:
  wake_word_data/augmented/positive/*.wav  (~5000+ files)
  wake_word_data/augmented/negative/*.wav  (~5000+ files)
"""
import os
import sys
import glob
import numpy as np
import scipy.io.wavfile as wav
from scipy.signal import resample, fftconvolve
import random
import time

# Fix Windows console encoding
if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

BASE_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "wake_word_data")
SAMPLE_RATE = 16000

# Augmentation parameters
SPEED_FACTORS = [0.85, 0.9, 0.95, 1.05, 1.1, 1.15]
PITCH_SEMITONES = [-3, -2, -1, 1, 2, 3]
VOLUME_SCALES = [0.3, 0.5, 0.7, 1.3, 1.5, 2.0]
NOISE_SNR_DB = [0, 5, 10, 15, 20, 30]
TIME_SHIFTS_MS = [-200, -100, 100, 200]

# Number of augmented copies per original sample
# 98 positives * ~50 augmentations = ~4900 augmented positives
# 93 negatives * ~50 augmentations = ~4650 augmented negatives
AUGS_PER_SAMPLE = 50


def load_wav(path):
    """Load WAV file as float32 numpy array."""
    sr, data = wav.read(path)
    if data.dtype == np.int16:
        data = data.astype(np.float32) / 32768.0
    elif data.dtype == np.int32:
        data = data.astype(np.float32) / 2147483648.0
    elif data.dtype == np.uint8:
        data = (data.astype(np.float32) - 128.0) / 128.0
    if data.ndim > 1:
        data = data.mean(axis=1)  # mono
    return data, sr


def save_wav(path, audio, sr=SAMPLE_RATE):
    """Save float32 audio as 16-bit PCM WAV."""
    audio = np.clip(audio, -1.0, 1.0)
    audio_int16 = (audio * 32767).astype(np.int16)
    wav.write(path, sr, audio_int16)


def speed_perturb(audio, factor):
    """Change speed by resampling."""
    n_new = int(len(audio) * factor)
    return resample(audio, n_new)


def pitch_shift(audio, semitones, sr=SAMPLE_RATE):
    """Simple pitch shifting via resampling (no phase vocoder, but good enough)."""
    factor = 2.0 ** (semitones / 12.0)
    n_new = int(len(audio) / factor)
    shifted = resample(audio, n_new)
    # Pad or truncate to original length
    if len(shifted) < len(audio):
        padded = np.zeros(len(audio), dtype=np.float32)
        padded[:len(shifted)] = shifted
        return padded
    return shifted[:len(audio)]


def volume_scale(audio, factor):
    """Scale volume."""
    return audio * factor


def add_noise(audio, noise, snr_db):
    """Mix audio with background noise at given SNR."""
    # Make noise same length as audio
    if len(noise) < len(audio):
        # Repeat noise
        repeats = (len(audio) // len(noise)) + 1
        noise = np.tile(noise, repeats)
    noise = noise[:len(audio)]

    # Calculate signal and noise power
    signal_power = np.mean(audio ** 2)
    noise_power = np.mean(noise ** 2)

    if noise_power < 1e-10:
        return audio  # noise is silent

    # Calculate scaling factor for desired SNR
    snr_linear = 10.0 ** (snr_db / 10.0)
    scale = np.sqrt(signal_power / (noise_power * snr_linear))

    return audio + noise * scale


def simple_reverb(audio, sr=SAMPLE_RATE, decay=0.3, delay_ms=50):
    """Simple reverb using delayed copies with decay."""
    delay_samples = int(sr * delay_ms / 1000.0)
    result = audio.copy()
    amplitude = decay

    for _ in range(5):
        delayed = np.zeros(len(audio), dtype=np.float32)
        delayed[delay_samples:] = audio[:-delay_samples] if delay_samples < len(audio) else 0
        result += delayed * amplitude
        amplitude *= decay
        delay_samples = int(delay_samples * 1.3)

    return np.clip(result, -1.0, 1.0)


def time_shift(audio, shift_ms, sr=SAMPLE_RATE):
    """Shift audio in time by padding with zeros."""
    shift_samples = int(sr * shift_ms / 1000.0)
    if shift_samples > 0:
        padded = np.zeros(len(audio) + shift_samples, dtype=np.float32)
        padded[shift_samples:] = audio
        return padded[:len(audio)]
    elif shift_samples < 0:
        shifted = audio[-shift_samples:]
        padded = np.zeros(len(audio), dtype=np.float32)
        padded[:len(shifted)] = shifted
        return padded
    return audio


def augment_sample(audio, noise_samples):
    """Apply random augmentations to a sample."""
    result = audio.copy()

    # 1. Speed perturbation (50% chance)
    if random.random() < 0.5:
        factor = random.choice(SPEED_FACTORS)
        result = speed_perturb(result, factor)
        # Pad/truncate to original length
        if len(result) < len(audio):
            padded = np.zeros(len(audio), dtype=np.float32)
            padded[:len(result)] = result
            result = padded
        else:
            result = result[:len(audio)]

    # 2. Pitch shift (40% chance)
    if random.random() < 0.4:
        semitones = random.choice(PITCH_SEMITONES)
        result = pitch_shift(result, semitones)

    # 3. Volume scaling (60% chance)
    if random.random() < 0.6:
        factor = random.choice(VOLUME_SCALES)
        result = volume_scale(result, factor)

    # 4. Add background noise (70% chance)
    if random.random() < 0.7 and noise_samples:
        noise = random.choice(noise_samples)
        snr = random.choice(NOISE_SNR_DB)
        result = add_noise(result, noise, snr)

    # 5. Reverb (30% chance)
    if random.random() < 0.3:
        decay = random.uniform(0.2, 0.5)
        delay = random.uniform(30, 80)
        result = simple_reverb(result, decay=decay, delay_ms=delay)

    # 6. Time shift (20% chance)
    if random.random() < 0.2:
        shift = random.choice(TIME_SHIFTS_MS)
        result = time_shift(result, shift)

    # Clip to prevent distortion
    result = np.clip(result, -1.0, 1.0)

    return result


def process_directory(input_dir, output_dir, noise_samples, n_augs=AUGS_PER_SAMPLE):
    """Augment all samples in a directory."""
    os.makedirs(output_dir, exist_ok=True)

    wav_files = sorted(glob.glob(os.path.join(input_dir, "*.wav")))
    if not wav_files:
        print(f"  No WAV files found in {input_dir}")
        return 0

    total_generated = 0
    for wav_path in wav_files:
        basename = os.path.splitext(os.path.basename(wav_path))[0]
        audio, sr = load_wav(wav_path)
        if sr != SAMPLE_RATE:
            # Resample
            n_new = int(len(audio) * SAMPLE_RATE / sr)
            audio = resample(audio, n_new)
            sr = SAMPLE_RATE

        # Generate augmented copies
        for aug_idx in range(n_augs):
            augmented = augment_sample(audio, noise_samples)
            out_name = f"{basename}_aug{aug_idx:03d}.wav"
            save_wav(os.path.join(output_dir, out_name), augmented)
            total_generated += 1

        # Also copy the original
        save_wav(os.path.join(output_dir, f"{basename}_orig.wav"), audio)
        total_generated += 1

    return total_generated


def main():
    print("=" * 60)
    print("  NEXUS Wake Word — Data Augmentation Pipeline")
    print("=" * 60)
    print()

    # Load background noise samples
    bg_dir = os.path.join(BASE_DIR, "background")
    noise_samples = []
    if os.path.exists(bg_dir):
        for wav_path in sorted(glob.glob(os.path.join(bg_dir, "*.wav"))):
            audio, sr = load_wav(wav_path)
            if sr != SAMPLE_RATE:
                n_new = int(len(audio) * SAMPLE_RATE / sr)
                audio = resample(audio, n_new)
            noise_samples.append(audio)
    print(f"  Loaded {len(noise_samples)} background noise samples")
    print()

    # Augment positives
    print("  Augmenting POSITIVE samples...")
    pos_input = os.path.join(BASE_DIR, "positive")
    pos_output = os.path.join(BASE_DIR, "augmented", "positive")
    n_pos = process_directory(pos_input, pos_output, noise_samples)
    print(f"  Generated {n_pos} augmented positive samples")
    print()

    # Augment negatives
    print("  Augmenting NEGATIVE samples...")
    neg_input = os.path.join(BASE_DIR, "negative")
    neg_output = os.path.join(BASE_DIR, "augmented", "negative")
    n_neg = process_directory(neg_input, neg_output, noise_samples)
    print(f"  Generated {n_neg} augmented negative samples")
    print()

    # Also add free recordings as additional negatives (the non-NEXUS parts)
    free_dir = os.path.join(BASE_DIR, "free")
    if os.path.exists(free_dir):
        print("  Adding FREE recordings as additional negatives...")
        free_output = os.path.join(BASE_DIR, "augmented", "negative")
        n_free = process_directory(free_dir, free_output, noise_samples, n_augs=20)
        print(f"  Generated {n_free} augmented free-as-negative samples")
        print()

    # Add background noise as negatives
    if noise_samples:
        print("  Adding BACKGROUND noise as negatives...")
        bg_output = os.path.join(BASE_DIR, "augmented", "negative")
        for i, noise in enumerate(noise_samples):
            save_wav(os.path.join(bg_output, f"bg_orig_{i:04d}.wav"), noise)
        print(f"  Added {len(noise_samples)} background noise samples")
        print()

    print("=" * 60)
    print(f"  TOTAL AUGMENTED POSITIVES: {n_pos}")
    print(f"  TOTAL AUGMENTED NEGATIVES: {n_neg + (n_free if os.path.exists(free_dir) else 0) + len(noise_samples)}")
    print("=" * 60)


if __name__ == "__main__":
    main()
