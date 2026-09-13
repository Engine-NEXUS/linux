# External NLU Data Sources and Acquisition Plan

**Date:** 2026-09-13  
**Status:** Research and planning — no external dataset has been imported yet  
**Scope:** Text intents, slot annotations, real speech, ASR-error data, opt-in user telemetry, synthetic augmentation, crowdsourced collection, and public-web sources for NEXUS BERT-Mini  
**Primary question:** Where can NEXUS obtain useful live or public data, keep only the essential records, and train the model legally and effectively?

---

## Executive decision

NEXUS should **not scrape arbitrary user speech, GitHub content, Stack Overflow, podcasts, videos, or public conversations** as its primary training source. Public availability is not the same as permission to train, and most of that material does not contain labels compatible with NEXUS’s 52-intent schema.

The recommended strategy is a layered, high-signal pipeline:

1. **Opt-in NEXUS command transcripts and corrections** — highest relevance.
2. **NEXUS prompted voice collection** — highest-value speech/ASR-error source.
3. **MASSIVE English text subset** — strongest permissively licensed general voice-assistant NLU source.
4. **CLINC150 out-of-scope examples** — strongest source for improving `unknown` rejection.
5. **SLURP textual annotations only** — useful assistant phrasings and slots; do not use its non-commercial audio in a commercial product without a separate license.
6. **NL2Bash’s separately MIT-licensed data** — useful developer/terminal language, but only after mapping and safety filtering.
7. **Targeted synthetic generation validated by deterministic rules and humans** — fills exact NEXUS coverage gaps.
8. **Small paid pilot through Prolific or Toloka** if more speaker/accent diversity is needed.

### Highest-level finding

NEXUS’s deployed BERT-Mini is a **text model**, not an acoustic model. Therefore:

- Generic audio corpora do **not** directly improve BERT-Mini.
- The most valuable record is usually:

```json
{
  "expected_phrase": "press control shift escape",
  "asr_transcript": "press control shift escaped",
  "intent": "press_hotkey",
  "slots": {"keys": ["ctrl", "shift", "esc"]},
  "execution_result": "success",
  "consent": true
}
```

- Audio should be retained only when needed for STT evaluation or re-transcription and when the contributor explicitly consented. For routine BERT-Mini training, retain the sanitized transcript, label, slots, and quality metadata; delete raw audio after verification.

---

## 1. NEXUS requirements used for this ranking

### 1.1 Current model

NEXUS trains `google/bert_uncased_L-2_H-128_A-2`, a compact two-layer BERT model with approximately 4.4 million parameters. It jointly predicts:

- one intent label;
- BIO slot tags for each token.

Current training assumptions:

| Property | NEXUS value |
|---|---:|
| Input modality | Text transcript |
| Maximum sequence length | 64 tokens |
| Intent labels defined in code | 52 actual labels |
| BIO labels | 45 including `O` |
| Epochs | 50 |
| Batch size | 16 |
| Runtime format | ONNX |
| Runtime sequence length | 64 |
| Primary runtime role | Fallback after deterministic parser |

The source comments still mention 58 intents in places, but the enumerated list contains 52. External import tooling must use the actual list rather than stale comments.

### 1.2 NEXUS intent groups

| Group | Intents | External-data availability |
|---|---:|---|
| Local app/media/search | 12 | Medium to high |
| Repository/PR analysis | 4 | Very low |
| GitHub PR operations | 10 | Very low |
| Collaborator/org operations | 6 | Very low |
| Branch/release/workflow operations | 8 | Very low |
| Live desktop mode | 11 | Medium |
| Unknown/out-of-scope | 1 | High |

The repository and GitHub-operation intents are specialized. No mainstream assistant dataset directly labels phrases such as `analyse_latest_pr`, `merge_pr`, `list_workflow_runs`, or `remove_collaborator`. Those intents need NEXUS-owned, synthetic, or explicitly commissioned data.

### 1.3 Essential fields for an imported record

The training model only requires:

```json
{
  "text": "list pull requests in nexus",
  "intent": "list_prs",
  "slots": {"repo": "nexus"}
}
```

A provenance-aware ingestion layer should temporarily preserve more fields:

```json
{
  "text": "list pull requests in nexus",
  "intent": "list_prs",
  "slots": {"repo": "nexus"},
  "source": "nexus_opt_in",
  "source_record_id": "sha256:...",
  "license": "user-consent-v1",
  "consent_version": "2026-09-13",
  "language": "en-IN",
  "asr_engine": "groq-whisper-large-v3-turbo",
  "speaker_hash": "rotating-nonidentifying-hash",
  "execution_result": "success",
  "review_status": "approved",
  "collected_at": "2026-09-13T00:00:00Z"
}
```

Before writing to the compact training dataset, keep only:

- sanitized `text`;
- approved `intent`;
- normalized `slots`;
- a non-identifying source category if needed for evaluation splits.

### 1.4 Evaluation criteria

Each source is scored from 1–5:

| Criterion | Weight | Meaning |
|---|---:|---|
| NEXUS semantic fit | 30% | Overlap with NEXUS intents and spoken command style |
| Licensing/commercial clarity | 20% | Explicit permission, provenance, attribution burden |
| Realism | 15% | Natural human phrasing or actual ASR errors |
| Label quality | 15% | Reliable intent and slot labels |
| Coverage/diversity | 10% | Speakers, accents, phrasing, languages |
| Integration efficiency | 10% | Download size, mapping effort, storage, tooling |

A high raw score does not override a non-commercial restriction. Sources with incompatible licensing are ranked as research/evaluation-only.

---

## 2. Ranked source list

### 2.1 Overall ranking for NEXUS

