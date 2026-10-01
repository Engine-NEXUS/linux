# NEXUS Testing Documentation

This directory contains repeatable testing procedures and release gates. Historical test results remain in their dated files elsewhere under `docs/`; documents here define the current procedure future contributors should follow.

| Area | Document | Purpose |
|---|---|---|
| NLU/BERT-Mini | [NLU Future Testing and Model Promotion Playbook](./nlu-future-testing-and-model-promotion-playbook.md) | Complete dataset, model, slot, OOS, safety, voice, external-data, training, promotion, and rollback procedure |
| Data foundations | [Data Foundation and Wake-Model Gates](./data-foundation-and-wake-model-gates.md) | NLU provenance, frozen evaluation hashes, wake-audio grouped splits, model fingerprints, and three-pass verification |
| NLU Phase 1 | [Phase 1 Evaluation Split Analysis](./phase-1-nlu-evaluation-split-analysis-2026-09-14.md) | Completed phrase-family-separated train, validation, calibration, final-test, and quarantine architecture with three-pass verification |
| NLU Phase 2 | [CLINC OOS Semantic Review](./phase-2-clinc-oos-semantic-review-2026-09-14.md) | Conservative NEXUS capability mapping, reviewed OOS baseline, supported-search trade-off, locked artifacts, and three-pass verification |
| NLU Phase 3 | [Candidate OOS Training Experiment](./phase-3-candidate-oos-training-experiment-2026-09-14.md) | Candidate-only retraining with 85 CLINC OOS rows: OOS rejection improved but final-test accuracy regressed; candidate NOT promoted |
| NLU Phase 4 | [Expanded Training Candidate Experiment](./phase-4-expanded-training-candidate-experiment-2026-09-14.md) | 455 structurally diverse training examples added: final-test accuracy recovered to 83.41%, OOS failures down to 4; candidate NOT promoted |
| NLU Phase 5 | [Verb-Aligned Candidate Experiment](./phase-5-verb-aligned-candidate-experiment-2026-09-14.md) | 204 verb-aligned examples: merge_pr 10%→100%, OOS failures down to 2, final-test 86.73%; candidate NOT promoted |
| NLU Phase 6 | [Targeted Candidate Experiment](./phase-6-targeted-candidate-experiment-2026-09-14.md) | 110 targeted examples: 12/13 weak intents at 100%, OOS failures down to 1, final-test 87.17%; candidate NOT promoted |
| NLU Phase 7 | [get_pr Closing Experiment](./phase-7-get-pr-closing-experiment-2026-09-14.md) | 110 get_pr-focused examples: OOS failures down to 0 (perfect), final-test 86.28%; get_pr gap is structural; candidate NOT promoted |
| NLU Phase 8 | [Temperature Scaling](./phase-8-temperature-scaling-2026-09-14.md) | T=1.0312 (well-calibrated), 0.85 gate confirmed optimal: 0 OOS failures, supported mapping issue is training not calibration |
| Wake word Phase A | [Phase A Production Retrain](./wake-word-phase-a-production-retrain-2026-09-14.md) | Baseline classifier retrain on pipeline v2 with 5K positive and negative clips |
| Wake word Phase B | [Phase B Audio Preprocessing](./wake-word-phase-b-audio-preprocessing-2026-09-14.md) | Audio normalization, high-pass filtering, dynamic gain scaling, and VAD |
| Wake word Phase C | [Phase C SpecAugment & Speed](./wake-word-phase-c-specaugment-speed-perturbation-2026-09-14.md) | SpecAugment time/freq masking and speed perturbation augmentation experiments |
| Wake word Phase D | [Phase D Speaker Verification](./wake-word-phase-d-speaker-verification-2026-09-14.md) | 512-dim ResNet speaker embeddings and cosine verification gates |
| Wake word Phase E | [Phase E Real Sample Recording](./wake-word-phase-e-real-sample-recording-2026-09-14.md) | Real microphone recording protocol, acoustic variation, and noise floors |
| Wake word Decision | [Training Decision 2026-09-15](./wake-word-training-decision-2026-09-15.md) | Final go/no-go release gates and deployment sign-off |
| Installer | [Installer Testing Checklist](../installer-testing-checklist.md) | Installer-specific validation |
| Full system | [Full System Test — 2026-09-08](../full-system-test-2026-09-08.md) | Historical end-to-end system test |
| Historical results | [Test Results — 2026-09-08](../test-results-2026-09-08.md) | Historical test outcome snapshot |
| Wake word | [Wake-Word Testing Strategy](../wake-word/11-testing-strategy.md) | Wake-word model validation procedure |
| Wake word Tier 3 | [Tier 3 Testing Strategy](../wake-word/19-tier3-testing-strategy.md) | Wake-word Tier 3 validation |
| Changelog Snapshot | [Comprehensive Changelog 2026-09-14](./comprehensive-changelog-2026-09-14.md) | Multi-phase training and audit retrospective |


## Release Verification Matrix (100% Passing)

| Subsystem / Suite | Command | Tests Run | Result | Duration |
|---|---|---|---|---|
| **Rust Unit Tests** | `cargo test --lib -- --test-threads=1` | **522 / 522** | **PASS** (100%) | ~31s |
| **Rust Compilation** | `cargo check` | **Entire Crate** | **PASS** (0 errors) | ~9s |
| **Frontend Production Build** | `npm run build --prefix frontend` | **820 modules** | **PASS** (100%) | ~6s |
| **Frontend Vitest Suite** | `npm test --prefix frontend` | **20 / 20** | **PASS** (100%) | ~1s |
| **Cloudflare Worker Suite** | `npm test --prefix server/worker` | **49 / 49** | **PASS** (100%) | ~0.6s |
| **NLU Data Foundation** | `python server/nlu/data_foundation.py validate` | **481 locked rows** | **PASS** (0 leakage) | ~1s |

## Standard NLU sequence

```powershell
nexus audit --dataset-only
nexus collect
nexus train --keep-temp
nexus audit
cd src-tauri
cargo check
cargo test --lib
cd ..
nexus build
nexus check
```

Do not promote a model merely because training or ONNX export completed. Follow the promotion gates in the NLU playbook.
