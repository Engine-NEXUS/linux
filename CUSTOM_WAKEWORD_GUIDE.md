# NEXUS Custom Voice Wake Word Guide

This document explains how to operate the newly deployed personalized wake word model in NEXUS and details all changes made to the training and inference pipeline.

---

## Quick Start: How to Run NEXUS

### 1. Launch NEXUS
From the root of the project:
```powershell
.\src-tauri\target\release\nexus.exe
```
Or to run with live color-coded logs:
```powershell
powershell -File .\scripts\run.ps1
```

### 2. Wake Word Activation
Speak aloud to your computer:
> **"Nexus"** or **"Hey Nexus"** or **"Nexus wake up"**

The on-screen orb will light up and transition to listening mode. Follow up with your voice command.

### 3. Keyboard Shortcut (Fallback)
Press `Ctrl + Shift + Space` at any time to activate NEXUS manually without voice.

---

## Deployed Changes Summary

1. **Acoustic Dataset:**
   - Collected 45 personalized voice samples of the wake word "nexus" in 16 kHz 16-bit mono.
   - Applied room reverberation, background noise, pitch shifts, and speed perturbations.

2. **Kaggle Training Pipeline Compatibility Fixes:**
   - Replaced obsolete `torchaudio.set_audio_backend` and replaced `torchaudio.info` with `soundfile`.
   - Patched deprecated spherical harmonics (`sph_harm`) in `acoustics/directivity.py`.
   - Standardized all WAV files to strict 16,000 Hz sample rate.
   - Opset 12 ONNX export (removes incompatible LayerNormalization operators).

3. **Standalone Single-File ONNX Export:**
   - Fixed PyTorch 2.5+ external data export bug (`nexus_v2.onnx.data` missing error).
   - Injected weights directly into the ONNX protobuf `raw_data` for a 100% self-contained model (`815 KB`).

4. **Rust Inference Engine (`src-tauri/src/wakeword_oww.rs`):**
   - Upgraded model loader to `tract_onnx::onnx().model_for_path(path)` for native Windows path support.
   - Preserved Automatic Gain Control (AGC) for whispered/quiet voice support.
   - Verified 5/5 unit tests passing with `cargo test --lib wakeword`.

For full architecture details and sensitivity tuning instructions, see:
[`docs/features/30-custom-voice-wake-word-model.md`](docs/features/30-custom-voice-wake-word-model.md)
