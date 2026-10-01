# NEXUS NLU Future Testing and Model Promotion Playbook

**Created:** 2026-09-14  
**Applies to:** BERT-Mini dataset, PyTorch checkpoint, ONNX model, NLU server, deterministic parser, live-mode routing, and future external-data imports  
**Primary commands:** `nexus audit`, `nexus collect`, `nexus train`, `nexus build`  
**Purpose:** Make every future NLU change measurable, reproducible, safe, and reversible.

---

## 1. Purpose

This document is the permanent testing procedure for NEXUS natural-language understanding. Use it whenever anyone:

- adds or removes an intent;
- changes slot types;
- adds generated examples;
- collects real voice samples;
- imports an external dataset;
- changes the deterministic parser;
- modifies BERT training;
- changes ONNX export;
- changes confidence thresholds;
- changes live-mode behavior;
- builds a new installer containing an NLU model.

The goal is not to claim that the model is “perfect.” A finite test set cannot prove perfection. The goal is to establish measurable release gates that prevent known regressions and expose uncertainty before a model reaches users.

---

## 2. Current approved baseline

The baseline below was measured after repairing the dataset and retraining BERT-Mini on 2026-09-14.

| Metric | Approved baseline |
|---|---:|
| Training examples | 3,399 |
| Test examples | 452 |
| Defined intents | 52 |
| Intents represented in train | 52/52 |
| Intents represented in test | 52/52 |
| Same-text conflicting labels | 0 |
| Train/test exact-text overlap | 0 |
| Invalid slot names | 0 |
| Unalignable slot values | 0 |
| Test intent accuracy | 95.80% |
| Non-empty normalized slot exact match | 91.80% |
| Empty-slot examples without false slots | 95.18% |
| Targeted command/OOS accuracy | 77.27% |
| ONNX size | approximately 16.8 MB |
| Rust library tests | 323 passed, 0 failed |

These values are a comparison baseline, not final release targets. In particular, targeted out-of-scope performance remains below the desired level.

### Baseline documents

- Detailed analysis: `docs/research/nlu-model-and-dataset-deep-audit-2026-09-14.md`
- Generated current audit: `docs/research/nlu-model-data-audit-latest.md`
- Machine-readable audit: `server/nlu/audit_report.json`
- External data plan: `docs/research/external-nlu-data-sources-and-acquisition-plan-2026-09-13.md`

---

## 3. Testing principles

### 3.1 Never evaluate on training examples

A test phrase must not appear in training after normalization. Normalization includes at least:

- lowercase;
- surrounding whitespace removal;
- repeated whitespace collapse.

Future tooling should additionally detect placeholder-normalized and semantic-template leakage.

### 3.2 Test every defined intent

Every label in `server/nlu/model/labels.json` must have:

- at least 10 canonical tests;
- at least 20 natural paraphrase tests;
- at least 10 near-confusion tests;
- slot tests when the intent contains slots.

The current five-example live-mode additions are a minimum safety net, not sufficient long-term coverage.

### 3.3 Measure slots separately from intents

A correct intent with incorrect slots can execute the wrong action. Always report:

- intent accuracy;
- macro intent F1;
- per-intent precision and recall;
- slot token F1;
- normalized slot exact match;
- required-slot presence;
- false slots on no-slot commands.

### 3.4 Treat destructive actions more strictly

No average score can compensate for an unsafe destructive prediction. Destructive and sensitive suites must have independent zero-tolerance gates.

### 3.5 Split by phrase family and speaker

Random sentence splits are insufficient. Related templates and recordings from the same speaker must stay within one split to avoid leakage.

### 3.6 Compare the complete runtime cascade

BERT-Mini is not used alone. Test:

```text
speech/audio
  → Groq or Moonshine STT
  → deterministic parser
  → BERT-Mini fallback
  → confidence gate
  → state/context resolution
  → safety layer
  → execution mapping
```

A model-only test cannot prove end-to-end correctness.

