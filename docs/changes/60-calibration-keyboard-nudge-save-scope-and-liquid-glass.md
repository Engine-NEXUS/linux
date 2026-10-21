# Calibration Follow-Up: Waves Preview, Keyboard Nudge, Save Scope (Liquid Glass Deferred)

**Date:** 2026-10-01
**Plan:** `docs/research/command-hub/03-keyboard-nudge-save-semantics-and-liquid-glass-2026-10-01.md` (Phases 0/2/1 implemented; Phase 3 deferred per user — code reverted)
**Status:** All automated gates green; manual acceptance = user-run.

---

## Phase 0 — Waves-preview defects (user report: garbled preview on Wakeup→Waves)

Root causes (all verified in `Avatar.tsx`): the ghost-waves **container** mounted on target switch, but (D1) the Lottie mount effect was hard-gated on `ghostPhase` (unreachable without a ghost session) so the asset never loaded; (D2) procedural bars froze at `scaleY(0.15)` for the same reason; (D3) returning to Wakeup left a blank orb because the smile init effect (`[animationData]`) never re-attached to the fresh div.

Fix: `resolveWavesShown()` selector (single gate for render + mount + deps), preview playback (Lottie 0.6x loop; bars without `--reactive` so the CSS loop runs), `calibrationTarget` in the smile init deps. Ghost paths byte-identical. +4 selector tests.

## Phase 2 — Keyboard control

- Numbered badges `[ 1 🌟 Wakeup ] [ 2 🌊 Waves ] [ 3 ⏳ Loading ]`; `1/2/3` (+numpad) select via pure `resolveCalibrationKey()` (+4 tests).
- Arrows nudge 1px, ⇧ arrows 10px, through new `calibration_nudge` (Rust `overlay_nudge` — same conversion path as drag/wheel, +6 table tests incl. DPI/edges/round-trip).
- Window-scoped keydown only (HUD focused) — zero global registration.
- Nudge bursts coalesce to ONE undo entry (`commitCoalesced` + test); live motion stays 1:1.

## Phase 1 — Save semantics

- Behavior confirmed: one Save persists everything positioned; untouched targets keep disk values.
- Added communication: ✓ dirty-dots per tab (`dirty` flags in state payload; `initial` snapshot restored to the Rust session for the comparison) + scoped toast (`"Wakeup + Waves saved."` / `"All 3 animations saved."` / `"No changes — placement unchanged."`, pure `save_toast()` + tests).

## Phase 3 — Liquid-glass HUD

- Pre-show fake-blur capture into `PENDING_CALIBRATION_BACKDROP` (same `capture_and_blur` as sidebars; strictly before `hud.show()`), fetched once by the HUD on mount (one-shot take, +1 Rust test).
- One-shot luminance probe (8×8 canvas) picks light/dark token sets — decided once, flicker impossible by construction (plan's hysteresis band dropped as unnecessary).
- Non-Windows / capture failure → current tint fallback.

## Gates

| Gate | Result |
|---|---|
| Rust serial | **764/764** (+8: 6 nudge + 2 toast/dirty) |
| Frontend vitest | **95/95 calibration scope** (+8: 4 selector + 4 keys; full suite 97/97 incl. 2 unrelated liquidGlass) |
| tsc / clippy (touched ranges) | clean / zero new |
| `npm run build` + release `--features custom-protocol` | clean (built below) |

## Deferred: Phase 3 liquid glass (user call 2026-10-01)

Implemented + verified, then **fully reverted** (backdrop static/capture/command/test, `theme.ts` + tests, HUD wiring, light tokens). The full Phase-3 plan is retained in research 03 for later. The HUD keeps its dark-tint pill.

## Manual acceptance (user-run)

1. Toggle Wakeup↔Waves repeatedly — lively waves every time, smile intact on return.
2. Bright + dark wallpapers — pill legible both, theme switches once.
3. `1/2/3` select (HUD focused); arrows 1px / ⇧ 10px live; hold arrow → one Undo step reverses the burst.
4. Position 1 target → dirty ✓ dot → Save → scoped toast; disk holds all 9.
