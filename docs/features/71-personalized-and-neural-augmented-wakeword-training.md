# Personalized & Neural-Augmented Wake-Word Training

## 1. Executive Summary

This engineering specification details the acoustic training run combining real human microphone recordings with high-diversity neural voice augmentation (`edge-tts` across 13 distinct voice personas, accents, rates, and pitches) to achieve near-perfect wake-word precision on both 2-syllable standalone `"NEXUS"` and 3-syllable `"Hey NEXUS"`.

---

## 2. Methodology & Dataset Engineering

### 2.1 Real-Mic Human Voice Recording (`scripts/record_wake_samples.py`)
- Resolved a file indexing bug where `count_existing` instead of `get_next_index` caused overwriting of existing samples when gaps existed.
- Captured real human voice samples via the user's local microphone setup, validating natural acoustic room reflections, mic gain levels, and vocal timbres.
- Auto-promoted 7 soundalike utterances to hard negative speech training data (`soundalike_from_rec_0087.wav` to `soundalike_from_rec_0093.wav`).

### 2.2 Neural Multi-Voice Augmentation (`scripts/generate_neural_balanced_dataset.py`)
- Automated generation of 250 high-fidelity 16kHz mono positive samples using Microsoft Neural voices via `edge-tts`.
- Sampled across 13 diverse neural personas:
  - English (US): Guy, Jenny, Christopher, Aria, Eric
  - English (UK): Sonia, Ryan, Libby
  - English (India): Neerja, Prabhat
  - English (Australia/Canada/Ireland): Natasha, Clara, Emily
- Parameter grid swept:
  - Rate variations: `-20%`, `-10%`, `+0%`, `+10%`, `+20%`
  - Pitch offsets: `-5Hz`, `+0Hz`, `+5Hz`
  - Wakeword phrasings: `"nexus"`, `"hey nexus"`, `"okay nexus"`
- Every synthesized sample was verified through local Whisper STT transcription prior to promotion into `wake_word_data/positive/`.
- Expanded the pristine positive library from 178 to **432 samples** (94 real human recordings + 88 SAPI + 250 Microsoft Neural).

### 2.3 Binary Focal Training (`scripts/train_local_wakeword.py`)
- Set `pos_weight = 2.2` in `BinaryFocalLoss(gamma=2.0)`.
- Dataset partitioning: **41,183 total windows** (35,004 train + 6,179 val: 7,776 positive, 25,918 hard speech negatives, 6,489 comfort/noise windows).
- Trained across 45 epochs with dynamic LR decay and linear resampling on the fly.
- Exported clean ONNX classifier (860 KB) with SHA-256 `2de9887f7902befa9d1d2e8f2a02d98b9534b4cad159ebef9ddad73e97760f5a`.

---

## 3. Verification Benchmark Results

The retrained model underwent the comprehensive 4-point benchmark audit via `scripts/verify_hardened_model.py`:

| Audit Suite | Target | Result | Status |
| :--- | :--- | :--- | :--- |
| **Audit 1: Problem Phrase** | Fast conversational *"Documentation Created & Synchronized"* (`test_doc.wav`) | **Peak Score: 2.613%** (Threshold: 68.0%) | **Passed (0 Triggers)** |
| **Audit 2: Fast Speech Negatives** | 186 multi-syllabic clips (`-tion`, `-sion`, fast compounds) | **False Alarm Rate: 0.00% (0 / 186)** (Max Peak: 17.18%) | **Passed (100% Rejection)** |
| **Audit 3: Positive Recall** | 432 pristine recordings (Human + Neural + SAPI) | **Recall at 0.68: 98.6% (426 / 432)** (Average Max: 98.0%, Median: 99.7%) | **Passed** |
| **Audit 4: Vocal Friction** | 90 clips (Throat clearing, gargling, coughs, fry) | **False Alarm Rate: 0.00% (0 / 90)** (Max Peak: 8.13%) | **Passed (100% Rejection)** |
| **Rust Unit Tests** | `cargo test --lib wakeword -- --test-threads=1` | **43 / 43 Passed** | **Passed** |
| **Release Build** | `cargo build --release --features custom-protocol,admin-brain` | **Exit code: 0** (Optimized binary generated) | **Passed** |
