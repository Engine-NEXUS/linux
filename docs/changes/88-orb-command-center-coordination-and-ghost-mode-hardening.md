# NEXUS — Orb & Main Command Center Coordination, Ghost Mode Hardening, and Particle Refinements

**Document ID**: 88  
**Date**: 2026-10-06  
**Status**: APPROVED & IN EXECUTION  
**Applies to**: `src-tauri` (Rust Core), `frontend` (Stage Overlay, WebGL Voice Orb, Zustand store)

---

## 1. Executive Summary & Problem Motivation

During recent live testing of NEXUS via `nexus start`, several interrelated issues surfaced across speech recognition, the Central Orchestrator, the Main Command Center validation gates, the Stage overlay, and the WebGL Voice Orb:

1. **Ghost Mode STT Mishearing & Sidebar Hijack**:
   * Speaking *"ghost mode"* was transcribed by STT as `'Ghost and warning.'` (and similar phonetic variants such as `'Worst motive.'`, `'Ghost to Mulder.'`, `'It goes to mode'`).
   * Because deterministic triggers did not match, the query fell through to `WorkerBackend` (Cloudflare Worker).
   * The Cloudflare Worker treated the utterance as a general question about ghost mode and computer warnings, returning a 458-character paragraph.
   * This opened the **Sidebar** window, confusing the user who expected mouse/cursor control.

2. **Ghost Mode Command Dropping (`[DROP] Ambient drop`)**:
   * While Ghost Mode was active, speaking `"Create a new tab."` produced:
     ```text
     [MAIN-CMD] Transcript: 'Create a new tab.' | Intent: nlu_result | Center: BrowserCenter | Evidence: Strong | Owner: Unenrolled (score: 0.000)
     [DROP] Ambient drop: evidence=Strong, ownership=Unenrolled, ghost_active=true - dropped transcript: 'Create a new tab.'
     ```
   * Even though the user’s utterance had `CommandEvidence::Strong`, `center.rs:action_disposition` dropped the turn because speaker biometric verification is optional and unenrolled on this system.

3. **Hotkey Interruption during Thinking Mode Causes Orb to Disappear Permanently**:
   * When the orb was in `thinking` state, pressing `Ctrl+Space` caused the orb to disappear forever rather than returning to `listening`.
   * In `hotkey.rs`, `Ctrl+Space` only initiated barge-in when `is_speaking` was true.
   * During `thinking`, `is_speaking` was false; `hotkey.rs` woke the microphone capture but **never called `orchestrator::cancel_active()`**.
   * The in-flight background query continued running; when it completed seconds later, its completion callback called `store.setVisible(false)` and `reset()`, tearing down the newly listening orb.

4. **Wakeup Orb Visibility in Ghost Mode**:
   * Users expect the Voice Orb to remain persistently visible on screen as long as a Ghost session is active.
   * Mishearing `"ghost mode"` prevented `ghost:session` from firing in the first place, and timeouts in `OrbFrame.tsx` could unmount the frame if `ghostActive` was not guarded directly in DOM visibility conditions.

5. **Particle Density & Size Tuning**:
   * User requested decreasing individual particle point size by **40%** (scale factor $0.60$) and increasing total particle count by **20%** (from 5,200 to 6,240).

6. **Screen Analysis & Screen Tour Preservation**:
   * User explicitly confirmed that Screen Analysis is working as intended: *"the screen anlaysis is perfectly alligned dont touch that at all it is perfect as i imagined"*.
   * All Screen Analysis, Gemini Vision, and Tour Overlay code paths must remain 100% untouched.

---

## 2. Review of Claude's GitHub Commits & PR #29

A complete comparison of the remote repository `Engine-NEXUS/NEXUS-Agent` and `Engine-NEXUS/WINDOWS` was conducted against the local workspace:

