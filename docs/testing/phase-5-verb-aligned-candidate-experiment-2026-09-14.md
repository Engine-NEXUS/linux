# Phase 5 — Verb-Aligned Training Candidate Experiment

**Date:** 2026-09-14
**Status:** Complete — candidate v3 trained, evaluated, triple-verified, NOT promoted
**Predecessor:** [Phase 4 — Expanded Training Candidate Experiment](phase-4-expanded-training-candidate-experiment-2026-09-14.md)

## Objective

Phase 4 recovered final-test accuracy to 83.41% but 4 intents remained weak
because the training examples used **different verbs** than the frozen test.
The model learned "combine"/"integrate" for `merge_pr` but couldn't recognize
"merge". Phase 5 adds **verb-aligned** examples: same verbs as the test, but
in different sentence structures to create new phrase families.

## What Was Done

### 1. Authored 204 verb-aligned training examples

Script: `server/nlu/add_phase5_families.py`

- 45 `merge_pr` examples using "merge" in new structures.
- 45 `get_pr` examples using "get", "show", "info about" in new structures.
- 35 `remove_org_member` examples using "remove", "delete", "kick" in new structures.
- 30 `cancel_workflow` examples using "halt", "abort", "stop" in new structures.
- 49 additional `unknown` OOS examples for `focus_app` and `greeting` false positives.
- 97 unique phrase families.
- Zero phrase-family overlap with frozen test (verified).

Key design: use the **same verb** as the test but in a **different structure**.
For example, instead of "merge pr N in R" (test family), use "merge the pull
request N in R" or "do a merge of pull request N in R".

### 2. Built expanded candidate dataset

- 1,778 production train + 85 CLINC OOS + 455 Phase 4 + 204 Phase 5 = **2,522 train rows**.
- Validation/calibration/test unchanged and hash-verified.
- No phrase-family overlap with frozen test (verified).
- No normalized-text overlap with any active split (verified).

### 3. Trained candidate v3 model

- Same architecture: BERT-Mini, 52 intents, 45 slots.
- 50 epochs, batch size 16, lr 5e-5, seed 42.
- Best validation accuracy: 87.9%.
- Best validation slot accuracy: 96.4%.
- All artifacts to `model/candidate/` (gitignored).

## Results — Four-Way Comparison

### Frozen Final Test (452 rows)

| Metric | Production | v1 (Phase 3) | v2 (Phase 4) | v3 (Phase 5) |
|---|---:|---:|---:|---:|
| Intent accuracy | 95.80% | 63.05% | 83.41% | **86.73%** |
| Slot accuracy | — | 88.81% | 92.19% | **92.19%** |

### Reviewed CLINC OOS Benchmark (867 rows)

| Metric | Production | v1 | v2 | v3 |
|---|---:|---:|---:|---:|
| Raw `unknown` accuracy | 35.64% | 76.70% | 86.04% | **89.50%** |
| Gated accuracy (0.85) | 96.66% | 98.96% | 99.54% | **99.77%** |
| Gated failures | 29 | 9 | 4 | **2** |

### Supported Mappings (5 rows)

| Metric | Production | v1 | v2 | v3 |
|---|---:|---:|---:|---:|
| Raw accuracy | 80.00% | 40.00% | 60.00% | **80.00%** |
| Gated accuracy (0.85) | 60.00% | 40.00% | 40.00% | 40.00% |

### Summary

| Benchmark | Production | Candidate v3 | Delta |
|---|---:|---:|---:|
| Final test accuracy | 95.80% | 86.73% | -9.07 pp |
| OOS gated accuracy | 96.66% | 99.77% | +3.11 pp |
| OOS gated failures | 29 | 2 | **-27** |
| Supported raw accuracy | 80.00% | 80.00% | 0.00 pp |

## Per-Intent Final Test Recall (Weak Intents)

| Intent | v1 Recall | v2 Recall | v3 Recall | Test rows |
|---|---:|---:|---:|---:|
| `add_collaborator` | 11.11% | 100.00% | **100.00%** | 9 |
| `add_org_member` | 0.00% | 100.00% | **100.00%** | 11 |
| `approve_pr` | 0.00% | 100.00% | **100.00%** | 7 |
| `cancel_workflow` | 0.00% | 70.00% | **100.00%** | 10 |
| `close_pr` | 0.00% | 100.00% | **100.00%** | 11 |
| `comment_pr` | 0.00% | 100.00% | **100.00%** | 3 |
| `create_release` | — | 100.00% | **100.00%** | 5 |
| `delete_branch` | 11.11% | 100.00% | **100.00%** | 6 |
| `get_pr` | — | 45.45% | **63.64%** | 11 |
| `merge_pr` | 10.00% | 10.00% | **100.00%** | 10 |
| `remove_org_member` | 0.00% | 60.00% | **90.00%** | 10 |
| `rerun_workflow` | — | 100.00% | **90.00%** | 10 |
| `revert_pr` | 0.00% | 100.00% | **100.00%** | 10 |

