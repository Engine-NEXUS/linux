# STT Hallucination Mitigation, Strict Wave Motion Rule & Intent Parser Tightening

## 1. Executive Summary

This engineering document specifies the resolution for four interconnected operational and perceptual issues in NEXUS:
1. **Unprompted Repository Errors**: Elimination of false-positive `OpenArchitect` routing that triggered `"No repository found sir. Open a repo in your browser or GitHub Desktop."` on truncated or unrecognized voice commands.
2. **Strict Wave Motion Rule**: Dynamic waves (`waves.json`) now oscillate **only** when mic audio is capturing (STT) or when NEXUS is synthesizing speech (TTS). During idle/rest states, wave bars remain fixed at a calm baseline (`0.15`) with the Lottie animation paused.
3. **Ghost Mode Visual Continuity**: Suppressed the top-right loading window (`loading.json`) usurpation during Ghost Mode, preserving the dynamic wave visualizer in-place at the wakeup orb's exact screen coordinates.
4. **STT Acoustic Energy Gate & Anti-Hallucination Filters**: Added length and RMS energy floor validation to filter out near-silent audio buffers before Groq Whisper dispatch, and expanded the hallucination filter to catch foreign language confabulations on acoustic silence.
5. **WhatsApp MCP Polling Noise Reduction**: Demoted background connection failure logs to `debug!` level when the WhatsApp bridge is offline.

---

## 2. Root Cause Analysis

### 2.1 The "No repository found sir" Bug
- **Discovery**: Users speaking natural voice commands or brief pauses (e.g. `"open..."`, `"ghost mode"`, or misrecognized speech) suddenly heard NEXUS respond: *"No repository found sir. Open a repo in your browser or GitHub Desktop."*
- **Mechanism**:
  1. In `src-tauri/src/intent_parser.rs`, the function `is_architect_command` contained:
     ```rust
     if trimmed == "open-" || trimmed == "open" {
         return true;
     }
     ```
     Any truncated command starting with `"open"` immediately evaluated to `ParsedIntent::OpenArchitect`.
  2. Additionally, `is_architect_fuzzy` matched any two-to-three word phrase starting with `"open "` where subsequent words began with common prefixes (`"our"`, `"are"`, `"art"`, `"mac"`, `"mas"`).
  3. When `OpenArchitect` was returned, frontend handlers in `recorder.ts` executed `open_architect()`, which called `autoDetectRepo()`. Because no IDE or GitHub Desktop window was active, it defaulted to the spoken error: `"No repository found sir. Open a repo in your browser or GitHub Desktop."`.

### 2.2 Wave Autonomous Movement & Loading Usurpation
- **Autonomous Wave Oscillation**: In `frontend/src/avatar/Avatar.tsx`, the animation loop ran a continuous sinusoidal function (`0.26 + 0.08 * Math.sin(now * 0.003 + i * 0.9)`) and kept the Lottie animation playing at `0.5` speed even when `src === "rest"`. This violated the core user expectation: **waves must only move when speech is actively present**.
- **Loading Window Usurpation**: When commands were processed, `orchestrator.rs` and frontend orchestrator/recorder handlers called `show_loading` and `setVisible(false)`, which hid the main orb/waves window and spawned the top-right 80x80 loading window (`loading.json`), breaking visual continuity.

### 2.3 STT Whisper Hallucinations on Silence & Low Energy
- **Acoustic Noise Ingestion**: When the frontend VAD sent short, low-energy, or near-silent audio chunks (< 0.005 RMS or < 200ms), Groq Whisper attempted transcription on background room noise or microphone silence.
- **Hallucinated Foreign Transcripts**: Without explicit speech energy, Whisper generated hallucinations such as:
  - `"Cunzon usarlo. Sarese manguer en us."`
  - `"One's a pane on Tadrao Vara one's a Yeraddy."`
  - `"from the"`
  - `"open drift"` / `"Open breath. Open breath."`
- The system lacked an upstream RMS energy gate prior to API dispatch.

### 2.4 WhatsApp MCP Offline Warning Flooding
- `auth_vault.rs` periodically polls `http://127.0.0.1:8765/mcp` for `pairing_status`. When the bridge process was not running, `mcp_client.rs` logged a full `WARN` level message every 30 seconds, cluttering the console output.

---

## 3. Engineering Implementation

### 3.1 Intent Parser Overhaul (`src-tauri/src/intent_parser.rs`)
- **Removed Truncated Matcher**: Deleted `if trimmed == "open-" || trimmed == "open"` in `is_architect_command`.
- **Dual-Requirement Fuzzy Matcher**: Rewrote `is_architect_fuzzy` to strictly require **both** an architecture keyword and a diagram/mapping keyword:
  - Architecture soundalikes: `["architecture", "architect", "arch", "arcade", "octach", "ark", "cat"]`
  - Diagram/mapping soundalikes: `["mapper", "map", "diagram", "graph", "remember", "member", "december"]`
  - Standalone words like `"our"`, `"are"`, `"art"`, `"mac"`, or `"master"` no longer trigger architectural mapping.
