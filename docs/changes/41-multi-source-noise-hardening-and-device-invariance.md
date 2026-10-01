# Change 41: Multi-Source Noise Hardening & Hardware Invariance

## Summary
Ingested 600 multi-source background noise profiles (mechanical keyboard, chassis fan resonance, office HVAC, domestic impulsive clinks, narrowband telecom) screened through `faster-whisper` anti-poisoning ASR. Retrained wake word neural weights with multi-device acoustic augmentations (Bluetooth bandpass, laptop fan resonance, distance attenuation, and ambient noise mixing) and verified cross-device stability across 5 hardware microphone profiles.

## Key Changes
- **Noise Ingestion & Anti-Poisoning Gate (`scripts/ingest_opensource_noise.py`)**:
  Synthesized and ingested 600 high-fidelity noise profiles; audited all background takes with `faster-whisper`, automatically purging 2 colliding legacy clips. Total verified background library: 998 clips.
- **Multi-Device Positive Data Augmentation (`scripts/train_local_wakeword.py`)**:
  Applied Bluetooth narrowband bandpass (300-3400Hz), laptop fan rumble (55-145Hz), distance attenuation (0.25x-0.40x), and ambient mixing to positive takes. Expanded training set to 11,760 positive windows + 35,715 negative windows.
- **Microphone Invariance Benchmark (`scripts/test_device_invariance.py`)**:
  Created 5-profile hardware stress test: Studio USB (96.6%), Laptop Mic Array (100.0%), Bluetooth Headset (92.1%), Far-Field Whisper (85.9%), Noisy Office (93.0%).
- **Batch Evaluation & Verification (`scripts/test_wake_live.py`)**:
  Batch evaluation on 3,000 files achieved 96.6% positive recall, 95.8% negative soundalike rejection, and 99.7% background noise rejection.
- **Test Invariants**:
  All 522 Rust unit tests, 43 wake word tests, and 20 frontend Vitest tests pass cleanly.

## Files Modified / Created
- `scripts/ingest_opensource_noise.py`
- `scripts/train_local_wakeword.py`
- `scripts/test_device_invariance.py`
- `scripts/test_wake_live.py`
- `src-tauri/resources/oww/nexus.onnx`
- `src-tauri/resources/oww/model_manifest.json`
- `.gitignore`
- `docs/features/58-multi-source-noise-hardening-and-hardware-invariance.md`
- `docs/changes/41-multi-source-noise-hardening-and-device-invariance.md`