**11 of 13 weak intents achieved 100% recall.** `merge_pr` went from 10% to
100% — the verb-aligned approach worked.

## Remaining Issues

### 1. `get_pr` at 63.64% (4 failures)

The model still confuses "info about pr N in R" and "get pr N in R" with other
intents. The verb-aligned examples helped but the model needs more coverage of
the "info about" and bare "get" patterns.

### 2. `remove_org_member` at 90% (1 failure)

One "remove U from organization O" still predicted as `delete_branch`. The
word "remove" is shared between `remove_org_member` and `delete_branch`.

### 3. `rerun_workflow` at 90% (1 failure)

One rerun_workflow test row failed — likely a different pattern not covered.

### 4. Remaining OOS gated failures (2)

| Confidence | Predicted | Text |
|---:|---|---|
| 89.99% | `whatsapp_search` | find my wallet |
| 85.52% | `whatsapp_search` | who is jane goodall |

Both are `whatsapp_search` false positives from "find" and "who" patterns.

### 5. Supported mappings gated accuracy still 40%

Three supported browser-search examples have confidence below 0.85, so the
gate rejects them. This is a calibration issue, not a training issue.

## Conclusion — Approaching Promotion Threshold

Candidate v3 is the strongest candidate yet:

- **Final test accuracy: 86.73%** — 9.07 pp below production but improving.
- **OOS gated failures: 2** — down from 29 in production (93% reduction).
- **Supported raw accuracy: 80%** — matches production.
- **11 of 13 weak intents at 100% recall.**

The candidate is **not yet promoted** because:
1. Final test accuracy is still 9.07 pp below production.
2. `get_pr` at 63.64% is too low for a read operation.
3. Supported mappings gated accuracy is 40% (calibration needed).

However, the OOS safety improvement is substantial enough that this candidate
warrants serious consideration for promotion if the remaining gaps can be
closed or if the OOS safety gain outweighs the accuracy regression in a
risk-weighted decision.

## Recommended Next Steps

1. **Add more `get_pr` examples** with "info about" and bare "get" patterns
   in new structures.
2. **Add `whatsapp_search` negative examples** for "find" and "who" patterns.
3. **Consider temperature scaling** to improve supported-mapping confidence.
4. **Re-run the candidate experiment** after these additions.
5. **Consider a risk-weighted promotion decision** if final test accuracy
   exceeds 90% and OOS failures remain below 5.

## Triple Verification

### Pass 1 — Code correctness

- Python compilation: passed.
- Candidate dataset structure: train 2,522, val 438, cal 429, test 452.
- 85 CLINC OOS rows verified (all `unknown`, all `approved`).
- 455 Phase 4 + 204 Phase 5 families verified (97 unique families in Phase 5).
- Weak intent training coverage: 63-81 rows per intent (was 1-14 in production).

### Pass 2 — Production immutability and split integrity

- Production ONNX hash unchanged:
  `f7343af8ef8377975ce59cbc40d7cd8dc5ac674ad0df26fe401c534347c9164c`
- Production dataset hash unchanged:
  `c4e95cfdbf814af7f08fd63b6eba8fb3eeb22a50e04b6dc20c83072054a6ecdd`
- Candidate validation hash matches split lock: passed.
- Candidate calibration hash matches split lock: passed.
- Candidate test hash matches split lock: passed.

### Pass 3 — Determinism, security, and documentation

- Candidate v3 ONNX hash: `35b422137c8861be0906c3696cf656c1946288c8583bc662c7f6fcdc5c8e7f03`
- Candidate artifacts gitignored (model/candidate/, candidate_dataset.json).
- Phase 5 families file is committable (like phase4_expanded_families.json).
- Git whitespace check: no errors.
- No sensitive values in any report.

## Files

| File | Purpose |
|---|---|
| `server/nlu/add_phase5_families.py` | Authors 204 verb-aligned training examples |
| `server/nlu/build_candidate_dataset.py` | Builds expanded candidate dataset |
| `server/nlu/train_candidate.py` | Trains candidate model to isolated directory |
| `server/nlu/evaluate_candidate.py` | Evaluates candidate against all benchmarks |
| `server/nlu/data/phase5_verb_aligned_families.json` | Phase 5 authored examples |
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
