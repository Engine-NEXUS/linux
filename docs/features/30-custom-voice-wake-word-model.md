# Feature 30: Custom Voice Wake Word Model & Operation Guide

## 1. Overview & Architecture

NEXUS uses **openWakeWord** (KWS) powered by a pure-Rust inference engine (`tract-onnx`). The system runs completely locally on-device with zero cloud latency, zero external API costs, and zero Python runtime overhead during wake detection.

### The 3-Stage Acoustic Pipeline

Every 80ms chunk (1,280 samples at 16 kHz) passes through:
1. **Audio Preprocessing & AGC:** Automatic Gain Control normalizes quiet, whispered, or loud speech to a standard target RMS (~0.03 / -30 dBFS).
2. **Acoustic Feature Extraction:**
   - `melspectrogram.onnx`: Converts raw audio to log-mel spectrogram frames.
   - `embedding_model.onnx`: Generates 96-dimensional acoustic embeddings over a 16-frame window (`[1, 16, 96]`).
3. **Custom Classifier (`nexus.onnx`):**
   - Multi-layer neural network trained specifically on the user's authentic vocal timbre and speech cadence.
   - Outputs a probability score in range [0.0, 1.0]. If the score exceeds the detection threshold, the engine transitions NEXUS to listening mode.

---

## 2. Changes Deployed in This Release

### A. Voice Dataset Creation
- **45 Real Voice Recordings:** Recorded authentic pronunciations of "nexus" in 16 kHz 16-bit mono PCM.
- **Audio Augmentation:** Audio samples augmented with pitch shifts, time stretching, room reverberation, and background noise to ensure robustness across different environments.

### B. Kaggle Training Pipeline & Compatibility Fixes
- **Torchaudio 2.x Modernization:** Patched obsolete `torchaudio.set_audio_backend()` calls and replaced broken `torchaudio.info()` with `soundfile.info()` to support modern Python 3.12+ environments.
- **SciPy Directivity Patch:** Fixed deprecated `sph_harm` spherical harmonics functions in `acoustics/directivity.py`.
- **Sample Rate Standardization:** Automatically converted all positive and negative audio samples to strict 16,000 Hz.
- **Stale Feature Cleanup:** Added automated purge of partial `.npy` cache files to prevent training dimension mismatches.
- **Opset 12 Normalization:** Replaced opset 18 `LayerNormalization` operators (incompatible with lightweight ONNX runtimes) with standard Linear + ReLU activations.

### C. Self-Contained ONNX Weight Embedding
- **The Problem:** Modern PyTorch (2.5+) exports external `.data` files by default (`nexus_v2.onnx.data`), leaving model weights detached from the graph and causing `Translating proto model to model: os error 2` during launch.
- **The Solution:** Implemented a single-file ONNX exporter that loads external weights and embeds all initializers directly into the ONNX protobuf `raw_data` with `data_location = DEFAULT`. The resulting `nexus.onnx` (~815 KB) is 100% self-contained with zero external dependencies.

### D. Rust Audio Engine (`src-tauri/src/wakeword_oww.rs`)
- **Native Path Resolution:** Upgraded model loader from in-memory stream reading (`model_for_read(&mut rdr)`) to native file path resolution (`tract_onnx::onnx().model_for_path(path)`), ensuring reliable platform loading.
- **Tensor Layout Alignment:** Configured the audio feature pipeline to feed `[1, 16, 96]` directly into the classifier.
- **Unit Test Suite:** Validated 5/5 tests passing via `cargo test --lib wakeword`.

---

## 3. How to Operate NEXUS

### Launching the Application

#### Option 1: Direct Release Executable (Fastest)
Run the compiled standalone executable from PowerShell or Command Prompt:
```powershell
.\src-tauri\target\release\nexus.exe
```

#### Option 2: Unified Developer Console
To see live color-coded logs (Rust audio events, WebView CDP logs, and STT state):
```powershell
powershell -File .\scripts\run.ps1
```

#### Option 3: Cross-Platform CLI
```bash
node nexus.mjs start
```

---

### Operating the Wake Word

1. **Say "Nexus"** at a normal conversational volume.
2. You can also say:
   - *"Nexus"*
   - *"Hey Nexus"*
   - *"Nexus wake up"*
3. **Visual Feedback:** The NEXUS orb on your screen will illuminate and transition to active listening mode.
4. **Speak Your Request:** Immediately follow with your command (e.g., *"Open YouTube"*, *"What's the weather"*, *"Search GitHub"*).

### Keyboard Fallback
If you are in a loud environment or prefer silent activation, press the global hotkey:
- Default: `Ctrl + Shift + Space`

---

## 4. Tuning Sensitivity (No Retraining Required)

If you find the wake word is triggering too easily or needs you to speak louder, you can adjust the detection threshold in `src-tauri/src/wakeword_oww.rs` (line ~478):

```rust
// Default threshold: 0.35
let threshold = 0.35f32;
```

- **Make it more sensitive (easier to wake):** Lower to `0.25` - `0.30`.
- **Make it more strict (reject background noise):** Raise to `0.45` - `0.55`.

After editing, rebuild with:
```powershell
cargo build --release --features custom-protocol
```

---

## 5. Verification & Testing

To run the automated verification suite against the installed model:

```powershell
cd src-tauri
cargo test --lib wakeword -- --nocapture
```

Expected output:
```
OK: melspectrogram.onnx (1087958 bytes)
OK: embedding_model.onnx (1326578 bytes)
OK: nexus.onnx (814834 bytes)
OK: nexus.onnx is a valid ONNX file
OK: WakeEngine initialized successfully with trained nexus.onnx
test result: ok. 5 passed; 0 failed; 0 ignored
```
