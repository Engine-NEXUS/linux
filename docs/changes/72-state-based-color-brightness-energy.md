# Feature 89 — State-Based Color, Brightness, Energy & Lighting (2026-10-02)

**Spec:** user-provided refinement brief — state palettes + energy hierarchy on the existing particle system. **Formation logic and animation untouched** (same particles, same shapes, same transitions).

## What changed (`frontend/src/avatar/voice-orb.js` only)

**Palettes (JS `PALETTE` + GL constants):**
- idle: warm grey `vec3(.80,.78,.76)` (kept subtle)
- **listening: calm gold** `vec3(1.0,.85,.58)` — champagne warmth, low controlled brightness
- **thinking: blue-violet** with a **travelling hue** — `ctt = mix(violet(.62,.55,1.0), electric-blue(.45,.72,1.0), .5+.5*sin(kph−t·1.2))`: the color literally travels through the strand twist phase (processing/computation feel)
- **speaking: vibrant magenta** body `vec3(1.0,.45,.85)` with white front highlights (`speakFace`/`speakWire` mix)

**Brightness/energy hierarchy (GL `strength`):**
- listening `×0.70` + voice terms split out and made **very subtle** (`mid·flow·.40`, pulse `.08→.05`, `listenReact .12→.06`) — calm, attentive
- thinking `×1.02` (medium-high) + knotSpark edges + travelling hue
- speaking `×1.18` (highest) + **rhythmic brightness synced with the radial breathing** (`strength += ww·.10·sin(t·.9+seed.w·2π)` — expands brighter, contracts dimmer)

**Text inherits the STATE palette (no separate text color):** `_paint` computes the dominant base state's tint (`stateTint`) → new `textTint` vec3 uniform → glyph particles use the state color active at formation (listening text = gold/white, thinking = purple/blue, speaking = magenta/white). The 2D fallback inherits via `tintRgb` (glow gradient + fillStyle now state-colored).

**Per-particle color lag (transitions coexist):** mid-transition, particles lean toward the target color with per-particle hash lag (`mixAmt = lag·.55·(1−min(1,mw·4))`) — some still show the old hue while others shifted (gold → gold/purple → purple/blue), settling to the uniform state color. Never a sudden swap, never a scene-wide filter.

## Verify
- vitest **154/154** (21 files); tsc 0.
- Build incident: first `node nexus.mjs build` failed with **LNK2038/LNK1319** (`esaxx_rs` rlib mismatch — stale incremental artifact from mixed builds, not a code error). Fix: `cargo clean -p esaxx-rs` → rebuild clean → **52.6 MB** binary. State-color code verified shipped in dist (9 matches: textTint/stateTint/textProg/orb:show_text).
- Zero Rust source changes.

## Live acceptance (user-run, `nexus start`)
1. Listening: calm gold-white sphere, low glow, gentle breathing; voice = very subtle brightness only.
2. Thinking: gradual gold→purple/blue shift with the hue travelling along the twisted strands; brighter edges.
3. Speaking: magenta/purple/white, brightest state, rhythmic brightness with expansion.
4. Acks during each state: text forms in THAT state's palette, particles visible inside letters.
5. Transitions: old/new colors coexist per particle; no rainbow, no neon.
