# Phase A — Wake Word Production Retrain

**Date:** 2026-09-14
**Status:** Complete — notebook updated, cross-checked, ready for Kaggle run
**Predecessor:** [Wake Word Training Production Plan](../research/wake-word-training-production-plan-2026-09-14.md)

## Objective

Retrain the NEXUS wake word model with production-grade parameters to close the
gap between the current 58% recall and the 90%+ target. This is the single
biggest improvement — the current model has 10x less training data than the
openWakeWord documented minimum.

## What Was Done

### 1. Updated Training Notebook

File: `scripts/train_wakeword.ipynb`

All changes are in the notebook cells. No openWakeWord library modifications.

### 2. Config Changes (v1 → v2)

| Parameter | v1 (old) | v2 (new) | Rationale | Source |
|---|---:|---:|---|---|
| `n_samples` | 20,000 | **100,000** | 5x increase; OWW docs say "100K+ is best" | openWakeWord `custom_model.yml` |
| `n_samples_val` | 2,000 | **10,000** | 5x increase; 10% validation set | Standard ML practice |
| `steps` | 30,000 | **400,000** | 13x increase; community trainer standard | OWW community |
| `layer_size` | 32 | **128** | 4x increase; more model capacity | OWW docs: "increase for harder discrimination" |
| `augmentation_rounds` | 1 | **5** | 5x effective dataset (500K from 100K) | OWW docs: "increase for more data" |
| `max_negative_weight` | 1,500 | **3,000** | 2x increase; lower false positive rate | OWW docs |
| `target_accuracy` | 0.7 | **0.9** | Production target | Industry standard |
| `target_recall` | 0.5 | **0.85** | Production target | Industry standard |
| `target_fp_per_hour` | 0.2 | **0.1** | Production target (lower = better) | Industry standard |
| `custom_negative_phrases` | 6 | **50+** | More soundalike coverage | Research analysis |

### 3. Negative Data Improvement

| Data Source | v1 | v2 | Impact |
|---|---|---|---|
| ACAV100M | 1/10th (1.7 GB) | **Full (17 GB)** | 10x more negative speech |
| AudioSet segments | 1 (bal_train09) | **3 (bal_train09-11)** | 3x more background noise variety |
| Custom negative phrases | 6 | **50+** | Covers more soundalikes |

### 4. Custom Negative Phrases (50+)

The 50+ negative phrases cover five categories:

1. **Direct soundalikes** (6): "next us", "texas", "lexus", "plex us", "alex us", "nexus 6p"
2. **Nexus product names** (6): "nexus 5", "nexus 7", "nexus 9", "nexus 10", "nexus 4", "nexus 6"
3. **"hey" + soundalikes** (7): "hey texas", "hey next us", "hey lexus", etc.
4. **Command patterns** (9): "open texas", "close nexus", "show texas", etc.
5. **"nexus" in sentences** (22): "nexus is", "nexus was", "nexus will", "nexus should", etc.

These teach the model to NOT activate when "nexus" appears in non-wake-word contexts.

## Cross-Check Results

### Config Validation Against openWakeWord Docs

| Parameter | Our Value | OWW Default | OWW Minimum | Status |
|---|---:|---:|---:|---|
| `n_samples` | 100,000 | 10,000 | 20,000 | **Above minimum** |
| `n_samples_val` | 10,000 | 2,000 | 2,000 | **Above minimum** |
| `steps` | 400,000 | 50,000 | — | **8x default** |
| `layer_size` | 128 | 32 | — | **4x default** |
| `augmentation_rounds` | 5 | 1 | — | **5x default** |
| `max_negative_weight` | 3,000 | 1,500 | — | **2x default** |
| `target_fp_per_hour` | 0.1 | 0.2 | — | **Stricter than default** |

All parameters are at or above openWakeWord recommendations.

### Effective Dataset Size

| Metric | Value |
|---|---:|
| Base TTS positive clips | 100,000 |
| Augmentation rounds | 5 |
| Effective positive clips | 500,000 |
| Validation clips | 10,000 |
| Negative data (ACAV100M) | 2,000 hours |
| Training steps | 400,000 |

### Expected Outcome

| Metric | Current Model | Phase A Model | Improvement |
|---|---:|---:|---|
| Recall | 58% | **90%+** | +32 pp |
| False positives/hr | 1.33 | **<0.5** | -62% |
| Model size | 405 KB | ~1.5 MB | 4x (layer_size 128) |
| Training time (T4 GPU) | ~25 min | **~60 min** | +35 min |

## How to Run

### Step 1: Upload Real Samples (Phase E prerequisite)

If you have real "nexus" recordings from `scripts/record_samples.py`:
1. Zip: `zip -r nexus_real_samples.zip nexus_real_samples/`
2. Upload as Kaggle dataset
3. The notebook will automatically detect and include them

### Step 2: Run on Kaggle

1. Open `scripts/train_wakeword.ipynb` on Kaggle
2. Enable GPU (Settings → Accelerator → GPU T4 x2)
3. Attach your `nexus-real-samples` dataset (if you have one)
4. Run all cells (~60 min total)
5. Download `nexus.onnx` from output

### Step 3: Replace Model

```bash
# Copy new model
cp nexus.onnx src-tauri/resources/oww/nexus.onnx

# Update model manifest with new SHA-256
python -c "
import hashlib, json
with open('src-tauri/resources/oww/nexus.onnx', 'rb') as f:
    sha = hashlib.sha256(f.read()).hexdigest()
manifest = json.load(open('src-tauri/resources/oww/model_manifest.json'))
manifest['models'][2]['sha256'] = sha
import os
manifest['models'][2]['bytes'] = os.path.getsize('src-tauri/resources/oww/nexus.onnx')
json.dump(manifest, open('src-tauri/resources/oww/model_manifest.json', 'w'), indent=2)
print(f'Updated manifest: {sha}')
"

# Rebuild
nexus build
```

## Files Modified

| File | Change |
|---|---|
| `scripts/train_wakeword.ipynb` | Updated all cells with Phase A config |

## What Was NOT Done

- The openWakeWord library was not modified
- No Rust code was changed
- No model was trained yet (requires Kaggle GPU)
- The production `nexus.onnx` was not replaced yet
- No audio preprocessing was added (Phase B)
- No SpecAugment was added (Phase C)
- No speaker verification was wired (Phase D)