- **Stray Punctuation & Token Sanitizer**: Added `strip_leading_stray_punctuation` to strip leading punctuation (`.`, `-`, `,`, `:`, `;`) and single-letter orphaned prefixes (e.g. `"S. Ghost mode."` $\rightarrow$ `"ghost mode"`).
- **Unit Testing**: Added `test_architect_false_positives_rejected` and `test_ghost_mode_stray_punctuation`. All 181 intent parser tests pass.

### 3.2 Strict Wave Motion Rule (`frontend/src/avatar/Avatar.tsx`)
- **Flat Resting Baseline**: Replaced sinusoidal idle wave generation with a fixed baseline target of `0.15`.
- **Lottie Play/Pause Control**:
  - `src === "rest"`: `wavesAnimRef.current.pause()`, container opacity reduced to `0.35`.
  - `active` (`src === "mic" || src === "tts"`): `wavesAnimRef.current.setSpeed(1.0); wavesAnimRef.current.play()`, container opacity set to `1.0`.
- **Speech Sync**: Waves react strictly to live capture RMS levels (`audio:level` / `mic_level`) during STT recording, or synthesized audio amplitude during TTS playback.

### 3.3 Preserving Waves at Orb Location & Suppressing Loading Usurpation
- **Backend Suppression (`src-tauri/src/orchestrator.rs`)**:
  ```rust
  if crate::ghost::session_active() {
      return Ok(());
  }
  ```
  `show_loading` immediately returns `Ok(())` during Ghost Mode sessions.
- **Frontend App Guard (`frontend/src/App.tsx`)**:
  Guarded `setLoadingVisible(true)` inside the `loadingVisible` effect with `!ghostActive`.
- **Net & Recorder Protection (`frontend/src/net/orchestrator.ts` & `frontend/src/audio/recorder.ts`)**:
  Suppressed `setVisible(false)` and `setLoadingVisible(true)` whenever `ghostActive` is true, ensuring the wave stage remains visible and anchored at the orb's screen location throughout user interaction.

### 3.4 Upstream RMS Energy Gate & Hallucination Filter (`src-tauri/src/stt.rs`)
- **Acoustic Pre-Gate**:
  - In `transcribe_audio` and `transcribe_samples`, audio buffers with length $< 3200$ samples (200ms at 16kHz) or RMS $< 0.005$ are immediately discarded without calling the API:
    ```rust
    if samples.len() < 3200 {
        return Ok(String::new());
    }
    let rms = (samples.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / samples.len() as f64).sqrt();
    if rms < 0.005 {
        return Ok(String::new());
    }
    ```
- **Expanded Anti-Hallucination Filter**:
  Added checks for:
  - Noise fragments: `"from the"`, `"in the"`, `"of the"`, `"to the"`, `"open drift"`, `"open breath"`, `"open breath. open breath."`.
  - Foreign / non-English silence artifacts: `"cunzon usarlo"`, `"sarese manguer"`, `"tadrao vara"`.
  - Repetition loops.
- **Vocabulary Tuning (`src-tauri/src/stt_groq.rs`)**:
  Cleaned `NEXUS_VOCABULARY` prompt to maintain high accuracy on common commands and apps without biasing toward uncommon phrases. All 23 STT unit tests pass.

### 3.5 WhatsApp MCP Log Demotion (`src-tauri/src/mcp_client.rs`)
- Demoted connection failure logs during background `pairing_status` probes from `warn!` to `debug!`, preventing console noise when WhatsApp integration is inactive.

---

## 4. Verification & Validation Matrix

| Component | Target / Test Suite | Result | Status |
| :--- | :--- | :--- | :--- |
| **Intent Parser** | `cargo test --lib intent_parser -- --test-threads=1` | 181 / 181 Passed | **PASSED** |
| **STT Engine** | `cargo test --lib stt -- --test-threads=1` | 23 / 23 Passed | **PASSED** |
| **Wakeword Engine** | `cargo test --lib wakeword -- --test-threads=1` | 46 / 46 Passed | **PASSED** |
| **Frontend Tests** | `npm --prefix frontend test` | 44 / 44 Vitest Passed | **PASSED** |
| **Frontend Build** | `npm --prefix frontend run build` | TypeScript + Vite Clean | **PASSED** |
| **Architect Rejection** | Truncated `"open"`, `"open-"`, `"our"`, `"are"` | Rejected by `is_architect_fuzzy` | **PASSED** |
| **Wave Resting State** | Silence / Rest state (`src === "rest"`) | Static bars (`0.15`), Lottie paused | **PASSED** |
| **Loading Suppression** | Ghost Mode Active (`ghostActive == true`) | Loading window suppressed, orb waves stay | **PASSED** |
| **Silent STT Rejection**| Audio buffer RMS $< 0.005$ or duration $< 200$ms | Transcribes as `""` without Groq API call | **PASSED** |
