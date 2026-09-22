# Phase 3 — Candidate OOS Training Experiment

**Date:** 2026-09-14
**Status:** Complete — candidate trained, evaluated, triple-verified, NOT promoted
**Predecessor:** [Phase 2 — CLINC OOS Semantic Review](phase-2-clinc-oos-semantic-review-2026-09-14.md)

## Objective

Train a **candidate** BERT-Mini model using the 85 approved CLINC OOS rows added
to the locked train split, then evaluate it against:

1. The frozen 452-row final test.
2. The 867-row reviewed CLINC OOS benchmark.
3. The 5-row supported-mapping benchmark.

The production model must not be modified. The candidate must not be promoted
unless it improves OOS safety without unacceptable regression elsewhere.

## What Was Done

### 1. Candidate dataset built

Script: `server/nlu/build_candidate_dataset.py`

- Copied the locked train/validation/calibration/test splits verbatim.
- Appended 85 reviewed and approved CLINC OOS rows to **train only**, mapped to
  `unknown` with empty slots.
- Verified validation/calibration/test hashes unchanged against `split_lock.json`.
- Verified no candidate row overlaps any active split by normalized text.
- Written to `server/nlu/data/candidate_dataset.json` (gitignored).
- Production `dataset.json` was not modified.

Candidate train: 1,863 rows (1,778 production train + 85 CLINC OOS).

### 2. Candidate model trained

Script: `server/nlu/train_candidate.py`

- BERT-Mini (`google/bert_uncased_L-2_H-128_A-2`), 52 intents, 45 slots.
- 50 epochs, batch size 16, lr 5e-5, seed 42.
- Checkpoint selection: best validation intent accuracy (slot accuracy as tiebreaker).
- All artifacts written to `server/nlu/model/candidate/` (gitignored).
- Production `model/` directory was not touched.

Training results:

| Metric | Value |
|---|---:|
| Best validation accuracy | 74.89% |
| Best validation slot accuracy | 94.10% |
| Final test intent accuracy | 63.05% |
| Final test slot accuracy | 88.81% |

### 3. Candidate ONNX exported

- File: `server/nlu/model/candidate/nexus_nlu_candidate.onnx`
- Size: 16.8 MB
- SHA-256: `3fcb190424737cda6f813612d9ed88ec0a5ac4d18d759be0ca4872cc647bf220`
- Labels: 52 intents, 45 slots (identical schema to production)

### 4. Candidate evaluated against all benchmarks

Script: `server/nlu/evaluate_candidate.py`

## Results — Candidate vs Production

### Frozen Final Test (452 rows)

| Metric | Production | Candidate | Delta |
|---|---:|---:|---:|
| Intent accuracy | 95.80% | 63.05% | **-32.75 pp** |

The candidate regressed severely on the frozen final test.

### Reviewed CLINC OOS Benchmark (867 rows)

| Metric | Production | Candidate | Delta |
|---|---:|---:|---:|
| Raw `unknown` accuracy | 35.64% | 76.70% | **+41.06 pp** |
| Gated accuracy (0.85) | 96.66% | 98.96% | **+2.30 pp** |
| Gated failures | 29 | 9 | **-20** |

The candidate dramatically improved OOS rejection. Raw unknown recall more
than doubled, and gated failures dropped from 29 to 9.

### Supported Mappings (5 rows)

| Metric | Production | Candidate | Delta |
|---|---:|---:|---:|
| Raw accuracy | 80.00% | 40.00% | -40.00 pp |
| Gated accuracy (0.85) | 60.00% | 40.00% | -20.00 pp |
| Gated failures | 2 | 3 | +1 |

The candidate regressed on supported mappings.

## Critical Finding — Why the Candidate Regressed on Final Test

The Phase 1 split restructuring reduced the training set from 3,399 rows to
1,778 rows by quarantining 1,051 phrase-family-leaking rows. While this fixed
evaluation contamination, it severely reduced training coverage for
specialized GitHub intents.

Per-intent recall on the frozen final test shows the damage is concentrated:

| Intent | Test rows | Candidate recall |
|---|---:|---:|
| `add_org_member` | 11 | 0.00% |
| `approve_pr` | 7 | 0.00% |
| `cancel_workflow` | 10 | 0.00% |
| `close_pr` | 11 | 0.00% |
| `comment_pr` | 3 | 0.00% |
| `remove_org_member` | 9 | 0.00% |
| `revert_pr` | 10 | 0.00% |
| `add_collaborator` | 9 | 11.11% |
| `merge_pr` | 10 | 10.00% |
| `delete_branch` | 9 | 11.11% |

