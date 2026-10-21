# Orb Shape, Color & Glitch Transition Redesign — Phase 2 (Claude session, 2026-10-04)

**Plan:** `C:\Users\Chitkul Lakshya\.claude\plans\c-users-chitkul-lakshya-downloads-phone-idempotent-snail.md` (Wakeup Orb Redesign; 5 phases). Phase 1 (window consolidation, [doc 73](73-window-consolidation-orb-into-stage.md)) landed first. This is **Phase 2 — shape, texture, and glitch-transition redesign**, scoped entirely to `frontend/src/avatar/voice-orb.js` (+ two stale doc comments in `Avatar.tsx`/`VoiceOrb.tsx`). No Rust changes.

## 0. Reference grounding

Re-extracted 15 frames (1.5fps) from the user's original reference video (`v2 (online-video-cutter.com).mp4`, 10s) via ffmpeg and read them directly before touching the shader, rather than relying on the Phase-2 plan's secondhand prose description. Confirmed:
- **Listening**: a calm, grainy, sandy-textured amber/brown sphere — dark, muted, not the pale gold the old palette used.
- **Thinking**: an open, flowing, multi-armed violet/indigo formation with bright near-white highlights near the reaching tips — distinct separated tendrils with real negative space between them, never a closed ring.
- **Speaking**: a dense, grainy magenta/pink sphere with a bright rim-light and a visibly irregular, lobed, non-circular silhouette (several frames show a rounded-triangle/pebble outline) — plus particles visibly breaking off the surface to form the response caption text.

## 1. What changed in `voice-orb.js`

### Listening — grain + amber/brown
`PALETTE[1]` (and the matching VS `cl`) moved from pale gold `[1.0,.85,.58]` to amber/brown `[.82,.55,.26]`. The existing per-particle pulse/drift radial terms were left as the "alive" motion layer (unchanged); the color shift alone was enough to read as the intended warm amber rather than gold.

