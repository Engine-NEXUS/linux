# Mic Stability + Meet Parity (2026-09-19)

Two researches combined: (A) Intel SST brand behavior + why Meet works,
(B) Meet's mute-model (never tear down live-but-quiet pipelines).
Plus: why Python recordings hear perfectly while the live path misses.

## 1. Combined problem–cause–solution table

| # | Problem (observed) | Cause (verified) | Solution (planned) |
|---|---|---|---|
| 1 | 1 wake out of 5 calls; misses cluster in quiet stretches | Silence-recovery restarts after ~5s of quiet-room silence; each restart = 10s grace blackout + SST burst-fade provocation. Engine deaf a large fraction of tests | Restart ONLY on terminal signals (cpal error, device loss, bit-exact zeros 60s+ with prior audio). Grace 10s → 2s |
| 2 | Recovery loop oscillates (6 restarts / 4 min in user log) | Quiet mistaken for dead + restart provokes the fade it treats as death (self-inflicted oscillation) | Same fix as #1 + backoff floor 60s between precautionary restarts |
| 3 | SST fades to zeros during idle | Intel DSP power-gates the mic path when the audio engine is idle (documented across Dell/HP/Lenovo; OEM driver recommended) | Keep-alive experiment: inaudible WASAPI render stream beside capture; 1-hr soak measures flatline rate before/after |
| 4 | Can't distinguish dead vs quiet vs gone | All three funnel into one "restart" response; true death signals (cpal err callback, device-enum loss) unwatched | Three-state handling: quiet→nothing; exact-zero+evidence→probe; error/device-loss→reopen. Heartbeat becomes observer-only (Meet's getStats role) |
| 5 | No user-visible mic truth | Silence looks identical to death in console | Heartbeat (SHIPPED): LIVE/SILENT bars every ~2s + mic self-test command (3s record + verdict) |
| 6 | OEM/enhancement variables unknown | SST behavior depends on OEM audio package + APO enhancements + 48kHz format; field units vary | User checklist: HP audio package, enhancements OFF, shared 48kHz; log negotiated format at startup |
| 7 | Python hears perfectly, live path misses (see §2) | Temporal luck + API-path differences (below) | A/B experiment (§3) decides; fix follows evidence |

## 2. Why Python recordings hear perfectly but live misses

Same hardware, same room, different results. Ranked hypotheses:

| # | Hypothesis | Evidence for | Evidence against | Decides it |
|---|---|---|---|---|
| H1 | **Temporal luck (burst vs fade).** Python opens fresh → speaks in 1–2s → catches the post-open burst. NEXUS runs 24/7 through fades + 10s blackouts | Camp: 200+ clean clips via always-fresh opens; live misses cluster in flatline/blackout windows per log timestamps | — | A/B (§3) + fix #1 (if misses vanish, H1 confirmed) |
| H2 | **API path (MME vs WASAPI).** Python default = MME (WaveIn, system APO/resample chain); NEXUS cpal = WASAPI shared (code comment admits SST silence here). Different gain staging, APO processing, resampling | MME and WASAPI are genuinely different driver paths on SST; array beamforming APO may differ per path | Both *can* work (probes passed on both at times) | A/B same-content dual capture |
| H3 | **Burst-phase alignment.** Even healthy SST delivers in bursts; Python's beep→speak protocol accidentally synchronizes to burst starts | Miss pattern correlates with flatline stretches, not with audio content | — | Same as H1 (fixed together) |
| H4 | **Format/buffer mismatch.** Wrong native rate, exclusive-mode grab, or starving buffers on the cpal side | Would produce consistent distortion, not intermittent misses | Misses are intermittent; hits are clean (0.9+ probs) | Downgraded — startup format log (#6) closes it |

**Check-twice protocol (standing rule):** no hypothesis graduates without (a) code-path evidence AND (b) a live A/B measurement. H1+H2 are code-evidenced; the A/B below is the measurement.

## 3. A/B experiment design (dual verification)

- **A1 (API path):** same utterance, simultaneous Python MME-16k + Python WASAPI-48k captures → compare RMS/clarity/Groq transcripts. Isolates H2.
- **A2 (temporal luck):** 10 scheduled wakes at random 1–5 min intervals on the live build, log trigger-or-miss + heartbeat context (blackout? fade? clean?). Isolates H1/H3. Pass bar: ≥8/10 with acoustic (not blackout) explanations for misses.
- **Verdict rule:** if A1 splits → fix is endpoint/mode alignment (Meet's exact device config); if A2 shows blackout-shaped gaps → fix is recovery redesign (§1 rows 1–2); if both clean → suspect model/threshold, reopen acoustic investigation.

## 4. Build order (mic-parity work items)

1. Recovery redesign (three-state + 2s grace + 60s floor) + unit tests
2. Startup format/device log + mic self-test command
3. Keep-alive render experiment + 1-hr soak measurement
4. A/B rig (A1 script + A2 scheduled protocol)
5. User checklist doc (OEM package, enhancements, format)
6. Re-soak 10× + FA re-check after 1–5 (nothing ships on mic claims without it)

## 5. Built (2026-09-19, in `wakeword_oww.rs` + `commands.rs` + `lib.rs`)

- `classify_stream_health` (pure, 5 tests) + `EXACT_ZERO_CBS` tracking on the
  audio path + `STREAM_ERROR` flag in both cpal err callbacks + device-presence
  poll (gated: only when already suspicious) + 60s restart floor with
  hard-evidence bypass + `mic_self_test` Tauri command (ring analysis, no new
  stream) + `micKeepAlive` setting (default true) + silent render keep-alive
  started in `run()` + `boh`-style unit tests for verdicts (442/442 suite,
  zero warnings both feature sets, incl. mock stubs).
- **Revision vs §4 plan: grace KEPT at 10s** (not cut to 2s). Reason: the 10s
  exists for documented startup-burst false triggers (0.9+ probs ≤7s after
  restarts). With restarts now rare (terminal-only), the blackout cost
  vanishes while burst protection stays. Cutting it would trade one FA class
  for another — net validated by the same log evidence that condemned the
  restart frequency.
- Awaiting: rebuild + 1-hr soak (restart count vs 6-restarts/4-min baseline)
  + scheduled 10× wakes + keep-alive on/off comparison.
