# Ghost Mode Voice Fixes — Issue Timeline & Root Causes (2026-09-25)

Every live defect hit while bringing Ghost Mode under voice control,
with the evidence that identified each, the fix, and the guard that
keeps it fixed. Companion: architecture reference (same folder);
implementation details per doc 37–46.

---

## 1. "Ghost mode" opened the wrong room (entry collision)

**Symptom** (log `17:07:35`): spoken "Ghost mode." →
`EnterGhostwriter { contact: None }` → dictation sidebar opened; the
card echoed a later mishearing ("Vost Mode.") as ink.

**Root cause:** the pre-existing Ghostwriter dictation entry owned
*every* "ghost mode" phrase; cursor-control Ghost Mode had no voice
entry at all (`ghost_enter` was command-only). Saying the words could
only ever reach the wrong session.

**Fix** (doc 42): new `EnterGhostControl` intent + `run_ghost_control_enter`
(ring + narration, no sidebar card). Bare-mode phrases stripped from the
dictation entry; dictation keeps writer-word triggers. Both matchers
strict (trailing words enter neither room).

**Guard:** `test_ghost_mode_means_cursor_control_not_dictation` — 10
phrases pinned to control; sentences *about* ghost mode pinned to
neither room.

## 2. Orb never showed waves (missing initial event)

**Symptom**: entry succeeded (session ACTIVE log) but the orb stayed a
smile; user waited without touching the mouse.

**Root cause** (confirmed by reading then by log): the orb's
`ghostActive` feeds only from `ghost:ring` events, and the backend
emitted those solely on cursor-*change*. A still cursor → zero events
→ `ghostActive` never flipped. The transition code was correct and
un-exercised.

**Fix** (doc 44): `ghost_enter` emits an initial
`ghost:ring {x, y, visible: true}` at the live cursor before anything
else. Also added the `stage-shell-v1` heartbeat client marker so "is
this UI even in this binary?" is answerable from logs.

## 3. Mic not hot — back to wake word forever (pipeline gap)

**Symptom** (log `18:26:48→58`): entry speech completes; zero BATON
capture lines for 50s; every follow-up needs a wake word.

**Root cause**: ghost session state and mic state were never
connected. Rust cpal listens always but only for the wake word;
command capture starts per wake/hotkey and stops per turn; the turn
ends with frontend `reset()` → idle. **Second layer** (doc 45): even
with the hot-mic loop wired, `triggerFollowupListen` never started
Rust *capture* — the orb showed "listening" while Rust captured
nothing (the confirm window did this; the shared path didn't).

**Fix**: guard chain in `triggerFollowupListen` (invoke
`start_stt_capture`) + `maybeGhostRelisten` at both turn ends
(echo-wait, live meeting check, session re-check) + empty-transcript
anti-nag (3 silent re-listens, then 1 nag + park). Legacy empty paths
untouched.

**Guard**: `ghostHotMic.test.ts` (relisten matrix, streak counting,
meeting skip, no-IPC-outside-ghost) + live BATON line asserted in the
drill script.

## 4. Transcripts dying silently inside the frontend (invisible failures)

**Symptom** (log `20:44/20:45`): capture fixed, transcripts produced
("Grab the mouse.", "open WatsApp.") — then nothing: no parse, no
orchestrator, no TTS, in-session only.

**Root cause**: an exception/crash upstream of any invoke, invisible —
Rust logs only see Rust. Plus two real causes found only because of
routing: mid-drill speech raced drills unarbitrated, and voice "stop"
never reached drills (`cancel_action` routed to Worker;
`live_cancel` had zero callers).

**Fix** (docs 40/46): stop-word intercept in `process_transcript`
(raw phrases required — bare "cancel" parses as Greeting; `cancel_action`
NluResult matched too), drill-depth flag, follow-up queue with
drop-oldest, and the temporary `debug_trace` p0–p4 taps for anything
that still dies silently.

## 5. Session starve-as-takeover (seen twice in your logs)

**Symptom**: `session yielded (human-takeover)` roughly 15–25s after
entry with no deliberate grab.

**Read**: the leash working *as designed* — you moved the mouse to
investigate while the mic wasn't reopening. Not a bug (unexplained
motion must abort), but it masked the real issue for a round. Kept as
behavior; the hot-mic fix removes the starvation that invited the
grab.

## 6. The covering window + scrollbars (stage chrome)

**Symptom**: fullscreen stage shows with h+v scrollbars over the
desktop.

