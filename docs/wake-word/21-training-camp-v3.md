# Training Camp v3 — Real-Voice Dataset Collection for `hey_nexus` / `nexus` Classifiers

> **Date:** 2026-09-16 (session 1, ongoing — paused for rest, resume pending)
> **Goal:** replace/augment the synthetic-only v2 training data with real-voice clips
> from the primary user, retrain on Kaggle, and ship classifiers that approach
> Alexa-grade reliability (recall ≥90% at ≤0.5 FA/hr, §2 targets).
> **Method:** interactive prompted-recording loop with instant QC, per-round model
> cross-checks, and envelope-based verification of imperfect clips.
> **Status:** collection phase — 131 train clips + 5 sealed heldout clips banked.
> Training phase not yet started.

---

## 1. Dual-gate verification protocol (mandatory)

Every phase below advances **only after passing TWO independent verification runs**
(Test A and Test B). A single passing run is treated as anecdote; two consecutive
passes constitute evidence. If Test B fails after Test A passed, the phase restarts
— the discrepancy itself is logged as a finding.

| Phase | Test A | Test B | Advance criteria |
|---|---|---|---|
| P0 audit+pack | Manifest-vs-disk reconciliation script (counts, paths, verdicts) | Independent re-run + manual spot-check of 5 random entries (audio exists, metadata sane) | Both runs: 0 orphans, 0 stale paths, counts match §3 |
| P1 train | Kaggle val metrics at final checkpoint meet ship thresholds | Re-run eval on a FRESH random seed / held-out slice the training run never saw | Both: recall ≥90%, FP/hr ≤0.5 on val; no NaN/stall in logs |
| P2 eval (honest) | Sealed heldout recall (clips in `wake_camp/heldout/`, never trained on) | Background FA test (Block E audio): 0 false wakes on room tone + TV/conversation | Both must pass; EITHER failure returns to P1 with new failure-mode data |
| P3 gap collection | New clips pass the camp QC gates (§5) | New clips flip the specific P2 failures when added to a eval-only probe set | Both: gates pass AND previously-failing heldout items now wake |
| P4 ship | Staged model scores ≥ P2 thresholds via the Rust harness (exact live pipeline incl. Phase-B preprocessor) | 24-hour live soak: user runs NEXUS normally, FA/hr counted from wake log | Both: thresholds hold AND soak FA/hr ≤0.5 |

Rationale for dual-gating: the v2 program's "0.994 validation" rested on a single
unrepeatable script (gitignored, since lost — see §9). No single-run claims ship again.

---

## 2. Operating targets (v3 exit criteria)

| Metric | v2 measured | v3 target | Alexa-class reference |
|---|---|---|---|
| Recall (heldout real voice) | ~0% on hey_nexus via replica scorer (§9 — disputed, under investigation) | ≥90% | ~95–97% |
| False alarms | 1.33/hr (reported) | ≤0.5/hr (≤0.1/hr with speaker gating, future) | ~0.006–0.03/hr cascaded |
| Eval negatives | 20 samples | hours (Block E + MUSAN/DEMAND in training) | 900+ hrs |

---

## 3. Dataset inventory (end of session 1)

131 train ACCEPTs + 5 sealed heldout ACCEPTs. Source of truth: `wake_camp/manifest.jsonl`
(167 total entries; non-accepts retained with SCRAP/RETRY verdicts + reasons for audit).

### 3.1 Accepted train clips by phrase/condition

| Phrase | near_normal | far (~3m) | fast | quiet | slow | loud | Subtotal |
|---|---|---|---|---|---|---|---|
| hey_nexus | 25 | 15 | 21 | 10 | 10 | 10 | 91 |
| nexus | 20 | 10 | — | — | — | 10 | 40 |
| **Total** | | | | | | | **131** |

### 3.2 Heldout (sealed, NEVER train)

`BASE01–BASE05` — hey_nexus/near_normal, RMS 0.039–0.106, spans 0.68–1.6s.
Sealed before any training use. P2 Test A runs exclusively on these (+20% clip
holdouts to be carved at pack time per the session plan).

### 3.3 Still to collect (Phase P3 or later)

| Block | Spec | Status |
|---|---|---|
| ok_nexus (Block C) | near 20, far 10, loud/quiet 5 each (40) | not started |
| Soundalikes (Block D) | lexus/texas/next-us/heynux/"nexus of…"/hey-alexa/hey-siri (40) | not started |
| Background (Block E) | room tone, TV, conversation (20) | not started — REQUIRED for P2 Test B |
| Enrollment (Block F) | hey_nexus ×5 close-mic | not started — needed for speaker-gating (P2 verification roadmap) |

Per the train-first revision (§10), B/C/D/E/F are scoped by P2 eval evidence, not
collected blindly — except Block E, which is prerequisite infrastructure for any
honest FA measurement.

---

## 4. Session chronicle (2026-09-16)