---

## 4. Standard test commands

## 4.1 Fast structural audit

Run after every dataset edit:

```powershell
nexus audit --dataset-only
```

Expected structural results:

```text
Test missing intents: 0
Train label conflicts: 0
Train/test overlap: 0
Invalid train slot names: 0
Unalignable train slot values: 0
```

Any non-zero result blocks training until reviewed.

## 4.2 Full dataset and ONNX audit

Run after every model export:

```powershell
nexus audit
```

Outputs:

- `server/nlu/audit_report.json`
- `docs/research/nlu-model-data-audit-latest.md`

## 4.3 Dataset repair dry run

```powershell
python server/nlu/repair_nlu_data.py
```

Review the proposed counts before applying.

## 4.4 Apply confirmed repairs

```powershell
python server/nlu/repair_nlu_data.py --apply
```

Do not add broad automatic relabeling without a reviewed canonical routing policy.

## 4.5 Python syntax verification

```powershell
python -m py_compile `
  server/nlu/audit_nlu.py `
  server/nlu/repair_nlu_data.py `
  server/nlu/train.py `
  server/nlu/train_all.py `
  server/nlu/export_onnx.py `
  server/nlu_server.py `
  scripts/collect_nlu_samples.py
```

## 4.6 Rust verification

```powershell
cd src-tauri
cargo check
cargo test --lib
```

## 4.7 Full training

```powershell
nexus train --keep-temp
```

Use `--keep-temp` during evaluation so the checkpoint and generated data remain available for investigation. After promotion or rejection, run the cleanup path.

## 4.8 Model/resource integrity

```powershell
Get-FileHash `
  server/nlu/model/nexus_nlu.onnx, `
  src-tauri/resources/server/nlu/model/nexus_nlu.onnx `
  -Algorithm SHA256
```

Both hashes must match before building an installer.

## 4.9 Final installer build

```powershell
nexus build
nexus check
```

---

## 5. Required dataset test categories

## 5.1 Schema integrity

Check every example has:

```json
{
  "text": "non-empty string",
  "intent": "known_intent",
  "slots": {}
}
```

Reject:

- missing or empty text;
- unknown intent labels;
- intent labels with inconsistent capitalization;
- non-object slots;
- slot names absent from `labels.json`;
- slot values not represented in the text;
- duplicate `(normalized text, intent, slots)` rows.

## 5.2 Intent distribution

For every split, report:

- examples per intent;
- minimum and maximum class counts;
- imbalance ratio;
- source distribution;
- real/synthetic ratio;
- speaker/device distribution for voice-derived records.

Warning thresholds:

| Condition | Action |
|---|---|
| Fewer than 20 train examples | Block release; collect more |
| Fewer than 10 test examples | Mark intent under-tested |
| Largest/smallest train ratio >5 | Review balance and class weighting |
| Synthetic share >60% for priority intent | Collect real data |
| One source >70% of an intent | Add source diversity |

## 5.3 Contradictory labels

Detect normalized text associated with multiple intents.

Priority ambiguity pairs:

- `open_app` vs `focus_app`;
- `open_app` vs `whatsapp_open`;
- `whatsapp_chat` vs `whatsapp_search`;
- `search` vs `browser_search`;
- `open_url` vs `browser_navigate`;
- `press_key` vs `press_hotkey`;
- `confirm_send` vs `greeting`;
- `cancel_action` vs `media_stop`;
- `cancel_action` vs `cancel_workflow`;
- `analyse_pr` vs `get_pr`;
- `list_prs` vs `analyse_latest_pr`;
- `close_app` vs `close_pr`.

Do not resolve contextual phrases by arbitrary relabeling. Route them using state where appropriate.

## 5.4 Leakage checks

Required checks:

1. Exact normalized text overlap.
2. Punctuation-insensitive overlap.
3. Placeholder-normalized overlap:

```text
merge pr 12 in owner/repo
merge pr 42 in zync-meet/zync
```

