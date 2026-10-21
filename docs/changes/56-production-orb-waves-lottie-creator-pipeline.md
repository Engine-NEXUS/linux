# Changes 56 — Production Orb Lottie/Creator Pipeline & Five-State Matrix (2026-09-30)

Chronological change ledger for the orb-animation session.
Plan + results: `docs/features/80-production-orb-creator-mcp-pipeline.md`
Research + defect dossiers: `docs/research/orb-waves/04-lottie-creator-mcp-production-session-2026-09-30.md`

**Gates (final):** `avatarAnim.test.tsx` 18/18 ✅ · `tsc --noEmit` clean ✅
(both green 3× across the session). No Rust touched; no tests added/changed.

---

## A. Infrastructure (temp tooling — `%TEMP%\opencode\`)

| # | Change | Detail |
|---|---|---|
| A1 | **Persistent MCP relay built** | `relay.mjs` owns the WS to LottieCreator `127.0.0.1:3847`; exposes HTTP bridge `127.0.0.1:3848` (`GET /health`, `POST /call {"tool","arguments"}`); log `relay.log`. Root-fixed the user's "could not connect" (per-call probes killed the socket on exit). Stopped at session end. |
| A2 | **Batch runner** | `run_batch.mjs <batch.json>` — sequential MCP calls, per-step output, `BATCH_OK` / `BATCH_ERRORS=n`. History: `batchA–G_*.json` (every MCP call this session). |
| A3 | **Schema dumper** | `schemas.mjs <tool…>` — description + JSON schema per tool (source of the `set_stroke.width` REQUIRED finding). |
| A4 | **Export audits** | `verify_exports.py` (wakeup: fps/ip/op/layers/keyframes/palette) and `verify_waves2.py` (waves: layers/markers/paints+widths → `PERFECT`/`DEFECT`). Stdlib-only. |

---

## B. Creator-side edits — waves file `746da7b3-…`

| # | Change | Before → After |
|---|---|---|
| B1 | Unlock 4 bars | `locked=True` → editable (`set_layer_lock` ×4) |
| B2 | Rename layers | raw names → `wave-bar-1..4`, left→right x = 30.5 / 43.5 / 56.5 / 69.5 |
| B3 | SOLID fills → trio bookend | bar-1 red `#ef4f25`, bar-2 blue `#259ed6`, bar-3 yellow `#fbdf38`, bar-4 red |
| B4 | Add segment | none → `waves-loop` 0–54 (25fps, op 55) |
| B5 | Re-lock | all 4 locked |
| B6 | **Delete bar-4** (this session) | 4 layers → 3 (`delete_layer OCW9L8Jg9o`); positions 30.5/43.5/56.5 remain |
| B7 | Stroke hairlines (D2 workaround) | `set_stroke width:0.1` per bar matched to its fill (red/blue/yellow) — attempted to neutralize leftover 6.5px group strokes; MCP only added layer-level hairlines (see D2) |
| B8 | Re-lock ×3 + verify | `read_scene` 3 layers, `list_segments` `waves-loop` intact |

**User hand steps:** scrub checks in Creator; Export (landed as
`waves-v2.json`); accepted final state ("good enough for now") with D2
residual (original 6.5px contrasting strokes inside shape groups).

---

## C. Creator-side edits — wakeup file(s)

| # | Change | Detail |
|---|---|---|
| C1 | First markers on `8e62ba76-…` | `loading-loop` 65–154 (`2FM7DYrYYE`), `smile-arrive` 155–209 (`E85rTmxeLA`), `hold` 194 (`2WqfPrFnrU`); baseline `wakeup_baseline.json` |
| C2 | **Wrong-file discovery (D3)** | user's tab switched to "Loading" draft `6ac52e72-…` (0 segments) — re-baselined via `read_scene` (210f/60fps/10 layers), fresh marker math from frame-identical keyframes |
| C3 | Batch E markers on `6ac52e72-…` | `loading-loop` **65–154** (`sqvLSbUk-d`), `smile-arrive` **155–209** (`zBovjxpeRQ`), `hold` **194** (`mLIS6YAJNf`); `list_segments` verified. Offset: Creator = local − 106 (171→65, 261→155, 300→194) |
| C4 | User export | `wakeup-v2.json` (17.7 KB) — audit: `v5.7.0 fr60 ip106 op316`, contract keyframes intact, trio palette → **PERFECT** |

No keyframe/easing/structure/palette content edits were made (design freeze).

---

## D. Asset rotations (`frontend/public/`)

