# Feature 85 — Live Optical Pass-Through Frosted Glass & Automatic Luminance Adaptation

**Date:** 2026-10-01  
**Category:** Window Compositing / Optical DWM Blur / Liquid Glass Material  
**Status:** SUPERSEDED — DWM passthrough path rejected by ADR-05 (`docs/architecture/06-liquid-glass-screenshot-blur.md`: DWM materials render solid-opaque on non-activating windows; see `docs/changes/62-ghost-fullscreen-blackout-fix.md`). Blur ships via the screenshot-capture pipeline instead. This spec is retained as design research only.  
**Supersession note (2026-10-02):** the `settings-sidebar.html` / `architect.html` / `pr-list.html` standalone pages listed below were removed — all views now render inside the unified `sidebar` window (`sidebar.html`, see `docs/changes/66-implementation-patterns-quick-reference.md` Pattern 2).  
**Research Spec:** `docs/research/liquid-glass/02-live-dwm-passthrough-blur-and-adaptive-luminance-architecture-2026-10-01.md`  
**Reference Asset:** User Uploaded Image `media_1790832308311.png` (Apple Liquid Glass Adaptive Split Demo)  

---

## 1. Feature Overview & Scope

Feature 85 implements **True Live Optical Pass-Through Frosted Glass** across all NEXUS overlay surfaces:
- **Response Sidebar** (`sidebar.html`)
- **Command Hub / Settings Sidebar** (`settings-sidebar.html`)
- **Architect Sidebar** (`architect.html`)
- **PR List Sidebar** (`pr-list.html`)
- **Floating Overlay Buttons & HUD Pills** (e.g. `calibrate-toolbar` / overlay action pills)

### Core User Invariants:
1. **Live Optical Motion Pass-Through:**
   Unlike a frozen snapshot, moving windows, active browser tabs, video playback, and moving mouse cursors behind the glass are **seen live through the glass in real-time at 60Hz/120Hz/144Hz**, transformed into a smooth, realistic frosted blur.
