# 37 — Ghost Ring + Takeover Leash (Phase 0+0b, 2026-09-25)

Ring + leash with zero AI motion: session machine, takeover detector,
Esc panic, ring UI, and all disarm paths. Nothing in NEXUS moves the
cursor yet — this phase proves the safety system before any hand exists.
Plan: `docs/features/64-ghost-mode-plan.md`.

---

## 1. What shipped

**`src-tauri/src/ghost.rs` (new):**
- `Session`: Idle → Active → Yielded (yielded ≠ failed: the user took
  over, probably finishing the task themselves — no retry, no error).
- `decide_takeover()` (pure, tested): commanded-motion window (+150ms
  easing settle) suppresses; settled target allows 2px slop; between
  steps ANY motion yields; first sight never judges.
- `observe_cursor()` per-tick hook (called from the stage hitbox loop —
  one poll thread, two consumers): takeover check + `ghost:ring`
  position events (physical px, emitted only on change: 30Hz spam cap).
- Dynamic Esc: registered on entry, unregistered on every exit path
  (never steals Escape globally — static registration would eat Esc in
  every app; verified against plugin 2.3.2 API).
- `abort_session()` (idempotent): ring-hide + Esc release + brief "You
  have it, sir." via the `stage:notice` rails (speak + local reset, zero
  handshake contact).
- `stand_down()` (silent): stage hide/kill disarms the session — no
  leash, no session. `note_blackout()` resets observed position so
  motion during an outage can't false-trigger after recovery.

**Wiring:** `lib.rs` (`mod ghost` + 3 commands), stage hitbox loop calls
`observe_cursor`, `stage_hide`/`stage_hide_kill` call `stand_down`,
blackout shutdown calls `note_blackout`. Linux-gated shortcut code
(mirrors `hotkey.rs`' `cfg(not linux))`).

**Ring UI (`frontend/src/stage/`):** 38px circle + glow + pulse +
AI dot, `pointer-events: none`, positioned from Rust physical px ÷
devicePixelRatio, shown/hidden by `ghost:ring` events (`ghost.css` +
StageApp listener).

## 2. Design decisions (why, with receipts)

- Real cursor, not triangle: hover/drag capable; ring (not second
  pointer) shows who's driving.
- Takeover overrules everything including queued plans; no auto-resume
  (rejected: resuming behind the user's back causes disasters).
- Key-release tracker deferred to Phase 1 (no keys pressed yet — no
  dead code shipped); keyboard-takeover hook scoped as follow-up, Esc
  covers emergencies.
- Entry is log-only (no TTS ambush); yield announces (handoff must be
  unambiguous).

## 3. Verification (twice)

Rust: 7 ghost tests (takeover matrix ×6 incl. settle/match/deviate +
session cycle) + 2 stage tests; full lib **531/531 serial**.
Frontend: tsc clean, production build clean, full suite **26/26**.
Clippy: zero hits in new code. Drill procedure for the live build
(`ghost_enter` → move mouse → ring dies + "You have it" <100ms;
Esc from active session; `stage_hide` mid-session disarms silently).

## 4. Deliberately not here (Phase 1+)

AI-commanded motion, `note_expected` callers, key tracking, overlap
engine, UIA resolver, calibration suite. The detector's suppress path
is tested but has no producer until Phase 2 — `note_expected`/
`note_idle` carry `#[allow(dead_code)]` and say so.
