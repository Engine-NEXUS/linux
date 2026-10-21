# Ghost Mode — Master Plan (Windows-only, 2026-09-25)

**Definition (locked):** a voice-entered session in which NEXUS operates
the user's REAL cursor and keyboard while the user keeps talking. A
glowing ring rides around the real cursor (never a second pointer) so
it is always visible who drives. Ends on completion, "stop", mouse
grab, Esc, or timeout. Visible control with a visible leash — the
opposite of stealth overlays.

**Voice entry (fixed 2026-09-25, doc 42):** "ghost mode" (+ "take the
mouse", "control my cursor", "ghost cursor", "take control") enters
cursor-control Ghost Mode. Bare-mode phrases were stripped from the
Ghostwriter dictation entry after a live collision; dictation keeps
writer-word triggers. Strict exact match.

**Orb waves (built 2026-09-25, doc 43):** on session start the smile
pinches flat (220ms) and grows a live 3-bar waveform in the Lottie's
own measured palette (blue/orange/yellow, tall-short-tall); reverses
on exit. Pure frontend, zero backend changes.

**Voice entry fixes (doc 44)** + **instant response & messaging
(doc 45/46)**: initial ring emit, hot-mic loop with anti-nag,
stage scrollbars, in-session app/message routing (desktop
WhatsApp drill — visible typing, confirm-gated send). Issue
timeline with root causes: `docs/research/ghost-mode/
03-irregularities-and-fixes-timeline.md`. Full symbol map and
Send-invariants: `ghost-mode-architecture-reference.md` (same folder).

## 1. Design rules (non-negotiable)

1. **Keyboard path first.** Win-search-open-type flows need zero
   grounding, zero pixels, millisecond latency. Mouse only where no key
   sequence exists (deletes ~70% of the grounding problem for
   launcher/chat flows).
2. **Same word, one entity — same motion, one driver.** Only one party
   moves the cursor at a time. Any unexplained motion = human hand =
   instant abort (see §3).
3. **Explicit entry, instant exit.** Per-task or session consent; abort
   is always one grab / one word away.
4. **Destructive clicks behind confirm; credential fields hard-refused**
   (existing safety.rs + gates apply unchanged).
5. **No auto-resume after takeover.** Takeover means "mine now" —
   resuming behind the user's back is how assistants cause disasters.

## 2. Example walkthrough (WhatsApp, the motivating drill)

1. "Open WhatsApp and message mom" → ghost session opens, ring appears:
   *"Taking the mouse, sir — grab it or tap Esc any time."*
2. `Win` → type "whatsapp" → `Enter` (keyboard-only, no grounding).
3. Focus chat search (`Ctrl+F` or Tab path); type contact (clipboard
   paste); `Enter`.
4. Type message (paste), `Enter` to send — **while the user keeps
   speaking** (act-lane and listen-lane overlapped; see Phase 3).
5. Glide back to pre-act position, *"Done, sir"*, session closes, ring dies.

## 3. Takeover protocol (deleted 2026-09-27, user directive)

**Takeover detection is DELETED. The mouse is ALWAYS free. Nothing
cursor-related ever ends a session or aborts a task.**

- **No takeover detection**: `decide_takeover`, the observe_cursor
  judgment, and `abort_task` are removed after three live misfires
  (00:15:37 yield on a keyboard-only "open" flow was the last straw).
  Motion is never judged. The drill keeps running under the user's
  hand.
- **Explicit exits end the session**: **Esc = the cancel button**
  (armed at session start, "esc-panic"), "exit/close/turn off ghost
  mode" (`ExitGhostControl` intent), stage hide/kill. Stop-words
  during drills abort the task only; during idle they inform "Nothing
  running, sir."
- **Ring = "AI is driving" indicator, truthfully**: ON only while a
  commanded motion is active (riding the glide), OFF otherwise (never
  rides the user's cursor). Orb waves are driven by the separate
  `ghost:session` event (session-scoped — survive turns, off on
  session end).
- **Esc panic:** dynamically registered on entry, unregistered on
  session end — never steals Escape globally.
- **Races closed:** in-flight suppression (mid-flight deviations are
  the AI's own glide), fresh poll before every click, atomic clicks
  never split, Esc from any state.
- **Scoped out:** keyboard-takeover hook (`WH_KEYBOARD_LL` +
  `LLKHF_INJECTED` flag) is a follow-up; Esc covers emergencies.
  Stage-hide kills the session (no leash, no session).

## 4. Ring (Clicky-like CSS, opposite semantics)

38px circle + glow + pulse + AI dot at 2 o'clock, `pointer-events:
none`, explicitly outside hitboxes, positioned from the Rust poll
(physical px → CSS via devicePixelRatio). Follows the real cursor —
when NEXUS moves the mouse the ring rides it; when the user grabs, it
dies instantly. Never on any capture-exclusion list (it must be seen).

## 5. Phases

- **Phase 0 (built):** ring + session + takeover detector + Esc panic.
  Zero AI motion. Proves poller, visual, leash.
- **Phase 0b (built with 0):** abort semantics, stand-down paths,
  announce rails, drill entry (`ghost_enter`).
- **Phase 1 (BUILT 2026-09-25, doc 38):** keyboard ghost — `launcher.rs`
  (Win-search open + focus verify), drill runner with per-step
  stop/session guards, `live_ghost_whatsapp` command (send stays behind
  confirm), voice-stop via `live_cancel`, announce rails. No key tracker
  needed (primitives are atomic — verified). Overlap is queued, not
  parallel (Phase 3).
- **Phase 2 (BUILT 2026-09-25, doc 39):** mouse ghost —
  `live/commands/mouse.rs` (eased glide, click/double/scroll/drag,
  restore), UIA-first resolver (exact > starts-with > contains; password
  fields never resolve), `live_ghost_click` with focus-verify before and
  after (never click blind, never click twice), suppress windows on all
  paths. Vision deferred until measured need.
- **Phase 3 (BUILT 2026-09-25, doc 40):** overlap engine — drill-depth
  flag + follow-up queue (cap 5, drop-oldest) + stop-word intercept in
  `process_transcript` (raw-phrase match, since bare "cancel" parses as
  Greeting; `cancel_action` NluResult matched too). Stop aborts with
  spoken confirm and never routes; the rest queue silently and drain in
  order after clean runs only (takeover/failure drops the queue — no
  resume behind the user). Nested drills re-queue into the same loop
  (DRAIN_CAP 10).
- **Phase 4 (BUILT 2026-09-25, doc 41):** hardening — 5-point
  calibration probe (`live_ghost_calibrate`, max/mean px + verdict),
  exclusive-fullscreen auto-pause (spoken, in both runners),
  refusal battery (pure scorer, password exclusion pinned to one layer,
  ghost tools in allowlist). Fixed: no glide-home after takeover.

## 6. Cross-platform appendix (Windows now, seams ready)

Windows full (SendInput/enigo, UIA, GetCursorPos). macOS full behind a
one-time Accessibility permission (CGEvent + AXUIElement; permission
check every launch). Linux X11 full (XTest + AT-SPI); Wayland partial
by design (portal-gated) with honest messaging. Code shape:
`windows_impl` / `macos_impl` / `linux_impl` behind the existing
`live/commands` split; enigo stays the shared actuator.

## 7. Verification doctrine (every phase, twice)

Unit (detector matrix, key-release order, session transitions) → live
drills (scripted task, grab-mid-flight <100ms abort, Esc from every
state, stop-word mid-type) → soak (30min normal use, zero false
aborts). Grounding (Phase 2+) gets before/after counts; refusal battery
never regresses.
