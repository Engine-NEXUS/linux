# 29 — Central Orchestrator Implementation

> **Date**: 2026-09-02
> **Type**: Architecture change
> **Impact**: High — centralizes all request lifecycle management

---

## Summary

Implemented a central orchestrator in Rust that owns the entire request
lifecycle: intent parsing, routing, request IDs, loading indicator,
acknowledgement, subsystem dispatch, result emission, and cancellation.

## What Changed

### New Files

1. **`src-tauri/src/orchestrator.rs`** (700+ lines)
   - `OrchestratorState` enum (Idle, Listening, Thinking, Speaking)
   - `Subsystem` enum (LocalCommand, WorkerBackend, Architect, None)
   - `OrchestratorEvent` enum (State, Loading, Ack, Result, Done, Error)
   - `process_transcript()` — main entry point
   - `route_intent()` — deterministic routing
   - `install_new_request()` — cancels previous, generates request ID
   - `dispatch_to_worker()` — HTTP POST to Cloudflare Worker
   - `show_loading()` / `hide_loading()` — loading indicator control
   - 36 unit tests (routing, lifecycle, cancellation, request IDs)

2. **`frontend/src/net/orchestrator.ts`** (237 lines)
   - `initOrchestratorListener()` — listens to "orchestrator:event"
   - `processViaOrchestrator()` — frontend entry point
   - `cancelOrchestrator()` — barge-in cancellation
   - `signalOrchestratorDone()` — signal TTS finished
   - Event handler translates events to Zustand store + TTS

### Modified Files

3. **`src-tauri/src/lib.rs`**
   - Registered `orchestrator` module
   - Added 6 Tauri commands: `orchestrator_process`, `orchestrator_cancel`,
     `orchestrator_done`, `orchestrator_status`, `orchestrator_show_loading`,
     `orchestrator_hide_loading`

4. **`frontend/src/App.tsx`**
   - Added `initOrchestratorListener()` call at startup

5. **`frontend/src/audio/recorder.ts`**
   - Replaced `sendTranscript()` with `processViaOrchestrator()` for
     Worker/unknown intents
   - Added `cancelOrchestrator()` call in `abortCapture()` (barge-in)

## Why

Before this change, request lifecycle was scattered across 6+ files with no
single owner. This caused:
- Duplicate loading state ownership (race conditions)
- No request IDs (stale result bugs)
- No centralized cancellation (barge-in didn't cancel Worker requests)
- Inconsistent ack timing
- No routing layer

## Test Results

```
cargo test --lib
test result: ok. 156 passed; 0 failed; 0 ignored
```

36 new orchestrator tests:
- 15 routing tests (every command type)
- 8 lifecycle tests (barge-in, cancellation, request IDs)
- 4 subsystem classification tests
- 9 existing tests preserved

## Architecture

See [docs/architecture/07-central-orchestrator.md](../architecture/07-central-orchestrator.md)
for the full architecture document.
