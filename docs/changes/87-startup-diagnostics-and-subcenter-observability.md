# Change 87 — Startup Diagnostics & Subcenter Observability Matrix

**Date**: 2026-10-06  
**Author**: Antigravity  
**Status**: Shipped & Verified  

---

## 1. Context & Motivation

The user requested comprehensive visibility during `nexus start`:
1. *"in the nexus start i want u to plan somwthing i need to know where is the codbease being broken or the execution being failed"*
2. *"i want every single loop hole details and cross check in the entire codebase to be shown in the nexus start logs"*
3. *"no detaied should be missed include the anaimation froented"*
4. *"main commandceter validation an sendingto sub center And sub center process"*
5. *"plana na dmake sure nothing misses out and perfeclty alligned without any issue"*

Before this change, failures across the system (audio driver drops, missing Python/Node engines, ambiguous NLU intents, subcenter validation drops, WebGL context errors) were either suppressed in PowerShell startup scripts (`scripts/run.ps1`), silently rejected without explicit reason, or logged only in disconnected debug flags.

---

## 2. Implementation Details

### A. Preflight 5-Layer System Audit Matrix (`src-tauri/src/diagnostics.rs`)
Added `run_full_system_audit(app_data_dir: &Path)` executing during Tauri setup before listeners start:
- **Layer 1 (Audio & Neural Models)**: Validates CPAL default input host/device, OpenWakeWord ONNX model (`melspectrogram.onnx`, `embedding_model.onnx`, `nexus.onnx`), and Kokoro-82M offline TTS weights.
- **Layer 2 (Frontend Overlay & CDP)**: Verifies `stage.html`, `main.html`, WebGL asset bundles, and warns if CDP debugging is inactive.
- **Layer 3 (Main Command Center)**: Evaluates deterministic grammar parser across 5 canonical intent tests, compound task planner splitter (`split_compound`), and directed speech gate (`directed::evaluate`).
- **Layer 4 (All 15 Sub-Centers)**: Verifies presence, tools, and execution runtimes across all 15 sub-centers (`AppCenter`, `BrowserCenter`, `SystemCenter`, `MediaCenter`, `YouTubeCenter`, `MessageCenter`, `CommerceCenter`, `GitHubCenter`, `ArchitectCenter`, `GhostCenter`, `GoogleCenter`, `MemoryCenter`, `DictationCenter`, `GreetingCenter`, `KnowledgeCenter`).
- **Layer 5 (External Integrations & Hardware)**: Checks Git binary, Python environment, Windows UAC / admin privileges, and network API credentials (Gemini, Google).
- Outputs a formatted ASCII table with `[PREFLIGHT]` tagged rows highlighting `[OK]`, `[WARN]`, and `[FAIL]`.

### B. Main Command Center Turn Ownership & Rejection Telemetry (`src-tauri/src/orchestrator.rs`)
- Added `[MAIN-CMD]` logs tracking ingested transcript, resolved intent label, target SubCenter, evidence score, and turn ownership verdict (`Owner`, `LikelyOwner`, `Uncertain`, `NonOwner`).
- Added `[DROP]` telemetry explicitly logging rejected turn ownership and ambient noise drops with exact root cause and evidence score.
- Added validation reason logs for `Validity::Unheard`, `Validity::NeedSlot`, and `Validity::Invalid`.
- Added `[SUB-ROUTE]` telemetry logging target SubCenter and subsystem before execution.
- Added `[SUB-PROC]` telemetry before and after execution across `CommandCenter`, `LocalCommand`, `WorkerBackend`, and `Mcp`.

### C. Self-Contained SubCenter Validation & Path Resolution
- **`src-tauri/src/center.rs`**: Connected `YouTubeCenter`, `SystemCenter`, and `BrowserCenter` validation to `center::validate` for `NluResult` intents.
- **`src-tauri/src/browser_center.rs`**: Made `BrowserCenter::validate` completely self-contained to eliminate circular recursion into `center::validate`.
- **`src-tauri/src/youtube_center.rs`**: Added `resolve_youtube_engine_path() -> Option<PathBuf>` to resolve `server/youtube/youtube_engine.py` robustly from executable directory, manifest, or CWD.
- **`src-tauri/src/app_registry.rs`**: Added `cached_app_count() -> usize` to report indexed apps during preflight audit.

### D. Frontend WebGL Animation & Stage Shell Telemetry
- **`frontend/src/avatar/voice-orb.js`**: Added `[ANIM]` console logs on WebGL context initialization (5200 particles, vendor, renderer), shader compilation errors, and orb state transitions.
- **`frontend/src/stage/main.tsx`**: Added `[ANIM] Stage overlay mounted: {width}x{height} (DPR: {dpr})` on stage startup.

### E. Launcher & CDP Streaming Unification (`scripts/run.ps1` & `scripts/cdp_monitor.js`)
- **`scripts/run.ps1`**:
  - Always passes `--remote-debugging-port=9222` to WebView2.
  - Starts `cdp_monitor.js` by default without requiring `-Debug`.
  - Removed log suppressions on audio hardware silence drops, permissions, and wake word loader.
  - Added dedicated ANSI color highlighting for `[PREFLIGHT]` (Cyan/Yellow/Red), `[MAIN-CMD]` (Green), `[SUB-ROUTE]` (Cyan), `[SUB-PROC]` (Magenta), `[DROP]` (Red), `[ANIM]` (Yellow), and `[FRONT]` (Yellow).
- **`scripts/cdp_monitor.js`**:
  - Broadened target URL matcher to capture dev server (`localhost:5173`) and stage tabs.
  - Formats incoming frontend logs cleanly as `[FRONT]` and exceptions as `[FRONT] [error]`.

---

## 3. Verification
- **Rust Typecheck**: `cargo check` in `src-tauri` passed with 0 errors.
- **Frontend Typecheck**: `npx tsc --noEmit` clean with 0 errors.
- **Frontend Test Suite**: `npx vitest run` passed 182/182 tests across 24 test suites.
- **Rust Unit Tests**: `cargo test --lib` verified.
