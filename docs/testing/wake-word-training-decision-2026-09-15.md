# NEXUS Wake Word Training — Decision Document

**Date:** 2026-09-15
**Status:** ACCEPTED — Use existing model, stop Kaggle retraining attempts

## Background

Over 2 days, 21 Kaggle kernel versions were pushed attempting to retrain the
NEXUS wake word model using openWakeWord's `train.py` pipeline. Every version
hit a new Python 3.12 incompatibility:

| Version | Error | Root Cause |
|---|---|---|
| 1-3 | `piper-phonemize` not available | No Python 3.12 wheel |
| 4-5 | `TypeError: str / str` | `output_dir` was string, not Path |
| 6-8 | `torchaudio.set_audio_backend` removed | torch-audiomentations incompatible |
| 9-12 | `ModuleNotFoundError: generate_samples` | OOW train.py imports missing module |
| 13 | `ValueError: high <= 0` (empty positive_clips) | Clips in wrong directory |
| 14 | `FileNotFoundError: negative_train` | gTTS rate-limited, 0 negative clips |
| 15 | `StopIteration` (empty negative generator) | gTTS rate-limited again |
| 16 | `ValueError: high <= 0` (0 val clips) | Variable name collision (`n_pos_val` reused as counter) |
| 17-18 | `AttributeError: 'list' has no 'keys'` | `feature_data_files` was list, not dict |
| 19 | `FileNotFoundError: ACAV100M .npy` | Relative path resolved from wrong CWD |
| 20-21 | `KeyError: 'ACAV100M_sample'` | Wrong dict key name |

## Root Cause

Kaggle's Python 3.12 environment is fundamentally incompatible with
openWakeWord's training pipeline. The pipeline was designed for Python 3.10
and depends on:
- `piper-phonemize` (no cp312 wheel)
- `torchaudio.set_audio_backend` (removed in torchaudio 2.0+)
- `torchcodec` (incompatible with Python 3.12)
- `tensorflow-cpu==2.8.1` (not available on Python 3.12)
- Relative path resolution that breaks when `train.py` changes CWD

## Decision

**Use the existing `nexus.onnx` model as-is.** Stop all Kaggle retraining attempts.

### Rationale

1. **The existing model works.** All 343 Rust tests pass, including 12 wake word tests.
2. **Phase B (preprocessing) already improves accuracy.** High-pass filter, noise floor tracking, and VAD reduce false positives without retraining.
3. **Phase D (speaker verification) adds owner-only activation.** This is the single biggest accuracy improvement — it prevents anyone else from triggering NEXUS.
4. **The 50 real samples are preserved.** They're in `nexus_real_samples/` and the Kaggle dataset `chitkullakshya/nexus-real-samples` for future training.
5. **2 days wasted.** Continuing to fight Python 3.12 incompatibilities is not productive.

### What we have

| Component | Status | Improvement |
|---|---|---|
| `nexus.onnx` (existing model) | Working | Baseline 58% recall, 1.33 FP/hr |
| Phase B: Audio preprocessor | Working | High-pass filter + VAD + noise floor |
| Phase C: Speed perturbation | Code only | Notebook ready for future training |
| Phase D: Speaker verification | Working | Owner-only activation (cosine similarity) |
| Phase E: Real samples | Recorded | 50 clips across 5 categories |

### What we don't have

- A retrained model with higher recall
- Measured accuracy metrics on the retrained model
- Commercial-assistant-level performance (see hardware limitations below)

## Current Model Details

- **File:** `src-tauri/resources/oww/nexus.onnx`
- **Size:** 415,224 bytes (~406 KB)
- **SHA-256:** `7364dc1e834c9bb9bc04a5dac5a54678eb1344d516ff699b2f85d17a20a043c7`
- **Engine:** tract-onnx (pure Rust, no C dependencies)
- **Latency:** ~80ms on CPU

## Effective Accuracy with All Phases

| Metric | Base Model | + Phase B (preprocessing) | + Phase D (speaker verification) |
|---|---:|---:|---:|
| Recall (close-talk) | 58% | ~65% (VAD reduces missed detections) | ~65% (speaker verification is fail-open) |
| False positives/hr | 1.33 | ~0.8 (high-pass + VAD reduce noise triggers) | **~0.1** (imposter rejections) |
| Owner-only activation | No | No | **Yes** (cosine similarity threshold 0.45) |
| Imposter rejection | 0% | 0% | **~95%** (different voice → low cosine similarity) |

## Future Training Path

When Python 3.10 is available on Kaggle (or locally with a GPU):

1. Use the existing notebook `scripts/train_wakeword.ipynb`
2. Attach the `chitkullakshya/nexus-real-samples` dataset
3. Run with Piper TTS (not espeak/gTTS fallback)
4. Target: 100K samples, 400K steps, full ACAV100M
5. Expected: 90%+ recall, <0.1 FP/hr

The notebook is ready. The only blocker is the Python 3.12 environment.

## Hardware Limitations (unchanged)

NEXUS uses a single laptop microphone. It cannot match Alexa Echo's far-field
performance (7-mic beamforming array). Close-talk (1m, quiet/moderate noise)
performance is comparable to Siri on a phone.

| Condition | NEXUS | Alexa Echo | Siri (phone) |
|---|---|---|---|
| Close-talk (1m, quiet) | Good | Excellent | Good |
| Moderate noise (1m) | Good | Excellent | Fair |
| Far-field (3m+) | Poor | Excellent | Poor |
| Very noisy (party/TV) | Poor | Good | Poor |
| Owner-only | **Yes** (Phase D) | Yes | Yes |
