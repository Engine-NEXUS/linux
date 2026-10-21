# Research & Technical Compendium: Unified Console Live Audio Telemetry, Debounced Neural Firing & Cloud Diagnostics Parity

**Date:** 2026-09-25  
**Domain:** Real-Time Speech Telemetry, Terminal UI Performance, Multi-Chunk Audio Processing & System Health Diagnostics  
**Status:** Validated & Synchronized  

---

## 1. Executive Summary

During testing of the `nexus start` unified runtime launcher against `nexus wake test`, three discrepancies were identified by user review:
1. **Absence of Real-Time Audio Telemetry in `nexus start`**: While `nexus wake test` rendered a live 12-slice speaker audio waveform (`Wave: [  ▂▄▆██▆▄▂  ]`) and status meter (`🎙️  VOICE [████████████████] 98.4% | RMS: 0.0245 (AGC 2.5x)`), `nexus start` lacked real-time visibility into whether the microphone was receiving speech energy or quiet ambient air.
2. **Double Triggering in Sequential 80ms Frames**: When saying "NEXUS", consecutive chunks (Chunk $N$ scoring 0.956 and Chunk $N+1$ scoring 0.992) triggered two identical wake events in the exact same second (`[19:50:33] TRIGGER #01` and `[19:50:33] TRIGGER #02`).
3. **Suppression of Boot Diagnostics**: The connection diagnostics table (`NEXUS Connection Diagnostics` from `src-tauri/src/diagnostics.rs` verifying Faster-Whisper STT, Edge-TTS/Piper, Cloudflare Worker `/health`, GitHub OAuth, and Google OAuth) was inadvertently filtered out along with verbose internal engine logs.

This technical paper documents the mathematical modeling, architecture, and verification of the streaming audio telemetry protocol, debounced candidate dispatch, and non-blocking in-place console rendering.

---

## 2. Theoretical Analysis & Root Causes

### 2.1 Acoustic Telemetry & Terminal I/O Disconnect
In `nexus wake test` (Python `scripts/test_wake_live.py`), the test script owns the PortAudio capture stream and stdout directly. Every 80ms chunk (1,280 samples at 16 kHz), the process computes:
$$\text{RMS} = \sqrt{\frac{1}{N} \sum_{i=0}^{N-1} x_i^2}$$
and updates the console using carriage return `\r`.

In contrast, in `nexus start`:
- PortAudio/CPAL audio capture runs inside the Rust desktop daemon (`nexus.exe`).
- Console output is captured by `scripts/run.ps1` via redirected stdout (`$LogDir\nexus_unified.log`).
- Previously, the Rust audio engine ran silently without emitting sub-second audio telemetry to disk, and `run.ps1` had a 500ms polling sleep interval which could not render continuous ~16 FPS visual waveforms.

### 2.2 Consecutive Neural Chunk Cascades (Double Triggering)
The physical utterance of the word "NEXUS" spans approximately 350ms to 550ms. At an 80ms chunk hop size, the trailing vowel and sibilant (`/-əs/`) remain present in the sliding 16-frame embedding buffer for 2 to 3 consecutive inference cycles:
- Cycle $k$: Features capture onset `/nɛk/` and peak of `/səs/` $\rightarrow P(k) = 0.956 \ge 0.68$ (Trigger 1).
- Cycle $k+1$ (80ms later): Features still hold the full articulation before sliding out $\rightarrow P(k+1) = 0.992 \ge 0.68$ (Trigger 2).

Without an atomic cooldown debounce in the message receiver loop (`rx.recv()` in `wakeword_oww.rs`), both candidates were dispatched to the frontend and console, firing two sequential notifications within 80 milliseconds.

### 2.3 AGC Double-Gain Amplification Flaw
In the audio pipeline:
$$\mathbf{x}_{\text{amp}} = \text{clamp}(\mathbf{x}_{\text{raw}} \cdot g_{\text{AGC}}, -1.0, 1.0)$$
In the initial implementation of `compute_waveform_string(&chunk, agc_gain)`, the function received the already amplified chunk $\mathbf{x}_{\text{amp}}$ and multiplied slice RMS by $g_{\text{AGC}}$ a second time:
$$\text{RMS}_{\text{slice}} = \sqrt{\frac{1}{M}\sum_{j} x_{\text{amp}, j}^2} \cdot g_{\text{AGC}} = \text{RMS}_{\text{raw}} \cdot g_{\text{AGC}}^2$$
With $g_{\text{AGC}} \approx 2.5\times$ to $20\times$, this squared gain factor ($6.25\times$ to $400\times$) clipped the waveform slices to maximum value (`█`) even on quiet room reverberation.

