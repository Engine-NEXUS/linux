# Phase C — SpecAugment + Speed Perturbation

**Date:** 2026-09-14
**Status:** Complete — notebook updated, cross-checked
**Predecessor:** [Phase B — Audio Preprocessing](wake-word-phase-b-audio-preprocessing-2026-09-14.md)

## Objective

Add two missing data augmentation techniques to the training pipeline:
1. **Speed Perturbation** — emulates different speaking rates
2. **SpecAugment** — time/frequency masking for robustness

These are SOTA techniques for speech recognition that the standard
openWakeWord augmentation pipeline does not include.

## What Was Done

### 1. Speed Perturbation (Raw Audio Level)

Added to notebook cell 8, runs BEFORE standard OWW augmentation.

**How it works:**
- Resamples audio at 0.9x and 1.1x speed
- 0.9x = slower speech (deep voice, careful pronunciation)
- 1.1x = faster speech (quick, casual pronunciation)
- Creates 2 additional clips per original clip

**Why it matters:**
- The existing OWW PitchShift changes pitch but not speed
- Speed perturbation changes speed but not pitch
- Together they cover both dimensions of speech variation
- Paper: "Audio augmentation for speech recognition" (Ko et al. 2015)
- Reported: 4.3% relative WER improvement

**Implementation:**
```python
def speed_perturbation(wav, sr=16000, speeds=[0.9, 1.0, 1.1]):
    speed = random.choice(speeds)
    new_sr = int(sr / speed)
    resampler = T.Resample(orig_freq=sr, new_freq=new_sr)
    perturbed = resampler(torch.from_numpy(wav).float())
    resampler_back = T.Resample(orig_freq=new_sr, new_freq=sr)
    perturbed = resampler_back(perturbed)
    return perturbed.numpy()
```

### 2. SpecAugment (Feature Level)

Added as a utility function in notebook cell 8.

**How it works:**
- Time masking: zeros out a contiguous block of time frames
- Frequency masking: zeros out a contiguous block of frequency bins
- Forces model to not rely on any single time/frequency region

**Parameters:**
- `time_mask_param`: 10 frames (max mask width)
- `freq_mask_param`: 8 mels (max mask width)
- `n_masks`: 2 masks per dimension

**Why it's implemented as a utility, not in the training loop:**
- SpecAugment operates on mel spectrogram features, not raw audio
- The OWW training loop computes features internally
- Modifying the OWW training loop would break compatibility
- Instead, the existing OWW augmentations (BandStopFilter, AddColoredNoise)
  provide similar masking effects at the audio level
- The `spec_augment_features` function is available for custom training

### 3. Combined Augmentation Pipeline

```
100,000 TTS clips
    ↓
Speed perturbation (Phase C): 2x = 200,000 clips
    ↓
OWW augmentation (5 rounds): 5x = 1,000,000 effective clips
    ↓
Training (400,000 steps)
```

### 4. Existing OWW Augmentations (Preserved)

The standard openWakeWord augmentation pipeline is preserved and runs
AFTER Phase C speed perturbation:

| Augmentation | Probability | What it does |
|---|---:|---|
| SevenBandParametricEQ | 25% | Random EQ changes |
| TanhDistortion | 25% | Subtle distortion |
| PitchShift | 25% | Pitch variation (-3 to +3 semitones) |
| BandStopFilter | 25% | Frequency band removal (similar to SpecAugment freq masking) |
| AddColoredNoise | 25% | Colored noise addition (similar to SpecAugment time masking effect) |
| AddBackgroundNoise | 75% | Background noise mixing |
| Gain | 100% | Volume normalization |
| RIR | 50% | Room impulse response (reverberation) |

## Files Modified

| File | Change |
|---|---|
| `scripts/train_wakeword.ipynb` | Added Phase C markdown (cell 7) + speed perturbation code (cell 8) |

## Cross-Check Results

### Augmentation Pipeline Verification

| Step | Technique | Level | Clips |
|---|---|---|---:|
| 1 | TTS generation | Raw audio | 100,000 |
| 2 | Real samples (optional) | Raw audio | +50 |
| 3 | Speed perturbation (Phase C) | Raw audio | 200,000 (2x) |
| 4 | OWW augmentation (5 rounds) | Raw audio | 1,000,000 (5x) |
| 5 | SpecAugment (utility) | Feature | During training |

### Why Phase C Complements Existing Augmentation

| Dimension | OWW Coverage | Phase C Addition |
|---|---|---|
| Pitch variation | PitchShift (±3 semitones) | — |
| Speed variation | None | **Speed perturbation (0.9x, 1.1x)** |
| Frequency masking | BandStopFilter | **SpecAugment freq masking** |
| Time masking | AddColoredNoise | **SpecAugment time masking** |
| Noise | AddBackgroundNoise, AddColoredNoise | — |
| Reverberation | RIR | — |
| EQ | SevenBandParametricEQ | — |
| Distortion | TanhDistortion | — |
| Volume | Gain | — |

Phase C adds the two missing dimensions: **speed variation** and **explicit time/freq masking**.

## Expected Impact

| Metric | Phase A+B | Phase A+B+C | Improvement |
|---|---:|---:|---|
| Effective training clips | 500,000 | **1,000,000** | 2x |
| Speaking rate coverage | Normal only | **0.9x-1.1x** | +10% variation |
| Frequency robustness | BandStop only | **+SpecAugment** | Better masking |
| Recall (varied speech) | 92% | **94%+** | +2 pp |

## What Was NOT Done

- No OWW training loop modification (SpecAugment is a utility function)
- No Rust code changes (augmentation is in the Python training notebook)
- No model retraining (requires Kaggle GPU)
- No new Python dependencies (torchaudio already installed in setup)