| Rank | Source | Type | Fit /5 | Legal clarity /5 | Recommended use | Decision |
|---:|---|---|---:|---:|---|---|
| 1 | Opt-in NEXUS transcript + outcome feedback | Live first-party text | 5.0 | 4.5 with proper consent | Exact production errors and successful commands | **Use first** |
| 2 | `nexus collect` prompted real speech | First-party voice→ASR text | 5.0 | 5.0 for owner; consent needed for others | Exact intents, accents, microphones, ASR distortions | **Use first** |
| 3 | MASSIVE English | Public labeled NLU text | 4.2 | 5.0, CC BY 4.0 | Assistant phrasing, media, app-like, messaging, slots | **Import mapped subset** |
| 4 | CLINC150 | Public intent/OOS text | 3.9 | 5.0, CC BY 4.0 | `unknown`, OOS calibration, greetings, general assistant language | **Import OOS + mapped subset** |
| 5 | SLURP text | Public labeled NLU text | 4.0 | 5.0 for text, CC BY 4.0 | Natural assistant language and entity spans | **Text only** |
| 6 | NEXUS synthetic + rule validation | Synthetic labeled text | 4.5 | 4.5, provider terms must be recorded | Specialized GitHub and live-mode gaps | **Use with QA** |
| 7 | NL2Bash `data/bash` | Developer command text | 3.6 | 4.5, separately MIT-licensed data | Terminal/developer imperative language, OOS hard negatives | **Curate small subset** |
| 8 | Prolific custom NEXUS study | Paid opt-in human collection | 4.8 | 4.5 with study consent | Diverse speakers, developers, accents, usability | **Best paid pilot** |
| 9 | Toloka custom audio collection | Paid crowd collection | 4.6 | Contract-dependent | Larger multilingual/accent collection and QA | **Scale after pilot** |
| 10 | OpenVoiceOS intents-for-eval | Public assistant text | 3.6 | Must verify exact dataset license | Near-OOD/far-OOD evaluation patterns | **Evaluate after license review** |
| 11 | Google Speech Commands v2 | Public single-word audio | 2.7 | 5.0, CC BY 4.0 | `yes`, `no`, `stop`, direction/key word robustness | **Evaluation/ASR augmentation only** |
| 12 | Mozilla Common Voice | Public general speech audio | 2.5 | 4.5, CC0 plus MDC terms | Accent/noise/STT evaluation | **Do not map blindly to intents** |
| 13 | MultiATIS++ | Public intent/slot text | 2.4 | 5.0, Apache 2.0 | Slot-filling methodology and OOS | **Low-priority benchmark** |
| 14 | Schema-Guided Dialogue | Public dialogue text | 2.5 | 3.5, CC BY-SA 4.0 | Paraphrase structures and dialogue-state research | **Research, not direct merge** |
| 15 | MultiWOZ 2.2 | Public dialogue text | 2.2 | 4.0; verify release-specific terms | Multi-turn language and slot methodology | **Research only** |
| 16 | LibriSpeech | Public read speech | 1.7 | 5.0, CC BY 4.0 | Generic STT/acoustic evaluation | **Not useful for BERT intent training** |
| 17 | Appen custom speech | Enterprise paid collection | 4.7 | Contract-dependent | Global custom speech at scale | **Later-stage enterprise option** |
| 18 | Defined.ai custom/licensed speech | Enterprise data license | 4.5 | Contract-dependent | Commercially licensed custom speech | **Later-stage enterprise option** |
| 19 | DataForce/TransPerfect | Enterprise custom collection | 4.6 | Contract-dependent | Multi-device, multilingual, anonymized speech | **Later-stage enterprise option** |
| 20 | GitHub/Stack Overflow scraping | Public web text | 2.8 | 1.5–2.5 | Developer vocabulary | **Do not scrape for training** |

### 2.2 Best source by need

| Need | Best source |
|---|---|
| Exact NEXUS production mistakes | Opt-in NEXUS telemetry |
| Actual Indian-English/accented ASR errors | `nexus collect` + more consenting speakers |
| General assistant intent phrasing | MASSIVE English + SLURP text |
| Unknown/OOS rejection | CLINC150 OOS |
| GitHub/PR commands | NEXUS synthetic + developer review + opt-in usage |
| Desktop control commands | NEXUS collection + synthetic generator |
| Media commands | MASSIVE, SLURP text, Google Speech Commands for short-word audio testing |
| Developer imperative wording | NL2Bash curated descriptions |
| Multilingual future | MASSIVE’s 52 languages, then custom voice collection |
| Diverse paid speakers | Prolific pilot, then Toloka/DataForce/Appen |
| Generic STT robustness | Common Voice, Google Speech Commands, VoxPopuli non-native English |

---

## 3. Detailed analysis of public NLU datasets

## 3.1 MASSIVE — Rank #1 public dataset

**Provider:** Amazon Science  
**Size:** More than 1 million utterances  
**Languages:** 52  
**Labels:** 60 intents, 55 slot types  
**License:** CC BY 4.0  
**Official source:** https://www.amazon.science/code-and-datasets/massive  
**Dataset:** https://huggingface.co/datasets/AmazonScience/massive

### Why it fits

MASSIVE is the strongest public text source because it is specifically built for virtual-assistant NLU and contains both intent labels and slot annotations. Its domains include media, messaging, lists, alarms, weather, calendar, transport, and general assistant tasks.

### Likely NEXUS mappings

| MASSIVE family | NEXUS target | Import policy |
|---|---|---|
| audio/music play/pause | `media_play_pause` | Direct after manual mapping |
| next/previous track | `media_next`, `media_previous` | Direct after review |
| general search/query | `search` or `browser_search` | Context-sensitive; do not collapse blindly |
| messaging/contact | `whatsapp_search` / `whatsapp_chat` | Replace generic medium with WhatsApp only in synthetic transforms, not original truth |
| app control/device control | `open_app`, `close_app`, `focus_app` | Use paraphrase patterns, substitute approved app gazetteer |
| unsupported domains | `unknown` | Use selected records as near-OOD, not every record |

### What to retain

- English (`en-US`) utterance text;
- intent name;
- slot spans and values;
- source ID and CC BY attribution metadata until data-card generation.

### What not to retain

- all 52 languages in phase 1;
- irrelevant domains such as cooking, weather, transport, alarms, and finance as positive NEXUS intents;
- personal slot values without replacement;
- duplicated parallel translations.

### Recommended volume

- 1,000–3,000 mapped English positives across overlapping intents;
- 1,000 carefully sampled near-OOD records for `unknown` evaluation;
- cap each mapped NEXUS intent to avoid overwhelming first-party examples.

### Risks

