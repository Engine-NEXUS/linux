# Ghost Mode Architecture — Runtime & Engine Reference (2026-09-25)

Structural companion to the plan (`docs/features/64-ghost-mode-plan.md`):
what each module actually exports, how state flows, and the Send/safety
invariants the compiler enforces. All symbols verified in-tree.

---

## 1. Session core — `src-tauri/src/ghost.rs`

State (all process-wide singletons):

| Symbol | Type | Purpose |
|---|---|---|
| `SESSION` | `parking_lot::Mutex<Session>` | Idle / Active / Yielded |
| `EXPECTED` | `Mutex<Option<((i32,i32), Instant)>>` | commanded target + settle deadline (suppress window) |
| `LAST_SEEN` | `Mutex<Option<(i32,i32)>>` | last polled cursor (takeover diff) |
| `LAST_RING_SENT` | `Mutex<Option<(i32,i32)>>` | change-detector for ring events |
| `GHOST_CANCEL` | `AtomicBool` | stop flag (voice stop / Esc / API) |
| `GHOST_BUSY` | `AtomicU32` | drill depth (nests) |
| `FOLLOWUP_QUEUE` | `Mutex<VecDeque<String>>` | mid-drill speech, cap 5, drop-oldest |
| `DEVIATION_TOL_PX` | 2 | slop for commanded-settle comparison |
| `FOLLOWUP_CAP` / `DRAIN_CAP` | 5 / 10 | queue + drain bounds |
| `STOP_PHRASES` | 8-entry list | normalized raw stop matching |

Pure functions (all unit-tested): `is_stop_phrase`. The takeover
detector (`decide_takeover`) is **deleted** (2026-09-27 user
directive — mouse use is always free, Esc is the cancel button).

Runtime functions (all Wry-typed — see §4):

- Probe: `stop_requested`, `drill_running`, `session_active`,
  `queue_followup`, `take_followups`, `drop_followups`,
  `drill_begin/end`, `note_expected(pos, settle_ms)`, `note_idle`,
  `note_blackout`
- Display: `announce` (`stage:notice` channel), `observe_cursor`
  (per-tick: ring **motion-only** — rides commanded glides, off
  otherwise; no takeover judgment), `ring_off`, `emit_session`
  (`ghost:session` — session-scoped orb signal) — called from the
  stage hitbox loop thread
- Session: `ghost_enter` (stage show → state → Esc arm → **session
  event**), `ghost_exit`, `ghost_abort` (generic wrapper → `ghost_wry`
  shim), `stand_down` (silent disarm on stage hide/kill)
- Session end: `abort_session` (**explicit exits only**: Esc /
  exit-phrase / stage hide/kill — mouse use never ends the session;
  the old `abort_task` was deleted with the takeover detector)

All 9 entry/exit paths reset `EXPECTED` — an uncleared suppress window
would blind takeover detection; audited per path (doc 39/41 findings).

## 2. Runners — `src-tauri/src/live/commands/`

| File | Exports (the hands) |
|---|---|
| `ghost_drill.rs` | `guard_step` (stop → session → fullscreen, in that order), `whatsapp_drill` (`keep_session` mode; registry-first open → search → paste-type; send NEVER included; `spawn_blocking` body; drain on standalone-clean only) |
| `launcher.rs` | `open_app_via_search` (Win+name+Enter, focus-verified, blank-rejecting; Windows-only) |
| `mouse.rs` | `eased_steps` (pure cosine), `current_pos`, `move_eased` (25ms steps, per-step stop poll, suppress registration), `click_at`/`double_click_at` (atomic at the click), `scroll`, `drag_to` (release-on-abort), `restore` (no-glide-home after takeover), `resolve_element`/`element_center`/`score_name` (UIA grounding: exact 3 > starts 2 > contains 1; password fields excluded upstream), `is_foreground_fullscreen` + `foreground_blocked`, `calibration_probe` + `CalibrationReport` |
| screen.rs (pre-existing) | `list_actionables` (password-skipping UIA list), `pick_ordinal`, `switch_browser_tab` |

Runner protocol (shared): safety screen → session enter (or reuse) →
announce → `drill_begin` → guarded steps → `drill_end` → session
exit-or-keep → announce outcome → drain (clean + standalone) or drop.

## 3. Wiring map (who calls what)

```
voice flow (wakeword_oww STT thread)
  → stt:transcript → recorder.processTranscript
      → (ghost silent-miss anti-nag: 3 re-listens → 1 nag → park)
      → parse (Rust parser <1ms → TS fallback)
      → orchestrator_process (via processViaOrchestrator)

process_transcript (orchestrator.rs), in order:
  1. Ghostwriter room entry / session intercept (pre-existing)
  2. Ghost DRILL-RUNNING intercept: stop-phrases (raw + parsed
     cancel_action) abort instantly; everything else queues silently
  3. Session active, no drill:
     OpenApp            → run_ghost_open  (runner outside-in, session kept)
     SendWhatsAppMessage/WhatsappChat → run_ghost_message → whatsapp_drill
  4. EnterGhostControl   → run_ghost_control_enter (ring only, no card)
  otherwise normal routing (LocalCommand / Worker / MCP / ...)

stage.rs hitbox loop thread (30ms)
  → stage click-through toggle + ghost observe_cursor (leash)
  + vault-monitor WhatsApp pairing probe (90s, other thread)
```

Voice "gateway" ports: `debug_trace` p0–p4 taps live in
`recorder::processTranscript` (TEMPORARY, `docs/changes/46` §2).

## 4. The runtime-erasure invariant (why Wry everywhere)

Ghost functions are **`AppHandle<tauri::Wry>`-typed, not generic**
`<R: Runtime>`. Reason (live-bug chain, compile-time-gated): the global
Esc handler registered via `tauri_plugin_global_shortcut` requires
`Send + Sync + 'static`, and `drain → drill → process_transcript` forms
an async cycle (E0733) whose futures cross threads. A generic `R`
poisons those futures with non-Send lifetimes (e.g. a held
`parking_lot` guard). Erasure lives in exactly one place:
`ghost_wry::g_wry` / `g_wry_ref` (same-layout pointer reinterprets —
`AppHandle<R>` is `{R::Handle, Arc<AppManager>, Arc<Mutex<EventLoop>>}`,
the Arc/Mutex layers are runtime-independent, and this crate only ever
instantiates Wry). The blocking drill body runs on `spawn_blocking` (a
plain `fn`, no held guards), keeping even the OS-actuation internals
Send-clean.

## 5. Live-drill matrix (per phase; procedure docs in changes 37–46)

| Drill | Asserts |
|---|---|
| entry | ring + reveal with zero mouse motion (initial emit) |
| waveform | smile pinch → 3-bar live waves (palette `#259ed6/#ef4f25/#fbdf38`), reverse on exit |
| hot-mic | "open whatsapp" with NO wake word → BATON line appears; 3 silences → 1 nag → park |
| messaging | send message → visible typing, NO send; chat-open variant opens chat |
| takeover | grab mid-glide/mid-type <100ms yield, strict no-glide-home, no resume |
| stop word | instant abort mid-drill, no Worker round-trip |
| fullscreen | exclusive-foreground app → spoken pause, no act |
| calibration | 5-point grid, max/mean px + verdict |
| overlap | second command mid-drill queues, drains in order (cap 10) |
