# NEXUS BERT-Mini Model and Dataset Deep Audit

**Audit date:** 2026-09-14  
**Dataset:** `server/nlu/dataset.json`  
**Model:** `server/nlu/model/nexus_nlu.onnx`  
**Audit command:** `nexus audit`  
**Status:** Structural defects repaired, model retrained, remaining semantic and OOS gaps documented

---

## Executive conclusion

The NEXUS BERT-Mini model is **not perfect**, and no finite dataset can prove perfection. It is now structurally much healthier and useful as a low-RAM intent router, but it still needs more real ASR data, hard out-of-scope examples, and broader natural phrasing for several live-mode intents.

### Final measured state

| Metric | Before repair | After repair/retraining | Interpretation |
|---|---:|---:|---|
| Training examples | 3,460 | 3,399 | Bad/conflicting/leaked rows removed |
| Test examples | 397 | 452 | 55 held-out live-mode tests added |
| Intents represented in test | 41/52 | 52/52 | Complete label coverage |
| Same-text conflicting labels | 29 | 0 | Fixed |
| Train/test exact-text overlap | 15 | 0 | Fixed |
| Invalid slot names | 6 | 0 | Fixed |
| Unalignable slot values | 15 | 0 | Fixed |
| Test intent accuracy | 97.98% (optimistic) | 95.80% (harder/cleaner) | Honest score decreased because test quality improved |
| Non-empty slot exact match | 13.17% | 91.80% | Major improvement after slot pipeline fixes |
| Empty examples without false slots | 4.76% | 95.18% | False slot hallucination largely fixed |
| Targeted command/OOS accuracy | 81.82% | 77.27% | Still weak; test became stricter and requires more hard negatives |

The old 97.98% intent score was misleading because:

- 11 live-mode intents had no test examples;
- 15 test phrases also appeared in training;
- many slot predictions were incorrect despite the intent being correct;
- only 8 `unknown` examples existed in the old test set;
- synthetic phrase families dominated coverage.

The new 95.80% score is lower but more trustworthy.

---

## 1. What was audited

### Dataset structure

- required top-level splits;
- example counts;
- every label against `labels.json`;
- malformed labels;
- duplicate rows;
- same text assigned to multiple intents;
- unknown slot names;
- required slot presence;
- slot values alignable to transcript text;
- train/test exact-text leakage;
- per-intent distribution and imbalance;
- missing test intents.

### ONNX model behavior

- model and tokenizer loadability;
- intent accuracy on the test set;
- per-intent accuracy;
- confidence distribution;
- intent confusion pairs;
- exact slot match;
- false slots on examples that should have no slots;
- targeted live-mode tests;
- targeted safety and out-of-scope tests.

### Training pipeline

- BERT token/slot alignment;
- list-valued slots (`keys`);
- padding/special-token masking;
- `O`-tag loss behavior;
- best-checkpoint selection;
- PyTorch-to-ONNX export;
- resource synchronization;
- post-training audit;
- temporary-file cleanup flow.

---

## 2. Critical defects found

## 2.1 Eleven intents had no test coverage

Missing from the original test set:

- `type_text`
- `press_key`
- `press_hotkey`
- `confirm_send`
- `cancel_action`
- `browser_new_tab`
- `browser_navigate`
- `browser_search`
- `whatsapp_open`
- `whatsapp_search`
- `focus_app`

The model could report excellent test accuracy while failing an entire product feature.

### Repair

Added five held-out examples for each of the 11 intents (55 tests total) and removed any matching training rows.

---

## 2.2 Train/test leakage

Fifteen normalized test phrases appeared in training, including examples such as:

- `open settings`
- `open the settings`
- `configure nexus`
- `launch whatsapp`
- `close notepad`
- `stop music`
- `search cats`
- `open github.com`

A model can memorize leaked phrases, inflating measured accuracy.

### Repair

`repair_nlu_data.py` now removes every training row whose normalized text occurs in the test set. The current exact overlap is zero.

---

## 2.3 Twenty-nine contradictory intent labels

Identical normalized text was assigned to multiple intents. Examples:

| Text | Conflicting labels | Canonical decision |
|---|---|---|
| `go to github.com` | `open_url`, `browser_navigate` | `browser_navigate` |
| `stop` | `media_stop`, `cancel_action` | `cancel_action` |
| `show whatsapp` | `open_app`, `whatsapp_open` | `whatsapp_open` |
| `who are you` | `greeting`, `unknown` | `unknown` |
| `send message to mom` | `whatsapp_chat`, `whatsapp_search` | `whatsapp_chat` |
| `launch settings` | `open_app`, `open_settings` | `open_settings` |
| `stop playing` | `media_play_pause`, `media_stop` | `media_stop` |
| `pause it` | `cancel_action`, `media_play_pause` | `media_play_pause` |
| `focus whatsapp` | `whatsapp_open`, `focus_app` | `focus_app` |
| `yeah` | `greeting`, `confirm_send` | `confirm_send` |

A classifier cannot learn deterministic labels when identical input has contradictory targets.

### Repair

Added explicit canonical routing policies in `repair_nlu_data.py`. Current same-text conflict count is zero.

### Remaining architectural issue

Some phrases are genuinely contextual. `yeah` may confirm an action only when a confirmation is pending. BERT-Mini receives only the current text, not conversation state. Contextual interpretation must remain in the orchestrator/state machine rather than being solved by more static training data.

---

## 2.4 Invalid `action` slots

Six `open_architect` examples used an undefined slot:

```json
{
  "text": "open architect",
  "intent": "open_architect",
  "slots": {"action": "open_architect"}
}
```

`action` is not part of the 45 BIO label list and could never be trained.

### Repair

Removed the invalid slot annotation while retaining the valid intent example.

---

## 2.5 Corrupt voice/STT samples were merged automatically

The previous collector auto-saved transcripts without review. Examples found in training included:

| Transcript | Intended target | Problem |
|---|---|---|
| `Dit van Vox.` | `type the quick brown fox` | Wrong language/garbage transcription |
| `Hello, bird.` | `type hello world` | Missing command verb and wrong payload |
| `Great hallo world.` | `type hello world` | Missing `type` verb |
| `Спасибо, Леван.` | `press f11` | Completely incorrect transcript |
| `Yes, sir.` | `press up` | Completely incorrect intent surface |
| `This max space.` | `press backspace` | Unalignable key slot |
| `Пресс-хаб.` | `press up` | Wrong-language transcription |
| `Let's meet at IBM.` | `type let's meet at 3pm` | Missing `type` command and incorrect entity |

These rows teach dangerous associations such as “Yes, sir” → `press_key`.

### Repair

Removed 16 clearly corrupt collected records. Changed the collector so every transcription now provides:

```text
Enter = save
r     = retry
 e    = edit transcript
s     = skip
q     = end collection
```

Slots are now derived from the approved/corrected transcript rather than the displayed prompt.

---

## 2.6 List-valued hotkey slots were never annotated

Hotkeys were stored as:

```json
{"keys": ["ctrl", "shift", "t"]}
```

The old annotation function converted the list to a Python string resembling:

```text
['ctrl', 'shift', 't']
```

That text does not occur in `press ctrl shift t`, so no hotkey slot tokens were labeled.

### Repair

List slot values are now handled item-by-item. Each observed key receives a `B-keys`/`I-keys` span.

---

## 2.7 `O` tokens were excluded from slot loss

The previous slot loss used:

```python
CrossEntropyLoss(ignore_index=SLOT_TO_ID["O"])
```

This ignored every non-slot token. The model learned where slots might exist but was never penalized for labeling ordinary words as slots. Results included nonsensical output such as:

```json
{
  "app_name": "pr",
  "text": "the",
  "pr_number": "in",
  "repo": "owner",
  "body": "/ rep"
}
```

### Repair

- `O` is now a normal supervised class.
- Only padding and special tokens use `-100`.
- Slot loss uses `ignore_index=-100`.

### Measured effect

Empty-slot examples without false predictions improved from **4.76% to 95.18%**.

---

## 2.8 Whitespace slot alignment broke punctuation and WordPiece tokens

The old training pipeline split text on whitespace. BERT tokenizes values differently:

```text
owner/repo      → owner / rep ##o
zync-meet/zync  → zyn ##c - meet / zyn ##c
ctrl            → ct ##rl
```

The gold value `owner/repo` could not match a whitespace token, so repository slots were not supervised correctly.

### Repair

Training now requests tokenizer character offsets and labels every token overlapping the actual character span. Verified examples:

