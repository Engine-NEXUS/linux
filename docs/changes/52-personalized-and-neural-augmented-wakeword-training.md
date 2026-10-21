# Change 52: Personalized & Neural-Augmented Wake-Word Training

**Date:** 2026-09-25  
**Domain:** Wake Word, OpenWakeWord, Audio DSP, Neural Augmentation  

---

## 1. Summary of Changes

### `scripts/record_wake_samples.py`
- Fixed recording indexing bug where `count_existing` instead of `get_next_index` caused overwriting of existing user voice samples when gaps or promotions were present.
- Interactive user recording session safely collected 5 pristine real-mic human voice samples (`nexus_0091.wav` – `nexus_0095.wav`) and auto-promoted 7 soundalikes into negative training data (`soundalike_from_rec_0087.wav` – `soundalike_from_rec_0093.wav`).

### `scripts/generate_neural_balanced_dataset.py` (NEW)
- Built automated high-diversity neural audio generator using Microsoft Neural voices via `edge-tts`.
- Generated 250 verified 16kHz mono positive samples across 13 diverse neural voices (US, UK, Indian accents; male & female) spanning pitch offsets (-5Hz to +5Hz) and rate variations (-20% to +20%).
- Integrated local Whisper cross-validation to ensure all synthesized samples clearly contained the target wakeword phonemes.
- Expanded the positive library from 178 to 432 pristine samples (94 human mic recordings + 88 SAPI synthesized + 250 Microsoft Neural synthesized).

### `scripts/train_local_wakeword.py`
- Updated training configuration with `pos_weight = 2.2` in `BinaryFocalLoss(gamma=2.0)`.
- Balanced 2-syllable standalone `"NEXUS"` and 3-syllable `"Hey NEXUS"` representations.
- Retrained model over 45 epochs across 41,183 total windows (35,004 train + 6,179 val: 7,776 positive, 25,918 hard speech negatives, 6,489 comfort/noise windows).
- Exported clean ONNX model to `src-tauri/resources/oww/nexus.onnx` (SHA-256: `2de9887f7902befa9d1d2e8f2a02d98b9534b4cad159ebef9ddad73e97760f5a`).

### `src-tauri/resources/oww/model_manifest.json`
- Synchronized `nexus.onnx` entry with new SHA-256 (`2de9887f7902befa9d1d2e8f2a02d98b9534b4cad159ebef9ddad73e97760f5a`).

---

## 2. Verification

- **4-Point Benchmark Audit (`scripts/verify_hardened_model.py`)**:
  - **Audit 1 (Problem Phrase `"Documentation Created and Synchronized"`)**: **2.613% peak score** (Threshold 68.0%) → **0 Triggers** (Pass).
  - **Audit 2 (Fast Speech Negatives — 186 files)**: **0.00% False Alarms (0/186)**, max peak 17.18% (Pass).
  - **Audit 3 (Pristine Positive Recall — 432 files)**: **98.6% Recall at 0.68 (426/432)**, Average max: **98.0%**, Median confidence: **99.7%** (Pass).
  - **Audit 4 (Vocal Friction & Throat/Gargle — 90 files)**: **0.00% False Alarms (0/90)**, max peak 8.13% (Pass).
- **Rust Unit Tests**:
  - 43/43 wakeword tests pass cleanly (`cargo test --lib wakeword -- --test-threads=1`).
- **Release Build**:
  - `cargo build --release --features custom-protocol,admin-brain` compiled with 0 errors.
