# 07 — Central Orchestrator Architecture

> **Status**: Implemented 2026-09-02
> **Files**: `src-tauri/src/orchestrator.rs`, `frontend/src/net/orchestrator.ts`
> **Tests**: 36 orchestrator tests, 156 total (all passing)

---

## 1. Problem Statement

Before the orchestrator, request lifecycle was **scattered across 6+ files** with no
single owner. This caused:

1. **Duplicate loading state ownership** — `recorder.ts`, `wsBridge.ts`, `App.tsx`,
   and `assistant.ts` all toggled `loadingVisible` independently.
2. **No request IDs** — A late result from an old request could overwrite a newer
   request's UI state (stale result bug).
3. **No centralized cancellation** — Barge-in (new wake while previous command is
   running) didn't cancel the in-flight Worker request.
4. **Inconsistent ack timing** — Some paths acked before parsing, some after, some
   not at all.
5. **No routing layer** — Every transcript went through the same path regardless of
   whether it was a local command (instant) or a Worker query (long-running).

### The Old Flow (Before Orchestrator)

```
User speaks
  → STT → transcript
  → recorder.ts:
      1. isLongRunningQuery(transcript)  ← regex check, pre-ack
      2. If long: speak "On it sir" immediately
      3. parseTranscriptEnhanced(transcript)  ← Rust parser
      4. If greeting: speak reply, return
      5. If local command: execute_command, return
      6. If architect: open_architect_with_auto_detect, return
      7. If unknown/analyse: sendTranscript(transcript) → wsBridge
  → wsBridge.ts:
      8. POST to Worker
      9. On result: emit "assistant:server" event
      10. On error: emit error event
  → App.tsx:
      11. Listens to "assistant:server" events
      12. Toggles loadingVisible independently
  → assistant.ts (Zustand store):
      13. Stores loadingVisible alongside orb state
      14. Multiple components can set/clear it
```

**Problems with this flow:**
- Steps 1, 7, 9-14 all touch loading state independently
- No request ID correlation between events
- No cancellation when a new request arrives
- `recorder.ts` makes routing decisions that should be centralized
- `wsBridge.ts` manages loading cleanup that should be owned by the dispatcher

---

## 2. Design Goals

The orchestrator was designed to be the **single control plane** for:

| Responsibility | Owner (before) | Owner (after) |
|---|---|---|
| Request lifecycle | scattered | **orchestrator.rs** |
| Intent routing | recorder.ts | **orchestrator.rs** |
| Request IDs | none | **orchestrator.rs** |
| Loading indicator | recorder.ts + wsBridge.ts + App.tsx | **orchestrator.rs** |
| Acknowledgement timing | recorder.ts (pre-ack) | **orchestrator.rs** |
| Subsystem dispatch | recorder.ts + wsBridge.ts | **orchestrator.rs** |
| Result/error emission | wsBridge.ts | **orchestrator.rs** |
| Cancellation | none | **orchestrator.rs** |
| Frontend event channel | "assistant:server" | **"orchestrator:event"** |

### Design Principles

1. **Deterministic routing first** — Use the existing Rust `parse_deterministic()`
   parser (<1ms) to route. No LLM routing call for predictable commands.

2. **Typed subsystems** — Route only to an explicit allowlist of known subsystems.
   No free-form model-selected subsystem names.

3. **Request IDs on every event** — Every event carries a `request_id` so stale
   results cannot mutate current UI state.

4. **Rust owns loading state** — The loading indicator window is shown/hidden from
   Rust via `show_loading()` / `hide_loading()`. The frontend store is updated for
   consistency but is not the owner.

5. **Local commands skip loading** — Greetings, app opens, and media controls are
   instant (<5ms). No ack, no loading indicator, no Worker round-trip.

6. **Long-running commands get ack + loading** — Worker and Architect subsystems
   emit an ack ("On it sir"), show the loading indicator, then dispatch.

7. **Barge-in cancels** — When a new request arrives, the previous request's
   `cancelled` flag is set. The Worker dispatcher checks this flag after the HTTP
   response and discards the result if cancelled.

---

## 3. Architecture

