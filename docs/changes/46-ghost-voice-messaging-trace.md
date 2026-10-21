# 46 — Ghost Cursor Control by Voice: Banner Removal, Trace Taps, Desktop Messaging (2026-09-25)

Three deliverables from the live-log analysis: the redundant TRIGGER
banner removed, a temporary trace system that makes silent frontend
deaths provable from `nexus start` alone, and voice-driven desktop
WhatsApp messaging inside ghost sessions (visible typing, no
auto-send). Plus one compile-time-forced Send refactor.

---

## 1. TRIGGER banner removal (display-only, your call honored)

The multi-line `🔔 [TRIGGER #NN]` banner duplicated the model-
probability lines with a *different* number (93.5% vs 99.5% — it
parsed a different field). Replaced with one concise truthful line:
`WAKE  wake word heard (HH:mm:ss, confidence NN%)`. `run.ps1` syntax
verified (pre-existing tokenizer errors elsewhere in the file are
unrelated — confirmed against the stashed original).

## 2. debug_trace + pipeline tap points (temporary, by design)

**Rust:** new `debug_trace(msg)` command (commands.rs, registered in
lib.rs) logging `TRACE {msg}` at INFO — displays in `nexus start`
because run.ps1 only suppresses mic/pairing/silence chatter.
**Frontend taps** (recorder.ts): p0 receipt (len + ghost state), p1
corrected text (catches correction-chain crashes), p2 parsed action,
p3 branch taken (local-execute / orchestrator), p4 outcome or throw.
Six lines per turn; temporary by design, delete after diagnosis.

## 3. Ghost desktop messaging (the feature)

In an open ghost session, `SendWhatsAppMessage` and `WhatsappChat`
now route to `run_ghost_message` → the desktop drill (open → search
contact → **paste-type visibly on screen**) instead of the MCP bridge
(which is down in your environment — connection refused). Empty
message = chat-open. **The send is NEVER included** — `live_whatsapp_send`
(confirm-gated) still owns it; nothing auto-fires. Session stays open
for follow-ups (drill gained a `keep_session` mode). Chat-open is
distinguishable in narration ("X is open, sir.").

## 4. Send-safety refactor (compile-time-forced)

`drain_ghost_followups` re-enters `process_transcript` → drill →
drain, an async cycle the compiler rejects (E0733). Fixes, in the
order found: boxed futures at both cycle edges (drain loop,
run_ghost_message), the drill's blocking OS steps moved to
`spawn_blocking` (enigo/Win32 internals are not Send — found at
compile time), and the ghost runtime markers erased at ONE boundary
(`ghost_wry::g_wry`/`g_wry_ref` in ghost.rs, same-layout pointer
reinterpretation — no field transmutes) so the Esc handler and
hitbox-loop thread stay Send. The `g_wry` unsafe block is the only
runtime-erasure site in the crate, documented, and would miscompile
loudly in any hypothetical multi-runtime build.

## Verification (twice)

Rust 550/550 serial + clean check, zero new warnings, zero clippy hits
in new files (10 borrow-_expr lints found during the sweep, fixed +
re-run). Frontend tsc clean ×2, 33/33 vitest, production build clean.
`nexus trace-ghost` static 12/12 ×2. Live script for the built app:
"ghost mode" → "open whatsapp" (ack <100ms, focused, session open) →
"send message to mummy saying i am busy" → WhatsApp opens, mummy found,
"i am busy" typed visibly, NO send → orb confirms draft. Trace lines
p0–p4 visible per turn; delete taps after review.