| # | Op | Detail | Backup |
|---|---|---|---|
| D1 | delete `waves.json` + promote first export | legacy 4-color waves → `waves-2.json` (12.8 KB, 4 bars) | `%TEMP%\opencode\waves_old_backup.json` |
| D2 | delete 4-bar `waves-2.json` + promote prune export | Creator's `waves-v2.json` (10.1 KB, 3 bars) → renamed `waves-2.json` (stale-file collision fixed, D4) | `%TEMP%\opencode\waves_4bar_backup.json` |
| D3 | delete legacy `wakeup.json` + promote export | `wakeup-v2.json` (17.7 KB) → renamed `wakeup.json` | `%TEMP%\opencode\wakeup_old_backup.json` |

Final `public/`: `loading.json` 6.6 KB (untouched) · `wakeup.json` 17.7 KB ·
`waves-2.json` 10.1 KB.

---

## E. Code — `frontend/src/styles.css`

| # | Location | Before → After |
|---|---|---|
| E1 | `.avatar-wrap--listening` (:56-58) | `transform: scale(1)` (dead) → `animation: pulse-listen 0.9s ease-in-out infinite` |
| E2 | NEW `@keyframes pulse-listen` (~:177-181) | — → `0%,100% {scale(1)} / 50% {scale(1.07)}` with rationale comment (slower + shallower than speaking) |
| E3 | REMOVED duplicate `@keyframes pulse-listen` (trailing block) | peak **1.12** deleted — CSS last-wins bug (D5): the stale copy had been silently governing listening, making it **deeper** than speaking (1.10). Post-fix grep count = 1 |
| E4 | untouched | `pulse-think` 1.08@1s, `pulse-speak` 1.10@0.55s, `--thinking/--speaking/--waiting` wrappers, `.orb--listening{pulse-listen 1s}` fallback (now resolves to the single 1.07 definition), `.ghost-bar--reactive{animation:none}` |

---

## F. Code — `frontend/src/avatar/Avatar.tsx`

| # | Location | Before → After |
|---|---|---|
| F1 | Header doc (L5-19) | stale wake-choreography sequencing → final matrix: idle freeze / listening zoom (0.9s shallow) / thinking loading (1.5x + pulse-think) / speaking zoom (0.55s deeper) |
| F2 | Normal-mode comment (~L249) | "speaking/idle/listening hold the smile" → "idle freezes on hold; speaking/listening hold the smile with their CSS zoom pulses" |
| F3 | `LOTTIE_FILES` (L124) | `["wakeup-v2.json","wakeup.json"]` → `["wakeup.json","wakeup-v2.json"]` (post-rotation: production first, no per-load 404; v2 = silent fallback slot) |
| F4 | Load comment (~L190 area) | "prefer wakeup-v2 (user-supplied juggling file)" → "wakeup.json is production (promoted from Creator-marked export); v2 is a future drop slot" |
| F5 | waves pickup chain (L220, L235) *(earlier rotation)* | `["waves.json","ghost-waves.json",…]` → `["waves-2.json","ghost-waves.json","waves.json"]` + matching log strings (last slot now dead — file deleted — harmless 404→next) |
| F6 | voice intensity floor *(earlier this session)* | `waveScaleForBar` rest 0.35 → **0.15** (L135) + mic/TTS clamps `Math.max(0.15, …)` (L417-418) + comment — lite voice now visibly moves bars above silence; full range lite≈0.2–0.4 / medium≈0.5–0.75 / loud≈0.8–1.0 |
| F7 | untouched (verified) | `SEG_LOADING [171,260]`, `SEG_SMILE_ARRIVE [261,316]`, `resolveAvatarAnim` (thinking→loading-loop, all else idle-smile), `LOTTIE_MAPS` both entries `[171,260]/[261,316]/300`, `shouldShowWaves`, `waveSource`, `WAVE_REST_FLOOR 0.015`, pinch/leave/entrance constants |

---

## G. Defects fixed during the session

