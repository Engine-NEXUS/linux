# Changelog 43 — Silence Phantom Elimination & Continuous Comfort Streaming

## Summary
Resolved the recurring issue where `nexus wake test` produced automatic wake triggers in complete silence without speech.

## Key Changes

### 1. Feature Extraction & Circular Buffers
- **`scripts/test_wake_live.py`**:
  - Implemented continuous comfort embedding advancement on silence chunks and VAD vetoes.
  - Pre-warmed and flushed embedding buffer with `comfort_embedding.npy` to eliminate LayerNorm zero-reset collapse.
  - Fixed impulsive sound filter baseline calculation and removed `rms < 0.05` cap.
  - Added smoothed AGC transition across 80ms chunks (`0.70 * prev + 0.30 * target`).
  - Added 2-frame temporal patience ($\ge 160\text{ms}$) for wake-word trigger firing.
- **`src-tauri/src/wakeword_oww.rs`**:
  - Added `COMFORT_EMBEDDING: [f32; 96]` const array representing ambient background noise.
  - Updated `AudioFeatures::new` and `AudioFeatures::reset` to initialize buffers with comfort embeddings instead of zeros.
  - Added `AudioFeatures::push_comfort_frame(&mut self)` and connected it to silence, VAD, and impulsive veto paths.
  - Updated `AudioPreprocessor::process` impulsive sound gate to use adaptive baseline without ceiling caps.
  - Enforced multi-frame confirmation in `calculate_average()`.

### 2. Neural Classifier Retraining & Calibration
- **`scripts/train_local_wakeword.py`**:
  - Initialized output linear layer bias to $-4.0$ to ensure proper negative prior calibration.
  - Integrated comfort noise loading and streaming into `extract_windows_from_audio`.
  - Added 600 synthetic comfort/transient negative windows to `X_neg`.
  - Retrained 60 epochs with $\text{pos\_weight} = 1.2$, achieving $0.0293$ validation loss, $98.8\%$ recall, and $0.4\%$ false alarm rate.
  - Exported updated ONNX graph to `src-tauri/resources/oww/nexus.onnx`.

### 3. Cryptographic Fingerprints & Foundation Locks
- **`src-tauri/resources/oww/model_manifest.json`**:
  - Updated SHA-256 hash and byte length for `nexus.onnx`.
  - Preflight validation passed cleanly (`python scripts/wake_data_foundation.py validate`).

## Verification
- **Continuous Silence (80s)**: 0 triggers, $0.0000\%$ max score.
- **Impulsive Spikes in Silence**: 0 triggers, $0.0220\%$ max score.
- **Microphone Invariance**: Passed across all 5 profiles ($87.1\% - 100.0\%$).
- **Rust Engine Suite**: 43/43 tests passed.