### High-Level Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                    CENTRAL ORCHESTRATOR (Rust)                    │
│                     src-tauri/src/orchestrator.rs                 │
│                                                                  │
│  ┌─────────────┐    ┌──────────────┐    ┌─────────────────────┐  │
│  │  Parse      │───▶│  Route       │───▶│  Install request    │  │
│  │  Intent     │    │  to          │    │  (ID + cancel flag) │  │
│  │  (<1ms)     │    │  Subsystem   │    │  (cancels previous) │  │
│  └─────────────┘    └──────────────┘    └─────────┬───────────┘  │
│                                                    │             │
│                    ┌───────────────────────────────┘             │
│                    ▼                                             │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                    DISPATCH TO SUBSYSTEM                   │  │
│  │                                                           │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐    │  │
│  │  │ LocalCommand │  │WorkerBackend │  │  Architect   │    │  │
│  │  │ (instant)    │  │ (long-running)│  │ (long-running)│    │  │
│  │  │              │  │               │  │              │    │  │
│  │  │ No ack       │  │ Ack + loading │  │ Ack + loading│    │  │
│  │  │ No loading   │  │ HTTP POST     │  │ Frontend     │    │  │
│  │  │ Emit "done"  │  │ to Worker     │  │ opens window │    │  │
│  │  │ immediately  │  │ Emit result   │  │              │    │  │
│  │  └──────────────┘  └──────────────┘  └──────────────┘    │  │
│  └───────────────────────────────────────────────────────────┘  │
│                                                                  │
│  Events emitted on "orchestrator:event" channel:                │
│    { type: "state",    state, request_id }                      │
│    { type: "loading",  visible, request_id }                    │
│    { type: "ack",      text, request_id }                       │
│    { type: "result",   text, request_id, analysis?, dialog? }   │
│    { type: "done",     request_id }                             │
│    { type: "error",    message, request_id }                    │
└──────────────────────────────────────────────────────────────────┘
           │
           │ Tauri event bus
           ▼
┌──────────────────────────────────────────────────────────────────┐
│               FRONTEND LISTENER (TypeScript)                     │
│                frontend/src/net/orchestrator.ts                  │
│                                                                  │
│  Listens to "orchestrator:event" → translates to:                │
│    • Zustand store updates (state, visible, loadingVisible)      │
│    • TTS playback (speak ack, speak result, speak error)         │
│    • Orb visibility (hide during loading, show during speaking)  │
│                                                                  │
│  Exports:                                                        │
│    processViaOrchestrator(transcript, dialogContext?)             │
│    cancelOrchestrator()                                          │
│    signalOrchestratorDone(requestId)                             │
│    initOrchestratorListener()                                    │
└──────────────────────────────────────────────────────────────────┘
```

### State Machine

```
                    ┌──────┐
                    │ Idle │
                    └──┬───┘
                       │ user speaks
                       ▼
                 ┌───────────┐
                 │ Listening │
                 └─────┬─────┘
                       │ STT produces transcript
                       ▼
                 ┌───────────┐
                 │ Thinking  │  ← orchestrator emits "state: thinking"
                 └─────┬─────┘
                       │ route + dispatch
                       │
              ┌────────┼────────┐
              │        │        │
              ▼        ▼        ▼
         ┌──────┐ ┌──────┐ ┌──────┐
         │Local │ │Worker│ │Arch  │
         │Cmd   │ │Back  │ │itect │
         └──┬───┘ └──┬───┘ └──┬───┘
            │        │        │
            │        │ ack    │ ack
            │        ▼        ▼
            │   ┌──────────────┐
            │   │  Speaking    │  ← orchestrator emits "ack"
            │   │  (ack TTS)   │
            │   └──────┬───────┘
            │          │ TTS finishes
            │          ▼
            │   ┌──────────────┐
            │   │  Loading     │  ← orchestrator shows loading window
            │   │  (waiting)   │
            │   └──────┬───────┘
            │          │ result arrives
            │          ▼
            │   ┌──────────────┐
            │   │  Speaking    │  ← orchestrator emits "result"
            │   │  (result TTS)│
            │   └──────┬───────┘
            │          │ TTS finishes
            ▼          ▼
         ┌──────────────────┐
         │      Done        │  ← orchestrator emits "done"
         │  (reset to idle) │
         └──────────────────┘
