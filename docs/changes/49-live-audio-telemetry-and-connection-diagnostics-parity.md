# Change 49: Live Audio Telemetry, Debounced Triggering & Connection Diagnostics Parity

**Date:** 2026-09-25  
**Domain:** Runtime Console UX & Wakeword Engine  

---

## 1. Summary of Changes

- **`src-tauri/src/wakeword_oww.rs`**:
  - Implemented `compute_waveform_string(&chunk)` to generate 12-slice Unicode waveform bars (` ▂▃▄▅▆▇█`) from AGC-normalized chunks without double-gain multiplication.
  - Added real-time telemetry emission in both silent branches (`prob=0.000 rms={:.4} gain=1.0`) and active speech branches (`wave=[{}] prob={:.3} rms={:.4} gain={:.1}`).
  - Added atomic cooldown debounce `LAST_NEURAL_FIRE_MS` (1.5s window) in `rx.recv()` to eliminate duplicate triggers on multi-frame utterances.
  - Relaxed `try_device` quiet-room silence threshold from `0.0001` to `1e-6` so low-noise environments on Intel SST arrays do not cause false hardware resets.

- **`scripts/run.ps1`**:
  - Enforced UTF-8 console output encoding (`[Console]::OutputEncoding = [System.Text.Encoding]::UTF8`).
  - Added `Make-Meter` helper function for 16-character progress bars.
  - Added `Clear-MeterLine` helper called before any standard log message to prevent line corruption.
  - Rendered `audio-telemetry:` lines in-place via carriage return (`\r`) with green `🎙️  VOICE` / gray `💤 QUIET` indicators and cyan `Wave: [...]` bars.
  - Reduced polling interval from 500ms to 60ms for smooth real-time animation.
  - Un-suppressed `NEXUS Connection Diagnostics` and `9router health:` logs to display full startup service verification in Cyan.

---

## 2. Verification

- `cargo test --lib wakeword -- --test-threads=1`: 43/43 tests passed.
- `cargo build --release --features custom-protocol`: Clean release binary created.
