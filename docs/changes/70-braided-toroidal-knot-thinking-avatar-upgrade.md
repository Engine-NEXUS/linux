# Change 70: Braided Toroidal Knot Ribbon for Thinking State

**Date**: 2026-10-02  
**Component**: `frontend/src/avatar/voice-orb.js`, `frontend/src/avatar/VoiceOrb.tsx`, `frontend/src/avatar/Avatar.tsx`  
**Reference Feature Doc**: `docs/features/87-shipnotes-webgl-voice-orb-integration.md`  

---

## 1. Overview & Motivation

The user provided visual references comparing:
- **Reference (Target)**: A glowing 3D braided toroidal knot ribbon with flowing particle streams in vibrant lavender/violet.
- **Previous implementation**: The generic squashed sphere with orbital belt lines from early `voice-orb` iterations (which deformed awkwardly into a blob/scatter during thinking).

The target visual corresponds to the parametric braided toroidal knot topology from Ship Notes `components/signal-orb` / `speaking-orb`.

---

## 2. Mathematical Implementation & Shader Architecture

### 1. Continuous Ribbon Topology in Vertex Shader (`VS`)
To convert the 12,000 points into a continuous ribbon without destroying sphere topology during other states, `sphere(count)` sets the 4th float attribute `seed.w` to normalized index $u = i / \text{count} \in [0, 1)$.

In the vertex shader:
$$\theta = u \cdot 2\pi \cdot 3.0 + 0.57 t \quad \text{(3 toroidal revolutions rotating at } 0.57 \text{ rad/s)}$$
$$\phi = u \cdot 2\pi \cdot 8.0 - 0.48 t \quad \text{(8 poloidal ribbon twists counter-rotating at } -0.48 \text{ rad/s)}$$
$$r_t = 0.7135 + 0.2135 \cos(\phi) + 0.1250 \sin(a)$$
$$b_x = r_t \cos(\theta), \quad b_z = r_t \sin(\theta), \quad b_y = 0.3385 \sin(\phi) + 0.0885 y$$

### 2. Angular Tilt Matrix
The knot is tilted by $\approx 34^\circ$ via rotation around the X-axis:
$$v_x = b_x$$
$$v_y = 0.77 b_y - 0.52 b_z$$
$$v_z = 0.52 b_y + 0.77 b_z$$

### 3. Scale & Center Alignment
- Unit radius of the sphere: $R \approx 1.0$.
- Bounding envelope of the braided toroidal knot: $R_{\text{knot}} \in [0.88, 1.05]$.
- Bounding radii match to within 5%, ensuring seamless alignment with zero jumping, clipping, or center-of-mass shift.

### 4. Color Palette & Sparkle Shading
- Shifted `thinking` color from amber (`#ff912e`) to radiant lavender/violet (`rgb(178, 151, 255)` / `#b297ff`).
- `tint` dynamically mixes `vec3(0.55, 0.42, 0.92)` (royal lavender) with `vec3(0.85, 0.75, 1.0)` (luminous pearlescent violet).
- Added `knotSpark`: travelling white core sparks on front loops (`depth > 0.65`) providing 3D depth and specular flow.
- Parity implemented in `_paint2D` for CPU canvas fallback.

---

## 3. Verification

- **Frontend Vitest**: **144 / 144 tests passed (100%)** across 19 test suites.
- **TypeScript**: Clean (`npx tsc --noEmit` exited with code 0).
- **Release Compilation**: Production release binary compiled via `node nexus.mjs build` to `src-tauri/target/release/nexus.exe` (51.2 MB).
