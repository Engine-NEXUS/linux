# 33 — Settings Sidebar: Research Report

> **Date**: 2026-09-06
> **Type**: Research / planning (no code changes)
> **Status**: Awaiting authorization to implement

---

## Table of Contents

1. [User's Vision](#users-vision)
2. [Current State of Each Feature](#current-state-of-each-feature)
3. [What Already Exists vs What's New](#what-already-exists-vs-whats-new)
4. [Architecture Options](#architecture-options)
5. [Per-Feature Implementation Plan](#per-feature-implementation-plan)
6. [Risks and Constraints](#risks-and-constraints)
7. [Recommended Phasing](#recommended-phasing)

---

## User's Vision

A single settings sidebar that controls everything:

1. **Authentication**
   - Google OAuth connect/disconnect
   - GitHub OAuth connect/disconnect
   - Gemini API key set/remove
   - Groq API key set/remove
2. **Volume control**
   - Default volume level that NEXUS sets when it speaks (already exists as `ttsVolume`)
3. **Orb position and size**
   - Left-to-right slider (horizontal position)
   - Top-to-bottom slider (vertical position)
   - Size control for the orb animation
4. **Wake animation window position**
   - Configurable position for the wake-up animation window

---

## Current State of Each Feature

### 1. Authentication — OAuth (Google + GitHub)

**Already fully implemented** in the sidecar + frontend.

**Backend (`server/sidecar/oauth.py`, 560 lines):**
- `POST /oauth/exchange` — exchanges auth code for tokens (Google + GitHub)
- `POST /oauth/refresh` — refreshes expired Google tokens
- `GET /oauth/status` — returns which providers are connected for a user
- `DELETE /oauth/disconnect` — removes a provider's tokens
- PKCE flow: client generates verifier, browser redirects to `NEXUS://oauth/callback`, client sends code + verifier to sidecar, sidecar exchanges with client secret (secret stays server-side)
- Google scopes: Gmail readonly, Calendar, Drive readonly, Meetings, openid, email, profile
- GitHub scopes: `repo read:org workflow`
- Token storage: `server/sidecar/db.py` — SQLite, per-user, with expiry tracking and auto-refresh

**Frontend (`frontend/src/setup/oauth.ts`):**
- `startOAuthFlow(provider)` — generates PKCE verifier, opens browser to provider's auth URL
- Deep-link handler catches `NEXUS://oauth/callback?code=...&state=...`
- Sends code + verifier to sidecar `/oauth/exchange`
- 3-tier browser open fallback (system default → Chrome → Edge)

**Rust deep-link registration (`src-tauri/src/lib.rs`):**
- Windows: registry `HKCU\Software\Classes\NEXUS\shell\open\command`
- macOS: `Info.plist` `CFBundleURLTypes`
- Tauri `deep-link` plugin handles the callback

**What's missing for the settings sidebar:**
- No UI in the **settings** window to trigger OAuth connect/disconnect — it's only in the **setup wizard**
- No status display (connected/disconnected) in settings — the sidecar has `/oauth/status` but settings doesn't call it
- No disconnect button in settings

**Verdict:** Backend is complete. Need to add OAuth connect/disconnect/status UI to the settings sidebar, calling the existing sidecar endpoints.

### 2. API Keys (Gemini + Groq)

**Partially implemented.**

**Groq API key:**
- Already stored in `settings.json` as `groqApiKey`
- Already read by `src-tauri/src/stt.rs` (`read_groq_api_key`) for cloud STT
- Already in the settings UI (`BackendTab` in `SettingsApp.tsx`) — but as a plain text input
- Already in the `Settings` struct in `commands.rs` (line 1132: `pub groq_api_key: String`)

**Gemini API key:**
- **Not currently stored or used anywhere in the codebase.**
- The `Settings` struct in `commands.rs` does NOT have a `gemini_api_key` field
- The sidecar has a generic `/apikeys/add` and `/apikeys/remove` endpoint that can store any provider's key
- The Worker (`server/worker/src/index.ts`) does not currently use Gemini

**Sidecar API key endpoints (`server/sidecar/oauth.py`):**
- `POST /apikeys/add` — `{ provider, key, user_id }` → stores in SQLite
- `DELETE /apikeys/remove` — `{ provider, user_id }`
- `GET /apikeys/list` — returns provider names (not the keys themselves)

**What's missing:**
- Gemini API key: need to add field to `Settings` struct, settings UI, and wire it to wherever Gemini will be used
- Both keys: the settings UI currently stores Groq locally in `settings.json`; the sidecar can also store it server-side. Need to decide: local-only or sidecar-synced?

**Verdict:** Groq key infrastructure exists. Gemini key needs new field + storage + UI. Both need to be surfaced in the settings sidebar.

### 3. Volume Control (TTS default volume)

**Already fully implemented.**

**Rust (`src-tauri/src/volume.rs`, 407 lines):**
- `save_and_set_volume(tts_volume: f32)` — saves current system volume, sets to `tts_volume` (0.0–1.0)
- `restore_volume()` — restores original volume after TTS
- Windows: Core Audio COM `IAudioEndpointVolume`
- macOS: CoreAudio `AudioObjectGetPropertyData` / `AudioObjectSetPropertyData`
- Linux: `wpctl` → `pactl` → `amixer` shell commands
- Thread-safe: `AtomicI32` for saved volume, `AtomicBool` for active state
- Rapid consecutive speaks handled correctly (doesn't overwrite saved volume)

**TTS integration (`src-tauri/src/tts.rs`, line 159):**
- `read_tts_volume(app)` reads from `settings.json` (default 75)
- Before playback: `save_and_set_volume(target)` (line 185)
- After playback: `restore_volume()` (line 198)
- `0` means "disabled" (don't adjust system volume)

**Settings UI (`SettingsApp.tsx`):**
- `ttsVolume` is in the `Settings` interface (line 35)
- Default: 75 (line 60)
- Already in `AudioTab` — but as a plain number input

**What's missing for the settings sidebar:**
- A proper slider UI (currently a number input)
- Live preview (set volume, test TTS, hear it at that volume)
- Maybe a "test" button that speaks a sample phrase

**Verdict:** Backend is complete and working. Need a slider UI in the settings sidebar.

### 4. Orb Position and Size

**Currently hardcoded.**

**Position (`src-tauri/src/window_manager.rs`, lines 12-35):**
- `position_orb()` computes position based on monitor size
- X: `(screen_width - orb_width) / 2` → **always centered horizontally**
- Y: `screen_height - orb_height - dock_offset - gap` → **always bottom**
- `dock_offset`: 48px Windows, 70px macOS, 36px Linux
- `gap`: 12px
- Orb size: **hardcoded 200px** (line 17: `let orb = 200i32`)

**Size (`src-tauri/src/dyn_windows.rs`, line 38):**
- `WindowConfig::main()`: `width: 200., height: 200.`
- `min_width: Some(200.), min_height: Some(200.)` — can't be resized
- `resizable: false`

**Settings storage:**
- No orb position or size fields in the `Settings` struct
- No Tauri commands to set orb position/size at runtime

**What's missing for the settings sidebar:**
- New settings fields: `orbX`, `orbY` (or `orbHorizontalPct`, `orbVerticalPct`), `orbSize`
- New Tauri command: `set_orb_position(x, y)` or `set_orb_position_pct(h_pct, v_pct)`
- New Tauri command: `set_orb_size(size)`
- `position_orb()` needs to read from settings instead of hardcoding center-bottom
- The orb window needs to be resizable (or recreated at the new size)
- Sliders in the settings sidebar: left-right (0-100%), top-bottom (0-100%), size (e.g. 100-300px)
- Live update: as the slider moves, the orb moves/resizes in real time

**Verdict:** Needs new settings fields, new Tauri commands, and modification of `position_orb()` to read from settings. The orb window is currently `resizable: false` with a min size of 200px — this needs to change or the window needs to be recreated.

### 5. Wake Animation Window Position

**Currently hardcoded.**

The "wake animation" is the orb itself — when the wake word triggers, the orb appears (via `show_overlay`) at the position set by `position_orb()`. There is no separate "wake animation window."

The loading indicator window (`WindowConfig::loading_indicator()`) is a separate 80×80 window at the top-right corner — but that's the loading spinner, not the wake animation.

**What the user likely means:**
- The position where the orb appears when NEXUS wakes up — this IS the orb position (feature #4 above)
- OR a separate animation that plays on wake (currently the orb just fades in via CSS `transform: translateY(100vh)` → `translateY(0)`)

**Verdict:** This is the same as orb position (#4). If the user wants a separate wake animation window, that would be new — but based on the current architecture, the orb IS the wake animation window.

---

## What Already Exists vs What's New

| Feature | Backend | Frontend UI | Settings storage | New work needed |
|---|---|---|---|---|
| Google OAuth | ✅ Complete | ⚠️ Setup wizard only | ✅ Sidecar DB | Add to settings sidebar |
| GitHub OAuth | ✅ Complete | ⚠️ Setup wizard only | ✅ Sidecar DB | Add to settings sidebar |
| Gemini API key | ❌ Not used | ❌ | ❌ | New field + storage + UI |
| Groq API key | ✅ Complete | ⚠️ Backend tab, plain input | ✅ settings.json | Add to settings sidebar with masking |
| TTS volume | ✅ Complete | ⚠️ Audio tab, number input | ✅ settings.json | Add slider to settings sidebar |
| Orb position | ❌ Hardcoded | ❌ | ❌ | New fields + commands + slider UI |
| Orb size | ❌ Hardcoded 200px | ❌ | ❌ | New fields + commands + slider UI |
| Wake animation position | Same as orb position | ❌ | ❌ | Same as orb position |

---

## Architecture Options

### Option A: Extend the existing settings window

Keep the current `SettingsApp.tsx` tabbed layout, add new tabs:
- "Accounts" tab (Google, GitHub, Gemini, Groq)
- "Appearance" tab (orb position, size, volume)

**Pros:** Minimal new infrastructure, reuses existing window/capabilities.
**Cons:** The current settings window is opaque (not a sidebar), doesn't match the glass aesthetic.

### Option B: New settings sidebar (recommended)

Create a new sidebar window (like the response sidebar / PR-list sidebar / architect sidebar) with the glass background, containing all settings sections.

**Pros:** Matches the user's vision ("a sidebar where I can control everything"), consistent with NEXUS's sidebar-based UI pattern, gets the glass background.
**Cons:** New window config, new HTML entry point, new capability file, new React app — but this pattern is well-established (3 sidebars already exist).

### Option C: Replace settings window entirely

Make the new settings sidebar the only settings UI, remove the old `SettingsApp.tsx`.

**Pros:** No duplication.
**Cons:** The old settings window has tabs that work fine; throwing it away is wasteful. Better to migrate gradually.

**Recommendation: Option B** — create a new settings sidebar, keep the old settings window for now, migrate tabs gradually.

---

## Per-Feature Implementation Plan

### Feature 1: Authentication section (Google + GitHub + Gemini + Groq)

**New Tauri commands needed:**
- `get_oauth_status()` — calls sidecar `/oauth/status`, returns `{ google: bool, github: bool }`
- `start_oauth(provider: String)` — calls existing `frontend/src/setup/oauth.ts` logic from Rust (or frontend invokes it)
- `disconnect_oauth(provider: String)` — calls sidecar `/oauth/disconnect`
- `save_api_key(provider: String, key: String)` — saves to `settings.json` (for Groq) or sidecar (for Gemini)
- `remove_api_key(provider: String)` — removes
- `get_api_keys_status()` — returns which keys are set (not the keys themselves)

**UI:**
- Google row: "Connected" / "Connect" button → "Disconnect"
- GitHub row: same
- Gemini API key: masked input + "Save" / "Remove"
- Groq API key: masked input + "Save" / "Remove"

**Backend changes:**
- Add `gemini_api_key: String` to `Settings` struct in `commands.rs`
- Add `gemini_api_key` to `DEFAULT_SETTINGS` in frontend

### Feature 2: Volume control slider

**No new backend work** — `ttsVolume` already exists.

**UI:**
- Slider 0-100 with label showing current value
- "Test" button that speaks "This is how NEXUS will sound at {volume} percent"
- Live update: as slider moves, save to settings (debounced)

**Tauri commands:**
- Reuse existing `save_settings`
- New: `test_tts_volume(volume: u8)` — speaks a test phrase at the given volume

### Feature 3: Orb position sliders

**New settings fields:**
- `orbHorizontalPct: f64` (0.0 = left, 0.5 = center, 1.0 = right)
- `orbVerticalPct: f64` (0.0 = top, 1.0 = bottom)
- Defaults: 0.5, 1.0 (current behavior: center-bottom)

**New Tauri commands:**
- `set_orb_position_pct(h_pct: f64, v_pct: f64)` — computes pixel position from percentages + monitor size, calls `win.set_position()`
- `get_orb_position_pct()` — returns current percentages

**Modified files:**
- `src-tauri/src/window_manager.rs` — `position_orb()` reads from settings instead of hardcoding center-bottom
- `src-tauri/src/commands.rs` — add `orbHorizontalPct`, `orbVerticalPct` to `Settings`
- `frontend/src/settings/` — new slider UI

**UI:**
- Horizontal slider: 0% (left) ↔ 100% (right), default 50%
- Vertical slider: 0% (top) ↔ 100% (bottom), default 100%
- Live update: as slider moves, orb moves in real time (debounced 50ms)

### Feature 4: Orb size slider

**New settings fields:**
- `orbSize: u32` (pixels, range 100-300, default 200)

**New Tauri commands:**
- `set_orb_size(size: u32)` — calls `win.set_size()`, updates `position_orb()` to keep orb at the right position

**Modified files:**
- `src-tauri/src/dyn_windows.rs` — `WindowConfig::main()` reads size from settings (or the window is resized at runtime)
- `src-tauri/src/window_manager.rs` — `position_orb()` uses settings size instead of hardcoded 200
- `frontend/src/styles.css` — `.orb` width/height reads from a CSS variable set by JS

**Constraint:** The orb window is currently `resizable: false` with `min_width: 200, min_height: 200`. To resize at runtime, either:
- Set `resizable: true` and use `win.set_size()` (simplest)
- Or recreate the window at the new size (heavier, but cleaner)

**Recommendation:** Set `resizable: true`, remove min constraints, use `set_size()`.

**UI:**
- Size slider: 100px ↔ 300px, default 200px
- Live update: as slider moves, orb resizes in real time

---

## Risks and Constraints

1. **Orb position at extremes** — if the user sets the orb to 0% horizontal (left edge) or 100% (right edge), part of the orb may be off-screen. Need to clamp to `[orb_size/2, screen_width - orb_size/2]`.

2. **Multi-monitor** — `position_orb()` uses `win.current_monitor()`. If the user has multiple monitors, the percentages apply to the current monitor. This is probably fine but should be documented.

3. **DPI scaling** — the orb size is in logical pixels (200px), but `set_position` uses physical pixels. The existing code already handles this (`scale = monitor.scale_factor()`). New code must do the same.

4. **Settings sidebar window** — needs the same glass treatment as other sidebars (transparent, backdrop blur, DWM corners). This is well-established (`dyn_windows.rs` pattern).

5. **OAuth in settings vs setup** — the setup wizard already has OAuth. Having it in settings too is fine (settings is for changing later), but need to make sure the deep-link callback works from both contexts.

6. **API key security** — API keys should be masked in the UI (`type="password"` with a show/hide toggle). The sidecar stores them server-side; `settings.json` stores them locally. Need to decide which is the source of truth for each key.

7. **Live orb movement performance** — updating window position on every slider tick (mousedown→mousemove) could be janky. Debounce to 50ms or use `requestAnimationFrame`.

8. **Orb CSS size** — the `.orb` class in `styles.css` has `width: 80px; height: 80px`. The window is 200px but the orb visual is 80px (centered in the window). If the user changes orb size, both the window AND the CSS orb need to resize. The CSS orb size should be a percentage of the window or a CSS variable.

---

## Recommended Phasing

### Phase 1: Settings sidebar shell (no new features)
- Create `frontend/settings-sidebar.html`, `frontend/src/settings-sidebar/` (new React app)
- Create `WindowConfig::settings_sidebar()` in `dyn_windows.rs` (glass sidebar, 400×800)
- Add capability file
- Migrate the existing settings tabs into the sidebar layout
- Verify: settings sidebar opens, shows existing settings, glass background works

### Phase 2: Authentication section
- Add OAuth status/connect/disconnect UI calling existing sidecar endpoints
- Add Gemini API key field to `Settings` struct
- Add masked API key inputs for Gemini + Groq
- Verify: can connect/disconnect Google + GitHub, can set/remove API keys

### Phase 3: Volume slider
- Replace number input with slider
- Add "Test" button
- Verify: slider changes TTS volume, test button speaks at that volume

### Phase 4: Orb position + size sliders
- Add `orbHorizontalPct`, `orbVerticalPct`, `orbSize` to settings
- Modify `position_orb()` to read from settings
- Add Tauri commands for live position/size updates
- Add slider UI with live update
- Verify: orb moves and resizes as sliders move, position persists across restarts

### Phase 5: Polish
- Persist all settings across restarts (already works via `settings.json`)
- Add "reset to defaults" for orb position/size
- Add visual feedback (orb position preview in settings sidebar)
- Remove old settings window (optional, can keep as fallback)

---

*This is a research document. No code has been changed. Awaiting authorization to implement.*
