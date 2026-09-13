# NEXUS Wake Word v3 — Training Plan

> Target: **>=95% recall at <=1 false alarm per hour** (Alexa/Jarvis-level)

## Current State (v2)

| Metric | v2 | Target (v3) |
|--------|----|----|
| Model | 1-layer DNN (197K params, 415KB) | Conv-Attention (500K params, ~2MB) |
| Training data | 5000 TTS only | 15K+ real + TTS + augmented |
| Negatives | 5000 TTS soundalikes | 15K+ real + TTS + ACAV100M (200hrs) |
| Recall | ~100% on owner (7/7) | >=95% across all speakers |
| False positives | 0 in 4 min | <=1 per hour (24hr test) |
| Architecture | Flatten -> Linear -> Sigmoid | Conv1d -> Conv1d -> Attention -> Pool -> Linear |

## What Was Done Today

### 1. Recorded Real Voice Samples (98 positive + 93 negative + 20 background + 12 free)

| Category | Files | Size | Phrases |
|----------|-------|------|---------|
| Positive | 98 | 6.0 MB | "nexus", "hey nexus", "ok nexus", "nexus please", "nexus wake up" |
| Negative | 93 | 5.7 MB | 41+ soundalikes + competing wake words + common phrases |
| Free | 12 | 3.7 MB | Natural speech with "NEXUS" mixed in |
| Background | 20 | 1.2 MB | Room noise (with music/TV) |

**Recording script:** `scripts/record_wake_samples.py`
- 16kHz mono, 2s clips
- 3-second countdown per clip
- RMS quality check (skips silence)
- Supports: positive, negative, free, background modes

### 2. Augmented Samples 50x (10,013 total)

| Category | Original | Augmented | Total |
|----------|----------|-----------|-------|
| Positive | 98 | 4,900 | 4,998 |
| Negative | 93 | 4,650 | 4,743 |
| Free (as neg) | 12 | 240 | 252 |
| Background | 20 | - | 20 |
| **Total** | **223** | **9,790** | **10,013** |

**Augmentation script:** `scripts/augment_wake_samples.py`
- Speed perturbation (0.85x - 1.15x)
- Pitch shifting (-3 to +3 semitones)
- Volume scaling (0.3x - 2.0x)
- Background noise mixing (0-30 dB SNR)
- Room reverb (simple convolution)
- Time shifting (+/-200ms)

### 3. Created v3 Training Notebook

**File:** `train_nexus_oww_v3.ipynb`

Key improvements:
- **Conv-Attention architecture** (LiveKit-inspired)
  - Conv1d(96->128) + Conv1d(128->128) + MultiHeadAttention(4 heads) + GAP + Linear
  - ~500K parameters (2.5x more capacity than v2)
  - LiveKit benchmark: 0.08 FA/hr vs 8.50 for DNN
- **Combined training data**: Real voice + TTS + augmented + ACAV100M
- **SpecAugment**: Time + frequency masking on mel features
- **3-stage curriculum**: 20K + 5K + 5K steps with LR decay
- **Hard-negative mining**: 32 pos + 96 neg per batch
- **DET curve evaluation**: Tests at 16 thresholds, reports FA/hour
- **ONNX export**: Compatible with existing Rust runtime (same I/O names)

## v3 Training Pipeline

```
Step 1: Upload wake_word_data.zip to Colab
        (zip created from augmented/ folder)

Step 2: Run train_nexus_oww_v3.ipynb on Colab (T4 GPU, ~3-4 hrs)
        - Cell 1-6: Setup + downloads + patches
        - Cell 7: Generate 10K TTS clips (Piper)
        - Cell 8: Augment + featurize all clips
        - Cell 9: Define Conv-Attention model + SpecAugment
        - Cell 10: Load all features + train (30K steps)
        - Cell 11: Full evaluation with DET curve
        - Cell 12: Export ONNX + download

Step 3: Replace src-tauri/resources/oww/nexus.onnx with nexus_v3.onnx

Step 4: Runtime validation
        - cargo test --features wakeword-oww
        - Live mic test (cargo tauri dev)
        - 24-hour false positive test
```

## Research Findings (Alexa/Jarvis-level)

| System | Architecture | Positives | Negatives | FA/hr | Recall |
|--------|-------------|-----------|-----------|-------|--------|
| Alexa | DNN-HMM / CNN-LSTM / CRA | 1M+ real | 10K+ hrs | <1 | 95-99% |
| Jarvis (NVIDIA) | MatchboxNet (TCN) | User data | User data | <1 | 95%+ |
| openWakeWord DNN | FC-DNN (197K) | 10K TTS | 2K hrs | 8.50 | 68.6% |
| LiveKit Conv-Attn | Conv-Attention | 10K TTS | 2K hrs | 0.08 | 86.1% |
| **NEXUS v3** | **Conv-Attention** | **15K real+TTS** | **200 hrs** | **<=1** | **>=95%** |

## Next Steps

1. **Record more samples** — aim for 500+ real positives (currently 98)
   - Different rooms, distances, volumes, times of day
   - Whispered, shouted, normal, fast, slow
   - With TV/music/traffic background noise

2. **Run v3 training on Colab** — `train_nexus_oww_v3.ipynb`

3. **Runtime validation** — replace model, test live

4. **Continuous improvement** — admin brain can collect more samples over time
   and retrain periodically

## Files Created

| File | Purpose |
|------|---------|
| `scripts/record_wake_samples.py` | Live voice sample recorder (positive/negative/free/background) |
| `scripts/augment_wake_samples.py` | Data augmentation pipeline (50x multiplier) |
| `scripts/test_mic_live.py` | Mic health check (RMS, silence %, frequency) |
| `train_nexus_oww_v3.ipynb` | v3 training notebook (Conv-Attention + SpecAugment) |
| `wake_word_data/` | Recorded + augmented samples (gitignored) |
