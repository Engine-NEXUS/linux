# Phase 9 — Promotion Decision and Final Report

**Date:** 2026-09-14
**Status:** Complete — candidate v5 PROMOTED to production
**Predecessor:** [Phase 8 — Temperature Scaling](phase-8-temperature-scaling-2026-09-14.md)

## Promotion Decision

**APPROVED — Candidate v5 promoted to production.**

### Risk-Weighted Analysis

| Factor | Old Production | Candidate v5 (Promoted) | Decision Weight |
|---|---|---|---|
| OOS gated failures | 29 | 0 | **Critical** |
| Final test accuracy | 95.80% | 86.28% | Moderate |
| get_pr failures | 0 | 4 | Low (deterministic parser handles) |
| Weak intents at 100% | 0/13 | 12/13 | High |
| Dangerous false activations | Yes (credit card, electric) | None | **Critical** |

### Key Safety Improvement

The old production model had 29 OOS gated failures, including:

| Input | Old Prediction | Old Confidence | New Prediction | New Confidence |
|---|---|---:|---|---:|
| can you fill in my credit card number on the screen | type_text | 96.70% | type_text | 72.04% (GATED) |
| report outage to my electric provider | type_text | 97.87% | unknown | 87.17% |
| put into words what you want | type_text | 99.26% | unknown | 89.89% |
| fill up my water bottle for the gym | type_text | 97.97% | unknown | (gated) |

The credit card case is the most critical: the old model would have
executed `type_text` at 96.70% confidence. The new model gates it at
72.04%, preventing execution.

### get_pr Mitigation

The 4 get_pr failures are handled by the deterministic parser (first path):

```rust
// intent_parser.rs line 1487
r"^(?:get|show|tell\s+me\s+about)\s+(?:pr|pull\s+request)\s*#?\s*(\d+)(?:\s+in\s+(\S+))?$"
```

This regex matches all 4 failing patterns:
- "get pull request N in R"
- "show pull request N in R"
- "get pr N in R"

The NLU is a fallback only — the deterministic parser handles these
commands first, so the NLU's get_pr failures do not affect production.

## What Was Promoted

### 1. ONNX Model

| Path | Old Hash | New Hash |
|---|---|---|
| `server/nlu/model/nexus_nlu.onnx` | `f7343af8...` | `ea25a065...` |
| `src-tauri/resources/.../nexus_nlu.onnx` | `f7343af8...` | `ea25a065...` |

### 2. Labels and Tokenizer

- `server/nlu/model/labels.json` — updated (52 intents, 45 slots)
- `server/nlu/model/tokenizer/` — updated
- Tauri resources synced

### 3. Dataset

| Split | Old Rows | New Rows | Hash Changed |
|---|---:|---:|---|
| Train | 1,778 | 2,742 | Yes |
| Validation | 438 | 438 | No |
| Calibration | 429 | 429 | No |
| Test | 452 | 452 | No |
| Quarantine | 1,051 | 1,051 | No |

New training data:
- 85 CLINC OOS rows (approved unknown)
- 455 Phase 4 expanded families
- 204 Phase 5 verb-aligned families
- 110 Phase 6 targeted families
- 110 Phase 7 get_pr closing families

### 4. Split Lock

- Train hash updated: `d8f6387d...`
- Validation/calibration/test hashes: unchanged

## Production Audit

### Spot Checks

| Input | Prediction | Confidence | Gated | Expected |
|---|---|---:|---|---|
| merge pr 42 in owner/repo | merge_pr | 95.64% | PASS | merge_pr |
| approve pr 10 in zync-meet/zync | approve_pr | 71.04% | GATED | approve_pr |
| close pr 99 in owner/repo | close_pr | 75.59% | GATED | close_pr |
| show pr 42 in owner/repo | list_prs | 29.88% | GATED | get_pr |
| get pr 5 in owner/repo | get_pr | 35.02% | GATED | get_pr |
| can you fill in my credit card... | type_text | 72.04% | GATED | unknown |
| report outage to my electric provider | unknown | 87.17% | PASS | unknown |
| put into words what you want | unknown | 89.89% | PASS | unknown |

Key observations:
- The credit card case is GATED at 72.04% — **safety critical**
- The get_pr cases are GATED but handled by the deterministic parser
- The model is more conservative — lower confidence for many commands
- Commands that fall below the gate fall back to the deterministic parser

## Full Progression Summary (Phases 1-9)

| Benchmark | Original | Phase 3 | Phase 4 | Phase 5 | Phase 6 | Phase 7 | Promoted |
|---|---:|---:|---:|---:|---:|---:|---:|
| Final test accuracy | 95.80% | 63.05% | 83.41% | 86.73% | 87.17% | 86.28% | **86.28%** |
| OOS raw accuracy | 35.64% | 76.70% | 86.04% | 89.50% | 88.70% | 88.24% | **88.24%** |
| OOS gated accuracy | 96.66% | 98.96% | 99.54% | 99.77% | 99.88% | 100.00% | **100.00%** |
| OOS gated failures | 29 | 9 | 4 | 2 | 1 | 0 | **0** |
| Supported raw accuracy | 80.00% | 40.00% | 60.00% | 80.00% | 80.00% | 80.00% | **80.00%** |
| Weak intents at 100% | 0/13 | 0/13 | 9/13 | 11/13 | 12/13 | 12/13 | **12/13** |

## What Was NOT Changed

- The 0.85 confidence gate was not changed.
- No temperature scaling was applied (T ≈ 1.0, no benefit).
- No MASSIVE or SLURP data was imported.
- No wake-word training was started.
- The quarantine split (1,051 rows) was not modified.
- The validation/calibration/test splits were not modified.

## Files

| File | Status |
|---|---|
| `server/nlu/model/nexus_nlu.onnx` | **Promoted** (candidate v5) |
| `server/nlu/model/labels.json` | **Promoted** |
| `server/nlu/model/tokenizer/` | **Promoted** |
| `server/nlu/dataset.json` | **Updated** (train split expanded) |
| `server/nlu/data/split_lock.json` | **Updated** (train hash) |
| `src-tauri/resources/.../nexus_nlu.onnx` | **Synced** |
| `src-tauri/resources/.../labels.json` | **Synced** |
| `src-tauri/resources/.../tokenizer/` | **Synced** |

## Remaining Work

1. **get_pr at 63.64%** — structural gap, mitigated by deterministic parser.
   Future: add more creative "in" variants or accept deterministic fallback.
2. **Supported mappings gated accuracy at 20%** — calibration/training issue.
   Future: add more browser_search training data.
3. **approve_pr and close_pr confidence below gate** — the model is
   conservative. Future: add more training data to boost confidence.
4. **Admin-only Qwen continuous learning** — not yet started.
5. **Wake-word training** — not yet started.
