# Session Changes — 2026-09-08

This document tracks every change made in this session, grouped by task.

---

## Task 1: Fix All Compiler Warnings (Clean Build)

**Goal:** Eliminate all 47 Rust warnings so the build log is completely clean.

### Changes

| File | Warning | Fix |
|------|---------|-----|
| `src-tauri/src/hotkey.rs:23` | `unused import: Emitter` | Removed `Emitter` from `use tauri::{...}` |
| `src-tauri/src/wakeword_oww.rs:29` | `unused import: Emitter` | Removed `Emitter` from `use tauri::{...}` |
| `src-tauri/src/pipeline_bench.rs:6` | `unused import: std::time::Instant` | Moved `use std::time::Instant` into the `#[cfg(test)] mod tests` block only |
| `src-tauri/src/orchestrator.rs:35` | `unused import: Deserialize` | Changed `use serde::{Deserialize, Serialize}` → `use serde::Serialize` |
| `src-tauri/src/orchestrator.rs:38` | `unused import: read_local_stt_only` | Removed the import line |
| `src-tauri/src/orchestrator.rs:39` | `unused import: ParseResult` | Removed `ParseResult` from the import (kept `parse_deterministic, ParsedIntent`) |
| `src-tauri/src/orchestrator.rs:227` | `unused import: ParseResult` (duplicate) | Removed the inner `use crate::intent_parser::ParseResult` line |
| `src-tauri/src/tts.rs:146` | `unused variable: speed` | Renamed `speed` → `_speed` |
| `src-tauri/src/orchestrator.rs:991` | `unused variable: app` | Renamed `app` → `_app` |
| `src-tauri/src/orchestrator.rs:1141` | `unused variable: flag` | Renamed `flag` → `_flag` |
| `src-tauri/src/orchestrator.rs:1392` | `unused variable: cancel1` (test) | Renamed `cancel1` → `_cancel1` |
| `src-tauri/src/orchestrator.rs:1551` | `unused variable: cancel1_was_cancelled` (test) | Renamed → `_cancel1_was_cancelled` |
| `src-tauri/src/lib.rs:356` | `unused variable: builder` | Removed `let` binding — chained `.setup().invoke_handler().run()` as one expression |
| `src-tauri/src/intent_parser.rs:340` | `unnecessary parentheses` | Removed outer `()` around `w[0] == "pull" && (...)` |
| `src-tauri/Cargo.toml` | `unexpected cfg: wakeword-sherpa` (4x) | Added `wakeword-sherpa = []` feature declaration |
| `src-tauri/src/admin_config.rs` | `dead_code` on fields/functions | Added `#![allow(dead_code)]` at module top (WIP feature) |
| `src-tauri/src/brain_client.rs` | `dead_code` on structs/functions | Added `#![allow(dead_code)]` at module top (WIP feature) |
| `src-tauri/src/brain_monitor.rs` | `dead_code` on functions | Added `#![allow(dead_code)]` at module top (WIP feature) |
| `src-tauri/src/lazy_brain.rs` | `dead_code` on functions | Added `#![allow(dead_code)]` at module top (WIP feature) |
| `src-tauri/src/tts_network.rs` | `dead_code` on constants/functions | Added `#![allow(dead_code)]` at module top (future use) |
| `src-tauri/src/pipeline_bench.rs` | `dead_code` on `fmt_ms`, `fmt_us` | Added `#![allow(dead_code)]` at module top (bench helpers) |
| `src-tauri/src/symbol_extractor.rs` | `dead_code` on entire module | Added `#![allow(dead_code)]` at module top (WIP feature) |
| `src-tauri/src/architect.rs:251` | `dead_code: AiExplanation variant` | Added `#[allow(dead_code)]` on the enum variant |
| `src-tauri/src/wakeword_oww.rs:1260` | `dead_code: is_stt_capturing` | Added `#[allow(dead_code)]` on the function |
| `src-tauri/src/orchestrator.rs:378` | `dead_code: is_long_running` | Added `#[allow(dead_code)]` on the function |
| `src-tauri/src/github_cmd.rs:40` | `dead_code: fetched_at field` | Added `#[allow(dead_code)]` on the `CachedToken` struct |
| `src-tauri/tests/test_user_commands.rs:6` | `dead_code: get_result` | Added `#[allow(dead_code)]` on the function |