### Thinking — braided-knot → open multi-strand wisp formation
The old shape was a single closed parametric braided torus (`3 strand-turns × 8 twists`) — structurally incapable of reading as loose open wisps no matter how it's colored. Replaced with a genuine multi-strand system:
- `u` (`seed.w`, already a continuous 0..1 index ramp from `sphere()`) is partitioned into `STRANDS=6` contiguous blocks — `strandIdx` (which tendril) and `sAlong` (0=core, 1=tip).
- Strand **directions** are spread evenly via a golden-angle Fibonacci-sphere formula (same math as `sphere()`'s own point distribution) rather than independently-hashed random directions. This was a mid-implementation fix: random per-strand hashes occasionally clump 2-3 directions close together (birthday-paradox effect), which visually reads as one blob instead of distinct arms — confirmed by screenshot comparison before/after.
- Each strand bends via two sine harmonics (per-strand randomized frequency/phase/curl) around its own local frame (`perp1`/`perp2`), so it genuinely curves in 3D instead of a straight spike.
- `reach = pow(sAlong, 1.7)` (ease-in) + a tapering ribbon cross-section (`tipTaper`) gives a thick core and a tendril that thins to a point at the tip.
- A slow whole-formation `turn(..., t*0.12)` spin on top of each strand's own wiggle gives the "twist and intertwine" liveliness from the reference.
- Color (`ctt`) now travels along `strandIdx`/`sAlong` instead of the old torus phase, and mixes toward white near each tip (`smoothstep(0.55,1.0,sAlong)`) for the bright highlight look. `knotSpark` (travelling specular highlight) redriven the same way.
- Ported the identical math to the `_paint2D` CPU-fallback path (small `normalize3`/`cross3` helpers added for the 3D vector math JS lacks natively).

### Speaking — lat/lon wireframe lattice → dense bumpy blob
The old shape sampled a fixed meridian/parallel lattice (`ttsLatticeOf`, a 100×50 grid) — a sparse wireframe mesh, not a solid "speaking" presence. Replaced with the base sphere distribution displaced by **two octaves of low-frequency 3D noise** (`lobeNoise`/`lobeNoise2`, amplitudes 0.26/0.16 after tuning up from an initial 0.14/0.09 that barely read against the soft canvas glow backdrop) layered under the pre-existing high-frequency grain — producing an irregular, lobed, non-circular silhouette instead of a sparse mesh. `ttsLatticeOf()`, the `lattice`/`grid` vertex attributes and their buffers, and `_ttsLattice`/`_ttsGrid` state are fully deleted (confirmed zero remaining references) rather than left as dead code.

### Glitch transition (new)
A short (~220ms) burst triggered whenever the **dominant** base-state weight flips — tracked in `_tick()` via a 4-way `_weights` max-index comparison against `_lastDom`, which naturally fires mid-blend (when the new state's weight crosses the old one) rather than at the instant the `state` attribute changes. Envelope: `glitchAmt = sin(gt·π)` for `gt∈[0,1]` (`gt=(now-glitchStart)/220`), else 0 — a smooth 0→1→0 hump, verified correct in isolation via a synthetic-timestamp sweep (not relying on a real-time rAF loop, which a headless preview harness couldn't sustain at 60fps for timing verification).
- **Vertex shader**: a large, fast-changing per-particle positional jitter (`noise(n*35+...,t*40,...)` etc., decorrelated from the calm ambient drift/grain noise via much higher time coefficients) added to `flight` right before projection — reads as a sudden tear, not part of normal idle motion.
- **Fragment shader**: a per-particle RGB-channel decorrelation flicker (channels pulse independently based on a `gl_PointCoord` hash) — a point-sprite approximation of chromatic-aberration/VHS-tear; true screen-space channel splitting needs a post-process pass, out of reach of this per-point architecture.
- 2D-canvas fallback gets the positional jitter only (quantized to ~90Hz via `hash3(i, floor(t*90), channel)` for a flickery look); the fragment-level color trick is WebGL-only (a deliberate, acceptable simplification for the lower-tier fallback).

## 2. Three real bugs found and fixed along the way

1. **Template-literal-breaking backticks**: four new GLSL/JS comments used markdown-style `` `identifier` `` backtick-quoting. Since the entire VS/FS shader source lives inside a JS template literal (`` const VS = `...`; ``), those backticks prematurely terminated the string, corrupting everything between them into invalid JS (`node --check` caught this immediately: `SyntaxError: Unexpected identifier 'u'`). Fixed by removing all markdown-backtick formatting from comments inside (and anywhere that could interact with) the shader strings.
2. **Pre-existing `mix is not defined` crash in the CPU/2D-canvas fallback path**: `_paint2D` (used whenever WebGL is unavailable) called a bare `mix(...)` in three places — this function has never existed in this file (only `mixNum` does); it would have thrown a `ReferenceError` on the very first paint in any non-WebGL context. This predates Phase 2 entirely (confirmed present in the original file read before any edits) and was never caught because the only existing test for this file is a "module imports without throwing" smoke test that never instantiates the element. Found by actually rendering the orb in a browser during Phase 2 verification. Fixed by renaming all three plain-JS call sites to `mixNum` (the GLSL-string `mix(` calls, which use the real GLSL builtin, are untouched).
3. **`textTint`/`sparkTint` uniform/varying never wired**: a concurrent uncommitted change in this same file added `gl.uniform3fv(u.textTint, stateTint)` without registering `'textTint'` in the uniform-lookup list (so `u.textTint` was always `undefined` → silently ignored by WebGL → text glyphs never actually got their intended state-colored tint), and the `sparkTint` varying was declared in both shaders and read in the fragment shader but never assigned a value in the vertex shader (undefined/implementation-dependent — likely always black, making the "specular white spark" highlight contribute no color, only an opacity boost). Both fixed in passing: `'textTint'` and `'glitch'` added to the uniform registration list, and `sparkTint = vec3(1.0,1.0,1.0)` assigned in the vertex shader.

## 3. One precision bug introduced and fixed during this phase

The new `glitch` uniform was declared as plain `uniform float glitch;` in both the vertex shader (which sets `precision highp float;`) and the fragment shader (which sets `precision mediump float;`). WebGL requires a uniform shared between both shader stages to have **matching** precision — linking failed with `Precisions of uniform 'glitch' differ between VERTEX and FRAGMENT shaders`, which was silently swallowed by `_setupRenderer`'s existing try/catch (falls back to 2D canvas, matching the Design's documented graceful-degradation behavior — but it meant the orb was silently running in the low-fidelity fallback with no visible error). Found via a patched copy of the module loaded in an isolated iframe to surface the suppressed compile/link error. Fixed by explicitly declaring `uniform mediump float glitch;` in both shaders.

## 4. Verification

- Visual: built a standalone HTML harness (scratchpad, not committed) loading the real `voice-orb.js` as a module, rendering all 4 states side-by-side plus isolated close-ups, served over a local static HTTP server and inspected via the built-in Browser pane. Confirmed: no console errors across all states and transitions; listening renders amber/brown; thinking renders as a visibly open, multi-armed, non-circular formation (clearly different from the old closed ring) after the strand-direction fix; speaking renders as a round-but-irregular grainy magenta sphere (further lobe-amplitude tuning beyond what a static screenshot in a non-GPU-representative harness can confidently validate is left for a live check in the real app).
- Math: the glitch envelope formula was independently verified via a synthetic-timestamp sweep (bypassing the harness's throttled rAF loop, which only sustained roughly 1 tick per ~750ms and made real-time polling unreliable) — confirms a clean 0→1→0 hump over exactly 220ms.
- `node --check` on the raw file (syntax), `npx tsc --noEmit` (clean), `npx vitest run` (154/154 across 21 files, unchanged), `npm run build` (clean, same pre-existing chunk-size warning as before Phase 2).
- Rust untouched — no `cargo` re-verification needed for this phase.

## 5. Not done / next

- Pixel-perfect match to the reference video's exact lobe prominence on `speaking` and strand density/curl on `thinking` is inherently iterative; both are tuned to a reasonable first pass and are worth a quick live look once seen in the real app with GPU rendering and real audio levels driving them (silent/no-audio preview in a headless harness is not representative).
- Phases 3 (response caption), 4 (live-speech caption), 5 (cleanup) remain pending per the plan.
