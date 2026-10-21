# 44 — Ghost Voice Entry: 3 Live Issues and Fixes (2026-09-25)

Live session replay (user log 17:45:28–17:46:04): "ghost mode" entered
correctly (`EnterGhostControl`, session ACTIVE, stage created, entry
speech spoken) — then (1) the orb never morphed to waves, (2) the orb
fell back to idle and demanded the wake word again, (3) a mouse move at
17:45:44 correctly yielded the session ("You have it, sir."), which is
the leash working, not a bug. Waves styling (transparent bg, measured
palette) was already correct in code — it never rendered, so nothing
there needed changing.

---

## Issue 1 — orb never learns the session started (no waves)

**Cause (confirmed by reading, not guessing):** the orb's `ghostActive`
flag feeds *only* off `ghost:ring` events, and the backend emitted those
only on cursor-position *change* (plus abort/exit). `ghost_enter`
emitted nothing. The user said the words and waited without touching
the mouse — zero ring events fired, `ghostActive` stayed false, the
pinch effect never ran. The transition code was fine; it was never
invoked. (A stale frontend build missing the waves UI would produce the
identical symptom — see verification below.)

**Fix:** `ghost_enter` now emits an initial `ghost:ring {x, y,
visible: true}` at the live cursor position (via `stage::cursor_pos`,
now `pub(crate)`), then session arming proceeds as before. One event,
zero motion required.

## Issue 2 — mic not hot: back to wake-word after entry

**Cause (design gap, verified against the pipeline):** ghost session
state and microphone state were never connected. The cpal Rust stream
listens always — but only for the *wake word*; command capture
(`stt-capture`) starts per wake/hotkey and stops after each turn.
`run_ghost_control_enter` ends with `clear_active_request` → frontend
`reset()` → idle, full stop. The 16s silence in the log *is* the system
working as (previously) designed.

**Fix — ghost hot-mic loop** (new `frontend/src/net/ghostHotMic.ts`):
after every turn end (`finishSpokenResult` and the `done` handler, post
550ms reset), call `maybeGhostRelisten()`: no-op unless `ghostActive`;
skip in meeting mode (live `meeting_active` check, never cached);
`waitForTtsIdle` echo guard (never capture our own tail audio);
re-check `ghostActive`; then the existing `triggerFollowupListen()`
path (`__NEXUS_WAKE__` → `startListening`), which bypasses wake-word
suppression by construction. Pure decision + counter helpers are unit
tested; the async orchestrator stays thin.

**Empty-transcript anti-nag** (`recorder.ts processTranscript`, the Rust
capture path only): ghost turns with no speech re-listen silently up to
`GHOST_SILENT_CAP` (3) consecutive empties, then fall through to the
single existing nag + park. Without this, the loop would speak "Didn't
catch that" every ~8s forever. Any heard speech resets the streak.
Legacy/VAD empty paths untouched (out of scope, documented).

## Issue 3 — build identity ("is the UI even in this binary?")

**Cause:** separate Rust/frontend builds mean entry can work while waves
code is absent, with no log line distinguishing the cases. **Fix:**
`stage_heartbeat` now takes an optional `client` marker
(`"stage-shell-v1"`, sent by StageApp); Rust logs it on first/fresh
beat. The running binary's UI vintage is now answerable from logs in 5
seconds.

## Verification (twice)

- Rust: `cargo check` clean; ghost/stage suites green (entry emit is a
  fire-and-forget emit beside tested logic — no new failure modes).
- Frontend **33/33** (new `ghostHotMic.test.ts`: relisten matrix,
  streak counting/reset, meeting skip, IPC untouched outside ghost
  mode), `tsc` clean ×2.
- No dataset, model, Worker, or backend-pipeline changes.

## Live test script (for the running app)

1. Rebuild + `nexus start`. Say "ghost mode" → ring + speech + waves
   without touching the mouse. Check logs for the `stage-shell-v1`
   heartbeat line.
2. Speak a command with no wake word → executes → mic reopens by
   itself. Repeat twice.
3. Stay silent 3 turns → exactly one "Didn't catch that", then parked
   quiet (no nag loop).
4. Grab the mouse mid-session → instant yield as before (leash
   untouched by this change).

## Trace tooling (temporary)

`nexus trace-ghost [--log run.log]` (`scripts/trace_ghost.py`,
wired additively in `nexus.mjs` — no existing command logic touched,
marked TEMPORARY for deletion after the fix lands). Two halves:

- **Static (9 links):** greps the tree for every pipeline joint —
  initial ring emit, orb listener, store flag, hot-mic module + all
  three hooks, capture invoke, waves visual + CSS, scrollbar CSS,
  heartbeat marker — plus a dist-bundle check (`ghostActive` in built
  assets) proving the binary actually ships the UI. Exit 1 on any miss.
- **Log analysis:** replays a pasted console session stage by stage
  (wake → transcript → intent → session → stage → heartbeat → TTS-done
  → relisten → yield). Relisten only counts *after* speech finished
  (timestamp-ordered, so the entry capture can't false-positive it).
  Prints stage verdicts, e.g. "session opened but mic never reopened —
  the capture gap."

Compatibility with the updated `run.ps1` console: the tool writes to
its own stdout (never through the log tailer), and any future
`ghost-trace:` backend markers display by default — the run.ps1 filters
only suppress mic/pairing/silence chatter, nothing else.
