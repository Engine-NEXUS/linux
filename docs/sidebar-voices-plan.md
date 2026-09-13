# Plan: Sidebar Width, Live Orb Preview, Voice Demos, Settings Blur Match

## Research Summary

### How the existing sidebar works

**Window creation** (`dyn_windows.rs`):
- `sidebar()` → 600px wide, 1000px tall, transparent, `decorations: false`, `always_on_top: true`, `focus: false`, `skip_taskbar: true`
- `settings_sidebar()` → 720px wide, 1000px tall, same flags
- Both get DWM rounded corners + `WDA_EXCLUDEFROMCAPTURE` on Windows
- Both get `apply_vibrancy(NSVisualEffectMaterial::Sidebar)` on macOS

**Blur effect** (the "liquid glass" look):
1. Rust captures the desktop pixels behind the window via `BitBlt` (Windows)
2. Rust applies a Gaussian blur (sigma=32) to the captured pixels
3. Rust emits a `sidebar:backdrop` event with a `data:image/png;base64,...` URI
4. Frontend sets `--sidebar-backdrop-image` CSS variable on `<html>`
5. CSS `::after` renders the blurred image as a background layer (`z-index: 0`)
6. CSS `::before` renders a specular edge sheen (`z-index: 1`)
7. Content sits at `z-index: 2`
8. A live-blur loop re-captures every 1s and emits new frames

**CSS structure** (both sidebars use the same pattern):
- `.sidebar-card` / `.settings-container` → the glass card
- `background-color: rgba(20, 20, 22, 0.92)` → dark tint over the blur
- `border: 1px solid rgba(255, 255, 255, 0.15)` → glass rim
- `border-radius: 18px` → rounded corners (matches DWM corners)
- `box-shadow` → inset highlights + drop shadow
- `::after` → blurred backdrop layer
- `::before` → specular edge sheen

### Current settings sidebar vs main sidebar differences

| Aspect | Main sidebar | Settings sidebar |
|--------|-------------|-----------------|
| Width | 600px | 720px |
| Card class | `.sidebar-card` | `.settings-container` |
| Backdrop `::after` | ✅ Same | ✅ Same |
| Sheen `::before` | ✅ Same | ✅ Same |
| `background-color` | `rgba(20, 20, 22, 0.92)` | `rgba(20, 20, 22, 0.92)` |
| `border-radius` | 18px | 18px |
| `box-shadow` | ✅ Same | ✅ Same |
| z-index layering | ✅ Same | ✅ Same |
| Live blur loop | ✅ Yes (1s interval) | ✅ Yes (1s interval) |
| `@import tokens.css` | ✅ Yes | ❌ No |
| Apple Music tokens | ✅ Yes | ❌ No |

**Finding:** The CSS is already a carbon copy. The settings sidebar CSS is missing `@import "../theme/tokens.css"` which defines Apple Music design tokens. This may cause subtle color differences. The blur mechanism is identical.

### Current orb position slider behavior

- Sliders call `liveUpdate(h, v, size)` on every `onChange`
- `liveUpdate` invokes `set_orb_position` Tauri command
- `set_orb_position` moves the orb window immediately
- **Problem:** The orb is the main window — it moves, but the user can't see it because the settings sidebar is covering the right edge of the screen where the orb likely is
- **Fix needed:** Show a visual preview of the orb position within the settings sidebar itself (the `.position-preview` box already exists but is static)

### Current audio tab

- Only has a volume slider + "Test" button (speaks "Testing volume level")
- No voice selection
- No voice demo playback
- No list of available voices

---

## Phase 1: Reduce sidebar width by 200px

**Change:** 600px → 400px for the response sidebar

Files to edit:
1. `src-tauri/src/dyn_windows.rs` — `sidebar()` width: 600 → 400, min_width: 600 → 400
2. `src-tauri/src/commands.rs` — all hardcoded `600i32` → `400i32` (6 occurrences in sidebar positioning math)
3. `src-tauri/src/commands.rs` — `tauri::LogicalSize::new(600.0, 1000.0)` → `400.0` (2 occurrences)

