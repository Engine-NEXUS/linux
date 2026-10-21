# P4 Rollout Rings — Runbook (2026-09-28)

Deployment plan 02, P4. Operational procedure: who, what, gates,
rollback. The SLO board (`scripts/collect_slo.py`) is the promotion
gate for every ring; no ring advances with a red board.

## Ring structure

| Ring | Who | Duration | Purpose |
|---|---|---|---|
| 0 — self | this machine | 1 week | live SLO baseline (FA/hr in real rooms, loop liveness, offline drill) |
| 1 — family | 3-5 machines (mic/driver variety: Realtek, USB, Bluetooth, Intel SST) | 1-2 weeks | hardware invariance, installer UX, family settings |
| 2 — external | 10 users | 2-4 weeks | churn-grade reliability evidence, support load |

## Ring promotion gate (all must be green)

1. `python scripts/collect_slo.py` board: parse-miss trend down,
   watchdog pokes rare + explained, 0 unexplained deaf sessions,
   0 silent quota denials, recovery restarts < 12/day sustained.
2. `cargo test --lib -- --test-threads=1` green (656/656 at freeze).
3. `cargo test --test e2e_voice_fixture` ≥ 49/50 + accent 15/15.
4. `python scripts/soak_wake_fa.py --full` < 1 FA / 8 h
   (known finding below).
5. Offline drill (`03-p31-offline-drill-walkthrough-2026-09-28.md`)
   completed without wedges on the promoting machine.
6. Installer artifact hashed and archived (§ rollback).

## Ring-1 findings (from this session's soak — act before ring 1)

1. **`necess_0003.wav` false accept (0.821 isolated, 1/500 clips).**
   Genuine in-file FA of the -xus/-cess soundalike class. Production
   `verifyWake` defaults **false** (instant-fire), so this class
   reaches the fire path today. **Ring-1 config: set
   `"verifyWake": true` in settings.json** (~250ms latency; the STT
   cross-check rejects "necessity" ≠ "nexus"). Also added to the
   next-retrain hard-negative list (soak_wake_fa.py now prints
   TRIGGER lines — promote the file list into
   `wake_word_data/negative/` weighting on the next
   `train_local_wakeword.py` run).
2. **Harness fix landed:** the soak previously manufactured one
   cross-file trigger by carrying wake embeddings across dataset
   clips (hey_0007 stitched from the previous file; isolated peak
   <0.09). Per-file fresh state is now the default (production-like:
   independent utterances + `reset_after_trigger`); `--stream`
   remains for continuity experiments.
3. **verifyWake decision recorded.** The instant-fire latency win
   (2026-09-25) traded away the soundalike net. Ring 1 re-enables it;
   re-evaluate after the retrain lands.

## Per-machine checklist (rings 1-2)

- Install ring artifact → run `nexus check` (bridge section live).
- Say the 10 smoke commands; note any miss into
  `missed_intents.jsonl` (automatic) — weekly `collect_slo.py`.
- Mic profile note: device name + driver (Device Manager).
- Any failure: capture `nexus_unified.log` tail + the
  `debug_trace` p0-p4 lines (temporary taps, still armed).

## Ring-1 artifact (built 2026-09-28, this freeze)

| Artifact | SHA-256 | Size |
|---|---|---|
| `NEXUS_0.1.0_x64_en-US.msi` (bundle/msi) | `494C1E74507912EDD856867C79849D0696970476F5C6656595DE037C8E8CFC78` | 201.7 MB |
| `nexus.exe` (release, custom-protocol + admin-brain) | `3A09C75CCEC6F0011151F39D1D22F554006FDD6260B89F3CF5477E8DEF722039` | 50.7 MB |

Contains everything through P3 + the P2 wake-prefix/ghost-routing
fixes + P1 watchdog. NSIS bundling is currently broken in this
environment (`RestartManager_StartSession` macro missing — NSIS
plugin cache; CI's windows-installer job is unaffected) — MSI is
the ring-1 artifact until the local plugin cache is fixed.

## Rollback

- Ring artifact = `nexus_<ring>_<date>.exe` (NSIS) archived with its
  SHA-256 next to this doc's companion in the release notes.
- Rollback = install previous archive. Settings/API keys/keychain
  survive (installer never wipes `%APPDATA%/com.nexus.assistant`).
- Model snapshot: `src-tauri/resources/oww/nexus.onnx` +
  `model_manifest.json` are versioned in git — a bad retrain rolls
  back by restoring the resources pair and rebuilding.

## Non-goals (unchanged)

Vision autonomy promises, unsupervised long-horizon compounds,
non-English. Any request landing in these routes to the tiered
promise (paper 01 §7).
