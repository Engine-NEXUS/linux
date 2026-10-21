# Feature 87: Ship Notes WebGL Voice Orb Integration

**Status**: IMPLEMENTED & VERIFIED  
**Date**: 2026-10-02  
**Component**: `frontend/src/avatar/voice-orb.js`, `frontend/src/avatar/VoiceOrb.tsx`, `frontend/src/avatar/Avatar.tsx`, `frontend/src/styles.css`  
**Source**: [`aqualang89/shipnotes-components`](https://github.com/aqualang89/shipnotes-components/tree/main/components/voice-orb)  
**Detailed Change Log**: `docs/changes/69-shipnotes-webgl-voice-orb-integration-and-overlay-lifecycle-fix.md`  

---

## 1. Overview

Replaced the legacy 2D Lottie vector avatar (`wakeup.json`) with the **Ship Notes WebGL Voice Orb** from `@aqualang89`. 
The avatar is now an interactive 3D particle sphere composed of 12,000 living WebGL points floating over the desktop on a transparent background.

---

## 2. Morphing States & Shader Visuals

The orb smoothly interpolates over **700ms** between 4 distinct states matching NEXUS's `AssistantState`:

| Assistant State | Color Palette | Visual Shader Behavior |
| :--- | :--- | :--- |
| **`idle`** | Soft Lilac (`#9e99de`) | Calm breathing sphere, slow organic drift (0.85 Hz). |
| **`listening`** | Vibrant Turquoise (`#29e8b5`) | Radial suction waves pulling points inward toward the center. Reacts to user speech volume. |
| **`thinking`** | Electric Purple / Magenta (`#d946ef` / `#c026d3`) | 3D rotating purple beaded starburst: 64 radial rays, intense white-magenta central nucleus, 15 concentric beaded steps, dual-axis 3D rotation, and outward wave pulse. |
| **`speaking`** | Pink & Violet (`#f5459e`) | Outward wave blast with bass deformation and transient sparks ejected on vocal peaks. |

---

## 3. Desktop Overlay & Lifecycle Architecture

### Non-Activating Desktop Overlay Handling
Because NEXUS's floating orb window is an unfocused desktop overlay (`WS_EX_NOACTIVATE` Win32 style), standard web browser assumptions break down:
1. **No `document.hidden` throttle**: WebView2 marks non-focused overlay windows as `document.hidden = true`. In `voice-orb.js`, this check was removed and replaced with an explicit `play()` / `pause()` lifecycle tied directly to the window's `visible` Zustand state.
2. **Dimension Collapse Prevention**: Flexbox percentage sizing in unconstrained containers can evaluate `this.clientWidth` to `0px` during offscreen mount. `_resize()` uses `getBoundingClientRect()` with an explicit `Math.max(80, ...)` floor and `.avatar-wrap` enforces a minimum 140×140px footprint.
3. **Zero Mic Lock Conflict**: `VoiceOrb` does not initiate a duplicate WebView2 `getUserMedia` mic stream (preventing Intel SST audio driver starvation of Rust's `cpal` capture loop). Audio reactivity is driven cleanly via `setLevel(vol)` hooked to `audioVolume` and `ttsActive`.
4. **Pointer Events Pass-Through**: WebGL canvas elements are styled with `pointer-events: none` in shadow DOM so desktop dragging (`startDragging`) and scroll-wheel resize calibration (100–300px) remain functional.

---

## 4. Verification

- **Frontend Tests**: 142 / 142 passed (100%) in Vitest across 18 test suites.
- **TypeScript**: Clean (`npx tsc --noEmit` exited 0).
- **Rust Backend**: 827 / 827 passed (100%) in `cargo test --lib`.
- **Release Binary**: `src-tauri/target/release/nexus.exe` (51.2 MB).
