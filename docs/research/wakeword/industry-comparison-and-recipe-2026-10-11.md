# Wake word: how the leaders reach production accuracy, and what NEXUS copies (2026-10-11)

Companion to `docs/features/107-wake-word-production-plan.md`.

## Published numbers (and their limits)

| System | Published result | Notes |
|---|---|---|
| Porcupine (Picovoice) | 97.1–97.3% recall at 1 false alarm per 10 h, 10 dB SNR | Vendor benchmark; 300+ recordings of 6 keywords incl. "jarvis", "alexa" |
| openWakeWord stock models | Goal: <5% false rejects at <0.5 false accepts/h | Project target with threshold tuning; no per-keyword table |
| openWakeWord (third-party test) | 8.5 false positives/h, 69% recall | Measured by LiveKit on its own "hey livekit" set |
| livekit-wakeword conv-attention | **0.08 false positives/h at 86.1% recall**, threshold 0.68 | Same set: 15,000 positives, 45,084 negatives, 25 h audio |
| Alexa, Siri, Google | Not published | Designs are published; numeric targets are internal |
| Consumer rule of thumb | About 0.1 false accepts/h at low single-digit false rejects | Vendor guidance (Sensory), not an Amazon/Apple figure |

These numbers come from different datasets, noise conditions and keywords. None of them ranks one engine
against another. The only meaningful comparison for NEXUS is our own replay harness on our own held-out
recordings.

## Methods that matter

1. **Cascade (Alexa, Google).** A small always-on detector tuned to wake "a handful of times per hour" on
   continuous speech, then a bigger verifier. Alexa's cloud verifier sends StopCapture on reject. The Google
   cascade paper trains both stages on the same data with similar miss rates, so the combined miss rate stays
   close to the second stage alone.
2. **Speaker check after detection (Siri).** A personalized speaker-verification step runs after the
   speaker-independent "Hey Siri" detector. The cascade paper reports speaker verification cutting false
   accepts **5–10×** for under 1% extra misses. Apple enrolls from real use.
3. **Same front end as ours, better head (livekit-wakeword).** It uses openWakeWord's frozen mel and
   embedding models, giving a (16, 96) feature matrix. That is exactly what `wakeword_oww.rs` computes. It
   swaps the flat MLP for 1D temporal convolutions plus multi-head self-attention and mean pooling. On the
   same validation set: 8.5 → 0.08 false/h.
4. **Real ambient negatives.**
   - openWakeWord ships precomputed ACAV100M negative features (~2,000 h, ~17 GB) in the same embedding space.
   - One public custom-model write-up ("hey-claude") found that synthetic noise cut false wakes from ~4,000/h
     to ~90/h, and real-world audio brought them to zero. Its quiet-room failures came from preamp hiss, hum
     and fan rumble, not digital silence.
   - It used real MIT room impulse responses.
5. **Checkpoint selection by false/h (microWakeWord).** Pick the checkpoint by false accepts per hour on real
   ambient audio (music, household, dinner party) first, then accuracy. Do not pick by validation loss. A
   balanced test split that looks good can still fire every few seconds in production.

## Why "nexus" is harder than "hey jarvis"

"Nexus" has two syllables and ends in a sibilant. That sibilant is shared with very common words (next,
access, process, -tion endings). "Hey jarvis", "hey siri" and "alexa" have three or four syllables. A longer
phoneme sequence gives the detector more evidence and a natural rejection margin. Closing the gap needs the
cascade (speaker check and verify) and real data. A better model alone will not do it. "Hey nexus" stays an
accepted variant.

## What NEXUS had (2026-10-10 audit)

- **Headline numbers came from training data.**
  - `verify_hardened_model.py` and `soak_wake_fa.py` score the training folders.
  - The trainer split by window, so clip and augmented copies landed on both sides.
- **No RIR augmentation and no random-SNR mixing.** 900 of 997 background clips are procedural, and an
  alphabetical `[:300]` cut drops most of them.
- **No capture of real fires or near-misses.** Only 5 rotating debug WAVs exist.
- **Python testers re-implement the pipeline** and have drifted from the Rust engine.

## Recipe adopted (summary; phases in doc 107)

1. **P0** measure honestly: a Rust replay harness over session-separated held-out data, plus ≥10 h of real
   ambient audio on this mic.
2. **P1** opt-in local capture of fires and near-misses, labeled in a review tool.
3. **P2** new recipe:
   - real positives weighted up
   - RIR, real noise at random SNR, speed/pitch, NEXUS's own TTS under the wake word
   - ACAV100M negatives and mined hard negatives
   - conv-attention head on the same 16×96 features
   - checkpoint chosen by false/h at the recall target
4. **P3** cascade: stage 1 → speaker check (ECAPA/WeSpeaker class, auto-enrolled from labeled live
   positives) → transcript verify only in the uncertain band. Add AEC for barge-in.
5. **P4** align with the orb and command center: no lost words after "nexus", pre-glow on candidate, a
   latency budget.
6. **P5** gates: ≥95% recall per condition and ≤0.1 false/h on ≥10 h held-out real audio, plus an 8 h live
   soak.

## Sources

- [Picovoice: benchmarking a wake word engine](https://picovoice.ai/blog/benchmarking-a-wake-word-detection-engine/)
- [Porcupine product page](https://picovoice.ai/products/voice/wake-word/)
- [openWakeWord on PyPI](https://pypi.org/project/openwakeword/0.4.0/)
- [livekit-wakeword](https://github.com/livekit/livekit-wakeword) and [blog](https://livekit.com/blog/livekit-wakeword)
- [A Cascade Architecture for Keyword Spotting on Mobile Devices (arXiv 1712.03603)](https://arxiv.org/pdf/1712.03603)
- [Apple: Personalized "Hey Siri"](https://machinelearning.apple.com/research/personalized-hey-siri)
- [Alexa: cloud-based wake word verification](https://developer.amazon.com/docs/alexa-voice-service/enable-cloud-based-wake-word-verification.html)
- [hey-claude openWakeWord model card](https://huggingface.co/gdiamos/hey-claude)
- [microWakeWord documentation](https://docsearch.algolia.com/mcp/docs/repo/ohf-voice/micro-wake-word)
