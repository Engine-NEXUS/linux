# Feature 89 — Orb Screen-Wide Entrance Burst & Real TTS-Audio Beat Sync

## Context

This continues directly off the completed 5-phase "Wakeup Orb Redesign" (docs 73–76, AGENTS.md's "Single-Stage Shell Migration Complete" entry). After that work shipped, live testing on the built app surfaced two things:

1. **A CSP positioning bug** — Tauri's per-page-load nonce injection into `style-src` silently makes `'unsafe-inline'` inert for the whole directive, which made every React `style={{}}` prop a no-op and collapsed the orb to invisible. Fixed by converting every dynamic-position/size assignment across the orb/caption/loading-indicator component tree to direct ref-based DOM mutation (`el.style.x = value`), matching the pattern the pre-existing `ghost-ring`/`ghost-pointer` elements already used successfully.
2. **A particle-text clipping bug** — `sampleTextPoints()` used a fixed 84px font regardless of phrase length, so anything past ~5–6 characters silently clipped on both edges of the 360px sampling canvas (e.g. "Right away." rendered as "ght awa"). Fixed by measuring text at a reference size and scaling down (never up) to fit 92% of the canvas width.

On top of those fixes, the user requested three further visual changes from reference images, with the explicit instruction to **plan, not immediately build**:

1. The orb's entrance (particles flying in to form the shape) should gather particles from **across the whole screen**, not a small area right around the orb.
2. The **thinking** state should become a dense, glowing purple/magenta **sunburst** — many thin rays from a bright core — and visibly **rotate**.
3. The **speaking** state should become a dense, irregular **pebble/potato**-shaped blob, with its shape actively **molded by the beats of NEXUS's own voice** (not a volume proxy), while keeping particle-text convergence legible.

A planning pass (3 Explore agents + 1 Plan agent, cross-checked against the actual file content) found: the existing entrance/scatter mechanism is bounded to the orb's own small canvas (no viewport-spanning math exists — the canvas is forced square and sized to the orb's own small host element, so particles literally cannot render outside it); the "thinking" state at the time was an open 6-strand curl-noise wisp (not the old toroidal knot), colored violet→blue; the "speaking" state was a layered-noise bumpy blob, colored magenta; and the only existing "beat" signal was a bass-onset transient detector fed by the **microphone's** volume reused as a stand-in — there was no real TTS-audio amplitude signal from Rust. Given the choice between a cheap proxy-based fix and building genuine TTS-audio beat sync, the user chose the real signal.

## Delivery approach

Four sub-phases, each independently verifiable:

**Sub-phase A** (screen-wide entrance) → **Sub-phase B** (thinking sunburst) → **Sub-phase C1** (speaking pebble silhouette) → **Sub-phase C2** (real TTS-beat sync, the largest piece — a new Rust capability).

---

## Sub-phase A — Screen-wide particle gather on entrance

