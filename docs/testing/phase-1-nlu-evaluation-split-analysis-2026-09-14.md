# Phase 1 NLU Evaluation Split Completion Report

**Date:** 2026-09-14  
**Status:** Complete; awaiting user cross-check before Phase 2  
**Scope:** Independent train, validation, calibration, final-test, and quarantine splits  
**Model retrained:** No  
**Production ONNX changed:** No  
**CLINC approved or merged:** No

## Objective

Phase 1 removes evaluation contamination from the BERT-Mini workflow. Previously, `train.py` shuffled the 452-row test set, used half for checkpoint selection, and called the other half final test. Repeated experiments therefore tuned indirectly against the test corpus.

The implemented structure is:

```text
train        gradient updates
validation   checkpoint selection and future early stopping
calibration  future temperature and acceptance-threshold fitting
final test   one-time candidate evaluation
quarantine   useful rows excluded because their phrase family appears in final test
```

## Phrase-family policy

Replacing a slot value does not create an independent example. These utterances are one family:

```text
merge pull request 12 into owner/repo
merge pull request 99 into nexus/app
```

The splitter normalizes text, replaces annotated slots with placeholders, replaces free numbers with `<number>`, normalizes leading fillers, and combines the result with the intent label.

No phrase family may span train, validation, calibration, or final test.

## Initial readiness failure

The first safe dry run blocked 13 intents because every available training template, or nearly every template, was already represented in final test:

- `add_collaborator`
- `add_org_member`
- `approve_pr`
- `cancel_workflow`
- `close_pr`
- `comment_pr`
- `create_release`
- `delete_branch`
- `get_pr`
- `merge_pr`
- `remove_org_member`
- `rerun_workflow`
- `revert_pr`

The script returned exit code `2` and did not modify the dataset. This prevented a random row split from producing misleadingly high scores.

## Independent family expansion

`server/nlu/add_phase1_families.py` added 297 reviewed synthetic examples across 22 sparse intents in two reviewed passes:

- 195 examples for the original 13 blockers;
- 102 examples for additional intents that had fewer than ten independent non-test families once the stricter requirement was applied.

The examples vary command structure rather than only repository names, PR numbers, branches, users, organizations, tags, or workflow IDs. The authored source manifest is `server/nlu/data/phase1_authored_families.json`.

These examples are intended to make leakage-safe split construction possible. They are not evidence of real-speech accuracy and do not replace future opt-in ASR testing.

## Final split result

| Split | Rows | Phrase families | Intents | Purpose |
|---|---:|---:|---:|---|
| Train | 1,778 | 1,184 | 52 | Gradient updates |
| Validation | 438 | 263 | 52 | Checkpoint selection |
| Calibration | 429 | 262 | 52 | Confidence calibration only |
| Final test | 452 | 348 | 52 | One-time candidate evaluation |
| Quarantine | 1,051 | — | — | Excluded template-leaking training rows |

The 1,051 quarantined rows were preserved in `dataset.json`; they were not deleted. Their phrase families overlap final test and therefore cannot be used for model optimization or checkpoint selection.

Conservation check:

```text
train + validation + calibration + quarantine
= 1778 + 438 + 429 + 1051
= 3696 rows
```

This equals the post-expansion training pool of 3,399 original rows plus 297 independently structured additions.

## Locked hashes

The split lock is `server/nlu/data/split_lock.json`.

| Data | SHA-256 |
|---|---|
| Train split | `508969bf5eabb5435ffc321a2f081d2825b1bdddce3c34d2aff42d0d7f836c49` |
| Validation split | `90430ce642957b9ee2b2cfdd3a2ffa5d7e54971b4a39a3575ed46a61ba3cf12b` |
| Calibration split | `e210acecb7dde5d3034d2428910f1af6582dc89beaa68072f4433633104de275` |
| Final-test split | `9dad4e743eabd96d2dd86d020c1752c759a04eaa55545fe1d5ba783319355b02` |
| Quarantine | `70a5db5c3a60bb08a6536117b50c6a926d4992226969fb03c4f0ff15ea6ce38b` |
| Frozen canonical final-test lock | `ecf00c6ae6a0b83d5b7121956aeb9547f35ad1a9e935c4f6f389c507986194e2` |

