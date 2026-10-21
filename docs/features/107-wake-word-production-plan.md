# 107 - Wake word "NEXUS": production plan (Alexa/Siri-level accuracy with real live clips)

Status: approved 2026-10-10. P0 (measurement) in progress, see docs/changes/124. Research and sources: docs/research/wakeword/industry-comparison-and-recipe-2026-10-11.md.

## Context

You asked for a wake word that works reliably on real "nexus" clips, including with background sound, at the
level of the pre-trained assistants (Jarvis, Hey Google, Siri, Alexa), and that hands off cleanly to the
command center and the wake orb.

What the code map found:

- **The headline numbers are scored on training data.** Recall 98.6%, 0 false alarms in 186 and so on come
  from `verify_hardened_model.py` and `soak_wake_fa.py` running on the same files the model was trained on.
  The trainer's 85/15 split shuffles windows, so windows from one clip and its augmented copies end up on
  both sides. On real owner clips (`wake_camp/sweep_v30.txt`) recall is 93.3% and 56% of negative cases
  fire. `necess_0003` is already a training negative and still scores 0.821.
- **The shipped recipe is thin.** No reverb (RIR) augmentation: the 270 `mit_rirs` go unused. No random-SNR
  mixing with real noise. 900 of the 997 background clips are procedurally generated, and an alphabetical
  `[:300]` cut drops most of them anyway. About 56% of positives are TTS. There is no TV, music or podcast
  audio in training.
- **There is no real-world loop.** The app keeps only 5 rotating, unlabeled debug WAVs. Near-misses, rejected
  candidates and misses are never saved.
- **Runtime drift and bugs:**
  - The threshold is 0.68 in the acoustic profile but 0.35 in the manifest.
  - The startup grace is 1.5 s in code but 10 s in logs, and the first reset is a no-op.
  - `wakeSensitivity` is never read.
  - The resampler has no anti-alias filter.
  - The profile's `device_name` is ignored.
  - The confirmation and in-engine speaker code is dead.
  - The Python tester has drifted from the Rust engine.
  - There is no echo cancellation (AEC), so the mic is simply muted during TTS.
  - Stage-2 verify adds a Groq round trip to every fire.
  - Nothing measures stage-1 → orb latency.

**Your decisions:**
- Opt-in, local-only live clip capture.
- Mostly you speak, so speaker verification is allowed.
- Target: **≥95% recall at ≤0.1 false wakes per hour**.
- Compute: this PC's CPU, plus Kaggle/Colab GPU.

## Research: how the leaders do it, and what we copy

| System | Published result | Method that matters |
|---|---|---|
| Porcupine | 97.1–97.3% recall at 1 false alarm per 10 h, at 10 dB SNR | Large real + synthetic data; tuned against noise |
| Alexa (Amazon) | Not published | Two stages: small on-device detector, then a bigger verifier; reject means stop capture |
| "Hey Siri" (Apple) | Not published; speaker check cuts false accepts **5–10×** for <1% extra misses | Speaker-independent detector → personalized speaker check, enrolled from real use |
| Google cascade paper | Stage 1 tuned to wake "a handful of times per hour" | Both stages trained on the same data with similar miss rates |
| openWakeWord stock ("hey jarvis") | Goal <5% misses at <0.5 false/h; one third-party test measured 8.5 false/h | Frozen mel + embedding front end; ACAV100M ~2,000 h precomputed negatives; RIR + noise |
| livekit-wakeword conv-attention | **0.08 false/h at 86% recall**, vs openWakeWord's flat head at 8.5 false/h on the same set | **Same 16×96 openWakeWord features as ours**, but a temporal conv + attention head instead of a flat MLP |
| microWakeWord | — | Picks checkpoints by false/h on real ambient audio (music, dinner party), not by validation loss |

Takeaways:

1. The front end we already run in Rust is the industry-standard one. The gap is in data, the head, evaluation
   and the cascade, not the front end.
2. Swapping the head is cheap, because the input shape doesn't change.
3. Real ambient audio, both for training negatives and for the false/h gate, matters more than anything else.
4. Speaker verification is the fastest route to the ≤0.1/h target.
5. "Nexus" has 2 syllables, while "hey jarvis" and "hey siri" have 3–4, so it is harder. The cascade and live
   data are what make up for that. "Hey nexus" stays as an accepted variant.

## Chosen method (five phases, measurement first)

### P0 — Honest measurement

Nothing else is trusted until this exists.

1. **Rust replay harness**, `src-tauri/dev_tools/wake_replay.rs`, with an `[[example]]` entry because
   `examples/` is git-ignored.
   - It drives the real `WakeEngine` and preprocessor from `wakeword_oww.rs` over WAV folders and long files.
   - It reports recall per condition, false/h, a DET curve, and the threshold at a target false/h.
   - This replaces the drifted `scripts/test_wake_live.py` as the source of truth, and fixes the broken
     `nexus wake devices` flag.
