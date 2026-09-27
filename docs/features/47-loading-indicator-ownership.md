# 47 — Loading Indicator Ownership

> **Date**: 2026-09-02
> **Status**: Centralized in orchestrator
> **Files**: `src-tauri/src/orchestrator.rs`, `src-tauri/src/commands.rs`,
> `src-tauri/src/dyn_windows.rs`, `frontend/src/App.tsx`

---

## Overview

The loading indicator is a small (80x80) transparent, click-through window
positioned at the top-right corner of the screen. It shows a Lottie animation
while a long-running command (PR analysis, architecture mapping) is in progress.

## Evolution

### Phase 1: Frontend-Owned (Pre-2026-09)

The loading indicator was controlled by **3 independent frontend components**:

1. `recorder.ts` — called `setLoadingVisible(true)` after ack TTS
2. `wsBridge.ts` — called `setLoadingVisible(false)` on result/error
3. `App.tsx` — watched `loadingVisible` in the Zustand store and called
   `tauriInvoke("show_loading_indicator")` / `tauriInvoke("hide_loading_indicator")`

**Problems:**
- Race conditions between show and hide calls
- Loading could get stuck visible if one component set it but another didn't clear it
- 3 separate IPC calls to show/hide the same window
- No request ID correlation — a late result from an old request could hide
  the loading indicator for a newer request

### Phase 2: Orchestrator-Owned (Current)

The orchestrator in Rust now owns the loading indicator:

```rust
// orchestrator.rs
pub fn show_loading<R: Runtime>(app: &AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        get_or_create_window(&app, WindowConfig::loading_indicator())?;
        // Position at top-right
        let win = app.get_webview_window("loading-indicator")?;
        win.set_position(PhysicalPosition::new(x, y));
        win.set_ignore_cursor_events(true);
        win.show();
    });
}

pub fn hide_loading<R: Runtime>(app: &AppHandle<R>) {
    destroy_window(app, "loading-indicator");
}
```

The orchestrator calls `show_loading()` and `hide_loading()` directly. The
frontend store is updated via the `"orchestrator:event"` channel for
consistency, but the frontend is no longer the owner.

**Benefits:**
- Single owner — no race conditions
- No IPC round-trip for show/hide
- Request ID on every loading event — stale results can't hide the indicator
  for a newer request
- Loading is destroyed (not hidden) on hide — releases WebView2 memory

## Window Properties

| Property | Value |
|---|---|
| Size | 80x80 px |
| Position | Top-right corner (7px right, 9px top inset) |
| Transparency | `transparent: true` |
| Click-through | `set_ignore_cursor_events(true)` |
| Always on top | `alwaysOnTop: true` |
| Skip taskbar | `skipTaskbar: true` |
| Lifecycle | Created on show, destroyed on hide |

## When Loading Shows

| Subsystem | Shows loading? | When |
|---|---|---|
| LocalCommand | No | Instant (<5ms) |
| WorkerBackend | Yes | After ack TTS, before Worker dispatch |
| Architect | Yes | After ack TTS, before architect window opens |
| None | No | Instant |

## When Loading Hides

| Trigger | What happens |
|---|---|
| Result arrives | `hide_loading()` called, `loading: false` emitted |
| Error occurs | `hide_loading()` called, `error` emitted |
| Cancellation | `cancel_active()` called, loading is cleared by frontend |
| Timeout | 60s timeout in frontend clears loading state |

## Files

| File | Role |
|---|---|
| `src-tauri/src/orchestrator.rs` | `show_loading()` / `hide_loading()` — the owner |
| `src-tauri/src/dyn_windows.rs` | `WindowConfig::loading_indicator()` — window config |
| `src-tauri/src/commands.rs` | `show_loading_indicator` / `hide_loading_indicator` — legacy IPC (still available) |
| `frontend/src/App.tsx` | Listens to `loadingVisible` store, calls legacy IPC as backup |
| `frontend/src/net/orchestrator.ts` | Listens to `orchestrator:event` loading events |