**The constraint that drove the design**: `scatterOf()` generates scatter origins in the orb's own local coordinate space, and the vertex shader maps that space straight to clip space with no separate projection matrix. The WebGL canvas itself is forced square and sized to the orb's small host `<div>` (positioned by Rust's `stage:orb_rect` event). A particle can never render outside the canvas's own pixel rect — widening the scatter radius alone cannot reach screen edges, because the canvas itself is too small to show it.

**Planned approach**: temporarily grow the real WebGL canvas to the full stage viewport during the ~900ms assemble window, widen `scatterOf()`'s radius to match, then shrink back once assembled — reusing the existing shader untouched (clip-space math is resolution-independent).

**What actually got built, and why it changed**: during implementation, this ran into a genuine CSS blocker the plan didn't anticipate — `OrbFrame.tsx`'s positioning `<div>` uses `transform: translate(...)` to place the orb, and a CSS `transform` creates a new *containing block* for any `position: fixed` descendant. That means a descendant can't escape to the real viewport the way the plan assumed; it would still be contained by the transformed ancestor. Growing the real canvas to viewport size while nested inside that ancestor wouldn't actually make it span the screen.

The fix: a **separate, lightweight 2D-canvas overlay** (`frontend/src/stage/EntranceBurst.tsx`) — not the WebGL shader/particle system at all — that covers the full stage viewport only for the entrance window. It generates edge/corner-biased starting points across the screen and animates them inward toward the orb's final rect using the *same* cubic in-out easing voice-orb.js's own `assemble()` tween uses, so the two visually converge in lockstep. It fades out over the final 25% of the burst so it blends into the real orb's own particles taking over, rather than a hard cut.

Wired into `OrbFrame.tsx` via a new `burstSeq` counter, bumped on every visible-false→true transition (the same edge `entered` already tracks, but independent of its 50ms one-shot reset, since the burst needs to run for the full ~900ms).

Deliberately scoped out: the Canvas2D (`_paint2D`) no-WebGL fallback keeps the old small-radius scatter — a one-time entrance effect on an already-reduced-fidelity fallback path isn't worth duplicating fullscreen-canvas logic for.

## Sub-phase B — Thinking state: purple sunburst + rotation

Reshaped the existing 6-strand open wisp formation in `voice-orb.js`:
- `STRANDS` raised from `6.0` to `20.0` for a dense ray count.
- Curl/bend weighting (`strandCurl`) reduced so rays shoot outward in straight-ish lines from the core rather than curling — a faint wobble only, not the old loose-smoke bend.
- Rotation rate raised from `t*0.12` to `t*0.4` — the whole formation already rotated rigidly as one object via a single `turn()` call; this just made it unambiguously visible, per the reference image.
- Color shifted from violet→electric-blue to violet→magenta (`PALETTE[2]` updated to match), since the reference is purple/magenta, not blue.
- The existing `knotSpark` traveling-highlight mechanic carried over unchanged (it depends only on `sAlong`/`strandIdx`/`depth`, not strand count or curl).
- Mirrored exactly in the CPU `_paint2D` fallback (required here, unlike sub-phase A, since thinking is a persistent looping state).

**Known collision — superseded by a concurrent session.** A separate, concurrent agent session also redesigned the thinking state the same day (documented in AGENTS.md's "3D Rotating Purple Starburst for Thinking State (2026-10-04)" entry, `docs/changes/71-3d-rotating-purple-starburst-thinking-avatar.md`), landing a different and more elaborate implementation on top of this one: 64 rays (vs. this sub-phase's 20), a dual-axis `rotate3D(p, yaw, pitch)` tumble (vs. this sub-phase's single-axis `turn()`), a 4-tier core/inner/mid/outer color gradient, and `PALETTE[2] = [0.82, 0.15, 0.96]`. That version is what currently ships. This sub-phase's own thinking-state code no longer exists in `voice-orb.js` as written — it's documented here for the historical record of what was actually built and verified in *this* session, and because the surrounding reasoning (why straight rays over curl, why rotation rate needed raising, why the color needed to shift off blue) directly informed the direction the concurrent session's version also landed on.

## Sub-phase C1 — Speaking state: pebble silhouette

Shifted the bumpy-blob noise weighting in `voice-orb.js` so the silhouette reads as a more irregular potato/pebble shape rather than smooth bumps: the dominant noise octave's frequency lowered (`n*1.5` → `n*0.65`) and its amplitude weight raised (`.26` → `.40`), while the secondary smoothing octave's weight dropped (`.16` → `.10`) — fewer, bigger, more irregular lobes instead of many small smooth ones. Mirrored in `_paint2D`.

This sub-phase's code is still live in the current `voice-orb.js` (confirmed — the concurrent session's edits only touched the thinking-state block).

## Sub-phase C2 — Real TTS-audio amplitude streaming + beat-driven mold

The largest piece: building genuine TTS-audio beat sync instead of the cheap mic-volume-proxy fix, per the user's explicit choice.

**Rust (`tts.rs`)**: new `compute_envelope(samples, sample_rate, frame_ms) -> Vec<f32>` — per-20ms-frame RMS, normalized so the loudest frame in the track reads as exactly 1.0 (absolute amplitude varies a lot between edge-tts/Piper and between voices, so a fixed reference level would make quiet voices barely move the orb). Both TTS engines already decode a full PCM buffer before playback (used for word-boundary timing and the rodio sink), so this reuses that same buffer — zero extra synthesis cost.

`CaptionTrack` (the struct already emitted at playback start for the response-caption feature, Phase 3) gained three fields: `envelope: Vec<f32>`, `frame_ms: u64`, `envelope_start_ms: u64`. The last one mirrors the exact cumulative-offset bookkeeping `CaptionWord.start_ms` already uses for a streamed multi-chunk reply — both ride the same absolute utterance timeline, so the frontend never special-cases chunking for either signal. All 6 `CaptionTrack` construction sites (cache pre-gen ×2, single-shot Piper/edge-tts/fallback ×3, streamed-chunk ×1) updated to compute and attach an envelope.

**Frontend (`captionScheduler.ts`)**: extended the existing single-anchor-per-utterance scheduling (already built for word-reveal timing) to also track envelope segments, each with its own absolute start offset. New `getEnvelopeLevel()` interpolates between the two nearest envelope frames at the current elapsed time, or returns `null` if no envelope has arrived yet (e.g., a chunk's engine couldn't produce one) — callers degrade to the old proxy on `null`.

**Frontend (`VoiceOrb.tsx`)**: a new per-frame `requestAnimationFrame` loop, active only while `state === "speaking"`, pumps `getEnvelopeLevel()`'s real value into the orb's existing `setLevel()` call whenever a value is available — taking priority over the `level` prop's mic-proxy effect the instant a real signal exists, and falling back to doing nothing (letting the proxy keep driving) the moment it isn't.

**Beat-driven mold (`voice-orb.js`)**: the onset term's weight raised (`.12*onset` → `.22*onset`), and the lobe-noise amplitude itself now scales with onset (`.40*lobeNoise*(1.0+.4*onset)`) rather than only adding a flat bump — a beat now visibly lurches the whole irregular silhouette. Both the onset-driven terms are damped by remaining particle-text convergence progress (`textDamp = 1.0 - 0.75*textProg`) so a beat never jitters glyphs mid-convergence, fading back to full intensity once text dissolves. This code is still live in the current `voice-orb.js`, unaffected by the concurrent session's thinking-state edits (different shader block).

## Explicit scope boundary

This is a deliberate degradation, stated up front rather than discovered later: the envelope reflects NEXUS's actual voice loudness, but it is **not** frame-accurate to specific phonemes/syllable stress — it's a 20ms-resolution RMS envelope, which is the right level of detail for "the blob visibly breathes/pulses with the voice" without needing phoneme-level audio analysis.

## Critical files

- `frontend/src/avatar/voice-orb.js` — scatter/assemble (A, unchanged — burst lives in a sibling overlay instead), speaking silhouette (C1) and beat-mold (C2) shader blocks + CPU mirrors. (Thinking-state block superseded by a concurrent session — see sub-phase B.)
- `frontend/src/stage/EntranceBurst.tsx` — new, sub-phase A's actual mechanism.
- `frontend/src/stage/OrbFrame.tsx` — `burstSeq` wiring for sub-phase A.
- `src-tauri/src/tts.rs` — `compute_envelope`, `ENVELOPE_FRAME_MS`, `CaptionTrack` extension, all 6 construction sites, streaming cumulative-offset wiring.
- `frontend/src/audio/captionScheduler.ts` — envelope segment tracking, `getEnvelopeLevel()`.
- `frontend/src/avatar/VoiceOrb.tsx` — the real-time envelope pump.
- `frontend/src/avatar/Avatar.tsx` — doc-comment updates only (shape descriptions); the mic-proxy `level` expression is unchanged, now purely a fallback.

## Verification

- `node --check` on `voice-orb.js`: clean.
- `tsc --noEmit`: clean.
- Frontend `vitest`: 154/154 passing across 21 suites.
- `cargo check`: clean (only pre-existing unrelated dead-code warnings).
- Rust `cargo test --lib tts::`: 17/17, including 4 new `compute_envelope` tests (empty input, all-silence, loudest-frame normalization, frame-count-matches-duration).
- Full Rust suite (`cargo test --lib -- --test-threads=1`): 874/874 passing, 0 failed, 1 ignored (pre-existing).
- Release build (`node nexus.mjs build`): clean, `nexus.exe` 52.7 MB.
- Manual live verification of the built app: pending the user's own test pass (entrance burst, speaking-beat reaction with mic covered to confirm it's voice-driven not mic-driven, text legibility mid-speech).
