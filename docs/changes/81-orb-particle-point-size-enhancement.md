# Change 81: Orb Particle Point Size Enhancement (2026-10-05)

## Problem & Motivation
The user requested: "increase the size of the particales in the orb".
While Change 80 enlarged the overall sphere projection scale (`0.61` → `0.72`), individual particle points were still calculated with a strict clamp (`max(1.6, point * pixels / 720.0)`), which on high-DPI displays rendered particles as microscopic dust specks. The user wanted each particle to be noticeably larger, luminous, and well-defined like the beaded starburst and glowing sphere reference designs.

## Implementation Details

### 1. WebGL Vertex Shader (`frontend/src/avatar/voice-orb.js`)
- Scaled `point` base and depth/rim weights:
  - Base: `1.1` → `1.9`
  - Depth weight: `0.9` → `1.4`
  - Rim weight: `0.45` → `0.7`
  - Pop sparkle boost: `3.6` → `4.5`
- Scaled `gl_PointSize` calculation:
  - Floor raised: `1.6` → `2.6` px
  - Divisor tightened: `pixels / 720.0` → `pixels / 480.0`
  - Result: Standard particles now render at ~2.8px to 4.2px (with dynamic pops reaching ~6.5px), reading as clear glowing beaded points rather than micro-dust.

### 2. 2D Canvas Fallback Parity (`frontend/src/avatar/voice-orb.js`)
- Updated `dot` radius calculation in `_paint2D`:
  - `Math.max(0.8, size/720 * ...)` → `Math.max(1.3, size/480 * (1.1 + .4*(pz+1) + rim*.8) * ...)`
  - Preserved identical visual weight across both WebGL and CPU 2D render paths.

## Verification
- `npx tsc --noEmit`: Clean (0 errors).
- `vitest run`: All 154/154 tests pass across 21 suites.
