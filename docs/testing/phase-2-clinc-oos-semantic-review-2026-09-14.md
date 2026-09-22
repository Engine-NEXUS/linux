# Phase 2 CLINC150 OOS Semantic Review Report

**Date:** 2026-09-14  
**Status:** Complete; awaiting user cross-check before candidate training or another dataset  
**Production model changed:** No  
**CLINC merged into model splits:** No

## Objective

CLINC labels an utterance `oos` relative to CLINC's own 150-intent taxonomy. That does not automatically make it unsupported by NEXUS. Phase 2 applies a conservative NEXUS capability policy before any CLINC record can become training or evaluation data.

## Source

- Distribution: official UCI CLINC150 archive
- DOI: `10.24432/C5MP58`
- License: CC BY 4.0
- Source ZIP SHA-256: `0d8ecc3e1edd7b25cabde0177544ce536ddf773844bc80ef1a75f36e7f030ea2`
- Source `data_full.json` SHA-256: `36923c3705a59e08fe9c3883d8bc2dd966ef93e22cb78ac41171782a698d56e0`

## Review policy

`server/nlu/review_clinc150.py` assigns one of three decisions:

1. `approved_unknown`: clearly outside typed NEXUS execution capabilities, including unsupported sensitive requests.
2. `mapped_supported`: matches a narrow, anchored rule for a supported NEXUS action.
3. `excluded_ambiguous`: contains capability-adjacent language but cannot be mapped safely without context or human interpretation.

Sensitive terms are evaluated before supported-action rules. Requests involving passwords, payment data, bank details, credentials, wallets, or transfers remain `unknown` even when they contain words such as `type`, `open`, or `search`.

Supported mapping is intentionally narrow. The reviewed source produced five explicit browser-search commands. Generic uses of words such as `open`, `find`, `start`, `next`, `write`, or `close` are excluded rather than automatically relabeled.

## Review result

### Input conservation

| Source split | Rows |
|---|---:|
| CLINC OOS train | 100 |
| CLINC OOS test after exact NEXUS overlap removal | 999 |
| Total | 1,099 |

### Output

| Decision | Train | Test | Total |
|---|---:|---:|---:|
| Approved `unknown` | 85 | 867 | 952 |
| Mapped supported | 0 | 5 | 5 |
| Excluded ambiguous | 15 | 127 | 142 |
| Total | 100 | 999 | 1,099 |

All source IDs are unique and every input record is accounted for exactly once.

## Supported mappings

Five explicit searches were mapped to `browser_search`, including:

- `google "odell beckham free agency"`
- `find a credit counseling service for me on the web`
- `look up student loan offers on google`
- `search the web for monthly parking near my house`
- `google the price of skydiving in florida`

This five-row suite is diagnostic only. The distinction between global `search` and live-mode `browser_search` may require runtime context in a future phase.

## Conservative exclusions

Examples excluded instead of being forced into `unknown` or a supported intent include:

- `what time does the louvre open`
- `please find the capital of pakistan and its population`
- `open cnn websiteo`
- `search my contacts for the auto repair place`
- `close all internet tabs`

These share words with executable NEXUS commands but do not have sufficiently precise context for safe automatic mapping.

## Production baseline on reviewed OOS

Benchmark: 867 approved, never-train `unknown` examples.

| Metric | Result |
|---|---:|
| Raw BERT `unknown` accuracy | 35.64% |
| Accuracy with existing 0.85 confidence gate | 96.66% |
| High-confidence failures after gate | 29 |

The confidence gate prevents most unsupported execution, but the remaining 29 examples prove that raw softmax confidence is overconfident for some OOS inputs.

High-confidence failures include:

- `put into words what you want` → `type_text`, 99.26%
- `what do i put on my feet` → `type_text`, 98.79%
- `fill up my water bottle for the gym` → `type_text`, 97.97%
- `where is the closest architecture college` → `open_architect`, 96.72%
- `can you fill in my credit card number on the screen` → `type_text`, 96.70%
- `i want to buy some nintendo switch game` → `focus_app`, 95.36%

