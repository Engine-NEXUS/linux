# Feature 80 — Production Orb: Lottie Creator-MCP Pipeline, Five-State Matrix & Asset Rotation

**Date:** 2026-09-30
**Owner (orb/Lottie lane):** opencode agent (user-directed hands-on session)
**Research:** `docs/research/orb-waves/04-lottie-creator-mcp-production-session-2026-09-30.md`
**Change ledger:** `docs/changes/56-production-orb-waves-lottie-creator-pipeline.md`
**Depends on:** doc 74 (UI-director/ghost gates), doc 63 (stage), `docs/research/orb-waves/01–03`

**One-liner:** Drove the LottieFiles Creator MCP end-to-end to ship a
re-marked, palette-verified **wakeup orb** and a pruned **3-bar waves** asset,
then locked the final five-state visual matrix in CSS (`idle` freeze /
`thinking` loading / `listening`+`speaking` zoom) — all green: 18/18 tests ×3,
`tsc` clean ×3, three layered verification passes per asset.

---

# 1. PLAN

## 1.1 Objective (user directive, verbatim intent)

- Edit orb animations in LottieFiles Creator via MCP to **production level**:
  "make it perfect… cross check thrice, don't break the design."
- Agent owns the Lottie/animation lane (supersedes the earlier hands-off rule
  for this scope); Gemini concurrent session owns orchestrator/intent/ghost/
  STT/sentinel/run.ps1 — fresh reads before touching any file.
- Final state matrix delivered exactly as the user specced:
  **idle (freeze) · thinking (loading) · listening & speaking (zoom in and out)**,
  etc., applied "to make it better for all the states."

## 1.2 Scope

**In scope**
1. Wakeup orb (`wakeup.json`) — Creator markers for the app's segment
   contract, export verification, asset rotation (old → new).
2. Ghost waves — rename/normalize bars to palette, prune bar-4, stroke
   cleanup (as far as MCP allows), export verification, asset rotation.
3. App-side state matrix: listening zoom, docs, pickup chains, voice-scale
   range, keyframes dedup.
4. Verification tooling: MCP relay, batch runner, export audit scripts,
   vitest/tsc gates.
5. Documentation: this doc + research + change ledger.

**Out of scope (explicitly deferred)**
- Any keyframe/easing/structure edit to wakeup content (design freeze).
- A state machine inside Creator (TS sequencer keeps ownership).
- Baked stagger in the waves loop (CSS owns entrance).
- `error` visual state (phase-2).
- PCM-driven TTS bar rhythm (future: real audio tap; today stylized rhythm).
- `loading.json` retirement, `recorder.ts:684` stale comment (other lanes).

## 1.3 Constraints & invariants carried forward

| # | Invariant | Where enforced |
|---|---|---|
| I1 | Waves visible ONLY in active ghost session + visible orb | `shouldShowWaves` (Avatar.tsx:103, tested) |
| I2 | Waves move ONLY on mic (≥0.015) or TTS activity; rest = static | `waveSource` + rAF play/pause gate |
| I3 | Palette = trio only (`#259ed6` `#ef4f25` `#fbdf38`) | verify scripts post-export |
| I4 | Segment contract 171–260 / 261–316 / 300 unchanged | keyframe audit of export |
| I5 | Design freeze: no content edits without approval | markers-only discipline |
| I6 | Every destructive file op gets a backup first | `%TEMP%\opencode\*backup*` |
| I7 | Dual-gate: tests + tsc green twice minimum | 3 runs across session |

## 1.4 Asset flow (planned pipeline)

```
Creator tab (fileId) ──MCP edits──► scene
        │                              │
        │ (human clicks Export)        │ read_scene / list_segments
        ▼                              ▼
  public/*.json (download name)   live digest audit
        │
        ▼
  verify_exports.py / verify_waves2.py  (trio + keyframes + layers)
        │
        ▼
  rotate (backup → delete old → rename new)
        │
        ▼
  vitest 18/18 + tsc  →  user: nexus build + live acceptance
```

---

# 2. RESEARCH (summary — full detail in research 04)