2. **A held-out test set split by session**, using the existing `scripts/wake_data_foundation.py` rules and
   creating `wake_word_data/manifest.jsonl`.
   - Seed it with the 177 condition-labelled clips in `wake_camp/raw` and the 5 real recordings in
     `wake_camp/retrain_pack/eval-negatives`.
   - Test sessions are never trained on.
3. **At least 10 h of held-out real negatives, recorded on this mic**: TV, YouTube and podcasts in English,
   Hindi and Telugu, music, conversation, typing, room tone with the fan, and NEXUS's own TTS through the
   speakers. Recorded with an extended `scripts/record_ambient.py`.
4. **One config source of truth.**
   - The acoustic profile supplies threshold, gain and grace to Rust, trainer and harness alike.
   - Remove or retire the manifest runtime block.
   - Fix the grace-period mismatch and the first-reset no-op.
5. **Baseline the current `nexus.onnx` with the harness.** Every later change must beat it.

### P1 — Live clip capture loop (opt-in)

1. New `src-tauri/src/wake_clips.rs` and setting `wakeClipCapture`, default false.
   - The setting must be added to `NexusSettings` (standing pitfall), with a toggle on the Command Hub Memory
     or Audio page.
2. **What gets saved.** The existing 2.5 s `VERIFY_RING` plus ~1 s after, as a WAV with a JSON sidecar
   (stage-1 prob, verify transcript and decision, RMS, device, `tts_playing`, timestamp, boot session id).
   Saved on:
   - every fire
   - verify reject
   - speaker reject
   - cooldown or in-flight drop
   - stage-1 near-miss (peak between 0.2 and the threshold, with speech present)
3. **Automatic label hints** (still confirmed by you):
   - Likely real wake: a verified fire followed by a parsed command.
   - Likely false wake: an immediate "no" or "cancel", an empty or non-command capture, or a silent dismiss.
   - Likely miss: a near-miss followed within 5 s by a second "nexus" or a hotkey wake.
4. **Retention.** Local only, never uploaded, deleted after 30 days, capped at 500 MB. Nothing is captured
   while a meeting mutes the mic (that path already exists).
5. **Review and collection commands:**
   - `nexus wake review`: plays each clip; you answer y/n/skip; it writes a manifest row.
   - `nexus wake collect-live`: a guided session covering near/far, quiet/loud, fast/slow, over music, over
     TTS and while typing. Spread over several days so sessions differ.

### P2 — Data and training recipe v2 (`scripts/train_wake_v2.py`)

The old trainer stays for A/B comparison.

**Positives:**
- Your real clips (wake_camp, the reviewed `nexus_*` recordings, labeled live clips) carry the most weight.
- Multi-voice TTS (Kokoro, edge-tts) is kept but weighted down. It keeps the model speaker-independent for
  family later.

**On-the-fly augmentation:**
- RIR from the 270 `mit_rirs`
- real noise mixed at random SNR from −5 to 20 dB
- speed 0.9–1.1 and pitch ±2 semitones
- gain and band-pass
- your measured mic EQ
- **NEXUS's own TTS mixed under "nexus"**, so the model learns barge-in

**Negatives:**
- The openWakeWord ACAV100M precomputed features (same embedding space, about 17 GB, processed on Kaggle).
- Real noise corpora (MUSAN or FSD50K; licenses checked; no NC-licensed data in the shipped model).
- Confusion words, and Hindi and Telugu speech.
- Hard negatives mined from live clips, with weights.

**Head:**
- A conv-attention temporal head on the 16×96 input (1D conv → multi-head self-attention → mean pool →
  sigmoid), exported to ONNX at an opset tract supports.
- The MLP head is kept as fallback.
- Rust only needs to load a different classifier file. The plan includes a tract operator check.

**Splitting and checkpoint selection:**
- Split by session or source group, never by window.
- Pick the checkpoint by **false/h on held-out real audio at the recall target**, not by validation loss.

**Hard-negative mining:** 2–3 rounds of train → run on long negatives → add the top false wakes with
weights → retrain.

**Compute:** head training runs on this CPU; full-scale ACAV and augmentation runs go to Kaggle T4.

### P3 — Cascade runtime (`wakeword_oww.rs`)

| Stage | Rule | Target |
|---|---|---|
| 1. KWS (new head) | Threshold from the DET curve | ~98% recall at ≤1–2 false/h |
| 1.5 Speaker check | Small ONNX speaker model (ECAPA or WeSpeaker class) replacing the weak OWW mean-pool embedding. Enrolled **automatically from your labeled live positives**. Fail-open if not enrolled. | 5–10× fewer false wakes |
| 2. Verify | Groq/STT verify only in the uncertain band (stage 1 at 0.5–0.9, or speaker score borderline). Confident and speaker-passed fires **instantly, with no cloud round trip**. Later, replaced by a local larger stage-2 acoustic verifier trained on the same data. | ≤0.1 false/h combined |

