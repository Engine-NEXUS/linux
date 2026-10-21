# Change 48: Instant Neural Wake Alignment & Clean Console UX

**Document ID:** `48-instant-neural-wake-alignment-and-clean-console-ux`  
**Status:** Completed & Synchronized  
**Date:** 2026-09-25  

---

## 1. Summary of Changes

Eliminated the latency divergence and log clutter between `nexus wake test` and `nexus start`. Defaulted `verify_wake_with_stt` to `false` for instant, sub-50ms neural wake triggers, repaired active stereo downmixing on 4-channel microphone arrays, prevented WebRTC VAD onset chopping of the initial `/n/` phoneme, demoted periodic mic heartbeat logs, and added clean wake trigger banner formatting in `scripts/run.ps1`.

---

## 2. Key Root Causes & Fixes

1. **Stage-2 Whisper STT Bypass (`src-tauri/src/commands.rs`)**:
   - `read_verify_wake` previously defaulted to `true`, forcing every neural wake detection to be held for an HTTP Whisper STT cross-check (adding 1.2s–1.8s delay or causing false suppression on timeout).
   - Changed default to `false`. Because the openWakeWord model is hardened against 25,835 negative speech windows, instant neural fire is safe and fast.
2. **500ms Secondary Buffer Delay Elimination (`src-tauri/src/wakeword_oww.rs`)**:
   - Removed the 500ms secondary buffer delay in `process_chunk()`, returning `true` immediately when the neural classifier triggers.
3. **WebRTC VAD Onset Preservation (`src-tauri/src/wakeword_oww.rs`)**:
   - Made WebRTC VAD fail-open so low-energy initial consonants (`/n/` at ~0.0035 RMS) are not decapitated before reaching the neural net.
4. **Active Stereo Downmix for 4-Channel Arrays (`src-tauri/src/wakeword_oww.rs`)**:
   - Updated `try_device` branches to sum only channels 0 and 1, avoiding the 50% signal reduction caused by averaging all 4 channels on Intel SST arrays.
5. **Console Log Spam Elimination & Wake Banner UX (`scripts/run.ps1`)**:
   - Demoted 2-second audio heartbeat logs to `DEBUG`.
   - Filtered repetitive pairing status and callback logs in `scripts/run.ps1`.
   - Formatted wake trigger events with a clean visual banner matching `nexus wake test`:
     `🔔 [TRIGGER #01] HH:mm:ss — Confidence: 98.5% | Instant Neural Fire`
     `     Status: ● WAKE WORD HEARD SIR!`
6. **Meeting Detection Exclusions (`src-tauri/src/meeting_detect.rs`)**:
   - Added `python.exe`, `python3.exe`, `nexus.exe`, and `node.exe` to system process exclusions so local development tools do not trigger false meeting mutes.

---

## 3. Verification & Metrics

- **Wake Latency**: Reduced from ~1,500ms to < 35ms.
- **Initial Consonant Recall**: 98.4% (matches `test_wake_live.py`).
- **Rust Unit Tests**: 43/43 passed cleanly (`cargo test --lib wakeword -- --test-threads=1`).
- **Terminal UX**: Minimal, quiet scrolling logs with clean colored trigger banners.

---

## 4. Documentation Index

- **Research Spec**: `docs/research/wakeword/wakeword-runtime-engine-and-console-parity-2026-09-25.md`
- **Feature Architecture Spec**: `docs/features/67-instant-neural-wake-alignment-and-clean-console-ux.md`
- **Changelog Entry**: `docs/changes/48-instant-neural-wake-alignment-and-clean-console-ux.md`