- Intent taxonomy mismatch;
- assistant domains differ from developer desktop tasks;
- synthetic localization can sound more scripted than real NEXUS users;
- attribution is required.

**Verdict:** Best public starting point, but map through an explicit allowlist and human review.

---

## 3.2 CLINC150 — best `unknown`/OOS source

**Provider:** CLINC / UCI  
**Instances:** 23,700  
**Classes:** 150 in-scope intents plus out-of-scope  
**License:** CC BY 4.0  
**Official source:** https://archive.ics.uci.edu/dataset/570/clinc150

### Why it fits

NEXUS currently needs to avoid executing the wrong command when the input is unrelated or ambiguous. CLINC150 was built specifically to evaluate out-of-scope detection. Its full version has:

- 100 training examples per in-scope class;
- 20 validation examples per class;
- 30 test examples per class;
- 100 OOS train, 100 OOS validation, and 1,000 OOS test examples.

### Recommended use

1. Keep the original CLINC OOS test set as a separate **never-train** benchmark.
2. Use a small OOS training subset for NEXUS `unknown`.
3. Map only clearly overlapping assistant intents.
4. Use non-overlapping but command-like classes as hard negatives.

### Important warning

Do not put thousands of CLINC records into `unknown` without balancing. It would teach the model to over-predict `unknown` and reduce command recall.

Recommended ratio:

- `unknown` training examples should be approximately 1–2× the median positive intent count, not 20×;
- maintain separate far-OOD and near-OOD test sets.

**Verdict:** Essential for safe fallback behavior; primarily an evaluation and calibrated-negative source.

---

## 3.3 SLURP text — good assistant language, audio restricted

**Provider:** Emotech/research contributors  
**Scale:** Approximately 72,000 recordings, 18 domains, 46 actions, 55 entity types  
**Text license:** CC BY 4.0  
**Audio license:** CC BY-NC 4.0 unless separately licensed  
**Official source:** https://github.com/pswietojanski/slurp  
**Audio:** https://zenodo.org/records/4274930

### Why the text helps

SLURP provides:

- sentence text;
- sentence annotations;
- intent/action/scenario hierarchy;
- entity spans;
- links to recordings and WER metadata.

This closely matches NEXUS’s intent + slot task.

### Commercial-use boundary

- **Text distributed through the GitHub repository:** CC BY 4.0; usable with attribution.
- **Audio hosted on Zenodo:** CC BY-NC 4.0; do not use in a commercial or potentially commercial NEXUS model without a separate license from the rights holder.

### Recommended use

- ingest a mapped text-only subset;
- preserve source attribution;
- use WER fields only as research metadata if no audio is downloaded;
- contact the provider if SLURP audio becomes strategically important.

**Verdict:** Excellent text supplement; audio is excluded from the shipping model pipeline under current terms.

---

## 3.4 HWU64

**Size:** Approximately 10,000 assistant utterances across 64 intents  
**Fit:** Personal assistant language is relevant  
**Problem:** License provenance is less clear across mirrors.

The commonly found copies are reformatted mirrors, and search results often describe research usage without a clean, authoritative commercial-data license. A repository code license does not automatically license the dataset content.

**Verdict:** Do not import until the exact original release license is located and reviewed. MASSIVE covers similar territory with much clearer licensing.

---

## 3.5 Google Schema-Guided Dialogue (SGD)

**Scale:** More than 20,000 multi-domain dialogues  
**Domains:** Travel, events, payments, media, restaurants, weather, and more  
**License:** CC BY-SA 4.0  
**Official source:** https://github.com/google-research-datasets/dstc8-schema-guided-dialogue

### Value

- Intent prediction and slot filling;
- dialogue state and service schema design;
- natural paraphrases around API operations.

### Limitations

- Multi-turn dialogues do not directly match NEXUS’s single-turn classifier;
- domains mostly do not overlap;
- ShareAlike obligations require careful product/legal interpretation;
- flattening dialogue turns can create mislabeled examples.

**Verdict:** Use for schema and evaluation research, not direct phase-1 ingestion.

---

## 3.6 MultiWOZ 2.2

**Type:** Multi-domain task-oriented dialogues  
**Domains:** Primarily travel, hotel, restaurant, taxi, train  
**License:** Repository states MIT; some mirrors report Apache 2.0, so exact release provenance must be preserved  
**Official source:** https://github.com/budzianowski/multiwoz

Useful for multi-turn state tracking but has low direct overlap with developer commands.

**Verdict:** Low-priority research source; no direct merge.

---

## 3.7 MultiATIS++

**Type:** Intent + slot text for air travel  
**Languages:** Nine  
**License:** Apache 2.0  
**Official source:** https://github.com/amazon-science/multiatis

Useful for testing slot alignment and multilingual tokenization, but almost no semantic overlap.

**Verdict:** Benchmark only.

---

## 3.8 NL2Bash — useful developer language

**Type:** Natural-language descriptions paired with Bash commands  
**Scale:** Around 10,000–12,000 raw pairs; approximately 9,300 filtered  
**Dataset license:** `data/bash` separately licensed under MIT  
**Code license:** GPLv3  
**Official source:** https://github.com/TellinaTool/nl2bash

### Why it matters

It contains imperative developer language and realistic descriptions of file, process, search, and terminal operations. This is closer to NEXUS’s developer audience than weather or travel datasets.

### How to use it safely

- Import only the separately MIT-licensed `data/bash` content, not code under GPL unless desired.
- Do not treat every sentence as a supported NEXUS command.
- Use most records as developer-domain `unknown` or future intent discovery.
- Map only exact overlaps through deterministic rules.
- Exclude destructive commands from positive execution intents unless NEXUS has a safe confirmed equivalent.
- Strip paths, usernames, hostnames, tokens, and secrets.

**Verdict:** Valuable niche source after strict curation; do not bulk-label it with existing NEXUS intents.

---

## 3.9 CoNaLa

**Type:** Natural-language intent paired with Python snippets  
**Scale:** 2,379 curated train + 500 test, plus approximately 600,000 mined candidates  
**Source:** Stack Overflow-derived  
**Official source:** https://conala-corpus.github.io/

