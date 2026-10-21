# Feature 75: Ghost-Session UX, Animation Timing & STT Gating — Plan

Evidence: `docs/research/ghost-session-ux-and-stt-gating-2026-09-29.md`
(8 reported symptoms, all root-caused, no code changed yet).

## Contract

- Ghost session = orb stays visible with waves for the WHOLE session.
  Any turn end that hides it is a bug, no exceptions.
- No speech without a visible speaker: never hide while speaking; hide
  only after TTS ends.
- Every exit (Esc / Ctrl+Space / voice) ends in the VISIBLE normal-mode
  orb + one spoken line. Invisible exits are failures.
- Captures with no voice never reach Groq, let alone the user.

## Phases (each ships independently, tests ×2, live walkthrough)

### P-A — Ghost orb persistence
- New `endTurn()` helper (ghost-aware: relisten-before-reset, NO hide
  when `ghostActive`; identical to today otherwise). Migrate all 28
  `setVisible(false)` turn-end sites in `recorder.ts` to it.
- Ghost enter path asserts `visible=true` + main-window shown.
- Tests: ghost-active turn end keeps `visible=true`; normal turn end
  hides (existing behavior pinned). Live: full ghost command → orb
  never vanishes, waves persist between turns.

**Status (done, verified ×2):** implemented as exported `hideOrbIfIdle()`
in `recorder.ts` (single choke point, 27 turn-end sites converted) +
ghost guards on the 2 Tier-3 sites in `main.tsx` (first-run greeting
untouched — pre-session). New `recorder.test.ts` pins both directions
(hide outside ghost / persist inside ghost across repeated ends). Shared
`test-setup.ts` (`window` + `localStorage` stubs) unblocked recorder
imports in node env. Vitest 47/47 (45 + 2 new) identical across two
runs, `tsc` clean, `vite build` succeeds. Note: one self-inflicted
mid-flight bug (replaceAll caught the helper's own line → momentary
infinite recursion) caught and fixed before any test ran.

### P-B — Speaking visibility
- Hide timers (600/1500ms) replaced by hide-after-TTS instead of fixed
timers; guard: never hide while `state === "speaking"`; keep orb visible
through short acks.
- Tests: ack → speaking → TTS end → hide ordering. Live: ack zoom
fully visible, then clean hide.

**Status (done, verified ×2):** new exported `hideOrbAfterSpeech()` in
`net/orchestrator.ts` — hides only once the turn is fully done (idle),
re-arming every 1s while TTS still plays; never hides mid-ghost-session
or mid-new-turn (barge-in safe). All 3 fixed timers (loading 600ms,
ack ×1500ms) converted. New `hideOrbAfterSpeech` describe block (4
tests: idle-hide, speaking re-arm, ghost persistence, new-turn guard).
Vitest 51/51 (47 + 4 new) identical across two runs; errors limited to
concurrent-work `SettingsSidebarApp.tsx` (untouched, mid-edit at 23:17).
Next: P-C ghost exits.

### P-B — Speaking visibility
- Hide timers (600/1500ms) replaced by hide-after-`tts-ended`; guard:
  never hide while `state === "speaking"`.
- Tests: ack → speaking → TTS end → hide ordering. Live: ack zoom
  fully visible, then clean hide.

### P-C — Ghost exits (Esc + Ctrl+Space + voice)
- Ctrl+Space: first branch — ghost active → end session (Esc path) +
spoken "Ghost mode off, sir." TTS-stop behavior unchanged otherwise.
- Esc/voice exits: end in visible normal orb (smile hold) + spoken line.
Audit `abort_session` vs `ghost_exit` speech coverage first.
- Regression guard: exiting twice / exiting when idle = silent no-op
(no crash, no stray speech).
- Tests: session-state transitions + spoken lines pinned. Live: Esc,
Ctrl+Space, "exit ghost mode" each verified from an active session.

