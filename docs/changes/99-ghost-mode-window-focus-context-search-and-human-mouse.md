# Change 99 — Ghost Mode Window Focus Polling, Contextual Search & Human Cursor Motion

**Date**: 2026-10-07  
**Scope**: Rust Desktop Automation (`live/commands/window.rs`, `live/commands/mouse.rs`, `screen.rs`, `orchestrator.rs`, `intent_parser.rs`)

---

## 1. Problem & Motivation

During live testing of Ghost Mode desktop automation, four issues were observed:
1. **App Opened in Background Without Focus**: Speaking *"Open WhatsApp"* launched WhatsApp, but it appeared behind the active browser window without gaining foreground focus.
2. **Global Browser Search Collision**: Speaking *"Search Mommy"* while WhatsApp was active triggered Google Search in Brave Browser (`Ctrl+L`), rather than searching for the contact in WhatsApp.
3. **6-Second Element Grounding Delay**: Speaking *"Click on the search bar"* took 5–6 seconds to respond because the local UI Automation accessibility tree omitted `Edit` controls, forcing an expensive Cloud Vision API round trip.
4. **Instant Cursor Teleporting**: The cursor dashed across the screen in 300 ms (12 steps at ~5,000 px/s) followed by an immediate 250 ms snap-back, appearing as a rapid double-teleport that the human eye could not track.

---

## 2. Root Cause Analysis

1. **Packaged App Initialization Lag**: WinUI 3 / packaged apps (e.g., WhatsApp) launch asynchronously via `shell:AppsFolder`. The main window HWND takes 800–1,500 ms to create and register. `focus_app_by_title` checked once at $t=0$ ms with zero retries, failing immediately. Windows' foreground lock policy (`SPI_SETFOREGROUNDLOCKTIMEOUT`) then prevented the app from stealing focus.
2. **Context-Free Intent Dispatching**: `ParsedIntent::Search` in `orchestrator.rs` was hardcoded to `run_browser_search` (Ctrl+L), without checking the active application or whether WhatsApp had just been opened.
3. **UIA Edit Control Omission**: `screen.rs` defined `CLICKABLE` controls as `[Button, Hyperlink, TabItem, MenuItem, ListItem, CheckBox, RadioButton, SplitButton]`, omitting `ControlType::Edit` and `ControlType::ComboBox`. Additionally, `score_name` failed on conversational noise suffixes like `"search bar"` $\to$ `"search"`. This triggered the cloud fallback (`locate_with_fallback`), adding a 5–6 second network delay.
4. **Fixed Velocity & Snapback**: `mouse.rs` hardcoded `move_eased` duration to 300 ms at 25 ms intervals (40 Hz) and immediately invoked `restore(home)` in 250 ms without arrival dwell.

---

## 3. Implementation Details

### A. Window Activation & Focus Polling (`live/commands/window.rs`)
- Implemented `wait_and_focus_app(partial_title: &str, timeout_ms: u64) -> bool` with a 150 ms polling loop up to 2,500 ms.
- In `focus_window`: Synthesized an `Alt` key tap (`super::super::keyboard::press_key("alt")`) before `SetForegroundWindow` to unlock Windows OS foreground restrictions.
- Added `get_foreground_window_title() -> Option<String>` and `is_foreground_app(partial: &str) -> bool`.
- Updated `orchestrator.rs::run_ghost_open` to use `wait_and_focus_app(&target, 2500)` in a blocking task.

### B. Context-Aware Search Routing (`orchestrator.rs` & `intent_parser.rs`)
- In `orchestrator.rs` Ghost Mode intent matching:
  - If `is_foreground_app("whatsapp")` is true: routes `"search <query>"` directly to `run_ghost_message(app, query, String::new())` (executing `whatsapp_drill` via `Ctrl+F` + contact name + `Enter`).
  - Otherwise: falls back to `run_browser_search(app, query)`.
- In `intent_parser.rs`: Added support for `"search <contact> on whatsapp"`, `"search for <contact> on whatsapp"`, and `"find <contact> on wa"` in `parse_whatsapp_command`.

### C. Sub-20ms Local UIA Search Bar Grounding (`screen.rs` & `mouse.rs`)
- Added `ControlType::Edit` and `ControlType::ComboBox` to `CLICKABLE` in `screen.rs`.
- In `mouse.rs::score_name`: Added conversational noise-word stripping (`" bar"`, `" box"`, `" button"`, `" field"`, `" input"`) and token overlap matching. `"search bar"` now matches WhatsApp's `"Search"` or `"Search or start new chat"` locally in **10–15 ms** (down from 6,000 ms).

### D. Fitts's Law Human Cursor Motion (`mouse.rs`)
- Implemented `calculate_glide_duration(from, to) -> u64` dynamically clamping duration between **550 ms and 750 ms** based on Euclidean distance ($500 + d \times 0.15$).
- Upgraded `move_eased` to 60 FPS interpolation (16 ms step intervals) for fluid visual tracking across high-refresh monitors.
- In `click_at` and `double_click_at`: Added a **100 ms arrival dwell** before mouse button activation.
- Updated `restore`: Applies dynamic glide duration and 16 ms interpolation.

---

## 4. Verification

- **TypeScript**: `npx tsc --noEmit` clean (0 errors).
- **Frontend Vitest**: `npm test -- --run` $\to$ 200/200 tests passing.
- **Rust Unit Tests**: `cargo test --lib -- --test-threads=1` $\to$ 1020/1020 passing (including new `test_score_name_noise_word_stripping` and `test_whatsapp_search_contact_parses_as_chat`).
- **Frontend Production Build**: `npm run build` cleanly compiled Vite bundle in 8.68s.
- **Release Binary**: Fresh optimized release binary built via `cargo build --release --features custom-protocol`.
