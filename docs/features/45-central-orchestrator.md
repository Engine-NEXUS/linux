# 45 — Central Orchestrator

> **Date**: 2026-09-02
> **Status**: Implemented
> **Files**: `src-tauri/src/orchestrator.rs`, `frontend/src/net/orchestrator.ts`

---

## Overview

The Central Orchestrator is the "main system" that manages all voice command
requests. It is the single owner of the request lifecycle — from intent parsing
to routing, acknowledgement, loading state, subsystem dispatch, result
emission, and cancellation.

## What It Does

1. **Parses intent** — Uses the deterministic Rust parser (<1ms) to understand
   what the user said.

2. **Routes to the correct subsystem** — Maps each intent to one of three
   subsystems:
   - `LocalCommand` — instant (<5ms), no network (open app, media, greeting)
   - `WorkerBackend` — long-running (2-20s), Cloudflare Worker (PR analysis,
     research, GitHub writes)
   - `Architect` — long-running, architecture mapper

3. **Manages request IDs** — Every request gets a unique 12-char ID. Every
   event carries this ID so stale results can't overwrite newer UI state.

4. **Owns the loading indicator** — Shows/hides the top-right loading window
   directly from Rust. No frontend IPC needed.

5. **Handles acknowledgement** — For long-running commands, emits an ack
   ("On it sir") that the frontend speaks via TTS.

6. **Cancels on barge-in** — When a new request arrives, the previous
   request's cancel flag is set. The Worker dispatcher discards results
   from cancelled requests.

7. **Emits typed events** — All events are typed (`OrchestratorEvent` enum)
   and sent on the `"orchestrator:event"` channel.

## How To Use

### From the Frontend (recorder.ts)

```typescript
import { processViaOrchestrator, cancelOrchestrator } from "../net/orchestrator";

// Process a transcript
const result = await processViaOrchestrator(transcript, dialogContext);
// result = { request_id, subsystem, handled_locally }

// Cancel on barge-in
await cancelOrchestrator();
```

### From Rust (Tauri command)

```rust
// The orchestrator_process command is registered in lib.rs
// Frontend calls: invoke("orchestrator_process", { transcript, dialogContext })
```

## Events

| Event | Payload | When |
|---|---|---|
| `state` | `{ state, request_id }` | State transition (thinking, speaking) |
| `loading` | `{ visible, request_id }` | Loading indicator show/hide |
| `ack` | `{ text, request_id }` | Acknowledgement ("On it sir") |
| `result` | `{ text, request_id, analysis?, dialog_state? }` | Subsystem result |
| `done` | `{ request_id }` | Request complete |
| `error` | `{ message, request_id }` | Error occurred |

## Test Results

- **36 orchestrator tests** — all passing
- **156 total tests** — all passing
- Routing tests cover every command type (open app, greeting, media, architect,
  search, analyse PR, analyse repo, check branch, unknown)
- Lifecycle tests verify barge-in, cancellation, request ID uniqueness