```text
owner / rep ##o → B-repo I-repo I-repo I-repo
5                → B-pr_number
ct ##rl          → B-keys I-keys
shift            → B-keys
```

Runtime token joining was also fixed so punctuation is reconstructed as:

```text
owner/repo
zync-meet/zync
```

instead of:

```text
owner / repo
zync - meet / zync
```

### Measured effect

Normalized exact slot match improved from **13.17% to 91.80%**.

---

## 2.9 Training exported ONNX twice and failed after successful training

`train.py` attempted an internal ONNX export, while `train_all.py` already ran the dedicated `export_onnx.py`. With the installed PyTorch version, the internal exporter emitted Unicode output that failed under Windows CP1252:

```text
UnicodeEncodeError: 'charmap' codec can't encode character
```

The 50-epoch training completed and saved the checkpoint, but the command still returned failure.

### Repair

- Removed duplicate ONNX export from `train.py`.
- Kept export ownership in `export_onnx.py`.
- `train_all.py` now performs training, stable export, synchronization, audit, and cleanup in separate steps.

---

## 2.10 Final test evaluated the last epoch instead of the best checkpoint

Training saved the best validation checkpoint but evaluated whichever model state existed after epoch 50.

### Repair

`train.py` now reloads `best_model.pt` before final test evaluation.

---

## 3. Current model results

### Structural dataset results

| Check | Result |
|---|---:|
| Training examples | 3,399 |
| Test examples | 452 |
| Defined intents | 52 |
| Train intents represented | 52/52 |
| Test intents represented | 52/52 |
| Unknown labels | 0 |
| Malformed labels | 0 |
| Exact duplicate rows | 0 |
| Same-text label conflicts | 0 |
| Train/test exact overlap | 0 |
| Invalid slot names | 0 |
| Unalignable slot values | 0 |

### Model results

| Metric | Result |
|---|---:|
| Test intent accuracy | 434/452 = **95.80%** |
| Non-empty slot exact match | **91.80%** |
| Empty examples without false slots | **95.18%** |
| Targeted live/safety/OOS accuracy | 17/22 = **77.27%** |

### Low-performing intent tests

| Intent | Correct | Accuracy | Main gap |
|---|---:|---:|---|
| `browser_navigate` | 1/5 | 20% | Spoken “dot”, new navigation syntax |
| `press_hotkey` | 1/5 | 20% | `control`/`escape`/natural combination syntax |
| `browser_new_tab` | 3/5 | 60% | Novel window/tab paraphrases |
| `cancel_action` | 4/5 | 80% | Overlap with workflow cancellation |
| `confirm_send` | 4/5 | 80% | Contextual confirmation language |
| `focus_app` | 4/5 | 80% | Novel focus verbs |
| `type_text` | 4/5 | 80% | “write email” and unsupported action ambiguity |
| `close_app` | 6/7 | 85.7% | WhatsApp-specialized routing overlap |
| `media_previous` | 6/7 | 85.7% | Short-form ambiguity |
| `unknown` | 7/8 | 87.5% | Too few hard developer-domain negatives |

These five-example live sets are small and therefore noisy, but they correctly expose missing phrase families.

### Targeted failures

| Input | Expected | Predicted | Confidence | Meaning |
|---|---|---|---:|---|
| `press control shift escape` | `press_hotkey` | `press_key` | 75.63% | Needs full-word key synonyms |
| `write an email to john` | `unknown` | `type_text` | 87.07% | `write` prefix over-generalizes |
| `delete all my files` | `unknown` | `list_pr_files` | 94.44% | Developer-domain hard negative missing |
| `open my bank account` | `unknown` | unrelated intent | 12.02% | Low confidence; should be rejected |
| `send my password to john` | `unknown` | `whatsapp_search` | 41.43% | Low confidence and sensitive phrase |

### Confidence gate added

NLU results below **0.85** are now rejected by the Rust client and fall through to safer handling. This blocks the low-confidence bank/password cases, although it cannot fix high-confidence semantic errors such as `delete all my files`.

The deterministic parser remains first in the runtime cascade, so canonical commands do not depend on the NLU threshold.

---

## 4. Why the model is still not “perfect”

## 4.1 Static text cannot resolve runtime context

Examples:

- `yes`, `yeah`, `go ahead` should mean `confirm_send` only when an action is pending;
- `stop` may cancel a live action or stop media depending on state;
- `switch to whatsapp` means focus an existing window, while `open whatsapp` means launch it;
- `send` requires a pending typed message.

These should be resolved by the state machine, not by adding thousands of contradictory static examples.

## 4.2 Unknown coverage is too small

Training currently contains only 44 `unknown` examples. The application can encounter unlimited unsupported requests. The model therefore overgeneralizes familiar words such as:

- `delete` + `files` → `list_pr_files`;
- `write` → `type_text`;
- `send` + contact → WhatsApp intent.

## 4.3 Synthetic data dominates live intents

Live-mode classes have 64–192 training rows each, but most are generated templates. Synthetic quantity gives good canonical behavior without guaranteeing natural speech generalization.

## 4.4 Test examples are still limited

The test set now covers every label but is not large enough to represent:

- accents;
- real Groq/Moonshine mistakes;
- background noise;
- different microphones;
- multi-turn context;
- unseen repository names;
- long text payloads;
- malformed URLs;
- code and file paths;
- all destructive or sensitive requests.

## 4.5 Intent taxonomy contains semantic overlap

`open_url` and `browser_navigate` need a clearer product distinction. Likewise:

- `open_app` vs `whatsapp_open`;
- `whatsapp_chat` vs `whatsapp_search`;
- `media_stop` vs `cancel_action`;
- `search` vs `browser_search`.

If runtime behavior is the same, merging labels may improve reliability more than collecting more data.

---

## 5. External dataset mapping based on research

Full source research: `docs/research/external-nlu-data-sources-and-acquisition-plan-2026-09-13.md`.

### Recommended sources for the detected gaps

| Current gap | Source | Use |
|---|---|---|
| Weak `unknown` and near-OOD behavior | CLINC150 | Balanced OOS train subset and frozen OOS test suite |
| General assistant paraphrases | MASSIVE English | Explicitly mapped text subset |
| Natural assistant entity language | SLURP text | Text only; audio is non-commercial |
| Developer-domain negatives | NL2Bash MIT data | Curated unsupported shell/file requests as hard negatives |
| Spoken hotkeys, URLs, app focus | `nexus collect` | Exact prompts through deployed Groq/Moonshine STT |
| Repository/PR/GitHub operations | First-party + validated synthetic | Public assistant datasets do not cover these labels |
| Accent/device variation | Prolific pilot | 30 speakers × 30 commands after local baseline |

### Data that should not be imported into production training

- Fluent Speech Commands: non-commercial/no-derivatives restrictions;
- Snips spoken datasets: research/non-commercial restrictions;
- SpokenWOZ: CC BY-NC;
- SLURP audio: CC BY-NC without separate license;
- broad GitHub or Stack Overflow scraping;
- random Kaggle mirrors with unclear provenance;
- podcasts, YouTube, Discord, Slack, or private conversations.

### Essential rule

NEXUS BERT-Mini consumes text. Generic audio datasets only help when they are used to evaluate or improve STT, or when their transcripts legitimately map to a NEXUS intent. Random speech audio must not be mislabeled as commands.

---

## 6. Prioritized next training plan

## Phase 1 — harden current English model

### Priority A: unknown/OOS safety

Add 300–500 reviewed hard negatives covering:

- destructive filesystem requests;
- banking/password/crypto requests;
- emails and unsupported messaging;
- system administration beyond supported commands;
- general questions;
- developer requests that mention PR/file/branch words but are not GitHub operations;
- near-miss commands with missing required information.

Use CLINC150 OOS for general diversity and NL2Bash for developer-domain hard negatives.

### Priority B: press-hotkey speech

Collect or generate:

- `ctrl` ↔ `control`;
- `esc` ↔ `escape`;
- `del` ↔ `delete`;
- `win` ↔ `windows key`;
- `cmd` ↔ `command`;
- “hold X and press Y”;
- “use X plus Y”;
- “X Y together”;
- two-, three-, and four-key combinations.

### Priority C: spoken URLs

Cover:

- `github.com`;
- `github dot com`;
- `github dot com slash owner slash repo`;
- `https colon slash slash`;
- subdomains;
- hyphenated domains;
- “in this tab”, “here”, “in the browser”.