| # | Event | Outcome |
|---|---|---|
| 0 | Prep (no mic): folder tree, `record_round.py` + QC gates, manifest/log scaffolding, dep check (numpy/scipy/sounddevice/onnxruntime ✅), model files present, no local GPU (→ Kaggle venue) | staged, mic untouched |
| 1 | Mic archaeology: MME+WASAPI flatline while Settings test heard 4% → privacy page check (desktop access ON, Python allowed) → no rogue mic-holders → Intel SST dropout diagnosed | root-caused to driver, not permissions |
| 2 | Driver revived via Windows mic test + louder speech; mic-check passed | Round 0 unblocked |
| 3 | Round 0 baseline: BASE01–05 ("hey nexus") with timing misses (0.14–0.28s fragments), 2 SST dropouts mid-round, mic re-checks | 5/5 ACCEPT, sealed to heldout |
| 4 | Scorer built (`score_wake.py`): exact Rust-pipeline replica. Three successive bugs found+fixed: (a) int16-scaling saturation, (b) missing mel normalization `(v/10)+2` (`wakeword_oww.rs:518`), (c) wrong mel framing (5 vs 8 frames/chunk; lookback-480; slice [4:80]) | replica now element-identical to Rust (modulo tract-vs-ORT numerics + Phase-B preprocessor, §9) |
| 5 | Replica scores ~0.000 on ALL user clips AND SAPI "nexus"/sentence → logged as open investigation (§9), session continued (data unaffected) | data-first decision |
| 6 | Block A round 1 (A01–A10): 8/10, median RMS 0.05 | voice fresh |
| 7 | Block A round 2 (A11–A20): 2/10, RMS 0.05→0.015 — rushing + missed windows | **break called per protocol** |
| 8 | Makeup M01–M10: 1/10 — uniform 0.99 peaks, 0.4s spans (barking). Calibration C01–C03: 0/3, same signature | coaching not converging |
| 9 | Waveform forensics (C03 vs A02): peaky crest 10.6 vs 7.2, no hard clipping → over-projection, not distortion. Span gate (5%-of-peak) shown to amputate peaky speech → **dual-threshold span gate** implemented | QC fix #1 |
| 10 | Re-QC: 1/12 recovered (M08→loud). 11 brisk clips envelope-verified (two-lobe + natural decay) → reclassed near_normal→fast with flags | fast block seeded |
| 11 | N01–N10: 0/10, same bark signature → decay-verified → fast (fast block now 22) | pivot decision |
| 12 | **Pivot**: stop demanding slower speech; engineer conditions instead. Far-field first (distance regulates gain/pace mechanically) | plan change, logged |
| 13 | Far F01–F10: 1/10 under 0.6s floor — but spans cluster 0.44–0.52 (speaker's TRUE natural pace) → **floor 0.6→0.35s** (all observed fragments were ≤0.3s) | QC fix #2 |
| 14 | Re-QC far: 9/9 flipped to ACCEPT. F11–F15: 5/5 | far COMPLETE 15/15 |
| 15 | Quiet Q01–Q10: 10/10. Slow S01–S10: 10/10 | both COMPLETE |
| 16 | W01–W10: 10/10 (W01–W05 hot → loud). V01–V09: 9/9 | near_normal COMPLETE 25/25 |
| 17 | L01–L04: 4/4 | loud 6/10 → 10/10 with L-round (hey_nexus COMPLETE: 91) |
| 18 | Block B near BN01–BN20: 20/20 (single-word floor 0.25s added). Far BF01–BF10: 9/10 + makeup → 10/10 | B-near, B-far COMPLETE |
| 19 | Post-break SST hard dropout: toggle failed, service restart (non-elevated) failed, MME+WASAPI zeros while Settings heard 3% → **reboot** (all data verified safe: 125 accepts at the time) | session suspended |
| 20 | Post-reboot: still dead → WASAPI woke on attempt 2 (0.013) →BV round exposed **drain bug** (rate-unaware drain left ~4s stale audio → tail fragments) → **rate-aware drain fix** | QC fix #3 |
| 21 | BX makeup 5/8, BY 4/4 → nexus loud COMPLETE 10/10. **Session total: 131 train + 5 heldout** | collection paused for rest |

---

## 5. QC system evolution (all changes + rationale)

### 5.1 Initial gates (round 0)
- RMS floor 0.005 (reject dropouts), clip-peak retry (>0.98 for >5ms),
  speech-span 0.6–2.5s via 20ms frames at 5%-of-peak energy threshold.

### 5.2 Fix #1 — dual-threshold span (after event 9)
Single relative gate amputated peaky-but-valid speech (a transient spike at 0.99
pushed the gate to 0.05, excluding the speech body). Now: span = max(span@5%-of-peak,
span@0.02-absolute). Strictly more correct; fragments (low absolute energy) still fail.

### 5.3 Fix #2 — floor 0.6s → 0.35s (after event 13)
Evidence: speaker's genuine natural pace is ~0.45s ("hey nexus") / ~0.5s ("nexus");
every confirmed fragment across 100+ takes measured ≤0.3s. The 0.6s floor was
rejecting the user's real voice. Single-word blocks use 0.25s (`--min-span`),
validated the same way (fragments ≤0.2s there).

### 5.4 Fix #3 — rate-aware drain (after event 20)
`drain()` budgeted in 16kHz units while a 48kHz WASAPI stream accumulates 3× faster;
~4s of stale audio sat ahead of fresh speech, producing tail-fragment takes.
Drain now uses the stream's own samplerate. (Applies to WASAPI path; MME unaffected.)

### 5.5 Per-take protocol (unchanged throughout)
Beep (880Hz/180ms) → 3s capture → instant QC → ACCEPT / RETRY-once / SCRAP.
SCRAP slots are never reused (fresh makeup IDs) to keep the manifest append-only.
3s breathing gaps; 2-min rests between blocks; QC-drift breaks per §5.6.

### 5.6 Break triggers (hit twice: events 7, 19)
Accept-rate drop >15% vs session average, median-RMS drift, or 2 consecutive
scraps on previously-nailed material. Plus human-called breaks (respected as data).

---

## 6. Reclassification log (every moved clip + evidence standard)

Standard of evidence for reclass without user listening (user couldn't play back
audio): (1) sustained high RMS across full span (not fading tails), (2) tight
duration clustering within the batch, (3) natural decay to digital zero at both
ends (cutoff fragments end mid-energy). M05 was envelope-plotted as the exemplar
(two lobes + full decay); batch-mates verified by decay + clustering. All carry
`acoustic-verified` (or `inferred`) flags in the manifest — excludable at pack time.

| Clips | From → To | Basis |
|---|---|---|
| M02–M07, M09, M10, C01–C03 (11) | near_normal → fast | brisk ~0.45s verified completes; fast block needed exactly this |
| N01–N10 (10) | near_normal → fast | same signature, decay-verified batch |
| M08 | near_normal → loud | peak 0.99 + re-QC accept; matches loud profile |
| W01–W05 (5) | near_normal → loud | peaks pinned >0.97; gain-reclass |
| F01, F03–F10 (9) | SCRAP → ACCEPT (far) | floor fix #2; no move needed |

---

## 7. Tooling built (`wake_camp/`)

| File | Purpose |
|---|---|
| `record_round.py` | Prompted recorder: mic-check (SST-dropout aware), beep→capture→QC, auto-retry, `--round/--clip/--background/--check`, `--min-span`, `--tries`, `--wasapi`, `--device`, persistent-stream + rate-aware drain, manifest+session logging |
| `score_wake.py` | Offline replica of the Rust oWW pipeline (chunking, lookback-480, int16-scale, AGC, gates, 8-frame mel + `(v/10)+2`, 76-frame window, 16-embedding classifier) for round cross-checks |
| `reqc.py` | Re-QC saved wavs under current gates; auto-reclass peak>0.85 passers to `loud/` |
| `flip_accept.py` | Flip SCRAP/RETRY→ACCEPT in manifest after gate fixes (never the reverse) |
| `move_fast.py` | Reclass file+manifest near_normal→fast with evidence notes |
| `fix_m08.py` | One-off manifest correction (reqc moved the file but not the entry — process gap, closed) |
| `pad_silence.py`, `to16k.py` | Clip padding (buffer-fill for scoring) + SAPI resampling |
| `gen_sapi.ps1`, `gen_sapi2.ps1` | SAPI TTS synthesis for scorer validation |
| `manifest.jsonl` | Append-only source of truth: clip_id, phrase, condition, path, verdict, reason, metrics, split |
| `session_2026-09-16.log` | Human-readable event log (rounds, decisions, investigations) |

---

## 8. Intel SST dropout — field findings (for `wakeword_oww.rs` silence-recovery work)

Observed driver behaviors this session (all on Intel SST mic array, driver 10.29.0.11192):

1. **Burst-then-flatline**: after open, brief audio (~0.5–1s) then digital zeros (RMS 1e-5). Matches the documented bursty profile.
2. **Idle sleep**: any 2–5 min gap without capture risks full dropout; first post-idle open usually returns zeros.
3. **Sound wakes the driver**: probes pass when the room has sound (user humming/counting); silent-room probes fail. Warm-up loop (`--tries 8–12` + continuous humming) is the reliable revival.
4. **Toggle ladder**: Device-Manager disable→5s→enable revived it twice; failed twice (later states needed reboot, then post-reboot still dead until WASAPI attempt 2).
5. **API-independent when wedged**: MME and WASAPI both returned zeros simultaneously; Settings self-test still heard 3% (separate path).
6. **Power-gating suspect**: "Allow the computer to turn off this device" unchecked mid-session (effect unverified — dropout recurred; needs longitudinal check).
7. **Non-elevated shells cannot restart Audiosrv** (`Restart-Service` access-denied) — recovery tooling must either request elevation or live with toggle/reboot.
8. **Recorder implication**: per-clip open/close maximizes cold-open lotteries; a process-lifetime persistent stream + warm-up retries is strictly superior (implemented §7). The Rust wake engine's always-open cpal stream is the right architecture — this session is independent confirmation.

---

## 9. Open investigations

### 9.1 Scorer-vs-live (~0.000 on SAPI "nexus")
The replica (§4 event 4–5, §7 `score_wake.py`) is element-identical to
`wakeword_oww.rs` except: tract-vs-ORT numerics and the **Phase-B preprocessor**
(80Hz high-pass + adaptive noise floor + VAD, `wakeword_oww.rs:231–289, 770–779`),
which is not yet replicated. Yet it scores ~0.000 even on SAPI "nexus" — the same
*class* of audio v2 was validated at 0.994 on. Candidate explanations, unranked:
(a) missing preprocessor replication, (b) model-file drift since August (the
0.994 script is gitignored and gone; repo has NO positive-audio test — only
`test_silence_never_triggers_wake`), (c) SAPI-vs-Piper voice gap larger than assumed.
**Resolution path**: Rust harness test feeding SAPI + user wavs through the real
`detect_chunk` (preprocessor included); if Rust also scores ~0, the model file is
suspect and v3 retraining becomes a replacement, not a refresh. Either way the
collected data is the fix.

### 9.2 A02 trailing energy
Accepted clip A02 shows above-threshold energy at the file tail (post-phrase noise —
chair/movement). Harmless at this scale; flagged for the P0 audit spot-check.

---

## 10. Train-first revision (supersedes blanket collection)

Adopted at user's proposal mid-session: with 131 real clips banked, **train before
collecting further**. P2 eval evidence scopes all remaining collection (ok_nexus,
soundalikes, enrollment; Block E background is prerequisite infrastructure for P2
Test B and proceeds regardless). Rationale: real voice is the bottleneck, GPU time
is not, and failure-mode-targeted data beats speculative volume.

### Kaggle recipe (P1 setup)
- Base: v3 notebook lineage (v2 config: 5 phrase variants → now real-clip-driven).
- Real clips augmented ×20 (speed ±10%, pitch ±3 semitones, MUSAN/DEMAND noise,
  MIT-RIR reverb already in camp) → ~2,600 real-voice variants inside the 50k
  synthetic stream. Synthetic = bulk, real = accent.
- Heads: new `hey_nexus.onnx` (primary) + refreshed `nexus.onnx` (fallback).
- Watch rules: log loss/acc/recall/FP-hr per checkpoint; stall (no gain 10k steps)
  → stop + inspect; regression vs previous checkpoint → quarantine checkpoint.
- Ship gate: §1 P1 row (dual-gate).

### P0 audit result (2026-09-16, post-collection)
- **Test A PASS**: `p0_audit.py` achieves 0 errors after `p0_fix.py`
  (dedupe 167→158 take-history rows; patched 5 stale W01–W05 paths left behind
  by the gain-reclass, which had updated condition/verdict but not path).
- **Test B PASS**: 5-clip spot-check (BASE02, M05, F07, BN14, Q04) — files valid
  16kHz/3.0s, recomputed RMS matches manifest to 1e-4.
- Lesson encoded: single-clip mode now honors `--min-span` (BF01 was misjudged
  at 0.35 before the fix); every reclass script must update path+condition+verdict
  atomically (`fix_m08.py` closed the reqc gap the same way).

### Kaggle log forensics — v6→v20 failure chain (2026-09-15/16)
Prior runs used kernel `chitkullakshya/nexus-wakeword-training` (GPU, T4) with the
50-clip `nexus-real-samples` dataset. All versions failed with
`ERROR: Model not found`; the failure mode evolved across versions, indicating
active iteration: missing feature dirs (v14) → 0-negative batches (v15) →
`train.py` line shifts 751→839→852 (v16–v19, augmentation/feature stages passing
further each time) → **`KeyError: 'ACAV100M_sample'` in `mmap_batch_generator`
(`data.py:827`) at v20**, i.e. training loop reached, background sampler crashed.
- Root cause class: **config key mismatch** — `batch_n_per_class` references a key
  absent from `feature_data_files`/`shapes`. Mechanically, any consistent key works
  (`train.py:839–857` builds transforms from the same dict).
- Critical finding: **repo ↔ kernel divergence.** `scripts/kaggle_kernel/
  nexus_wakeword_training.ipynb` uses `'ACAV100M_sample'` consistently in both
  dicts (no mismatch), so the running v20 kernel is NOT this file — it was edited
  on kaggle.com (versions v6→v20) without syncing back. The repo copy is stale.
- v20 root cause CONFIRMED from the run's own stdout (log line t=688s):
  that run used `feature_data_files: {'ACAV100M': ...}` (upstream-template key,
  no suffix) while `batch_n_per_class` used `'ACAV100M_sample'` → KeyError at
  first background batch. The live kernel pulled 2026-09-16 22:53 shows
  `'ACAV100M_sample'` in both dicts (cell-identical to repo) — i.e. consistent,
  so the mismatch was introduced and later reverted in un-synced Kaggle-side
  edits. Mechanically any consistent key works (`train.py:839-857` derives
  transforms from the same dict); correctness does not depend on the suffix.
- Fix workflow (blocked on Kaggle API key, §12): pull live kernel → align the two
  keys → add `hey_nexus` head + 131 camp clips + new dataset version → push → run
  → per-checkpoint log watch. Also: no API key exists anywhere in the codebase
  (searched `KAGGLE_KEY`, `KAGGLE_USERNAME`, `kaggle.json` — zero hits).

## 12. Blocked: Kaggle API key (2026-09-16)
No Kaggle credentials exist in the codebase or environment. To pull/push/run the
kernel programmatically, place the key at `%USERPROFILE%\.kaggle\kaggle.json`
(from kaggle.com → Settings → API → Create New Token) and confirm — never commit
it (ensure `.kaggle/` stays gitignored). Until then: pack build proceeds locally;
upload/push/run wait.

---

## 11. Risks

1. **Scorer-live divergence (§9.1)** could mean the Rust pipeline itself under-scores
   real speech — retraining on data scored by a broken ruler compounds the error.
   Mitigated by P4 Test A (Rust-harness eval before ship).
2. **Single-speaker overfit**: 131 clips, one voice. Mitigated by synthetic bulk +
   augmentation; family-voice extension is future work, not a v3 blocker.
3. **Brisk-pace bias**: speaker's 0.45s pace dominates; slow block (0.5–0.9s) only
   partially offsets. Watch P2 recall on slow heldout; collect slow round 2 if needed.
4. **SST dropout during future rounds**: warm-up protocol + persistent stream handle
   it; NEXUS's own always-open stream is unaffected by camp tooling.

---

*Next update: P0 audit results + Kaggle pack manifest. This file is the living record;
append, don't rewrite.*

## 13. P1 execution — v22 run (2026-09-16/17, overnight)
- Pack: `wake_camp/pack_v3/` — 131 camp ACCEPT/train clips (91 hey_nexus + 40 nexus,
  per-condition dirs + `pack_manifest.json` with sha256) + 50 legacy clips, pushed as
  new private version of `chitkullakshya/nexus-real-samples` (`--dir-mode zip`:
  `camp.zip` + `old.zip` — NOTE: first push without `--dir-mode` uploaded only the
  manifest; folders were skipped. Always use `--dir-mode zip`).
- Kernel: `chitkullakshya/nexus-wakeword-training` pushed as **v22** (source =
  live pull, keys consistent `'ACAV100M_sample'` both sides; dual-phrase
  `target_phrase=['nexus','hey nexus']` single model retained per minimal-diff —
  per-phrase split only if eval demands). Push tooling notes: CLI `push -p`
  needs absolute paths; `code_file` must match disk filename exactly
  (metadata said underscores, pull produced hyphens — renamed to underscores).
- Key storage: `~/.kaggle/access_token` (+ `kaggle.json` backup), user-only ACL,
  outside repo. CLI 2.2.4 requires the new token format.
- Watch: `kaggle kernels status` polling (live per-checkpoint streaming is not
  exposed by the CLI mid-run; website shows live logs). Dual-gate watch rules
  from §10 apply at completion: P1 Test A (final metrics) + Test B (fresh-seed
  re-eval) before P2 honest eval on sealed heldout.
- **v22 outcome (FAILED at 95%)**: KeyError fix verified — data gen (2000/200),
  5-round augment, features, and the full 100k-step training loop completed
  (~18 min train). Crash at ONNX export: `ModuleNotFoundError: No module named
  'onnxscript'` (newer torch requires it; not in setup cell). No model written.
  SECOND finding from v22 stdout: only **20/181** real clips entered training —
  the upload cell `break`s after the first wav directory
  (`camp/nexus/near_normal`). Both fixed in v23 (`patch_kernel_v23.py`):
  (1) `!pip install -q onnxscript` in setup, (2) break removed → recursive copy
  (flat names verified collision-free across camp/old sets).
- **v23 VOID (process failure, discarded from training record):** the pushed code
  was byte-identical original — `patch_kernel_v23.py` iterated `cell["source"]`
  assuming list-of-lines, but nbformat allows single-string sources; the char
  loop appended "applied" without changing anything (assert passed on a lie),
  and hyphen/underscore filename flip-flops masked it through two push cycles.
  Fixed script normalizes sources to line lists AND asserts markers in the
  serialized bytes plus a disk round-trip check. **New mandatory rule:
  pull-back-verify every push** (`kernels pull` → grep markers) before claiming
  a version contains a fix. v23's failure (same onnxscript crash, same 20-clip
  copy) therefore proves nothing about the fixes.
