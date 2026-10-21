# Ghost FIFO Queue ("Real Ghost Mode") + Hotkey/Esc Realignment — Implementation Record

**Date:** 2026-09-30 → 2026-10-01
**Plan:** `NEXUS-PLAN-GHOST-FIFO-2026-09-30-V2` (pasted by user; file not in repo — source of truth is this doc + research doc `05`)
**Research spec:** `docs/research/ghost-mode/05-fifo-voice-command-queue-and-hotkey-realignment-plan-2026-09-30.md`
**Status:** Phases 1–5 automated gates GREEN. Live drills (F1–F7, H1–H7) + release build = user-run acceptance, pending.
**Lane note:** Phase 3 edited `ghost.rs` + `orchestrator.rs` (Gemini lane). All changes are additive/bracketed; no existing behavior removed except the documented `drop-oldest → drop-newest` policy flip.

---

## 0. What was built (one paragraph per phase)

- **Phase 1 — Hotkey barge-listen + STT capture abort.** Ctrl+Space during TTS now stops speech, drains 150ms of DAC/room tail, and starts listening (previously returned idle). A second press while listening aborts the Rust cpal capture via a new `stop_stt_capture` command that reports `had_speech`, so the orb hides only when no voice was underway. A capture-session generation kills orphaned in-flight buffers at the receiver (no phantom `""` transcripts after cancel).
- **Phase 2 — Frontend Esc realignment (C3).** Escape closes the response sidebar (lightbox first, then window via the store's full dismiss path) and the settings sidebar. The settings window's old Ctrl+Space-to-close handler was replaced — Ctrl+Space is now uniformly "talk to NEXUS" (D3): with any window visible the hotkey closes windows AND wakes.
- **Phase 3 — Approach E core.** The mid-drill transcript queue became a typed FIFO (`QueuedCmd` + `SlotClass`): stop-words take a priority lane (preempt + purge, never queue), same-slot pendings supersede (AppOpen/Navigate only, D8), overflow drops newest (D2 flip), depth ≥2 speaks "Queued, sir." once per session (D5/D6), every dequeued step re-verifies grounding (Rule 3) and runs under a 15s watchdog (D7) with a 1000ms inter-step gap (D4). Previously unbracketed runners (`run_ghost_open`, `run_browser_search`, `run_browser_search_focus`) got RAII drill guards + drain passes.
- **Phase 4 — Cadence compression.** Echo-poll cap 3000ms → 1000ms; ghost turn-end beats 550ms → 250ms (normal mode untouched). `verify_grounding` shipped inside Phase 3.
- **Phase 5 — Gates.** 746/746 Rust lib + 19/19 integration tests green; tsc clean; 71/71 frontend vitest green; zero new clippy warnings.

---

## 1. Every file, every change, and WHY

### 1.1 `src-tauri/src/wakeword_oww.rs` (Phase 1)

| Change (approx. lines) | What | Why |
|---|---|---|
| `CAPTURE_SESSION_ID: AtomicU64` + `is_stale_session()` (~2531–2548) | Monotonic capture generation + pure predicate | Gap 3: a buffer handed to the receiver thread before a cancel would emit `stt:transcript: ""` after it, causing a phantom no-input retry. Tagging lets the receiver drop it silently. Pure fn = unit-testable without threads. |
| Channel `Vec<f32>` → `(u64, Vec<f32>)` (decl ~2529, creation ~3100) | Session tag travels with the buffer | Required transport for the stale-drop; tag is captured at take-time in the audio callback, checked at receive-time. |
| `transcribe_and_emit(session, buffer)` | Signature + send of tuple | Plumbs the tag through the spawn boundary (audio callback → thread → channel). |
| Receiver stale check (~3135–3150) | `continue` (emit NOTHING) on stale session | Dropping — not emitting `""` — is the entire point: a cancelled turn must stay dead. Emitting `""` would feed the silent-miss counter. |
| `SttAbortResult { had_speech, elapsed_ms }` + `abort_stt_capture()` (~2680–2730) | Abort primitive: bump session FIRST, clear `STT_CAPTURING`, meter-settle `LEVEL_TX→0.0`, clear buffer + all counters incl. `STT_PAUSE_COUNT` | Order matters: session bump before flag clear closes the race where a buffer taken between the two would look live. `had_speech` (`voiced >= STT_MIN_VOICED_CHUNKS`) lets the frontend distinguish "nothing said → hide" from "mid-word → let finish". `elapsed_ms = total*80` (80ms chunks) for diagnostics. `STT_PAUSE_COUNT` reset added beyond plan for full state parity with `start_stt_capture`. Safe no-op when idle. |
| Mock-wake stub | Returns `{false, 0}` | `mock-wake` builds have no audio pipeline; without the stub the `stop_stt_capture` command wouldn't compile under that feature. |
| 4 tests (`test_abort_resets_capture_state`, `test_abort_had_speech_arms`, `test_stale_session_predicate`, `test_abort_invalidates_session`) | State reset, both `had_speech` arms, predicate truth table, generation bump | Prove the cancel contract without hardware: no residue, correct arm, receiver predicate correct. |
| Fix during build: `super::CAPTURE_SESSION_ID` | Audio-callback block uses `super::` scope | First compile failed (E0425); one-token fix. |

### 1.2 `src-tauri/src/commands.rs` (Phases 1 + 3)

| Change | What | Why |
|---|---|---|
| `stop_stt_capture` command (~2530) | Wraps `abort_stt_capture()` | IPC surface for the frontend second-press; mirrors `start_stt_capture` shape. |
| `NexusSettings` += `ghost_depth_ack: bool`, `ghost_step_timeout_ms: u64`, `ghost_turn_gap_ms: u64` (camelCase, `#[serde(default=…)])` | D4/D5/D6/D7 tunables | Struct round-trips through `save_settings`; serde defaults keep old settings.json files and old frontends working (fail-soft `unwrap_or_default` in `get_settings`). |
| `impl Default` += `true / 15000 / 1000` | Locked D-values as defaults | Single source of truth for defaults. |
| `read_ghost_depth_ack / _step_timeout_ms / _turn_gap_ms` | Loose JSON readers (camelCase + snake_case fallback, clamped) | Same fail-open pattern as `read_verify_wake`: ghost must never break on a corrupt/missing settings file. Timeout floored at 1000ms (below that the watchdog is meaningless); gap capped at 5000ms (above that the queue feels dead). |

### 1.3 `src-tauri/src/lib.rs` (Phase 1)

| Change | What | Why |
|---|---|---|
| `commands::stop_stt_capture` in `generate_handler!` (~988) | Registers the IPC command | Without this the frontend `invoke("stop_stt_capture")` fails at runtime (compile gives no warning — verified by grep that no other registration was needed). |

### 1.4 `src-tauri/src/hotkey.rs` (Phases 1 + 2)

| Change | What | Why |
|---|---|---|
| Speaking branch → barge-listen (~92–125) | After stop+cancel: `drop_followups()` (barge reconcile, Zylos step 4), spawn 150ms DAC drain, then the identical wake sequence as idle | Old code `return`ed after stopping TTS (dead mic). 150ms covers sound-card + room tail so it can't poison the new capture; the frontend's 300ms mute overlaps it (defense in depth, different layers). Reuses the wake block verbatim — no forked path (the file's bug history is forked paths). |
| Window branch → close + fall through to wake (~155–220, D3) | Removed the `else`: windows close, then the shared wake block always runs | Makes Ctrl+Space uniformly "talk to NEXUS". Close logic untouched (incl. `stage_hide` vs raw destroy for the blackout watchdog). |

### 1.5 `frontend/src/main.tsx` (Phase 1)

| Change | What | Why |
|---|---|---|
| `state === "listening"` guard → abort flow | Dynamic-import `invoke("stop_stt_capture")`; `had_speech==false` → `abortCapture()` + `reset()` + `setVisible(false)`; else let the turn finish | Previously a second press was swallowed ("already listening, ignoring") with no way to cancel — and no Rust abort existed at all. Dynamic import matches `triggerFollowupListen` (no top-level `invoke` import in this file, avoids cycle risk). `had_speech==true` path deliberately does nothing: killing mid-word corrupts the turn. |

### 1.6 `frontend/src/sidebar/SidebarApp.tsx` + `sidebarStore.ts` + `sidebarEsc.test.ts` (Phase 2)

| Change | What | Why |
|---|---|---|
| Esc effect → lightbox-first, else `hide()` | Hierarchy + full dismiss path | Plan said `invoke("hide_sidebar")` directly — WRONG path: skips `stopTts()` and the 400ms delayed-destroy guard, and risks the mount-caution that protects "Here is the analysis, sir". `hide()` reuses everything. |
| `resolveEscDismiss()` pure helper in store + 2 tests (new file `sidebarEsc.test.ts`) | `true→close-overlay`, `false→close-window` | No jsdom in repo (node env, no component-mount precedent) so effects are untestable without new deps; the helper locks the hierarchy contract in the only testable place. |

### 1.7 `frontend/src/settings-sidebar/SettingsSidebarApp.tsx` (Phases 2 + 3)

| Change | What | Why |
|---|---|---|
| Ctrl+Space-to-close → Esc-to-close | D3 coherence | After D3 the global hotkey wakes NEXUS even with settings focused; a local Ctrl+Space closer would fight it (close + wake simultaneously). Esc is window-scoped, zero OS risk. Drawer hierarchy impossible — `showCustomCreds` lives in `AuthTab`, not the window root (documented limitation; drawer keeps its toggle). |
| `Settings` interface + `DEFAULT_SETTINGS` += `ghostDepthAck/ghostStepTimeoutMs/ghostTurnGapMs` | Round-trip preservation | `save_settings` overwrites settings.json from the frontend object — without these keys a Save would drop the Rust-side values back to defaults. (Settings UI controls deferred — values editable via file; defaults are the locked D-values.) |

### 1.8 `src-tauri/src/ghost.rs` (Phase 3 — Gemini lane)

| Change | What | Why |
|---|---|---|
| `FOLLOWUP_QUEUE: VecDeque<String>` → `VecDeque<QueuedCmd>` | Typed entries | Strings can't carry slot/arrival metadata; supersede and id-tracing need the struct. |
| `SlotClass` (5 variants) + `classify_slot()` | Singleton-slot detection | D8: only AppOpen/Navigate supersede (two "new tab"s must mean two tabs; "open Chrome" after "open Brave" must not open both — F3). Conservative: unknown → GeneralAction (append, never supersede). Dictation checked FIRST — "start dictation" carries the "start " prefix but is a mode command (caught by test, fixed). |
| `GhostDrillGuard` (RAII + `Default`) | Bracket unbracketed runners | Check-1 correction: live drills already bracket internally; guard targets ONLY `run_ghost_open`/`run_browser_search`/`run_browser_search_focus`. Counter-based so balanced nesting is safe. |
| `enqueue_command()` → `(kept, depth)` | Supersede-or-append + drop-newest | D2 flip (oldest = primary intent; newest at cap is mic spam). Returns depth so the caller can ACK. |
| `dequeue_command()` | FIFO pop | Head-of-line order guarantee. |
| `DEPTH_ACK_SPOKEN` + `reset_depth_ack()` (called in `ghost_enter`) | Once-per-session ACK | "Queued, sir." at depth ≥2 without per-command chatter; reset per session so it can fire again next session. |
| `verify_grounding()` | Rule 3: browser slots require `get_active_browser_url().is_some()` | Plan named `is_browser_active()` — DOESN'T EXIST (Check-1 catch). A URL read proves a browser holds the foreground; F4 (queued click landing in Notepad) dies here with one spoken line. |
| 7 tests (cap-drop-newest, supersede, FIFO-append, slot table, drop-purge, ack-reset, guard-bracket) | Replaced `test_followup_queue_cap_drops_oldest` | Old test asserted the retired drop-oldest policy. New suite locks every rule. |

### 1.9 `src-tauri/src/orchestrator.rs` (Phase 3 — Gemini lane)

| Change | What | Why |
|---|---|---|
| Drill-overlap stop arm += `drop_followups()` | Rule 1 completion | Previously a stop aborted the step but left pendings queued — the F1 bug ("cancel" effectively queued behind its target). |
| Drill-overlap queue arm → `enqueue_command` + depth-ACK (gated by `read_ghost_depth_ack` + `DEPTH_ACK_SPOKEN`) | Rules 2+4 wiring | Silent queueing preserved (TTS mid-drill echoes into the hot mic); ACK once at depth 2. Cap rejections log-and-drop (deliberately silent — same echo reason; noted limitation). |
| `drain_ghost_followups` rewrite | Serial executor: stop-check → dequeue → DRAIN_CAP → verify_grounding → τ sleep → stop-recheck → boxed `process_transcript` under `tokio::time::timeout` | The heart of Approach E. τ from settings (D4); watchdog from settings (D7, "Skipping step, sir." — F6); grounding skip ("Target window lost, sir — skipping." — F4); gap-interrupting stop recheck (F1/F2 land even mid-gap). Box::pin preserved (recursion cycle). Build fix: `label` clone (borrow-after-move E0382). |
| Guards + clean-gated drains in `run_ghost_open`, `run_browser_search`, `run_browser_search_focus` | Bracket + drain | These runners never set the drill flag, so mid-action speech routed rival turns. Guard scope ends BEFORE drain — draining while `drill_running()` is true would re-queue forever (verified against ghost_drill/mouse ordering). Unclean (stop/abort) → `drop_followups()`, mirroring existing runners. `run_ghost_message` deliberately UNTOUCHED — `whatsapp_drill` brackets+drains internally; a second drain would be redundant (Check-1 correction #2). |

### 1.10 `frontend/src/net/ghostHotMic.ts` (Phase 4)

| Change | What | Why |
|---|---|---|
| `waitForAudioIdle` default 3000 → 1000ms | Echo-poll cap | The 3s poll stacked dead air onto every ghost turn; the 300ms DAC mute in `startListening` already absorbs the hardware tail. Sole caller is `maybeGhostRelisten`. Poll granularity 100ms unchanged. |

### 1.11 `frontend/src/net/orchestrator.ts` + `orchestrator.test.ts` (Phase 4)

| Change | What | Why |
|---|---|---|
| 3 turn-end beats → `ghostActive ? 250 : 550` (`finishSpokenResult`, `done` handler, error-path `endGhostTurn`) | Ghost-only cadence | Normal-mode timing byte-identical (existing 550-advance tests pass untouched). Ghost relisten starts 300ms sooner per turn — the measurable half of the 3–6s → ~1s gap closure (other half is the echo-poll cap). |
| +1 test: ghost beat fires at 250ms, session survives | Locks the branch | Asserts idle at 250 (not 249) + `ghostActive` preserved through reset. |

---

## 2. Plan deviations (Check-1 corrections — the plan was ~85% accurate)

| # | Plan claim | Reality found by audit | Resolution |
|---|---|---|---|
| 1 | Gap 1: no runners call `drill_begin()` | `ghost_drill.rs:75` + `mouse.rs:316` DO; only the 3 orchestrator-level runners lack it | Guards added to those 3 only |
| 2 | Gap 2: `drain_ghost_followups` called from nowhere | Called at `ghost_drill.rs:152` + `mouse.rs:457`, standalone-runs only (`keep_session` split is load-bearing) | No second drain in `run_ghost_message`; new drains only where none existed |
| 3 | `browser_url::is_browser_active()` | Doesn't exist | `get_active_browser_url().is_some()` (cross-platform fn) |
| 4 | main.tsx bare `invoke()` | No top-level import | Dynamic import (file's established pattern) |
| 5 | Sidebar Esc → direct `invoke("hide_sidebar")` | Skips TTS-stop + 400ms guard | Route through store `hide()` |
| 6 | Settings "attach Esc listener" | Had Ctrl+Space-to-close (D3-incoherent post-change) | Replaced, not added |
| 7 | `classify_slot("start dictation")` → Dictation (implied by test) | Prefix order gave AppOpen | Dictation check first (test caught it — the system working as designed) |

---

## 3. Evidence (gates)

| Gate | Result |
|---|---|
| Rust lib serial | **746/746** (740 baseline + 7 new − 1 replaced + 4 Phase-1… net verified by runner output) |
| Rust integration (`e2e_voice_fixture`, `offline_commands`, `test_user_commands`) | **19/19** pass; `phase2_integration` 8 ignored (pre-existing) |
| `cargo test --no-run` (all targets compile) | Clean — no stale refs to removed `queue_followup`/`take_followups` |
| tsc `--noEmit` | Clean (×4 runs across phases) |
| Frontend vitest | **71/71** (68 baseline + 2 Esc + 1 ghost-beat) |
| Clippy | **Zero new** warnings/errors in touched ranges (1 pre-existing error `google/mail.rs:241`, 97 pre-existing warnings, all outside edit ranges — verified by line-range sweep) |
| Grep-gate: no global `Escape` registration outside `ghost.rs` | Pass (`ghost.rs:445` only; `hotkey.rs:43` is the generic hotkey registrar, `browser_url.rs:331` is a key-send string) |
| `Subsystem::None` / `new_request_id` / `speak_line` sync-shape | Confirmed pre-existing; reused, not redefined |

---

## 4. Known limitations & deferred items (honest list)

1. **Cap-overflow is silent.** At depth 5, newest commands are logged-and-dropped with no voice feedback (TTS mid-drill echoes). Acceptable at cap 5; revisit if users hit it.
2. **"Close tab" classifies AppOpen.** The `close ` prefix rule (needed so "close chrome" supersedes "open chrome") also catches "close tab" — two rapid "close tab"s merge. Edge case, documented; fix = exact-phrase exception if ever reported.
3. **Silent-step 250ms fast path not implemented.** Drain sleeps the configured τ (1000ms) before every dequeued step; D4's 250ms silent optimization is deferred (nearly all ghost steps narrate; measurable follow-up).
4. **No settings UI for the 3 new tunables.** Interface + defaults round-trip correctly; sliders/toggles deferred.
5. **Release build + live drills are user steps.** `cargo build --release`, F1–F7, H1–H7 below.
6. **Ghost-Esc vs window-Esc overlap.** During a live ghost session with a sidebar focused, Esc fires BOTH the global session-abort and the window close. Judged coherent ("stop everything") — no guard added. Revisit if confusing in practice.

---

## 5. Live acceptance checklist (user-run — DO NOT SKIP)

### 5.1 Script test (the user's scenario)
Say in ghost mode, ~1s apart: "open Brave" → "create a new tab" → "search for almonds".
Assert: FIFO order · gaps ≤1.5s narrated · zero duplicates · "Queued, sir." heard once.

### 5.2 Failure drills F1–F7 (from plan §5.2)
| ID | Sequence | Must produce |
|---|---|---|
| F1 | "open Brave" → "actually, cancel" | `GHOST_CANCEL`, boundary abort, queue purged, "Stopped, sir. Ghost mode is still on." |
| F2 | long task → "stop" | Preempts at next boundary, no 60s wait |
| F3 | "open Brave" → "open Chrome" | Only Chrome opens (supersede) |
| F4 | "open Brave" → click Notepad → "new tab" | "Target window lost, sir — skipping.", no keystrokes in Notepad |
| F5 | 3 rapid commands | 1 executes, 2 queue, single "Queued, sir." |
| F6 | poisoned step | 15s watchdog → "Skipping step, sir." → queue continues |
| F7 | Esc with 3 queued | Session exits, all purged, zero ghost actions |

### 5.3 Hotkey matrix H1–H7 (from plan §5.3)
H1 ghost-live→ends session · H2 TTS→stops + listens (orb `listening`) · H3 listening+silent 2nd-press→orb hides · H4 listening+speech 2nd-press→turn completes · H5 idle→wakes · H6 Esc on sidebar/settings→closes, OS apps unaffected · H7 Ctrl+Space on open window→closes AND wakes.

### 5.4 Regression
Normal (non-ghost) mode on the existing e2e fixture — behavior byte-identical (all ghost branches are `ghostActive`/`drill_running`-gated; normal beats unchanged at 550ms by test).

---

## 6. Post-build incident: "localhost is not reachable" (my build error, fixed)

- **Symptom (user report):** after launching the freshly built `nexus.exe`, every window blank — localhost unreachable.
- **Root cause (agent error):** I ran plain `cargo build --release`, omitting `--features custom-protocol`. Per `AGENTS.md` + `Cargo.toml:155`, Tauri picks window URLs purely from that flag: off → `devUrl http://localhost:5173` (needs `npm run dev`); on → embedded `frontend/dist`. Binary forensics confirmed (`http://localhost:5173` baked in; nothing listens on 5173 — port scan showed only NLU 39218 + brain 39219 up, which is normal).
- **Compounding issue:** `frontend/dist` was stale (22:07, predating all Phase 1/2/4 frontend edits) — `cargo build` never rebuilds the frontend.
- **Fix applied:** `npm run build` (fresh dist, 6.8s) → `cargo build --release --features custom-protocol` (5m30s, 50.9 MB binary). Cargo accepts unknown features with a hard error — it didn't, so the flag is effective.
- **Rule going forward:** NEVER ship a bare `cargo build --release`. Always `pwsh ./scripts/build.ps1` or the exact two-step above. User's 30-second check: kill any Vite dev server, launch `nexus.exe` — windows must paint instantly; or use the CDP check in `AGENTS.md` (expect `tauri.localhost`, never `localhost:5173`).

## 7. File manifest (review checklist)
- [ ] `src-tauri/src/wakeword_oww.rs` — session tag, abort, stale-drop, 4 tests
- [ ] `src-tauri/src/commands.rs` — `stop_stt_capture`, 3 settings fields + defaults + readers
- [ ] `src-tauri/src/lib.rs` — handler registration (1 line)
- [ ] `src-tauri/src/hotkey.rs` — barge-listen branch, close-then-wake (D3)
- [ ] `src-tauri/src/ghost.rs` — slots, queue, guard, ack, grounding, 7 tests
- [ ] `src-tauri/src/orchestrator.rs` — overlap Rules 1/2/4, drain rewrite, 3× guard+drain
- [ ] `frontend/src/main.tsx` — second-press abort flow
- [ ] `frontend/src/App.tsx` — exit branch arms `hideOrbAfterSpeech(3000)` (§8 fix)
- [ ] `frontend/src/sidebar/SidebarApp.tsx` + `sidebarStore.ts` + `sidebarEsc.test.ts` — Esc hierarchy
- [ ] `frontend/src/settings-sidebar/SettingsSidebarApp.tsx` — Esc replaces Ctrl+Space; 3 settings keys
- [ ] `frontend/src/net/ghostHotMic.ts` — 1000ms echo cap
- [ ] `frontend/src/net/orchestrator.ts` + `orchestrator.test.ts` — ghost 250ms beats + test
