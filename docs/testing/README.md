# NEXUS Testing Documentation

This directory contains repeatable testing procedures and release gates. Historical test results remain in their dated files elsewhere under `docs/`; documents here define the current procedure future contributors should follow.

| Area | Document | Purpose |
|---|---|---|
| NLU/BERT-Mini | [NLU Future Testing and Model Promotion Playbook](./nlu-future-testing-and-model-promotion-playbook.md) | Complete dataset, model, slot, OOS, safety, voice, external-data, training, promotion, and rollback procedure |
| Installer | [Installer Testing Checklist](../installer-testing-checklist.md) | Installer-specific validation |
| Full system | [Full System Test — 2026-09-08](../full-system-test-2026-09-08.md) | Historical end-to-end system test |
| Historical results | [Test Results — 2026-09-08](../test-results-2026-09-08.md) | Historical test outcome snapshot |
| Wake word | [Wake-Word Testing Strategy](../wake-word/11-testing-strategy.md) | Wake-word model validation procedure |
| Wake word Tier 3 | [Tier 3 Testing Strategy](../wake-word/19-tier3-testing-strategy.md) | Wake-word Tier 3 validation |

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