Runtime fixes in the same phase:
- Add an anti-alias low-pass before resampling.
- Honor the profile `device_name`.
- Fix the 4-channel downmix.
- Map `wakeSensitivity` to threshold offsets.
- Delete the dead confirmation and in-engine speaker code.
- Add `[WAKE-LAT]` timing for stage 1 → speaker → verify → `orb:wake` → listening.

**AEC (last part of P3).** WebRTC AEC3 (`webrtc-audio-processing` crate) fed the TTS PCM that `tts.rs`
already decodes as its reference signal. This lets "nexus" interrupt NEXUS mid-sentence without the
blanket TTS mute.

### P4 — Orb and command center alignment

1. **No lost words.** Today capture starts only after verify returns, so in "nexus open chrome" said without
   a pause, "open chrome" can be lost.
   - Start buffering at the stage-1 candidate.
   - On accept, hand the command capture the audio from just after the detected end of the wake word.
   - On reject, discard it.
2. **Orb timing.** A subtle pre-glow on the candidate, the full wake on accept, and a silent fade on reject.
   No "didn't catch that" after a borderline false wake; that clip is auto-flagged as a likely false wake.
3. **`orb:wake` payload.** Carries confidence, path (instant/verified) and latency, so the orb and the
   command center can log and act on it.
4. **Latency budget:**
   - Instant path: under 150 ms from end of word to orb.
   - Verified path: under 700 ms.
   - Listening state reached before the user starts the command.

### P5 — Continuous improvement and release gates

- Loop: review clips weekly → retrain → replay harness gate → swap the model only if it beats the current one
  on held-out recall **and** false/h.
- Optional later: over-the-air wake model updates, reusing the `nlu_update.rs` pattern.
- **Release gates:**
  - Held-out real recall ≥95% in every condition group (near, far, quiet, loud, fast, noisy, during TTS).
  - ≤0.1 false/h on ≥10 h of held-out real audio.
  - An 8 h live soak.
  - The latency budget met.

## Critical files

- **Runtime:** `src-tauri/src/wakeword_oww.rs`, `src-tauri/src/acoustic_profile.rs`,
  `src-tauri/src/commands.rs` (settings, `read_verify_wake`), new `src-tauri/src/wake_clips.rs`,
  `src-tauri/src/tts.rs` (AEC reference), `src-tauri/src/window_manager.rs` (`wake_orb_src`),
  `frontend/src/stage/orbRuntime.ts`, `src-tauri/resources/oww/`.
- **Tooling:** new `src-tauri/dev_tools/wake_replay.rs`, new `scripts/train_wake_v2.py`,
  `scripts/wake_data_foundation.py`, `scripts/record_ambient.py`, `nexus.mjs` (wake
  review/collect-live/replay), `wake_camp/` (clips and manifest to merge).
- **Docs:** `docs/research/wakeword/` (comparison and recipe), `docs/features/` (plan),
  `docs/changes/` (one entry per phase), and an AGENTS.md entry.

## Verification

- **P0:** the harness reproduces today's real-clip numbers (about 93% recall, high false wakes on the
  wake_camp negatives). Unit tests confirm the harness and runtime use the same preprocessing and threshold.
  `wake_data_foundation.py validate --require-data` passes with no speaker or session crossing splits.
- **P1:** Rust tests for the clip writer (retention, size cap, sidecar schema, nothing written when the
  setting is off or a meeting is active). A live check: 10 wakes plus 5 near-misses produce correctly hinted
  clips.
- **P2:** v2 beats the baseline on the held-out set in the harness. Report the DET curve, the per-condition
  table and false/h on 10 h of real audio. The tract load test for the new head passes.
- **P3/P4:** `cargo test --lib -- --test-threads=1` and vitest stay green. Mutation-check the cascade policy
  tests. Live: `[WAKE-LAT]` logs meet the budget; "nexus open chrome" with no pause executes; "nexus" during
  TTS interrupts with AEC on.
- **Final:** the release gates above, measured with the replay harness and an 8 h live soak, in a report under
  `docs/research/wakeword/`.

## Sources

- [Picovoice: wake word benchmark](https://picovoice.ai/blog/benchmarking-a-wake-word-detection-engine/)
- [Porcupine product page](https://picovoice.ai/products/voice/wake-word/)
- [openWakeWord](https://pypi.org/project/openwakeword/0.4.0/)
- [livekit-wakeword](https://github.com/livekit/livekit-wakeword)
- [Cascade architecture for keyword spotting (arXiv 1712.03603)](https://arxiv.org/pdf/1712.03603)
- [Apple: Personalized "Hey Siri"](https://machinelearning.apple.com/research/personalized-hey-siri)
- [Alexa cloud wake word verification](https://developer.amazon.com/docs/alexa-voice-service/enable-cloud-based-wake-word-verification.html)
- [hey-claude openWakeWord write-up](https://huggingface.co/gdiamos/hey-claude)
- [microWakeWord docs](https://docsearch.algolia.com/mcp/docs/repo/ohf-voice/micro-wake-word)
