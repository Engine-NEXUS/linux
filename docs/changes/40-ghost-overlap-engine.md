# 40 — Ghost Phase 3: Overlap Engine (2026-09-25)

Act ∥ listen lanes with queued follow-ups: the user can keep speaking
while a drill drives, stop-words abort instantly, and everything else
runs in order when the main steps finish. Plan:
`docs/features/64-ghost-mode-plan.md` (Phase 3); leash: doc 37.

---

## 1. What the codebase actually did before (the finding)

Two facts shaped the design, both verified by reading (not assumed):

- **Voice "stop" never reached the drill.** "Stop" parses to a
  `cancel_action` NluResult → `route_intent` sends NluResults to
  `WorkerBackend`; bare "cancel" parses as Greeting upstream. The
  `live_cancel` Tauri command (which sets the ghost stop flag) had
  **zero callers** — no frontend code invokes any `live_*` command, and
  the voice pipeline never routed to them. The stop path existed but was
  unreachable by voice.
- **Mid-drill speech raced the drill.** Drills run as plain Tauri
  commands (no `ACTIVE_REQUEST`), while STT capture is an independent
  thread — so a second transcript processed *concurrently* through the
  full pipeline, fighting the drill for mouse/keyboard with no
  arbitration.

## 2. What shipped

**Run-queue (`ghost.rs`):** `GHOST_BUSY` drill-depth counter (bare test
sessions don't set it — they never swallow commands), `FOLLOWUP_QUEUE`
(cap 5, drop-oldest), `DRAIN_CAP` 10, normalized stop-phrase list
(`stop/stop it/cancel/halt/abort/never mind/forget it/hold on`).

**Intercept (`process_transcript`, after the ghostwriter block, before
command-center):** while a drill runs —
- stop-phrase (raw match — required, since bare "cancel" parses as
  Greeting) OR parsed `cancel_action` NluResult → `request_stop` +
  session abort + live-state reset + spoken "Stopped, sir." via a fresh
  LocalCommand request. Never routed, never queued.
- anything else → queued **silently** (no TTS — speech over the user's
  ongoing speech is echo; returns a `None`-subsystem ProcessResult the
  frontend ignores beyond bookkeeping).

**Drain (`drain_ghost_followups`, orchestrator):** called by both drill
runners after main steps + session exit, and ONLY on a clean run (steps
Ok + no stop + session still Active — resuming behind a takeover is
rejected). Items re-enter the normal pipeline in order; loop continues
while non-empty (nested drills queue deeper) up to DRAIN_CAP, then drops
with a log. Fresh stop-words during drain still abort via the same
intercept when a nested drill is up.

**Runner protocol change:** `drill_begin/end` brackets in both drills;
on failure/abort the queue is dropped with a log (stale context).
Mouse drill additionally **skips glide-home after stop/takeover** —
a real flaw found while wiring (it used to glide into the user's hands).

**Frontend confirm-flow interplay (documented, not changed):** when a
GitHub/MCP confirm card is pending, the frontend consumes stop-words
itself ("no/cancel/stop" → abort card). Drills never open confirm cards,
so the Rust intercept owns the drill case; the two layers don't fight
(the frontend path only triggers with a pending card).

## 3. Verification (twice)

Rust full lib **540/540 serial** (new: stop-phrase normalization incl.
"Stop"/"NEVER MIND" and near-miss rejections, queue cap + drop-oldest
+ drain semantics, busy-counter nesting + saturation). `cargo check`
clean, zero new warnings. No frontend files changed. Live drill
procedure for the built app: start `live_ghost_whatsapp`, speak a second
command mid-type ("open calculator") → main draft completes, follow-up
executes, both narrated; speak "stop" mid-drill → immediate "Stopped,
sir." with no Worker round-trip; grab mid-drill with a queued item →
queue dropped, never resumes behind the user.
