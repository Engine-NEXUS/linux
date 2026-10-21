# Change 94: Fullscreen Overlay Dismissal, Callout Hitbox Close, Hotkey Barge-In & WebView2 Resilience

## Date: 2026-10-06

### Summary of Changes

1. **Screen Tour Callout Interactive Hitbox & Visual Close Button**:
   - `frontend/src/stage/TourOverlay.tsx`:
     - Registered physical pixel hitboxes with Rust via `invoke("stage_set_hitboxes", { source: "tour", rects: [...] })` whenever a callout is placed on screen.
     - Registered empty hitboxes `rects: []` on unmount and during callout transitions/wipes so the stage remains 100% click-through everywhere else.
     - Added `.tour-callout-close` (`×`) button in the top-right corner with `onClick={handleDismiss}`.
     - Added global `Escape` key listener in `TourOverlay.tsx` triggering `handleDismiss()`.
     - `handleDismiss()` wipes the callout and invokes `orchestrator_cancel` to halt in-flight TTS narration and tour tasks.
   - `frontend/src/stage/tour.css`:
     - Added `pointer-events: auto;` for `.tour-callout.in`.
     - Styled `.tour-callout-close` with subtle transparent background, white glyph, and a red glow on hover.
     - Adjusted `.tour-callout` padding to `14px 38px 14px 52px` to prevent text collision with the close button.

2. **StageApp & Spatial Annotation Escape Key Dismissal**:
   - `frontend/src/stage/main.tsx`:
     - Added `Escape` key listener in `StageApp` to cancel active orchestrator tasks and hide active guide bubbles and pointers.
     - In `SpatialAnnotationLayer`, scoped hitboxes under `source: "spatial"` and added `Escape` key listener to clear pins and active highlights.

3. **Multi-Source Hitbox Registration in Rust Backend**:
   - `src-tauri/src/stage.rs`:
     - Updated `stage_set_hitboxes` IPC command to accept `source: Option<String>` with a `"legacy"` fallback, allowing independent callers (`"spatial"`, `"tour"`, `"legacy"`) to register and clear hitboxes without argument mismatch errors.
   - `src-tauri/src/live_glass.rs`:
     - Replaced raw IPC invocation with direct `crate::stage::set_hitbox_source("live-glass", stage_rects)` call.

4. **Synchronous Barge-In on Stage Kill & IPC Cancel**:
   - `src-tauri/src/orchestrator.rs`:
     - Updated `orchestrator_cancel()` IPC command to call `request_barge_in("ipc-cancel")`, ensuring TTS audio is cut immediately and active turns/tours are stopped cleanly.
   - `src-tauri/src/hotkey.rs`:
     - In the `Ctrl+Alt+X` stage kill-switch handler, added `crate::orchestrator::request_barge_in("hotkey-stage-kill")` before `stage_hide_kill()`, guaranteeing that playing audio stops and active requests abort even when destroying the stage overlay.

5. **Root Cause Analysis & Offline Resilience Plan**:
   - Documented the root cause of "This site can't be reached" (`ERR_CONNECTION_REFUSED`) when running without the Vite dev server, and documented why the production binary must be built with `--features custom-protocol`.
   - Created detailed architecture doc: `docs/features/94-fullscreen-overlay-dismissal-and-webview-resilience-plan.md`.

---

### Verification
- `npx tsc --noEmit`: 0 errors (clean)
- `npm test -- --run`: 189/189 tests passed
- `cargo check`: clean
- `cargo test --lib -- --test-threads=1`: 997/997 passed
- `cargo build --release --features custom-protocol`: release binary compiled
