# Feature 82 — Interactive Animation Calibration HUD & Command Hub Overlay

**Date:** 2026-10-01  
**Category:** Visual Experience & Desktop Layout Control  
**Status:** SPECIFIED & READY FOR IMPLEMENTATION  
**Research Spec:** `docs/research/command-hub/01-interactive-animation-alignment-and-calibration-architecture-2026-10-01.md`  
**Execution Plan:** `animation_alignment_calibration_plan.md`  

---

## 1. Feature Overview

Feature 82 provides an interactive on-screen calibration HUD that lets users visually position and scale all three primary NEXUS avatar animations directly on the live desktop:
1. **Wakeup Orb**: Smiling/idle/listening avatar.
2. **Ghost Waves**: Multi-color 3-bar audio reactive soundwaves.
3. **Loading Indicator**: 3-circle Lottie loop running during long cloud inferences.

The user launches the calibrator with one click from the Command Hub. The Command Hub sidebar temporarily disappears to leave the screen completely unobstructed. A floating liquid-glass toolbar appears at the top center of the screen, providing 3 target selector tabs, 3 live sliders (Horizontal, Vertical, Size), instantaneous synchronous desktop preview, Undo/Redo history, and Save/Cancel buttons that restore the Command Hub.

---

## 2. User Interaction & Workflow

```
[ Command Hub (Settings) ]
       │
       ▼ (Click: "Launch On-Screen Calibrator")
[ Command Hub Hides ] ──────► [ Top-Center Calibration HUD Appears ]
                                       │
                                       ├─ Select: [ Wakeup | Waves | Loading ]
                                       │
                                       ├─ Drag: [ Horizontal (0-100%) ]
                                       │        [ Vertical (0-100%)   ]
                                       │        [ Size (px bounds)    ]
                                       │        (Animation moves & resizes live on desktop)
                                       │
                                       ├─ Click: [ ↶ Undo | ↷ Redo ]
                                       │
                                       ├─ Click: [ ✕ Cancel ] ──► (Revert changes & return to Settings)
                                       │
                                       └─ Click: [ ✓ Save ]   ──► (Persist settings & return to Settings)
```

---

## 3. UI Specifications

### 3.1 Command Hub Launchpad Card
- **Location:** `SettingsSidebarApp.tsx` $\to$ `Display` Tab.
- **Card Design:**
  - Card Title: **Animation Alignment & Calibration**
  - Card Description: *Visually position and resize the Wakeup Orb, Ghost Waves, and Loading Indicator across your workspace using an interactive on-screen alignment HUD.*
  - Primary Action Button:
    - Label: **"Launch On-Screen Calibrator"**
    - Icon: Compass / crosshairs icon (`⌖` or `🎯`)
    - Styling: Gradient pill button (`#259ed6` to `#1d4ed8`), elevated with liquid-glass backdrop.
  - Secondary Action:
    - Button: **"Reset All to Defaults"** (Orb & Waves at center-bottom 200px, Loading at top-right 80px).

### 3.2 Top-Center Floating HUD (`calibrate-toolbar`)
- **Dimensions:** 580px width × 230px height, non-resizable, frameless, transparent.
- **Position:** Top-center of the screen with 24px vertical offset from the top display bezel.
- **Layout:**
  - **Header Row:**
    - Left: Badge `NEXUS ALIGNMENT HUD` + Target Subtext.
    - Right: Undo Button (`↶`), Redo Button (`↷`), and Reset Button (`↺`).
  - **Target Segmented Switcher:**
    - 3 pills: `🌟 Wakeup Orb`, `🌊 Ghost Waves`, `⏳ Loading Indicator`.
    - Clicking a pill immediately updates the on-screen desktop preview to reflect that animation at its current calibrated coordinates.
  - **Slider Controls (3 Rows):**
    - Row 1: **Horizontal**: Left (`0%`) $\leftrightarrow$ Right (`100%`) with value readout (e.g. `50% Center`).
    - Row 2: **Vertical**: Top (`0%`) $\leftrightarrow$ Bottom (`100%`) with value readout (e.g. `100% Bottom`).
    - Row 3: **Size**: Scale bounds (e.g. `100px` $\leftrightarrow$ `300px` for Orb/Waves; `40px` $\leftrightarrow$ `160px` for Loading).
  - **Footer Action Row:**
    - Left: `✕ Cancel` (Hotkey: `Esc`). Discards changes, closes HUD, and restores Command Hub.
    - Right: `✓ Save Changes` (Hotkey: `Enter`). Persists to `settings.json`, closes HUD, and restores Command Hub with a success toast notification.

---

## 4. Settings Data Model & Defaults

The following 9 keys are supported in `NexusSettings`:

| Key Name (JSON/camelCase) | Rust Field | Type | Default | Description |
|---|---|---|---|---|
| `orbHorizontalPct` | `orb_horizontal_pct` | `f64` | `0.5` | Wakeup orb horizontal percentage (0.0 = left, 1.0 = right) |
| `orbVerticalPct` | `orb_vertical_pct` | `f64` | `1.0` | Wakeup orb vertical percentage (0.0 = top, 1.0 = bottom) |
| `orbSize` | `orb_size` | `u32` | `200` | Wakeup orb window and visual diameter in pixels (100–400; raised 2026-10-04 for the fullscreen-era orb) |
| `wavesHorizontalPct` | `waves_horizontal_pct` | `f64` | `0.5` | Ghost waves horizontal percentage (0.0 = left, 1.0 = right) |
| `wavesVerticalPct` | `waves_vertical_pct` | `f64` | `1.0` | Ghost waves vertical percentage (0.0 = top, 1.0 = bottom) |
| `wavesSize` | `waves_size` | `u32` | `200` | Ghost waves width and scale in pixels (100–400; raised 2026-10-04) |
| `loadingHorizontalPct` | `loading_horizontal_pct` | `f64` | `0.95` | Loading indicator horizontal percentage (0.0 = left, 1.0 = right) |
| `loadingVerticalPct` | `loading_vertical_pct` | `f64` | `0.02` | Loading indicator vertical percentage (0.0 = top, 1.0 = bottom) |
| `loadingSize` | `loading_size` | `u32` | `80` | Loading indicator width/height in pixels (40–160) |

---

## 5. Verification & Acceptance Criteria

1. **Modal Handshake:** Clicking "Launch On-Screen Calibrator" hides the Command Hub and opens the HUD at top-center within 150ms.
2. **Desktop Preview Fidelity:** When switching between Wakeup, Waves, and Loading tabs, the correct animation renders on the desktop without delay.
3. **Real-time Synchronous Motion:** Dragging any of the 3 sliders repositions or resizes the live desktop animation at 60 FPS without jitter or latency.
4. **Fluid Avatar Scaling:** Wakeup Orb and Ghost Waves scale smoothly up to 300px and down to 100px without SVG stroke clipping.
5. **Undo/Redo History:** The user can move the sliders multiple times and click Undo (`↶`) or press `Ctrl+Z` to step back cleanly through prior states.
6. **Cancel Restoration:** Clicking Cancel or pressing `Esc` restores all animations to their initial pre-session state and re-opens the Command Hub.
7. **Save Persistence:** Clicking Save writes all 9 parameters to `settings.json`, destroys the calibration window, and re-opens the Command Hub.
