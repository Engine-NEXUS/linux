# Improvement Ledger — living document

**How to use:** after each implementation phase, fill the entry: *what was broken → what we copied/changed (upstream file → our file) →
how it was tested → before/after numbers → star re-score*. Never write a number here that wasn't measured; leave `TBD`.
Plan: [doc 05](05-implementation-plan-2026-10-05.md). Classification: [doc 04](04-cross-check-and-case-classification-2026-10-05.md).

## Baseline ("Before") — captured 2026-10-04/05

| Metric | Value | Source / how measured |
|---|---|---|
| Rust lib tests | 874/874 per AGENTS.md; the 2026-10-04 `turn_detect` run showed **878 tests in the lib** (875 others + 3 new) — full suite pass count **not re-run** | `cargo test --lib -- --test-threads=1` — **TBD re-run** |
| Frontend tests | 154/154 | `vitest run` — TBD re-run |
| Wake word | recall 97.5 %, false-alarm 1.67 %, val loss 0.0369 | AGENTS.md (2026-10-01 retrain) |
| STT turn end rule | 800 ms silence (10 × 80 ms) / 1.2 s patient (15) / 10 s cap / 8 s no-speech | `wakeword_oww.rs` consts L2592–2603 |
| Idle RAM | ~104 MB before first transcription; ~232 MB after | AGENTS.md (2026-09-01) |
| e2e command fixture | 49/50 | AGENTS.md |
| GPL in binary | **Yes** — espeak-ng via `piper-rs` | doc 02 |
| Smart Turn | built, unwired; Rust↔Python mel diff 3.0e-5; 129 ms/8 s (opt-level 3, first call) | doc 03 |
| Star average (18 rows) | 2.9 (self-assessed) | doc 01 §8 |

## Entries (fill after implementation)

### P0 — Licence audit & Smart Turn prototype  — **done 2026-10-04**
* **Problem:** unknown licence exposure; hand-tuned endpointing.
* **Fix:** audit (doc 02); standalone Smart Turn module (`turn_detect.rs`) + `ort` direct dependency (pinned, same as piper-rs).
* **Tested:** `cargo test --lib turn_detect` 3/3 (mel parity vs HF ≤ 3e-5; model loads; policy table). tract could not run the quantized model → ONNX Runtime.
* **Result:** finding #1 (GPL espeak-ng) documented; endpointing benefit **unproven** (needs real-speech eval).
* **Re-score:** none yet.

### P1.1–1.2 — Silero VAD — **implemented, flag OFF (`vadSilero`), live A/B pending** (2026-10-05)
* **Problem:** capture decides "voice" with fixed RMS thresholds (0.01 start / 0.006 continue). Documented in AGENTS.md: Intel SST noise floor can exceed lower thresholds (16 s noise captures → Groq hallucinations), and quiet speech under 0.006 is missed.
* **Fix (Case 1):** `src-tauri/src/vad.rs` — Silero v4 on `ort` (design + model from Handy, MIT), 512-sample frames with carried LSTM state; decision in `wakeword_oww.rs::chunk_voice_flags` (pure): neural mode = continue at p ≥ 0.35, start at p ≥ 0.5, never below RMS 0.002; model missing/erroring ⇒ automatic fallback to the legacy RMS rule. `resources/vad/silero_vad_v4.onnx` bundled via `tauri.conf.json`.
* **Tested (automated):** `vad::tests` 5/5 — noise/silence never voice; real-speech fixture ≥ 40 % of 80 ms chunks voiced; chunking-independence (1280 vs 100-sample feeds, state diff < 1e-5); reset determinism; latency. `chunk_voice_flags` tests 2/2 — energy mode proven identical to the legacy hysteresis; neural mode table (loud-noise-rejected, quiet-speech-accepted, hysteresis, silence floor).
* **Measured:** Silero ≈ **0.98 ms per 80 ms chunk** (debug test profile; ≈ 1 % of the chunk budget). Python check: speech frames p up to 1.00; digital silence max 0.04, 3 mV mic noise max 0.13, white noise 0.05 RMS max 0.24.
* **NOT measured yet:** false captures per 120 s of real room noise and cut-off rate on real speech — needs a live session with `"vadSilero": true`. Before/After: **TBD**.
* **Re-score:** none until the live A/B.

