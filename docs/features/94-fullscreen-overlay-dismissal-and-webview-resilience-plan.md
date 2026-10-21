# Architecture & Feature Plan 94: Fullscreen Overlay Dismissal, Escape Hatches & WebView2 Resilience

## Executive Summary
This document establishes the architectural design, root cause analysis, and multi-layered escape hatches for fullscreen overlay states in NEXUS (comprising the Stage window, Screen Tour callouts, Spatial Annotations, and WebView2 runtime lifecycle).

When a transparent fullscreen overlay covers the entire primary monitor (1920×1080), a user must never be trapped or left without immediate, intuitive means to dismiss, stop, or recover control. Furthermore, transient network/dev server disconnections (e.g. Vite port 5173 unavailable) must never manifest as an opaque Chromium error page ("This site can't be reached") across the user's workspace.

---

## 1. Root Cause Analysis: "This Site Can't Be Reached" & Blackout Shutdown

### The Incident
When running `nexus start` without an active Vite dev server (or after an interrupted build), the user observed:
1. An opaque grey/white "This site can't be reached" (`ERR_CONNECTION_REFUSED`) error screen covering the desktop.
2. Three seconds later, the terminal logged:
   ```
   [RUST] stage: blackout detected (window_gone=false, stale_beat=true) — incident #1
   [EXIT] NEXUS process exited.
   ```

### Technical Root Cause
1. **DevUrl Fallback In Release Profile**:
   In `src-tauri/Cargo.toml`, `custom-protocol = ["tauri/custom-protocol"]` is an optional feature. When building with `cargo build --release` without `--features custom-protocol`, Tauri defaults to `devUrl: "http://localhost:5173"` defined in `tauri.conf.json`.
2. **Missing Local Server**:
   Because no local Vite dev server was running on port 5173, WebView2 attempted to connect, received `ERR_CONNECTION_REFUSED`, and Chromium rendered its default opaque error screen across the 1920×1080 transparent window surface.
3. **Heartbeat Blackout Detection**:
   The stage watchdog in `src-tauri/src/stage.rs` requires a heartbeat (`stage_heartbeat`) from the frontend every 2 seconds. Because the error page loaded instead of `stage.html`, no JavaScript executed and no heartbeat arrived. Once the 8-second cold-boot grace period expired, the watchdog declared a blackout and destroyed the stage window, resulting in process termination.
4. **Resolution In Release Builds**:
   Release binaries must always be compiled with:
   ```powershell
   cargo build --release --features custom-protocol
   ```
   This embeds all frontend assets into the binary via `tauri://localhost`, completely eliminating runtime network dependencies on port 5173.

---

## 2. Comprehensive Escape Hatches: How to Stop in Any Fullscreen State

NEXUS implements 5 distinct layers of escape mechanisms, ensuring that regardless of whether the system is responsive, busy, or crashed, the user retains instant control:

| Layer | Trigger | Target Subsystem | Implementation Details |
| :--- | :--- | :--- | :--- |
| **Layer 1: Native Global Kill-Switch** | **`Ctrl + Alt + X`** | OS-level Rust backend | Registered directly in Win32 via `tauri-plugin-global-shortcut`. It completely bypasses the renderer process. Pressing `Ctrl + Alt + X` immediately stops TTS audio, cancels in-flight orchestrator requests, destroys the Stage window, and disables the overlay for the session. |
| **Layer 2: Interaction Hotkey** | **`Ctrl + Space`** | Audio & Window Orchestrator | Performs DAC audio drain, cancels in-flight narration/tours, cuts TTS speech mid-sentence, and destroys open secondary windows (Sidebar, Settings, Setup). |
| **Layer 3: Interactive Callout Close** | **`×` Button Click** | Screen Tour UI | Active callout bounding box is registered as a physical Win32 mouse hole (`stage_set_hitboxes`). Hovering lights up the button, and clicking invokes `orchestrator_cancel` to immediately halt narration and wipe overlays. |
| **Layer 4: Keyboard Escape** | **`Escape` Key** | Stage & Overlays | In `TourOverlay`, `SpatialAnnotationLayer`, and `StageApp`, pressing `Escape` clears active bounding boxes, guide bubbles, pointers, and cancels in-flight tasks. |
| **Layer 5: Voice Commands** | *"NEXUS, stop"* / *"cancel"* / *"stand down"* | STT & Intent Router | Gated intent triggers `cancel_active()`, instantly stopping speech and aborting tasks. |

---

## 3. Five-Pillar Architectural Specification

### Pillar 1: Dynamic Hitbox Registration & Direct Callout Dismissal
- **Mechanism**: The stage window operates with `WS_EX_TRANSPARENT` / `set_ignore_cursor_events(true)` by default so that mouse clicks pass through to background applications.
- **Contract**:
  1. When `TourOverlay` renders a callout, it calculates the physical pixel rectangle:
     $$\text{Rect} = \{ x: \lfloor p_x \cdot \text{dpr} \rfloor, y: \lfloor p_y \cdot \text{dpr} \rfloor, w: \lfloor w \cdot \text{dpr} \rfloor, h: \lfloor h \cdot \text{dpr} \rfloor \}$$
  2. It invokes `stage_set_hitboxes("tour", [Rect])`.
  3. The Win32 cursor monitor thread in `stage.rs` opens mouse events only while the cursor resides inside this rectangle.
  4. The close button (`.tour-callout-close`) is rendered at the top-right.
  5. On click or on `Escape`, `handleDismiss()` wipes the callout, clears hitboxes (`stage_set_hitboxes("tour", [])`), and issues `orchestrator_cancel`.

### Pillar 2: Global Hotkey & Barge-In Hardening
- **Mechanism**: `hotkey.rs` manages global key combinations.
- **Contract**:
  - `Ctrl + Alt + X` executes `request_barge_in("hotkey-stage-kill")` before calling `stage_hide_kill()`.
  - `orchestrator_cancel` IPC command invokes `request_barge_in("ipc-cancel")`.
  - `request_barge_in` enforces four strict operations in order:
    1. Audio cut: `stop_tts()`
    2. Turn cancellation: `cancel_active()`
    3. State reset: `clear_tts_playing()`
    4. Followup drop: `drop_followups()`

### Pillar 3: WebView2 Offline Fallback & Error Page Suppression
- **Mechanism**: Transparent background enforcement at the CoreWebView2 controller level.
- **Contract**:
  - Set default background color to transparent (`0x00000000`) on WebView2 controllers so failed navigations never paint an opaque surface.
  - In `run.ps1`, verify binary metadata or enforce `--features custom-protocol` during compilation.

### Pillar 4: Stage Watchdog Decoupling & Graceful Recovery
- **Mechanism**: Watchdog monitors renderer liveness without terminating the whole application.
- **Contract**:
  - If a renderer heartbeat is missing, the stage window is destroyed and recreated up to 3 times via exponential backoff (2s, 4s, 8s).
  - Voice services (STT, audio engine, wakeword) remain online even if visual stage surfaces fail.

### Pillar 5: Multi-Monitor & DPI Dynamic Bounds Management
- **Mechanism**: Dynamic coordinate translation between physical pixels and CSS logical units.
- **Contract**:
  - `tourGeometry.ts` projects physical screen coordinates $(x, y, w, h)$ to CSS pixels by dividing by `devicePixelRatio` and clamping against viewport boundaries.
  - Monitor dimension changes (`WM_DISPLAYCHANGE`) dynamically update the stage viewport.
