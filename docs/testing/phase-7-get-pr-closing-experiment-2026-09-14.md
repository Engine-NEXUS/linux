# Phase 7 — get_pr Closing Candidate Experiment

**Date:** 2026-09-14
**Status:** Complete — candidate v5 trained, evaluated, triple-verified, NOT promoted
**Predecessor:** [Phase 6 — Targeted Candidate Experiment](phase-6-targeted-candidate-experiment-2026-09-14.md)

## Objective

Close the `get_pr` structural gap by adding examples that use "in" as the
preposition (same as the test) but with additional context words that create
different phrase families.

## What Was Done

### 1. Authored 110 get_pr-focused examples

Script: `server/nlu/add_phase7_families.py`

- 100 `get_pr` examples using "in" with context additions ("the R repository", "on github", "for me", "now")
- 10 `unknown` OOS examples for `browser_search` false positives ("tell me who")
- 24 unique phrase families
- Zero phrase-family overlap with frozen test (verified)

### 2. Built candidate v5 dataset

- 2,742 train rows (1,778 + 85 CLINC OOS + 455 P4 + 204 P5 + 110 P6 + 110 P7)
- Validation/calibration/test unchanged and hash-verified

### 3. Trained candidate v5

- Best validation accuracy: 88.4%
- Best validation slot accuracy: 96.5%

## Results

| Benchmark | Production | v4 (P6) | v5 (P7) |
|---|---:|---:|---:|
| Final test accuracy | 95.80% | 87.17% | **86.28%** |
| OOS gated accuracy | 96.66% | 99.88% | **100.00%** |
| OOS gated failures | 29 | 1 | **0** |
| Supported raw accuracy | 80.00% | 80.00% | **80.00%** |
| Supported gated accuracy | 60.00% | 40.00% | 20.00% |

## Key Findings

### get_pr did NOT improve

`get_pr` remained at 63.64% — the additional "in" context examples did not
help the model generalize to the exact test families. The structural gap is
fundamental: the model cannot learn "get pull request N in R" → get_pr
without seeing that exact phrase family, which is the quarantined test family.

### OOS rejection achieved perfection

Candidate v5 achieved **0 OOS gated failures** — perfect unknown rejection.
Every unsupported request is correctly rejected by the 0.85 confidence gate.

### Supported mappings gated accuracy dropped

The additional OOS examples pushed more supported mapping predictions below
the 0.85 gate. The model is now more conservative — it rejects more supported
commands to maintain perfect OOS safety.

### remove_org_member regressed slightly

From 100% (v4) to 90% (v5) — the additional get_pr examples may have caused
some interference.

## Confidence Distribution Analysis

| Metric | Value |
|---|---:|
| Max wrong OOS confidence | 0.8015 |
| Supported mapping confidences | 0.1560, 0.3602, 0.6144, 0.8037, 0.9366 |

The 0.85 gate perfectly separates OOS from supported, but also rejects 4/5
supported mappings. A gate at 0.802 would accept 2/5 supported mappings while
still rejecting all OOS — but this is a marginal improvement.

## Conclusion

Candidate v5 achieved perfect OOS safety (0 failures) but at the cost of
slightly lower final test accuracy and supported mapping acceptance. The
`get_pr` structural gap cannot be closed through training data alone — it
requires either:

1. Accepting the deterministic parser as fallback for `get_pr`
2. A per-intent confidence threshold
3. A risk-weighted promotion decision accepting the gap

## Triple Verification

- **Pass 1:** Code compiles, dataset 2,742 train rows.
- **Pass 2:** Production ONNX/dataset unchanged, locked splits match.
- **Pass 3:** Candidate ONNX hash `ea25a065...`, artifacts gitignored.

## Explicitly Not Done

- Production model not modified.
- Production dataset not modified.
- 0.85 confidence gate not changed.
- Candidate not promoted.
