# Change 77 — Orb Entrance Burst, Pebble Speaking Silhouette & Real TTS-Audio Beat Sync

## Problem & Motivation

Following the completed 5-phase Wakeup Orb Redesign (docs 73–76), live testing surfaced two bugs (a CSP-induced invisible orb, a particle-text clipping bug) which were fixed separately. On top of those, the user requested — via reference images, with an explicit "plan this" rather than build directly — three further visual changes: a screen-wide particle gather on wake, a purple/magenta rotating sunburst for "thinking," and a pebble-shaped "speaking" blob whose deformation is driven by NEXUS's own voice beats rather than a stand-in signal. A full plan-mode pass (3 Explore agents mapping the exact current shader/entrance/audio-pipeline code, 1 Plan agent designing the approach, then a clarifying question to the user on scope) preceded any code changes. The user explicitly chose to build genuine TTS-audio amplitude streaming rather than accept a cheaper mic-proxy approximation.

## Implementation

### Sub-phase A — Screen-wide entrance (plan deviation, documented)
- The plan's primary recommendation (temporarily grow the real WebGL canvas to viewport size during assemble) turned out to be blocked by a CSS fact only surfaced during implementation: `OrbFrame.tsx`'s positioning `<div>` uses `transform`, which creates a containing block that traps any `position: fixed` descendant — it can't actually reach the real viewport.
- Built the plan's already-named fallback instead: `frontend/src/stage/EntranceBurst.tsx`, a separate lightweight 2D-canvas overlay covering the full stage viewport only during the ~900ms assemble window. Particles start at edge/corner-biased random points and converge toward the orb's final rect using the identical cubic in-out easing `voice-orb.js`'s own `assemble()` uses, fading out over the final 25% so it blends into the real orb particles taking over.
- `OrbFrame.tsx` gained a `burstSeq` counter, bumped on every visible-false→true transition, independent of the existing `entered` prop's 50ms one-shot reset.

### Sub-phase B — Thinking sunburst (superseded)
- Reshaped the existing 6-strand wisp to 20 straighter, less-curled rays, raised whole-formation rotation rate `t*0.12`→`t*0.4`, shifted color from violet→blue to violet→magenta, mirrored in CPU `_paint2D`.
- **Note**: a concurrent agent session landed a different, more elaborate thinking-state redesign the same day (64 rays, dual-axis `rotate3D` tumble, 4-tier color gradient — see AGENTS.md's "3D Rotating Purple Starburst" entry and `docs/changes/71-3d-rotating-purple-starburst-thinking-avatar.md`), which now ships instead of this sub-phase's version. Documented here for the record; see `docs/features/89-orb-screen-wide-entrance-and-tts-beat-sync.md` for the full account.

### Sub-phase C1 — Speaking pebble silhouette
- Lowered the dominant lobe-noise octave's frequency (`n*1.5`→`n*0.65`) and raised its weight (`.26`→`.40`), lowered the secondary octave's weight (`.16`→`.10`) — fewer, bigger, more irregular lobes. Mirrored in `_paint2D`. Still live in the shipped `voice-orb.js`.

### Sub-phase C2 — Real TTS-audio beat sync (new Rust capability)
- `tts.rs`: new `compute_envelope(samples, sample_rate, frame_ms) -> Vec<f32>` — peak-normalized per-20ms-frame RMS, computed from the PCM buffer both TTS engines already decode before playback.
- `CaptionTrack` extended with `envelope`, `frame_ms`, `envelope_start_ms` (the last mirroring the existing cumulative-offset convention `CaptionWord.start_ms` already uses for streamed multi-chunk replies). All 6 construction sites updated.
- `captionScheduler.ts`: tracks envelope segments per chunk; new `getEnvelopeLevel()` interpolates the current real voice amplitude, or returns `null` if unavailable.
- `VoiceOrb.tsx`: new rAF loop active only while speaking, pumps the real envelope into the orb's existing `setLevel()`, falling back to the old mic-proxy `level` prop the instant no envelope value is available.
- `voice-orb.js`: onset term's weight raised (`.12`→`.22`), lobe amplitude itself now scales with onset (`(1.0+.4*onset)`) so a beat visibly lurches the silhouette rather than just adding a flat bump. Both onset terms damped by `textDamp = 1.0 - 0.75*textProg` so beat deformation fades out during particle-text convergence and returns once text dissolves. Mirrored in `_paint2D`.

## Verify

- `node --check` voice-orb.js: clean.
- `tsc --noEmit`: clean.
- Frontend `vitest`: 154/154 across 21 suites.
- `cargo check`: clean (pre-existing unrelated warnings only).
- `cargo test --lib tts::`: 17/17 (4 new `compute_envelope` tests).
- Full Rust suite serial: 874/874 passed, 0 failed, 1 ignored.
- Release build (`node nexus.mjs build`): clean, `nexus.exe` 52.7 MB.
- Manual live verification: pending user test pass.

## Docs

`docs/features/89-orb-screen-wide-entrance-and-tts-beat-sync.md` (full architecture spec, context, and the collision/deviation record).