These are the same 13 intents that were blocked in Phase 1 because they lacked
independent phrase families. The 297 authored examples added in Phase 1 were
not enough to compensate for the 1,051 quarantined rows.

## Remaining High-Confidence OOS Failures

The candidate still has 9 gated OOS failures. The highest-confidence ones:

| Confidence | Predicted | Text |
|---:|---|---|
| 97.71% | `type_text` | put into words what you want |
| 97.31% | `type_text` | put words on the paper about what you want |
| 96.78% | `type_text` | what do i put on my feet |
| 91.83% | `type_text` | report outage to my electric provider |
| 91.45% | `browser_new_tab` | program my new robot to bring me snacks |
| 90.63% | `browser_new_tab` | i want to install new tiles in my kitchen |
| 89.78% | `focus_app` | show me a cool nintendo switch game |
| 86.87% | `type_text` | do i have to put my mouth on theirs when doing cpr |
| 85.88% | `focus_app` | save my text on my laptop hard drive |

The `type_text` intent is particularly prone to false activation because words
like "put", "words", "text", "paper" appear in many non-command utterances.

## Conclusion — Do NOT Promote This Candidate

The candidate is **not safe for promotion** because:

1. **Final test accuracy dropped from 95.80% to 63.05%** — a 32.75 percentage
   point regression. Multiple specialized GitHub intents have 0% recall.
2. **Supported mapping accuracy dropped from 80% to 40%** — the candidate
   is worse at recognizing valid NEXUS commands.
3. The OOS improvement is real and significant (41 pp raw, 20 fewer gated
   failures), but it does not justify the regression on known commands.

The OOS improvement proves that adding reviewed CLINC OOS training data helps
unknown rejection. The regression proves that the Phase 1 training set is too
small after quarantining 1,051 rows.

## Recommended Next Steps

Before any candidate can be promoted, the training set must be expanded to
recover the coverage lost to Phase 1 quarantine:

1. **Add more real training examples for the 13 blocked intents** — not just
   paraphrased templates, but structurally diverse command forms.
2. **Consider restoring some quarantined rows** whose phrase families can be
   made independent through structural rewriting rather than slot substitution.
3. **Add more OOS training data** — 85 rows is a good start but the candidate
   still has 9 high-confidence OOS failures, several from `type_text` false
   activation.
4. **Re-run the candidate experiment** after expanding training data.
5. **Do not change the production model or the 0.85 confidence gate** until a
   candidate passes both final test and OOS benchmarks.

## Triple Verification

### Pass 1 — Code correctness and candidate isolation

- Python compilation: passed.
- Candidate dataset structure: train 1,863, val 438, cal 429, test 452.
- 85 OOS rows in candidate train, all mapped to `unknown`, all `approved`.
- Candidate training report and evaluation report consistent.

### Pass 2 — Production immutability and split integrity

- Production ONNX hash unchanged:
  `f7343af8ef8377975ce59cbc40d7cd8dc5ac674ad0df26fe401c534347c9164c`
- Production dataset hash unchanged:
  `c4e95cfdbf814af7f08fd63b6eba8fb3eeb22a50e04b6dc20c83072054a6ecdd`
- Candidate validation hash matches split lock: passed.
- Candidate calibration hash matches split lock: passed.
- Candidate test hash matches split lock: passed.

### Pass 3 — Determinism, security, and documentation

- Candidate ONNX hash: `3fcb190424737cda6f813612d9ed88ec0a5ac4d18d759be0ca4872cc647bf220`
- Git whitespace check: no errors.
- Phase 1 and Phase 2 reports indexed in `docs/testing/README.md`.
- Candidate artifacts gitignored (model/candidate/, candidate_dataset.json).
- No sensitive values in candidate reports.

## Files

| File | Purpose |
|---|---|
| `server/nlu/build_candidate_dataset.py` | Builds candidate dataset with 85 OOS rows |
| `server/nlu/train_candidate.py` | Trains candidate model to isolated directory |
| `server/nlu/evaluate_candidate.py` | Evaluates candidate against all benchmarks |
| `server/nlu/data/candidate_dataset.json` | Candidate dataset (gitignored) |
| `server/nlu/model/candidate/` | Candidate model artifacts (gitignored) |
| `server/nlu/model/candidate/candidate_training_report.json` | Training metrics |
| `server/nlu/model/candidate/candidate_evaluation_report.json` | Evaluation metrics |

## Explicitly Not Done

- The production ONNX model was not modified.
- The production `dataset.json` was not modified.
- The 0.85 confidence gate was not changed.
- No CLINC rows were merged into `dataset.json`.
- No MASSIVE or SLURP data was imported.
- No wake-word training was started.
- The candidate was not promoted to production.
- No calibration or temperature scaling was applied.