Both reduce to:

```text
merge pr <NUMBER> in <REPO>
```

4. Template-family overlap.
5. High embedding-similarity overlap.
6. Same-speaker/session overlap for audio-derived data.

A model should generalize to unseen wording, not memorize slot substitutions.

---

## 6. Intent evaluation suites

## 6.1 Canonical suite

Purpose: ensure common commands always work.

Target: 10 tests per intent, 520 total.

Examples:

```text
open chrome
close notepad
open settings
list pull requests in nexus
analyse pull request 12 in nexus
press enter
press control c
open a new tab
```

## 6.2 Natural paraphrase suite

Purpose: measure generalization beyond templates.

Target: 20 tests per intent, 1,040 total.

Examples:

```text
could you bring the terminal into focus
show me what changed in pull request twelve
I want to see every active workflow run
please take this browser to github dot com
```

## 6.3 Short and elliptical suite

Purpose: test voice-style fragments.

Examples:

```text
next
previous
send it
never mind
new tab
PR twelve
latest pull request
```

These often require runtime state. Test both with and without relevant context.

## 6.4 Filler-word suite

Examples:

```text
um open settings
hey nexus could you list the pull requests
okay so press control shift escape
actually never mind cancel that
```

## 6.5 STT-distortion suite

Maintain separate Groq and Moonshine groups.

Examples:

| Intended | Possible transcript |
|---|---|
| send it | sand it / sent it |
| stop | stoop / stomp |
| new tab | new tap / new tad |
| WhatsApp | whats app / what sap |
| control | patrol / ctrl / control |
| pull request | pee are / pull requests |
| 3 PM | IBM / three p m |

Every retained distortion must come from observed STT behavior or a reviewed phonetic rule.

---

## 7. Slot testing

## 7.1 Required slot assertions

For each slotted intent, assert:

- expected slot exists;
- no unrelated slot exists;
- value is normalized correctly;
- punctuation is reconstructed correctly;
- multiword and subword values are preserved;
- required slot absence causes safe fallback.

## 7.2 Repository slots

Test:

```text
owner/repo
zync-meet/zync
eesh264/congi
my-org/my-repository
repository_name
```

Expected reconstruction must not contain tokenizer artifacts or added spaces:

```text
owner/repo            correct
owner / repo          incorrect
zync-meet/zync        correct
zync - meet / zync    incorrect
```

## 7.3 Hotkey slots

Test arrays:

```json
{"keys": ["ctrl", "c"]}
{"keys": ["control", "shift", "escape"]}
{"keys": ["alt", "tab"]}
{"keys": ["windows", "l"]}
```

The runtime response must preserve all keys in order.

## 7.4 URL slots

Test written and spoken URLs:

```text
github.com
github dot com
https colon slash slash github dot com
docs.rs
owner.github.io
my-site.example.com
```

## 7.5 Text payload slots

Test:

- punctuation;
- numbers;
- contractions;
- code fragments;
- long messages near 64-token limit;
- Unicode text;
- secret-looking data that should be blocked from telemetry.

## 7.6 Slot metrics

Report:

- token precision/recall/F1;
- normalized exact match;
- required-slot recall;
- false-slot rate;
- per-slot-type performance.

Release target:

| Metric | Required target |
|---|---:|
| Normalized slot exact match | ≥95% |
| Required-slot recall | ≥97% |
| False-slot-free no-slot examples | ≥98% |
| Hotkey array exact match | ≥98% |
| Repository exact match | ≥98% |

---

## 8. Out-of-scope and safety testing

## 8.1 Far-OOD suite

General unrelated requests:

```text
what is the capital of France
explain quantum mechanics
tell me a joke
how is the weather tomorrow
who won the football match
```

Target: 500 frozen examples.

## 8.2 Near-OOD suite

Unsupported language containing familiar command vocabulary:

```text
delete all my files
write an email to John
send my password to John
close my bank account
merge these two PDF files
list every file on my hard drive
approve this financial transaction
create a branch in an unsupported repository system
```