```

---

## 4. Core Types

### OrchestratorState

```rust
pub enum OrchestratorState {
    Idle,
    Listening,
    Thinking,
    Speaking,
}
```

Mirrors the frontend `AssistantState`. The orchestrator is the single source of
truth for state transitions.

### Subsystem

```rust
pub enum Subsystem {
    LocalCommand,    // open/close app, media, greeting — instant, no network
    WorkerBackend,   // PR analysis, GitHub writes, research — Cloudflare Worker
    Architect,       // architecture mapper — Rust + Worker enrichment
    None,            // unparseable/empty
}
```

### OrchestratorEvent

```rust
pub enum OrchestratorEvent {
    State    { state: OrchestratorState, request_id: String },
    Loading  { visible: bool, request_id: String },
    Ack      { text: String, request_id: String },
    Result   { text: String, request_id: String,
               analysis: Option<Value>, dialog_state: Option<Value> },
    Done     { request_id: String },
    Error    { message: String, request_id: String },
}
```

All events carry `request_id` so the frontend can reject stale events.

### ActiveRequest (internal)

```rust
struct ActiveRequest {
    id: String,
    cancelled: Arc<AtomicBool>,
    subsystem: Subsystem,
}
```

Stored in a global `Mutex<Option<ActiveRequest>>`. Only one request is active at
a time. Installing a new request cancels the previous one.

---

## 5. Routing Logic

### Route Intent → Subsystem

```rust
fn route_intent(intent: &ParsedIntent) -> Subsystem {
    match intent {
        // Local commands — handled in Rust, no network
        | OpenApp { .. }
        | OpenUrl { .. }
        | CloseApp { .. }
        | WhatsappChat { .. }
        | MediaPlayPause | MediaNext | MediaPrevious | MediaStop
        | Greeting { .. }
        => Subsystem::LocalCommand,

        // Architecture mapper — Rust + Worker enrichment
        OpenArchitect => Subsystem::Architect,

        // Everything else goes to the Worker
        | AnalyseRepo { .. }
        | AnalysePr { .. }
        | AnalyseLatestPr { .. }
        | CheckBranch { .. }
        | Search { .. }
        | NluResult { .. }
        | Unknown { .. }
        => Subsystem::WorkerBackend,
    }
}
```

### Long-Running Classification

```rust
fn is_long_running(subsystem: &Subsystem) -> bool {
    matches!(subsystem, Subsystem::WorkerBackend | Subsystem::Architect)
}
```

Local commands are instant (<5ms). Worker and Architect are long-running
(2-20 seconds).

---

## 6. Request Lifecycle

### `process_transcript()` — The Main Entry Point

```
1. Parse intent (deterministic, <1ms)
2. Route to subsystem
3. Install new request (cancels previous)
4. Emit "state: thinking"
5. Handle based on subsystem:

   LocalCommand:
     - Emit "done" immediately
     - Clear active request
     - Return (frontend handles execution)

   WorkerBackend:
     - Emit "ack: On it sir."
     - Show loading indicator (Rust owns this)
     - HTTP POST to Worker
     - Check cancel flag after response
     - Emit "result: ..." (or "error: ...")
     - Hide loading indicator
     - Clear active request
     - Note: "done" is emitted by frontend after TTS finishes

   Architect:
     - Emit "ack: On it sir."
     - Show loading indicator
     - Return (frontend opens architect window)
     - Frontend emits "done" when window opens
```

### Cancellation

```rust
fn install_new_request(subsystem: Subsystem) -> (String, Arc<AtomicBool>) {
    let id = new_request_id();
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // Cancel the previous request
    let mut guard = ACTIVE_REQUEST.lock().unwrap();
    if let Some(prev) = guard.as_ref() {
        prev.cancelled.store(true, Ordering::Relaxed);
    }

    *guard = Some(ActiveRequest { id: id.clone(), cancelled: cancel_flag.clone(), subsystem });
    (id, cancel_flag)
}
```

The Worker dispatcher checks the cancel flag after the HTTP response:

```rust
if is_cancelled(&cancel_flag) {
    return Err("cancelled".into());  // discard the result
}
```

### Request ID Format

- Generated from UUID v4 (hex chars only, hyphens stripped)
- First 12 hex characters — enough for uniqueness in logs
- Example: `a1b2c3d4e5f6`

---

## 7. Loading Indicator Ownership

The orchestrator owns the loading indicator **from Rust**. No frontend IPC needed.

```rust
pub fn show_loading<R: Runtime>(app: &AppHandle<R>) {
    // Spawn async to avoid blocking event delivery
    tauri::async_runtime::spawn(async move {
        // Create the dynamic window
        get_or_create_window(&app, WindowConfig::loading_indicator())?;
        // Position at top-right corner
        let win = app.get_webview_window("loading-indicator")?;
        // Calculate position based on monitor scale
        let monitor = win.current_monitor()?;
        let scale = monitor.scale_factor();
        let screen = monitor.size();
        let x = screen.width - (80 * scale) - (7 * scale);
        let y = 9 * scale;
        win.set_position(PhysicalPosition::new(x, y));
        // Click-through
        win.set_ignore_cursor_events(true);
        // Show
        win.show();
    });
}

