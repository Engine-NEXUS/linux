# Change 71 — 3D Rotating Purple Starburst Thinking Avatar

## Overview
Replaced the `thinking` state avatar animation in the 3D WebGL particle sphere with a **3D rotating purple beaded starburst / radial ray sphere**, directly grounded in the user's reference design (`media_1791105468869.png`).

The previous implementation used 20 loose ribbons with organic noise bends and 1D Y-axis spinning, reading as smoke wisps rather than a sharp geometric starburst. The new implementation features:
1. **64-Ray Fibonacci Starburst**: 64 laser-straight radial rays distributed uniformly across 3D spherical space.
2. **Dense White-Magenta Nucleus**: Blinding central core bloom ($r < 0.14$) with additive glow blending (`#ffffff` / `#ffa5fb`).
3. **Concentric Beaded Spoke Quantization**: Particles along each ray align into 15 concentric radial shells with narrow needle beam thickness, creating visible concentric rings and straight radial spines.
4. **Continuous 3D Tumbling Rotation ("Purple should not be fixed and rotating")**: Dual-axis continuous rotation (yaw $\omega_y = 0.45$, pitch $\omega_x = 0.28$), allowing full 3D parallax and tumbling depth.
5. **Outward Travelling Energy Ripple**: Dynamic outward wave ($\sin(18 \cdot r_{\text{norm}} - 3.5t)$) and specular travelling sparks firing from the nucleus to the outer purple tips so particles continuously pulse with thought energy.
6. **Vibrant 4-Tier Color Gradient**:
   - Core ($r < 0.14$): Pure incandescent white to hot pink (`#ffffff` / `#ffa5fb`)
   - Inner rays ($r \in [0.14, 0.45]$): Intense neon magenta (`#d946ef` / `#c804f5`)
   - Mid rays ($r \in [0.45, 0.75]$): Vivid electric purple (`#a855f7` / `#8d00ca`)
   - Outer ray tips ($r > 0.75$): Deep royal purple (`#6b21a8` / `#4b008a`)
7. **Scale Alignment & Zero-Pop Transition**: Bounding radius $R \approx 1.05$ matches the idle and listening spheres ($R \approx 1.0$), ensuring 700ms morph transitions are completely seamless.
8. **CPU 2D Fallback Parity**: Full parity implemented in `_paint2D` in `voice-orb.js`.

## Files Modified
- `frontend/src/avatar/voice-orb.js`:
  - `PALETTE[2]` updated to vibrant electric purple/magenta `[0.82, 0.15, 0.96]`.
  - Added `rotate3D(p, yaw, pitch)` function for continuous dual-axis 3D tumbling rotation.
  - Vertex shader (`VS`) thinking equations replaced with 64-strand beaded spoke radial math, core nucleus clustering, and dynamic outward energy pulse waves.
  - State palette color calculation (`ctt`), core glow boost (`coreBoost`), and specular spark travel (`knotSpark`) updated.
  - Canvas 2D fallback (`_paint2D`) updated with identical 64-strand beaded starburst equations.
- `frontend/src/avatar/VoiceOrb.tsx`:
  - Updated docstring describing 3D rotating purple beaded starburst.
- `frontend/src/avatar/Avatar.tsx`:
  - Updated docstring describing 3D rotating purple beaded starburst.
- `docs/features/87-shipnotes-webgl-voice-orb-integration.md`:
  - Updated thinking state specifications and reference image mapping.
- `AGENTS.md`:
  - Added Change 71 entry.

## Verification
- Vitest: 154 / 154 passed across 21 test suites.
- TypeScript: `npx tsc --noEmit` passed clean.
- Rust: `cargo test --lib` passed 827 / 827 tests.
- Production binary compiled via `node nexus.mjs build`.
