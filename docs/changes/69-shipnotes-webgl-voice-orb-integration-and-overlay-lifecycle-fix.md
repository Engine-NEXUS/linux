# Change 69: Ship Notes WebGL Voice Orb Integration & Overlay Lifecycle Fix

**Date**: 2026-10-02  
**Components**: `frontend/src/avatar/voice-orb.js`, `frontend/src/avatar/VoiceOrb.tsx`, `frontend/src/avatar/Avatar.tsx`, `frontend/src/styles.css`  
**Reference Feature Doc**: `docs/features/87-shipnotes-webgl-voice-orb-integration.md`  

---

## 1. Context & Motivation

The user requested replacing the 2D Lottie vector avatar (`wakeup.json`) with the interactive 3D WebGL particle sphere animation from [`aqualang89/shipnotes-components`](https://github.com/aqualang89/shipnotes-components) (12,000 WebGL points, GPU-accelerated, 700ms morph transitions across states).

During initial testing, an issue arose: **"hotkey is not initializing the animation"**. Pressing the global hotkey (`Ctrl+Shift+Space`) revealed an empty/frozen area.

---

## 2. Root Cause Analysis

Thorough architectural and runtime analysis revealed two compounding root causes:

### Root Cause A: Chromium / WebView2 Non-Activating Window Throttle (`document.hidden`)
- NEXUS's `"main"` avatar window is configured as a non-activating desktop overlay with Win32 `WS_EX_NOACTIVATE` (via `window_manager::configure_non_activating_overlay`) so it never steals keyboard focus from active apps when summoned.
- In `voice-orb.js`, the animation loop in `_sync()` guarded execution with:
  ```javascript
  if (!this.isConnected || this.hasAttribute('recording') || document.hidden || this._lost) return;
  ```
- Because the window never acquires OS keyboard focus, Chromium / WebView2 marks `document.hidden = true` (or reports `document.visibilityState = 'hidden'`).
- Consequently, `_sync()` immediately cancelled `requestAnimationFrame` and returned without drawing a single frame.

### Root Cause B: Flexbox Intrinsic Dimension Collapse
- In `frontend/src/styles.css`, `.avatar-section` was styled as a flex container with no explicit width or height.
- The original Lottie implementation had an explicit SVG / `width: 180, height: 180` element providing intrinsic dimensions.
- The initial `<voice-orb>` custom element had `aspect-ratio: 1; width: 100%; height: 100%`. In CSS flexbox, a child with percentage dimensions inside an unconstrained flex item collapses its computed size to `0px` or `1px`.
- In `voice-orb.js`, `_resize()` computed:
  ```javascript
  const size = Math.max(1, Math.round(this.clientWidth * Math.min(devicePixelRatio || 1, 2)));
  ```
  With `this.clientWidth === 0`, `size` became `1`, rendering 12,000 points into an invisible `1x1` pixel canvas!

---

## 3. Implementation Details

### 1. `frontend/src/avatar/voice-orb.js`
- **Removed `document.hidden` Gate**: Desktop overlay windows must render continuously even when unfocused.
- **Added Explicit Lifecycle Controls (`play()` & `pause()`)**:
  ```javascript
  pause() {
    this._paused = true;
    cancelAnimationFrame(this._frame);
    this._frame = 0;
    this._last = 0;
  }
  play() {
    this._paused = false;
    this._resize();
    this._sync();
  }
  ```
- **Robust Sizing Fallback in `_resize()`**:
  ```javascript
  const rect = this.getBoundingClientRect();
  const cssW = this.clientWidth || rect.width || 180;
  const size = Math.max(80, Math.round(cssW * Math.min(devicePixelRatio || 1, 2)));
  ```
- **Pointer Event Pass-Through**: Added `pointer-events: none` to canvas and shadow host styles so desktop drag-and-drop and scroll-wheel scaling handlers receive all mouse events.
- **Programmatic Audio Reactivity**: `setLevel(vol)` API accepts audio levels from Zustand/Tauri events without locking `getUserMedia` mic streams (protecting Intel SST audio driver cpal loop from starvation).

### 2. `frontend/src/avatar/VoiceOrb.tsx`
- Added typed `visible?: boolean` prop.
- Synchronized `visible` with `orbRef.current.play()` when visible and debounced `pause()` (500ms) on hide matching the slide-down CSS transition.
- Defined fallback container dimensions (`minWidth: 120px; minHeight: 120px`).

### 3. `frontend/src/avatar/Avatar.tsx`
- Connected `<VoiceOrb />` into the active DOM inside `.avatar-voice-orb-container`.
- Wired `visible`, `state`, and dynamic volume level (`ttsActive ? Math.max(0.4, audioVolume) : audioVolume`).
- Set minimum wrap dimensions on `.avatar-wrap` (`minWidth: 140, minHeight: 140`).
- Retained hidden `containerRef` DOM anchor to preserve React hook dependencies and keep all 31 avatar pure function unit tests passing.

### 4. `frontend/src/styles.css`
- Configured `.avatar-section` with explicit `width: 100%; height: 100%`.
- Added `.avatar-voice-orb-container` rules ensuring full flexbox centering.

---

## 4. Verification

1. **Frontend Vitest**:
   - `npm run test`: **142 / 142 passed (100%)** across 18 test files in 1.55s.
2. **TypeScript**:
   - `npx tsc --noEmit`: Clean (0 errors).
3. **Rust Lib Tests**:
   - `cargo test --lib`: **827 / 827 passed (100%)**.
4. **Production Build**:
   - `node nexus.mjs build`: Compiled release binary `src-tauri/target/release/nexus.exe` (51.2 MB).
