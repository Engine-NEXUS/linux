# Change 80: Orb Particle Text & Scale Upgrades (2026-10-04)

## Problem & Motivation
The user requested further refinements to the Voice Orb's presence and particle text rendering:
1. **Orb Size & Density:** The user requested "add 200 more particles and increase the size of it".
2. **Text Rendering:** The user requested that the particle text look "filled inside" rather than just outlined, and that it should appear only for a "fraction of a second" before the regular HTML caption takes over, with a decreased font size.

## Implementation Details

### 1. Scale & Particle Density (`voice-orb.js`, `VoiceOrb.tsx`)
- Increased the default WebGL particle count from `5000` to `5200` (and the coarse pointer fallback from `4000` to `4200`).
- Increased the overall orb scale by modifying the `perspective` projection multiplier in the vertex shader (`gl_Position`) from `0.61` to `0.72`. This makes the orb visually larger on the desktop overlay without breaking its bounding box.

### 2. Filled Particle Text (`voice-orb.js`)
- Modified `sampleTextPoints()` to increase the particle sampling density. Previously, it skipped every 2 pixels (`y+=2`, `x+=2`), which resulted in a wireframe/hollow look for the text glyphs. It now samples every pixel (`y+=1`, `x+=1`), drawing 4× as many particles to the text targets, resulting in a solid, "filled inside" appearance.
- Adjusted the base font size down from `84px` to `54px` and constrained the max width to `W * 0.70`, ensuring the text reads clearly and compactly at the center of the orb.

### 3. Flash-Text Timing (`voice-orb.js`)
- Decreased the default `holdMs` for the particle text phase from `1600ms` to `400ms` (and lowered the hard minimum from `600ms` to `300ms`). 
- When the `orb:show_text` event fires, the particles rapidly morph into the spoken word, hold for a fraction of a second, and immediately dissolve back into the current orb state as the standard DOM-based `ResponseCaption` continues to display above it.

## Verification
- **Rust Backend:** Compiled successfully; no structural changes.
- **Frontend Tests:** Ran `vitest run` — all 154/154 tests passed across 21 test suites.
- **Visuals:** Verified the orb occupies more of the viewport and text particles form a dense, filled shape before dissolving.
