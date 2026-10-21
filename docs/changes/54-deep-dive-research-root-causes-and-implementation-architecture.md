# Change 54: Deep Dive Research, Root Causes, and Implementation Architecture

**Date:** 2026-09-29  
**Domain:** System Architecture, Ghost Mode, Browser Automation, Wave Visualizer, Intent Isolation  

---

## 1. Summary of Identified Defects & Root Causes

### 1. Ghost Mode & Wake Up Waves Disappearance
- **Root Cause**: `Avatar.tsx` restricted the wave visualizer to `ghostActive === true`. On wake word activation outside Ghost Mode, `ghostPhase` stayed `"smile"` and rendered only the facial orb. During Ghost Mode, the resting state scale of `0.15` combined with `0.35` opacity reduced the 70px–120px wave bars to tiny 10px–18px stubs, rendering them visually invisible on desktop screens.

### 2. Strict Wave Motion Invariant
- **Root Cause**: The audio-reactive loop had a high resting threshold (`WAVE_REST_FLOOR = 0.04`) and deadband in `wakeword_oww.rs`, causing quiet or conversational speech to evaluate to `"rest"` and freeze the waves during active speech.

### 3. Inability to Search or Type in Browser Line-by-Line
- **Root Cause**: `ParsedIntent::Search` in `orchestrator.rs` was routed to `Subsystem::WorkerBackend` (Cloudflare Worker web search) instead of `crate::live::commands::browser::search` (local `Ctrl+L` $\to$ `type_text` $\to$ `Enter`). Bare `"search"` was unhandled in `intent_parser.rs`. Ghost Mode lacked handlers for `BrowserSearch`, `TypeText`, and continuous line-by-line dictation.

### 4. 1-2s Delay in App Open Voice Confirmation
- **Root Cause**: `run_ghost_open` called `speak_line` with the dynamic string `format!("{target} open, sir.")`. Dynamic phrases always miss the in-memory cache and incur a 1,200ms–1,800ms round-trip to Microsoft Edge-TTS cloud over WebSocket before audio can play.

### 5. False GitHub "No repository found sir" on Unrecognized Commands
- **Root Cause**: `frontend/src/audio/recorder.ts` line 354 explicitly expanded truncated `"open"` or `"open-"` into `"open architecture mapper"`. This routed cut-off or unrecognized speech to `open_architect` $\to$ `open_architect_with_auto_detect`, which failed and spoke `"No repository found sir. Open a repo in your browser or GitHub Desktop."`. Additionally, `isArchitectQuery` in `recorder.ts` matched generic verbs like "create", "show", "check".

---

## 2. Comprehensive Solutions Architecture

### Architecture Component 1: Intent Isolation & GitHub Keyword Gate
- Remove line 354 mic truncation expansion in `recorder.ts`.
- Restrict `isArchitectQuery` and `isLongRunningQuery` to explicit developer keywords (`"analyse"`, `"repo"`, `"pull request"`, `"pr"`, `"github"`).
- Remove loose regexes in Cloudflare Worker so generic questions never trigger GitHub repository resolution.

### Architecture Component 2: Local Browser Search & Typing Integration
- Implement `BrowserSearch`, `BrowserSearchFocus`, `TypeText`, `StartDictation`, and `StopDictation` in `intent_parser.rs`.
- Route browser search in `orchestrator.rs` directly to `browser::search(query)` (`Ctrl+L` $\to$ `keyboard::type_text` $\to$ `Enter`).
- Add continuous line-by-line typing in Ghost Mode using `keyboard::type_text`.

### Architecture Component 3: Instant App-Open Voice Feedback
- Use pre-cached `"Ok sir."` (< 5ms) in `run_ghost_open` to eliminate the 1.5s–2.0s Edge-TTS cloud synthesis delay.

### Architecture Component 4: Unified Waves Visualizer & Speech Sync
- Render the wave visualizer upon wake word activation and Ghost Mode.
- Provide a clean, visible resting baseline height (40px) at opacity 1.0 with **zero oscillation** when silent.
- Drive dynamic real-time wave oscillation strictly when STT vocal energy is present or TTS is playing.

---

## 3. Related Documentation

- Research: [`docs/research/voice-and-ghost-mode-root-cause-analysis-and-flow-hardening-2026-09-29.md`](../research/voice-and-ghost-mode-root-cause-analysis-and-flow-hardening-2026-09-29.md)
- Feature Specification: [`docs/features/73-ghost-waves-browser-search-typing-and-intent-isolation.md`](../features/73-ghost-waves-browser-search-typing-and-intent-isolation.md)
- Master Changelog: [`docs/changes/CHANGELOG.md`](./CHANGELOG.md)