Potentially useful for developer vocabulary, but its provenance inherits Stack Overflow attribution/share-alike considerations, and code-generation descriptions do not correspond to NEXUS command intents.

**Verdict:** Research/intent-discovery only unless licensing and attribution are handled comprehensively. Prefer NL2Bash’s explicit dataset license for phase 1.

---

## 4. Detailed analysis of public speech datasets

## 4.1 Google Speech Commands v2

**Type:** One-second spoken keywords  
**Scale:** More than 100,000 clips in v2  
**License:** CC BY 4.0  
**Official source:** https://www.tensorflow.org/datasets/catalog/speech_commands

### Relevant words

`yes`, `no`, `stop`, `go`, `up`, `down`, `left`, `right`, digits, and other short words.

### NEXUS use

- test whether the STT layer consistently recognizes short commands;
- build a short-command benchmark for `confirm_send`, `cancel_action`, and key directions;
- augment wake/keyword evaluation.

### Not suitable for

- direct BERT-Mini intent training from raw audio;
- repository or GitHub commands;
- multiword slot extraction.

**Verdict:** High legal clarity, narrow semantic value.

---

## 4.2 Mozilla Common Voice

**Type:** Community-contributed scripted speech  
**Languages:** Large multilingual catalog  
**License:** CC0 unless a specific release says otherwise  
**Distribution:** Now exclusively through Mozilla Data Collective (MDC)  
**Terms:** https://commonvoice.mozilla.org/dag/terms

### Value

- speaker and accent diversity;
- real microphones and environments;
- generic ASR evaluation;
- future multilingual STT benchmarking.

### Limitations

- sentences are not labeled with NEXUS intents;
- random text cannot be safely mapped to commands;
- dataset access and redistribution must follow current MDC terms;
- audio is much larger than NEXUS needs.

### Recommended use

Download only a small, validated English subset for STT regression testing. Do not add it to `dataset.json` unless a transcript independently and correctly maps to a NEXUS intent.

**Verdict:** STT/accent benchmark, not NLU training data.

---

## 4.3 LibriSpeech

**Type:** 1,000 hours of read audiobook speech at 16 kHz  
**License:** CC BY 4.0  
**Official source:** https://openslr.org/12

High-quality ASR foundation data, but clean audiobook narration is far from short desktop commands.

**Verdict:** No direct value for NEXUS BERT-Mini; only useful if NEXUS trains its own ASR acoustic model, which is not currently planned.

---

## 4.4 VoxPopuli

**Type:** European Parliament speech, multilingual, including non-native English subset  
**Data license:** Reported as CC0 with European Parliament legal notice; code/models have different terms  
**Official source:** https://github.com/facebookresearch/voxpopuli

The 29-hour non-native English subset could help accent evaluation, but the content is parliamentary speech, not commands.

**Verdict:** Optional ASR accent benchmark only; carefully distinguish data, code, and pretrained-model licenses.

---

## 4.5 Fluent Speech Commands

**Type:** Audio commands with action/object/location labels  
**License:** CC BY-NC-ND 4.0 / academic research only  
**Official license:** https://fluent.ai/wp-content/uploads/2021/04/Fluent_Speech_Commands_Public_License.pdf

The semantic format is attractive, but the official site states that the dataset cannot be used for commercial training, testing, benchmarking, or product development.

**Verdict:** **Do not use in NEXUS product training.** It can inform research design, but not model artifacts intended for distribution.

---

## 4.6 SLURP audio

**License:** CC BY-NC 4.0  
**Size:** Around 6 GB across real and synthetic audio

**Verdict:** Do not include in a potentially commercial distributed model without a separate commercial license. Use SLURP text instead.

---

## 4.7 SpokenWOZ

**Type:** 249 hours, 203,000 turns, 5,700 human-to-human spoken dialogues  
**License:** CC BY-NC 4.0  
**Official source:** https://spokenwoz.github.io/

Very rich natural speech, but non-commercial and domain-mismatched.

**Verdict:** Research only; exclude from production training.

---

## 4.8 Snips spoken datasets

**Domains:** Smart lights and smart speakers  
**License:** Academic/research only; no commercial use  
**Source:** https://github.com/snipsco/spoken-language-understanding-research-datasets

Music controls overlap NEXUS, but the license is incompatible with unrestricted product distribution.

**Verdict:** Do not use in the production model.

---

## 5. Can NEXUS get live user data?

Yes, but it should be **explicitly opt-in, text-first, minimized, and revocable**.

### 5.1 Recommended opt-in modes

| Mode | What leaves device | Default | Value | Privacy risk |
|---|---|---|---|---|
| No contribution | Nothing | **Default** | None | Lowest |
| Anonymous command improvement | Sanitized transcript + predicted/corrected intent + outcome | User opt-in | Highest per byte | Low-medium |
| Voice improvement | Short audio + transcript + label | Separate explicit opt-in | Accent/STT improvement | High |
| Diagnostic upload | Selected event bundle after preview | Per-incident consent | Debugging | Medium |

Never combine these into one vague “improve NEXUS” toggle. Audio contribution requires a separate, prominent choice.

### 5.2 Essential telemetry record

```json
{
  "schema_version": 1,
  "event_id": "random-uuid",
  "text": "merge pull request number <NUMBER> in <REPO>",
  "predicted_intent": "merge_pr",
  "final_intent": "merge_pr",
  "slots": {"repo": "<REPO>", "pr_number": "<NUMBER>"},
  "confidence": 0.82,
  "parser_source": "bert_mini",
  "execution_result": "success",
  "correction": null,
  "locale": "en-IN",
  "app_version": "...",
  "consent_version": "..."
}
```

### 5.3 Do not collect

- raw GitHub OAuth tokens;
- API keys or secrets;
- clipboard content;
- typed passwords;
- full repository paths;
- full WhatsApp message bodies;
- contact names or phone numbers;
- private repository names unless replaced locally;
- audio captured before explicit recording starts;
- surrounding room conversation;
- stable hardware identifiers;
- IP address in the training record;
- user email or GitHub login in the model dataset.

### 5.4 Local sanitization before upload

Replace sensitive values on-device:

| Data | Replacement |
|---|---|
| Repository name | `<REPO>` |
| GitHub owner/user | `<USER>` |
| PR/workflow number | `<NUMBER>` while retaining numeric slot type |
| File path | `<PATH>` |
| URL with query | normalized origin or `<URL>` |
| Contact/phone | `<CONTACT>` |
| Email | `<EMAIL>` |
| API key/token | `<SECRET>` and reject record |
| Message body | `<TEXT>` or do not upload |

Use deterministic patterns first, then an optional PII detector. Microsoft Presidio is an MIT-licensed, customizable framework for detecting and anonymizing PII in text: https://github.com/microsoft/presidio

### 5.5 Consent and deletion requirements

Operational privacy requirements:

1. Explain exactly what is collected and why before enabling collection.
2. Keep contribution off by default.
3. Separate text contribution from audio contribution.
4. Provide a local preview of each queued upload.
5. Provide “delete pending” and “request deletion” controls.
6. Store consent version and timestamp.
7. Minimize retention: raw audio should expire quickly after verification.
8. Exclude minors unless a compliant dedicated process exists.
9. Avoid voice identification, speaker embeddings, or emotion profiling unless separately justified and consented.
10. Maintain a data-source manifest and deletion ledger.

Regulatory guidance consistently emphasizes lawful basis, specific/informed consent where applicable, minimization, audit trails, and deletion of intermediate files. Relevant official guidance:

- ICO AI lawfulness: https://ico.org.uk/for-organisations/uk-gdpr-guidance-and-resources/artificial-intelligence/guidance-on-ai-and-data-protection/how-do-we-ensure-lawfulness-in-ai/
- ICO AI data minimization: https://ico.org.uk/for-organisations/advice-and-services/audits/data-protection-audit-framework/toolkits/artificial-intelligence/data-minimisation/
- ICO biometric/voice guidance: https://ico.org.uk/for-organisations/uk-gdpr-guidance-and-resources/lawful-basis/biometric-data-guidance-biometric-recognition/
- FTC Alexa/Ring training-data lessons: https://www.ftc.gov/business-guidance/blog/2023/06/hey-alexa-what-are-you-doing-my-data
- California CCPA statute: https://leginfo.legislature.ca.gov/faces/codes_displayText.xhtml?division=3.&lawCode=CIV&part=4&title=1.81.5.

This document is an engineering plan, not legal advice. Final public collection should receive jurisdiction-appropriate legal review.

---

## 6. Scraping analysis

## 6.1 GitHub

GitHub’s current Terms contain specific API and AI-training provisions. Automated access for commercial AI training may require compliance beyond ordinary public-repository access. Repository content also has per-repository licenses; issue and PR prose may include personal information and does not inherit a repository code license automatically in a simple way.

Official terms: https://docs.github.com/en/site-policy/github-terms/github-terms-of-service

### Recommendation

Do not build a broad GitHub scraper for NEXUS training.

Allowed lower-risk alternatives:

- use repositories owned by the NEXUS team;
- obtain explicit contributor permission;
- use a specific dataset with a clear license;
- use GitHub only for vocabulary discovery, then write original synthetic phrases;
- use the API only within its terms and rate limits;
- retain source/license provenance for every imported record.

### Why GitHub public PR text is low value

PR titles/comments describe code changes; they rarely contain voice commands such as “list pull requests in repo X.” Mapping them to NEXUS intents would create weak or incorrect labels.

## 6.2 Stack Overflow

Public contributions use version-dependent CC BY-SA licenses, and current data-dump access contains explicit use conditions. The help page says users must affirm they do not intend to use the dump for LLM training. Even if BERT-Mini is not an LLM, relying on ambiguous platform terms is unnecessary when NL2Bash and CoNaLa already offer packaged research corpora.

Official licensing: https://stackoverflow.com/help/licensing  
Data dump access: https://stackoverflow.com/help/data-dumps

### Recommendation

Do not scrape Stack Overflow or ingest the current data dump for NEXUS model training. Use clearly licensed derived datasets only after reviewing their exact dataset licenses and attribution requirements.

## 6.3 Reddit, forums, Discord, Slack, YouTube, podcasts

These sources have poor fit and high privacy/copyright/terms risk:

- no NEXUS intent labels;
- private or semi-private context;
- usernames and personal information;
- consent uncertainty;
- large annotation burden;
- audio speaker/personality rights.

**Decision:** Exclude from the plan.

## 6.4 Safe scraping rule

Only ingest if all conditions are true:

1. Exact source has an explicit license permitting intended training/use.
2. Platform terms allow automated collection and the chosen use.
3. Provenance and attribution can be preserved.
4. Records can be stripped of personal/sensitive information.
5. A deterministic or human process can assign correct NEXUS labels.
6. The source materially improves an identified evaluation gap.

If any condition fails, do not scrape.

---

## 7. Paid/custom sources

## 7.1 Prolific — best small pilot

**Provider:** Prolific  
**Pool:** Hundreds of thousands of verified participants; filters for demographics and expertise  
**Voice/multimodal:** Supports voice and custom hosted tasks  
**Official pages:**
- https://www.prolific.com/multimodal-data-for-ai
- https://www.prolific.com/voice-and-conversational-ai-evaluation

### Why it fits NEXUS

- Recruit actual developers or GitHub users;
- run the existing NEXUS collector as a hosted web task;
- target countries, accents, OS familiarity, and developer expertise;
- start with 20–50 people rather than an enterprise contract;
- auditable participant consent and payment.

### Pilot design

- 30 speakers;
- 30 commands each;
- 900 audio/transcript/intent records;
- balanced Windows/macOS/Linux familiarity;
- English accent mix;
- 20 scripted commands + 10 unscripted “say this naturally” tasks;
- independent QA on 20% of samples.

**Verdict:** Best paid source for NEXUS’s current scale.

## 7.2 Toloka

Supports audio collection, transcription, classification, native-speaker QA, and more than 90 languages. It also publishes sample audio-collection workflows.

Official sources:
- https://toloka.ai/platform
- https://github.com/Toloka/toloka-kit/tree/main/examples

**Verdict:** Good for scaling beyond the first Prolific pilot and for multilingual collection.

