# Change 55: Ghost Waves, Browser Search, Line-by-Line Dictation & Intent Isolation Implementation

**Date:** 2026-09-29  
**Status:** Completed & Verified  

---

## 1. Problem Statement & Root Cause Summary

During real-world voice testing, five interrelated operational defects were encountered:
1. **Waves Missing / Dimmed on Wake Up**:
   - `Avatar.tsx` gated waves on `ghostActive`, never showing them on standard wake word triggers (`state === "listening"` kept `ghostPhase === "smile"`).
   - In Ghost Mode, `targets` at rest collapsed to `0.15` and opacity was set to `0.35`, shrinking the 70px–120px bars to 10px–18px nearly invisible stubs.
   - `WAVE_REST_FLOOR = 0.04` was too high, treating normal conversational mic volume (0.02–0.035 RMS) as "rest" and freezing the animation during speech.
2. **Strict Wave Motion Invariant**:
   - Waves must never animate autonomously during rest/idle states. Motion must only occur during active microphone capture (user speaking) or TTS audio playback (NEXUS speaking).
3. **Browser Search & Line-by-Line Dictation**:
   - In `orchestrator.rs`, `ParsedIntent::Search` routed to `WorkerBackend` (web search) instead of driving the local browser.
   - `browser::search(query)` existed in `live/commands/browser.rs` (`Ctrl+L` $\to$ type $\to$ `Enter`), but was uncalled by the orchestrator.
   - Saying bare `"search"` returned `None` in `parse_search_command`, falling to `ParsedIntent::Unknown`.
   - No typing or line-by-line dictation mode existed for continuous input into open applications.
4. **App-Open TTS Cloud Latency (1.5s–2.0s)**:
   - `run_ghost_open` spoke dynamic string `format!("{target} open, sir.")`. Dynamic strings miss `state.cache`, incurring an Edge-TTS WebSocket cloud network handshake.
5. **False "No repository found sir"**:
   - In `frontend/src/audio/recorder.ts` line 354, `if (/^open-?$/i.test(t.trim())) { t = "open architecture mapper"; }` rewrote any pause after "open" into codebase mapping, triggering `open_architect_with_auto_detect`.
   - Overly broad regexes in `recorder.ts` and `wsBridge.ts` matched generic queries with words like `"create"`, `"show"`, `"check"` and `"code"`, `"project"`.
6. **Brain Monitor Retrain Path**:
   - In `brain_monitor.rs`, relative executable resolution assumed `resources/server/nlu/merge_and_train.py` without checking file existence, causing `[Errno 2] No such file or directory`.

---

## 2. Changes Implemented

### 2.1 Intent Isolation (`frontend/src/audio/recorder.ts` & `frontend/src/net/wsBridge.ts`)
- Deleted the mic truncation rewrite in `recorder.ts` line 354.
- Hardened `isLongRunningQuery` in `recorder.ts` to strictly require explicit PR/branch/repo analysis verbs (`analyse`, `review`, `deep dive`, `blast radius`) with explicit repository nouns (`repo`, `repository`, `codebase`).
- Gated `isArchitectQuery` in `recorder.ts` and `wsBridge.ts` to explicit phrases: `"architecture mapper"`, `"codebase diagram"`, `"open architecture mapper"`.

### 2.2 Local Browser Search & Typing Integration (`src-tauri/src/intent_parser.rs`, `orchestrator.rs`, `browser.rs`)
- Added `BrowserSearch { query }`, `BrowserSearchFocus`, `StartDictation`, `StopDictation` to `ParsedIntent` and `intent_to_label`.
- Added `parse_browser_search_command` and `parse_dictation_command` in `intent_parser.rs`:
  - Bare `"search"`, `"search bar"`, `"focus search"`, `"focus address bar"` $\to$ `BrowserSearchFocus`.
  - `"search in browser <query>"` $\to$ `BrowserSearch { query }`.
  - `"start typing"`, `"type whatever i say"`, `"type line by line"` $\to$ `StartDictation`.
  - `"stop typing"`, `"stop dictation"`, `"done typing"` $\to$ `StopDictation`.
- In `orchestrator.rs`:
  - Added atomic `DICTATION_ACTIVE`. When active, every transcript is typed line-by-line directly into the active window (`keyboard::type_text(&format!("{}\n", text))`) until stopped.
  - In Ghost Mode (and outside ghost mode), `Search` and `BrowserSearch` dispatch to `browser::search(query)` (`Ctrl+L` $\to$ types query $\to$ `Enter` in <300ms).
  - `BrowserSearchFocus` invokes `browser::focus_search_bar()` (`Ctrl+L`) and speaks cached `"Ready to search, sir."`.
  - `type_text` invokes `keyboard::type_text(&text)`.