The split hash and canonical evaluation-lock hash use different serialized representations, so their values are intentionally different. Both validate the same unchanged 452 final-test rows.

## Training changes

`train.py` now requires all four active splits:

```python
train_data, val_data, calibration_data, test_data = load_dataset()
```

Behavior:

- training uses `train` only for gradients;
- checkpoint selection uses `validation` only;
- `calibration` is loaded and explicitly reserved;
- final test is evaluated only after the best validation checkpoint is loaded;
- the previous `random.shuffle(test_data)` and 50/50 test division were removed.

`train_all.py` validates the data foundation both before modifications and again after generation, merge, repair, and cleaning. Any operation that changes a locked split unexpectedly blocks training.

No training run was started during Phase 1, so the current production model remains available while the evaluation architecture is reviewed.

## Triple verification

### Pass 1 — code and focused tests: passed

- Python compilation passed for family generation, split preparation, data foundation, training, unified training, and tests.
- Node CLI syntax passed.
- Five focused tests passed:
  - slot substitutions remain in one family;
  - structurally different commands remain separate;
  - intentional family leakage is rejected;
  - all current splits match their lock and cover 52 intents;
  - training does not divide or select checkpoints using final test.
- Data-foundation validation passed.

### Pass 2 — split integrity and conservation: passed

- Pairwise exact-row overlap: zero.
- Pairwise normalized-text overlap: zero.
- Pairwise phrase-family overlap: zero.
- Train coverage: 52/52 intents.
- Validation coverage: 52/52 intents.
- Calibration coverage: 52/52 intents.
- Final-test coverage: 52/52 intents.
- Split lock exactly matches the dataset.
- Frozen final-test canonical hash remained unchanged.
- All 3,696 post-expansion pool rows are accounted for.
- Quarantine has no normalized-text overlap with train, validation, or calibration.
- Existing structural audit passed with 1,778 active training rows and 452 final-test rows.

### Pass 3 — runtime preparation and repeatability: passed after one fix

Initial pass 3 found that rerunning the splitter after application recalculated readiness from the reduced training split and returned the historical blocked status. It did not mutate data, but the command was not semantically idempotent.

The root cause was fixed: when validation and calibration already exist, the command validates the existing locked splits directly instead of rerunning pre-split readiness analysis.

Restarted pass 3 results:

- training loader returned `(1778, 438, 429, 452)`;
- repeated split preparation returned success without modifying dataset or lock;
- dataset SHA-256 remained `c4e95cfdbf814af7f08fd63b6eba8fb3eeb22a50e04b6dc20c83072054a6ecdd`;
- split-lock SHA-256 remained `c70dec8349537cc51eea15a1e8f13871609617bc3cb4921431ae0d6ca38873ed`;
- production source/resource ONNX parity passed;
- production ONNX remained `f7343af8ef8377975ce59cbc40d7cd8dc5ac674ad0df26fe401c534347c9164c`;
- five focused tests passed again;
- data-foundation validation passed again;
- whitespace validation passed with existing line-ending warnings only;
- sensitive-value scan returned no matches.

## Remaining limitations

Phase 1 fixes evaluation structure, not model quality. The following remain for later phases:

- calibration metrics and temperature fitting;
- macro F1 and risk/coverage candidate comparison;
- semantic review of CLINC records;
- energy and centroid OOS experiments;
- real ASR evaluation;
- MASSIVE and SLURP imports;
- wake-word model retraining.

The 297 authored rows are synthetic. Future promotion still requires independent real-user and ASR evaluation.

## Phase 1 conclusion

Phase 1 is technically complete:

- all four active splits cover 52 intents;
- no exact or phrase-family leakage exists between active splits;
- the final test remained unchanged;
- contaminated rows were quarantined, not destroyed;
- training no longer uses final-test rows for checkpoint selection;
- split hashes are locked;
- all three verification passes succeeded.

No Phase 2 work may begin until the user cross-checks this report and explicitly approves proceeding.
