# Changelog: Dual-Target Wake Word Balancing & Rust Engine Synchronization

## Changes Made (2026-09-25)

### 1. Training Pipeline & Feature Extraction
- **`scripts/generate_balanced_positives.py`**: Added multi-rate, multi-speaker sample generator with in-memory Whisper verification. Synthesized and verified 88 clean positive samples.
- **`scripts/train_local_wakeword.py`**:
  - Implemented `include_transitions=True` in `extract_windows_from_audio` to capture post-speech transition frames.
  - Aligned window sampling to active speech envelope.
  - Retrained with Binary Focal Loss (`gamma=2.0`, `pos_weight=2.0`) across 25,428 training samples.
  - Re-exported self-contained ONNX model (`nexus.onnx`).

### 2. Rust Backend (`src-tauri/`)
- **`wakeword_oww.rs`**:
  - Reduced startup grace period from 10 seconds to 1.5 seconds (`1500ms`), resolving the unresponsive boot dead-zone.
  - Set `vad_enabled: false` in `AudioPreprocessor::with_profile` to prevent speech onset decapitation before the neural model.
- **`resources/oww/model_manifest.json`**:
  - Updated SHA-256 checksum for `nexus.onnx` (`7c98741c8a0b4f594853db3567123309929ae0e7efdaad64683803a3154c094b`).

### 3. Verification & Build
- 4-suite audit benchmark passed (Problem phrase: 2.83% max, Fast negatives: 0.00% FA, Positive recall: 96.6% @ 0.68).
- 43/43 Rust unit tests passed (`cargo test --lib wakeword -- --test-threads=1`).
- Full release binary built at `src-tauri/target/release/nexus.exe` (50.1 MB).