**Risk:** The sidebar content (text responses, analysis dashboards) was designed for 600px. At 400px, some content may overflow or look cramped. The CSS uses `width: 100%` so it will adapt, but charts/tables may need horizontal scrolling.

**Verification:** Build + launch + say a command that triggers the sidebar → check it's 400px wide and content fits.

---

## Phase 2: Live orb position preview animation

**Problem:** When the user moves the orb position slider, the orb window moves on the desktop, but the user can't see it because the settings sidebar is on top.

**Fix:** Make the `.position-preview` box inside the settings sidebar show a live animated orb that moves in real-time as the slider drags.

Current state:
- `.position-preview` is a static box with a dot
- The dot position updates via React state (`hPct`, `vPct`)
- The dot already moves when the slider changes

What's missing:
- The orb animation (the actual pulsing/breathing animation that the real orb has)
- Smooth transition on the dot position (so it glides, not jumps)
- The dot should look like the actual orb (gradient, glow, pulse)

Files to edit:
1. `frontend/src/settings-sidebar/settings-sidebar.css` — add orb animation to `.position-preview-orb` (pulse, glow, gradient)
2. `frontend/src/settings-sidebar/SettingsSidebarApp.tsx` — add `transition: all 0.1s` for smooth movement

**Implementation:**
```css
.position-preview-orb {
  background: radial-gradient(circle, rgba(250, 88, 106, 0.9) 0%, rgba(250, 88, 106, 0.3) 70%);
  border-radius: 50%;
  box-shadow: 0 0 12px rgba(250, 88, 106, 0.6);
  transition: left 0.08s ease-out, top 0.08s ease-out, width 0.08s ease-out, height 0.08s ease-out;
  animation: orb-pulse 2s ease-in-out infinite;
}

@keyframes orb-pulse {
  0%, 100% { transform: scale(1); box-shadow: 0 0 12px rgba(250, 88, 106, 0.6); }
  50% { transform: scale(1.1); box-shadow: 0 0 18px rgba(250, 88, 106, 0.8); }
}
```

**Also:** The slider already calls `liveUpdate` which invokes `set_orb_position` — so the real orb IS moving. The user just needs to see the preview too. Both happen simultaneously.

---

## Phase 3: Voice demos in Audio tab

**Goal:** Show all available TTS voices with a demo button for each.

### Available voices

**Edge TTS (cloud, primary):** 400+ voices. The relevant en-US voices are:
- en-US-AvaNeural (female, default)
- en-US-AndrewNeural (male)
- en-US-EmmaNeural (female)
- en-US-BrianNeural (male)
- en-US-ChristopherNeural (male)
- en-US-EricNeural (male)
- en-US-GuyNeural (male)
- en-US-JennyNeural (female)
- en-US-MichelleNeural (female)
- en-US-RogerNeural (male)
- en-US-SteffanNeural (male)
- en-US-AriaNeural (female)
- en-US-DavisNeural (male)
- en-US-NancyNeural (female)
- en-US-SaraNeural (female)

**Piper (local fallback):** Only `en_US-amy-medium` (female, 60 MB)

### Implementation plan

1. **New Tauri command: `speak_text_with_voice`**
   - File: `src-tauri/src/tts.rs`
   - Like `speak_text` but takes a `voice` parameter
   - Uses edge-tts for cloud voices, Piper for local
   - Returns audio bytes or plays directly

2. **New Tauri command: `list_tts_voices`**
   - File: `src-tauri/src/commands.rs`
   - Returns a JSON array of available voices with metadata:
     ```json
     [
       { "id": "en-US-AvaNeural", "name": "Ava", "gender": "Female", "provider": "edge-tts", "language": "en-US" },
       { "id": "en-US-AndrewNeural", "name": "Andrew", "gender": "Male", "provider": "edge-tts", "language": "en-US" },
       { "id": "piper-amy", "name": "Amy (Offline)", "gender": "Female", "provider": "piper", "language": "en-US" }
     ]
     ```

