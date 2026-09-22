# Stuck Wakeup Animation — Root-Cause Research (2026-09-18)

**Symptom (user report + log):** after a long WorkerBackend reply (PR #68
analysis, ~15s of audio plus a second TTS chunk), the orb's wakeup/loading
animation never disappears. Short replies recover visibly; long replies wedge
it. User asked: is this the "ghost mode" I applied? **No — verdict in §6.**

**Conclusion up front:** the orb parks in `speaking` with the loading-loop
segment forever because the `done` handshake is missing on the
`WorkerBackend` success path. The `result` handler speaks with no `onEnd`
and `signalOrchestratorDone()` has zero call-sites. A secondary,
independent bug can wedge the small top-right loading spinner: the
`loading-indicator` window has two owners (Rust direct + frontend IPC
mirror) and `show_loading` was `spawn`ed-async while `hide_loading` is a
sync destroy, so a create can land after its paired destroy.

---

## 1. What paints the "wakeup" animation

Main orb window (`main`, 200×200, `index.html`).

- `frontend/src/avatar/Avatar.tsx:1-22` — Lottie `wakeup.json`,
  `SEG_LOADING=[171,260]`, `SEG_SMILE_ARRIVE=[261,316]`,
  `FRAME_HOLD_SMILE=300`.
- `frontend/src/avatar/Avatar.tsx:34-48` `resolveAvatarAnim()`:
  - `listening` → `{segment: SEG_LOADING, loop: false, speed: 1.5, mode: wake-loading}`
  - `thinking | speaking` → `{segment: SEG_LOADING, loop: true, speed: 1.5/1.2, mode: loading-loop}`
  - `idle` → `{segment: SEG_SMILE_ARRIVE, loop: false, mode: idle-smile}`
- `frontend/src/avatar/Avatar.tsx:71-83,105-118` — `applyState()` +
  `onComplete`: `wake-loading → wake-smile → holding(goToAndStop(300))`.
  Guard at `:73-76` skips restart if mode unchanged (`thinking→speaking`
  share `loading-loop`, only speed changes).
- `frontend/src/avatar/Avatar.tsx:137-176` — `useEffect[state]` applies the
  segment; `useEffect[visible]` does `play()` / delayed `pause(500ms)`
  (stays alive during slide-down); CSS fallback orb if Lottie not loaded.
- `frontend/src/styles.css:50-76` — `.avatar-wrap--listening` (scale 1.05),
  `--thinking` (`pulse-think` 1s), `--speaking` (`pulse-speak` 0.5s).
- `frontend/src/styles.css:78-96` — CSS fallback orb (radial gradient +
  glow; `.orb--listening/.orb--thinking/.orb--speaking` animations).
- `frontend/src/styles.css:35-47` — window slide: `#app.app--visible`
  vs `#app.app--hidden` (translateY + opacity + pointer-events).
- `frontend/src/App.tsx:56-75` — `visible → invoke(show_overlay)`,
  `!visible → 600ms → invoke(hide_overlay)`.
- `frontend/src/App.tsx:86-98` — separate loading window:
  `loadingVisible → invoke(show|hide_loading_indicator)`.
- `frontend/src/store/assistant.ts:10,47-89` — states
  `idle|listening|thinking|speaking`, `visible`, `loadingVisible` (funneled
  through `store/loadingMachine.ts` dwell/failsafe). `:113-120` canonical
  `transition()` map: **`speaking → idle` only via `done`**.
- `frontend/src/store/loadingMachine.ts:16-19,36-71` — `MIN_DWELL` 800ms,
  `FAILSAFE` 120s, `loadingShow/Hide/Settle`.

Entry paths (`visible=true` + `listening`):

| File:line | Path |
|---|---|
| `frontend/src/main.tsx:167-168` | `__NEXUS_WAKE__` → `wakeWithGreeting()` → `startListening()` |
| `frontend/src/main.tsx:190-193,230-234` | wake entry (Rust calls via `eval`, not Tauri events) |
| `frontend/src/main.tsx:209-218` | first-run greeting (`speaking`, then reset after speak) |
| `frontend/src/main.tsx:269-298` | Tier-3 `command-detected` fast path |
| `frontend/src/audio/recorder.ts:439` | legacy VAD path |
| `frontend/src/audio/vad.ts:17` | `onSpeechStart → listening` |

---

## 2. Every event that drives the orb (new channel)

`frontend/src/net/orchestrator.ts:117` listens on `orchestrator:event`:

