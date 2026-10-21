# 43 — Ghost Waves: Smile-to-Waveform Transition (2026-09-25)

When a ghost session starts (mic stays hot), the orb pinches its smile
flat and grows a live 3-bar waveform in the Lottie's own palette. When
the session ends, it reverses. Pure frontend — zero Rust changes (the
`ghost:ring` events already existed).

---

## 1. Palette provenance (measured, not guessed)

`frontend/public/wakeup.json` scanned for static fill colors
(`"c":{"a":0,"k":[r,g,b]}`): exactly three — blue `#259ed6`, orange-red
`#ef4f25`, yellow `#fbdf38` (the "3 colored circles" from the loading
segment). The waves use these verbatim, left to right, tall-short-tall
like a live waveform (120/70/120px in the 180px box).

## 2. Transition design (why pinch, not swap)

A hard swap reads as a glitch; a pinch reads as a morph. `GHOST_PINCH_MS
= 220`: Lottie container `scaleX → 0.12` + fade (CSS transition), then
waves grow in (`scale 0.6 → 1`, 250ms). Exit reverses: waves unmount,
Lottie unpinches and resumes exactly the segment the current state
dictates. The Lottie is paused while waves show (no competing motion,
no wasted CPU).

Each bar oscillates `scaleY` on its own duration/delay (900/700/1100ms,
0/150/300ms) so they never move in lockstep — pinned by test.

## 3. State wiring (no new backend)

- `assistant.ts`: `ghostActive` boolean. Set from `ghost:ring`
  visibility in `App.tsx` (30Hz events, edge-guarded so unchanged values
  never retrigger subscribers). Deliberately NOT cleared by `reset()` —
  the mic stays hot across turns until the session ends.
- `Avatar.tsx`: `ghostPhase` machine (smile → pinching → waves),
  hold-frame/TTS logic skipped while waves own the visual, `waiting`
  glow composes independently (confirm-wait during ghost still glows).
- Meeting suppression, barge-in, failsafe paths untouched — they drive
  `state`/`ttsActive`, which the ghost visual intentionally ignores
  while active (the mic is hot either way).

## 4. Verification (twice)

Frontend **29/29** (new: palette/order/shape, phase offsets,
pinch-beat bound), `tsc` clean, production build clean. No Rust files
changed. Live check for the built app: say "ghost mode" → smile
pinches → waves dance in blue/orange/yellow; speak commands (waves keep
dancing across turns); end session → smile returns mid-state.
