# Thinking-State Single-Axis Globe Spin (2026-10-04)

**User asks:** (1) wake formation reads as a "small transparent box" — causes analyzed, no box edge exists anywhere (all containers transparent; the confine IS the 200px orb canvas); (2) while TTS initializes, particles must rotate in ONE direction, left-to-right, infinitely — rim/border preserved.
**Scope:** thinking-state rotation only. Formation/box work already shipped (doc 78: 2× particles, 400px rails, thinking-entry burst).

## Finding that reshaped the plan
The approved plan assumed the braided knot (no rigid spin). The tree had moved: the concurrent session replaced the knot with a **64-ray 3D starburst** (`rotate3D(spokePos, t*0.45, t*0.28)` — dual-axis tumble). The tumble's pitch term moves particles vertically, violating "one direction". Fix applied to the CURRENT code, not the plan's assumed code.

## What changed (`frontend/src/avatar/voice-orb.js`, 4 lines + comments)
- GL: `rotate3D(spokePos, t*0.45, t*0.28)` → `rotate3D(spokePos, t*0.55, 0.0)` — pure yaw: front flows +x (left-to-right), infinite, non-reversing (~11s/rev); strand ripple underneath untouched.
- 2D fallback: yaw `t*0.45→t*0.55`, pitch `t*0.28→0.0` (parity; with pitch 0 the pitch stage is identity).
- Rim/border untouched (`rim` + depth terms) — the neat border in the user's screenshot survives rotation (circular symmetry).

## Small-box causes (analysis, for the record)
1. Canvas confinement (primary): orb canvas = OrbFrame div = orb rect (200px default) — `assemble()` scatter clips at the canvas edge; only `EntranceBurst` (900ms) is screen-wide.
2. Hotkey-while-visible: no false→true edge → no burst/assemble at all (state change only).
3. Burst subtlety: 140 dots, ≤3px, alpha ≤0.7, 900ms — easy to miss.
4. Default still 200px (400 needs calibration scroll).
5. Stale-binary check: taskbar presence / Alt-Tab → old `main` OS window → `nexus start`, never the Start-Menu shortcut.

## Verify
- tsc 0; vitest **154/154**; release binary **52.7 MB**; spin rate confirmed in dist.
- Uncommitted. Live (`nexus start`): ask a question → starburst visibly globe-spins left-to-right through the whole synthesis gap, rim stays neat.