| Event | Handler lines | What it does |
|---|---|---|
| `state` | `:124-129` | `store.setState(ev.state)` |
| `loading` | `:131-145` | `setLoadingVisible`; if true, orb hides after 600ms |
| `ack` | `:147-170` | speaks "On it sir" (unless local ack given), orb hides after 1500ms |
| `result` | `:197-206` | `setLoadingVisible(false)`, `setVisible(true)`, `speaking`, `speak(text)` — **no `onEnd`, no done signal** |
| `done` | `:244-252` | `setLoadingVisible(false)`, `setVisible(true)`, `reset()` after 550ms — **the ONLY reset path for this flow** |
| `error` | `:218-233` (approx) | speaks error, reset after 3000ms |
| `confirm` | `:235-251` (approx) | speaks prompt, waits (correct — user must answer) |
| `conflict_report` | `:253-288` (approx) | speaks + sidebar panel |

`signalOrchestratorDone()` (`net/orchestrator.ts:482`, invokes Rust
`orchestrator_done`) existed with **zero call-sites** — dead code.

Legacy channel (still active): `assistant:server` in
`frontend/src/net/wsBridge.ts:109-137,420-616`. Note the asymmetry: legacy
`wsBridge result` (`:553-590`) speaks **with** `onEnd → reset +
setVisible(false)` — it terminates correctly. Only the new orchestrator
`WorkerBackend Ok` path orphans.

Hide paths that DO exist: `done → reset()`; listening 8s auto-hide
(`App.tsx:36-49`, listening-only — **no backup for `speaking`**);
`recorder.ts` local flows (`await speak(); setVisible(false); reset`);
barge-in/cancel (`main.tsx:130-155,166,396-412`).

---

## 3. Root cause H1 (primary): the missing `done` handshake

Rust side (`src-tauri/src/orchestrator.rs:695-796`, `Subsystem::WorkerBackend`):

```
:697-704  emit Ack{text}                  ("On it sir")
:707-714  emit Loading{visible:true} + show_loading()
:717-724  dispatch_to_worker(...).await   (120s timeout, 9Router skipped for analyse)
:727-734  emit Loading{visible:false} + hide_loading()
:745-753  Ok → emit Result{text, analysis, dialog_state}   // ONLY Result
:754-758  comment: "`done` is emitted by the frontend after TTS finishes …
           emitting done here would cause stopTts() to cancel the response"
:786-791  Err → emit Error + emit Done     // error path terminates fine
```

Contrast: `GitHub` (`:1092-1097`), `Mcp`/`CommandCenter` arms **do** emit
`Done` on success. `WorkerBackend Ok` is the only success path that does
not — by design (TTS would be cut), but the frontend half of the contract
was never implemented.

Result: long response → multi-chunk `speak()` holds `state=speaking`;
audio ends; nothing transitions to `idle`; Lottie stays in `loading-loop`
(`Avatar.tsx:44-46`); `#app` stays `app--visible`. This matches the PR #68
log exactly: 361152 PCM samples (~15s) + a second chunk, `playback
completed` at 16:24:18, no state change after.

Why short replies *seem* fine: single-chunk speech ends quickly and the
user usually speaks again (barge-in/new-wake resets state), masking the
orphan. Long replies leave the orb visibly wedged with no further input.

---

## 4. Contributing cause H2: chunked-TTS completion races

`frontend/src/audio/ttsPlayer.ts:168-217` `speak()`:

- Starts with `stopTts()` (bumps `ttsGeneration`, emits `tts-ended`) —
  when `result` arrives while the ack ("On it sir") still plays, ack audio
  is killed but the ack path already scheduled `setVisible(false)` at
  1500ms, racing `result`'s `setVisible(true)`.
- Sentence-streamed `splitForSpeech()` (`:224-242`, `<150` chars =
  single chunk): `for chunk: await playKokoro(chunk, …, last ? onEnd :
  undefined)`. Per-chunk `tts-ended` fires N times per logical utterance.
- Barge-in abort (`:210-213`) silently `return`s — no `onEnd`, no
  hide/reset (owned by the cancel flow, but any gap leaves `speaking`).
- `waitForTtsIdle` pollers (`recorder.ts:542-563`, 10s timeout) can
  resolve in the inter-chunk `rustTtsPlaying=false` gap.

Long WorkerBackend responses (>150 chars → multi-chunk) hit this every
time; short replies do not. H2 alone cannot wedge the orb (H1 is the
blocker), but it explains flicker/overlap on long replies.

---

## 5. Secondary bug: loading-spinner re-creation race

Window inventory (`dyn_windows.rs` + `tauri.conf.json:12-13` has
`"windows": []` — everything but the orb is on-demand):