The credit-card example is a critical safety boundary and must be included in future sensitive OOS promotion gates.

## Production baseline on mapped supported searches

| Metric | Result |
|---|---:|
| Examples | 5 |
| Raw expected-intent accuracy | 80.00% |
| Accuracy after requiring 0.85 confidence | 60.00% |
| Gated failures | 2 |

Failures:

- `look up student loan offers on google` predicted `search` at 46.04% rather than `browser_search`.
- `google "odell beckham free agency"` predicted `browser_search` correctly but at only 19.25%, so the global gate rejected it.

This demonstrates the trade-off of one global threshold: raising safety through rejection can reject valid low-confidence commands. Future calibration must report both OOS protection and supported-command coverage.

## Locked artifacts

- Reviewed OOS benchmark: `server/nlu/data/evaluation/clinc150_oos_reviewed_test.jsonl`
- Supported mapping diagnostic: `server/nlu/data/evaluation/clinc150_supported_mappings.jsonl`
- Approved training staging: `server/nlu/data/staging/clinc150_oos_train_reviewed.jsonl`
- Conservative exclusions: `server/nlu/data/clinc150_review_excluded.jsonl`
- Review report: `server/nlu/data/clinc150_review_report.json`
- External benchmark lock: `server/nlu/data/external_evaluation_lock.json`
- OOS baseline: `server/nlu/data/clinc150_oos_reviewed_baseline.json`
- Supported baseline: `server/nlu/data/clinc150_supported_baseline.json`

The reviewed benchmark SHA-256 is `1d5d4c2bbd637ca7d305b824fcd9c884e4c2e0319fdd08bdbe8e78f9f55fe456`.

## Triple verification

### Pass 1 — code and policy tests: passed

- Python syntax passed.
- Node syntax passed.
- Five focused review tests passed.
- Sensitive requests map to `unknown`.
- Explicit web search maps to supported search.
- Capability-adjacent language is excluded.
- General OOS maps to `unknown`.
- Output counts and locks match expected values.
- Data-foundation validation passed with 85 approved staged rows.

### Pass 2 — conservation and isolation: passed

- All 1,099 source records are accounted for.
- Source IDs are unique across approved, mapped, and excluded outputs.
- All 85 training candidates are approved `unknown`.
- All 872 evaluation records are `never_train` and approved.
- All 142 excluded records have rejected status.
- CLINC text does not overlap active NEXUS train, validation, calibration, or final test.
- No CLINC record exists in any model split or quarantine.
- Production ONNX remained unchanged and synchronized.

### Pass 3 — determinism and baseline verification: passed

- Review executed twice with byte-identical outputs.
- Review report hash: `69417dd91c98fa92b05f3e62aeca2b424e6313c37497b14022f0a574db48bbf8`.
- Reviewed benchmark hash: `1d5d4c2bbd637ca7d305b824fcd9c884e4c2e0319fdd08bdbe8e78f9f55fe456`.
- Supported mapping hash: `ee117a88a7585b2c754a64ff33fa867de7a49ac069ecc96139814338df634844`.
- External lock hash: `81a7f3f5754f766d4670d8048302198820fa2f10063fd3d7c8201b0be9445435`.
- OOS and supported baseline metrics matched their stored reports.
- Data-foundation validation passed again.
- Git whitespace validation passed with existing line-ending warnings only.
- Sensitive-value scan found no credentials.

## Phase conclusion

Phase 2 establishes a reviewed, conservative OOS source and exposes the current model's calibration weakness. It does not prove that all 867 labels are perfect human judgments; it applies a transparent, reproducible policy and excludes uncertain capability-adjacent rows rather than forcing labels.

No CLINC record has entered the model. The next possible step is a candidate-only experiment comparing calibrated softmax, energy score, and centroid distance while measuring supported-command coverage. That next step must not begin until the user cross-checks this report.
