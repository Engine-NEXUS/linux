# STT Empty-Transcript & Mic Pipeline — Research, Root-Cause Ranking, Approaches & Plan

**Date:** 2026-10-01 · **Status: A→C→B→E IMPLEMENTED 2026-10-01 (gates green).**
**Trigger:** live run 14:40–14:41 — wake word fires (0.985/0.991), Rust capture starts, frontend releases mic (~9s later), `stt:transcript = ''`, retry captures again. User spoke; nothing was taken.
**Companion ask:** visuals must follow mic truth — zoom in/out ONLY while listening, loading circles ONLY while thinking.

---

## 1. What the log actually proves (timeline reconstruction)

| Time | Event | Meaning |
|---|---|---|
| 14:40:28–44 | WAKE probs 0.985 → 0.991 → 0.521 → 0.985 → 0.834 → 0.995 | **The cpal stream hears audio fine** (same stream feeds STT) |
| 14:41:04 | `stt-capture: started (cpal-side capture, no baton pass)` | Rust begins buffering |
| 14:41:13 | `cpal stream resumed (mic baton pass — frontend released mic)` | `__NEXUS_RELEASE_MIC__` → `resume_wakeword` fired |
| 14:41:15 | `stt:transcript = ''` | Rust emitted empty (endpoint or timeout or filtered) |
| 14:41:17–25 | second identical cycle | deterministic, not a one-off |

Two load-bearing facts: **(a)** wake works on the same stream minutes apart, so the mic is not hard-dead; **(b)** the 9s gap matches the frontend 8s no-speech abort (`App.tsx` 8s timer → `abortCapture` → `releaseMicStream` → baton log), NOT a user action.

## 2. Pipeline map (ownership per stage)

```
WAKE (Rust cpal + OWW, threshold 0.68)
  → start_stt_capture()              [Rust: clears buffer, CAPTURING=true]
  → cpal callback appends 80ms chunks [Rust only — frontend opens NOTHING on wake]
  → endpoint: silence≥5 chunks (400ms) after ≥3 voiced chunks,
              patient 12 chunks (~1s) after 2 pauses,
              cap 125 chunks (10s), no-speech 100 chunks (8s)
  → transcribe_and_emit → Groq primary → local Moonshine fallback
  → hallucination filter + phantom guard (either can map to "")
  → emit "stt:transcript"
  → processTranscript("") → silent dismiss (hideOrbIfIdle + reset, no TTS)
```

Parallel frontend timer: **8s no-speech watchdog** (`App.tsx`) → `abortCapture()` → `releaseMicStream()` → `resume_wakeword`. This does NOT stop the Rust capture — so both timers fire and the turn always ends in `""`.

## 3. Hypotheses, ranked with evidence

| # | Hypothesis | For | Against | Verdict |
|---|---|---|---|---|
| **H1** | cpal stream went silent mid-turn (Intel SST dropout; known bug, RMS→0) | Repeat empties; SST history in this repo | Wake probs high on the SAME stream at :28/:44; dropout is usually total until restart, yet captures "start" cleanly | **Possible (intermittent)** — needs RMS evidence per turn |
| **H2** | Frontend mic hog starved cpal | Baton log proves a release path ran | No wake-path opener exists: `warmMic` disabled, `resumeVad`/`captureUntilSilence` uncalled on wake, levels come from Rust `audio:level`; the baton log fires **unconditionally** (release fn has no stream check) → it is noise, not proof of hogging | **Weak, but not excludable** — a STALE stream (setup/enrollment/paramCapture leftovers) can't be ruled out without a holders audit |
| **H3** | Speech never crossed the energy gates (soft voice, late start, AGC) | Cheapest explanation for a single miss | Twice in a row with an experienced user; wake fired (different, lower gate) | **Possible** — measure, don't guess |
| **H4** | Transcription failed and the error path yields `""` | Groq transient/429, or local sidecar cold/missing model | Diagnostics show Groq present; would normally log a warn | **Possible** — error-path audit needed (`stt_groq` err → fallback → emit shape) |
| **H5** | Phantom/hallucination filter ate real speech | Short commands (< 3 voiced chunks) are indistinguishable from blips by design | "Open Brave"-length speech clears the gate easily | **Unlikely for normal commands; possible for 1-word commands** |
| **H6** | Dual-timeout race guarantees the `""` outcome | Both 8s timers fire; frontend abort can't stop Rust; retry loop + orb flicker follow deterministically | This explains the *shape* (retry loop), not the *silence* | **Architectural defect regardless — fix even if H1–H5 resolve the instance** |

