# 70 — Ghost Mode Complete Program Guide (phases 0–4 + voice integration, 2026-09-25)

One-stop feature doc: what Ghost Mode is, why it is shaped this way,
every phase's decisions, the voice-integration fixes, and what remains
honestly un-built. Deep dives: `ghost-mode-architecture-reference.md`
(symbol map, wiring, Send-invariants) and
`docs/research/ghost-mode/` (Clicky teardown, grounding costs, issue
timeline) alongside this file. Implementation record:
`docs/changes/37–46`.

---

## 1. Definition (locked, user's terms)

A **voice-entered session** in which NEXUS operates the user's REAL
cursor and keyboard while the user keeps talking. A glowing ring rides
around the real cursor (never a second pointer) so it is always visible
who drives. Ends on completion, "stop", mouse grab, Esc, stage hide, or
drill-failure. Visible control with a visible leash — deliberately the
opposite of stealth overlays (Clicky-style theater, opposite ethics).

## 2. Standing design rules

1. **Keyboard path first** — Win-search-open-type flows need zero
   grounding, zero pixels. Mouse only where no key sequence exists.
2. **One entity, one driver** — only one party moves the cursor per
   instant; any uncommanded motion = human hand = yield.
3. **Explicit entry, instant, multi-path exit** — voice word in;
   grab/Esc/stop/stage-hide/timeout out. No auto-resume after takeover.
4. **Send NEVER auto-fires** — desktop typing stays visible; the
   confirm gate owns the send. Credential fields + blocklist refused.
5. **Narrate on entry and exit, never over a live turn.**
6. **Fullscreen foreground pauses (not aborts)** — clicking into games
   clicks the void; say so instead.

## 3. What was built, phase by phase (all verified twice, serial Rust + frontend + trace 12/12)

| Phase | Built | Core decisions |
|---|---|---|
| **0 — ring + leash** (doc 37) | session machine; pure takeover detector (suppress window + 2px slop + first-sight-inert); dynamic Esc (registered on entry, released on every exit — never global); ring events on change; silent stand-down on stage hide; blackout position reset | leash before hands; zero AI motion shipped |
| **0b** (doc 37) | abort semantics ("You have it, sir."), all disarm paths | takeover ≠ failure ≠ completed |
| **1 — keyboard ghost** (doc 38) | `launcher.rs` (Win-search, focus-verified); WhatsApp drill runner with per-step stop/session guards; `live_ghost_whatsapp`; voice "stop" reachable (`live_cancel`); announce rails; **no key-release tracker needed** (primitives press+release atomically, verified) | Win-search flows need no grounding; overlap is queued (queued, not parallel) |
| **2 — mouse ghost** (doc 39) | eased glide (25ms steps, per-step stop poll), atomic clicks, scroll (no motion), drag (release-on-abort), restore (no-glide-home after takeover); UIA-first resolver (exact>starts>contains, password fields excluded upstream); `live_ghost_click` focus-verified before AND after | suppress windows on every path; vision deferred until measured need |
| **3 — overlap engine** (doc 40) | drill-depth flag; follow-up queue (cap 5, drop-oldest); stop-word intercept in `process_transcript` (raw phrases — bare "cancel" parses as Greeting); drain in order on clean-standalone only; nested drills re-queue (cap 10) | voice "stop" became reachable; silent queueing (no TTS echo) |
| **4 — hardening** (doc 41) | 5-point calibration probe + verdict; exclusive-fullscreen auto-pause; refusal battery (pure scorer, password-exclusion single-layer pinning); real-flaw fix: no glide-home after stop/takeover | boring-in-the-good-way pass |
| **voice integration** (docs 42–46) | entry split ("ghost mode" = cursor control; dictation keeps writer words); waves orb + palette transfer; initial ring emit; stage scrollbars; hot-mic capture loop + anti-nag; heartbeat client marker; in-session app/message routing (desktop drill, registry-first, send confirm-gated); banner removal; debug_trace taps; Send-safe runtime refactor | each live defect traced from a real log walk, not review luck |

## 4. Architecture pointers

- Module/state map + runner protocol + call graph:
  `ghost-mode-architecture-reference.md`.
- Compile-time-guaranteed Send/Rust refactor rationale (single
  `ghost_wry` erasure boundary; `spawn_blocking` for blocking OS acts).
- Research base: clicky teardown, grounding options and cost ledger,
  OS matrix (Win full now; macOS/X11 seams ready; Wayland caveat per
  the plan §6).

## 5. Success criteria met

- Hands-off entry by voice: say "ghost mode" once (no wake word inside).
- Cursor ring + orb waves follow/control visibly; stage chrome absent.
- "open whatsapp" in-session → ack <100ms, app focused, session open.
- "send message to mummy saying i am busy" → app open, contact found,
  message typed **visibly**, send gated; "search for mummy" opens chat.
- Stop word / mouse grab / Esc / stage-hide: instant, safe, narrated.
- Zero full-blackout UX under watchdog (stage plan doc 63).

## 6. Honest remainders (documented, not bugs)

- UAC/elevated windows: UIPI silently eats clicks (OS-level); elevation
  detection is the named follow-up.
- Vision grounding deferred until a measured need (per research ledger;
  UIA covers the current use cases exactly and free).
- Keyboard-takeover hook (`WH_KEYBOARD_LL`) deferred; mouse-of-takeover
  + Esc covers emergencies.
- Multi-monitor stage covering = primary monitor only (single-stage
  shell step 1; migration plan staged thereafter).
- `debug_trace` p0–p4 taps + `trace_ghost` tool are TEMPORARY (delete
  after the voice-entry fix is confirmed live).
