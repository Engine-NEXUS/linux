#!/usr/bin/env python3
"""
NEXUS Multi-Source Background Audio Generator
Generates realistic ambient, mechanical, and acoustic background noises:
  1. Fan, AC, HVAC motor rumble & airflow (low-frequency resonance + noise)
  2. Keyboard typing & mouse/trackpad clicks (transients + resonance)
  3. Office ambient / room reverb (pink & brownian noise with spectral filtering)
  4. Desk & household object sounds (pen taps, mug thuds, paper rustling)
  5. Multi-talker conversational babble (multiple voices overlayed at low volume)
Saves 16kHz mono 2.0s WAV files to wake_word_data/background/
"""

import os
import sys
import random
import asyncio
import subprocess
from pathlib import Path
import numpy as np
import scipy.io.wavfile as wavfile
from scipy.signal import butter, lfilter

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

ROOT = Path(__file__).resolve().parent.parent
BG_DIR = ROOT / "wake_word_data" / "background"
BG_DIR.mkdir(parents=True, exist_ok=True)

SR = 16000
DUR = 2.0
N_SAMPLES = int(SR * DUR)

def butter_lowpass(cutoff, fs=SR, order=4):
    nyq = 0.5 * fs
    normal_cutoff = min(cutoff / nyq, 0.99)
    b, a = butter(order, normal_cutoff, btype='low', analog=False)
    return b, a

def butter_bandpass(lowcut, highcut, fs=SR, order=3):
    nyq = 0.5 * fs
    low = max(lowcut / nyq, 0.01)
    high = min(highcut / nyq, 0.99)
    b, a = butter(order, [low, high], btype='band')
    return b, a

def generate_hvac_fan(idx):
    """Simulate laptop chassis fan, ceiling fan, or AC blower."""
    t = np.linspace(0, DUR, N_SAMPLES, endpoint=False)
    
    # Motor hum: fundamental + harmonics
    fund = random.choice([50.0, 60.0, 75.0, 90.0, 110.0, 140.0])
    hum = 0.15 * np.sin(2 * np.pi * fund * t)
    hum += 0.08 * np.sin(2 * np.pi * fund * 2 * t)
    hum += 0.04 * np.sin(2 * np.pi * fund * 3 * t)
    
    # Airflow noise (filtered white noise)
    noise = np.random.normal(0, 1, N_SAMPLES)
    b, a = butter_lowpass(random.uniform(250, 600))
    airflow = lfilter(b, a, noise)
    airflow /= (np.max(np.abs(airflow)) + 1e-6)
    
    audio = hum * random.uniform(0.1, 0.3) + airflow * random.uniform(0.3, 0.7)
    # Target RMS between 0.008 and 0.03
    audio = audio / (np.sqrt(np.mean(audio**2)) + 1e-6) * random.uniform(0.008, 0.025)
    return audio.astype(np.float32)

def generate_keystrokes(idx):
    """Simulate mechanical keyboard typing and mouse clicks."""
    audio = np.zeros(N_SAMPLES, dtype=np.float32)
    # Add 2 to 7 keystrokes in 2 seconds
    n_keys = random.randint(2, 7)
    
    for _ in range(n_keys):
        pos = random.randint(int(0.1 * SR), int(1.8 * SR))
        k_len = random.randint(int(0.02 * SR), int(0.08 * SR))
        
        # Transient burst + resonant ring
        t_k = np.linspace(0, k_len / SR, k_len)
        freq = random.uniform(1200, 4500)
        decay = np.exp(-t_k * random.uniform(50, 150))
        noise = np.random.normal(0, 1, k_len) * decay
        click = np.sin(2 * np.pi * freq * t_k) * decay + noise * 0.7
        click /= (np.max(np.abs(click)) + 1e-6)
        
        gain = random.uniform(0.03, 0.15)
        end_pos = min(pos + k_len, N_SAMPLES)
        audio[pos:end_pos] += (click[:end_pos - pos] * gain).astype(np.float32)
        
    # Ambient background floor
    bg = np.random.normal(0, 0.004, N_SAMPLES)
    audio += bg.astype(np.float32)
    return audio

def generate_room_ambience(idx):
    """Simulate ambient room acoustics, pink/brown noise, distant street noise."""
    # Pink noise approximation: sum of filtered noise
    noise = np.random.normal(0, 1, N_SAMPLES)
    b, a = butter_bandpass(random.uniform(80, 150), random.uniform(1500, 4000))
    audio = lfilter(b, a, noise)
    
    # Random slow volume breathing (people moving / distant cars)
    envelope = 1.0 + 0.3 * np.sin(2 * np.pi * random.uniform(0.2, 0.8) * np.linspace(0, DUR, N_SAMPLES))
    audio *= envelope
    
    audio = audio / (np.sqrt(np.mean(audio**2)) + 1e-6) * random.uniform(0.006, 0.022)
    return audio.astype(np.float32)

def generate_desk_objects(idx):
    """Simulate pen taps, cup clinks, paper rustles, chair shifts."""
    audio = np.random.normal(0, 0.003, N_SAMPLES).astype(np.float32)
    
    n_events = random.randint(1, 3)
    for _ in range(n_events):
        pos = random.randint(int(0.2 * SR), int(1.7 * SR))
        dur_s = random.uniform(0.05, 0.25)
        e_len = int(dur_s * SR)
        t_e = np.linspace(0, dur_s, e_len)
        
        obj_type = random.choice(["tap", "rustle", "thud"])
        if obj_type == "tap":
            freq = random.uniform(2000, 5000)
            burst = np.sin(2 * np.pi * freq * t_e) * np.exp(-t_e * 80)
        elif obj_type == "rustle":
            burst = np.random.normal(0, 1, e_len) * np.sin(np.pi * t_e / dur_s)
            b, a = butter_bandpass(1000, 5000)
            burst = lfilter(b, a, burst)
        else: # thud
            freq = random.uniform(100, 300)
            burst = np.sin(2 * np.pi * freq * t_e) * np.exp(-t_e * 30)
            
        burst /= (np.max(np.abs(burst)) + 1e-6)
        gain = random.uniform(0.02, 0.08)
        end_pos = min(pos + e_len, N_SAMPLES)
        audio[pos:end_pos] += (burst[:end_pos - pos] * gain).astype(np.float32)
        
    return audio

def save_clip(path, audio):
    audio_int16 = (np.clip(audio, -1.0, 1.0) * 32767).astype(np.int16)
    wavfile.write(str(path), SR, audio_int16)

def generate_all_background(count=300):
    print("=" * 65)
    print(f"  GENERATING {count} MULTI-SOURCE BACKGROUND SOUNDS")
    print("=" * 65)
    
    existing = len(list(BG_DIR.glob("*.wav")))
    print(f"Existing background clips: {existing}")
    
    generators = [
        ("hvac_fan", generate_hvac_fan, int(count * 0.30)),
        ("keystrokes", generate_keystrokes, int(count * 0.30)),
        ("room_ambience", generate_room_ambience, int(count * 0.25)),
        ("desk_objects", generate_desk_objects, int(count * 0.15)),
    ]
    
    created = 0
    for name, gen_fn, n in generators:
        for i in range(n):
            audio = gen_fn(i)
            filename = f"gen_bg_{name}_{i+1:04d}.wav"
            save_clip(BG_DIR / filename, audio)
            created += 1
            
    total = len(list(BG_DIR.glob("*.wav")))
    print(f"\nGenerated {created} new background clips.")
    print(f"Total background clips now in {BG_DIR}: {total}")
    print("=" * 65)

if __name__ == "__main__":
    generate_all_background(300)