### P1.3 — Smart Turn wire-in — **implemented, flag OFF (`smartTurn`), live eval pending** (2026-10-05)
* **Fix:** `wakeword_oww.rs::smart_turn_poll` + worker thread (model never runs in the audio callback; callback ships a ≤8 s snapshot via `sync_channel(1)` and reads the latest verdict via `try_lock`). A verdict is valid only for its own silence run (`SMART_TURN_RUN` bumped on every voiced chunk). Smart Turn can end a turn **earlier** (≥ `min_silence`, verdict ≥ threshold) or hold it **open longer** (verdict < threshold ⇒ limit raised to `patient_chunks`); it can never shorten a turn below `min_silence`. Failure/model missing ⇒ legacy rule. `endpoint_reason` reports `"smart_turn"` in `stt:turn_stats`.
* **Tested (automated):** `smart_turn_poll_off_and_run_invalidation` (flag-off identity; too-early silence; end-now; hold-open; run invalidation) + `turn_detect` 5/5 (mel parity, model load, policies).
* **Open risk (honest):** on TTS speech the model said "complete" (0.92–0.99) for every phrase, including cut-offs — so with `High` eagerness (240 ms) it could cut hesitant speakers. Defaults are therefore Medium (400 ms earliest). **Do not enable by default** until the 30-clip evaluation (doc 03) shows fewer premature cuts than the 800 ms rule.
* Before/After: **TBD** (needs real recordings).

### P1.4 — Barge-in instant "stop" — **implemented (always on, narrowly scoped)** (2026-10-05)
* **Problem (live tests):** during TTS only a transcript containing "nexus" interrupted (exact-only verify) — a bare "stop" did nothing.
* **Fix:** `wakeword_oww.rs::barge_stop_decision` hooked into `verify_candidate`: for barge-in candidates only (prob 0.0 = sustained speech while TTS plays), a transcript that is **exactly** a stop phrase (`ghost::STOP_PHRASES` + "stop talking / be quiet / that's enough"; punctuation/case normalised; ghost-exit phrases excluded) calls `request_barge_in("barge-stop")` + emits `tts:stop`, with **no** wake. Whole-utterance rule means our own TTS echo ("Stopped typing, sir.") cannot self-interrupt.
* **Tested (automated):** `barge_stop_decision_table` (exact phrases ✓; echo sentences ✗; real-wake probabilities ✗; ghost exits ✗; empty/noise ✗).
* **Plan target corrected:** doc 05 promised "< 300 ms". That is **not achievable on this path**: it needs ~6 sustained chunks (≈ 0.5 s) + VAD-trim + one STT round trip (Groq ≈ 0.3 s+ network) ⇒ expect roughly **1–1.5 s**. A sub-300 ms stop needs an on-device stop-word spotter (new item, not started). Live latency **not measured yet**.
* Known limits: one barge attempt per TTS session (`BARGE_ATTEMPTED`) — a first noise attempt consumes it; unchanged by this work.

### P1.5 — `turnEagerness` — **implemented** (2026-10-05)
* `turn_detect::{Eagerness, policy_for, smart_end, extended_limit}`; `settings.json` key `turnEagerness: "low"|"medium"|"high"` (unknown ⇒ medium) read via `commands::read_turn_eagerness`. Low = 560 ms earliest / thr 0.7 / hold to ~1.8 s; Medium = 400 ms / 0.5 / ~1.2 s; High = 240 ms / 0.4 / no extension. Table-tested (strict ordering, 240 ms floor). Only effective when `smartTurn` is on.

### Phase 1 verification (2026-10-05)
* `cargo test --lib -- --test-threads=1`: **888 passed, 0 failed, 1 ignored** (earlier same-day run before the barge/eagerness/poll tests: 884). No new compiler warnings in touched files.
* Frontend untouched. Tauri bundle config gained `resources/vad/*` and `resources/smart_turn/*` (release build **not** re-run).
* All three behaviour changes are OFF by default except the narrowly-scoped barge "stop"; with flags off the capture loop is behaviour-identical (proven by the energy-mode table test and flag-off test).