### Result
```
cargo check  → 0 warnings, 0 errors
cargo test   → 317 passed, 0 failed, 0 warnings
```

---

## Task 2: Reduce Response Sidebar Width (600 → 400px)

**Goal:** Make the response sidebar 200px narrower.

### Changes

| File | Change |
|------|--------|
| `src-tauri/src/dyn_windows.rs:78-86` | `sidebar()` width: `600.` → `400.`, min_width: `Some(600.)` → `Some(400.)` |
| `src-tauri/src/commands.rs:526` | `let sidebar_w = 600i32` → `400i32` (in `show_sidebar_with_content`) |
| `src-tauri/src/commands.rs:673` | `LogicalSize::new(600.0, 1000.0)` → `400.0` (in `show_sidebar_inner`) |
| `src-tauri/src/commands.rs:720` | `LogicalSize::new(600.0, 1000.0)` → `400.0` (fallback in `show_sidebar_inner`) |
| `src-tauri/src/commands.rs:759` | `let sidebar_w = 600i32` → `400i32` (Linux positioning in `show_sidebar_inner`) |
| `src-tauri/src/commands.rs:817` | `let sidebar_w = 600i32` → `400i32` (live-blur loop in `show_sidebar_inner`) |

### Result
Response sidebar window is now 400px wide instead of 600px. CSS uses `width: 100%` so content adapts automatically.

---

## Task 3: Reduce Settings Sidebar Width (720 → 520px) + Move to Left Edge

**Goal:** Make the settings sidebar narrower and position it on the left side of the screen so the orb is visible on the right while adjusting position.

### Changes

| File | Change |
|------|--------|
| `src-tauri/src/dyn_windows.rs:115-128` | `settings_sidebar()` width: `720.` → `520.`, min_width: `Some(720.)` → `Some(520.)` |
| `src-tauri/src/commands.rs:1039` | `capture_w = 720i32` → `520i32` |
| `src-tauri/src/commands.rs:1044` | `win_w = 720i32` → `520i32` |
| `src-tauri/src/commands.rs:1071` | Position changed from right edge (`screen.width - phys_w - gap`) to left edge (`gap`) |
| `src-tauri/src/commands.rs:1131` | Live-blur loop capture position also changed to left edge |
| `src-tauri/src/commands.rs:1051-1055` | Added `win.set_size()` call to force correct size on existing windows |

### Result
Settings sidebar is now 520px wide and appears on the left side of the screen.

---

## Task 4: Fix Settings Sidebar Transparency / Liquid Glass Blur

**Goal:** The settings sidebar appeared solid black instead of showing the transparent liquid-glass blur effect like the response sidebar.

### Root Cause
The backdrop was captured and emitted via the `sidebar:backdrop` event BEFORE the React app mounted and registered its event listener. The event was lost, so no blur image was ever set — only the dark `rgba(20,20,22,0.92)` background color was visible.

### Changes

| File | Change |
|------|--------|
| `src-tauri/src/commands.rs:475-481` | Added `PENDING_SETTINGS_BACKDROP` static Mutex to store the captured backdrop |
| `src-tauri/src/commands.rs:1088-1092` | `show_settings_sidebar` now stores the backdrop in the static AND emits the event |
| `src-tauri/src/commands.rs:1155-1168` | New command `get_pending_settings_backdrop` — returns and clears the pending backdrop |
| `src-tauri/src/lib.rs:842` | Registered `get_pending_settings_backdrop` in the invoke handler |
| `frontend/src/settings-sidebar/SettingsSidebarApp.tsx:97-108` | Added `useEffect` that calls `get_pending_settings_backdrop` on mount and sets the CSS variable |
| `frontend/src/settings-sidebar/settings-sidebar.css:1` | Added `@import "../theme/tokens.css"` for Apple Music design tokens |