- **v24 (ran with verified patches):** 181/181 real clips in training; full 100k
  training loop completed; export succeeded (with non-fatal onnxscript
  version-converter warning). BUT: (a) eval cell crashed AFTER export
  (`ImportError: cannot import name 'Model'` — cwd/package path; save/report
  cells never ran, so no `training_report.json`), and (b) heldout eval with the
  FIXED scorer (§9 resolution below): **v24 0/5, v2 1/5 (BASE02 @0.998)**.
  Diagnosis: real voice drowned — batch saw ACAV 1024 vs positive 50, and the
  positive pool itself was espeak-dominated. v24 learned espeak (0.902 on its
  own positives) but not the owner. Fix: real oversample ×10 (1810 ≈ espeak
  2000 parity) + eval path fix + in-run VERIFY recall probe.
- **v25 (ERROR at t≈510s):** died on `IndentationError` in the patched STEP 2 —
  the splice put the oversample `for` at the `if`'s own indent. Whole-cell
  rewrite adopted as policy after this (patch-by-splice is banned for logic
  changes). All-cells compile gate added to the patch script (with shell-magic
  masking that preserves block structure).
- **v26 (ERROR, same):** pushed via `;`-chained command AFTER the patch script
  failed — the broken code went up anyway. Policy: conditional chains only
  (`push` runs solely if patch exits 0). The same validator then caught a
  SECOND splice bug pre-push (eval-cell path fix at col-0 under an `if`),
  which became a whole-cell rewrite too.
