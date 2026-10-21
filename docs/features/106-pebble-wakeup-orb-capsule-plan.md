# Feature Plan: 106 — Pebble Wakeup Design for Voice Orb

**Date**: 2026-10-09  
**Status**: Plan & Localhost Preview Only (Awaiting User Review)  
**Feature ID**: 106  
**Scope**: Organic Pebble Morphology, Fluid Bezel Scoop Geometry, Wakeup Transitions, Localhost Interactive Showcase

---

## 1. Executive Summary & Core Requirements

Based on the user's uploaded screen capture (`media_1791559638603_e232f274.png`), this feature introduces an organic **Pebble Wakeup Design** for the Voice Orb background container.

### Directives:
1. **Preserve Previous Design**: The existing "Classic Capsule" (pitch black `#000000 !important` with 10px rounded corners) **must not be removed or altered**. The pebble design is strictly an additional selectable style.
2. **Organic Pebble Aesthetics**: Replace rigid rectangular boundaries with smooth, fluid, natural pebble contours and continuous curvature transitions.
3. **Pebble Wakeup Animation**: A dedicated dynamic entrance sequence where the pebble fluidly extrudes from the bezel or expands organically with spring physics as the Voice Orb wakes up.
4. **Localhost Preview First**: Implement an interactive showcase on `http://localhost:5173/pebble-showcase.html` allowing the user to review, test wakeup sequences, toggle top/bottom positions, and fine-tune aesthetics before enabling it in the Command Hub.

---

## 2. Pebble Morphologies Evaluated

We have developed 4 distinct variations for user testing in the localhost preview:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ 1. Fluid Bezel Scoop (Direct Hand Sketch Translation)                       │
│    A continuous catenary curve flowing from the screen bezel, wrapping      │
│    around the orb in a smooth organic bowl with zero-crease fillets.         │
├─────────────────────────────────────────────────────────────────────────────┤
│ 2. Polished Obsidian Pebble (High-Order Squircle)                           │
│    A tactile river-stone silhouette (border-radius: 42px / 36px) with       │
│    continuous curvature (G2), specular rim reflection, and deep shadows.   │
├─────────────────────────────────────────────────────────────────────────────┤
│ 3. Liquid Dynamic Island Pebble                                             │
│    An elongated organic pebble pod (border-radius: 9999px) docking flush    │
│    to the top/bottom bezel with fluid spring expansion on wake.             │
├─────────────────────────────────────────────────────────────────────────────┤
│ 4. Asymmetric Natural River Stone                                           │
│    An organic, sculpted asymmetric pebble evoking natural river basalt.     │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Wakeup Dynamics & Physics

The **Pebble Wakeup Sequence** features three stages:

### Stage 1: Fluid Extrusion / Spring Drop (0ms - 450ms)
- **Top Dock**: Originates at `height: 0px` along the top bezel and fluidly extrudes downward using an organic spring overshoot curve:
  $$\text{transition: transform 0.64s cubic-bezier}(0.34, 1.56, 0.64, 1)$$
- **Voice Orb Integration**: The internal WebGL particle system initiates `assemble()` at $+80\text{ms}$ into the drop, synchronizing the particle vortex with the container arrival.

### Stage 2: Resonant Settling & Breathing (450ms onwards)
- The pebble gently settles into position with a subtle micro-rebound.
- While active (Listening / Thinking / Speaking), a very slow organic breathing modulation ($0.3\%$ scale at $0.2\text{Hz}$) keeps the container visually dynamic.

### Stage 3: Fluid Retraction (Disperse / Sleep)
- Upon turn completion and speech finish, the pebble smoothly retracts back into the screen bezel (or scales down) as particles disperse.

---

## 4. Localhost Interactive Showcase (`pebble-showcase.html`)

An interactive testing suite served at `http://localhost:5173/pebble-showcase.html`:

- **Design Selector**: Switch instantaneously between:
  1. *Classic Capsule* (Existing reference)
  2. *Fluid Bezel Scoop* (User sketch)
  3. *Polished Obsidian Pebble* (Squircle stone)
  4. *Dynamic Island Pebble* (Fluid pod)
  5. *Asymmetric River Stone* (Natural basalt)
- **Position Toggle**: Test both **Top Dock** and **Bottom Dock**.
- **Interactive Wakeup Trigger**: "✦ Wake Orb" button triggers the full animated entrance, state transitions (`idle` $\to$ `listening` $\to$ `thinking` $\to$ `speaking` $\to$ `idle`), and exit retraction.
- **Audio Reactivity Simulation**: Slider to test amplitude pulsing of the pebble perimeter.
- **Live Curve & Dimension Tuning**: Real-time controls for width, depth, fillet radius, border specular, and shadow intensity.

---

## 5. Production Integration Plan (Post-User Review)

Once the user approves the preferred design on localhost:

1. **Settings Store (`frontend/src/store/assistant.ts`)**:
   Add `capsuleStyle: "classic" | "pebble"` (defaults to `"classic"`).
2. **Stage Renderer (`frontend/src/stage/OrbFrame.tsx`)**:
   Render `.orb-capsule` when `"classic"`, and `.orb-pebble` when `"pebble"`.
3. **Command Hub Settings (`frontend/src/settings-sidebar/SettingsSidebarApp.tsx`)**:
   Add a clean toggle card under Avatar / Orb settings:
   - `[Classic Capsule]` | `[Organic Pebble]`
4. **Rust Settings Persistence (`src-tauri/src/settings.rs`)**:
   Add `capsule_style: Option<String>` to `NexusSettings`.

---

## 6. Execution Steps
1. Build `frontend/pebble-showcase.html` with complete WebGL Voice Orb integration and CSS/SVG pebble designs.
2. Register `pebbleShowcase` in `frontend/vite.config.ts`.
3. Verify live in browser on `http://localhost:5173/pebble-showcase.html`.
4. Solicit user feedback on the curve and wakeup dynamics.