---

## 3. Engineering Implementation

### 3.1 High-Performance Telemetry Protocol (Rust Engine)
In `src-tauri/src/wakeword_oww.rs`:
- Implemented `compute_waveform_string(chunk: &[f32]) -> String`: Slices the 1,280 samples into 12 equal sub-bands, computes slice RMS directly against the AGC-normalized signal, and maps energy levels to unicode blocks (` ▂▃▄▅▆▇█`) scaled over nominal dynamic range $(0.0003 \le \text{RMS} \le 0.0220)$.
- Integrated dual-cadence telemetry emission:
  - **Speech Activity** ($\text{RMS} > 0.0006$ or $P > 0.05$): Emits every 2 frames (160ms) for high-fidelity animation:
    `audio-telemetry: wave=[  ▂▄▆██▆▄▂  ] prob=0.984 rms=0.0245 gain=2.5`
  - **Ambient Silence** ($\text{RMS} \le \text{silence\_threshold}$): Emits every 6 frames (480ms) to conserve I/O bandwidth:
    `audio-telemetry: wave=[            ] prob=0.000 rms=0.0002 gain=1.0`

### 3.2 1.5-Second Atomic Debouncing (`LAST_NEURAL_FIRE_MS`)
In `wakeword_oww.rs`:
```rust
static LAST_NEURAL_FIRE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

while let Ok(candidate) = rx.recv() {
    let now_ms = monotonic_ms();
    let last_fire = LAST_NEURAL_FIRE_MS.load(std::sync::atomic::Ordering::Relaxed);
    if now_ms.saturating_sub(last_fire) < 1500 {
        tracing::debug!("wake-word: consecutive candidate suppressed by 1.5s cooldown");
        continue;
    }
    LAST_NEURAL_FIRE_MS.store(now_ms, std::sync::atomic::Ordering::Relaxed);
    ...
}
```
This guarantees that consecutive high-confidence frames from a single spoken phrase are cleanly aggregated into exactly one trigger event.

### 3.3 Seamless In-Place Console Rendering & Diagnostics (`scripts/run.ps1`)
In `scripts/run.ps1`:
- Configured UTF-8 terminal encoding:
  `[Console]::OutputEncoding = [System.Text.Encoding]::UTF8`
- Reduced tail loop sleep from 500ms to 60ms (~16 FPS) for smooth waveform animation.
- Implemented `Clear-MeterLine` helper that safely clears the in-place meter line (`\r` + 95 spaces + `\r`) whenever an asynchronous log event (STT transcript, WAKE trigger banner, command execution, or diagnostics check) arrives.
- Un-suppressed the `NEXUS Connection Diagnostics` table (`^[╔║╠╚]`), rendering service connection statuses (Faster-Whisper STT, Edge-TTS/Piper, Worker, GitHub OAuth, Google OAuth) in distinct Cyan.

---

## 4. Verification & Validation Metrics

| Metric | Target | Result | Status |
| :--- | :--- | :--- | :--- |
| **Rust Wakeword Unit Tests** | 43/43 Passing | 43/43 Passed (0 failures) | PASS |
| **Console Waveform Display** | Live 12-Slice Waveform in `nexus start` | Identical to `nexus wake test` | PASS |
| **Single-Utterance Triggers** | Exactly 1 trigger per utterance | 1 trigger (0 double hits) | PASS |
| **Connection Diagnostics** | Cloud & local services visible at boot | Complete Cyan diagnostic box displayed | PASS |
| **Terminal Cleanliness** | Zero trailing characters on scroll | Cleared via `Clear-MeterLine` | PASS |

---

## 5. Architectural Synchronization
- Main Windows Repository: `Engine-NEXUS/WINDOWS` (`docs/research/wakeword/unified-console-audio-telemetry-and-diagnostics-parity-2026-09-25.md`)
- Research Repository: `Engine-NEXUS/NEXUS-PAPERS` (`wakeword/unified-console-audio-telemetry-and-diagnostics-parity-2026-09-25.md`)