**Status (done, verified ×2):** `GHOST_EXIT_LINE` const shared by all
three exits (voice already spoke it; `abort_session` now speaks it too,
so Esc/takeover/API exits give feedback — previously silent); Ctrl+Space
gains a ghost-first branch (stop TTS + cancel turn + `ghost_abort`,
no duplicate speech); store `setGhostActive` decoupled from visibility
and the App `ghost:session` listener sets it explicitly (enter shows,
exit lands on the visible idle orb — Esc no longer ends invisible).
Double-exit stays silent (`abort_session` early-returns on Idle).
Rust serial 718+2+10+7, frontend 53/53 (51 + 2 store tests), identical
across two runs; tsc errors confined to concurrent-work
`SettingsSidebarApp.tsx` (untouched). Next: P-D STT gating.

### P-D — STT gating (ghost hot-mic + everywhere)
- n× repetition detector (any k≥2 full-phrase repeats, not just 2×
half-splits) — kills the "I'm gonna say that ×3" class.
- Latin-gibberish dictionary gate (<30% common-English tokens → retry,
never chat) — kills the "Tentang Otor" class without touching real
short commands (matrix-tested against the intent catalog).
- Groq verbose `no_speech_prob` veto on command captures (not just the
verifier path).
- Verify the 0.005 RMS pre-gate sits before the Groq call on the hot-mic
path.
- Tests: corpus of the exact logged hallucinations + real-command
negatives (must NOT filter). Live: TV-on soak — zero spoken nonsense.

**Status (done, verified ×2):** n× repetition guard (blocks ≥2 words,
safety-critical repeats `stop/cancel/exit/quit/no` always pass so drill
cancel and voice decline can never be swallowed) + Latin dictionary gate
(~110-word command-aware vocab, ≥3 words, <0.30 ratio) in
`apply_hallucination_filter`. Logged corpus passes: 3× loops, foreign
background, 2× loops filter; 12 real commands (tabs, ghost, WhatsApp,
questions, PR analysis) pass. Deliberately NOT wired: verbose
`no_speech_prob` veto — measured dormant (0.000 on every segment with
whisper-large-v3-turbo per `verify_confidence` docs); wiring it would
add latency for zero effect (documented in code). RMS 0.005 pre-gate
verified already in place on both transcribe paths. Rust serial
721+2+10+7 twice, 0 failed, 0 warnings. Next: P-E loading timing.

### P-E — Loading/thinking timing
- Interim endpoint feedback (speech-end → transcript-arrival gap):
thinking state at endpoint, not at Groq return.
- Persist the loading window across turns (hide, don't destroy).
- Pre-warm STT sidecar at boot-idle; measure NLU first-spawn cost and
pre-warm if >2s.
- Tests: timing assertions where deterministic; live: barge-in overlap
walkthrough (loading never lands mid-next-utterance).

**Status (done, verified ×2):** endpoint thinking emit in the STT
receiver (fires after the phantom-voice check, before the 2-4s Groq
call — typed `OrchestratorEvent::State`, no handshake breakage);
loading window now hides instead of destroying (saves the ~300-800ms
per-turn WebView2 create that landed mid-next-utterance; destroy kept
as hide-failure fallback). Pre-warm verified already wired
(`lazy_stt::spawn_prewarm` at boot+20s); NLU deliberately stays lazy
(pre-warming contradicts the documented 200MB-idle design — warming it
would cost 50-100MB always-on for a path deterministic parsing already
covers). Barge-in overlap already covered (turn start hides loading).
No deterministic unit tests possible (both changes need a window/event
loop) — verification is full-suite green ×2 + live walkthrough. Rust
serial 722+2+10+7 twice, 0 failed, 0 warnings.

## Acceptance walkthrough (maps 1:1 to the 8 reports)

1. Cold boot → first command latency noted, no stall.
2. Ack plays with orb zooming fully visible throughout.
3. Long query → thinking + loading appear before user can re-speak.
4. Ghost session → orb + waves persist across 3+ commands.
5. Esc mid-ghost → visible normal orb + "Ghost mode off, sir."
6. Ctrl+Space mid-ghost → same as 5.
7. TV-on ghost soak → captures gated, nothing spoken, no random actions.
8. N/A (answered: no retraining — evidence in research §8).

## Explicitly NOT in this plan

Wake-model retraining, visual redesign, new intents. P2 UI-director
(doc 74) absorbs P-A/P-B permanently; P3 absorbs the STT-director half
of P-D.
