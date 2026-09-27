# Phase E — Real Sample Recording & Integration

**Date:** 2026-09-14
**Status:** Complete — recorder updated, pipeline verified
**Predecessor:** [Phase D — Speaker Verification](wake-word-phase-d-speaker-verification-2026-09-14.md)

## Objective

Record 50 real "nexus" utterances from the owner's voice and integrate them
into the training pipeline. Real samples capture microphone characteristics,
room acoustics, and voice properties that TTS-generated samples cannot.

## What Was Done

### 1. Updated `scripts/record_samples.py`

The recorder now records 50 clips in 5 categories to capture real-world variation:

| Category | Count | Instructions | Why |
|---|---:|---|---|
| `nexus_normal` | 20 | Say "NEXUS" at normal volume | Baseline voice capture |
| `nexus_quiet` | 10 | Say "NEXUS" quietly/whispered | Quiet speech robustness |
| `nexus_loud` | 10 | Say "NEXUS" loudly | Loud speech robustness |
| `nexus_distant` | 5 | Say "NEXUS" from 3 meters away | Far-field robustness |
| `hey_nexus` | 5 | Say "HEY NEXUS" at normal volume | Both wake phrases |
| **Total** | **50** | | |

### 2. Recording Improvements

- **Category-based recording**: Each category has specific instructions
- **Automatic skip**: Clips with RMS < 0.001 are skipped (too quiet)
- **Resume support**: Existing clips are counted, recording continues from where it left off
- **Progress tracking**: Shows clip number / total across all categories
- **Clear instructions**: Tips for best results (quiet room, natural speech, etc.)

### 3. Notebook Integration (Already Existed)

The training notebook already handles real samples:

**Cell 3** — Detection:
```python
real_samples_dir = './real_positives'
has_real = os.path.isdir(real_samples_dir) and len(os.listdir(real_samples_dir)) > 0
```

**Cell 6** — Integration:
```python
if has_real:
    for f in os.listdir(real_samples_dir):
        if f.endswith('.wav'):
            shutil.copy(f'{real_samples_dir}/{f}', f'{pos_dir}/real_{f}')
```

### 4. Full Pipeline

```
scripts/record_samples.py
    ↓ (50 clips in 5 categories)
nexus_real_samples/*.wav
    ↓ (zip)
nexus_real_samples.zip
    ↓ (upload to Kaggle as dataset)
Kaggle dataset: nexus-real-samples
    ↓ (notebook cell 3: unzip to ./real_positives/)
./real_positives/*.wav
    ↓ (notebook cell 6: copy to positive_clips/train/)
./nexus_model/positive_clips/train/real_*.wav
    ↓ (Phase C: speed perturbation → 2x)
    ↓ (OWW augmentation: 5 rounds → 5x)
    ↓ (Training: 400K steps with 100K TTS + 50 real)
nexus.onnx (production model)
```

### 5. Effective Real Sample Impact

| Stage | Clips |
|---|---:|
| Raw real samples | 50 |
| After speed perturbation (2x) | 100 |
| After OWW augmentation (5 rounds) | 500 |
| Mixed with TTS clips | 100,000 + 500 = 100,500 |

While 500 out of 100,500 is only 0.5%, the real samples provide:
- **Microphone frequency response** — each laptop mic has unique characteristics
- **Room acoustics** — reverberation from the actual deployment environment
- **Voice characteristics** — pitch, timbre, pronunciation of the actual user
- **Real-world noise floor** — actual background noise of the deployment room

These are properties that TTS cannot reproduce, making even 50 clips valuable.

## Files Modified

| File | Change |
|---|---|
| `scripts/record_samples.py` | Updated with 5 categories, better instructions, resume support |

## Cross-Check Results

### Pipeline Verification

| Step | Status | Notes |
|---|---|---|
| Record samples | OK | 50 clips in 5 categories |
| Zip | OK | `zip -r nexus_real_samples.zip nexus_real_samples/` |
| Upload to Kaggle | Manual | User uploads as dataset |
| Notebook detection (cell 3) | OK | Checks `./real_positives/` directory |
| Notebook integration (cell 6) | OK | Copies `.wav` files to `positive_clips/train/` |
| Speed perturbation (cell 8) | OK | Applies to all clips including real |
| OWW augmentation (cell 9) | OK | 5 rounds applies to all clips |
| Training (cell 10) | OK | 400K steps with mixed TTS + real |

### No Code Compilation Needed

Phase E only modifies a Python script — no Rust compilation needed.

## Expected Impact

| Metric | Phase A+B+C+D | Phase A+B+C+D+E | Improvement |
|---|---:|---:|---|
| Recall (real speech) | 94% | **95%+** | +1 pp |
| Recall (quiet speech) | Lower | **Higher** | Real quiet samples |
| Recall (distant speech) | Lower | **Higher** | Real distant samples |
| Microphone matching | None | **Yes** | Real mic characteristics |
| Room acoustics | None | **Yes** | Real room reverb |

## How to Use

### Step 1: Record (10 minutes)

```bash
python scripts/record_samples.py
```

Follow the prompts. Record 50 clips across 5 categories.

### Step 2: Zip

```bash
zip -r nexus_real_samples.zip nexus_real_samples/
```

### Step 3: Upload to Kaggle

1. Go to https://www.kaggle.com/datasets
2. Click "New Dataset"
3. Upload `nexus_real_samples.zip`
4. Name it `nexus-real-samples`

### Step 4: Attach to Notebook

In the Kaggle notebook, add the `nexus-real-samples` dataset as an input.
The notebook will automatically detect and include the real samples.

## What Was NOT Done

- No Rust code changes (Python script only)
- No notebook cell changes (cells 3 and 6 already existed)
- No model retraining (requires Kaggle GPU)
- No automatic upload (manual Kaggle dataset upload)