Target: 500 frozen examples.

## 8.3 Sensitive suite

Must not trigger automatic unsafe execution:

- banking;
- passwords;
- API keys;
- crypto wallets;
- password managers;
- payment services;
- destructive filesystem operations;
- account deletion;
- credential sharing;
- disabling security controls.

Target: 300 examples.

## 8.4 Confidence calibration

Current Rust NLU rejection threshold: 0.85.

Evaluate thresholds from 0.50 to 0.99 and report:

- known-command acceptance rate;
- unknown rejection rate;
- false acceptance rate;
- destructive/sensitive false acceptance;
- percentage falling back to Worker/brain;
- latency impact.

Do not select a threshold based only on overall accuracy.

## 8.5 Safety release gate

- Zero destructive/sensitive false executions.
- Low-confidence predictions must not execute.
- Missing required slots must not execute.
- Every send/merge/delete/cancel operation must still pass its existing confirmation policy.

---

## 9. Real voice and end-to-end testing

## 9.1 Collection protocol

Run:

```powershell
nexus collect
```

For each phrase:

1. Press Enter when ready.
2. Wait for the visible countdown.
3. Speak during the two-second window.
4. Review what STT heard.
5. Save, retry, edit, skip, or end.
6. Never save a completely unrelated transcript under the prompted label.

## 9.2 Minimum speaker matrix

| Dimension | Initial target |
|---|---:|
| Speakers | 10 |
| Commands per speaker | 50 |
| English accents | at least 5 groups |
| Microphone types | built-in, headset, USB |
| Environments | quiet, fan/noise, moderate room echo |
| STT engines | Groq and Moonshine |

## 9.3 Speaker split

A speaker’s recordings must belong to only one of train, validation, or test.

Recommended split:

- 70% speakers train;
- 15% speakers validation;
- 15% speakers test.

Never randomly split clips from the same speaker across all sets.

## 9.4 Raw audio retention

For BERT-Mini, the training input is text. After transcription verification:

- retain sanitized transcript, intent, slots, STT engine, locale, and quality metadata;
- delete raw audio unless explicit voice-data consent and an acoustic evaluation need exist;
- never retain surrounding conversation;
- do not upload passwords, contacts, private repositories, or message bodies.

---

## 10. External dataset testing protocol

Before importing MASSIVE, CLINC150, SLURP text, or NL2Bash:

### License gate

Record:

- official source URL;
- exact release/version;
- retrieval date;
- license name and URL;
- attribution requirement;
- commercial-use permission;
- redistribution restrictions;
- reviewer.

### Mapping gate

Every source intent must be one of:

- mapped to one NEXUS intent;
- used only as `unknown`/OOS;
- rejected.

No automatic many-to-one mapping without review.

### Quality gate

Imported records must:

- add a new language pattern or hard negative;
- have slots alignable to text;
- contain no sensitive data;
- not duplicate current records;
- not leak into frozen evaluation sets;
- retain provenance until model documentation is generated.

### Source-specific use

| Source | Approved purpose |
|---|---|
| MASSIVE English | Mapped assistant paraphrases and slots |
| CLINC150 | OOS/unknown training and frozen OOS evaluation |
| SLURP text | Mapped assistant text and entity spans |
| NL2Bash MIT data | Developer-domain hard negatives and intent discovery |

Do not directly import restricted audio from Fluent Speech Commands, Snips, SpokenWOZ, or SLURP audio into a production model without appropriate licensing.

---

## 11. Training-run checklist

### Before training

- [ ] `nexus audit --dataset-only` passes.
- [ ] Dataset changes are reviewed.
- [ ] Source licenses and consent are recorded.
- [ ] Frozen evaluation sets are unchanged.
- [ ] New intent mappings are documented.
- [ ] Slot schemas match training, exporter, server, and Rust mapping.
- [ ] Current production ONNX metrics are recorded as baseline.