### Priority D: real voice data

Run `nexus collect` with transcript review enabled. Collect at least:

- 10 samples × 23 collector intents from the admin;
- 5–10 additional speakers if available;
- Groq and Moonshine transcripts kept as separate source groups;
- different microphones and noise conditions.

Do not auto-save incorrect transcripts. Retry, edit, or skip them.

## Phase 2 — improve evaluation

Create frozen suites:

| Suite | Target size |
|---|---:|
| Every-intent canonical | 520 (10 × 52) |
| Natural paraphrases | 1,040 (20 × 52) |
| Real ASR transcripts | 460+ for 23 voice intents |
| Near-OOD | 500 |
| Far-OOD | 500 |
| Destructive/sensitive safety | 300 |
| Confusion pairs | 25 per priority pair |
| Slot exact-match | 20 per slotted intent |

Split by template family and speaker, not random sentence, to prevent leakage.

## Phase 3 — import public data carefully

1. MASSIVE English mapped subset: 1,000–3,000 records.
2. CLINC150: frozen OOS test plus balanced training subset.
3. SLURP text: mapped intent/slot records only.
4. NL2Bash: curated hard negatives and future intent discovery.
5. Preserve license, version, source ID, and attribution manifests.

Do not merge millions of records. Keep only examples that add a new phrase, slot pattern, ASR error, or hard negative.

## Phase 4 — model promotion rules

Promote a new ONNX model only if:

- macro intent F1 improves;
- all 52 intents have adequate evaluation coverage;
- slot exact match remains above 90% and improves toward 95%;
- unknown/OOS recall reaches the selected threshold;
- no priority intent regresses by more than two percentage points;
- destructive/sensitive false-execution rate is zero;
- real ASR suite improves;
- latency and RAM remain inside budget;
- all training sources have approved provenance.

---

## 7. Tooling implemented

## `nexus audit`

Runs the complete audit:

```bash
nexus audit
```

Dataset-only mode:

```bash
nexus audit --dataset-only
```

Outputs:

- `server/nlu/audit_report.json` — machine-readable details;
- `docs/research/nlu-model-data-audit-latest.md` — generated summary.

Checks:

- labels;
- slots;
- duplicates;
- contradictory intent labels;
- train/test leakage;
- intent coverage;
- class distribution;
- ONNX intent accuracy;
- exact slot match;
- false slots;
- targeted command/OOS cases.

## `repair_nlu_data.py`

Dry run:

```bash
python server/nlu/repair_nlu_data.py
```

Apply repairs:

```bash
python server/nlu/repair_nlu_data.py --apply
```

It is also integrated into `nexus train`, after generated/collected examples are merged.

## Updated `nexus train`

The pipeline now performs:

1. Basic cleaning;
2. synthetic generation;
3. merge generated and collected data;
4. canonical conflict/slot/leakage repair;
5. training;
6. stable ONNX export;
7. resource synchronization;
8. post-training audit;
9. temporary-file cleanup unless `--keep-temp` is supplied.

---

## 8. Verification performed

| Verification | Result |
|---|---|
| Python syntax compilation | Passed |
| Dataset structural audit | Passed: zero listed structural defects |
| ONNX loading | Passed |
| ONNX inference over 452 tests | Passed |
| `nexus audit` CLI | Passed |
| BERT-Mini 50-epoch retraining | Passed |
| Stable ONNX export | Passed, 16.8 MB |
| Resource synchronization | Passed |
| Rust `cargo check` | Passed |
| Runtime slot punctuation reconstruction | Implemented in source and bundled NLU server |

---

## Final assessment

BERT-Mini remains appropriate for the low-RAM reflex/routing layer. The repaired model now has:

- clean and non-contradictory structural data;
- complete intent test coverage;
- no exact split leakage;
- much stronger slot supervision;
- 91.80% normalized slot exact match;
- 95.80% intent accuracy on the curated test set;
- a reusable audit command and repair pipeline.

It is not ready to be called perfect because targeted OOS accuracy is only 77.27%, real multi-speaker ASR coverage is limited, and several live intents have weak novel-phrasing performance. The next gains should come from **reviewed real speech, CLINC150 OOS, MASSIVE mappings, developer hard negatives, and contextual state handling**—not unlimited repetitive synthetic examples.