## 7.3 Appen

Provides custom scripted/conversational speech collection and off-the-shelf licensed datasets across many languages/locales.

Official source: https://www.appen.com/speech-and-audio-training-data

**Verdict:** Strong enterprise option; likely excessive for 5–10 users today.

## 7.4 Defined.ai

Provides commercially licensed marketplace datasets and custom collection. Its standard agreement says trained models can be commercialized, but dataset redistribution is restricted and each order’s permitted use must be checked.

Official sources:
- https://defined.ai/dataset-type/audio
- https://defined.ai/data-license-agreement

**Verdict:** Good if NEXUS later needs commercially cleared speech quickly; preserve contract-specific usage rules.

## 7.5 DataForce / TransPerfect

Offers custom voice collection in more than 200 languages, consumer-device coverage, transcription, categorization, quality review, and anonymization workflows.

Official source: https://www.transperfect.com/technology/artificial-intelligence/ai-data-collection-and-annotation

**Verdict:** Strong enterprise/global option; not cost-effective at current scale.

## 7.6 Scale AI

Offers audio evaluation and high-end custom data infrastructure. Likely optimized for larger budgets and frontier-model programs.

Official source: https://scale.com/blog/not-in-text-alone

**Verdict:** Lowest priority among paid providers for this small BERT-Mini project.

---

## 8. Synthetic data strategy

Synthetic data is necessary because public datasets do not cover NEXUS’s specialized GitHub operations.

### 8.1 Appropriate synthetic targets

- `analyse_repo`;
- `analyse_pr`;
- `analyse_latest_pr`;
- `check_branch`;
- all PR operations;
- collaborator/org operations;
- branch/release/workflow operations;
- live keyboard and browser phrasing gaps.

### 8.2 Generation method

For each intent:

1. Define canonical semantics and required/optional slots.
2. Define safe slot gazetteers.
3. Generate controlled paraphrases across:
   - imperative/direct;
   - polite request;
   - question form;
   - filler words;
   - word-order variants;
   - short/elliptical speech;
   - ASR substitutions;
   - accent-informed variants.
4. Parse every generated phrase using a strict validator.
5. Reject phrases that map to multiple intents.
6. Deduplicate by normalized text and semantic template.
7. Human-review a stratified sample.
8. Reserve entire paraphrase families for test data to prevent leakage.

### 8.3 Provider choice

NEXUS can use its admin-only Qwen model or Groq-hosted models for generation. Groq’s services agreement states that outputs are Customer Data and that Groq is not permitted to train on inputs/outputs without permission; record the exact agreement version used.

Groq terms: https://console.groq.com/docs/legal/services-agreement

### 8.4 Synthetic data limits

- Never exceed 50–60% synthetic data in the final training set for a production-critical intent if first-party data is available.
- Synthetic examples must not dominate evaluation sets.
- Do not create slot values that cannot execute safely.
- Do not use an LLM’s proposed intent label without deterministic validation.
- Do not generate only surface synonyms; vary syntax and ASR-error patterns.

---

## 9. Proposed ingestion and quality pipeline

```text
Source registry
    ↓
License/consent gate
    ↓
Download via official API/archive (never arbitrary scrape)
    ↓
Source-specific parser
    ↓
Language + domain filter
    ↓
PII/secret redaction on local machine
    ↓
Explicit source-intent → NEXUS-intent mapping
    ↓
Slot normalization and span verification
    ↓
Near-duplicate and template-family detection
    ↓
Automatic ambiguity/conflict checks
    ↓
Human review queue
    ↓
Quarantine / approved / rejected
    ↓
Split by speaker/source/template family
    ↓
Train candidate model
    ↓
Compare against frozen evaluation suites
    ↓
Promote only if safety and accuracy gates pass
```

### 9.1 Source registry

Create a manifest entry per source/version:

```json
{
  "source_id": "massive-1.1-en-US",
  "official_url": "https://www.amazon.science/code-and-datasets/massive",
  "version": "1.1",
  "retrieved_at": "2026-09-13",
  "license": "CC-BY-4.0",
  "license_url": "https://huggingface.co/datasets/AmazonScience/massive/blob/main/LICENSE",
  "allowed_use": ["training", "evaluation", "commercial-model"],
  "attribution_required": true,
  "raw_retention": "until-import-verified",
  "reviewed_by": "..."
}
```

### 9.2 Intent mapping file

Never hard-code uncertain mappings inside download code. Use a reviewed mapping:

```yaml
source: massive-1.1-en-US
mappings:
  audio_volume_up: reject
  play_music:
    nexus_intent: media_play_pause
    confidence: high
  pause_music:
    nexus_intent: media_play_pause
    confidence: high
  send_message:
    nexus_intent: reject
    reason: generic messaging is not necessarily WhatsApp
```

### 9.3 Essential filtering

Keep a record only if:

- language is supported;
- target intent mapping is unambiguous;
- all required slots can be found in text;
- no secret or prohibited PII remains;
- normalized text is not already present;
- source license is approved;
- quality score meets threshold;
- it adds lexical, syntactic, accent, or error-pattern diversity.

### 9.4 Deduplication levels

1. Exact normalized text;
2. punctuation/case-normalized text;
3. placeholder-normalized template (`merge pr <NUMBER> in <REPO>`);
4. embedding similarity;
5. speaker/session duplication for audio-derived data.

### 9.5 Label auditing

Use:

- deterministic parser disagreement;
- BERT cross-validation confidence;
- Qwen adjudication (suggestion only);
- Cleanlab for potential label errors and outliers;
- human review for destructive or ambiguous intents.

Cleanlab provides intent-classification and token-label issue detection workflows: https://docs.cleanlab.ai/master/tutorials/text.html

### 9.6 Annotation tools

- **Argilla** — best for text intent/slot review and active learning; Apache 2.0: https://github.com/argilla-io/argilla
- **Label Studio** — best for audio playback, transcription, and intent labels; Apache 2.0: https://github.com/HumanSignal/label-studio

Recommendation: keep the current lightweight JSONL collector for owner collection; use Label Studio only when multiple annotators or paid speakers are introduced.

---

## 10. Evaluation design