**Root cause**: `stage.html` shipped zero CSS — browser-default body
margin + visible overflow on a fullscreen window guarantees both.
Transparency/click-through were always correct.

**Fix** (doc 45 §1): 5-line style block (margin/overflow/transparent/
viewport lock). Confirmed by `nexus trace-ghost` check 7.

## 7. False TRIGGER banner (console display, your call)

The `run.ps1` banner restated the probability with a different number
(93.5% vs 99.5%) and bloated every wake to 5 lines. Replaced with one
truthful line (doc 46 §1). Display-only; no engine behavior changed.

## 8. Send-safety regression risk (compile-time-forced)

Routing messages through the drill introduced an async cycle (drain →
drill → process_transcript, E0733) and non-Send futures (blocking
enigo internals + runtime-marked ghost handles). Fixed structurally:
boxed futures at both cycle edges, blocking drill body on
`spawn_blocking`, and runtime erasure at exactly one documented site
(`ghost_wry::g_wry`/`g_wry_ref`). Every fix was found by the compiler,
not by review luck — the strongest guard the codebase has.

## 9. "Prem on WhatsApp." starves — 23:31 log walk (2026-09-25 late)

**Symptom**: entry ("Ghost mode.") fine; the *entry speech* then plays
~3s; the hot-mic reopen fires at 23:31:43, captures "Prem on WhatsApp."
at :48… and then **the session yields (human-takeover) at :50** — before
any routing happens. Two visible losses: no waves, no WhatsApp.

**Root causes (three, compounding):**

1. **Waveform race (the "no smile→waves"):** the takeover yield at
   23:31:50 emits `ghost:ring {visible:false}` AFTER the initial emit
   that should flip `ghostActive=true` — and the frontend store's
   `reset()` was also clearing `ghostActive` (handled in §9b). The orb
   could flip AND un-flip before React ever rendered waves: a real
   race between the leash and the UI, triggered by the mouse grab you
   made while waiting.

2. **Hot-mic ordering (frontend)**: `finishSpokenResult` called
   `reset()` BEFORE `maybeGhostRelisten()` — `reset()` clears
   `ghostActive`, which IS the relisten gate. Order inverted = gate
   always false = loop dead. The starve-then-grab pattern this produced
   matched #5 exactly.

3. **"Prem on WhatsApp." as chat-open**: parses as
   `WhatsappChat { contact: "Prem" }` (the `whatsapp ` prefix pattern
   strips nothing — no "saying", no " on wa" suffix in that phrasing),
   routed to `run_ghost_message` → drill with empty message → the
   drill REFUSED "empty message" → the drill never even tried to
   search, so nothing visibly executed. The chat-open half wasn't
   implemented in the drill (orchestrator passed `"", but the drill
   guarded it).

**Fixes this round:**

- Drill: `chat_open` mode — empty message is a first-class chat-open
  (open + search contact + **no typing**), narrated `"Prem is open,
  sir. What do you want to send?"`. Also produced the orb's spoken
  confirmation with the contact name (formerly a hardcoded
  "Draft ready…" regardless).
- Frontend ordering: `finishSpokenResult`/`done` now call
  `maybeGhostRelisten()` BEFORE `reset()` (the gate reads ghostActive;
  reset clears it). Reset also explicitly drops `ghostActive` so every
  wake/expiry resets it to a known state.
- Entry speech: "Ghost mode on, sir." → **"Ghost mode initialized,
  sir."** (user's wording).

**Guards added**

- The takeover-vs-starvation chain: the drill now opens the app with
  zero user-mouse dependency (registry ShellExecuteW path — no cursor
  needed to click Start), so during the 1s launch wait the user has no
  reason to touch the mouse; plus the takeover suppression window
  covers the registry path even without motion (`note_expected` before
  each step).
- `reset()` now owns `ghostActive` (inverted from "preserved") — with
  hot-mic ordering fixed, a clean session-under-ghost turn no longer
  clears the flag mid-loop; a true reset (new wake, cancel) does.

---

## 10. Mouse use kills the session (takeover armed 100% of session — user directive)

**Symptom** (23:31:50 + user report): "even if I am using the cursor it
shouldn't off" — any mouse movement during idle-in-session (or during
keyboard-only drills, where the AI never touches the cursor) yielded
the session + "You have it, sir."

**Root cause (line-level):** `decide_takeover`'s between-steps arm
(`(None, Some(prev)) => prev != actual`) judged ANY motion a takeover
whenever no commanded target existed — which is the state during
keyboard-only drills and idle listening. The detector was designed for
"grab mid-AI-glide" but armed the entire session. The ring lied the
same way (rode the USER's cursor during idle, displaying "AI
controlling"), and the waves lifecycle was tangled (reset cleared
ghostActive per turn; ring visible:false raced the initial emit).

