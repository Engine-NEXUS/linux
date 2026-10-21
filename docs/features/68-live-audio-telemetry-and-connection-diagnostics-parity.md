# Feature 68: Live Audio Telemetry, Trigger Debouncing & Connection Diagnostics Parity

**Date:** 2026-09-25  
**Domain:** Console UX, Acoustic Telemetry, System Diagnostics & Robust Triggering  
**Status:** Implemented & Verified  

---

## 1. Overview

Users launching NEXUS via `nexus start` now enjoy the exact same rich, real-time vocal feedback present in `nexus wake test`:
- **Real-Time Speaker Audio Waveform & Status Meter**: Displays a 12-slice animated speaker waveform (`Wave: [  ▂▄▆██▆▄▂  ]`) and live confidence/energy meter (`🎙️  VOICE [████████████████] 98.4% | RMS: 0.0245 (AGC  2.5x)`) that dynamically reflects vocal speech and quiet ambient states.
- **Connection Diagnostics on Startup**: Displays the full ASCII diagnostics table verifying Faster-Whisper (STT), Edge-TTS & Piper (TTS), Cloudflare Worker `/health`, GitHub OAuth, and Google OAuth status so service availability is immediately clear on boot.
- **Single-Trigger Neural Debouncing**: Eliminates duplicate trigger firing where consecutive 80ms chunks triggered redundant events in the same second (`TRIGGER #01` and `TRIGGER #02`).

---

## 2. Key Architecture & Enhancements

### 2.1 Streaming Audio Telemetry Pipeline
1. **Rust Engine (`src-tauri/src/wakeword_oww.rs`)**:
   - `compute_waveform_string(&chunk)`: Computes RMS across 12 equal time-domain slices of the AGC-normalized audio chunk and maps amplitude to Unicode block characters (` ▂▃▄▅▆▇█`).
   - Dispatches formatted `audio-telemetry: wave=[{}] prob={:.3} rms={:.4} gain={:.1}` lines.
   - Throttled dynamically: 160ms cadence during active speech ($\text{RMS} > 0.0006$ or $P > 0.05$) and 480ms cadence during quiet silence.
2. **Unified Launcher (`scripts/run.ps1`)**:
   - Configures console encoding to UTF-8 (`[Console]::OutputEncoding = [System.Text.Encoding]::UTF8`).
   - Polls log output at 60ms intervals for responsive ~16 FPS terminal rendering.
   - Implements `Make-Meter` (16-char progress bar `████░░░░`) and in-place carriage return `\r` line rendering.
   - Integrates `Clear-MeterLine` to cleanly erase the in-place status line whenever asynchronous log events (STT transcriptions, trigger banners, command executions, or error messages) are output.

### 2.2 Consecutive Neural Chunk Debouncing
- In `wakeword_oww.rs`, added an atomic timestamp tracker `LAST_NEURAL_FIRE_MS` to enforce a 1.5-second cooldown window on candidate triggers.
- Prevents subsequent frames of the same physical utterance from re-firing duplicate wake events.

### 2.3 Cloud & Local Connection Diagnostics
- Un-suppressed the diagnostics table in `scripts/run.ps1` so the formatted box (`╔|║|╠|╚`) from `src-tauri/src/diagnostics.rs` renders prominently in Cyan.
- Surfaces latency and connection status for STT, TTS, Worker, GitHub, and Google OAuth.

---

## 3. Verification & Testing

- **Rust Unit Tests**: Passed 43/43 tests cleanly (`cargo test --lib wakeword -- --test-threads=1`).
- **Release Build**: Compiled `nexus.exe` release binary with custom protocol enabled.
- **Console Rendering**: Validated in-place `\r` rendering with zero screen tearing or trailing character ghosting.