### P2 — Rust STT spike — TBD (WER Δ, cold start, RAM)
### P3 — GPL-free offline TTS (Kokoro in the Feature-83 slot) — **implemented 2026-10-05; live online/offline run + voice audition pending**
* **Problem:** GPL espeak-ng linked via Piper; 60 MB download per voice change from an unpinned URL; no hash pins; rapid A→B→C could leave the offline voice on A.
* **Fix:** Kokoro-82M engine on `ort` + `misaki-rs` (no espeak); one shared model + one 0.5 MB voice file; latest-wins swap queue; pinned revision + SHA-256 for every catalog voice; verify-then-atomic-replace; online-first with automatic return to cloud; idle unload; Piper, its 61 MB model and the 8 MB espeak data removed. Full record: `docs/changes/81-kokoro-local-tts-replaces-piper.md`.
* **Tested (automated):** `cargo test --lib` **902 passed / 0 failed** (888 before the phase); new tests: 12 engine, 18 catalog, 14 swap (latest-wins, plan matrix, verify/rollback, pins), unload policy; `cargo tree` → **0 espeak packages**; frontend tsc clean, vitest 154/154.
* **Measured:** intelligibility by ASR round-trip WER 0.03–0.10 (25 clips, 5 voices); speed ≈ real-time on the i7-1355U (RTF 0.7–1.7, noisy); RAM +128 MB load, 259–369 MB typical, 654 MB at 300 tokens (hence chunk cap).
* **Honest cost:** offline first-audio for a 3 s sentence is ≈ 2–3 s (Piper's claimed ~40 ms was not re-measured) — mitigated only by sentence streaming. This trades offline latency/RAM for a clean licence and better voices.
* **Model variant (measured):** int8 kept — same transcripts as fp32 (16/16) at 129 vs 351 MB model RAM and 92 vs 326 MB download; fp16 rejected (WER 0.25 vs 0.04); q8f16 slower and larger-RAM than int8. fp32 is ~1.6x faster (RTF 0.62 vs 0.98).
* **Not done:** end-to-end run with a real network loss, release build, voice audition (incl. an int8-vs-fp32 listening A/B), bundling decision for first-launch-offline users.
* **Re-score:** hold TTS at ★★★ until the audition; licence blocker (doc 02 #1) cleared.

### P4 — Skills / memory / scheduler — TBD
### P5 — MCP server / browser — TBD
### P6 — Sandbox / exec policy — TBD
### P7 — Grounding — TBD
### P8 — Directed-speech gate — **implemented 2026-10-05; live evaluation pending** (change 82)
* **Problem:** in a ghost session's open mic, anything the parser can't match goes to the cloud LLM — TV, side talk, our own TTS echo, STT hallucination loops (your real log: 96 unique unmatched utterances, mostly noise).
* **Fix:** `directed.rs` — deterministic gate (origin tag + vocative/command/echo/loop/filler/length/cue/product-vocabulary rules), only for hot-mic turns; explicit turns never gated; ignored text logged locally; 3 consecutive ignored turns park the hot mic.
* **Tested (automated):** 11 Rust tests + 5 frontend tests; full suites Rust 912/0, frontend 159/159.
* **Measured:** replay of the real log: **75 % of unmatched utterances ignored (72/96)**, 24 kept of which ~4 are false accepts; hand-made labeled set 0/31 false accepts, 1/25 false ignores (optimistic — not held-out).
* **Not measured:** live false-accept/false-ignore with real TV/echo, 8-hour soak, STT-confidence features. Doc 08 targets (≤ 2 % / ≤ 5 %) remain unverified goals.
* **Re-score:** hold Safety at ★★★★ until a live run.
### P9 — Proactive speech policy — **implemented 2026-10-05; live evaluation pending** (change 83)
* **Problem:** the Sentinel spoke deadline alerts the instant they fired — cutting off NEXUS mid-reply (frontend `speak()` stops current audio), talking over the user, and in a meeting being **silently lost** (frontend mutes all speech).
* **Fix:** `proactive_policy.rs` — pure engine with injected clock: urgency-tiered breakpoints (user not speaking / NEXUS not speaking / no drill / not in a meeting), Critical speaks within seconds and **during meetings** via a self-expiring TTS-mute override (your decision; setting `proactiveCriticalInMeeting`), heads-up beat + "Urgent, sir." lead-in, rate limits (20 s gap, 6/hour), snooze, dedup, expiry that counts only speakable time.
* **Tested (automated):** 15 new tests incl. deferral timelines and an exhaustive "never over the user / never in a meeting (non-critical)" sweep; Rust suite **927 passed, 0 failed**. Two real design bugs found and fixed by the timeline tests (expiry counted meeting/snooze time; new alerts were credited with pre-existing idle time).
* **Not done / honest limits:** no live test; **nothing produces a Critical alert yet** (the deadline alert is High, so it waits for a meeting to end rather than speaking in it); Medium/Low sources not fed; no UI for `proactive:nudge`/`proactive:card`; "not now" voice command not wired; all thresholds are hypotheses (the CUI-2021 paper publishes no numbers).
* **Re-score:** Scheduler/proactive stays ★★ until Critical sources exist and a live run confirms timing.
### P10 — Screen-context memory — TBD (design only, needs your scope decision)

## Star scorecard (re-score after each phase)

| Pillar | Before | After P1 | After P3 | After P4 | After P5 | After P6 |
|---|---|---|---|---|---|---|
| VAD / turn / barge-in | ★★ | TBD | | | | |
| TTS | ★★★ | | TBD | | | |
| Skills | ★★ | | | TBD | | |
| Memory | ★★★ | | | TBD | | |
| Scheduler | ★★ | | | TBD | | |
| MCP | ★★★ | | | | TBD | |
| Browser | ★★ | | | | TBD | |
| Sandbox | ★ | | | | | TBD |
| **Average (18 rows)** | 2.9 | TBD | TBD | TBD | TBD | TBD |

## Honest-failure log (things that didn't work)

* tract-onnx 0.23 cannot analyse Smart Turn's quantized Conv1d (`ConvHir`) → had to use `ort`.
* Smart Turn on SAPI TTS clips: all 0.92–0.99 — TTS prosody can't validate a prosody-based model.
* Quantized model: Python onnxruntime vs Rust `ort` differ on out-of-distribution input (0.118 vs 0.171 on a pure tone) — don't pin probabilities.
