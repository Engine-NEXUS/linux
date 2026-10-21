# 38 — Ghost Phase 1: Keyboard Ghost + WhatsApp Drill (2026-09-25)

The first phase where NEXUS actually drives: keyboard-only task runs
inside ghost sessions. No mouse motion anywhere — the motivating
WhatsApp drill (open → search → type) completes with zero grounding,
zero pixels, zero vision. Plan: `docs/features/64-ghost-mode-plan.md`
(Phase 1); takeover leash: doc 37.

---

## 1. What shipped

**`live/commands/launcher.rs` (new):** `open_app_via_search(app_name)` —
Win key → type name → Enter → verify via the existing
`focus_app_by_title` (AttachThreadInput). Rejects blank names; returns
an honest error when the window can't be confirmed (the drill aborts on
it — launches are never silently assumed). Windows-only impl with a
clear cross-platform error; `macos_impl`/`linux_impl` seams slot in
later. Registered in `live/commands/mod.rs`.

**Ghost cancel flag (`ghost.rs`):** `GHOST_CANCEL` atomic +
`request_stop()` / `stop_requested()` / clear-on-entry. Voice "stop"
flows through `live_cancel`, which now takes `AppHandle`, sets the flag
*and* aborts the session (safe when idle — abort is a no-op off-session;
JS invoke signatures unchanged since Tauri injects the handle).

**Drill runner (`live/commands/ghost_drill.rs`, new):**
`whatsapp_drill(app, contact, message)` — validate inputs (non-empty +
contact denylist via the existing `safety_check`, e.g. "my bank
account" is refused) → `ghost_enter` (ring, Esc, cleared stop flag) →
announce "Taking the mouse" → guarded Win-search open → guarded contact
search → guarded type → `ghost_exit` + "Draft ready — say send".
**Send is never included**: `live_whatsapp_send` (confirm-gated) owns
it, unchanged. Every step checks stop-requested AND session-active, so
a mouse grab mid-drill freezes the next step (takeover already yielded
the session underneath). Any outcome — success, stop, takeover, error —
always exits the session and announces the result.

**Command + registration:** `live_ghost_whatsapp(contact, message)`
(`live/mod.rs`) runs the same triple safety screen as the manual path
(open_app / whatsapp_search / type_text) before the drill starts;
registered in `lib.rs` beside the other `live_` commands.

**Announce helper (`ghost::announce`)**: one-line orb speech over the
`stage:notice` rails (no handshake contact), used for session entry,
draft-ready, errors, and the takeover handoff.

## 2. Findings that shaped the code

- **No key-release tracker needed.** All keyboard primitives
  (`press_key`, `press_hotkey`, paste) are synchronous and atomic —
  press and release happen inside one call with no await/step boundary
  between them (verified by reading `keyboard.rs`). Abort-between-steps
  therefore cannot strand a modifier. Documented in the drill module;
  the tracker stays a Phase 2 concern (held motion states don't exist
  until the mouse module).
- **Overlap is queued, not parallel (honest scoping).** The drill runs
  synchronously in its command, but continued speech still captures via
  the independent Rust STT thread and follow-ups execute after. True
  act∥listen lanes are Phase 3 — not claimed here.
- **`live_cancel` signature change is JS-safe.** Tauri injects
  `AppHandle`; frontend `invoke("live_cancel")` call sites pass no new
  args.

## 3. Verification (twice)

Rust full lib **534/534 serial** (new: launcher blank-name rejection,
drill denylist/allow safety tests). `cargo check` clean, zero new
warnings. No frontend files changed — frontend suites unaffected
(26/26 from Phase 0 still current). Live drill procedure for the built
app: invoke `live_ghost_whatsapp` ("mummy", "hi") → ring + "Taking the
mouse" → WhatsApp opens, chat found, message typed, "Draft ready" →
say send → existing confirm flow sends. Abort drills: "stop" mid-type,
mouse grab mid-search (<100ms freeze, "You have it"), Esc mid-drill.
