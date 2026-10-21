# Alexa-Style Barge-In: Stop TTS Mid-Speech on Wake Word or Hotkey (Plan, 2026-10-02)

**Goal:** saying "NEXUS" or pressing Ctrl+Space while NEXUS is speaking stops speech *immediately* and starts listening — like Alexa. **Scope is STOPPING, not detection** (wake-during-TTS still arrives via the v4 sustain+verify path; making detection itself instant is a separate DSP task, §7).

## 1. How it works today (researched, no changes)

**Single speech pipeline.** Every spoken line flows `speak_line` (`orchestrator.rs:3500`, bare event emit) → frontend `Result` → `speak()` (`ttsPlayer.ts:168`) → `speak_text` IPC (`tts.rs:158`) → rodio `play_audio` (`tts.rs:618`). No Rust-direct audio exists — frontend `state` (`speaking`/`thinking`) tracks all speech.

**Three stop primitives (all working):**
1. `stop_tts()` IPC (`tts.rs:147`) → bumps `TTS_GENERATION`; rodio polls every 20 ms and `sink.stop()`s (`tts.rs:632-637`). Stop latency ≤ ~20 ms + invoke overhead.
2. Frontend `stopTts()` (`ttsPlayer.ts:279`) → bumps frontend generation (stale `speak()`/`onEnd` suppressed), clears `rustTtsPlaying`, invokes `stop_tts`, cancels Web Speech fallback, emits `tts-ended`.
3. `cancel_active()` (`orchestrator.rs:1829`) → marks `ACTIVE_REQUEST` cancelled + clears; `install_new_request` (`:258`) auto-cancels the previous turn.

**Hotkey barge is COMPLETE (the model):** `hotkey.rs:89-120` — `stop_tts` + `cancel_active` + `tts_playing=false` + `drop_followups` + 150 ms DAC drain + `start_stt_capture` + `__NEXUS_WAKE__` eval. Ghost-exit branch (`:73-87`) same hygiene.

**Wake-word barge is the HOLE:**
- While `tts_playing`, `should_suppress_wake()` drops all wake chunks (`wakeword_oww.rs:1708`, Layer 3 `meeting_detect.rs:104-106`). Only the v4 path survives: sustained speech → one prob-0.0 candidate (`:1763-1792`) → STT exact-"nexus" verify → `VERIFIED_BYPASS` fire.
- The fire path (`wakeword_oww.rs:3509-3559`) does **none** of the hotkey hygiene: no `stop_tts`, no `cancel_active`, no `tts_playing=false`, no `drop_followups`, no DAC drain. It relies 100% on frontend `wakeWithGreeting` (`main.tsx:150-184`).
- Frontend stop is **conditional**: `if (wasSpeaking || thinking)` (`main.tsx:155`). Any state drift (idle/listening while rodio still plays — early `onEnd`, failsafe reset, Rust/frontend generation skew) = TTS talks over the new turn.
- Ghost drill stop-words (`orchestrator.rs:826-827`) likewise skip `stop_tts` — old narration plays until "Stopped, sir" arrives via the `speak()` leading edge.

**Timing stack (four different drains):** hotkey 150 ms (`hotkey.rs:110`) · frontend wake 300 ms (`main.tsx:179`) · `tts-ended`→`tts_playing=false` 500 ms (`lib.rs:619`) · post-TTS wake-mute 2000 ms (`wakeword_oww.rs:1814`). The 2 s gate drops *wake* chunks only, not the STT capture — safe but must not be disturbed.

## 2. Gap matrix

| # | Gap | Severity |
|---|---|---|
| G1 | Wake fire path has zero barge hygiene (Rust) | P0 — the core hole |
| G2 | Frontend wake stop conditional on `speaking`/`thinking` | P0 — drift = talk-over |
| G3 | Ghost drill stop-word skips `stop_tts` | P1 — slow-feeling stop |
| G4 | Four inconsistent drain constants (150/300/500/2000) | P2 — echo-risk tuning |
| G5 | No single choke point; barge logic copied in 3 places | P2 — future drift |

## 3. Execution plan (in order, each phase independently verifiable)