### 2.3 Instant App-Open TTS (<5ms) (`src-tauri/src/orchestrator.rs` & `tts.rs`)
- In `run_ghost_open`, replaced `format!("{target} open, sir.")` with `"Ok sir.".to_string()`, utilizing the pre-cached in-memory audio.
- Added `"Ready to search, sir"` and `"Stopped typing, sir"` to `CACHED_PHRASES` in `tts.rs`.

### 2.4 Unified Wave Visualizer, Wake Choreography & Motion Rules (`frontend/src/avatar/Avatar.tsx`)
- **Plan B Wake Choreography & Wave Sequencing**:
  - When the wake word triggers (`state === "listening"` from `"idle"`), the orb executes its full Lottie wake animation first: loading circles (`SEG_LOADING` [171, 260] at 1.5x speed, ~0.9s) $\to$ smile arrival (`SEG_SMILE_ARRIVE` [261, 316] at 1.5x speed, ~0.6s) $\to$ holds stable frame at frame 300.
  - Upon smile arrival completion (`onComplete`), `wakeChoreographyDone` sets to `true`, triggering the pinch transition (`GHOST_PINCH_MS = 220ms`) into the 3-bar audio-reactive wave visualizer.
  - If speech processing advances to `speaking` or `thinking` before the wake choreography finishes, the waves transition takes over immediately to visualize speech synthesis / network processing.
  - In Ghost Mode (`ghostActive = true`), the wave visualizer activates immediately without playing the standard wake circles.
- **Wave Visibility & Motion Invariants**:
  - Resting scale is fixed at `0.35` (42px/25px/42px height) with container opacity at `0.85`, ensuring clean, prominent visibility without dimming.
  - Strict Motion Invariant: At rest, targets are clamped to `0.35` with `w.pause()`. Zero autonomous sinusoidal breathing during idle/rest.
  - Speech Oscillation: Dynamic scaling from `0.35` to `1.0` during user speech (`micLevel`) or TTS playback (`ttsWaveLevel`).
  - Set `WAVE_REST_FLOOR = 0.015` so normal conversational mic levels dynamically drive the waveform.

### 2.5 Handshake Synchronization & Ghost Session Visual Persistence (`src-tauri/src/orchestrator.rs`)
- Fixed synchronous `Done` emission race condition in `speak_line`: previously, `speak_line` emitted `Result` and `Done` in the exact same millisecond, triggering frontend `case "done"` (which cleared `currentRequestId = null` and scheduled `store.reset()` in 550ms) while TTS was still speaking the opening syllables.
- Removed premature `Done` emission from `speak_line`, `run_ghost_control_enter`, and stop/exit handlers, restoring the contract where the frontend's `ttsPlayer` speaks the full message and signals completion via `signalOrchestratorDone` / `finishSpokenResult` upon audio playback end.
- This ensures Ghost Mode hot-mic re-listen (`maybeGhostRelisten()`) fires faithfully after entry speech finishes, keeping the wave visualizer visible and hot for incoming commands.

### 2.6 Brain Monitor Retrain Path (`src-tauri/src/brain_monitor.rs`)
- Added filesystem existence verification to probe `resources/server/nlu/merge_and_train.py`, `server/nlu/merge_and_train.py`, and `../server/nlu/merge_and_train.py` before spawning python.

---

## 3. Verification Matrix

| Verification Check | Target Suite | Result |
| :--- | :--- | :--- |
| Intent Parser Unit Tests | `cargo test --lib intent_parser` | **182/182 Passed** (0.38s) |
| Orchestrator Unit Tests | `cargo test --lib orchestrator` | **52/52 Passed** (0.15s) |
| Wakeword Neural & DSP Tests | `cargo test --lib wakeword` | **46/46 Passed** (0.69s) |
| Frontend Vitest Suite | `npm --prefix frontend test` | **44/44 Passed** (0.63s) |
| Frontend Production Build | `npm --prefix frontend run build` | **Clean build** (3.95s) |
| Release Binary Build | `cargo build --release --features ...` | **Clean binary** (53.2 MB) |

---

## 4. Post-Ship Bug Fixes (2026-09-29)

### 4.1 Esc Not Stopping Ghost Mode (`src-tauri/src/ghost.rs`)

**Root cause**: `abort_session` (the Esc panic handler) sets `SESSION = Yielded` but NOT `Idle`. On the next voice entry (`"ghost mode"`) → `ghost_enter` is called → it called `register_esc` without first unregistering. The `tauri_plugin_global_shortcut` plugin rejects or stacks on a shortcut that is already registered. This caused the Esc handler to silently fail on the second (and every subsequent) Ghost Mode session after the first Esc press — the user could no longer stop Ghost Mode with Esc.

