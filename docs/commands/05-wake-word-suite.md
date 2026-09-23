# NEXUS CLI — Apex Wake Word Suite & Hardware Invariance Commands

This document details the complete wake word subsystem suite under `nexus wake`: calibration, real-time testing, multi-device benchmarks, neural retraining, open-source noise ingestion, and audio recording.

---

## 1. `nexus wake probe`

### Description
Performs a 1.5-second ambient room acoustic scan and Fast Fourier Transform (FFT) spectral analysis on the host laptop's microphone. Detects low-frequency fan/chassis vibrations (such as Intel Smart Sound 113.3 Hz resonance) and automatically tunes the DSP parameters: highpass filter cutoff (80–160 Hz), hardware pre-gain (1.0x–3.0x), silence threshold, and impulsive noise gate (8.0x).

### Syntax
```powershell
nexus wake probe
# Alias:
nexus wake calibrate
```

### Generated Output
Saves a hardware-calibrated profile to `src-tauri/resources/oww/acoustic_profile.json` and `%APPDATA%/com.nexus.assistant/acoustic_profile.json`.

### Example Output
```text
=================================================================
  NEXUS MICROPHONE HARDWARE ACOUSTIC PROBER
=================================================================
  [•] Probing default capture device: 'Microphone Array (Intel® Smart Sound)'
  [•] Host API: WASAPI | Native Sample Rate: 48,000 Hz | Channels: 4
  [•] Recording 1.5s ambient room noise (remain silent)...

  FFT Spectral & Resonance Analysis:
  ─────────────────────────────────────────────────────────────────
  • Ambient Noise Floor   : 0.00012 RMS (-78.4 dBFS)
  • Peak Chassis Rumble   : 113.3 Hz (+22.3 dB prominence)
  • Dynamic DSP Tuning    :
      → Highpass Filter   : Auto-tuned to 128.3 Hz (fan hum purged)
      → Pre-Gain Factor   : Auto-tuned to 2.50x (quiet MEMS boost)
      → Silence Gate      : Auto-tuned to 0.00300 RMS
      → Impulsive Gate    : 8.0x (rejects coughs/throat-clears)
      → Trigger Threshold : 0.50

  ✓ Saved calibrated profile to %APPDATA%\com.nexus.assistant\acoustic_profile.json
=================================================================
```

---

## 2. `nexus wake test`

### Description
Launches a real-time live microphone audit in your terminal. Streams audio through the adaptive DSP pipeline (high-pass filter, AGC, circular Mel-spectrogram, embedding sliding window, and 128-dim neural classifier) and displays live confidence scoring, input gain, and visual energy bars.

### Syntax
```powershell
nexus wake test
# Custom threshold override:
nexus wake test --threshold 0.40
```

### Visual Interface
- **👂 Listening `[████████░░░░░░░░░░░░]  42.1%`**: Live confidence score.
- **🔔 `[TRIGGER #01]`**: Alerts in green when confidence exceeds the threshold. Immediately invokes `reset_after_trigger()` to flush the 16-frame embedding buffer, preventing phantom cascades.

---

## 3. `nexus wake test --batch`

### Description
Runs an automated batch evaluation across all 3,000 WAV files in the local wake word dataset to verify true positive recall, soundalike negative rejection, and background noise immunity.

### Syntax
```powershell
nexus wake test --batch
```

### Dataset Benchmark Results
```text
═════════════════════════════════════════════════════════════════
  📂 BATCH DATASET RECALL & DISCRIMINATION TEST
═════════════════════════════════════════════════════════════════
  Threshold: 0.50
  Category        Clips    Triggered    Rate       Avg Score  Result
  ────────────────────────────────────────────────────────────
  Positive (Recall) 560      541           96.6%      96.8%     ★ Flawless
  Negative (Reject) 1442     61             4.2%       4.8%     ★ Flawless
  Background Noise 998      3              0.3%       0.7%     ● Good
═════════════════════════════════════════════════════════════════
```

---

## 4. `nexus wake test --devices`

### Description
Executes a multi-device hardware invariance benchmark. Evaluates the model against 5 simulated hardware microphone responses (Studio Condenser, Laptop Array with Fan Rumble, Bluetooth Headset, Far-Field Whispering, and Noisy Office).

### Syntax
```powershell
nexus wake test --devices
# or:
nexus wake devices
```

### Multi-Device Benchmark Results
```text
══════════════════════════════════════════════════════════════════════
  🎙️  NEXUS MULTI-DEVICE MICROPHONE INVARIANCE BENCHMARK
══════════════════════════════════════════════════════════════════════
  Testing 560 positive 'NEXUS' calls across 5 hardware profiles (Threshold: 0.50)...

  Hardware Profile                              Detected   Recall     Status
  ────────────────────────────────────────────────────────────────────
  Studio USB Condenser (Flat 20Hz-20kHz)        541/560     96.6%     ★ Flawless
  Laptop Mic Array (113.3Hz Fan + Intel Smart Sound) 560/560    100.0%     ★ Flawless
  Bluetooth Headset (300-3400Hz Narrowband VoIP) 516/560     92.1%     ★ Flawless
  Far-Field / Quiet Whispering (0.25x Gain)     481/560     85.9%     ● Strong
  Noisy Office (Typing + HVAC Noise @ 15dB SNR) 521/560     93.0%     ★ Flawless
══════════════════════════════════════════════════════════════════════
  ✓ ALL HARDWARE PROFILES PASSED BENCHMARK WITH >80-95% RECALL!
══════════════════════════════════════════════════════════════════════
```

---

## 5. `nexus wake train`

### Description
Retrains the 128-dim wake word ONNX neural classifier (`src-tauri/resources/oww/nexus.onnx`). Applies 5 multi-device acoustic data augmentations (telecom bandpass, laptop fan resonance, distance attenuation, and ambient noise mixing) to all positive recordings, training with `BCEWithLogitsLoss(pos_weight=8.0)` and Cosine Annealing.

### Syntax
```powershell
nexus wake train
```

---

## 6. `nexus wake ingest`

### Description
Synthesizes and ingests 600 high-fidelity background noise clips across mechanical keyboard switches, 113.3 Hz chassis fan hum, office HVAC, domestic impulsive sounds, and narrowband telecom noise. Automatically passes every clip through `faster-whisper` anti-poisoning ASR screening to purge colliding clips.

### Syntax
```powershell
nexus wake ingest
```

---

## 7. `nexus wake record`

### Description
Interactive continuous voice sample recorder for gathering new training audio.

### Syntax
```powershell
# Record 300 positive wake word calls ("NEXUS"):
nexus wake record positive 300
nexus wake record 300

# Record 100 negative soundalikes ("next", "lexus", "texas"):
nexus wake record negative 100

# Record continuous free-form speech with natural pauses:
nexus wake record free 60

# Record ambient room noise without speech:
nexus wake record background 30
```

---

## 8. `nexus wake stats`

### Description
Displays sample counts and disk storage usage across all wake word audio categories (`positive`, `negative`, `background`, `free`).

### Syntax
```powershell
nexus wake stats
```