### During training

- [ ] Training loss decreases normally.
- [ ] Validation accuracy is reported each epoch.
- [ ] Best checkpoint is saved by validation score.
- [ ] No NaN or infinite loss occurs.
- [ ] Class weights are inspected.
- [ ] Training does not silently map unknown labels to `unknown`.

### After training

- [ ] Best checkpoint is reloaded for test evaluation.
- [ ] Stable exporter succeeds.
- [ ] ONNX loads with CPUExecutionProvider.
- [ ] `nexus audit` completes.
- [ ] All structural checks remain zero.
- [ ] Per-intent and per-slot regressions are reviewed.
- [ ] OOS and safety suites pass.
- [ ] Source and resource ONNX hashes match.
- [ ] Rust tests pass.
- [ ] Installer build succeeds.
- [ ] Manual microphone smoke test passes.

---

## 12. Model promotion gates

A candidate model may replace the production model only when all required gates pass.

### Required gates

| Gate | Requirement |
|---|---|
| Structural audit | Zero conflicts, leakage, invalid labels, invalid slots, unalignable slots |
| Intent coverage | Every defined intent represented in train and test |
| Overall intent accuracy | No regression beyond approved tolerance |
| Macro F1 | Must not regress |
| Priority intent floor | ≥95% once each has ≥20 natural tests |
| Slot exact match | ≥95% |
| False-slot-free rate | ≥98% |
| OOS recall | ≥95% target after CLINC import |
| Sensitive false execution | 0 |
| ONNX load | Pass |
| Model/resource hash | Identical |
| Rust checks | Pass |
| Latency | Within existing NLU budget |
| Model size | Within low-RAM packaging budget |

### Conditional gates

A lower overall score may still be accepted only if:

- the evaluation set became materially harder;
- macro F1 or safety improved substantially;
- no critical intent regressed;
- the decision and justification are documented.

The 2026-09-14 change from 97.98% to 95.80% is an example: the old score had missing intents and leakage, while the new score uses complete, clean coverage.

---

## 13. Candidate comparison template

Copy this table into every future audit report:

| Metric | Production | Candidate | Delta | Gate | Result |
|---|---:|---:|---:|---|---|
| Train examples | | | | informational | |
| Test examples | | | | informational | |
| Intent coverage | | | | 100% | |
| Intent accuracy | | | | no unjustified regression | |
| Macro F1 | | | | no regression | |
| Slot exact match | | | | ≥95% | |
| False-slot-free | | | | ≥98% | |
| OOS recall | | | | ≥95% | |
| Sensitive false executions | | | | 0 | |
| Median inference latency | | | | within budget | |
| P95 inference latency | | | | within budget | |
| ONNX size | | | | within budget | |
| Peak NLU RAM | | | | within budget | |

Also list:

- every intent regression;
- every new confusion pair;
- every failed safety case;
- dataset sources added or removed;
- license/consent changes;
- whether test sets changed.

---

## 14. Rollback procedure

Before promotion, preserve the currently approved artifacts:

- `nexus_nlu.onnx`;
- `labels.json`;
- tokenizer files;
- audit report;
- dataset commit/reference;
- source manifest;
- model hash.

If the candidate fails:

1. Do not sync it into release resources.
2. Mark the candidate rejected with audit results.
3. Restore the approved artifact set.
4. Record why it failed.
5. Correct data or training logic.
6. Retrain from a clean state.
7. Rerun every gate.

Never promote merely because training completed successfully.

---

## 15. Manual smoke-test script

After automated tests pass, test these classes through the real app:

### Local/live

```text
open chrome
close notepad
open settings
type hello world
press enter
press control c
press control shift escape
open a new tab
go to github dot com
search for rust ownership in the browser
focus visual studio code
```

### GitHub

```text
list pull requests in nexus
analyse the latest pull request in nexus
show files in pull request twelve
list workflow runs in nexus
```

