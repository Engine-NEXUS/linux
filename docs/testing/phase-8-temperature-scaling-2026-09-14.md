# Phase 8 — Temperature Scaling and Gate Calibration

**Date:** 2026-09-14
**Status:** Complete — temperature fitted, optimal gate analyzed
**Predecessor:** [Phase 7 — get_pr Closing Experiment](phase-7-get-pr-closing-experiment-2026-09-14.md)

## Objective

Apply temperature scaling on the calibration split (429 rows, never used for
training or checkpoint selection) to improve confidence calibration, then
find the optimal confidence gate that maximizes supported mapping acceptance
while keeping OOS failures at zero.

## What Was Done

Script: `server/nlu/calibrate_temperature.py`

1. Loaded the candidate v5 ONNX model.
2. Collected logits for all 429 calibration rows.
3. Fitted temperature T by minimizing negative log-likelihood.
4. Applied T to OOS benchmark and supported mapping confidences.
5. Swept gates from 0.50 to 0.95 to find the optimal threshold.

## Results

### Temperature

| Metric | Value |
|---|---:|
| Optimal temperature T | 1.0312 |
| Interpretation | Model is already well-calibrated (T ≈ 1.0) |

The temperature is very close to 1.0, meaning the model's confidence scores
are already well-calibrated. Temperature scaling provides minimal benefit.

### Gate Analysis (with T=1.0312)

| Gate | OOS Failures | OOS Accuracy | Supported Gated | Supported Failures |
|---:|---:|---:|---:|---:|
| 0.500 | 14 | 98.39% | 3 | 0 |
| 0.550 | 10 | 98.85% | 3 | 0 |
| 0.600 | 7 | 99.19% | 2 | 0 |
| 0.650 | 6 | 99.31% | 2 | 0 |
| 0.700 | 3 | 99.65% | 2 | 0 |
| 0.750 | 1 | 99.88% | 2 | 0 |
| **0.800** | **0** | **100.00%** | **1** | **0** |
| 0.850 | 0 | 100.00% | 1 | 0 |
| 0.900 | 0 | 100.00% | 1 | 0 |
| 0.950 | 0 | 100.00% | 0 | 0 |

### Key Findings

1. **T ≈ 1.0** — the model is already well-calibrated. Temperature scaling
   does not meaningfully change the confidence distribution.

2. **Gate 0.80 with T=1.0312** achieves 0 OOS failures and accepts 1/5
   supported mappings — same as the current 0.85 gate without temperature.

3. **Gate 0.75** would accept 2/5 supported mappings but introduces 1 OOS
   failure — not worth the safety trade-off.

4. **The supported mappings issue is a training problem, not a calibration
   problem.** The model genuinely has low confidence for 3/5 supported
   browser-search examples (0.156, 0.360, 0.614). Temperature scaling cannot
   fix this because it preserves the ranking.

5. **Max wrong OOS confidence** dropped from 0.8015 (without T) to 0.7801
   (with T=1.0312) — a small improvement that slightly widens the safety
   margin.

### Confidence Distribution

| Metric | Without T | With T=1.0312 |
|---|---:|---:|
| Max wrong OOS confidence | 0.8015 | 0.7801 |
| Supported: search the web... | 0.9366 | ~0.93 |
| Supported: find a credit... | 0.8037 | ~0.78 |
| Supported: google the price... | 0.6144 | ~0.58 |
| Supported: look up student... | 0.3602 | ~0.33 |
| Supported: google "odell..." | 0.1560 | ~0.14 |

## Conclusion

Temperature scaling confirms the model is well-calibrated. The 0.85 gate
remains appropriate — it provides perfect OOS safety (0 failures) with
minimal supported mapping acceptance. The supported mapping issue requires
more browser_search training data, not calibration.

## Recommendation

Keep the 0.85 confidence gate. Do not apply temperature scaling in
production (T ≈ 1.0 provides no benefit). Address supported mapping
confidence through future training data expansion.

## Files

| File | Purpose |
|---|---|
| `server/nlu/calibrate_temperature.py` | Temperature fitting and gate analysis |
| `server/nlu/model/candidate/temperature_calibration.json` | Calibration report |

## Explicitly Not Done

- The production ONNX model was not modified.
- The 0.85 confidence gate was not changed.
- No temperature scaling was applied to production.
- The candidate was not promoted.