| ID | Defect | Resolution |
|---|---|---|
| D1 | `set_fill` left group strokes → 4-color export incl. stray `#4a90e2` | `set_stroke` hairlines (B7) + full prune at source (B6); post-export audit now trio-only |
| D2 | `set_stroke` requires `width` AND appends (can't replace group strokes) — bars keep 6.5px contrasting outlines | documented; **user accepted** (one hand-delete per bar from Creator = perfect later) |
| D3 | Wrong Creator file edited (digest ids unstable, tab titled "Loading") | fresh `read_scene` baseline, fingerprint = layers + frame count, markers re-placed (C3) |
| D4 | Export filename collision (`waves-v2` vs pickup `waves-2`) → stale file would load | rotation procedure + backups (D2 row) |
| D5 | **Duplicate `@keyframes pulse-listen`** (stale 1.12 last-wins over new 1.07) | trailing duplicate deleted (E3); single definition, grep = 1 |
| D6 | "loading" naming = 3 things (spinner file / tab title / segment) | clarified in docs; marker rename to `thinking-circles` noted as future nicety |
| D7 | Review Q: "loading marker is listening?" | answered with authority: loading circles = **thinking** only (`resolveAvatarAnim`); listening = smile + zoom |

---

## H. Verification log

| Gate | Runs | Result |
|---|---|---|
| MCP `read_scene` / `list_segments` after every batch | batches A–G | edits confirmed live |
| `verify_exports.py` (wakeup export) | 1 | PERFECT — contract keyframes + trio |
| `verify_waves2.py` (waves export) | 1 (final) | `PERFECT - trio only` (3 layers) |
| `npx vitest run src/avatar/avatarAnim.test.tsx` | 3 | **18/18** each |
| `npx tsc --noEmit` | 3 | **clean** each |
| User scrub checks (Creator) | 3 | circles window / face arrival / final exports accepted |

## I. Docs written

| Doc | Purpose |
|---|---|
| `docs/features/80-production-orb-creator-mcp-pipeline.md` | plan · implementation · results · file inventory |
| `docs/research/orb-waves/04-lottie-creator-mcp-production-session-2026-09-30.md` | toolchain research, frame math, D1–D7 dossiers, verification methodology, artifacts |
| `docs/changes/56-production-orb-waves-lottie-creator-pipeline.md` | this ledger |
| `docs/features/README.md` | index row 80 added |

## J. Open follow-ups

- [x] D2 cosmetic cleanup — SUPERSEDED by full bar rebuild (batch J):
  static stroked lines + trim growth, export audits PERFECT (round caps,
  trio, no pulse keys).
- [ ] Live acceptance rebuild (`nexus build` + `nexus start`): 5 states →
  ghost voice scale → TTS rhythm → Esc (user's step).

## K. Same-day follow-up — 1.25x speed + turn-close hardening

| # | File:line | Change |
|---|---|---|
| K1 | `Avatar.tsx` speeds | all states 1.0 → **1.25** (user: "everything faster"); zoom untouched (frame-locked, speed-independent) |
| K2 | `avatarAnim.test.tsx` | speed asserts → 1.25 (3 spots) |
| K3 | `orchestrator.ts` +`closeTurnOnSpeechEnd` | new helper: normal-mode turns close at speech end; ghost → false (legacy paths own) |
| K4 | `orchestrator.ts` result | empty-text `else → finishSpokenResult` (D9) |
| K5 | `orchestrator.ts` error | `currentRequestId` set + speak onEnd/catch → close helper (D10); 3s ghost timer preserved |
| K6 | `orchestrator.ts` conflict_report | same close pattern (D10) |
| K7 | `orchestrator.test.ts` | close-contract suite (closes normal, leaves ghost open) |
| K8 | gates | full frontend suite **66/66 (8 files)** + `tsc` clean |

## L. Same-day follow-up 2 — rest dots + random shimmer (file untouched)

| # | File:line | Change |
|---|---|---|
| L1 | audit correction | trim 50/50 renders NOTHING (zero-length subpaths culled) — Creator preview was right; rest-empty root-caused (D12) |
| L2 | `Avatar.tsx` +`restDotScale`/`REST_DOT_X` | procedural rest dots at exact file geometry (30.5/43.5/56.5%, ⌀12px, trio), organic non-order shimmer (0.31/0.47/0.23Hz + phases) |
| L3 | `Avatar.tsx` drive loop | Lottie visible ONLY when active (opacity 1/0, was 0.85); dots visible ONLY in silence; seamless handoff (same spots/sizes) |
| L4 | `avatarAnim.test.tsx` | rest-dot suite (geometry + range + non-lockstep) |
| L5 | gates | full frontend suite **68/68 (8 files)** + `tsc` clean |
| L6 | relay | stopped on user request (MCP disconnected) |
| L7 | rest-dot Y (D13) | dots eyeballed at 50% while file bars sit at y=24.2 → 46px jump every rest↔speech handoff; `REST_DOT_Y=24.2` (file geometry), test pins it |
- [ ] `recorder.ts:684` stale `wakeChoreographyDone` comment (Gemini lane, untouched).
- [ ] `loading.json` spinner retirement (future).
