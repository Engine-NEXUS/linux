# Feature 88 — One Continuous Particle Simulation: Living Particle Entity + Particle-Generated Text (2026-10-02)

**Spec:** user-provided particle-entity brief (spherical cloud → deformation → strands → particle text → dissolve → reform → speaking → sphere).
**Constraint honored:** the ENTIRE visual is the particle system — no pre-rendered sphere, no overlay text, no crossfades. The same 5,000 particles perform every transformation.

## What was built (`frontend/src/avatar/voice-orb.js`)

**Architecture:** 5th weight channel `'text'` + per-particle `textPos` attribute buffer (glyph targets + ambient-halo flags). The shader mixes ONE cloud position across the four base states (renormalized in JS so the additive mix keeps its full shape), then morphs into glyphs via a staggered per-particle progress — one continuous simulation throughout.

1. **Idle sphere** (kept): organic drift + breathing, 5,000 individual particles.
2. **Listening** (upgraded): 50% pulse radially with voice reactivity; the frozen half now **continuously fluctuates** (subtle sinusoid + bass term) — never fully static.
3. **Thinking — major transformation** (rebuilt): replaced the static starburst scaffold with the **braided toroidal knot** (θ=3·2πu+0.57t, φ=8·2πu−0.48t, rt=0.7135+0.2135cosφ+0.125sin(t·1.1+u·2π), 34° tilt) — flowing curved strands that twist and intertwine. `thinkIn` drives the entry: sphere **contracts inward** (×0.58) then **stretches out** into the knot over ~700ms — particles travel, never a fade. `knotSpark` white sparks travel the front loops. Bounding radius ≈1.05 matches the sphere (no jump/clip).
4. **Particle-generated text** (new): `setText(text, holdMs)` → offscreen-canvas glyph sampling (`sampleTextPoints`, 2-4 particles per glyph point via prime-7919 scatter, z-jitter for volume; 12-30% stay as a dim ambient halo) → **converge** (~700ms eased, per-particle stagger + decaying swirl = detach/travel/converge) → **hold** (~1.6s) → **dissolve** (~600ms: state reverts so the sphere reforms while particles stream back with a swirl burst) → cleanup. Warm-white glyphs (`vec3(1.0,.97,.88)`), crisp point rendering, external state flips cancel the phase (override wins).
5. **Speaking** (kept + text-allowed): dotted lattice waves, bass deformation, transient sparks.
6. **Final reform** (natural): weights lerp back to sphere with smooth rotation + breathing.
7. **2D fallback parity**: braided knot + text morph + ambient halo in `_paint2D`.
8. **Live wiring (one choke point)**: `speak()`/`speakCached()` (`ttsPlayer.ts`) emit `orb:show_text` for short spoken lines (≤22 chars, no newlines) → `VoiceOrb.tsx` listener → `setText` in every window's orb. Acks like "Ok sir." form in particles; long replies never trigger it. Optional `text` prop added for static callers.

## Verify
- vitest **148/148** (20 files; +1 node-safe guard test — jsdom not installed, DOM behavior verified via build + live); tsc 0.
- Release binary **51.3 MB**; shader shipped to dist (textProg/knot/sampleTextPoints verified present).
- Zero Rust changes.

## Live acceptance (user-run, `nexus start`)
1. Wake → idle sphere alive (orbit + drift + breathe).
2. Speak → listening sphere fluctuates, reacts to voice, never static.
3. Ask something → thinking: sphere contracts, stretches, breaks symmetry into twisted ribbon strands, rearranges continuously.
4. Short acks ("Ok sir.") → particles detach, converge into readable glyphs, hold, dissolve back.
5. Long reply → speaking waves pulse; sphere reforms smoothly after.
