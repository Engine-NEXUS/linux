# Change 44: Training Data Poison Elimination, Focal Loss & Multi-Syllabic Speech Hardening

## Overview
Eliminated positive training data poisoning and resolved false triggers on fast multi-syllabic speech phrases like `"Documentation Created & Synchronized"` through automated Whisper ASR dataset purging, synthesis of 186 fast speech negatives, and Binary Focal Loss ($\gamma = 2.0$) optimization.

## Changes Made

### 1. Acoustic Data Quarantine & Soundalike Promotion
- `wake_word_data/positive/`: Audited all 560 clips using parallel `faster-whisper`.
- Quarantined 220+ conversational sentences and silent files to `wake_word_data/quarantined_bad_positive/`.
- Promoted 345 verified phonetic soundalikes (*"texas"*, *"open access"*, *"nixes"*, *"in excess"*) into `wake_word_data/negative/`.
- Retained 89 pristine positive NEXUS audio samples.

### 2. Multi-Syllabic Negative Synthesis
- `scripts/generate_fast_negatives.ps1`: Synthesized 186 fast speech negative audio files across words ending in `-tion`, `-sion`, `-ction`, and fast phrase compounds.

### 3. Binary Focal Loss & Canonical Training
- `scripts/train_local_wakeword.py`: Implemented `BinaryFocalLoss(gamma=2.0, pos_weight=2.0)` to eliminate easy negative gradient swamping.
- Hard speech negatives oversampled against background noise pool.
- Pinned `dynamo=False` and `opset_version=14` in `torch.onnx.export` to guarantee a self-contained, single-file ONNX binary for Rust Tract runtime.

### 4. Verification & Model Manifest
- `scripts/verify_hardened_model.py`: Created standardized 4-audit verification script.
- `src-tauri/resources/oww/nexus.onnx`: Replaced model with newly trained 860,081-byte self-contained ONNX model.
- `src-tauri/resources/oww/model_manifest.json`: Updated SHA-256 (`732215361337d0b1d729312c494c3c24e00f79c7ef72f8e48b319325099b1e6f`) and byte size.

## Verification Results
- **Problem Phrase (`test_doc.wav`)**: Peak score 18.755% (0 triggers, threshold 68.0%).
- **Fast Speech Negatives (186 clips)**: 0 / 186 false alarms (0.00%, peak 26.76%).
- **Pristine Positive NEXUS Recall (89 clips)**: 93.3% at 0.68, 97.8% at 0.50 (median 96.3%).
- **Vocal Friction Rejection (90 clips)**: 0 / 90 false alarms (0.00%, peak 9.24%).
- **Rust Unit Tests**: 43 / 43 tests passed cleanly (`cargo test --lib wakeword -- --test-threads=1`).
