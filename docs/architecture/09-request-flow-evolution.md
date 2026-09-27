# 09 — Request Flow Evolution

> **Status**: Architecture shifted 2026-09-02
> **Tracks**: The journey from scattered request handling to the central orchestrator

---

## 1. Evolution Timeline

### Phase 1: Original Flow (Pre-2026-08)

```
User speaks → STT → transcript → sendTranscript() → Worker → response
```

Everything went to the Worker. Local commands (open app, media) required a
network round-trip. No loading indicator. No ack.

### Phase 2: Local-First Routing (2026-08)

```
User speaks → STT → transcript
  → recorder.ts:
      → parseTranscriptEnhanced() (Rust parser)
      → If local command: execute locally, no Worker
      → If unknown: sendTranscript() → Worker
```

Added local-first routing. Local commands (open app, media, greeting) now
execute in Rust without a network round-trip. But loading state was still
managed by `recorder.ts` and `wsBridge.ts` independently.

### Phase 3: Instant Ack (2026-08-31)

```
User speaks → STT → transcript
  → recorder.ts:
      → isLongRunningQuery(transcript) ← regex check (<1ms)
      → If long-running: speak "On it sir" IMMEDIATELY (before parsing)
      → parseTranscriptEnhanced() (Rust parser, may take 3-4s for NLU)
      → If local: roll back loading, execute locally
      → If long-running: sendTranscript() → Worker
```

Added instant ack for long-running queries. The user hears "On it sir"
immediately, before the parser even finishes. But this created duplicate
ack handling (pre-ack in recorder.ts + server ack from wsBridge.ts).

### Phase 4: Central Orchestrator (2026-09-02 — CURRENT)

```
User speaks → STT → transcript
  → processViaOrchestrator(transcript)
  → Rust orchestrator:
      1. parse_deterministic() (<1ms)
      2. route_intent() → LocalCommand | WorkerBackend | Architect
      3. install_new_request() (cancels previous, generates request_id)
      4. Emit "state: thinking"
      5. If long-running: emit "ack", show loading (from Rust)
      6. Dispatch to subsystem
      7. Emit "result" or "error"
      8. Hide loading
  → Frontend listener:
      → Speaks ack via TTS
      → Hides orb
      → Speaks result via TTS
      → Resets to idle
```

The orchestrator is now the **single owner** of the entire request lifecycle.
No more scattered state management.

---

## 2. Before vs After Comparison

### State Ownership

| Responsibility | Before (Phase 3) | After (Phase 4) |
|---|---|---|
| Intent routing | `recorder.ts` (TypeScript) | `orchestrator.rs` (Rust) |
| Request IDs | none | `orchestrator.rs` (UUID-based) |
| Loading indicator show | `recorder.ts` + `App.tsx` | `orchestrator.rs` (Rust) |
| Loading indicator hide | `wsBridge.ts` + `App.tsx` | `orchestrator.rs` (Rust) |
| Ack timing | `recorder.ts` (pre-ack) + `wsBridge.ts` (server ack) | `orchestrator.rs` (single ack) |
| Result emission | `wsBridge.ts` ("assistant:server") | `orchestrator.rs` ("orchestrator:event") |
| Error emission | `wsBridge.ts` | `orchestrator.rs` |
| Cancellation | none | `orchestrator.rs` (AtomicBool per request) |
| Barge-in | partial (session.cancelled) | `orchestrator.rs` (install_new_request cancels previous) |
| Frontend event channel | "assistant:server" | "orchestrator:event" |

### Event Channels

**Before (Phase 3):**
```
wsBridge.ts emits on "assistant:server":
  { type: "state", state: "thinking" }
  { type: "ack", text: "On it sir." }
  { type: "result", text: "..." }
  { type: "done" }
  { type: "error", message: "..." }
  ← NO request_id on any event
```

