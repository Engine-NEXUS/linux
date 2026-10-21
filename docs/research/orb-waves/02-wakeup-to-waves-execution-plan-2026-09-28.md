# Wakeup → Waves — Execution + Implementation Plan (2026-09-28)

Companion to `01-speech-synced-waves-research-2026-09-28.md`.
Approaches are ordered so every phase is independently shippable.

## P0 — SHIPPED this turn (verified)

**Rust** (`src-tauri/src/wakeword_oww.rs`)
- `level_from_rms` map + `test_level_from_rms_contract`.
- `LEVEL_TX: sync_channel(4)`; callback hook (every 2nd chunk,
  try_send, never blocks); settle-zero on capture stop; dedicated
  `audio-level-fwd` thread emitting `audio:level {level}` with
  drain-to-latest + < 0.02 duplicate suppression.

**Deterministic parser** (`intent_parser.rs`)
- "create a new tab" / "create new tab" → `browser_new_tab`
  (live miss from the user's log) + `test_parse_live_create_new_tab_verbs`.

**Frontend**
- `micLevel` store field (clamped, reset-safe) + `audio:level`
  listener (`main.tsx`).
- Avatar: `waveSource` / `waveScaleForBar` / `ttsWaveLevel`
  (pure, 5 new tests), rAF drive loop (mic → TTS → rest, eased),
  `.ghost-bar--reactive` (kills the free CSS loop only in waves
  phase), optional `ghost-waves.json` Lottie layer (play/pause +
  opacity gated the same way), `wakeup-v2.json` auto-pickup with
  `wakeup.json` fallback via `LOTTIE_MAPS`.
- 8 new tests total (5 avatar + 1 Rust level + 1 Rust phrase +
  1 existing-suite growth).

**Verify.** Rust 645/645 serial; clippy zero in touched ranges;
tsc clean; vitest 40/40.

## P1 — Asset swap (user action, no code)

1. Export both LottieFiles references as `.json`.
2. Drop into `frontend/public/` as `wakeup-v2.json` +
   `ghost-waves.json`. Rebuild frontend (`nexus build`).
3. If v2 moves segments, update ONLY `LOTTIE_MAPS["wakeup-v2.json"]`
   in `Avatar.tsx` (loading / smileArrive / hold). If the juggling
   section is a separate frame range, add a `speaking` map entry +
   a 6-line branch in `applyState` (pattern already isolated).
4. Acceptance: ghost enter → pinch → waves; talk → bars follow
   voice with < 200 ms perceived lag; silence → rest; TTS →
   rhythm; Esc → smile.

## P2 — True TTS envelope (follow-up, optional)

Tap PCM in Rust before the rodio sink → emit `audio:tts-level` on
the existing channel → Avatar prefers it over `ttsWaveLevel` when
fresh (< 300 ms), falls back otherwise. Replaces the stylized
rhythm with ground truth. No frontend architecture change (source
selector gains one branch + test).

## P3 — Stage migration (when plan 63 moves the orb)

Waves live in `Avatar.tsx`, which moves with the orb into the
stage window as a DOM layer — no rewrite. Only re-verify geometry
(`stage/geometry.ts`) and hitbox rects (waves are pointer-events
none; no hitbox needed).

## Risks → mitigations

- **Event spam / re-render storm.** 6 Hz tiny JSON + store write
  read via `getState` in rAF (no subscriptions) — measured safe.
- **Callback blocking.** try_send on bounded channel cannot block;
  worst case a level drops (latest-wins anyway).
- **v2 layout drift.** Single-map contract (§P1.3); fallback file
  always present, so a bad v2 degrades to v1, never to blank.
- **Ghost session without capture** (keyless/vision-only turns):
  waves rest at 0.15 — correct (no speech, no motion).
- **Normal (non-ghost) mode unchanged.** Drive loop mounts only in
  waves phase; wakeup.json sequencing untouched.

## Open diagnosis note (user's "create a new tab" log)

Two independent causes, both addressed/explained:
1. **Phrase gap (fixed P0).** Only exact "new tab" / "open (a) new
   tab" parsed; "create a new tab" fell into the NLU lottery.
2. **Normal mode needs the wake word per command (by design).**
   The excerpt shows no ghost session ACTIVE line — if the session
   wasn't live, post-command silence is correct behavior, not a
   bug. If it WAS live and still silent, capture the `debug_trace`
   p0–p4 lines and the loop audit reopens.
