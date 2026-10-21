# Change 83: Dynamic Pulsing Rays & Inter-Line Thread Connections (2026-10-05)

## Problem & Motivation
Following the fix for the solid 3D nucleus core in Change 82, the user requested:
> *"now the linees should move decrase and increase and create lika thread one line connects to another line accordingly plan first"*

The static radial spoke lines lacked organic motion, and adjacent lines were completely isolated without visual coherence or interconnected filaments.

## Implementation Details

### 1. Dynamic Ray Undulation ("lines move decrease and increase")
- **Cadence & Wave**: Modulated the reach of each ray using a continuous spherical harmonic traveling wave at a medium-energetic cadence (~0.5 Hz):
  $$\text{rayBreath}(k, t) = 0.82 + 0.28 \sin(k \cdot 0.45 + 2.5 t) \cos(y_k \cdot 3.2 - 1.6 t)$$
- **Effect**: Spoke rays dynamically extend up to $1.10\times$ and retract down to $0.54\times$ in rolling, organic breathing waves across the sphere.

### 2. Inter-Line Thread Connections ("create like a thread one line connects to another line")
- **Partitioning**: 72% of ray particles form the beaded radial spoke beams, and 28% form elastic luminous connecting threads linking ray tips to their spatial Fibonacci neighbor ($k \leftrightarrow k+8$).
- **Fibonacci Neighbor Math**: Spatial neighbor direction $\vec{d}_{\text{neighbor}}$ is computed analytically in 4 GLSL instructions without texture or buffer lookups:
  ```glsl
  float nIdx = mod(strandIdx + 8.0, 64.0);
  float nSy = 1.0 - 2.0 * (nIdx + 0.5) / 64.0;
  float nSr = sqrt(max(0.0, 1.0 - nSy * nSy));
  float nSang = nIdx * 2.399963229728653;
  vec3 neighborDir = vec3(nSr * cos(nSang), nSy, nSr * sin(nSang));
  ```
- **Elastic Catenary Thread**:
  $$\vec{p}_{\text{thread}} = \text{mix}(\vec{p}_{\text{tip}, k}, \vec{p}_{\text{tip}, \text{neighbor}}, t_{\text{thread}}) - \vec{n}_{\text{sag}} \cdot \sin(t_{\text{thread}} \pi) \cdot 0.07$$
  As lines increase and decrease in reach, the connecting threads stretch, pull, and flex between the lines accordingly.

### 3. Traveling Thread Sparks & Luminescence
- Added `threadSpark`: energetic luminous pulses traveling along the connecting threads from one line into the next line:
  ```glsl
  float threadSpark = pow(max(0.0, sin(tThread * 6.2831853 - t * 4.5 + strandIdx * 0.7)), 8.0) * isThread * step(0.3, depth);
  ```
- Strengthened thread luminosity (`strength += isThread * 0.35 + threadSpark * 1.4`).

### 4. CPU Fallback Parity
- Implemented 1:1 mathematical parity in `voice-orb.js` `_paint2D` canvas fallback (`rayBreath`, neighbor calculation, and catenary thread interpolation).

## Verification
- `vitest run`: All 154/154 tests pass across 21 suites.
- `tsc --noEmit`: Clean compilation.
- Live Vite playground reload: Verified on `http://localhost:5173/`.
