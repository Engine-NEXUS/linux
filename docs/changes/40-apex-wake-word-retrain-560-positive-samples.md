# 40: Apex Wake Word Evolution — 560 Clean Samples Retrain & Verification

**Date**: 2026-09-23  
**Scope**: openWakeWord ONNX Classifier, Faster-Whisper Anti-Poisoning Audit, Model Manifest Fingerprint

---

## 1. Overview & Context

To maximize wake word recall across challenging acoustic environments (whispers, 2–3m distance, morning voice, keyboard background noise), a recording session added over 100 new positive voice samples. Before training, the automated 4-tier anti-poisoning pipeline (`scripts/audit_positive_samples.py`) audited all takes to ensure zero corrupted or conversational speech entered the training tensors.

---

## 2. Audit & Training Results

### A. Anti-Poisoning Scraper Audit (`scripts/audit_positive_samples.py`)
- **Total Scanned**: 562 positive audio files
- **Passed (Verified NEXUS Phonemes)**: **560 pristine positive samples** (100% compliant)
- **Quarantined**: 2 non-nexus takes isolated to `wake_word_data/quarantined_bad_positive/`:
  - `nexus_0525.wav` (*"we'll get in excess"*)
  - `nexus_0566.wav` (*"we'll be next this"*)

### B. Neural Classifier Training (`scripts/train_local_wakeword.py`)
- **Positive Windows**: 1,680 windows (extracted from 560 pristine recordings)
- **Negative Windows**: 20,754 windows (1,442 multilingual soundalikes + 400 background noise clips)
- **Total Dataset**: 22,434 windows (17,947 train / 4,487 validation)
- **Loss Function**: `BCEWithLogitsLoss(pos_weight=8.0)` + Cosine Annealing scheduler (60 epochs)
- **Peak Validation Recall**: **98.2%**
- **False Alarm Rate**: **0.4% – 0.6%** (99.4%+ soundalike/background noise rejection)

### C. ONNX Graph Export & Verification
- Exported calibrated ONNX model with `SigmoidWrapper` to `src-tauri/resources/oww/nexus.onnx` (860,071 bytes).
- Model SHA256: `854c8a717d90c8f9a695ae3aa44c9cb4ddd721cecc87f01341291046a1f33633`.
- Model manifest updated via `scripts/wake_data_foundation.py fingerprint-models`.

---

## 3. Test & Verification Matrix
- `cargo test --lib test_nexus_classifier_tract_vs_onnxruntime`: **Passed (0 errors)**
- `cargo test --lib wakeword_oww::tests`: **43/43 passed (0 failed)**
- `cargo test --lib -- --test-threads=1`: **522/522 passed (0 failed)**
- `python scripts/wake_data_foundation.py validate`: **Passed (0 errors)**