pub fn hide_loading<R: Runtime>(app: &AppHandle<R>) {
    destroy_window(app, "loading-indicator");
}
```

**Key properties:**
- 80×80 transparent window at top-right corner
- 7px right inset, 9px top inset
- Click-through (`set_ignore_cursor_events(true)`)
- Destroyed on hide (releases WebView2 memory)
- Show is async (WebView creation doesn't block event delivery)

---

## 8. Frontend Integration

### Event Listener (`frontend/src/net/orchestrator.ts`)

```typescript
listen<OrchestratorEvent>("orchestrator:event", (event) => {
    switch (ev.type) {
        case "state":    store.setState(ev.state); break;
        case "loading":  store.setLoadingVisible(ev.visible); break;
        case "ack":      speak(ev.text); hideOrb(); break;
        case "result":   speak(ev.text); showOrb(); break;
        case "done":     reset(); break;
        case "error":    speak(ev.message); reset(); break;
    }
});
```

### Recorder Integration (`frontend/src/audio/recorder.ts`)

The recorder now calls `processViaOrchestrator()` instead of `sendTranscript()`:

```typescript
// OLD: await sendTranscript(transcript);
// NEW:
const result = await processViaOrchestrator(transcript);
if (result?.handled_locally) {
    return;  // orchestrator emitted "done"
}
// Worker/Architect — orchestrator listener handles the rest
```

### Barge-in Integration

`abortCapture()` now calls `cancelOrchestrator()`:

```typescript
export async function abortCapture(): Promise<void> {
    void cancelOrchestrator();  // cancel active orchestrator request
    await stopRecording();
    await closeSession();
    useAssistant.getState().reset();
}
```

---

## 9. Tauri Commands

| Command | Purpose |
|---|---|
| `orchestrator_process` | Process a transcript through the orchestrator |
| `orchestrator_cancel` | Cancel the active request (barge-in) |
| `orchestrator_done` | Signal that TTS finished (frontend → Rust) |
| `orchestrator_status` | Get current state (diagnostics) |
| `orchestrator_show_loading` | Show loading indicator (manual, if needed) |
| `orchestrator_hide_loading` | Hide loading indicator (manual, if needed) |

---

## 10. Test Coverage

### 36 Orchestrator Tests (all passing)

**Routing tests (every command type):**

| Test | Input | Expected Subsystem |
|---|---|---|
| `test_route_open_app` | "open chrome" | LocalCommand |
| `test_route_open_url` | "open youtube.com" | LocalCommand |
| `test_route_close_app` | "close chrome" | LocalCommand |
| `test_route_whatsapp_chat` | "open chat with mom" | LocalCommand |
| `test_route_greeting_hello` | "hello" | LocalCommand |
| `test_route_greeting_thanks` | "thank you" | LocalCommand |
| `test_route_media_pause` | "pause" | LocalCommand |
| `test_route_media_next` | "next" | LocalCommand |
| `test_route_architect_explicit` | "open architecture mapper" | Architect |
| `test_route_search_query` | "search for rust" | WorkerBackend |
| `test_route_analyse_pr` | "analyse PR 24 in zync" | WorkerBackend |
| `test_route_analyse_repo` | "analyse zync" | WorkerBackend |
| `test_route_analyse_latest_pr` | "analyse the pr in zync" | WorkerBackend |
| `test_route_check_branch` | "check latest branch of servx" | WorkerBackend |
| `test_route_unknown_goes_to_worker` | "what is the meaning of life" | WorkerBackend |

**Lifecycle tests:**

| Test | What it verifies |
|---|---|
| `test_barge_in_cancels_previous` | New request cancels old |
| `test_cancel_active_sets_flag` | cancel_active() sets cancel flag |
| `test_install_and_cancel` | Two requests, second is active |
| `test_signal_done_doesnt_panic` | signal_done doesn't crash |
| `test_request_ids_are_unique` | IDs are unique |
| `test_request_id_is_alphanumeric` | IDs are hex chars only |
| `test_request_id_is_short` | IDs are ≤12 chars |
| `test_pick_ack_returns_valid_phrase` | Ack is from valid list |

**Subsystem classification tests:**

| Test | What it verifies |
|---|---|
| `test_local_commands_are_not_long_running` | LocalCommand → no loading |
| `test_worker_backend_is_long_running` | WorkerBackend → loading |
| `test_architect_is_long_running` | Architect → loading |
| `test_none_is_not_long_running` | None → no loading |

---

## 11. Files Changed

| File | Change |
|---|---|
| `src-tauri/src/orchestrator.rs` | **NEW** — 700+ lines, full orchestrator implementation |
| `frontend/src/net/orchestrator.ts` | **NEW** — 237 lines, frontend event listener |
| `src-tauri/src/lib.rs` | Registered `orchestrator` module + 6 Tauri commands |
| `frontend/src/App.tsx` | Initialize orchestrator listener at startup |
| `frontend/src/audio/recorder.ts` | Call `processViaOrchestrator()` instead of `sendTranscript()`; call `cancelOrchestrator()` on abort |

---

## 12. What's Next

The orchestrator is implemented and tested. Remaining work:

1. **Remove old `assistant:server` event handling** — `wsBridge.ts` still has
   the old event listener. It can be removed once all paths use the orchestrator.

2. **Architect subsystem dispatch** — Currently the Architect path returns early
   and the frontend opens the architect window. The orchestrator should own the
   full architect lifecycle in the future.

3. **More subsystems** — PR analysis, GitHub writes, and contributor checks
   should become explicit subsystems with typed input/output contracts.

4. **Server-side authentication** — The orchestrator should pass an
   authenticated session token (not just user_id) to the Worker.
