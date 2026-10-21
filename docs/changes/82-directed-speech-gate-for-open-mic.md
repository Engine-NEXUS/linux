# Change 82 — Directed-speech gate for open-mic (ghost hot-mic) turns

**Date:** 2026-10-05 · **Plan:** `docs/research/jarvis-landscape/08-case-2-plan-2026-10-05.md` §C2-2 · **Phase:** 8

## Problem
While a Ghost Mode session keeps the mic open, everything heard is transcribed. Anything the deterministic parser does not recognise falls through to the cloud LLM chat (`ParsedIntent::Unknown → WorkerBackend`). TV, a side conversation, NEXUS's own TTS echo and STT hallucination loops could therefore reach the LLM. The user's real `missed_intents.jsonl` (96 unique unmatched utterances) is mostly exactly that: fragments ("then the", "left."), foreign-sounding garbage, and loops ("I'm going to go." ×5).

## Design (ideas from Apple's multi-stage voice trigger + isair's engagement-gated "intent judge"; clean-room, no code copied)
* **Only open-mic turns are gated.** Each capture is tagged with who opened the mic: wake word, hotkey, confirmation/clarification window and follow-up questions are `Direct` (**never gated**); only the ghost hot-mic loop's own re-opens are `HotMic`. The origin is frozen at capture stop, so the hot-mic loop re-opening the mic immediately cannot change the verdict of the turn just heard.
* **Deterministic decision, no model, < 1 ms** (`src-tauri/src/directed.rs`, pure `evaluate`). Order: explicit turn → accept · dictation mode on → accept · deterministic parser recognises it → accept · mentions "nexus" (not in a repetition loop) → accept · **echo of our own recent speech** (≥ 60 % of the transcript's *content* words appear in the last 30 s of what NEXUS said; ≥ 2 content words) → ignore · repetition / hallucination loop → ignore · only filler words → ignore · longer than 14 words → ignore · starts like a request/command/question, a request phrase ("can you…", "please"), or ends with "?" → accept · mentions a NEXUS feature/app (ghost, hub, command, WhatsApp, browser, settings, Spotify, Chrome, GitHub, Gmail, YouTube, dictation, screenshot) → accept · otherwise ignore.
* **Ignored utterances are logged locally** (text only, `missed_intents.jsonl`, source `directed_gate`, 5 MB rotation — approved by you) and emit `directed:ignored`.
* **Loop behaviour:** an ignored turn counts like a silent one: the first two consecutive ignored turns re-listen quietly; the third **parks the hot mic** until you wake NEXUS again (so a television can't keep the loop and the cloud-STT bill running). The ghost session itself stays live.
* Fails **open** everywhere (gate unavailable ⇒ accept). Off switch: `"directedGate": false` in `settings.json`.

## Files
`src-tauri/src/directed.rs` (new) · `wakeword_oww.rs` (`start_stt_capture_with_origin`, origin snapshot at capture stop) · `commands.rs` (`start_stt_capture(origin)`) · `tts.rs` (`note_spoken` in `speak_text`/`speak_cached`) · `lib.rs` (module + command) · `frontend/src/net/directedGate.ts` (+test) · `frontend/src/stage/orbRuntime.ts` (`triggerFollowupListen(hotMic)`) · `frontend/src/net/ghostHotMic.ts` · `frontend/src/audio/recorder.ts` (one check in `processTranscript`, ghost sessions only).

## Verification
* `cargo test --lib -- --test-threads=1`: **912 passed, 0 failed** (6 ignored dev helpers). Frontend: tsc clean, vitest **159/159**. No new compiler warnings in touched files.
* New Rust tests: explicit turns never gated; recognised commands/dictation always pass; vocative; echo (incl. "open the repository" ≠ echo — a bug the test caught: function words were counting as overlap, now content words only); loops/fillers/long lines; assistant-shaped requests; origin snapshot freezing; a 56-line labeled set.
* **Measured on your real log** (ignored dev test `replay_real_missed_intents`, all 96 unique unmatched utterances treated as hot-mic turns): **72 ignored (75 %)** — no_cues 55, too_long 8, repetition 5, filler 4 — **24 kept**. The kept lines are mostly genuine or garbled-but-real commands ("Open", "Type.", "Cancel the ghost mode.", "NEXUS comment hub.", "Prem on WhatsApp.", "Wave browser."); about **4 are false accepts** ("Get the rest of the world.", "Open. Puri yunai. Andang on the east. Good.", "What? Standerling.", "Open's got to look at me. Ghost mode.").

## Honest limits
* **The labeled-set numbers (0/31 false accepts, 1/25 false ignores) are optimistic**: the rules were written after reading the same log, so it is not held-out. The replay above is the more honest number, and it only covers utterances that *failed* to match (it cannot show false ignores among utterances that did match — those bypass the gate).
* The gate **cannot tell a garbled command from noise** ("The post mode." for "ghost mode" is ignored). The real fix for those is better aliases/STT; the new `directed_gate` log entries feed that work.
* It uses the **deterministic parser only** (not NLU/brain), so a command only the NLU recognises, phrased without any cue, would be ignored in hot-mic mode.
* STT-confidence features (no_speech, avg_logprob) from the design are **not plumbed yet** — unused.
* **Not live-tested** (no real TV/side-conversation/echo session) and the targets in doc 08 (≤ 2 % false-accept, ≤ 5 % false-ignore, 8-hour soak) are **not yet measured**.
* Behaviour change to be aware of: three ignored utterances in a row park the hot mic.
