# Alexa-Style Barge-In — Wake Word & Hotkey Cut TTS Mid-Speech (2026-10-02)

**Plan:** `docs/research/voice-barge-in/alexa-style-barge-in-plan-2026-10-02.md` (all 7 phases executed).

## Problem
Hotkey already stopped TTS mid-speech (`hotkey.rs:89-120`), but the wake-word path did nothing in Rust (`wakeword_oww.rs` fire body) and the frontend stop was conditional on `speaking`/`thinking` (`main.tsx:155`) — state drift let old replies talk over new turns.

## Changes
- **Phase 0 (choke point):** `orchestrator::request_barge_in(reason)` (`orchestrator.rs`) — `stop_tts` → `cancel_active` → `clear_tts_playing` → `drop_followups`, plus `BARGE_DAC_DRAIN_MS = 150`. Hotkey TTS branch migrated (identical order, zero behavior change). New `wakeword_oww::clear_tts_playing()` (+ mock-wake no-op).
- **Phase 1 (wake fire):** fire path calls the choke point before `start_stt_capture()`; `__NEXUS_WAKE__` eval moved into an async spawn after the DAC drain (receiver loop never blocks). Meeting privacy untouched (barge candidates only exist when TTS-only muted).
- **Phase 2 (frontend):** unconditional `stopTts()` + `stopVad()` at `startListening` entry (idempotent; turn-teardown branching preserved, 300 ms delay still gated on prior speech).
- **Phase 3 (drill stop):** stop-word arm calls the choke point before `request_stop()` — narration cuts instantly.
- **Phase 4 (audit):** drains documented (hotkey 150 / frontend 300 / tts-ended grace 500 / post-TTS wake-mute 2000); no tuning changes.
- **Phase 5 (tests):** `test_request_barge_in_clears_active` (Rust); `ttsPlayer.test.ts` stop-idempotency ×2 (frontend, mock fixed to promise-returning `invoke`).

## Verify
- Rust **828/828** serial (1 new); vitest **144/144** (2 new); tsc 0; release binary 51.2 MB; warnings steady at 34 pre-existing, zero new.
- Uncommitted. Live matrix (`nexus start`): wake-cut ≤100 ms, hotkey regression, drill-stop instant, idle-wake normal, meeting suppression unchanged.
- Out of scope (unchanged): wake *detection* latency during TTS (~1-2 s v4 path), bare-"stop" routing.