**Single decisive discriminator (runnable TODAY, no code):** the `audio:level` event already streams per-chunk RMS to the frontend during capture. Next miss, check console/store `micLevel` trace for that turn: **levels moved + `""` → STT/filter fault (H4/H5)**; **levels flat at 0 → mic fault (H1/H2/H3)**. Plus one `mic_self_test` run (2.5s verify-ring report) as the static mic-health baseline.

## 4. Approaches (rated)

| Approach | Speed | Delay | Error risk | Miscommunication risk | Complexity |
|---|---|---|---|---|---|
| **A. Instrument-first (RECOMMENDED first): turn packet** — attach `{rms_max, rms_mean, voiced_chunks, endpoint_reason, stt_path, filter_verdict}` to the transcript event + `debug_trace` | ★★★★★ (no behavior change) | ★★★★★ (zero) | ★★★★★ (read-only) | ★★★★★ | ★★★★☆ |
| **B. Single-owner endpoint** — Rust owns stop; frontend 8s abort forwards `stop_stt_capture` instead of only releasing; delete the parallel timer | ★★★★☆ | ★★★★★ (kills the guaranteed-`""` race) | ★★★★☆ | ★★★★★ | ★★★☆☆ |
| **C. Mic-holder audit + exclusive-open guard** — track every `getUserMedia` opener with timestamps; refuse/warn double-open; auto-disable stale tracks; log holders on every empty turn | ★★★☆☆ | ★★★★☆ | ★★★★☆ | ★★★★☆ | ★★★☆☆ |
| **D. Gain/threshold retune** — ONLY with A-data: AGC on the STT path, threshold audit | ★★☆☆☆ | ★★★☆☆ | ★★☆☆☆ (retunes regress quietly) | ★★★☆☆ | ★★☆☆☆ |
| **E. Retry-with-escalation UX** — 1st miss: silent auto-relisten (no speech); 2nd consecutive miss: one spoken "I didn't hear you" + visible listening pulse | ★★★☆☆ | ★★★★☆ | ★★★★☆ | ★★★★★ (honest feedback) | ★★★★☆ |

**Recommended sequence: A → C → B → E. D only with measurement data.** A is pure observability (zero regression surface) and tells us which of H1–H5 is real; C closes the only unmeasured hole; B removes the deterministic race; E makes remaining misses humane.

## 5. Visual↔mic-truth gating (your second ask — verified current state)

Current mapping (already as you want it — `Avatar.tsx`, tested in `avatarAnim.test.tsx`):

| State | Visual today | Correct? |
|---|---|---|
| `listening` | smile holds + rAF **frame-locked breathe** (54f period), no CSS pulse | ✅ zoom in/out while listening |
| `thinking` | `loading-loop` circles exclusively | ✅ loading only while thinking |
| `speaking` | smile + zoom (deeper period) | ✅ |
| `idle` | parked hold frame, no motion | ✅ |

