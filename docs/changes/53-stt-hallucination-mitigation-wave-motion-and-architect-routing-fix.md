# Change 53: STT Hallucination Mitigation, Strict Wave Motion Rule & Intent Parser Tightening

**Date:** 2026-09-29  
**Domain:** STT, NLU Intent Parsing, Frontend Avatar Dynamics, Ghost Mode Stage  

---

## 1. Summary of Changes

### `src-tauri/src/intent_parser.rs`
- **Removed Truncated Architect False Positives**: Removed `if trimmed == "open-" || trimmed == "open"` in `is_architect_command` which was routing pauses and cut-off utterances to `OpenArchitect`.
- **Tightened `is_architect_fuzzy`**: Required both architecture soundalike keywords (`"architecture"`, `"architect"`, `"arch"`, `"arcade"`, `"octach"`, `"ark"`, `"cat"`) and diagram/mapping keywords (`"mapper"`, `"map"`, `"diagram"`, `"graph"`, `"remember"`, `"member"`, `"december"`). Eliminated greedy triggers matching common words like `"our"`, `"are"`, `"art"`, `"mac"`, or `"master"`.
- **Stray Punctuation Sanitization**: Added `strip_leading_stray_punctuation` to strip leading punctuation marks and single-character prefixes (e.g. `"S. Ghost mode."` $\rightarrow$ `"ghost mode"`).
- **Unit Tests**: Added `test_architect_false_positives_rejected` and `test_ghost_mode_stray_punctuation`. 181/181 intent parser tests passed.

### `frontend/src/avatar/Avatar.tsx`
- **Enforced Strict Wave Motion Rule**: Removed autonomous sinusoidal wave breathing during idle/rest states. Waves now rest statically at a low baseline target (`0.15`).
- **Lottie Playback Control**: Pauses the Lottie animation (`wavesAnimRef.current.pause()`) and dims container opacity to `0.35` when `src === "rest"`. Resumes playback (`setSpeed(1.0); play()`) with full opacity (`1.0`) strictly when speech activity occurs (`src === "mic" || src === "tts"`).

### `src-tauri/src/orchestrator.rs`
- **Suppressed Loading Usurpation**: Updated `show_loading` to immediately return `Ok(())` if `crate::ghost::session_active()` is true, preventing the top-right loading window from popping up during Ghost Mode.

### `frontend/src/App.tsx`, `frontend/src/net/orchestrator.ts`, `frontend/src/audio/recorder.ts`
- **Preserved Waves at Orb Screen Location**: Guarded `setLoadingVisible(true)` and `setVisible(false)` across orchestrator and architect event handlers with `!ghostActive`. In Ghost Mode, the wave visualizer stays anchored at the original wakeup orb coordinates.

### `src-tauri/src/stt.rs` & `src-tauri/src/stt_groq.rs`
- **Acoustic Pre-Gate**: Added audio length ($< 3200$ samples / 200ms) and RMS energy floor ($< 0.005$) checks in `transcribe_audio` and `transcribe_samples`. Audio buffers lacking vocal energy are immediately discarded as empty strings without making cloud API calls.
- **Anti-Hallucination Filter**: Expanded hallucination filters to catch noise fragments (`"from the"`, `"in the"`, `"of the"`, `"open drift"`, `"open breath"`), repetition loops, and foreign silence artifacts (`"cunzon usarlo"`, `"sarese manguer"`, `"tadrao vara"`).
- **Vocabulary Tuning**: Streamlined `NEXUS_VOCABULARY` in `stt_groq.rs` to avoid foreign acoustic bias. 23/23 STT unit tests passed.

### `src-tauri/src/mcp_client.rs`
- **Demoted Offline Warnings**: Changed log level from `warn!` to `debug!` when background pairing probes fail against `http://127.0.0.1:8765/mcp`, eliminating spam when the WhatsApp bridge is offline.

---

## 2. Verification

- **Rust Unit Tests**:
  - `cargo test --lib intent_parser -- --test-threads=1`: **181 / 181 Passed**
  - `cargo test --lib stt -- --test-threads=1`: **23 / 23 Passed**
  - `cargo test --lib wakeword -- --test-threads=1`: **46 / 46 Passed**
- **Frontend Test Suite**:
  - `npm --prefix frontend test`: **44 / 44 Passed**
- **Frontend Build**:
  - `npm --prefix frontend run build`: **Clean Vite production build**
- **Rust Release Build**:
  - `cargo build --release --features custom-protocol,admin-brain`: Compiled clean release binary.
