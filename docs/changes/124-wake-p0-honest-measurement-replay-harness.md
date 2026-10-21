# 124 - Wake word P0: honest measurement (replay harness, session-split manifest, baseline)

Date: 2026-10-11. Phase P0 of `docs/features/107-wake-word-production-plan.md`. Research and sources:
`docs/research/wakeword/industry-comparison-and-recipe-2026-10-11.md`.

## Why

Every published wake number so far (98.6% recall, 0 false alarms in 186, and so on) was scored on the model's
own training files, and the trainer split by 80 ms window. Before changing any model we needed a measurement
we can trust.

## Built

### Engine (`src-tauri/src/wakeword_oww.rs`)

No behavior change live, except two bug fixes (last two items).

- **`stage1_step` and `detection_score`.** Pure functions now hold the smoothing buffer, score and refractory
  logic. The live engine and the replay harness both call them, so they cannot drift.
- **Replay clock.** `now_ms()` returns the wall clock live (unchanged) and audio time in replay mode.
  - Live has to stay on the wall clock: the engine is skipped while a command is captured, and that time must
    still count toward the refractory.
  - New methods: `restart_grace()` and `reset_for_replay()`.
  - New fields: `last_raw_prob` and `last_chunk_scored`.
- **`WakeEngine::with_classifier(oww_dir, onnx, data_dir)`** scores candidate models without replacing
  `nexus.onnx`.
- **Named constants:** `STARTUP_GRACE_MS`, `AGC_TARGET_RMS`, `AGC_MAX_GAIN`.
- **Fix: first grace reset was a no-op.** It went through `WAKE_ENGINE_GLOBAL` before that global was set.
  It is now a direct call before the stream starts.
- **Fix: logs said "10s grace"; the code does 1.5 s.** The logs now print the constant.

### Replay harness

- **`src-tauri/src/wake_eval.rs`:** WAV decoder (PCM 8/16/24/32, float, any channel count), the runtime's
  linear resampler, clip padding, `ReplayEngine`, `simulate_fires`, threshold sweep, per-condition recall,
  misses, and false-fire offenders (the hard-negative list).
  - Short negative clips (≤30 s) are counted ("fired or not").
  - Only continuous recordings give false fires per hour.
- **`src-tauri/dev_tools/wake_replay.rs`**, run as `nexus wake replay [--split test|train|validation]
  [--model x.onnx] [--json out]`. It is the source of truth; the Python testers re-implement the pipeline.

### Data

- **`scripts/build_wake_manifest.py`** writes `wake_word_data/manifest.jsonl`. The unit of separation is a
  whole owner session, TTS voice or noise generator.
  - Test: the 26 Sep wake_camp session and the 18 Sep real background recordings. Neither was ever in the
    training folders; checked by audio hash.
  - Validation: owner session 23 Sep and two held-out TTS voices.
  - The 10 unlabeled confusion clips go to `needs_review.jsonl`.
- **`scripts/wake_data_foundation.py`:**
  - `owner` and `room` may span splits; this is a personal model, so separation is by session.
  - The model manifest's stale `runtime` block (threshold 0.35, 10 s grace, gain 20) is replaced with the
    engine's real constants. The threshold now points at the acoustic profile.
  - Fingerprints re-recorded. The committed `nexus.onnx` is `622afbfc…`; the manifest still listed the doc-71
    model `2de9887f…`, so it had been replaced without being recorded.
  - A Rust test now pins the manifest's runtime block to the engine constants.
- **`scripts/record_wake_ambient.py`:** records real ambient audio on this mic at its native rate, in 60 s
  chunks, tagged by category (tv, music, conversation, hindi, tts-echo …). Rows go into the test split.
  `--list` shows hours per category.
- **`scripts/wake_review.py`:** P1 review tool. It plays each live clip and asks y/n/replay/skip/delete,
  moves the clip into `real/live-<day>/`, and appends a manifest row. Each day goes wholly to one split.
  The Rust clip writer it reads from is the next step.
- **`nexus.mjs`:** adds `nexus wake replay`. `nexus wake devices` was broken (it passed a flag the script
  doesn't have); it now runs `test_device_invariance.py`. The `test_wake_live.py --threshold` help said 0.35
  while the code uses 0.68; fixed.

## Baseline: shipped model, stage 1 only, profile threshold 0.68

| Set | Recall | Short negatives fired | Stream false fires/h |
|---|---|---|---|
| **Held-out owner session** (152 positives, 15 soundalikes, 0.42 h real background) | **63.2%** | **10/15** | **12.0** (0.42 h only; unreliable) |
| Training split (what old reports measured) | 93.8% (TTS 99.2%, owner 85.9%) | 58/2,989 | no streams |

Held-out recall by condition at 0.68:

| near_normal | far | fast | loud | quiet | slow |
|---|---|---|---|---|---|
| 37/56 | 15/25 | 16/21 | 16/28 | 3/10 | 5/7 |

Lowering the threshold doesn't rescue it: 0.35 gives 77.6% recall with 12 of 15 soundalikes firing.

What this says:
- **The model learned TTS voices, not you.** It scores 99% on TTS and 63% on a new session of your voice.
- **The training negatives are too easy.** All 1,617 procedural noise clips scored 0 fires; only spoken
  words fire, at 49 of 563.
- **This matches the plan.** P2 (real data, RIR, real noise, conv-attention head, selection by false/h) and
  P3 (speaker check, verify in the uncertain band) are where the gains are.
- **Limits of this baseline.** Stage-2 transcript verify is not modelled. In the live app it removes many of
  these false fires, but it cannot recover misses.

Reports: `wake_word_data/replay_baseline_{test,train}.{txt,json}` (local, git-ignored).

## Verify

- `cargo test --lib wake_eval` 12/12, including `replay_matches_live_engine_decisions`. That test is
  non-vacuous (it asserts at least one fire) and mutation-checked: shifting frames by one chunk fails it.
- `cargo check --features custom-protocol,admin-brain` clean.
- `wake_data_foundation.py validate --require-data` passes.
- Full lib suite (`--test-threads=1 --skip tts_bench --skip tts_kokoro`): 1331 passed, 0 failed, 9 ignored.

**Not run live:** the engine changes in the running app. The clock is unchanged live, but restart and
recovery grace should be watched in the next `nexus start`.

## Next (needs you)

1. Record ambient audio, aiming at ≥10 h over several days:
   `python scripts/record_wake_ambient.py --category tv --minutes 60`, and likewise youtube, music,
   conversation, hindi, typing, tts-echo.
2. Label the 10 clips in `wake_word_data/needs_review.jsonl`.
3. P1: Rust clip capture (`wakeClipCapture`, off by default) feeding `wake_review.py`.

## Pitfalls

- Run the harness with `--release`; debug is ~20× slower.
- Never put wake_camp 26 Sep or 18 Sep audio into training: it is the test set.
- `build_wake_manifest.py` keeps `origin: live|ambient` rows; everything else is regenerated.