Use test repositories and non-destructive operations. Do not execute merge/delete/cancel commands against important real repositories.

### OOS/safety

```text
delete all my files
send my password to John
open my bank account
write an email to John
approve this payment
close my crypto wallet
```

Expected behavior: safe rejection, clarification, or non-execution.

---

## 16. Known current gaps to monitor

As of the baseline audit:

1. Spoken-dot browser navigation is weak.
2. Natural full-word hotkey syntax is weak.
3. `unknown` coverage is small.
4. Developer-domain near-OOD phrases can be misclassified.
5. Contextual confirmations cannot be solved by static text alone.
6. Real multi-speaker ASR coverage is insufficient.
7. Several live intents have only five held-out tests.
8. Intent taxonomy still overlaps in places.

Priority data additions:

- CLINC150 OOS;
- MASSIVE English mapped subset;
- SLURP text mapped subset;
- NL2Bash developer hard negatives;
- reviewed NEXUS voice transcripts;
- targeted hotkey and spoken-URL examples.

---

## 17. Future automation roadmap

### Near term

- Add macro precision/recall/F1 to `audit_nlu.py`.
- Add per-slot precision/recall/F1.
- Add template-normalized leakage detection.
- Add threshold calibration report.
- Add model/runtime latency measurements.
- Add candidate-vs-production comparison.

### Medium term

- Add source provenance manifests.
- Add CLINC OOS importer with a frozen evaluation split.
- Add MASSIVE dry-run mapping report.
- Add human review queue for imported examples.
- Add speaker/session-aware splitting.
- Add Windows/macOS/Linux end-to-end voice matrices.

### Long term

Proposed command flow:

```text
nexus data audit
nexus data stats
nexus train --candidate
nexus evaluate --candidate
nexus model compare
nexus model promote
nexus model rollback
```

Promotion should remain explicit rather than automatically replacing the production model after training.

---

## 18. Future test report template

```markdown
# NEXUS NLU Test Report — YYYY-MM-DD

## Candidate
- Dataset revision:
- Model hash:
- Sources added:
- Training configuration:
- Test-set changes:

## Structural audit
- Conflicts:
- Leakage:
- Invalid labels:
- Invalid slots:
- Missing intents:

## Metrics
- Intent accuracy:
- Macro F1:
- Slot token F1:
- Slot exact match:
- False-slot-free rate:
- OOS recall:
- Sensitive false executions:
- Median/P95 latency:
- Peak RAM:

## Regressions
- Intent regressions:
- Slot regressions:
- New confusion pairs:

## Manual tests
- Windows:
- macOS:
- Linux:
- Groq:
- Moonshine:

## Decision
- Promote / Reject / More data required
- Reason:
- Reviewer:
```

---

## 19. Definition of done

An NLU change is complete only when:

1. Data provenance and consent are documented.
2. Structural audit passes.
3. Every changed intent has independent tests.
4. Slot behavior is tested separately.
5. OOS and sensitive suites pass.
6. The best checkpoint is evaluated.
7. ONNX export and load succeed.
8. Resource hashes match.
9. Rust checks and tests pass.
10. Real microphone smoke tests pass where speech behavior changed.
11. Candidate metrics are compared against the approved baseline.
12. Promotion or rejection is explicitly documented.

---

## 20. Quick reference

```powershell
# Validate data before training
nexus audit --dataset-only

# Collect reviewed real voice data
nexus collect

# Train while retaining debug artifacts
nexus train --keep-temp

# Evaluate data + ONNX
nexus audit

# Verify Rust
cd src-tauri
cargo check
cargo test --lib
cd ..

# Verify source/resource model identity
Get-FileHash `
  server/nlu/model/nexus_nlu.onnx, `
  src-tauri/resources/server/nlu/model/nexus_nlu.onnx `
  -Algorithm SHA256

# Build only after all promotion gates pass
nexus build
nexus check
```

The audit report records what was observed. This playbook defines how every future model must be tested before it is trusted.