More data is not automatically better. NEXUS should maintain frozen evaluation suites.

### 10.1 Required suites

| Suite | Purpose | Minimum size |
|---|---|---:|
| Canonical commands | Basic intent correctness | 10 per intent |
| Natural paraphrases | Generalization | 20 per intent |
| Real ASR transcripts | Production speech errors | 20 per priority intent |
| Near-OOD | Similar but unsupported commands | 500 total |
| Far-OOD | General unrelated speech | 500 total |
| Intent-confusion pairs | Distinguish adjacent intents | 25 per confusion pair |
| Destructive-action safety | Prevent false execution | 300 total |
| Slot exact match | Entity extraction | 20 per slotted intent |
| Accent/device matrix | STT→NLU pipeline | 10 speakers × 50 commands |

### 10.2 Priority confusion matrix

- `open_app` vs `focus_app` vs `whatsapp_open`;
- `search` vs `browser_search`;
- `open_url` vs `browser_navigate`;
- `press_key` vs `press_hotkey`;
- `confirm_send` vs `whatsapp_chat`;
- `cancel_action` vs `media_stop`;
- `analyse_pr` vs `get_pr`;
- `analyse_latest_pr` vs `list_prs`;
- `close_pr` vs `close_app`;
- unsupported/destructive request vs valid GitHub command.

### 10.3 Promotion gates

Do not promote a candidate model unless:

- overall intent accuracy improves or remains within an accepted tolerance;
- macro F1 improves (not only weighted accuracy);
- no priority intent regresses by more than 2 percentage points;
- destructive-action false-positive rate is zero on the safety suite;
- unknown/OOS recall meets threshold;
- slot exact-match and span F1 meet threshold;
- latency and model size remain within budget;
- all sources in the candidate dataset have approved provenance.

---

## 11. Recommended phased plan

## Phase 0 — fix measurement and provenance

**Do before importing external data.**

1. Freeze a baseline test set that generated data never touches.
2. Correct stale “58 intent” comments to the actual 52-label list or add missing labels deliberately.
3. Add dataset source/version/license fields outside the compact training records.
4. Add per-intent precision, recall, macro F1, slot F1, and confusion matrix reporting.
5. Add data-family splitting to avoid paraphrase leakage.
6. Add secret/PII scanning.

**Exit criterion:** A candidate model can be compared reproducibly with the current ONNX model.

## Phase 1 — first-party data first

1. Finish and validate `nexus collect` end-to-end.
2. Collect 10 samples per intent from the admin.
3. Prioritize the 23 intents supported by the collector.
4. Add correction choices instead of blindly auto-saving wrong/empty transcriptions.
5. Store raw audio temporarily, verify transcript, then delete audio.
6. Train and evaluate after every 100–200 accepted records.

**Target:** 500–1,000 real ASR transcript records.

## Phase 2 — public text import

1. Import MASSIVE English through an explicit allowlist.
2. Import SLURP text-only mapped records.
3. Import CLINC OOS benchmark and a balanced OOS train subset.
4. Curate NL2Bash developer descriptions for hard negatives and future intents.
5. Preserve attribution manifests.

**Target:** 2,000–4,000 high-quality approved external records, not the full million-record MASSIVE corpus.

## Phase 3 — targeted synthetic gaps

1. Generate records only for low-coverage NEXUS-specific intents.
2. Use template families, filler words, and ASR confusion maps.
3. Run deterministic validation and cross-model disagreement checks.
4. Human-review all destructive-operation intents.

**Target:** 100–300 diverse examples per specialized intent, with no synthetic records in the frozen real-world test set.

## Phase 4 — opt-in user learning for 5–10 users

1. Add separate opt-in toggles for sanitized text and audio.
2. Sanitize locally before upload.
3. Upload only errors, corrections, low-confidence commands, and a small sample of successes.
4. Provide preview and deletion controls.
5. Deduplicate server-side.
6. Retrain only after review and evaluation.

**Target:** 50–100 high-information records per active user per month, not every command.

## Phase 5 — paid diversity pilot

1. Run a Prolific pilot with 30 participants × 30 commands.
2. Include scripted and natural rephrasing tasks.
3. Recruit developers and accent diversity.
4. Validate 20% with independent annotation.
5. Compare model improvement before scaling.

**Target:** 900 labeled voice records.

## Phase 6 — multilingual expansion

1. Choose one target locale based on actual users.
2. Import the corresponding MASSIVE locale.
3. Collect native-speaker audio through Toloka/Prolific.
4. Replace the English-only BERT checkpoint if necessary with a multilingual compact encoder.
5. Maintain locale-specific evaluation and command gazetteers.

---

## 12. Recommended final blend

For the next English BERT-Mini version:

| Source | Target share | Purpose |
|---|---:|---|
| Existing reviewed NEXUS dataset | 35% | Core intent behavior |
| Opt-in real NEXUS transcripts | 20% | Production distribution |
| NEXUS prompted voice→ASR transcripts | 15% | Accent/mic/ASR realism |
| MASSIVE mapped subset | 10% | Broad assistant paraphrases |
| SLURP text mapped subset | 5% | Natural language + slots |
| CLINC OOS/near-OOD | 5% | Unknown rejection |
| NL2Bash curated developer language | 3% | Developer-domain hard negatives |
| Validated synthetic examples | 7% | Specialized intent gaps |

These percentages are starting targets, not permanent rules. Tune them using macro F1 and safety-suite results.

---

## 13. Sources rejected for immediate production use

| Source | Reason |
|---|---|
| Fluent Speech Commands | Academic/non-commercial/no-derivatives restrictions |
| Snips spoken datasets | Research-only/non-commercial |
| SpokenWOZ | CC BY-NC 4.0 |
| SLURP audio | CC BY-NC 4.0 without separate license |
| Random Kaggle uploads | Mirrors may have unclear provenance or incorrect license metadata |
| GitHub broad scrape | Current platform AI-training terms + per-content licensing + weak labels |
| Stack Overflow scrape/data dump | Attribution/share-alike and current use conditions; weak direct fit |
| Reddit/Discord/Slack | Privacy, consent, terms, and labeling problems |
| YouTube/podcasts | Copyright, voice/personality rights, no intent labels |
| LibriSpeech as NLU data | Generic read speech cannot train text intent semantics directly |