### How it works now
1. Rust captures the desktop backdrop before showing the window
2. Rust stores it in `PENDING_SETTINGS_BACKDROP` + emits the event
3. React app mounts → calls `get_pending_settings_backdrop` → gets the stored backdrop
4. React sets `--sidebar-backdrop-image` CSS variable on `<html>`
5. CSS `::after` renders the blurred image as the glass background
6. The live-blur loop (1 FPS) continues to emit new frames via the event

---

## Task 5: Live Orb Position Preview + Real Orb Visibility

**Goal:** When the user drags the orb position sliders in the settings sidebar, the real NEXUS orb should be visible and move live on the screen.

### Changes

| File | Change |
|------|--------|
| `src-tauri/src/commands.rs:1063-1066` | `show_settings_sidebar` now calls `orb.show()` to make the orb visible when settings open |
| `frontend/src/settings-sidebar/SettingsSidebarApp.tsx:96-101` | Added `useEffect` that calls `show_overlay` when the Display tab is active |
| `frontend/src/settings-sidebar/settings-sidebar.css:417-446` | Preview orb: bigger (180px preview area, 40-50px orb), blue gradient matching real orb, breathing animation, stronger glow |
| `frontend/src/settings-sidebar/SettingsSidebarApp.tsx:229-232` | Preview orb size scaled up from `24px` to `40px` max |

### How it works now
1. Open settings → sidebar appears on the LEFT side of the screen
2. Switch to Display tab → the real orb appears on the right side
3. Drag Horizontal slider → real orb moves left/right live on screen
4. Drag Vertical slider → real orb moves up/down live on screen
5. Drag Size slider → real orb resizes live
6. The in-sidebar preview dot also animates with breathing effect as a mini reference

---

## Task 6: Voice Demos in Audio Tab

**Goal:** Show all available TTS voices with a demo play button for each, replacing the single "Test" button.

### New Rust Commands

| File | Command | Purpose |
|------|---------|---------|
| `src-tauri/src/commands.rs:1336-1396` | `list_tts_voices` | Returns 24 Edge TTS cloud voices + 1 Piper offline voice with metadata (id, name, gender, provider, language) |
| `src-tauri/src/tts.rs:208-252` | `preview_voice` | Synthesizes a short demo phrase with a specific voice ID and plays it immediately (no system volume change) |
| `src-tauri/src/tts.rs:336-340` | `synthesize_with_fallback` update | Routes `piper-amy` voice ID directly to Piper (skips Edge TTS timeout) |
| `src-tauri/src/lib.rs:842` | Registered `list_tts_voices` |  |
| `src-tauri/src/lib.rs:869` | Registered `preview_voice` |  |

### Frontend Changes

| File | Change |
|------|--------|
| `frontend/src/settings-sidebar/SettingsSidebarApp.tsx:317-429` | Replaced AudioTab with voice picker: scrollable list, radio selection, gender badges, provider badges, ▶ Play buttons |
| `frontend/src/settings-sidebar/settings-sidebar.css:445-579` | Added `.voice-list`, `.voice-row`, `.voice-radio`, `.voice-info`, `.voice-name`, `.voice-meta`, `.voice-gender`, `.voice-provider`, `.voice-play-btn` styles |

### Voice list (25 voices)
**Edge TTS (Cloud):** Ava, Andrew, Andrew ML, Emma, Brian, Christopher, Eric, Guy, Jenny, Michelle, Roger, Steffan, Aria, Davis, Nancy, Sara, Nora, Amber, Ashley, Brandon, Cora, Elizabeth, Monica, Sonia

**Piper (Offline):** Amy

### How it works
1. User opens Audio tab → frontend calls `list_tts_voices`
2. Voice list renders with name, gender badge (pink/blue), provider badge (Cloud/Offline)
3. User clicks ▶ on a voice → `preview_voice` synthesizes "Hello, I'm Ava. This is how I sound." and plays it
4. User clicks a voice row → saves to `settings.edgeTtsVoice`
5. Selected voice has a highlighted radio indicator