**Fix (re-scope, doc 47):**
- `decide_takeover`: takeover now REQUIRES a commanded target
  (in-flight window suppresses; settled deviation = task abort). No
  commanded target → motion is never a takeover — the mouse is free.
- **Task-abort vs session-end split**: a mid-glide grab aborts the
  TASK (`request_stop` + ring off + "Task stopped, sir — you have the
  mouse. Ghost mode is still on.") while the session stays live, Esc
  stays armed, hot-mic keeps flowing. Explicit exits ("exit/close/turn
  off ghost mode" via the new `ExitGhostControl` intent, Esc, stage
  hide/kill) end the session.
- **`ghost:session` event**: session-scoped orb signal (waves +
  ghostActive) — fires on enter/exit/stand-down only, never per turn.
  `ghost:ring` is now positional-only. `reset()` no longer clears
  ghostActive (session-scoped; the earlier per-turn clear is reverted).
- Idle-in-session stop-words inform "Nothing running, sir." instead of
  routing to the Worker; drill stop-words abort the task only.

**Guards:** re-scoped takeover matrix (idle motion never yields ×3,
settled deviation = task abort), session-event wiring test, drill
exit-skips-on-abort. The old `test_takeover_commanded_motion_suppressed`
caught the first re-scope draft removing the in-flight suppression
mid-flight deviations are the AI's own glide — restored; tests remain
the guard that catches re-scope mistakes.

1. **Data autopsy before data edits** — "list_prs" had 151 rows; the
   regex was the bug. Repeated for orbs, taps, banners.
2. **State machines need a handshake on both ends.** Rust withholds
   `Done`; someone must close it. Every "stuck" symptom so far has
   been one-sided bookkeeping.
3. **Nothing may open or drive without a spoken narration and an
   exit.** Every runner: announce → guard → act → exit → outcome.
4. **Live logs beat theory.** Every fix above came from a timestamp
   walk of a real session, not from unit tests alone.

---

## 11. Takeover detection deleted entirely (2026-09-27, user directive)

**Symptom** (00:15:37 live log): "open Watson" → 5s later "session
yielded (human-takeover) — user has the mouse". The session died with
no cursor grab — the detector's last arm (settled deviation under a
commanded target) misfired on a keyboard-only open flow. Note: the log
format predates the re-scope build (user was running an older binary),
but the directive is stronger than a fix: **remove takeover entirely**.

**Fix**: `decide_takeover` deleted along with the observe_cursor
judgment branch and `abort_task`. The user's mouse is ALWAYS free —
motion is never judged, tasks are never aborted by cursor movement,
the session never yields. Explicit exits only:
- **Esc = the cancel button** (armed at session start, "esc-panic")
- "exit/close/turn off ghost mode" voice (ExitGhostControl)
- stage hide/kill (stand_down)

Entry narration updated ("Esc cancels any time" — the old "grab the
mouse or tap Esc to take over" taught the wrong mental model).
`note_expected`/`note_idle` remain — ring ride-along only.

**Guards**: all decide_takeover matrix tests removed (nothing left to
pin — the function is gone); session cycle + stop-phrase tests
retained. Rust 642/642 serial ×2.

1. **Data autopsy before data edits** - "list_prs" had 151 rows; the
   regex was the bug. Repeated for orbs, taps, banners.
2. **State machines need a handshake on both ends.** Rust withholds
   Done; someone must close it. Every "stuck" symptom so far has
   been one-sided bookkeeping.
3. **Nothing may open or drive without a spoken narration and an
   exit.** Every runner: announce -> guard -> act -> exit -> outcome.
4. **Live logs beat theory.** Every fix above came from a timestamp
   walk of a real session, not from unit tests alone.

---

## Lessons (standing rules)

1. **Data autopsy before data edits** - "list_prs" had 151 rows; the
   regex was the bug. Repeated for orbs, taps, banners.
2. **State machines need a handshake on both ends.** Rust withholds
   Done; someone must close it. Every "stuck" symptom so far has
   been one-sided bookkeeping.
3. **Nothing may open or drive without a spoken narration and an
   exit.** Every runner: announce -> guard -> act -> exit -> outcome.
4. **Live logs beat theory.** Every fix above came from a timestamp
   walk of a real session, not from unit tests alone.