**After (Phase 4):**
```
orchestrator.rs emits on "orchestrator:event":
  { type: "state", state: "thinking", request_id: "a1b2c3d4e5f6" }
  { type: "loading", visible: true, request_id: "a1b2c3d4e5f6" }
  { type: "ack", text: "On it sir.", request_id: "a1b2c3d4e5f6" }
  { type: "result", text: "...", request_id: "a1b2c3d4e5f6", analysis: {...} }
  { type: "done", request_id: "a1b2c3d4e5f6" }
  { type: "error", message: "...", request_id: "a1b2c3d4e5f6" }
  ← EVERY event has request_id
```

### Loading Indicator Control

**Before (Phase 3):**
```
recorder.ts:
  → useAssistant.getState().setLoadingVisible(true)  ← frontend store
  → tauriInvoke("show_loading_indicator")            ← IPC to Rust

wsBridge.ts:
  → useAssistant.getState().setLoadingVisible(false)  ← frontend store
  → tauriInvoke("hide_loading_indicator")             ← IPC to Rust

App.tsx:
  → useEffect on loadingVisible
  → tauriInvoke("show_loading_indicator")              ← ANOTHER IPC call
  → tauriInvoke("hide_loading_indicator")

Problem: 3 independent owners, potential race conditions
```

**After (Phase 4):**
```
orchestrator.rs:
  → show_loading(&app)   ← Rust creates the window directly
  → hide_loading(&app)   ← Rust destroys the window directly
  → emit "loading" event ← frontend store updated for consistency

Problem: NONE. Single owner, no race conditions.
```

---

## 3. Routing Comparison

### Before (Phase 3) — TypeScript Routing in recorder.ts

```typescript
// recorder.ts — 4 routing decisions in one function
function isLongRunningQuery(transcript: string): boolean {
    const hasAnalyse = /\b(analy[sz]e|...)\b/.test(t);
    const hasPR = /\b(pr|pull\s*request)\b/.test(t);
    const hasRepo = /\b(repo|...)\b/.test(t);
    return (hasAnalyse && (hasPR || hasRepo)) || ...;
}

// Then later:
if (intent.action === "open_architect") { /* architect flow */ }
else if (intent.action === "greeting") { /* greeting flow */ }
else if (intent.action !== "unknown") { /* local command flow */ }
else { /* Worker flow */ }
```

**Problems:**
- Routing logic split between `isLongRunningQuery()` (regex) and intent parsing
- `isLongRunningQuery()` runs BEFORE intent parsing — can misclassify
- 4 separate code paths with different loading/ack behavior
- No typed subsystem — just if/else branches

### After (Phase 4) — Rust Routing in orchestrator.rs

```rust
fn route_intent(intent: &ParsedIntent) -> Subsystem {
    match intent {
        | OpenApp { .. } | Greeting { .. } | MediaPlayPause | ...
        => Subsystem::LocalCommand,
        | OpenArchitect => Subsystem::Architect,
        | AnalysePr { .. } | Search { .. } | Unknown { .. } | ...
        => Subsystem::WorkerBackend,
    }
}
```

**Benefits:**
- Single routing function, exhaustive match
- Typed `Subsystem` enum — compiler enforces all paths handled
- Routing happens AFTER intent parsing (no misclassification)
- Each subsystem has a clear dispatch path

---

## 4. Cancellation Comparison

### Before (Phase 3) — No Real Cancellation

```typescript
// wsBridge.ts
let longRunningInFlight = false;

// On new request:
if (isLongRunningInFlight()) {
    // Queue or dedup — but the OLD request keeps running!
    await handleDuplicateOrQueuedLongRunning(transcript);
    return;
}
```

**Problem:** The old Worker request keeps running. When it returns, its result
could overwrite the new request's UI state. The only "cancellation" was
`session.cancelled = true` which just prevents the result from being emitted —
but the HTTP request still runs to completion.

### After (Phase 4) — Real Cancellation with Request IDs

