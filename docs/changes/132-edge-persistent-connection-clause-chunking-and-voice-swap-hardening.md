# 132 — Speed-first voice: persistent Edge connection, wake-time warm-up, earlier streaming, first-clause chunking, Feature 83 hardening

Date: 2026-10-17. Decision record: end of `docs/features/136-...md` (speed first, Edge online, local only offline, Smart plan stays, never always-loaded).

## Built
1. **One shared Edge client** (`tts_edge.rs`). Before, a new `EdgeTtsClient` was built for every sentence, so every call paid a fresh TLS + WebSocket handshake. Now a single client keeps a warm socket (pool of 1, idle TTL 45 s, replenished after each use). Any failed call drops the client so the next call starts clean.
2. **Wake-time pre-warm** (`tts_edge::prewarm`, hooked next to the NLU pre-warm in `start_stt_capture_with_origin`): when the user starts speaking, the socket is opened (only if the link looks up and Edge is healthy; rebuilt only when the client is older than 30 s).
3. **Streaming starts earlier** (`STREAM_MIN_CHARS` 150 -> 60): a normal two-sentence answer now speaks sentence 1 while sentence 2 is synthesized.
4. **First-clause chunking** (`tts::split_first_clause` / `plan_chunks`): when the first sentence has 14+ words and a comma/semicolon/colon that leaves a head of >= 4 words and a tail of >= 3, the first audio is only the head. No comma = no split (never cuts mid-clause).
5. **Feature 83 hardening (the owner's voice-selection plan):** the manifest is written write-then-rename (atomic); `twin_ready_for` now also requires the installed voice file's SHA-256 to match the manifest (a crash between "file replaced" and "manifest written" self-heals); the loaded engine's voice is swapped only between utterances (`tts::speech_active()`, bounded 120 s wait), never mid-reply.

## Verify
* New tests: `tts::first_clause_tests` (5), `tts_edge::prewarm_tests` (2), `voice_catalog::manifest_integrity_tests` (2).
* Speed evidence: `cargo test --release --lib tts_edge::latency_bench -- --ignored --nocapture` prints `LATBENCH` lines (fresh-per-call vs shared warm client vs idle + wake pre-warm). 
* **Measured (2026-10-17, release build, this laptop, on battery, real Edge, voice en-GB-RyanNeural, 50-character line):**

| Case | Time to the complete first sentence |
|---|---|
| Old: new client per call (6 runs) | 635-1361 ms, **median 1009 ms** |
| New: shared warm client (6 runs) | 308-496 ms, **median 409 ms** (-60 %) |
| New: after 50 s idle, wake-time pre-warm, call 2.5 s later | **290 ms** |

  Not measured: end-to-end turn (needs a rebuild + live use; read the `[LAT]` lines), the first-clause split and the 60-character streaming threshold (unit-tested for correctness only, no timing).
* Full lib run: all voice tests pass (90 of 90 filtered). Unrelated failures seen: the two Spotify parser tests (another session's music-control change) and, only in the parallel full run, `auth_vault::google_account_registry_roundtrip...` and `memory::test_profile_carries_phone...` (both pass alone: shared global state, not this change). Dead-code gate shows new `vision.rs` fields (another session); baseline not updated.

## Not done
* Streaming the cloud LLM answer into speech (start speaking while the answer is still being written) - needs router + orchestrator + frontend changes; next item.
* Learned Edge-voice bank, connectivity state machine, Smart warm/cold triggers.
* Prosody auto-emotion still varies per line (planned removal).

## Addendum - live log 2026-10-17 02:20 (first run of the new build)
* **Bug: orb/turn closed 10 s into an 18 s answer** ("stuck-speaking watchdog: silent 10s, forcing turn close" while the volume lease was still held until 02:20:39). Cause: `rustTtsPlaying` in `ttsPlayer.ts` is ONE boolean shared by overlapping speech calls. The "One moment sir." ack was superseded by the answer; the ack's `speak_cached` promise resolved a beat later and its `finally` set the flag to false while the answer was playing, so the watchdog counted 10 "silent" ticks. Fix: only the current utterance (`ttsGeneration === myGen`) may clear the flag / emit `tts-ended` / fire `onEnd`, in `playKokoro` and `speakCached` (a superseded cached call also no longer falls back to speaking its stale phrase). Regression test in `ttsPlayer.test.ts`, mutation-checked (fails without the guard).
* **Bug: "check down my laptop" went to the cloud** (STT mishearing of "shut down"). Fixed properly by `docs/changes/134-...` (strict list for every logged mishearing + structural loose match that asks once).
* Observed, not changed: `[LAT] stt 638 | parse 49 | tts 856 | total 1545` - the stock ack was heard 1.5 s after the end of speech; the 856 ms between parse and first audio for a stock line is longer than a cached play should take (the frontend awaits two IPC round trips - meeting state and settings - before invoking `speak`). To investigate.
* Needs `nexus build` + `nexus start` to take effect.
* **2026-10-17 note:** the working tree was rebuilt on `D:\ULTRON` after the C: folder was emptied; the code and tests of this record were re-applied and re-verified there.