2. **Automatic Luminance Adaptation:**
   The glass automatically senses whether the desktop background directly behind it is light/vibrant or dark/shadowed (as demonstrated in the user's reference split-button image `media_1790832308311.png`).
3. **Permanent Crisp White Text:**
   The typography remains **100% white (`#FFFFFF`) across all backgrounds** without the user ever needing to switch themes or toggle settings.

---

## 2. Visual Architecture & State Mapping

```
                                [ Windows Desktop Background ]
                                               │
                        ┌──────────────────────┴──────────────────────┐
                        ▼                                             ▼
            [ LIGHT / VIBRANT BACKGROUND ]                [ DARK / OBSIDIAN BACKGROUND ]
            (Luminance Y > 130)                           (Luminance Y <= 130)
                        │                                             │
                        ▼                                             ▼
           ┌─────────────────────────────┐               ┌─────────────────────────────┐
           │   LIGHT FROSTED GLASS       │               │   DARK FROSTED GLASS        │
           │ • Live DWM Optical Blur     │               │ • Live DWM Optical Blur     │
           │ • Tint: rgba(255,255,255,0.2)│              │ • Tint: rgba(0,0,0,0.38)    │
           │ • Saturation: 160%          │               │ • Saturation: 130%          │
           │ • Occlusion Shadow: 0 1px 3px│              │ • Specular Rim: 0.18 alpha  │
           │ • Text: PURE WHITE (#FFFFFF)│               │ • Text: PURE WHITE (#FFFFFF)│
           │   (Protected by depth shadow│               │   (Luminescent glow)        │
           │   and occlusion rim)        │               │                             │
           └─────────────────────────────┘               └─────────────────────────────┘
```

---

## 3. Technical Implementation Specification

### 3.1 Live DWM Compositor Activation (`live_glass.rs`)
In `src-tauri/src/live_glass.rs`:
- Uses the Win32 `SetWindowCompositionAttribute` API directly on the window's `HWND`:
  ```rust
  #[repr(C)]
  pub struct AccentPolicy {
      pub accent_state: u32,   // 3 = ACCENT_ENABLE_BLURBEHIND, 4 = ACCENT_ENABLE_ACRYLICBLURBEHIND
      pub accent_flags: u32,   // 2 = DRAW_ALL_BORDERS
      pub gradient_color: u32, // 0x00000000 (fully transparent mask so CSS controls the tint)
      pub animation_id: u32,
  }
  ```
- **Focus Resilience:** Overrides `WM_NCACTIVATE` and applies `DWMWA_TRANSITIONS_FORCEDISABLED` so that DWM never dims or disables the blur when the window is non-activating or loses focus.

### 3.2 Real-Time Background Luminance Probe (`luminance_probe.rs`)
- Runs a lightweight Tokio task sampling an 8×8 grid beneath the window rect every 250ms via GDI `GetPixel` or `BitBlt` (<0.1ms compute time).
- Computes ITU-R BT.709 relative luminance:
  $$Y = 0.2126 \cdot R + 0.7152 \cdot G + 0.0722 \cdot B$$
- Applies a 25-point hysteresis filter ($Y_{light} > 140$, $Y_{dark} < 115$) to prevent rapid flickering when moving across desktop contrast borders.
- Dispatches event to frontend:
  ```json
  { "mode": "light", "luminance": 162.4 }
  ```

### 3.3 CSS Liquid Glass Styling Engine (`liquid-glass.css`)
```css
/* Base Live Glass Container */
.live-glass-surface {
  background: transparent !important;
  color: #ffffff !important;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  transition: all 250ms cubic-bezier(0.4, 0, 0.2, 1);
}

/* Light / Colorful Desktop State (Left half of reference image) */
[data-glass-luminance="light"] .live-glass-card {
  background-color: rgba(255, 255, 255, 0.24);
  border: 1.5px solid rgba(255, 255, 255, 0.55);
  box-shadow: 
    0 16px 40px rgba(0, 0, 0, 0.14),
    inset 0 1px 1.5px rgba(255, 255, 255, 0.80),
    inset 0 -1px 2px rgba(0, 0, 0, 0.10);
  backdrop-filter: saturate(160%) contrast(108%);
  -webkit-backdrop-filter: saturate(160%) contrast(108%);
}

[data-glass-luminance="light"] .live-glass-text {
  color: #ffffff !important;
  text-shadow: 0 1px 3px rgba(0, 0, 0, 0.42), 0 2px 8px rgba(0, 0, 0, 0.18);
}

/* Dark Desktop State (Right half of reference image) */
[data-glass-luminance="dark"] .live-glass-card {
  background-color: rgba(0, 0, 0, 0.38);
  border: 1.5px solid rgba(255, 255, 255, 0.18);
  box-shadow: 
    0 20px 50px rgba(0, 0, 0, 0.45),
    inset 0 1px 1px rgba(255, 255, 255, 0.35),
    inset 0 -1px 1px rgba(0, 0, 0, 0.35);
  backdrop-filter: saturate(130%) contrast(102%);
  -webkit-backdrop-filter: saturate(130%) contrast(102%);
}

[data-glass-luminance="dark"] .live-glass-text {
  color: #ffffff !important;
  text-shadow: 0 0 14px rgba(255, 255, 255, 0.30);
}
```

---

## 4. Verification & Acceptance Criteria

1. **Live Motion Pass-Through:**
   - Dragging a browser window or tab behind the Response Sidebar shows the content moving live through the glass in real-time.
   - Moving the mouse cursor behind a transparent region displays the blurred cursor moving in real-time.
2. **Automatic Adaptation:**
   - Placing the glass button over a bright, colorful wallpaper or white document triggers `light` mode: the glass turns milky white with saturation boost and specular sheen.
   - Moving it over a dark IDE or black wallpaper triggers `dark` mode: the glass turns deep obsidian.
3. **Text Legibility Invariant:**
   - All text remains pure white (`#FFFFFF`) with 100% legibility (WCAG AA > 4.5:1) in both modes.
4. **Performance Gate:**
   - GPU-accelerated blur executes at the monitor's native refresh rate (60Hz–144Hz) with 0% CPU overhead.
   - The luminance probe consumes <0.01% CPU.