**Phase 0 — choke point + constant.** New `orchestrator::request_barge_in(reason: &str)`: `stop_tts` + `cancel_active` + `MeetingState.tts_playing=false` (via global, same accessor hotkey uses) + `ghost::drop_followups` — in that order (audio first, bookkeeping after). New `const BARGE_DAC_DRAIN_MS: u64 = 150` in the same module. Pure-order unit test with a recording fake is overkill; test = existing-path regression (Phase 5). *Files: `orchestrator.rs` only.*

**Phase 1 — wake fire path (fixes G1).** In `wakeword_oww.rs` fire body after `start_stt_capture()` (`:3514`): call `request_barge_in("wake-fire")`, then `tokio::time::sleep(BARGE_DAC_DRAIN_MS)` before the `__NEXUS_WAKE__` eval. The fire loop is sync — spawn the sleep+eval on the existing async runtime pattern (mirror `hotkey.rs:106-118`). Meeting/manual-pause privacy is untouched: barge candidates can only exist when `tts_only` (`:1756-1762`), so this code is unreachable under meeting suppression.

**Phase 2 — unconditional frontend stop (fixes G2).** `main.tsx:wakeWithGreeting`: call `stopTts()` + `stopVad()` unconditionally at entry (idempotent: generation bump + stateless rodio stop + `speechSynthesis.cancel` — all safe when idle), keep the 300 ms DAC delay only when `wasSpeaking`. Rationale: stopping silence is free; missing a stop costs a talk-over.

**Phase 3 — drill stop-word (fixes G3).** `orchestrator.rs:826`: add `stop_tts` + `cancel_active` before `request_stop()` (narration dies instantly; "Stopped, sir" still speaks after via normal `speak_line` → leading edge).

**Phase 4 — drain audit (fixes G4, no behavior change unless measured).** Document the four constants and their owners in this file's §1 table (done); change nothing unless live testing shows echo — then tune `BARGE_DAC_DRAIN_MS` in one place (now possible because of Phase 0).

**Phase 5 — tests.** Rust: choke-point order test (audio-stop invoked before queue purge — via `#[cfg(test)]` hook or log-sequence assertion on existing `test_cancel_active_sets_flag` pattern, `orchestrator.rs:5008`); parser untouched (no new phrases → no new intent tests). Frontend: `wakeWithGreeting` unconditional-stop test is DOM-heavy — cover with a `stopTts`-idempotency unit test in `ttsPlayer` (double-call while idle = no throw, generation +2). Manual matrix (§5).

**Phase 6 — gates + build.** `cargo test --lib -- --test-threads=1`, `vitest`, `tsc`, `node nexus.mjs build`. Zero new warnings.

## 4. Risks & mitigations

- **Double-stop:** all primitives idempotent (generation bumps, atomic stores, `sink.stop()` on empty sink). Safe.
- **Stop during idle:** `stop_tts` with nothing playing bumps generation only; next `speak_text` takes a fresh generation (`tts.rs:169`) — no lost speech. Safe.
- **Ghost narration killed by wake barge:** intended (Alexa semantics); session stays, drill stop-words still work via Phase 3 path.
- **`tts-ended` grace vs immediate re-listen:** `stopTts` emits `tts-ended` → 500 ms task clears flag; new capture starts at once but the 2 s post-TTS mute only gates *wake* chunks, and sustained user speech re-arms barge. No deadlock.
- **Fire-loop blocking:** sleep moved off the candidate-receiver thread (async spawn) — 1.5 s cooldown (`LAST_NEURAL_FIRE_MS`) unaffected.

## 5. Manual verification matrix (user-run, `nexus start`)

1. Long reply speaking → say "NEXUS …" → audio cuts ≤100 ms, orb listens, no echo of own voice in transcript.
2. Same via Ctrl+Space (regression — already worked).
3. Ghost drill running → "stop" → narration cuts instantly + "Stopped, sir. Ghost mode is still on."
4. Wake during idle → normal greeting, no regression (no spurious stop logs).
5. Meeting mode → wake suppressed as before (privacy unchanged).

## 6. Rollback

Each phase is additive and independently revertible (`git diff` per file). Kill-switch: none needed — worst case is today's behavior (conditional stop).

## 7. Explicitly out of scope

Instant wake *detection* during TTS (v4 sustain+verify latency ~1-2 s stays); bare-"stop" barge without the wake word (routes through STT transcript, unchanged); Web Speech fallback removal; TTS ducking instead of cutting.