* **PR #29 Title**: *"Orb entrance burst, pebble speaking silhouette, real TTS-beat sync"*
* **Merge Status**: **MERGED** at commit `6a803de` on `Engine-NEXUS/WINDOWS` (URL: `https://github.com/Engine-NEXUS/WINDOWS/pull/29`).
* **Local Head**: `c8611b0` (the branch head of PR #29).
* **Code Diff**: `git diff HEAD origin/main` is **empty** (0 differences). All merged features are present locally.

### Breakdown of Claude's 11 Sequential Commits
1. `c8611b0` — **Screen-Wide Entrance Burst & TTS Beat Sync**:
   * Added `EntranceBurst.tsx`: 2D canvas overlay converging particles across the entire viewport upon wake.
   * Added irregular pebble speaking silhouette (`n*0.65` dominant lobe octave).
   * Added real audio amplitude streaming via `compute_envelope()` in `tts.rs` (peak-normalized 20ms RMS volume stream).
2. `6129d30` — **Dead CSS Cleanup**: Removed deprecated window styles, linked docs in `AGENTS.md`.
3. `461fc46` — **Streaming STT Live Captions**: Piped Moonshine WebSocket STT into `LiveCaption.tsx`.
4. `3cbcef3` — **Response Captions**: Timed reveal off Edge-TTS boundary events via `captionScheduler.ts`.
5. `a89fa82` — **Orb Palette & State Shapes**: Amber listening palette, 6-strand golden wisp thinking state, bumpy blob speaking shape.
6. `ff42fa7` — **Single-Stage Shell Migration**: Retired separate `main` and `loading-indicator` OS windows into `stage.html`.
7. `380dcc3` — **Ghost Mode Phonetic Safety**: Added phonetic mishearing normalization for stop words (`stahp`, `concel`).
8. `e5a221e` — **Brain Confidence Bounds**: Capped hallucinated brain-only click/window labels below acceptance thresholds.
9. `ac249e4` — **Local Window & Click Execution**: Wired `minimize`, `maximize`, `focus`, and UIA click resolution to local Win32 APIs.
10. `8f4873f` — **In-Process BERT-Mini**: Removed external Python sidecar, running ONNX BERT-Mini intent classification in-process.
11. `d61c517` — **Ghost Mode Foundation**: Core cursor/keyboard control, screen grounding, and offline voice pipeline stabilization.

---

## 3. Deep Architecture: How the Voice Orb Coordinates with the Main Command Center

The Voice Orb and the Main Command Center communicate over Tauri IPC events and shared memory synchronization:

```
 ┌────────────────────────────────────────────────────────────────────────────────────────┐
 │                         STAGE OVERLAY (Win32 Transparent Canvas)                       │
 │                                                                                        │
 │  ┌─────────────────────────────────────────────────┐  ┌─────────────────────────────┐  │
 │  │          WebGL Voice Orb (<voice-orb>)          │  │     Loading & Captions      │  │
 │  │  • Idle: Breathing harmonic sphere              │  │  • EntranceBurst.tsx        │  │
 │  │  • Listening: 50% anchor cage + 50% sand cymatics│ │  • ResponseCaption.tsx     │  │
 │  │  • Thinking: 3D rotating purple starburst (64)  │  │  • LiveCaption.tsx          │  │
 │  │  • Speaking: Pebble blob undulating to TTS beat │  │  • LoadingIndicator.tsx     │  │
 │  └────────────────────────▲────────────────────────┘  └──────────────▲──────────────┘  │
 └───────────────────────────┼──────────────────────────────────────────┼─────────────────┘
                             │                                          │
 ┌───────────────────────────┴──────────────────────────────────────────┴─────────────────┐
 │                       FRONTEND STAGE ORCHESTRATION (Zustand Store)                     │
 │   • OrbFrame.tsx: CSSOM positioning, dispersion, entrance bursts, visibility          │
 │   • orbRuntime.ts: Event listeners (orb:wake, stt:transcript, ghost:session)           │
 │   • assistant.ts: State machine (idle ⇆ listening ⇆ thinking ⇆ speaking)              │
 │   • captionScheduler.ts: Streams 20ms RMS envelope values to VoiceOrb.setLevel()       │
 └───────────────────────────▲──────────────────────────────────────────▲─────────────────┘
                             │                                          │
                 Tauri IPC: orb:wake, stage:orb_rect,       Tauri IPC: stt:transcript,
                 stage:orb_visible, ghost:session           tts:speak, tts:stop
                             │                                          │
 ┌───────────────────────────┴──────────────────────────────────────────┴─────────────────┐
 │                       RUST CENTRAL ORCHESTRATOR & MAIN COMMAND CENTER                  │
 │                                                                                        │
 │  ┌──────────────────────────────────────────────────────────────────────────────────┐  │
 │  │ orchestrator.rs (Turn Lifecycle Engine)                                          │  │
 │  │ • ACTIVE_REQUEST: Unique turn ID, atomic cancel flags                            │  │
 │  │ • request_barge_in(): Cuts audio, cancels request, 250ms DAC flush               │  │
 │  │ • tts.rs: compute_envelope() calculates 20ms RMS volume stream                   │  │
 │  └──────────────────────────────────────────┬───────────────────────────────────────┘  │
 │                                             │ Routes & Validates                       │
 │  ┌──────────────────────────────────────────▼───────────────────────────────────────┐  │
 │  │ center.rs (Main Command Center & Sub-Center Registry)                            │  │
 │  │ • Validity Gate: Unheard / NeedSlot / Clarify / Valid                            │  │
 │  │ • ActionDisposition: CommandEvidence × TurnOwnership × ghost_active             │  │
 │  │ • direct_ui(UiDirective): Visual transitions (Loading, Ghost Session)            │  │
 │  └──────────────────────────────────────────┬───────────────────────────────────────┘  │
 │                                             │ Dispatches                               │
 │  ┌──────────────────────────────────────────▼───────────────────────────────────────┐  │
 │  │ 15 Sub-Centers (BrowserCenter, GhostCenter, SystemCenter, YouTubeCenter, ...)   │  │
 │  └──────────────────────────────────────────────────────────────────────────────────┘  │
 └────────────────────────────────────────────────────────────────────────────────────────┘
```

### 3.1 IPC Event Map
1. `stage:orb_rect` (Rust $\to$ Stage): Calibrated physical pixel bounding box $(x, y, w, h)$.
2. `stage:orb_visible` (Rust $\to$ Stage): Physical presence and Win32 `WM_NCHITTEST` hitbox activation.
3. `orb:wake` (Rust $\to$ Stage): Instructs frontend runtime to transition state into `listening`.
4. `stt:transcript` (Rust $\to$ Stage): Delivers recognized transcript along with turn metadata (`ownership`, `score`, `language`).
5. `stage:loading_visible` (Rust $\to$ Stage): Controlled by `center::direct_ui(UiDirective::Loading(..))` to show/hide the spinner.
6. `ghost:session` (Rust $\to$ Stage): Controlled by `center::direct_ui(UiDirective::Session(..))` to toggle ghost waves and persistent display.
7. `tts:speak` (Rust $\to$ Stage): Streams PCM audio, word boundaries, and 20ms RMS volume envelope array.
8. `tts:stop` (Bidirectional): Immediate stop of audio playback and buffer drainage.

---

## 4. Root Causes & Engineering Solutions

### Issue 1: Ghost Mode Soundalike Mishearings
* **Root Cause**: In `intent_parser.rs`, `parse_ghost_control_entry` used strict trigger matching (`TRIGGERS`). Mishearings like `"ghost and warning"`, `"worst motive"`, `"ghost to mulder"`, and `"it goes to mode"` were not present in `normalize_phonetic_mishearings`. The turn fell through to `WorkerBackend`, which generated an informational summary in the sidebar.
* **Fix**:
  1. Add `"ghost and warning"`, `"ghost on warning"`, `"ghost warning"`, `"worst motive"`, `"ghost to mulder"`, `"it goes to mode"`, `"goes to mode"` to `normalize_phonetic_mishearings`.
  2. Add the soundalike triggers to `parse_ghost_control_entry`.
  3. Strip punctuation before phrase matching to prevent trailing periods from blocking exact matches.

### Issue 2: Ghost Mode Unenrolled Turn Dropping
* **Root Cause**: In `center.rs:action_disposition`:
  ```rust
  match (evidence, ownership) {
      (CommandEvidence::Strong, TurnOwnership::Verified) => ActionDisposition::Allow,
      (CommandEvidence::Strong, TurnOwnership::Unenrolled) if !ghost_active => ActionDisposition::Allow,
      (CommandEvidence::Medium, TurnOwnership::Verified) => ActionDisposition::Allow,
      (_, _) if ghost_active => ActionDisposition::AmbientDrop,
      _ => ActionDisposition::Clarify,
  }
  ```
  Because biometric enrollment is not active, `ownership` is always `Unenrolled`. When `ghost_active == true`, the guard `if !ghost_active` caused `(CommandEvidence::Strong, TurnOwnership::Unenrolled)` to fall through to `AmbientDrop`.
* **Fix**:
  Allow `(CommandEvidence::Strong, TurnOwnership::Unenrolled)` whether `ghost_active` is true or false.

### Issue 3: Hotkey Interruption during Thinking Mode
* **Root Cause**: In `hotkey.rs`, `Ctrl+Space` checked `is_speaking`. During `thinking`, `is_speaking` was false. The hotkey woke the microphone capture but never called `orchestrator::cancel_active()`. The in-flight background query finished late and triggered `store.setVisible(false)`, hiding the orb.
* **Fix**:
  In `hotkey.rs`, check if an orchestrator request is active (`orchestrator::has_active_request()`) or state is `thinking`. If so, invoke `orchestrator::request_barge_in("hotkey-thinking")` and `orchestrator::cancel_active()`, emitting a cancellation event to flush frontend bridges.

### Issue 4: Wakeup Orb Persistence in Ghost Mode
* **Root Cause**: In `OrbFrame.tsx`, `const cssVisible = visible || dispersing;` did not guard against hide calls when `ghostActive` was true.
* **Fix**:
  1. Update `cssVisible` to: `const cssVisible = visible || dispersing || ghostActive;`.
  2. Guard `useAssistant.getState().reset()` so `ghostActive` preserves `visible: true`.

---

## 5. Mathematical Particle Specifications

Per requirements:
* **Size Reduction by 40%** (scale factor $0.60$):
  * **WebGL Vertex Shader** (`voice-orb.js`):
    * Base term: $1.9 \times 0.60 = \mathbf{1.14}$
    * Depth term: $1.4 \times 0.60 = \mathbf{0.84}$
    * Rim term: $0.7 \times 0.60 = \mathbf{0.42}$
    * Point size floor: $\max(2.6 \times 0.60, \dots) \to \mathbf{\max(1.6, point \cdot pixels / 480.0)}$
    * Sparkle boost: $4.5 \times 0.60 = \mathbf{2.7}$
  * **2D Canvas Fallback** (`voice-orb.js`):
    * Dot floor: $1.3 \times 0.60 = \mathbf{0.78} \to \mathbf{0.8}$
    * Multiplier: $(1.1 + 0.4(pz+1) + 0.8rim) \times 0.60 = \mathbf{(0.66 + 0.24(pz+1) + 0.48rim)}$
* **Particle Count Increase by 20%** (scale factor $1.20$):
  * **Desktop Default**: $5,200 \times 1.20 = \mathbf{6,240}$ particles.
  * **Coarse Pointer**: $4,200 \times 1.20 = \mathbf{5,040}$ particles.
  * **Fibonacci Seed Parity**: `mod(floor(seed.w * 6240.0 + 0.5), 2.0) < 0.5`.

---

## 6. Observability & Logging Strategy for `nexus start`

Enhance logging to surface every loophole, failed intent, or dropped command:
* `[PHONETIC-NORM] Normalized '{raw}' → '{canonical}'`
* `[PARSER-MISS] No deterministic match for '{raw}' → routing to {subsystem}`
* `[ORB-HOTKEY] Hotkey pressed in state: '{state}' | cancelling active request: {id} → transitioning to listening`
* `[DROP] Explicit drop cause: evidence={evidence:?}, ownership={ownership:?}, ghost_active={ghost_active}`
* `[SUB-EXEC] Center: {center} | Validated: {ok} | Action: {action}`