```rust
fn install_new_request(subsystem: Subsystem) -> (String, Arc<AtomicBool>) {
    let cancel_flag = Arc::new(AtomicBool::new(false));
    
    // Cancel the previous request
    if let Some(prev) = guard.as_ref() {
        prev.cancelled.store(true, Ordering::Relaxed);
    }
    
    *guard = Some(ActiveRequest { id, cancelled: cancel_flag.clone(), subsystem });
    (id, cancel_flag)
}

// In dispatch_to_worker:
if is_cancelled(&cancel_flag) {
    return Err("cancelled".into());  // Discard the result
}
```

**Benefit:** Each request has its own `Arc<AtomicBool>` cancel flag. When a new
request arrives, the old flag is set to `true`. The Worker dispatcher checks
this flag after the HTTP response and discards the result if cancelled.

---

## 5. File Changes Summary

### Files Removed from Critical Path

| File | Old Role | New Role |
|---|---|---|
| `wsBridge.ts` | Owned loading state, result/error emission | Still present but deprecated; orchestrator listener takes over |
| `recorder.ts` (routing) | Made routing decisions | Now just calls `processViaOrchestrator()` |
| `App.tsx` (loading) | Toggled loading window via IPC | Still toggles store for consistency, but Rust owns the window |

### Files Added

| File | Role |
|---|---|
| `src-tauri/src/orchestrator.rs` | Central orchestrator (Rust) |
| `frontend/src/net/orchestrator.ts` | Frontend event listener |

### Files Modified

| File | Change |
|---|---|
| `src-tauri/src/lib.rs` | Registered orchestrator module + 6 commands |
| `frontend/src/audio/recorder.ts` | Call `processViaOrchestrator()` instead of `sendTranscript()` |
| `frontend/src/App.tsx` | Initialize orchestrator listener at startup |

---

## 6. Migration Status

| Component | Status |
|---|---|
| Orchestrator Rust module | ✅ Implemented, 36 tests passing |
| Orchestrator frontend listener | ✅ Implemented |
| Recorder calls `processViaOrchestrator()` | ✅ For Worker/unknown intents |
| `cancelOrchestrator()` on barge-in | ✅ Called from `abortCapture()` |
| Loading indicator owned by Rust | ✅ `show_loading()` / `hide_loading()` |
| Request IDs on all events | ✅ Every event carries `request_id` |
| Cancellation via `AtomicBool` | ✅ Checked after Worker response |
| Old `assistant:server` channel removed | ⏳ Still present in `wsBridge.ts` (deprecated) |
| Architect subsystem fully owned by orchestrator | ⏳ Frontend still opens the window |
| Local commands fully owned by orchestrator | ⏳ Frontend still calls `execute_command` directly |

### What Still Uses the Old Path

1. **Architect flow** — `recorder.ts` still handles `open_architect` directly
   (calls `open_architect_with_auto_detect`). The orchestrator emits ack +
   loading, but the frontend manages the architect window.

2. **Local commands** — `recorder.ts` still handles greetings and local
   commands directly (calls `execute_command`). The orchestrator emits "done"
   immediately for these, but the frontend does the actual execution.

3. **`wsBridge.ts`** — Still has the old `assistant:server` event listener.
   It's still imported by `recorder.ts` for `setLongRunningInFlight` and
   `isDuplicateLongRunning`. These can be removed once the orchestrator
   fully owns the queue/dedup logic.

---

## 7. Future: Full Orchestrator Ownership

The end state is for the orchestrator to own **everything**:

```
User speaks → STT → transcript
  → processViaOrchestrator(transcript)
  → orchestrator.rs:
      1. Parse intent
      2. Route to subsystem
      3. For ALL subsystems (including local):
         - Dispatch
         - Collect result
         - Emit result
         - Emit done
  → Frontend listener:
      - Speaks result
      - Resets to idle
```

This requires:
1. Moving `execute_command` (local command execution) into a Rust subsystem
2. Moving architect window opening into a Rust subsystem
3. Moving the queue/dedup logic from `wsBridge.ts` into `orchestrator.rs`
4. Removing the `assistant:server` event channel entirely
5. Removing `wsBridge.ts` loading state management
