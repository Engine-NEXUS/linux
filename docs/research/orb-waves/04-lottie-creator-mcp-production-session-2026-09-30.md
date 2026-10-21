# Research 04 — LottieFiles Creator MCP Production Editing Session (2026-09-30)

Companion to `docs/features/80-production-orb-creator-mcp-pipeline.md`.
Chronological change ledger: `docs/changes/56-production-orb-waves-lottie-creator-pipeline.md`.

This document records everything learned while driving the LottieFiles Creator
MCP (`@lottiefiles/creator-mcp@0.2.2`) to edit the two production orb assets —
`wakeup.json` (normal-mode orb) and the ghost wave bars — including tool
schemas, persistence quirks, verification methodology, every defect discovered,
and the exact math binding Creator frames to app frames.

---

## 1. Toolchain Research

### 1.1 MCP server

| Property | Value |
|---|---|
| Package | `@lottiefiles/creator-mcp@0.2.2` |
| Tools | 108 (schema dump: `schemas.mjs <tool…>`) |
| Transport | WebSocket `127.0.0.1:3847` |
| Requirement | A connected LottieFiles **Creator browser tab** (the MCP is the tab's backplane — no tab, no tools) |
| Export | **No export tool exists.** Export is always a human click in the Creator UI (Export → Lottie JSON). MCP can only mutate the live scene. |

### 1.2 Persistent relay (infrastructure built this session)

| Piece | Path | Role |
|---|---|---|
| Relay daemon | `%TEMP%\opencode\relay.mjs` | Owns the WS to `:3847`, survives across sessions |
| HTTP bridge | `127.0.0.1:3848` | `GET /health` · `POST /call {"tool","arguments"}` |
| Relay log | `%TEMP%\opencode\relay.log` | Errors / reconnects |
| Batch runner | `%TEMP%\opencode\run_batch.mjs <batch.json>` | Sequential tool calls with per-step result lines + `BATCH_OK` / `BATCH_ERRORS=n` |
| Schema dumper | `%TEMP%\opencode\schemas.mjs <tool…>` | Prints tool description + JSON schema |
| Ad-hoc probe | `%TEMP%\opencode\mcp_probe.mjs`, `mcp_call.mjs` | One-shot experiments (caution: killed the server on exit — see 1.3) |

**Root cause of the user's "could not connect" error:** early probes opened the
WS, called, and **exited — tearing the socket down**, which also knocked the
Creator tab's connection offline. The persistent relay fixed this class of
failure permanently: exactly one long-lived process owns `:3847`. Relay is
stopped with `Stop-Process` on any `node.exe` whose command line matches
`*relay.mjs*` when the session ends.

### 1.3 Tool schemas learned (subset actually used)

| Tool | Key schema facts | Gotcha |
|---|---|---|
| `read_scene` | no args → `{scene, layers[]}` digest | **Reports names/ids/positions/locks ONLY — no fills, no strokes.** Paint verification is impossible through MCP (see 3.2). |
| `list_segments` | no args | Creator segments (markers), not app segments |
| `add_segment` / `rename_segment` / `remove_segment` | `name`, `start_frame`, `end_frame` (recreate = id changes) | Zero-based inclusive frames in **Creator** numbering |
| `set_layer_lock` | `layer_id`, `locked` | Required **before** most edits — exported bars arrive locked |
| `delete_layer` | `layer_id` | Refuses/needs unlock first; selection-aware ("delete selected") |
| `set_fill` | `layer_id`, `fill: {type:'SOLID', color:{r,g,b}}` (0–255) | Replaces **layer-level** fill only — paints buried in `shapes` groups untouched (defect D1) |
| `set_stroke` | `layer_id`, **`width` REQUIRED**, `fill` object | Same reach problem + observed **append** semantics (defect D2) |
| `set_scene_fps` | `fps` | Waves file stayed 25fps; wakeup is 60fps |
| `select_segment` | `segment_id` — sets Creator work area | Optional; used to land the playhead on a marker for user scrub |

### 1.4 Persistence model (critical)

- **Lottie JSON export only persists what the export button writes.** Creator
  session state (scene name/id, selection) is NOT the file.
- **Reconnecting the tab yields a fresh "Main Scene" digest** even for the same
  `fileId` (observed: wakeup scene reported `cZ5R3Jap2x` after reconnect while
  the working file was `6ac52e72-…`). Markers and layer edits DO persist in the
  Creator file; only the digest identity is unstable. Never key logic off
  `scene.id`.
- The user can open a **different fileId mid-session** (tabs titled by file
  name, e.g. "Loading"). Every batch must start with `read_scene` to confirm
  WHICH file is attached (layer names + frame count are the fingerprint).

---

## 2. Frame Math & Content Research

### 2.1 Wakeup — Creator ⇄ app offset

| | Creator (60fps) | App (`wakeup.json`) |
|---|---|---|
| Composition | `ip 0 / op 210`, 10 layers | `ip 106 / op 316`, same 10 layers |
| **Offset** | **Creator frame = local frame − 106** | local = creator + 106 |

Conversion table used for all markers:

| App segment (contract) | Creator marker | Duration |
|---|---|---|
| `loading` 171–260 | `loading-loop` **65–154** | 89f |
| `smile-arrive` 261–316 | `smile-arrive` **155–209** (210 = last frame → 209) | 54f |
| `hold` 300 | `hold` **194** (single frame) | 0f |

### 2.2 Wakeup keyframe contract — verified identical in the new export

Parsed from the exported `wakeup.json` (script `verify_exports.py`):

| Layer | Keyframes (local frames) | Meaning |
|---|---|---|
| `circle m` | opacity 170→250 (exit 170–260 window) | center circle leaves during loading |
| `circle el` | opacity 175→255 | left circle |
| `circle er` | opacity 180→260 | right circle |
| `eye L` | opacity 175→176 | old eye swaps out early |
| `eye R` | opacity 180→181 | old eye swaps out |
| `eye L 02` | opacity 254→255 | **new eye fades in at ~254** |
| `eye R 02` | opacity 259→260 | new eye fades in at ~259 |
| `face ctrl` | scale 139→289, pos 136→286 | face settles by 289 |
| `mouth` | scale 249→250 | mouth settles |
| `extra motion ctrl` | pos 106→375 | spans full comp |

**Conclusion:** the "Loading" draft export is frame-identical to the contract —
circles fully gone by 260, face settled 289, hold 300 valid ⇒
`LOTTIE_MAPS` needed **zero changes** (design freeze honored: no keyframe
content edits were ever made — markers only).

### 2.3 Waves

| Property | Value |
|---|---|
| Composition | 25fps, `ip 0 / op 55` (54 usable frames) |
| Layer order (left→right) | `wave-bar-1` x=30.5 · `wave-bar-2` x=43.5 · `wave-bar-3` x=56.5 (· `wave-bar-4` x=69.5 — deleted this session) |
| Original ids | bar-1 `n_NrnBeFuW` · bar-2 `plwXj9rca9` · bar-3 `EFJv9mYmQT` · bar-4 `OCW9L8Jg9o` |
| Segment | `waves-loop` 0–54 (exported marker present, `tm: 0`) |
| Intended paint | fills only: bar-1 red `#ef4f25`, bar-2 blue `#259ed6`, bar-3 yellow `#fbdf38` (bookend red, trio only) |

App pickup chain: `["waves-2.json", "ghost-waves.json", "waves.json"]`
(Avatar.tsx:220) — waves-2 is production; the other two are silent fallback
slots (waves.json no longer exists in `public/`, harmless 404→next).

### 2.4 Palette (three colors, user mandate)

| Token | Hex | RGB |
|---|---|---|
| blue | `#259ed6` | 37,158,214 |
| red | `#ef4f25` | 239,79,37 |
| yellow | `#fbdf38` | 251,223,56 |

No other colors allowed in either asset. Sampled from original wakeup strokes;
enforced post-export by the verify scripts (see 3.2).

---

## 3. Verification Methodology (thrice-cross-check rule)

### 3.1 Live scene checks (after every MCP batch)

```
POST /call {"tool":"read_scene","arguments":{}}     → layers, names, ids, positions, locks
POST /call {"tool":"list_segments","arguments":{}}  → marker names + ranges
```
Every batch in `%TEMP%\opencode\batchA–G_*.json` was followed by one of these.
Batches: A/B/C (waves unlock→rename→fill→segment→lock, 17 calls), E (wakeup
segments on "Loading"), F (bar-4 delete + strokes), G (stroke retry).

### 3.2 Exported-JSON audits (authoritative — MCP cannot see paints)

- **`verify_exports.py`** — for a given file: version/fps/ip/op, layer list,
  keyframe time-ranges for pos/scale/opacity per layer, recursive paint dump
  (fills + strokes with hex), palette set membership.
- **`verify_waves2.py`** — waves-specific: markers, layer count, every
  fill/stroke with hex **and stroke width**, `RESULT: PERFECT - trio only` or
  `DEFECT - n extra paints`.

Both run with plain `python`, no deps (stdlib `json`).

### 3.3 App-side gates (run three times across the session, all green)

```
npx vitest run src/avatar/avatarAnim.test.tsx   → 18/18
npx tsc --noEmit                                → clean
```
Test surface: `resolveAvatarAnim` (5-state mapping), `waveSource`,
`waveScaleForBar` (floor 0.15), `shouldShowWaves` (ghost-only invariant),
entrance/exit timing.

### 3.4 Human scrubs (user in Creator)

1. Circles-only at playhead 65–150, face-arrival after 155 → accepted.
2. Full smile visible at Creator frame 134 on the "Loading" draft (used to
   prove the draft ≠ the previously marked file, forcing a fresh baseline).
3. Final export spot-check before rotation → accepted as good-enough.

---

## 4. Defects Found & Resolution Log

### D1 — `set_fill` leaves group-level strokes (4-color violation)
**Symptom:** post-edit export showed fills correct but every bar still carried
its original stroke in a *different* color, plus bar-4 in a 4th color
(`#4a90e2` — not in the trio). **Cause:** `set_fill` replaces the layer-level
fill; the legacy `st` paints live inside `shapes → groups`, out of reach.
**Fix attempted:** `set_stroke` per bar matched to its fill color.

### D2 — `set_stroke` appended instead of replacing (and needs `width`)
**Symptom 1:** first attempt failed zod validation — `width` is **required**
(unlike `set_fill`). **Symptom 2:** with `width: 0.1` the tool added a NEW
layer-level hairline stroke rather than replacing the 6.5px group stroke —
final export carries **two strokes per bar**: original 6.5px contrasting +
0.1px self-colored. **Cause:** `replace_first` targets `layer.strokes[0]`,
which is empty; the real strokes are group-scoped.
**Resolution:** MCP cannot reach these paints. Two options documented:
(a) hand-delete the Stroke entry per bar in Creator (1 min), (b) accept.
**User decision: accepted as good-enough for now** — all colors are trio,
geometry/structure perfect; outlines are cosmetic at orb scale.

### D3 — File-id confusion ("which file is attached?")
**Symptom:** markers appeared missing; scene id didn't match any known file.
**Cause:** user's Creator tab was on the "Loading" draft (`6ac52e72-…`), a
different fileId than the earlier wakeup file — plus unstable digest ids (1.4).
**Resolution:** re-baselined from `read_scene` (210 frames/60fps fingerprint),
re-placed markers there, verified via export. Lesson: **fingerprint by layers
+ frame count, never by scene id or tab title.**

### D4 — Export filename collision (waves)
**Symptom:** `public/` contained both stale `waves-2.json` (12.8KB, 4 bars) and
fresh `waves-v2.json` (10.1KB, 3 bars) — the app would silently load the stale
one. **Cause:** Creator's export default name differed from the pickup name.
**Resolution:** rotation procedure — backup old → delete → rename new
(backups: `%TEMP%\opencode\waves_4bar_backup.json`).

### D5 — Duplicate `@keyframes pulse-listen` (found during doc-writing)
**Symptom:** claimed "missing keyframes" for `.orb--listening` was wrong — a
pre-existing `pulse-listen` (peak **1.12**) lived at the bottom of
`styles.css`; the new definition (peak **1.07**) sat above it, and CSS
**last-wins** meant the *old, deeper* keyframes silently governed listening —
making listening zoom **deeper than speaking** (design intent: shallower).
**Fix:** deleted the trailing duplicate; single definition at
`styles.css:177` (1.07 @ 0.9s wrapper). Verified: grep count = 1, 18/18, tsc.

### D6 — Naming confusion in review ("loading" = 3 things)
The word "loading" collided: (1) `loading.json` (top-right spinner, untouched,
future retire candidate), (2) Creator tab title "Loading" (wakeup draft file
name), (3) `loading-loop` segment marker (circles = **thinking** state).
**Resolution:** documented mapping; recommendation: marker could be renamed
`thinking-circles` someday — same data, clearer name.

### D7 — State-mapping misreading during Q&A
User asked "marker loading is it listening state right" → answered with the
authoritative table: loading circles = **thinking** (+ speaking shares the
loop only in legacy paths); listening = smile + zoom. Confirmed by
`resolveAvatarAnim` (Avatar.tsx:41-44): only `thinking` returns
`loading-loop`; listening/speaking/idle return `idle-smile`.

---

## 5. Design Decisions Locked This Session

1. **Five-state matrix (user directive, final):**
   - `idle` → **freeze** on hold frame 300 (zero motion)
   - `thinking` → **loading** circles loop 171–260 @1.5x + `pulse-think`
   - `listening` → smile + **zoom in/out** (`pulse-listen` 0.9s, 1.0→1.07)
   - `speaking` → smile + **zoom in/out** (`pulse-speak` 0.55s, 1.0→1.10, deeper+faster than listening so states stay distinguishable)
   - `waiting` → steady glow (existing modifier, untouched)
   - `error` → deferred (phase-2), not in scope
2. **Ghost waves stay ghost-only** (`shouldShowWaves = visible && ghostActive`,
   Avatar.tsx:103) — no waves on plain wake. Waves move ONLY on STT mic
   activity or TTS (`waveSource`, floor `WAVE_REST_FLOOR = 0.015`).
3. **Design freeze:** no keyframe/easing/structure/palette changes to wakeup
   content without explicit approval — only markers were added.
4. **No state machine in Creator** — TS sequencer (`resolveAvatarAnim` +
   Avatar rAF loop) keeps single ownership; segments are navigation aids only.
5. **No stagger baked into the waves loop** — entrance stagger is CSS
   (`waveEnterDelayMs`, 90ms steps).
6. **Asset rotation over in-place overwrite** — old file backed up to
   `%TEMP%\opencode\` + git history before any delete/rename.

---

## 6. Artifacts Left on Disk

| Path | What |
|---|---|
| `%TEMP%\opencode\relay.mjs` / `relay.log` | persistent MCP relay (port 3848) |
| `%TEMP%\opencode\run_batch.mjs`, `batchA–G_*.json` | batch history (every MCP call made) |
| `%TEMP%\opencode\schemas.mjs`, `mcp_probe.mjs`, `mcp_call.mjs` | schema/probe tooling |
| `%TEMP%\opencode\verify_exports.py`, `verify_waves2.py` | export audit scripts |
| `%TEMP%\opencode\scene_baseline.json`, `wakeup_baseline.json` | pre-edit digests |
| `%TEMP%\opencode\waves_old_backup.json` | legacy 4-color waves.json (pre-rename) |
| `%TEMP%\opencode\waves_4bar_backup.json` | 4-bar waves-2.json (pre-prune) |
| `%TEMP%\opencode\wakeup_old_backup.json` | legacy wakeup.json (pre-promotion) |

## 7. Open Items

- [ ] Cosmetic: 6.5px contrasting bar strokes (D2) — user hand-delete in
  Creator + re-export when they want bars perfectly solid.
- [ ] Stale comment: `recorder.ts:684` still references removed
  `wakeChoreographyDone` (noted earlier, untouched — Gemini lane).
- [ ] `loading.json` top-right spinner — candidate for retirement once the
  stage/orb covers all feedback.
- [ ] Rebuild + live acceptance (`nexus build` + `nexus start`) — **user's
  step**: 5 states → ghost voice scale (lite vs loud) → TTS rhythm → Esc.

---

## 8. Follow-up research (same day) — speed parity, phase-lock, audio gating

### R11 — Shipped waves Lottie is fully static (parsed proof)
`waves-2.json` layers report `scale static:[100,100,100]`, `opa static:100`,
no keyframe arrays at all. With the file present, the app always takes the
Lottie branch (Avatar.tsx render) — the audio-reactive procedural bars are
unreachable in production; the branch only toggled container opacity
0.85↔1. Dots→bars existed solely in the fallback. **Fix shipped:**
level→frame scrub (`goToAndStop(level × (total−1))`) replaces play/pause;
silence parks at frame 0. Verified forward-compatible: no-op on the static
comp, live once growth keyframes exist.

### R12 — 1.5x/1.0s accidental resonance (why speed + sync ship together)
89 frames ÷ (60fps × 1.5) = **0.988s/rev** ≈ `pulse-think` 1.0s — the old
zoom felt synced by luck. At 1.0x the loop is **1.481s**; any wall-clock
pulse now beats against it. Hence atomic shipment: speeds to 1.0x AND zoom
to the frame-locked rAF driver in the same change, with tests pinning both.

### R13 — CSS can never phase-match Lottie (formal)
A CSS `animation` starts at class-apply time; the Lottie playhead starts at
segment-play time. The phase offset between them is arbitrary per transition
and unobservable from CSS — no duration tuning can fix this, only reading
`animation.currentFrame` per rAF and deriving scale purely (`zoomForFrame`).
Periods chosen: 89f (thinking = 1 rev), 54f ≈ 0.9s (listening/speaking
breathe, preserving the old feel minus drift).

### Creator spec — waves growth keyframes (pending user/MCP session)
Per bar (shape layer), scale-Y keyframes: frame 0 → 15%, frame 54 → 100%,
staggered (bar-1 leads, bar-3 trails ≈ the old 90ms CSS stagger language).
Opacity stays 100 throughout (rAF container gate already handles fade).
After export: `verify_waves2.py` must show `scale` keyframe ranges per layer;
then rotate into `public/waves-2.json`. Scrub code needs no changes.

### D8 — `ghost-grow` container zoom deleted
`.ghost-waves { animation: ghost-grow 250ms }` (scale 0.6→1) replayed on
every waves-div mount — silent-room zoom with zero audio. The 220ms pinch
already owns the transition; keyframes block removed. Entrance bloom
(`ghost-enter-rise`, stagger preserved) now fires on first audio-active tick
per phase (`grownRef`), closing 650ms after the last bar lands.

### D9 — `result` with empty text parked the orb in `speaking` forever
`net/orchestrator.ts` only closed the handshake inside `if (ev.text)`; the
backend withholds `done` on success (`network.rs:290`), so a textless result
left `currentRequestId` set + state `speaking` + the hide timer re-arming
forever. Fix: `else → finishSpokenResult(ev.request_id)` (id already stored).

### D10 — `error` / `conflict_report` spoke with no completion (normal mode)
Both set `speaking` + bare `speak()` (no `onEnd`); the error path's 3s timer
only ends ghost turns (`endGhostTurn` is a no-op outside ghost). Normal-mode
backend failures = dead air + stuck orb. Fix: `currentRequestId =
ev.request_id` + speak `onEnd/catch → closeTurnOnSpeechEnd` (normal only;
ghost behavior byte-preserved). Follow-up backend `done`s are harmless
(idempotent id check + idempotent `reset()`).

### R14 — Command-center connection map (verified by trace)
`wake (main.tsx:179) → listening → STT transcript → thinking → Rust
orchestrator → orchestrator:event → {ack → speaking (result follows) |
result → speaking → finishSpokenResult → orchestrator_done + reset | done →
reset | error/conflict → speaking → closeTurnOnSpeechEnd | confirm →
speaking + awaitingInput → 5s window → idle} → hideOrbAfterSpeech (state-aware,
never mid-speech/ghost)`. `transition()` gate honored on the wsBridge path;
server `state` events apply directly. Every request closes exactly once.
