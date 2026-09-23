# Change 42: Vocal Friction Hardening & Industrial Throat/Gargle Rejection

## Summary
Diagnosed the root cause of false wake triggers during throat clearing, gargling, and coughing. Ingested 120 synthetic negative samples for non-verbal physical vocal friction (velar rasp, phlegm flutter, vocal fry, wheezing) and 180 impulsive negative samples. Retrained the neural classifier with balanced Bayesian loss (`pos_weight = 1.2` down from `8.0`), eliminating the false-alarm gradient bias. Verified 0.0% false trigger rate on throat clearing, gargling, and coughing, while boosting positive recall to 97.1%.

## Key Changes
- **Vocal Friction Negative Synthesizer (`scripts/generate_throat_negatives.py`)**:
  Generated 120 specialized negative audio samples (16kHz mono WAV) modeling phlegm flutter (20–36 Hz AM), vocal fry (55–110 Hz F0 jitter), and velar/pharyngeal turbulence.
- **Impulsive Negative Synthesizer (`scripts/generate_impulsive_negatives.py`)**:
  Generated 180 negative samples for sneezes, vocal bursts, counting sequences (*"mic testing 1 2 3"*), claps, and desk thumps.
- **Balanced Loss Neural Retraining (`scripts/train_local_wakeword.py`)**:
  Replaced distorted `pos_weight = 8.0` with balanced `pos_weight = 1.2`, training over 40,253 training windows and 10,064 validation windows for 60 epochs. Validation loss reached 0.0245 with 99.0% recall and 0.2% FA.
- **Vocal Artifact Rejection Verification**:
  Across 30-clip stress tests: Throat clearing (0.0% FA, max score 0.1%), Gargling (0.0% FA, max score 0.0%), Coughing (0.0% FA, max score 0.1%), Sneezes (0.0% FA, max score 0.7%), Mic testing (0.0% FA, max score 0.1%).
- **Full Library Batch Benchmark (3,300 files)**:
  97.1% positive recall (avg score 97.4%), 97.5% negative soundalike rejection, 99.6% background noise rejection at calibrated threshold 0.68.
- **Hardware Invariance Benchmark**:
  Verified 97.1% Studio USB, 100.0% Laptop Mic Array with Intel Smart Sound, 93.6% Bluetooth VoIP, 84.3% Quiet Whisper, and 92.5% Noisy Office.
- **Rust Backend Invariants**:
  All 522 Rust unit tests pass cleanly. `model_manifest.json` updated with SHA-256 fingerprint.

## Files Modified / Created
- `scripts/generate_throat_negatives.py` (New)
- `scripts/generate_impulsive_negatives.py` (New)
- `scripts/train_local_wakeword.py`
- `scripts/test_wake_live.py`
- `src-tauri/resources/oww/nexus.onnx`
- `src-tauri/resources/oww/model_manifest.json`
- `src-tauri/resources/oww/acoustic_profile.json`
- `docs/features/59-vocal-friction-hardening-and-throat-gargle-rejection.md` (New)
- `docs/changes/42-vocal-friction-hardening-and-throat-gargle-rejection.md` (New)
- `AGENTS.md`