- **v27 (ERROR at t≈2553s):** oversample verified working in-log
  (1810 real + 2000 espeak = 3810 positives); training + export completed;
  eval crashed on `ValueError: tflite framework selected but onnx provided`
  (upstream flipped Model's default backend). Save/report cells never ran.
- **v28 (ERROR at t≈2565s):** both eval fixes worked (no ImportError, no
  ValueError); died one line later in MY probe: `float(m.predict(...))` —
  newer oWW `predict()` returns `{label: score}` dict. Fix: dict-guard.
- **v29 (COMPLETE — first green run since v2):** all cells passed, model +
  report in outputs. In-run VERIFY claimed 20/1810 real recall — PROVEN WRONG
  by independent eval (the in-run probe misuses streaming `Model.predict`;
  do not trust it; P1 metrics come from `eval_recall.py` only).
- **P1 Test A (independent, `eval_recall.py` @0.35, exact Rust replica):**
  train recall ≈119/131 ≈91% — per-group: hey_nexus far 100%, fast 95%,
  loud 100%, slow 100%, quiet 90%, near 80%; nexus far 100%, near 100%,
  loud 50%. **Heldout (sealed BASE01–05): 2/5 = 40%.**
  Verdict vs ship gate (≥90% heldout): **FAIL → back to P1 per protocol.**
  Read: model learned the owner's voice (train 91%) but generalizes weakly to
  unseen takes; weak spots = heldout near_normal (cold-voice Round-0
  distribution shift suspected) + nexus loud (50% even on seen data —
  saturation distortion + short-word cold-start, §9 corollary).
- **v30 (COMPLETE):** code-identical rerun on pack v2 (200 wavs).
  In-run VERIFY: real 140/2000, espeak 4/10 (in-run probe under-reads —
  established v29; ignore for gating).
- **P1 Test A (independent `eval_recall.py` @0.35): HELDOUT 9/10 = 90.0% —
  GATE MET.** Train: hey_nexus far/fast/loud/slow 100%, near 91%, quiet 90%;
  nexus far 90%, near 100%, loud 79%. Misses (10): BASE01(0.23), A07/A09/A10,
  Q01, BF05, BV04/BX01/BX06/LN06 — scattered borderlines (0.02–0.31), no dead
  zeros; H01–H05 cold voice ALL HIT (distribution gap closed).
  P1 Test B (live Rust harness) + P2 Test B (background FA) remain before ship.
  **Next: Block E background collection → FA sweep → operating-point selection
  → ship v30 as hey_nexus primary (bare-nexus lenient fallback).**
- **Block E + P2 Test B (`fa_sweep.py`, 25 min negatives, exact trigger
  logic):** silence#1+#2, TV, conversation, household-activity.
  @0.35 → **4 triggers = 9.6 FA/hr — GATE FAIL (target ≤0.5).**
  Breakdown: silence#1 1×(0.40 thump), silence#2 0, TV 1×(0.74 speech),
  convo 1×(0.40–0.49), active 1×(0.64). Sweep is conservative (no Phase-B VAD,
  no 500ms RMS confirmation — live kills the thump; speech triggers survive).
  Estimated true live FA ≈3–5/hr: 6–10× over target.
- **Stage-2 verifier BUILT (2026-09-17, `wakeword_oww.rs` + `commands.rs`):**
  2.5s ring (`VERIFY_RING`, fed every chunk) → stage-1 candidate → dedicated
  `wake-verify` thread → `stt::transcribe_samples` (Groq→Moonshine chain,
  10s timeout) → fire only if transcript contains "nexus".
  Fail-open on STT error/timeout/short-audio/spawn-failure; single-flight
  guard + self-send `VERIFIED_BYPASS` (single fire path preserved, no dup).
  Kill-switch: `"verifyWake": false` (default true, `read_verify_wake`).
  Cost: +~250ms Groq typical. Dual-gate: (A) 4 new unit tests green (413/413
  suite, zero warnings both feature sets); (B) FA re-sweep — all 4 trigger
  segments transcribed via Groq: thump→"Gracias.", TV→"Thank you.",
  convo→hallucinated Japanese, active→"No." — **4/4 SUPPRESS, projected
  9.6/hr → ~0/hr** on sample. Ship blockers remaining: rebuild + live soak
  (P4); settings-UI toggle for verifyWake (JSON-only for now).
- **Champion-challenger contract (user directive 2026-09-17):** v30 is champion
  and stays shipped until a challenger beats it on BOTH existing heldout
  (BASE+H, no regression) AND new tests. Improvement must prove itself;
  regression keeps v30. "Improved not broken," verified twice.
- **v31 (ERROR at t≈440s):** accent cell used `asyncio.run()` — illegal inside
  Jupyter's already-running loop (RuntimeError). Fix: async main + top-level
  await (compile-gate now uses ALLOW_TOP_LEVEL_AWAIT).
- **v32 (COMPLETE but hollow):** accent cell ran (135 clips, async-main fix
  worked) but died in augment: `ValueError: Clip does not have the correct
  sample rate` — espeak-ng writes 22050 Hz, pipeline demands 16000 Hz (edge
  path had ffmpeg conversion, espeak path didn't). No model.
- **v33 (COMPLETE):** accent cell ran clean (135 clips, 0 dropped).
- **Champion-challenger VERDICT: v33 LOSES, v30 retains the title.**
  v33 heldout 5/10 (50%) vs v30 9/10 (90%); train near 71% (was 91%), far
  93% (was 100%), nexus loud 58% (was 79%). Only fast/loud/slow stay 100%.
  Diagnosis: accent-data interference — 135 foreign-voice clips (incl.
  robotic espeak) pulled the boundary off the owner at fixed capacity.
  Lesson: for a PERSONAL device, owner-heavy mixtures win; multi-accent
  joint training needs either more capacity or fine-tune staging, not
  naive pooling. v33 archived, never ships. Ship candidate remains v30
  (recall gate met) pending rebuild + live soak (P4).
- **Rebuild with v30 + verifier (2026-09-18, `target/release/nexus.exe`,
  51 MB):** v2 `nexus.onnx` backed up to `nexus_v2_backup.onnx`; v30
  `nexus.onnx` (13 KB) + `nexus.onnx.data` (856 KB) installed. Two code fixes
  required: (1) `load_onnx_model` now uses `model_for_path` (cursor-based
  load has no base dir → external `.data` unresolvable; verified by
  `test_wake_engine_initializes` on the split model); (2) stage-2 verifier
  (`VERIFY_RING` 2.5s + `wake-verify` thread + `verify_transcript` gate +
  `verifyWake` setting default-true). `resources/oww/**/*` already covers
  the `.data` file. NOTE: `cargo tauri build` fails locally — its
  beforeBuildCommand spawns npm with cwd=repo-root instead of src-tauri
  (works when run directly); testing used `cargo build --release --features
  custom-protocol`. Full suite 413/413 + mock-wake clean, zero warnings.
- **P3 executed (2026-09-17):** H01–H05 sealed heldout (cold voice, physically
  moved to `heldout/`); K01–K10 cold near_normal train (RMS ~0.03 — the missing
  distribution confirmed); LN01–LN10 nexus loud (LN04 dropped; LN10 mis-spoken
  as "hey nexus" → owner-ordered DELETE + clean retake, no reclass).
  Pack v2: 150 camp + 50 legacy = 200 wavs. **v30 pushed (code-identical,
  data-only change), watched run in progress.** P2 gates for v30: fresh
  H-heldout recall (primary) + BASE-heldout + nexus-loud group recall.
- **§9.1 RESOLVED (scorer cold-start bug):** the replica initialized the
  embedding buffer EMPTY (requiring 16 consecutive speech chunks ≈1.3s before
  first score) while Rust pre-fills zeros and scores immediately. Short isolated
  words were structurally unscorable — a pure harness artifact. Fixed
  (`score_wake.py` zero-init); all prior "~0.000" readings void. Proof: v24
  scores 0.902 on its own training positive; v2 scores 0.998 on BASE02.
  Corollary for live: after silence, the first ~1.3s of speech scores against
  zero context — short isolated words are inherently disadvantaged live too;
  longer phrases ("hey nexus" > "nexus") carry more scoring windows.
- CLI notes: `kernels logs` needs `PYTHONUTF8=1` on Windows (unicode crash
  otherwise); session log arrives UTF-16; `kernels output` supports
  `--file-pattern` for selective download (full output is GBs of intermediate
  wavs — always filter to `nexus.onnx` + `training_report.json`).

## 16. External-architecture review (2026-09-19, 5 parallel agents)

Compared against: NousResearch/hermes-agent + derivatives, OpenVoiceOS/Mycroft,
Home Assistant Assist, cascade KWS literature (Google/Apple/Amazon/U2/DS/ZP),
Porcupine/Snowboy. Grades: HA Assist ASSESSSMENT strong (validates backward
confirmation; their pipeline structurally cannot hit our 0.986 failure);
cascade literature decisive (0 of 9 references verify forward; Sun et al.
SLT 2016: spotters fire near keyword END by design); Porcupine properly
caveated (97%@1FA/10h on THEIR corpus, not ours); OVOS report TRUNCATED
(sections 1–3 missing — unverified); HERMES report FLAGGED (star/commit
counts "247k/37k" fail sanity — NousResearch has no such numbers; treat all
its uncited figures as unverified, architecture claims as plausible).
- Cross-verified by ≥2 agents: backward-confirmation (all), VAD pre-gate,
  trigger_level tension (their consecutive-frames vs our single-frame fast
  path — open question), STT-verifier unique to us (competitors do transcript
  match INSTEAD of acoustic, never combined), BoH post-filtering.
- Steal queue (ranked): consecutive-frame confirmation experiment (#1),
  per-phrase cooldown map, VAD pre-gate on wake engine, temperature pin
  (validated below), AEC barge-in (later, hardware-dependent).
- **no_speech calibration (`wake_camp/nospeech_calib.txt`): 16 clips
  (4 FA + 11 NX + determinism repeat) via verbose_json — EVERY segment
  reports no_speech_prob = 0.000, wakes and hallucinations alike. The v3
  confidence veto is therefore DORMANT on whisper-large-v3-turbo (never
  fires; harmless). avg_logprob evaluated as replacement and REJECTED
  (true NX01 -0.809 overlaps garbage -0.8x — no separating threshold).
  Determinism repeat (NX05 ×2, temp=0): byte-identical outputs — pin
  validated; earlier run-to-run flips came from the unpinned path.
  Production gating stays: word gate (two-key) only.

## 14. Verifier v2 + confusion science (2026-09-18)

- **Computational confusable map (`confusables.py`, CMUdict edit-distance):**
  d=1: lexus, nexis, texas, next's, necklace + nexas/nexuss/nekus artifacts;
  d=2: 45 words incl. axis/exit/access/plexus/census. Full list
  `wake_camp/confusables.txt`. Disposition: near-list → verifier accept-set;
  common words (texas/next/access) → excluded (TV frequency) + acoustic
  negatives; all → NLU correction map queue.
- **NX experiment (30 owner takes, Groq-verified):** nexus 13 (43%), lexus 11
  (37%), access 2, process/alexis/plexus/letsjust 1 each. STT flips run to
  run (R1: 9 nexus, R2: 7 lexus). Every confusion is d≤2 on the computed map.
- **Prompt-bias experiment (REJECTED — textbook bias symmetry):** prompt flips
  11/11 lexus→nexus (recall fix) BUT also flips 2/4 FA segments (TV + convo
  onset) into exact "Nexus." readings (FA catastrophe — exact matches bypass
  two-key). A decoder prior cannot distinguish true from false nexus
  acoustics. Verifier passes None; `stt_groq` keeps the prompt param as API.
- **VAD-trim experiment (MIXED, kept for latency):** 3s→0.6s payloads, but
  accuracy noisy both directions (Whisper wants more context than bare words;
  Groq sampling nondeterminism confounds small-n reads). Kept for
  smaller/faster STT calls; no accuracy claims.
- **Verifier v2 SHIPPED (code, unbuilt):** VAD-trim + two-key phonetic gate
  (exact `nexus` any prob; {lexus,nexis,nexas,nexuss,nekus,lexis} require
  stage-1 ≥0.6; texas/next/access deliberately excluded) + `verifyWake`
  kill-switch + Groq `prompt` plumbing (verifier passes None per above).
  Projected: recall ~80% (24/30 NX) with FA ~0 preserved. Dual-gate Test B:
  live 10× self-test + TV re-soak after rebuild.

## 15. Verifier v3 — confidence gate (2026-09-19, research-led)

Research (Groq docs + OpenWhispr field thread + 3 Whisper-hallucination
papers) changed two decisions: (1) Groq exposes per-segment `no_speech_prob`
/ `avg_logprob` via `verbose_json` — a model-native phantom signal, better
than any energy heuristic; (2) ultra-short clips hallucinate MORE, so trim
was widened (±300ms, ~1.5s payloads) and now serves latency, not truth.
- `transcribe_with_groq_verbose` + `transcribe_samples_verbose` (plain paths
  untouched); temperature pinned 0; segments parsed defensively (defaults).
- Confidence veto: suppress ONLY if ALL segments report no_speech ≥ 0.85
  (deliberately above the literature's 0.6 — mangled true speech scores
  mid-range); empty segments → defer to word gate.
- `boh_miner.py`: field transcripts → BoH/confusion proposals (human-approved).
- 437/437 tests (segment-parse robustness, confidence matrix), zero warnings
  both features. Dual-gate Test B unchanged: live 10× self-test with
  transcript+confidence logging + TV re-soak after rebuild.

## §17 — 100-combo review (2026-09-19)
Full gate matrix (99 cases: 60 pos + 39 neg): v30 AND v2 both recall 26/60 (43.3%), FA 2/39 (5.1%) — models trade BN01/BV05 single cases; the STT+word gate dominates, not the classifier. ~28/34 misses are Groq confabulations on both attempts despite KWS fire; ~5 are genuine KWS-weak clips (BASE01 0.23, BASE03 0.46, BV04 0.30, BX01 0.16); 1 fused-form tail (A20 canixis). 2 FAs are by-design near+high (SAPI lexus). v30 wins near_normal/confusion, v2 wins loud — 10-clip heldout too thin to crown; NO retrain justified yet. Flagged: earlier 185/190 + BASE 5/5 not reproducible from current files. BLOCKED on owner ears: 5-clip contamination check (BF02/BASE02/F04/A06/Q04). Full review: docs/wake-word/22-hundred-combo-review.md.