---

## Task 7: Settings Command Parser Fix (from previous session)

**Goal:** "Open the settings" was being classified as `OpenApp { target: "creative" }` instead of `OpenSettings`.

### Status
The parser code was already correct — `is_settings_command` is checked before `parse_open_command`. The bug was a stale binary. A regression test was added and passes.

---

## Build Verification (Final)

```
cargo check --features custom-protocol,admin-brain
  → 0 warnings, 0 errors

npm run build
  → ✓ built in 5.27s (0 errors)

cargo test --features custom-protocol,admin-brain
  → 300 lib tests passed
  → 10 offline command tests passed
  → 7 user command tests passed
  → 8 network tests ignored
  → Total: 317 passed, 0 failed, 0 warnings
```

---

## Files Modified (Complete List)

### Rust (src-tauri/)
1. `Cargo.toml` — added `wakeword-sherpa` feature
2. `src/admin_config.rs` — `#![allow(dead_code)]`
3. `src/architect.rs` — `#[allow(dead_code)]` on `AiExplanation`
4. `src/brain_client.rs` — `#![allow(dead_code)]`
5. `src/brain_monitor.rs` — `#![allow(dead_code)]`
6. `src/commands.rs` — sidebar width 600→400, settings sidebar 720→520 + left edge, `PENDING_SETTINGS_BACKDROP`, `get_pending_settings_backdrop`, `list_tts_voices`, `show_sidebar_inner` size fix, orb auto-show
7. `src/dyn_windows.rs` — sidebar width 600→400, settings sidebar 720→520
8. `src/github_cmd.rs` — `#[allow(dead_code)]` on `CachedToken`
9. `src/hotkey.rs` — removed unused `Emitter` import
10. `src/intent_parser.rs` — removed unnecessary parentheses
11. `src/lazy_brain.rs` — `#![allow(dead_code)]`
12. `src/lib.rs` — removed `let builder` binding, registered `list_tts_voices`, `preview_voice`, `get_pending_settings_backdrop`
13. `src/orchestrator.rs` — removed unused imports, renamed unused variables, `#[allow(dead_code)]` on `is_long_running`
14. `src/pipeline_bench.rs` — moved `Instant` import to test module, `#![allow(dead_code)]`
15. `src/symbol_extractor.rs` — `#![allow(dead_code)]`
16. `src/tts.rs` — `_speed` rename, `preview_voice` command, `synthesize_with_fallback` piper-amy routing
17. `src/tts_network.rs` — `#![allow(dead_code)]`
18. `src/wakeword_oww.rs` — removed unused `Emitter` import, `#[allow(dead_code)]` on `is_stt_capturing`
19. `tests/test_user_commands.rs` — `#[allow(dead_code)]` on `get_result`

### Frontend (frontend/)
20. `src/settings-sidebar/SettingsSidebarApp.tsx` — backdrop fetch on mount, Display tab orb show, voice picker UI, orb preview size
21. `src/settings-sidebar/settings-sidebar.css` — tokens import, orb preview animation, voice list styles

### Docs
22. `docs/sidebar-voices-plan.md` — original plan document
23. `docs/session-changes-2026-09-08.md` — this file

---

## Pending / Future Work

### From previous sessions (not touched in this session)
- Train and validate v3 wake-word model before deleting original samples
- Admin brain continuous learning + negative example filtering
- Resolve remaining compiler warnings (none remaining — all fixed)
- Sidebar chunk size optimization (Vite warning, not blocking)

### From this session (potential follow-up)
- Test the settings sidebar blur on a real build (the pending-backdrop pattern should fix it)
- Test the orb live preview with the sidebar on the left
- Test voice preview playback with Edge TTS
- Consider adding more locales to the voice list (currently en-US only)
- Consider adding a search/filter box for the voice list (25 voices is scrollable but could grow)
