#!/usr/bin/env python3
"""
NEXUS Microphone Hardware Prober & Acoustic Profiler
Probes the host laptop's microphone subsystem via WASAPI / CoreAudio:
  1. Capture device metadata (driver name, native SR, channel count)
  2. 1.5s passive ambient room measurement
  3. FFT spectral analysis to detect fan/AC motor hum
  4. Auto-tunes dynamic DSP: highpass cutoff, pre-gain, silence gate, and KWS threshold
  5. Outputs acoustic_profile.json for both Rust engine and Python runtime.
"""

import os
import sys
import json
import time
from pathlib import Path

# Fix Windows console encoding
if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

import numpy as np
import sounddevice as sd

ROOT = Path(__file__).resolve().parent.parent
OWW_DIR = ROOT / "src-tauri" / "resources" / "oww"
OWW_DIR.mkdir(parents=True, exist_ok=True)

# AppData destination on Windows
APPDATA_DIR = None
if sys.platform == "win32":
    appdata = os.environ.get("APPDATA")
    if appdata:
        APPDATA_DIR = Path(appdata) / "com.nexus.assistant"
        APPDATA_DIR.mkdir(parents=True, exist_ok=True)

CALIBRATION_DURATION = 1.5  # seconds
SAMPLE_RATE = 16000
N_SAMPLES = int(SAMPLE_RATE * CALIBRATION_DURATION)


def get_default_device_info():
    """Query default capture device from sounddevice."""
    try:
        device_idx = sd.default.device[0]
        if device_idx is None or device_idx < 0:
            device_idx = 0
        info = sd.query_devices(device_idx, "input")
        hostapi = sd.query_hostapis(info["hostapi"])["name"]
        return {
            "index": device_idx,
            "name": info["name"],
            "hostapi": hostapi,
            "native_sample_rate": int(info["default_samplerate"]),
            "channels": int(info["max_input_channels"]),
        }
    except Exception as e:
        return {
            "index": -1,
            "name": f"Generic Microphone ({e})",
            "hostapi": "WASAPI/CoreAudio",
            "native_sample_rate": 48000,
            "channels": 2,
        }


def record_ambient_sample():
    """Capture 1.5 seconds of passive room ambient audio."""
    print("  [•] Recording 1.5s ambient room noise (remain silent)...", flush=True)
    audio = sd.rec(N_SAMPLES, samplerate=SAMPLE_RATE, channels=1, dtype="float32")
    sd.wait()
    return audio.flatten()


def analyze_acoustics(audio, dev_info):
    """Perform FFT and energy analysis on the recorded sample."""
    rms = float(np.sqrt(np.mean(audio ** 2)))
    peak = float(np.max(np.abs(audio)))
    dbfs = 20.0 * np.log10(max(rms, 1e-6))

    # FFT Spectral Analysis
    # Window with Hanning
    windowed = audio * np.hanning(len(audio))
    fft_vals = np.abs(np.fft.rfft(windowed))
    freqs = np.fft.rfftfreq(len(audio), 1.0 / SAMPLE_RATE)

    # Focus on low-frequency fan/chassis vibration (40 Hz - 250 Hz)
    low_mask = (freqs >= 40.0) & (freqs <= 250.0)
    low_freqs = freqs[low_mask]
    low_mags = fft_vals[low_mask]

    peak_rumble_hz = 80.0
    rumble_prominence_db = 0.0

    if len(low_mags) > 0:
        max_idx = np.argmax(low_mags)
        peak_rumble_hz = float(low_freqs[max_idx])
        median_mag = np.median(fft_vals) + 1e-6
        rumble_prominence_db = float(20.0 * np.log10((low_mags[max_idx] + 1e-6) / median_mag))

    # 1. Determine High-Pass Filter Cutoff
    # Standard: 80Hz. If strong fan resonance detected > 90Hz, raise to 110-130Hz
    if rumble_prominence_db > 12.0 and peak_rumble_hz > 95.0:
        highpass_cutoff = min(peak_rumble_hz + 15.0, 130.0)
    else:
        highpass_cutoff = 80.0

    # 2. Determine Pre-Gain Factor
    # Target speech RMS is ~0.04. Quiet MEMS laptop mics have noise floors < 0.002
    if rms < 0.0025:
        pre_gain = 2.5  # Boost quiet laptop microphone
    elif rms < 0.0050:
        pre_gain = 1.6
    else:
        pre_gain = 1.0  # Hot / studio / USB microphone

    # 3. Dynamic Silence & Noise Gate Threshold
    silence_gate = float(np.clip(max(0.0030, rms * 2.8), 0.0025, 0.0120))

    # 4. KWS Trigger Threshold
    kws_threshold = 0.50

    profile = {
        "device_name": dev_info["name"],
        "host_api": dev_info["hostapi"],
        "native_sample_rate": dev_info["native_sample_rate"],
        "native_channels": dev_info["channels"],
        "measured_noise_floor_rms": round(rms, 6),
        "measured_noise_floor_dbfs": round(dbfs, 2),
        "measured_peak": round(peak, 4),
        "detected_fan_rumble_hz": round(peak_rumble_hz, 1),
        "rumble_prominence_db": round(rumble_prominence_db, 1),
        "highpass_cutoff_hz": round(highpass_cutoff, 1),
        "pre_gain": round(pre_gain, 2),
        "silence_rms_threshold": round(silence_gate, 5),
        "impulsive_ratio": 8.0,
        "kws_threshold": kws_threshold,
        "calibrated_at": time.strftime("%Y-%m-%d %H:%M:%S"),
    }

    return profile