| Label | URL | Size | Defined |
|---|---|---|---|
| `main` (orb) | `index.html` | 200×200 | `dyn_windows.rs:35-43` |
| `loading-indicator` | `loading.html` | 80×80 | `dyn_windows.rs:50-58` |
| `pr-list-sidebar` | `pr-list.html` | 500×1000 | `dyn_windows.rs:106-114` |
| `sidebar` (response) | `sidebar.html` | 400×1000 | `dyn_windows.rs:78-86` |
| `architect-sidebar` | `architect.html` | 900×1000 | `dyn_windows.rs:91-99` |

Rust `show_loading` call sites: `orchestrator.rs:714` (WorkerBackend),
`:815` (Architect — **no paired hide in Rust**), `:854` (GitHub),
`:1124` (Mcp), `:1879` (CommandCenter), `:2219` (mcp_confirm).
Rust hide sites: `:734` (WorkerBackend common), `:767` (Err double),
`:885/:906/:944` (GitHub), `:1135` (Mcp), `:1897` (CC), `:2263`
(confirm), plus `commands.rs:1424` IPC.

Frontend mirror: `App.tsx:86-98` (`loadingVisible → show|hide IPC`),
writers in `net/orchestrator.ts:136/181/212/221/240/258`,
`net/wsBridge.ts:457/601/607/219`, `main.tsx:166/409`,
`recorder.ts` (14 rollback/error branches). Store gating:
`assistant.ts:62-89` + `loadingMachine.ts:36-71` (800ms min dwell —
delays the hide IPC, widening the race; 120s failsafe).

The race (pre-fix): `show_loading` was `spawn`ed-async while
`hide_loading` is a sync destroy. Fast Worker reply → `:734` destroys
nothing → spawned task creates + `show()`s the window with no hide left
in flight → 80×80 spinner stuck top-right. Same inversion possible
between the frontend show/hide IPCs.

**Fix applied:** `show_loading` runs inline
(`orchestrator.rs:2394-2397` + body) — create always completes before
dispatch starts, so the paired destroy cannot land early. The frontend
mirror is now ordered purely by event sequence (show from `loading`,
hide from `result`), both idempotent.

---

## 6. "Ghost mode" verdict: it does not exist

- **Frontend (`frontend/src`, whole `frontend/`): zero hits** for
  `ghost`/`ghostwriter`/`ghost-mode`/`ghost_mode`/`Ghost` in
  `*.{ts,tsx,css}` — no class, no setting, no string in any window, store,
  style, or theme file.
- **Backend (`src-tauri/src`): "ghost" means exactly two things:**
  1. Ghostwriter dictation mode — `ghostwriter.rs:3,123-126` (exit
     phrases `close/exit/stop ghostwriter|ghost mode`),
     `intent_parser.rs:121-123,183,284,2294-2323,5218-5246`
     (`ParsedIntent::EnterGhostwriter`), `orchestrator.rs:527,551-552`,
     `lib.rs:64`.
  2. A code comment — `live/commands/window.rs:5,12` ("ghost-hands"
     `AttachThreadInput` trick reference).

Nothing the user applied can cause the stuck animation. The cause is §3.

---

## 7. Fixes applied (2026-09-18) + verification

1. `net/orchestrator.ts:111` `finishSpokenResult(spokenFor)` — TTS
   `onEnd` (and TTS-failure catch) → request-id-guarded reset +
   `orchestrator_done`. Stale `onEnd` (barge-in moved on / cancelled)
   returns false and touches nothing. Wired in the `result` handler
   (`:197+`).
2. `App.tsx:51-85` 60s speaking failsafe — silent 60s in `speaking`
   (observed via exported `isRustTtsPlaying()`, `ttsPlayer.ts:30`)
   forces the `done`-equivalent reset; still-playing replies re-arm.
3. `orchestrator.rs:2394+` inline `show_loading` (spawn removed).
4. Tests: `frontend/src/net/orchestrator.test.ts` (3 tests: current-turn
   resets + signals done; stale-after-barge-in untouched;
   stale-after-cancel untouched). Full frontend 14/14, `tsc` clean,
   Rust `cargo check` clean, orchestrator 43/43 serial.
5. Known flake (pre-existing, unrelated): `test_install_and_cancel`
   fails under parallel threads (shared `ACTIVE_REQUEST` global; the
   test itself notes parallel dependence). Passes isolated ×2 and 43/43
   with `--test-threads=1`.

**Live verification still needed (requires running app):** trigger a long
reply (Analyse PR), confirm the orb returns to idle-smile after audio
ends, and confirm no top-right spinner survives fast replies.