The gap is **state correctness, not mapping**: `zoomForFrame` breathes from the playhead whether or not audio flows, so a *stuck* `speaking`/`listening` state (e.g. after an empty transcript mishandling) plays zoom over silence. Plan (with B): **liveness-gated visuals** — scale breathe amplitude by trailing mic/TTS activity (`micLevel`/`ttsActive` already in store; clamped, never fully flat so it can't look dead on quiet speech) + a stuck-state watchdog (state `speaking` with no TTS-progress and no transcript for >X → force `finishSpokenResult` path). Unit tests: amplitude scaling table, watchdog trigger/no-trigger matrix.

## 6. Verification plan (per approach)

- **A:** unit tests for the packet builder (all endpoint reasons, both STT paths, both filter verdicts); live: 5 wake→speak turns, attach packets to the log; decision gate — H1–H5 resolved by data.
- **B:** unit tests (abort forwards stop; no double-stop; ghost hot-mic unaffected); e2e: no-speech turn ends exactly once, single `""`, no retry loop.
- **C:** unit test (double-open refused; stale auto-released); live: `mic_self_test` before/after; holders logged on empty turns for a week.
- **E:** frontend tests (silent first miss, single nag on second, pulse visible); live acceptance script (2 consecutive silent turns).
- **Visuals:** `avatarAnim` amplitude tests + watchdog tests; live: kill mic permission mid-`speaking` → zoom settles, no stuck motion.

## 7. Definition of done

1. Next 10 live misses each carry a turn packet naming the cause class (no more blind `""`).
2. Single-owner endpoint: one timer, one stop path, no parallel abort.
3. `mic_self_test` green + holders audit clean on the user's machine.
4. Visuals provably follow audio truth (tests + live kill-mid-speech drill).
5. Docs updated in lockstep; no new global listeners or OS-level mic changes.

---

## 8. Implementation record (2026-10-01)

| Approach | What landed | Files | Tests |
|---|---|---|---|
| A — turn packet | `TurnStats{session,rms_max,rms_mean,voiced,total,endpoint,stt_path,filter}` travels with the buffer; `endpoint_reason()` mirrors the stop predicate; `LAST_STT_PATH/LAST_FILTER` markers in all 3 transcribe fns; `stt:turn_stats` emitted at every receiver exit (incl. stale/empty/phantom/runtime-fail); frontend listener logs + `debug_trace` | `wakeword_oww.rs`, `stt.rs`, `main.tsx` | endpoint table, RMS stats, wire shape, marker round-trip |
| C — holders audit | `micHolders.ts` registry; shared-getter records + reuses (also fixes a per-call stream leak); release funnel records; p0 trace carries holders summary | `micHolders.ts` (+6 tests), `main.tsx`, `recorder.ts` | acquire/release/idempotency/summary |
| B — single owner | 8s timer queries non-destructive `stt_capture_had_speech()`; voice underway → hands off (no hide); silence → legacy path (late Rust success still lands) | `wakeword_oww.rs`, `commands.rs`, `lib.rs`, `App.tsx` | query arms (incl. no-residue) |
| E — escalation | miss-streak: 1st silent auto-relisten (reset-first so the wake guard can't misread it as cancel), 2nd single nag + deterministic close; streak resets on heard speech | `recorder.ts`, `recorder.test.ts` | policy mapping |
| Visuals | stuck-speaking watchdog inside `hideOrbAfterSpeech` (10 quiet 1s re-arms → `finishSpokenResult`; genuine TTS re-arms forever; ghost untouched); 60s failsafe kept as backstop | `orchestrator.ts`, `orchestrator.test.ts` | force-close, no-fire-while-playing |

**Design correction during build (B):** the first cut forwarded the destructive `stop_stt_capture` from the timeout — review caught it amputating slow starters (stop clears the Rust buffer, so "let Rust finish" was a lie). Replaced with a pure query command; abort stays exclusive to the explicit second-press cancel. Documented here so the regression is never reintroduced.

**Flake note:** one full-suite vitest failure during build (`stuck-speaking` 10-timer marathon) passed in isolation twice — timing-fragile under load. Rewrote the test to seed 9 ticks + single firing (accumulation still covered by the re-arm test). Full suite green ×2 after.

**Gates:** Rust 788/788 serial (includes other lane's ongoing additions) · vitest 108/108 ×2 · tsc clean · clippy zero-new in touched ranges.
**Live acceptance (user-run):** next miss must arrive with a `stt:turn_stats` line + `turn ... holders=...` trace — paste both and the cause class is named.