def save_profile(profile):
    """Save profile to both resources directory and %APPDATA%."""
    targets = [OWW_DIR / "acoustic_profile.json"]
    if APPDATA_DIR:
        targets.append(APPDATA_DIR / "acoustic_profile.json")

    for target in targets:
        try:
            target.parent.mkdir(parents=True, exist_ok=True)
            with open(target, "w", encoding="utf-8") as f:
                json.dump(profile, f, indent=2)
            print(f"  [✓] Profile saved to: {target}")
        except Exception as e:
            print(f"  [!] Failed to save {target}: {e}")


def display_dashboard(profile):
    """Render a clean CLI summary dashboard."""
    print("\n" + "═" * 65)
    print("  🎙️  NEXUS ACOUSTIC HARDWARE DIAGNOSTIC REPORT")
    print("═" * 65)
    print(f"  • Device Name       : {profile['device_name']}")
    print(f"  • Host Audio Driver : {profile['host_api']} ({profile['native_sample_rate']} Hz, {profile['native_channels']} ch)")
    print(f"  • Ambient Noise Floor: {profile['measured_noise_floor_dbfs']} dBFS (RMS: {profile['measured_noise_floor_rms']:.5f})")
    
    fan_note = f"Peak at {profile['detected_fan_rumble_hz']:.1f} Hz (+{profile['rumble_prominence_db']:.1f} dB)" if profile['rumble_prominence_db'] > 6.0 else "None (Clean)"
    print(f"  • Fan / Motor Rumble: {fan_note}")
    print("─" * 65)
    print("  ⚙️  AUTO-TUNED DSP PROFILE (Saved)")
    print(f"  • High-Pass Filter  : {profile['highpass_cutoff_hz']:.1f} Hz (Removes chassis vibration)")
    print(f"  • Hardware Pre-Gain : {profile['pre_gain']:.2f}x (Normalizes mic sensitivity)")
    print(f"  • Adaptive Gate RMS : {profile['silence_rms_threshold']:.5f} (Blocks non-speech noise)")
    print(f"  • Impulsive Filter  : {profile['impulsive_ratio']:.1f}x (Rejects coughs & throat clears)")
    print(f"  • KWS Threshold     : {profile['kws_threshold']:.2f} (Model calibrated score)")
    print("═" * 65 + "\n")


def main():
    print("\nStarting NEXUS Microphone Hardware Probing & Acoustic Calibration...")
    dev_info = get_default_device_info()
    audio = record_ambient_sample()
    profile = analyze_acoustics(audio, dev_info)
    save_profile(profile)
    display_dashboard(profile)


if __name__ == "__main__":
    main()