| # | Finding | Impact |
|---|---|---|
| R1 | Persistent relay on `:3848` needed — per-call probes killed the `:3847` WS (user's "could not connect") | Infrastructure: `relay.mjs` owns the socket all session |
| R2 | No MCP export tool — export is a human click | Flow always ends in user hand-off |
| R3 | Creator frame = **local − 106** (wakeup `ip106/op316` @60fps; Creator `0–210`) | All marker math |
| R4 | Scene digest ids are unstable across reconnects; fingerprint by layers + frame count | Prevented wrong-file edits (D3) |
| R5 | `read_scene` reports **no paints** — verification must parse the exported JSON | Two Python audits built |
| R6 | `set_fill` misses group-scoped strokes; `set_stroke` needs `width` and appends (D1/D2) | 4-color violation found → hairline workaround → user accepted residual |
| R7 | Creator exports can land under a different filename (`waves-v2.json`) | Rotation procedure (D4) |
| R8 | Draft "Loading" file was frame-identical to contract (keyframes verified) | `LOTTIE_MAPS` untouched |
| R9 | CSS last-wins duplicate `pulse-listen` (1.12) silently beat the new 1.07 (D5) | Dedup during doc write |
| R10 | "loading" naming collision (spinner file / tab title / segment) (D6) | Clarified mapping in docs |

---

# 3. IMPLEMENTATION

## 3.1 Creator-side edits (waves file `746da7b3-e937-466b-a9bb-d0aeac3f71e8`)

**Batch A–E (17 calls, each read-back verified):**
1. Unlock 4 bars (`set_layer_lock` ×4).
2. Rename layers → `wave-bar-1..4` (left→right x = 30.5 / 43.5 / 56.5 / 69.5;
   ids `n_NrnBeFuW` `plwXj9rca9` `EFJv9mYmQT` `OCW9L8Jg9o`).
3. SOLID fills → red / blue / yellow / red bookend (trio bookend pattern).
4. Add segment `waves-loop` 0–54 (25fps comp, op 55).
5. Re-lock all.

**Batch F–G (prune + strokes, this session):**
6. Unlock ×4 → **`delete_layer` `wave-bar-4`** → 3 bars remain
   (positions 30.5 / 43.5 / 56.5).
7. `set_stroke` ×3 `width: 0.1` matched to each bar's fill
   (attempted group-stroke replacement — see D2: only added layer-level
   hairlines; original 6.5px group strokes unreachable via MCP).
8. Re-lock ×3. `list_segments` re-verified `waves-loop` intact.

**User hand steps:** scrub-checks in Creator; Export (filename came out
`waves-v2.json`); final accepted "good-enough" despite D2 residual.

## 3.2 Creator-side edits (wakeup files)

- Earlier file `8e62ba76-…`: first marker set placed (ids
  `2FM7DYrYYE` `E85rTmxeLA` `2WqfPrFnrU`), baseline saved — then the user
  switched to the **"Loading" draft `6ac52e72-…`** (D3).
- **Batch E on `6ac52e72-…`** (fresh baseline: 210 frames, 60fps, 10 layers):
  - `loading-loop` **65–154** (= local 171–260)
  - `smile-arrive` **155–209** (= local 261–316; 210 would be past last frame)
  - `hold` **194** (= local 300)
  - `list_segments` verified all three.
- User exported → `wakeup-v2.json` (17.7 KB) → audit green (see 3.4).

## 3.3 App-side code changes

### `frontend/src/styles.css`
| Change | Detail |
|---|---|
| `.avatar-wrap--listening` | was `transform: scale(1)` (dead static) → `animation: pulse-listen 0.9s ease-in-out infinite` |
| **NEW** `@keyframes pulse-listen` (line ~177) | `0%,100% scale(1)` → `50% scale(1.07)` — listening zoom: same language as speaking but slower + shallower |
| **REMOVED** duplicate `@keyframes pulse-listen` (trailing, peak 1.12) | CSS last-wins bug D5 — it had been governing `.orb--listening` and would have made listening deeper than speaking |
| untouched | `pulse-think` (1.08 @1s), `pulse-speak` (1.10 @0.55s), `.avatar-wrap--thinking`, `--speaking`, `--waiting`, `.orb--listening{…1s}` fallback, `.ghost-bar--reactive { animation: none }` |

### `frontend/src/avatar/Avatar.tsx`
| Change | Detail |
|---|---|
| Header doc (L5–19) | Sequencing rewritten to the final matrix: idle freeze / listening zoom / thinking loading / speaking zoom (replaced stale wake-choreography wording) |
| Normal-mode comment (~L249) | "listening holds the smile with its CSS zoom pulse" |
| `LOTTIE_FILES` (L124) | `["wakeup.json", "wakeup-v2.json"]` — promoted order so the production file loads first (no per-load 404; v2 slot is a silent fallback for future drops) |
| `LOTTIE_MAPS` (L119–122) | **unchanged**: `wakeup.json` + `wakeup-v2.json` both `[171,260]/[261,316]/300` — verified identical |
| Waves pickup chain (L220, L235) | `["waves-2.json", "ghost-waves.json", "waves.json"]` + matching log strings (earlier rotation; `waves.json` no longer exists — dead fallback slot, harmless) |
| Voice-scale floor (earlier this session) | `waveScaleForBar` rest 0.35 → **0.15** (L135), mic/TTS clamp `Math.max(0.15, …)` (L417–418), so lite voice visibly moves bars above silence |
| untouched | `resolveAvatarAnim` (I4 contract), `shouldShowWaves` (I1), `waveSource` (I2), `WAVE_REST_FLOOR=0.015`, pinch/leave/entrance constants |

### `frontend/public/` asset rotation
| Op | From | To | Backup |
|---|---|---|---|
| delete + rename | legacy `waves.json` | `waves-2.json` (via Creator export `waves-v2.json`) | `waves_old_backup.json` |
| delete + rename (2nd) | 4-bar `waves-2.json` (12.8 KB) | 3-bar `waves-2.json` (10.1 KB, export `waves-v2.json`) | `waves_4bar_backup.json` |
| delete + rename | legacy `wakeup.json` | promoted `wakeup.json` (17.7 KB, from `wakeup-v2.json`) | `wakeup_old_backup.json` |

Final `public/` inventory: `loading.json` 6.6 KB (untouched spinner),
`wakeup.json` 17.7 KB (production orb), `waves-2.json` 10.1 KB (production
ghost bars).

## 3.4 Verification executed (thrice rule)

**Layer 1 — live scene after every batch:** `read_scene` (layers/ids/locks/
positions) + `list_segments` (marker ranges). Batches A–G each followed by
read-back; final states recorded in research 04 §3.1.

**Layer 2 — exported-JSON audits (authoritative):**
- `verify_exports.py` on `wakeup-v2.json`: `v5.7.0 fr60 ip106 op316`, 10
  layers matching app names, keyframe table byte-for-byte contract (circles
  out 250–260, eye swap 175/180 → 254/259, face settles 289, hold 300),
  palette set = `{#259ed6, #ef4f25, #fbdf38}` → **PERFECT**.
- `verify_waves2.py` on final waves export: 3 layers, marker `waves-loop 0`,
  every paint trio-checked → **`RESULT: PERFECT - trio only`** (after D2
  accepted: renders as fill + hairline matching fill).
- Intermediate audits caught D1 (4 colors incl. `#4a90e2`) and D4 (filename
  collision).

**Layer 3 — app gates (×3 runs, all green):**
```
npx vitest run src/avatar/avatarAnim.test.tsx → 18 passed (18)
npx tsc --noEmit                              → clean
```
Covers: 5-state mapping (`thinking→loading-loop`, others `idle-smile`),
`waveSource` rest rules, `waveScaleForBar` floor 0.15 + ≤1 cap + mid-band
motion, `shouldShowWaves` ghost-only truth table, pinch/leave timings.

**Layer 4 — human scrubs:** user playhead-checked circles-only window,
face-arrival window, and final exports in Creator before each rotation.

---

# 4. RESULT

## 4.1 Final state matrix (locked)

| State | Visual | Mechanism | File:line |
|---|---|---|---|
| `idle` | **freeze** on hold frame 300 | no segment restart, no pulse | Avatar.tsx:44 + LOTTIE_MAPS |
| `thinking` | **loading** circles 171–260 @1.5x loop + wrapper pulse | `resolveAvatarAnim` → `loading-loop` + `.avatar-wrap--thinking{pulse-think}` | Avatar.tsx:41-42, styles.css:60-62 |
| `listening` | smile holds + **zoom in/out** 0.9s, 1.0→1.07 | `.avatar-wrap--listening{pulse-listen}` | styles.css:56-58, 177-181 |
| `speaking` | smile holds + **zoom in/out** 0.55s, 1.0→1.10 (deeper+faster) | `.avatar-wrap--speaking{pulse-speak}` | styles.css:64-66, 168-171 |
| `waiting` | steady glow, no pulse (existing) | `.avatar-wrap--waiting` | styles.css:71-74 |
| ghost | pinch → 3-bar waves, mic/TTS-driven | `shouldShowWaves` + `waveSource` + play/pause gate | Avatar.tsx:103, 155-166 |
| `error` | — | deferred phase-2 | — |

Listening vs speaking remain distinguishable (shallower + slower), which is
why the D5 duplicate removal mattered — the stale 1.12 keyframes would have
inverted that relationship.

## 4.2 Assets shipped

| File | Size | Verified properties |
|---|---|---|
| `frontend/public/wakeup.json` | 17.7 KB | 60fps, `ip106/op316`, 10 layers, contract keyframes intact, trio palette, Creator markers 65–154 / 155–209 / 194 |
| `frontend/public/waves-2.json` | 10.1 KB | 25fps, `ip0/op55`, **3** bars left→right red/blue/yellow fills, `waves-loop` marker, trio-only paints (accepted hairline outlines, D2) |

## 4.3 Test results (final run after D5 fix)

- `avatarAnim.test.tsx`: **18 passed / 18** ✅
- `tsc --noEmit`: **clean** ✅
- (Both gates executed 3× across the session — I7 dual-gate satisfied.)

## 4.4 Invariants proven intact

- **I1** ghost-only waves: `shouldShowWaves(visible, ghostActive)` truth table green.
- **I2** motion-only-on-activity: `waveSource` rest cases green; rAF pause gate untouched.
- **I3** trio-only: audits `PERFECT` on both final exports.
- **I4** segment contract: `LOTTIE_MAPS` unchanged; keyframe audit identical.
- **I5** design freeze: zero content edits — markers/layers/paints only.
- **I6** backups: three `*backup*.json` in `%TEMP%\opencode\` + git history.

## 4.5 Known-accepted imperfection

Bar outlines: each wave bar retains its original **6.5px contrasting stroke**
inside the shape group (red fill outlined yellow, etc. — all trio colors).
MCP cannot reach group-scoped paints (research D1/D2). **User decision:
good-enough for now.** Path to perfect: hand-delete the Stroke entry per bar
in Creator → re-export → `verify_waves2.py` → rotation.

## 4.6 Next step (user)

`nexus build` → `nexus start` → acceptance:
1. Wake → smile hold (idle freeze).
2. Speak → listening zoom; NEXUS talks → speaking zoom (deeper/faster).
3. Ask something complex → thinking circles loop.
4. Ghost mode → pinch → waves: **lite vs loud** voice scale (rest 0.15),
   TTS rhythm follows speech, Esc → smile returns.
5. Paste `[WAVES] drive` console lines if anything misbehaves.

## 4.7 Files touched (complete inventory)

**Code (2):**
- `frontend/src/styles.css` — listening animation, new keyframes, duplicate removal
- `frontend/src/avatar/Avatar.tsx` — header/comment docs, `LOTTIE_FILES` order, pickup chain, voice floor (earlier)

**Assets (3 live, 3 backups):**
- `frontend/public/wakeup.json` (promoted), `waves-2.json` (pruned), `loading.json` (untouched)
- backups: `waves_old_backup.json`, `waves_4bar_backup.json`, `wakeup_old_backup.json`

**Docs (4):** this feature 80 · research 04 · changes 56 · features README index row (80 added).

**Tooling (left in `%TEMP%\opencode\`):** `relay.mjs`, `run_batch.mjs` +
`batchA–G`, `schemas.mjs`, `verify_exports.py`, `verify_waves2.py`,
baselines + backups.

**Tests:** 23/23 avatar (`zoomForFrame` suite + speed asserts) · full frontend
suite 66/66 across 8 files · `tsc` clean (all ×2 this session).

---

# 6. FOLLOW-UP (same day) — 1.25x Speed + Command-Center Turn-Close Hardening

## 6.1 Wakeup faster: everything 1.25x (user directive)
`resolveAvatarAnim` speeds → 1.25 all states (Avatar.tsx). Frame-based
`zoomForFrame` tracks any speed automatically (frames are frames — no zoom
changes needed). Tests updated to pin 1.25.

## 6.2 Orb ↔ command-center connection audit (traced, then hardened)
Traced every request path: Rust orchestrator → `orchestrator:event` →
`net/orchestrator.ts` switch → `assistant.ts` store → orb. Handshake
verified: `result`→speak→`finishSpokenResult`→`orchestrator_done`+reset,
`done`→reset, `confirm`→5s window→idle, wake→listening, ghost session events.
Three stuck-in-`speaking` gaps found + fixed (`closeTurnOnSpeechEnd`,
normal-mode only, ghost paths untouched):
- `result` with empty text (backend withholds `done`) → close immediately.
- `error` (normal mode had no completion; ghost 3s timer preserved) → close
  at error-speech end.
- `conflict_report` (spoke with no completion) → close at summary end.
Accepted risk (unchanged): `ack` with no following `result` (backend hang) —
covered by the 60s App speaking failsafe; `wsBridge` legacy override branch.
Tests: `closeTurnOnSpeechEnd` suite (closes normal turns, leaves ghost turns
open). Full suite 66/66, tsc clean.

---

# 5. FOLLOW-UP (same day) — Original Speed, Frame-Locked Zoom, Audio-Gated Waves

User review raised three linked issues; analysis + execution below
(full dossiers: research 04 §8).

## 5.1 All speeds → original 1.0x (editor parity)
`resolveAvatarAnim` ran thinking @1.5x / speaking @1.2x via `setSpeed`
(Avatar.tsx) — the editor plays 1.0x, so the app never matched it. Now all
four states are 1.0x (test "all states play at the original 1.0x speed").
**Coupling found:** 89f loop @60fps @1.5x = 0.99s/rev accidentally resonated
with the old 1s CSS `pulse-think`; at true speed the loop is 1.48s, so the
speed fix forced the sync fix (§5.2) — shipped atomically, never separately.

## 5.2 Frame-locked zoom replaces CSS pulses (Q3)
CSS pulses (`pulse-listen/-speak/-think`) are free-running wall-clock timers
with arbitrary phase vs the Lottie timeline — same tempo is never in-sync.
Replaced by `zoomForFrame(st, frame, holding, waiting)` (Avatar.tsx, pure,
unit-tested) + one normal-mode rAF loop reading `anim.currentFrame`:
thinking zooms exactly once per 89f revolution (peak 1.04); listening/
speaking breathe on a 54f period once `holding` (peaks 1.035 / 1.08 —
speaking deeper, states distinguishable); idle/waiting/mid-arrival return
exactly 1. The loop owns wrapper `transform` (+ forces `transition:none`
while driving) and releases both when ghost/hidden. The three
`.avatar-wrap--*` pulse rules are now `animation: none`; dead
`pulse-think` keyframes removed (fallbacks `.orb--*` keep theirs).

## 5.3 Waves grow dots→bars ONLY on STT/TTS (Q1)
Causes removed: (1) `.ghost-waves { animation: ghost-grow 250ms }` container
zoom on every mount — deleted (pinch already transitions); (2) timer-based
`wavesEnter` bloom — now raised by the drive loop on the **first
audio-active tick per phase** (`grownRef`), never on a silent mount.
**Discovery:** the shipped `waves-2.json` is fully static (no keyframes), so
the Lottie branch rendered motionless full bars while the audio-reactive
procedural bars sat unreachable. Fix: drive loop now **scrubs the playhead
by live level** (`goToAndStop(level × (total−1))`, silence parks at 0) instead
of play/pause — a no-op on the static comp, and live the moment growth
keyframes land. **Pending (Creator):** dots→bars keyframes in the waves file
(frame 0 = scaleY ~0.15 dots → frame 54 = full bars, staggered per bar);
then re-export → audit → rotate, zero code changes needed.