3. **New Tauri command: `preview_voice`**
   - File: `src-tauri/src/tts.rs`
   - Synthesizes a short demo phrase ("Hello, I'm Ava. This is how I sound.")
   - Plays it immediately
   - Uses the specified voice ID

4. **Frontend: Voice picker in Audio tab**
   - File: `frontend/src/settings-sidebar/SettingsSidebarApp.tsx`
   - Replace the simple "Test" button with a scrollable voice list
   - Each voice row has: name, gender badge, provider badge, ▶ Play button
   - Clicking Play calls `preview_voice` with that voice ID
   - Currently selected voice has a highlight/radio indicator
   - Selecting a voice saves it to `settings.edgeTtsVoice`

5. **Voice list fetching**
   - On Audio tab mount, call `list_tts_voices`
   - Cache the result in React state
   - Show a loading spinner while fetching

### UI design for voice list

```
┌─────────────────────────────────────────────┐
│ Voice Selection                             │
├─────────────────────────────────────────────┤
│ ● Ava          Female  Cloud    ▶ Play      │
│   Andrew       Male    Cloud    ▶ Play      │
│   Emma         Female  Cloud    ▶ Play      │
│   Brian        Male    Cloud    ▶ Play      │
│   ...                                        │
│   Amy (Offline) Female  Local   ▶ Play      │
└─────────────────────────────────────────────┘
```

---

## Phase 4: Settings sidebar blur effect match

**Finding from research:** The settings sidebar CSS is already a carbon copy of the main sidebar CSS. Both use:
- Same `::after` backdrop layer
- Same `::before` specular sheen
- Same `background-color: rgba(20, 20, 22, 0.92)`
- Same `border-radius: 18px`
- Same `box-shadow` stack
- Same z-index layering

**The only difference is:** The settings sidebar CSS is missing `@import "../theme/tokens.css"` which defines Apple Music design tokens (colors, gradients). Without these tokens, any CSS that references `var(--apple-text-primary)` etc. will use fallback values.

**Fix:**
1. Add `@import "../theme/tokens.css";` at the top of `settings-sidebar.css`
2. Verify the backdrop event listener is working (it is — line 104 of SettingsSidebarApp.tsx)
3. Verify the live-blur loop is running (it is — lines 1076-1100 of commands.rs)

**If the blur still doesn't match after adding the import:** The issue may be that the settings sidebar window is wider (720px) than the capture region. The capture coordinates in `show_settings_sidebar` use `720i32` which matches the window width, so this should be correct.

**Additional check:** The `show_settings_sidebar` function emits `sidebar:backdrop` (line 1065) which is the same event name the settings sidebar listens for (line 104 of SettingsSidebarApp.tsx). This is correct.

**Verification:** Open the settings sidebar and the response sidebar side by side → compare the blur/glass appearance.

---

## Execution Order

1. **Phase 1** — Reduce sidebar width (Rust changes, 8 edits)
2. **Phase 4** — Fix settings blur (1 CSS import line)
3. **Phase 2** — Live orb preview animation (CSS + TSX)
4. **Phase 3** — Voice demos (Rust command + frontend UI)
5. **Build + test** — `nexus build` + manual testing

## Files to edit

| File | Phase | Changes |
|------|-------|---------|
| `src-tauri/src/dyn_windows.rs` | 1 | sidebar width 600→400 |
| `src-tauri/src/commands.rs` | 1 | 600→400 in 8 places |
| `frontend/src/settings-sidebar/settings-sidebar.css` | 2, 4 | orb animation + tokens import |
| `frontend/src/settings-sidebar/SettingsSidebarApp.tsx` | 2, 3 | orb preview + voice list UI |
| `src-tauri/src/tts.rs` | 3 | `preview_voice` command |
| `src-tauri/src/commands.rs` | 3 | `list_tts_voices` command |
| `src-tauri/src/lib.rs` | 3 | register new commands |
