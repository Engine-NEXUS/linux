# 47 — Takeover Re-Scope: Mouse Use Never Ends the Session (2026-09-25)

User directive: "even if I am using the cursor it shouldn't off — only
when the command is related to the agent taking over the cursor should
it off." The takeover detector was armed 100% of the session; now it
only arms while the AI is actually driving. Plan: `docs/features/
64-ghost-mode-plan.md` §3 (re-scoped); timeline: `docs/research/
ghost-mode/03-...md` #10.

---

## 1. What the code proved (line-level)

`decide_takeover`'s between-steps arm (`(None, Some(prev)) => prev !=
actual`) judged ANY motion a takeover whenever no commanded target
existed — the state during keyboard-only drills (the WhatsApp example:
open/search/type never touch the cursor) and idle-in-session. The
23:31:50 yield fired while the session was merely listening. The ring
lied identically (rode the USER's cursor during idle while displaying
"AI controlling"), and the waves lifecycle was tangled (reset cleared
`ghostActive` per turn; ring `visible:false` raced the initial emit).

## 2. The re-scope (semantics)

**Takeover requires a commanded target. Everything else keeps the
session alive.**

| Situation | Before | After |
|---|---|---|
| Idle-in-session, user moves mouse | session yields | nothing — session stays, ring off |
| Keyboard-only drill, user uses mouse | session yields | nothing — no detection armed |
| Mouse-drill mid-glide, user grabs | session yields | **task abort** ("Task stopped, sir — you have the mouse. Ghost mode is still on."), session stays |
| Task completes | task ends, session stays | same |
| "stop" during drill | task + session abort | task abort only, session stays |
| "stop" during idle | routed to Worker (wrong) | "Nothing running, sir." |
| "exit/close/turn off ghost mode" | dictation-only exits | **`ExitGhostControl` intent ends the cursor session** |
| Esc / stage hide/kill | session end | session end (explicit exits) |

## 3. What shipped

**Rust (`ghost.rs`):**
- `decide_takeover` re-scoped: in-flight window suppresses (mid-flight
  deviations are the AI's own glide); settled deviation = task abort;
  **no commanded target → motion never yields** (between-steps arm
  deleted).
- **`abort_task`** (new): `request_stop` + suppress clear + ring off +
  "Task stopped, sir — you have the mouse. Ghost mode is still on."
  Esc stays armed; hot-mic keeps flowing.
- **`abort_session`** narrowed to explicit exits (Esc, exit-phrase,
  stage hide/kill): ring off + Esc release + **`ghost:session {active:
  false}`**.
- **`ghost:session` event** (new): session-scoped orb signal — fires on
  enter/exit/stand-down only, never per turn. `ghost:ring` is now
  positional-only (rides commanded glides via the change-detector,
  `ring_off` when idle).
- `observe_cursor`: takeover → `abort_task`; ring emits only while a
  commanded target exists (never rides the user's cursor).

**Parser (`intent_parser.rs`):** `ParsedIntent::ExitGhostControl` +
`parse_ghost_control_exit` (exit/close/turn off/stop/quit/leave ghost
mode, ghost off, stand down + STT soundalikes; strict exact match like
the entry) + label arm + LocalCommand routing.

**Orchestrator:** session-active block gains the exit branch
(ghost_exit + "Ghost mode off, sir.") and the idle stop branch
("Nothing running, sir." — never routed to the Worker); drill
stop-words now task-abort only (no `ghost_abort` — session stays).

**Frontend:** App listener reads `ghost:session` (waves +
`ghostActive`); `ghost:ring` listener is display-only; `reset()` no
longer clears `ghostActive` (session-scoped — the earlier per-turn
clear reverted; the relisten-before-reset ordering stays correct).

## 4. Verification (twice)

- Rust 549/549 serial + clean check. Re-scoped takeover matrix (idle
  motion never yields ×3, settled deviation = task abort, in-flight
  suppressed — the old suppression test CAUGHT my first draft removing
  it; tests remain the guard).
- Frontend tsc ×2 + 33/33 + production build clean; trace tool updated
  to the new wiring (check 2 = `ghost:session` listener), 12/12 ×2.
- Live script for the built app: "ghost mode" → waves → use the mouse
  freely (nothing happens, ring off) → "open whatsapp" → mid-type grab
  (task stops, session live, spoken "Task stopped… still on") → next
  command with no wake word → "exit ghost mode" (waves off).

## 5. Honest note

The takeover-during-glide behavior (task abort) is the ONE remaining
judgment — fighting the AI's glide must stop the motion. If you want
even that to be ignored (glide continues under your hand), that's a
one-line change to `decide_takeover` — say the word.