**Fix**: Added an explicit `unregister_esc(&app)` call immediately before `register_esc` in `ghost_enter`, ensuring a clean slate on every entry regardless of prior session state.

### 4.2 Ctrl+Space Shows Waves for Split Second Then Disappears (`frontend/src/audio/recorder.ts`)

**Root cause**: On a Ctrl+Space tap with no voice, Rust-side STT captured silence, applied its RMS VAD, and emitted `stt:transcript = ""`. `processTranscript("")` fired and spoke "Didn't catch that sir" (a short phrase ~1s). This raced with the Plan B Lottie wake choreography (loading circles → smile arrival → `wakeChoreographyDone = true` → 220ms pinch → waves visible). The TTS finished almost simultaneously with the wave pinch completing, triggering `setVisible(false)` + `reset()` → `state = "idle"` → `wakeChoreographyDone = false` → wave transition: `shouldShowWaves = false` → waves collapse. Net result: waves appeared for ~100-300ms, then vanished.


**Fix**: Empty transcript in non-ghost mode now performs a **silent dismiss** — `setVisible(false)` with no TTS, no state change to `"speaking"`. This gives the orb a clean slide-down without any wave animation involvement. Ghost mode silent-miss cap behavior (re-listen quietly up to `GHOST_SILENT_CAP`) is preserved.

### 4.3 Regression Fix: Orb Not Appearing on Hotkey + Smile Flip (`recorder.ts` & `Avatar.tsx`)

**Root cause 1 — Hotkey guard blocked (recorder.ts)**: The fix in §4.2 called `setVisible(false)` immediately but delayed `reset()` by 600ms. During those 600ms `state` remained `"listening"`. The next Ctrl+Space press called `startListening()` which guards `if (s.state === "listening") { return; }` → early exit → `setVisible(true)` never called → orb never appeared. The symptom: hotkey presses had no effect after any silent dismiss.

**Root cause 2 — Smile animation flip (Avatar.tsx)**: With `reset()` delayed 600ms, the Lottie `onComplete` could still fire during the slide-down (Tauri window stays open 600ms before `hide_overlay`). `onComplete` for `wake-smile` sets `wakeChoreographyDone = true` → wave transition: `state !== "idle" && wakeChoreographyDone = true` → `shouldShowWaves = true` (because `state` was still `"listening"`) → wave pinch started off-screen → then `reset()` fires at 600ms → `state = "idle"` → `shouldShowWaves = false` → leaving → smile animation played visibly in the Tauri window. Symptom: smile animation flipped when ghost mode waves were appearing.

**Fix 1 (`Avatar.tsx` - Exclusive Waves & Choreography Sequence)**: The user clarified that the wave visualization should **only** appear during Ghost Mode, but the full wake-up choreography (circles → smile) must still complete *before* swapping to waves. `shouldShowWaves` is now strictly `visible && ghostActive && wakeChoreographyDone`. In normal mode, the orb simply plays its native Lottie animations. When Ghost Mode is initialized, the orb waits to finish its smile arrival, then seamlessly pinches into the waves and stays there.

**Fix 2 (`Avatar.tsx` - Prevent Smile Flip During Slide-Down)**: The `reset()` immediate call caused `state` to become `"idle"`. The `applyState` hook saw this and immediately forced the Lottie to jump to the `idle-smile` segment. Since the orb takes 600ms to visually slide down (CSS transition), the user saw this abrupt frame jump as a "flip". Added a `!visible` early-return guard to the `applyState` hook: if the orb is sliding down, it freezes on its current frame instead of jumping.
**Fix 3 (`Avatar.tsx` - Atomic Entrance)**: Added an `isEntering` guard to `applyState`. Previously, if TTS audio started playing ("Ghost mode initialized") before the orb finished arriving, the `state="speaking"` transition would pause the animation, causing it to freeze indefinitely and never transition to waves. The entrance choreography is now uninterruptible. Furthermore, `onComplete` now re-evaluates the state upon entrance completion, ensuring no state changes are permanently lost.

**Fix 4 (`Avatar.tsx` & `styles.css` - Normal Mode UX Alignment)**: The user requested that the Lottie "loading circles" (`loading-loop`) should ONLY play during the `thinking` state. During `listening` (STT) and `speaking` (TTS), the Lottie should strictly display the static smiling orb (`idle-smile`/`holding`) and rely entirely on CSS-based zoom-in-and-out pulsing (`pulse-listen` and `pulse-speak`) to convey activity. Removed the fallback to `loading-loop` for the speaking state in `resolveAvatarAnim` and updated `avatar-wrap--listening` to use `pulse-listen` instead of a static scale.
### 4.4 Final Verification
| Verification Check | Result |
| :--- | :--- |
| Frontend Vitest | **44/44 Passed** |
