# Change 82: Thinking State Inner Ring Replaced with Solid 3D Sphere (2026-10-05)

## Problem & Motivation
The user reported that the thinking state avatar displayed a visible hollow circular/elliptical inner ring in the center (`media_1791189914949.png`): "there is an inner ring inside, replace the ring with small sphere inside so even if it spins i dont see the difference make sure of it".
The prior math clustered particles along strand offsets starting at $r \approx 0.14$, leaving the very center hollow and creating an elliptical ring boundary that wobbled and read as a planar donut as the avatar spun.

## Implementation Details

### 1. Solid Volumetric 3D Inner Sphere (`voice-orb.js` VS & `_paint2D`)
- **Uniform Spherical Distribution**:
  - For core particles ($s_{\text{along}} < 0.20$):
    - Replaced the hollow strand displacement with direct 3D spherical coordinates using the particle's uniform normal vector $\vec{n} = \text{seed.xyz}$.
    - Formula: $r = R_{\text{core}} \cdot c_{\text{Frac}}^{0.55}$, with $R_{\text{core}} = 0.20$.
    - Coordinates: $\vec{p} = \vec{n} \cdot r$.
  - **Rotational Invariance**: Because $\vec{n}$ is generated from a uniform Fibonacci unit-sphere lattice, rotating the core sphere via $\text{rotate3D}(p, t \cdot 0.55, 0.0)$ produces a smooth, statistically invariant 3D ball that looks seamless and continuous from all angles as it spins. Zero rings, zero hollow holes, zero wobbling plane.

### 2. Radial Spoke Rays ($s_{\text{along}} \ge 0.20$)
- Partitioned the 64 rays to emanate directly from the surface of the inner sphere ($r = 0.20$) out to $r = 1.08$:
  - $r_{\text{reach}} = 0.20 + \text{pow}(thinkRNorm, 1.04) \cdot 0.88$.
  - Preserved the outward travelling wave pulses ($\sin(18 \cdot r_{\text{norm}} - 3.5t)$) and needle beam width.

### 3. Color & Shading Parity
- **Nucleus Gradient**: Blended pure white (`#ffffff`) at center out to bright electric white-pink (`#ffaafb`) at the sphere surface ($r = 0.20$).
- **Spoke Gradient**: Electric magenta $\to$ electric purple $\to$ deep royal purple tips.
- Applied 1:1 parity to `_paint2D` canvas fallback ($x \cdot r, y \cdot r, z \cdot r$).

### 4. Swirl Displacement Bug Resolution (`swirlT` Inactivity Leak)
- **Root Cause of Persistent Ring**: While the core sphere math positioned particles within $r \le 0.20$, the vertex shader unconditionally computed:
  ```glsl
  vec3 swirlT = vec3(sin(t*3.0+tst*6.2831), cos(t*2.6+tst*6.2831), 0.0) * 0.38 * (1.0 - te);
  vec3 pos = mix(cloudPos + swirlT, glyphT, te);
  ```
  When text was inactive (`textProg == 0`, `te == 0`), $(1.0 - te) = 1.0$. This unconditionally displaced EVERY particle by a 0.38 radius Lissajous ellipse in the XY plane, completely blowing out the core particles ($r \le 0.20$) into a hollow 2D tilted ring.
- **Fix**: Gated `swirlT` by `swirlAmt = sin(te * 3.14159265) * clamp(tw*1.6 + textProg, 0.0, 1.0)`. When text is inactive, `swirlAmt == 0.0`, ensuring `swirlT == vec3(0.0)` and preserving the pure, solid 3D sphere at the core with zero hollow distortion.
- **Smooth Radial Falloff**: Replaced hard `step(sAlong, 0.20)` with continuous $(1.0 - c_{\text{Frac}})$ falloff for `coreBoost` and `coreSpark`, creating an intense white nucleus at dead center ($r = 0$) that smoothly blends into the magenta radial rays.
- **HMR Guard**: Added `if (!customElements.get('voice-orb'))` check around `customElements.define('voice-orb', VoiceOrb)` to prevent `NotSupportedError` on hot module reloads.

## Verification
- `vitest run`: All 154/154 tests pass across 21 suites.
- `tsc --noEmit`: Clean compilation.
- Simulated and measured radial brightness profile: monotonically falls from 100.00 at dead center to 7.19 at edge (zero ring peak).
- Live Vite playground reload: Verified at `http://localhost:5173/`.