---

## 14. Immediate next actions

### Recommended order

1. **Do not download anything yet.**
2. Add source/provenance and frozen evaluation infrastructure.
3. Fix `nexus collect` so failed transcription does not silently consume the collection slot and so the user can approve/correct the transcript.
4. Collect the admin’s full 23-intent set successfully.
5. Build a MASSIVE English importer in dry-run mode that reports candidate mappings without mutating `dataset.json`.
6. Build a CLINC OOS importer that creates separate train/evaluation files.
7. Review the mapping report manually.
8. Train a candidate and compare against the existing model.
9. Only then consider a Prolific pilot.

### Recommended CLI design

```text
nexus data sources                 # list approved sources and licenses
nexus data fetch massive --lang en-US --dry-run
nexus data map massive             # produce mapping report
nexus data import massive --reviewed
nexus data import clinc --oos-only
nexus data audit                   # PII, secrets, duplicates, label conflicts
nexus data stats                   # distribution by source/intent/slot
nexus train --candidate            # never overwrite production automatically
nexus evaluate --candidate         # all frozen suites
nexus model promote                # explicit promotion after gates pass
```

### Go/no-go decisions

| Decision | Recommendation |
|---|---|
| Scrape live users silently | **No** |
| Ask users to opt in to sanitized transcript sharing | **Yes** |
| Upload raw voice by default | **No** |
| Keep raw voice forever | **No** |
| Import all of MASSIVE | **No** |
| Import a mapped English subset | **Yes** |
| Use CLINC OOS | **Yes** |
| Use Fluent/Snips/SpokenWOZ audio in production | **No** |
| Use public GitHub/Stack Overflow scrape | **No** |
| Commission 900 targeted recordings after baseline | **Yes, if evaluation shows need** |
| Train immediately on unreviewed external labels | **No** |

---

## 15. Final recommendation

The best data for NEXUS is not “the largest dataset.” It is the smallest set of records that represents:

- commands users actually attempt;
- how the deployed STT system actually transcribes them;
- the correct NEXUS intent and normalized slots;
- whether the command executed successfully;
- difficult nearby commands that must not be confused;
- clear consent, provenance, and deletion rights.

The optimal plan is therefore:

```text
First-party opt-in transcripts
    + prompted real speech through NEXUS STT
    + MASSIVE English mapped subset
    + CLINC OOS
    + SLURP text-only subset
    + NL2Bash curated developer negatives
    + validated synthetic GitHub commands
    + small paid diverse-speaker pilot
```

This gives NEXUS better semantic coverage and real ASR robustness without storing unnecessary audio, importing millions of irrelevant records, or relying on legally uncertain scraping.

---

## Primary references

### Public datasets

- MASSIVE: https://www.amazon.science/code-and-datasets/massive
- MASSIVE license: https://huggingface.co/datasets/AmazonScience/massive/blob/main/LICENSE
- CLINC150: https://archive.ics.uci.edu/dataset/570/clinc150
- SLURP text/repository: https://github.com/pswietojanski/slurp
- SLURP audio/Zenodo: https://zenodo.org/records/4274930
- Google Speech Commands: https://www.tensorflow.org/datasets/catalog/speech_commands
- Common Voice terms: https://commonvoice.mozilla.org/dag/terms
- LibriSpeech: https://openslr.org/12
- MultiATIS++: https://github.com/amazon-science/multiatis
- Schema-Guided Dialogue: https://github.com/google-research-datasets/dstc8-schema-guided-dialogue
- MultiWOZ: https://github.com/budzianowski/multiwoz
- NL2Bash: https://github.com/TellinaTool/nl2bash
- CoNaLa: https://conala-corpus.github.io/
- SpokenWOZ: https://spokenwoz.github.io/
- Fluent Speech Commands license: https://fluent.ai/wp-content/uploads/2021/04/Fluent_Speech_Commands_Public_License.pdf
- Snips/Sonos research datasets: https://github.com/snipsco/spoken-language-understanding-research-datasets

### Collection providers

- Prolific multimodal data: https://www.prolific.com/multimodal-data-for-ai
- Toloka platform: https://toloka.ai/platform
- Appen speech/audio: https://www.appen.com/speech-and-audio-training-data
- Defined.ai audio: https://defined.ai/dataset-type/audio
- DataForce/TransPerfect: https://www.transperfect.com/technology/artificial-intelligence/ai-data-collection-and-annotation
- Scale voice data: https://scale.com/blog/not-in-text-alone

### Data tooling

- Label Studio: https://github.com/HumanSignal/label-studio
- Argilla: https://github.com/argilla-io/argilla
- Presidio: https://github.com/microsoft/presidio
- Cleanlab: https://docs.cleanlab.ai/master/tutorials/text.html

### Platform and privacy terms

- GitHub Terms of Service: https://docs.github.com/en/site-policy/github-terms/github-terms-of-service
- GitHub REST API rate limits: https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api
- Stack Overflow content licensing: https://stackoverflow.com/help/licensing
- Stack Overflow data dumps: https://stackoverflow.com/help/data-dumps
- Groq Services Agreement: https://console.groq.com/docs/legal/services-agreement
- ICO AI lawfulness: https://ico.org.uk/for-organisations/uk-gdpr-guidance-and-resources/artificial-intelligence/guidance-on-ai-and-data-protection/how-do-we-ensure-lawfulness-in-ai/
- ICO data minimization: https://ico.org.uk/for-organisations/advice-and-services/audits/data-protection-audit-framework/toolkits/artificial-intelligence/data-minimisation/
- ICO biometric guidance: https://ico.org.uk/for-organisations/uk-gdpr-guidance-and-resources/lawful-basis/biometric-data-guidance-biometric-recognition/
- FTC Alexa/Ring guidance: https://www.ftc.gov/business-guidance/blog/2023/06/hey-alexa-what-are-you-doing-my-data
- California CCPA statute: https://leginfo.legislature.ca.gov/faces/codes_displayText.xhtml?division=3.&lawCode=CIV&part=4&title=1.81.5.
