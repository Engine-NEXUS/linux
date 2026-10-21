# Ghost Waves Visibility, In-Session Actions, Pacing & TTS Barge-In — Research + Plan (2026-09-28)

From the 21:30 live test. Plan + research only — no code changed.

## A. Waves not visible — 3 concrete causes found

1. **Asset filename mismatch (hard bug).** You downloaded
   `frontend/public/waves.json` (12.6 KB, present on disk). The
   loader fetches `ghost-waves.json` (`Avatar.tsx:209`) → 404 →
   silent `.catch(() => {})` → the Lottie layer never mounts. The
   2026-09-28 plan named `ghost-waves.json`; your file is `waves.json`.
   **Fix (1 line):** try `waves.json` then `ghost-waves.json` in the
   loader (same pattern as `wakeup-v2.json` fallback).
2. **The orb hides between turns → waves vanish mid-session.**
   Waves render inside the orb window (`Avatar` in `main`). Every
   turn-end calls `setVisible(false)` (slide-down) + reset; during
   the listening gaps between commands the orb is hidden, so even a
   mounted waves layer is off-screen. Your log shows capture →
   transcript → silence → capture — the orb was hidden most of the
   time. **Fix plan:** while `ghostActive`, keep the orb visible
   (skip the hide in `endTurn` paths or re-show on session start and
   hold until exit). Session-scoped visibility, exactly like the
   waves' scope.
3. **Rest state is visually near-invisible.** When nobody speaks,
   bars rest at `scaleY(0.15)` (by design, "still when silent") —
   three short 18px pills. Combined with cause 2, the session looks
   wave-less. **Fix plan:** a gentle idle breathing motion (CSS loop
   at 0.25–0.4 scale) while ghost is active but silent — visible
   "session live" signal, still clearly distinct from speech motion.

Diagnostic cross-check for after the fixes: `audio:level` events
only flow during STT capture; if bars never move while you speak,
tap `debug_trace` for `audio:level` receipts (see E).

## B. Ghost actions not acting (open brave / new tab)

Log facts: `'open brief'` (STT mishear) → `'open brave.'` → no
action; `'Open tab. Create a new tab.'` → no action; `'new tab.'`
×2 → no action; `'open YouTube in brief'` → **RUST mcp: whatsapp
request failed** (wrong subsystem entirely).

Hypotheses (ranked, each with its disambiguator):
1. **Unified console doesn't show frontend logs** — zero `[NEXUS]`
   yellow lines in the paste, so the frontend branch taken is
   invisible. The `debug_trace` p0–p4 taps exist but print to the
   frontend console only. **Fix first:** forward `debug_trace` to the
   Rust log (invoke → `tracing::info!`) so `nexus start` shows the
   branch. Without this every ghost routing bug is blind-debugged.
2. **`run_ghost_open` silent-fail on "brave".** The registry may lack
   a Brave entry and the failure path may not announce (the error
   path announces only via `announce`; a silent Err is possible).
   Needs the trace from (1) to confirm.
3. **"open youtube in brave" → WhatsApp MCP** means the deterministic
   parser missed and NLU/brain classified it as a WhatsApp intent —
   a routing bug to fix with a deterministic browser-navigate pattern
   (`open <site> in <browser>` → focus browser + navigate).
4. **`new tab` in-session** routes to local `execute_command`
   (`ctrl+t` via enigo) — but if Brave isn't foreground the chord
   lands elsewhere. In-session browser commands should run through a
   ghost runner: focus browser first, then `ctrl+t` (same shape as
   the WhatsApp drill).

## C. Command pacing ("delay when I speak for the next command")

Current timing: endpoint fires 400 ms after your last syllable (fast
limit) → transcript → turn executes → `endTurn` → relisten after TTS
idle → new capture starts immediately. Two commands in one breath
("Open tab. Create a new tab.") arrive as ONE transcript and parse
only the tail. **Plan:** a configurable post-turn settle
(default ~600 ms) between turn end and the next capture, plus a
multi-sentence transcript splitter (execute sentence 1, queue
sentence 2) so two-in-one-breath commands both run. The settle also
fixes the "captures my next sentence's first word" class.

## D. TTS barge-in: speak = stop speaking

Current behavior (`wakeword_oww.rs:1707-1750`): during TTS-mute,
sustained speech (6 chunks ≈ 0.5 s above RMS 0.01) earns ONE
verify attempt whose transcript **must contain "nexus"**
(`VERIFY_EXACT_WORDS`, `:1894`). So saying "stop" mid-speech does
NOTHING — TTS keeps talking. That's your exact complaint.

**Plan (two-tier):**
1. **Instant stop tier:** when the sustained-speech barge threshold
   fires (0.5 s real speech, TTS-only-mute, not meeting/paused),
   immediately `stop_tts` + `orchestrator_cancel` — don't wait for
   the "nexus" verify. Risks backchannels/coughs stopping speech;
   mitigations: RMS floor + sustain already filter coughs; one-shot
   per TTS session (existing `BARGE_ATTEMPTED` re-arm); after
   stopping, capture the interrupting audio (`start_stt_capture`)
   so "stop" or the next command is actually processed.
2. **Command tier (existing):** if the verify finds "nexus", full
   wake+capture path as today.
Both tiers behave identically in ghost and normal mode — the mute
gate is mode-independent already.

## E. Ctrl+Space must stop TTS

`hotkey.rs:67-115`: today Ctrl+Space closes visible windows OR wakes
NEXUS — it never stops TTS. **Plan:** add a first branch — if
`is_tts_playing()` (meeting_state flag), `stop_tts` +
`orchestrator_cancel` and consume the keypress (no window close, no
wake). Applies globally (ghost + normal), matching your "same
TTS — Ctrl+Spacebar stopping".

## F. Waves in normal mode ("hey nexus")

Per your earlier directive the waves are ghost-session-scoped. Your
note reads as: the STOP-TTS behavior (D/E) should be identical in
normal mode — it is, by design (D/E are mode-independent). Waves
stay session-scoped unless you say otherwise.

## Execution order (when you say go)

1. A1 waves asset loader (`waves.json` + fallback) + A2
   session-scoped orb visibility + A3 idle breathing.
2. B1 `debug_trace` → Rust log forwarding (unblocks all diagnosis).
3. D1 instant-stop barge-in + E1 Ctrl+Space stop (one landing —
   both are stop_tts/cancel wiring + tests).
4. C1 post-turn settle + multi-sentence splitter.
5. B3/B4 deterministic browser patterns + in-session browser ghost
   runner.
Verify each landing: cargo serial + clippy touched + tsc/vitest +
fixture + turn-end gate; live re-test the 21:31 script (ghost →
open brave → new tab → Esc).
