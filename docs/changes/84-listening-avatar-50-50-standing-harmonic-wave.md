# Change 84: Listening Avatar 50/50 Acoustic Sand Cymatics & Soundbar Beat Motion

## Motivation & Context
User directive:
> "the particlaes is moving like a thread i dont want that it should be random moving not like a thread and it should only move where the usear is speaking
> 
> like a soundbar beat system 
> where the sand moves according to the sound plan that"

Previously:
- Continuous spherical harmonic waves caused neighboring particles to move together in coherent wave fronts, creating visible "thread-like" rings and ribbons wrapping around the sphere.
- Particles moved continuously even when the user was completely silent.
- The user requested:
  1. No thread-like lines: particles must move decorrelated and randomly.
  2. Only move when the user is speaking: at silence, particles rest quietly on the sphere.
  3. Like an acoustic soundbar / Chladni plate: sand grains jumping and scattering in/out to sound beats!

## Design & Implementation

### 1. 50% Fixed Anchor Cage
- Fibonacci parity partition $\left(\left\lfloor \text{seed.w} \cdot 5200.0 + 0.5 \right\rfloor \bmod 2\right) < 0.5$:
  - Strictly $r = 1.0$ rotating in a steady frame (`turn(n, t * 0.14)`).
  - High crispness (`crisp = 0.65`) and golden-amber beads (`#f2b859` / `vec3(0.95, 0.72, 0.35)`).
  - Represents the solid speaker drumhead / base plate.

### 2. 50% Acoustic Sand Grains (Decorrelated Random Cymatics)
- Hashes $h_1, h_2, h_3$ generated from decorrelated pseudo-random noise per particle, ensuring 0% spatial coherence and 0% thread lines.
- Sound energy gating:
  $$\text{soundEnergy} = \max(\text{low}, \max(\text{mid}, \text{high})) \cdot 0.75 + \text{onset} \cdot 0.85$$
  $$\text{voiceLevel} = \text{clamp}\left(\frac{\text{soundEnergy} - 0.02}{0.98}, 0.0, 1.0\right)$$
- When silent ($\text{voiceLevel} == 0$):
  $$\Delta r = 0.0 \implies r_{\text{sand}} = 1.0$$
  All sand grains rest still on the sphere surface. Zero movement, zero thread loops.
- When user speaks ($\text{voiceLevel} > 0$):
  $$\text{bounce} = \sin(t \cdot (18.0 + 34.0 \cdot h_1) + h_2 \cdot 6.2831853)$$
  $$\text{beatKick} = (h_1 - 0.5) \cdot 2.0 \cdot \text{onset} \cdot 0.45$$
  $$\text{jitter} = (h_3 - 0.5) \cdot 0.16 \cdot \text{mid}$$
  $$r_{\text{sand}} = 1.0 + \text{voiceLevel} \cdot (0.35 \cdot \text{bounce} + \text{beatKick} + \text{jitter})$$
  - Sand grains bounce radially in and out ($r \in [0.55, 1.45]$) in direct response to voice volume and acoustic beat hits.

### 3. Non-Negotiable Invariants & Parity
- **Thinking state strictly untouched**: Solid nucleus sphere, 64 rays, tip connecting threads, single-axis globe spin remain 100% frozen.
- **Speaking state strictly untouched**: Pebble blob and TTS beat sync remain 100% frozen.
- **1:1 Fallback Parity**: Identical sound energy gating and decorrelated sand cymatics math implemented in `_paint2D` CPU fallback.

## Verification
- `npx tsc --noEmit`: Clean, 0 errors.
- `npx vitest run`: 154/154 tests passed across 21 test suites.
- Visual inspection on Vite dev server at `http://localhost:5173/`.
