# NLU Intent & Decision Models — Jev, Laya, and What to Actually Do

> **Part of the 2026-10 competitive & platform audit.**
> Decision record: [`docs/features/62-competitive-and-platform-audit-2026-10.md`](../62-competitive-and-platform-audit-2026-10.md)
> Companion compendium (identical content, canonical home):
> [`NEXUS-PAPERS → research/2026-10-competitive-audit/`](https://github.com/Engine-NEXUS/NEXUS-PAPERS)
>
> This file is **evidence**. It records what was verified on 2026-10-02, from
> where, and what could not be verified. It is not a work order — see the spec above
> for that.

> **What this covers:** The Jev and Laya verdict with independent benchmarks, the fine-tune path, and the local model landscape.


**Date:** 2026-10-02
**Scope:** Can "System One" decision models (Jev by TypeSafe AI, Laya by ConvAI
Innovations) replace our BERT-Mini intent tier? What is the state of the art for
local intent classification with calibrated confidence in Oct 2026?
**Scope note:** Multilingual is **out of scope**, which materially simplifies this
decision — see §7.

---

## 1. Verdict

| | Adopt | Pilot | Reject |
|---|---|---|---|
| **Jev** (TypeSafe, closed) | — | — | ✅ **Reject.** Fails 2 of 3 hard constraints: not offline-capable, and every independent latency measurement is 267–1017 ms p50 vs our ~50 ms budget. |
| **Laya** (ConvAI, open) | — | ✅ **Pilot only as a labelled-data sanity oracle** — not a runtime dependency | |
| **DeBERTa-v3-small + SiFT fine-tuned** | ✅ **Adopt** | | |

**Headline finding:** Laya's own marketing comparison table is **not reproducible
by the only independent head-to-head**, and Laya **loses by 22 accuracy points** on
the metric that matters most for us.

---

## 2. What Jev and Laya actually are

Neither is an LLM. They take a state (text/email/ticket/JSON) plus **typed
questions** and return **typed probabilistic answers** — `choice`, `score`,
`noul` — in a single forward pass. No text generation, so nothing to parse and
nothing to hallucinate.

### Jev (TypeSafe AI)

- Released **2026-09-15** after 2 years stealth, alongside a **$40M seed led by DCVC**
- Founded by **Diogo Almeida** — ex-OpenAI (Instruction Following, InstructGPT,
  ChatGPT, GPT-4), ex-Google Brain. Credited co-inventor of RLHF.
- Trained exclusively on synthetic data with **RLCD** (Reinforcement Learning for
  Calibrated Decisions)
- $0.042/M input tokens, **output free**, 32k context
- **Proprietary, early access + waitlist. No SLA. No weights, no paper.**
- Architecture undisclosed — TechCrunch: *"outside observers suspect it's built on
  top of an open-weight LLM."*
- TypeSafe explicitly admits **"We can't prove it isn't subsidized"** on pricing
- One model, one modality (text)
- Demand exceeded supply: *"the company briefly lost the ability to serve users
  from its API"*

### Laya (ConvAI Innovations / NandhaKishor M)

- **Apache 2.0 open weights**, released **2026-09-18** (13 days before this audit)
- `convaiinnovations/laya` on HuggingFace; PyPI `laya`; github.com/NandhaKishorM/laya
- Backbones: **ModernBERT-large 421M** (English) / **mmBERT-base 322M** (multilingual)
- Ships a **Jev-compatible HTTP API** (`laya-serve`, `POST /v1/systemone`) — an
  existing Jev client just repoints its `baseUrl`
- Also `laya[onnx]`, `laya[fast]` (TileLang CUDA), `laya[serve]`, `laya[mcp]`,
  `laya[langchain]`, `laya[llamaindex]`, `laya[crewai]`
- TypeScript/Node client: `laya-ts` (local ONNX, no Python) and `laya-client`
  (talks to `laya-serve`)

---

## 3. Independent verification — the decisive evidence

### 3.1 `sysone-bench` — the only fair head-to-head

**github.com/instax-dutta/sysone-bench** — MIT, created **2026-09-21**. Tiny
(6 stars, 1 author) but unusually rigorous:

- SHA-256-sealed manifest, verified before the run **and inside the container**
- One seed (42), **byte-identical question dicts for every model**
- 4 CPU / 12 GB containers, CPU-only
- Paired cluster bootstrap (20k replicates) + permutation tests + Holm correction
- `benchmark.report` **refuses to emit** a report unless recomputed counts match
  each run's sealed `summary.json`

**v1 (2026-09-21), 310 hand-labelled states — Jev wins all 3 suites:**

| Suite | Laya | Jev | Laya ECE | Jev ECE |
|---|---|---|---|---|
| triage (n=160) | 0.800 | **0.894** | 0.079 | **0.034** |
| guardrails (n=60) | 0.883 | **0.950** | 0.065 | **0.055** |
| moderation (n=90) | 0.833 | **0.989** | 0.082 | **0.054** |

**v2.0.0 (2026-09-26), 1,190 cases, 1,240 scored decisions, 9 suites:**

| Suite | Jev 1.13.0 | **Laya 0.3.11 (CPU)** | Qwen2.5-1.5B PCD |
|---|---|---|---|
| **all suites** | **0.9065** | **0.6863** | 0.6048 |
| triage, 5 intents | 0.9323 | 0.8750 | 0.7812 |
| banking77, 12 intents | 0.9479 | 0.8125 | 0.5833 |
| guardrails (2 binary) | 1.0000 | 0.7604 | 0.2917 |
| multilingual intent, 5 lang | 1.0000 | 0.4500 | 0.6833 |

> *"Jev wins every suite."* 8 of 9 paired deltas survive Holm correction.
> (Triage +0.057, Holm p=0.0627, is honestly *not* claimed.)

**Independence limits the author admits:** one human reviewer, no second
annotator, **no inter-annotator agreement, no kappa**, no adjudication artifact.
`provenance.json` records `independent_human_review: false`. Multilingual suite
flagged as *"the weakest evidence in the report."*

**Verdict on independence:** genuinely independent of both vendors,
methodologically excellent, but single-rater and small-n. It is credible because
**five other unrelated groups reach the same conclusion** — not because it is one
airtight study.

### 3.2 🔴 The calibration claim is reversed

Laya's site claims **ECE 0.081 vs Jev 0.246, "3× better calibration."** This is a
sleight of hand:

- The 0.081 comes from `typed-decisions` **after fitting one temperature per
  (question-type, option-count)**. The 0.246 is a third-party number from a
  different dataset and setup.
- **Laya's own BENCHMARKS.md on the same benchmark** shows
  `laya-typed-decisions` **raw ECE 0.213 vs Jev's 0.144**. **Raw Laya is worse.**
- **sysone-bench v2, same questions, same run:** Jev choice ECE **0.00092**,
  Laya **0.00213**. **Jev is better.** The author notes *"temperature scaling is
  not a dependable fix and the raw numbers are the ones to quote."*
- **Soft-distribution accuracy:** Jev **0.580** vs Laya **0.471**.

⚠️ Fair counterweight, also from sysone-bench: *"The low ECE of Jev's choice
numbers comes from near-saturated probabilities, not from a better calibration
method."* Jev's great ECE may be a saturation artifact.

### 3.3 The RLCD thesis is under active independent attack

**Issue #741** (`NandhaKishorM/laya`, author `PengyiZhang`, **2026-09-29, still
open, 0 comments**) — a controlled 3-arm ablation of Laya's *own* fine-tuning
notebook, on Laya's *own* model and data:

| Arm | Hard acc | ECE | Brier |
|---|---|---|---|
| A: GRPO + proper reward (**the RLCD recipe**) | 79.08% | 11.18% | 0.0543 |
| **B: plain soft cross-entropy** | **80.50%** | 11.82% | **0.0501** |
| C: direct differentiable proper-scoring | 79.92% | 12.04% | 0.0516 |

**The headline training method of BOTH vendors shows no gain over soft
cross-entropy** — slightly worse on accuracy and worse on Brier. The argument is
mathematically correct: soft CE *is* the expected log score, already a strictly
proper scoring rule with the same optimum; GRPO's score-function estimator just
adds Monte-Carlo variance. They also note: *"the notebook ships no CE-only arm, so
the RLCD term's benefit has never been isolated in a controlled comparison."*

→ **If we fine-tune Laya we should use plain soft CE and skip the RLCD term.**

### 3.4 Two shipped bugs that hit our exact shape

**Issue #790 — INT8 ONNX export collapses the model** (found 2026-10-01, fix #792 merged):

| Variant | Decision agreement vs fp32 | Max prob drift |
|---|---|---|
| ONNX fp32 | **100%** (0 drift) | 0.000 |
| dynamic **per-channel** — *shipped default* | **32%** | 0.92 |
| dynamic per-tensor | 67% | 1.00 |
| static QDQ MinMax per-tensor | 45% | 0.92 |

→ **fp32 export is faithful; INT8 is not usable. No INT8 memory saving.**

**Issue #783 — `choice` is NOT permutation-invariant** (**PR still OPEN**):
`option_order_flip_rate: before = 0.13 → after = 0.00`. Laya's own BENCHMARKS.md
measures flip rate **0.150 on English 20-option MASSIVE intent, 0.230 on
multilingual**. **At our 55 options this is worse.** A voice assistant cannot flip
15%+ of decisions because the label dictionary was reordered.

### 3.5 The zero-shot quality floor — confirmed, and worse than advertised

| Model | Accuracy | Random | **Majority class** |
|---|---|---|---|
| `laya` base | **0.362** | 0.318 | **0.461** |
| `laya-multilingual` base | 0.352 | 0.318 | **0.461** |

> **The shipped base checkpoints score BELOW the majority-class baseline.**
> Laya's README states this plainly: *"Laya is a fast base to specialise, not a
> zero-shot decision engine."*

The closest published analogue to our task, from Laya's own benchmark:

| Task | Accuracy |
|---|---|
| **Support triage, 10-way queue** | **0.502** ← essentially chance |
| Model routing (domain), held out | 0.639 |
| Moderation, held out | 0.530 (macro-F1 0.400) |
| Banking77, 77 labels | **0.425** |

Banking77's 0.425 is **architectural, not capability**: options share a fixed
`head_max_len=192` (English), so at 77 options each label gets ~3 tokens.
**Laya's own rule: "Keep choice questions under ~20 options."**

🔴 **At our 55 intents on the English checkpoint, `(192-16)//55 ≈ 3.2 tokens per
label` — we are on the wrong side of that cliff.** Their suggested workaround
(`head_max_len=512`, `max_len=1024+`) is config surgery, not a fix, and adds tokens
to a path that is already 580 ms.

**Reproduction, in Laya's favour:** issue #776 independently re-ran Laya's MASSIVE
+ XNLI claims from a fresh harness — all four reproduce **within 0.0003**,
deterministic across machines. But that same issue surfaced a critical caveat:
**thresholds do not transfer across option counts.** At a 0.90 accuracy target the
advised `min_confidence` gate was **0.8074 for 3-option questions vs 0.9944 for
20-option.** *"A single global threshold cannot serve both shapes."* (Tracked #394.)

### 3.6 Jev's independent verification is genuinely strong

Five independent evaluations, not one:

| Source | Finding |
|---|---|
| **jevals.com** (2026-09-18, CC-BY per-decision logs) | noul/PubMedQA: Jev **69.0**, tied #1 with Gemini 3.8 Flash (73.0); $0.03 vs $0.80 per 1,000; p95 0.65 s vs 5.5 s. choice/Banking77: Jev **67.8 (79.7% acc)** 2nd. Latency from residential line: **median 0.44–0.48 s, p95 0.65–0.69 s** |
| **nibzard/decision-model-benchmark** (2026-09-29, **40,226 decisions**, $1.71) | Banking77 79.2% acc, p50 316 ms, p95 570 ms. CLINC150: **88.6%**, OOS precision 90.3% / recall 81.2%. *"Jev's native confidence remains a provider-defined score, not a calibrated probability."* |
| **AbdelStark** (preregistered protocol) | BTZSC 72 labels: **Jev 0.870 vs GLiNER2.5 0.610**; coverage at ≤5% error **0.860 vs 0.270**. DAIR Emotion: Jev Brier **0.846**, zero prob on the true label for **16%** of examples |
| **Arize AI** (Sept 2026, **23,325 judgements**, two human-labelled datasets, 0 failed calls) | Tuned cutoff 0.80 → Jev **87% acc / 13% FPR**, **identical to Opus 5 to within a thousandth**. 141 ms. ~300× cheaper. At the *default* 0.5 cutoff Jev looked 7 points worse. **"Threshold tuning changed the comparison enough to change which judge I would choose."** |
| **LangChain** (2026-09-19) | 0.44 s, $0.00035/call; quality-score variance **92–913× lower** than LLM judges |

Plus `harrymunro/jev-laya-benchmark` (1,470 items): **Jev 64/64 vs Laya 20/64** on 64
Feishu scenarios; **Jev 97.6 vs Laya 36.9** on 40 Japanese questions.

### 3.7 Jev real-world adoption — thin but real

✅ **Vercel** — Pranit Sharma (SWE), via TechCrunch: replaced an OpenAI
classifier with Jev → *"five to 18 times more quickly and with greater accuracy."*
✅ **Arize AI** — *"I think we are going to rearchitect our apps to use Jev."*
✅ **LangChain** — published a guide.
✅ Armin Ronacher, Nikhil Mudholkar (Bryo AI CTO) — *"the only one that hands back
a real probability."*

⚠️ But: early access + waitlist, no SLA, no weights, no paper. **We are not on the
waitlist and should not plan around being on it.**

---

## 4. Laya adoption — serious project, loud README

| Metric | Value |
|---|---|
| Stars / forks | **29,765 / 2,580** in 13 days |
| Contributors | **30+** (top 3 = 676 of ~1,100 contributions) |
| Issues + PRs | 841+, 95 open |
| PyPI | **31 releases in ~2 weeks**, ~149k downloads/week |
| HF likes | 4,816 (main) |

**Verdict: it is a serious project.** The evidence is behavioural, not just
stars — non-maintainers filed four genuinely expert reports (#741 RLCD ablation,
#790 INT8 collapse, #776 successful reproduction, #783 permutation invariance),
all found real bugs, and **maintainers fixed #792 and #42 and published corrected
numbers**, including admitting that a committed benchmark table **did not
reproduce on 6 of 51 languages**.

⚠️ But the star count should be discounted: 29.7k in 13 days is a launch-shaped
spike. **31 releases in 14 days and 95 concurrently-open issues indicate a
*moving* API and heavy support load, not maturity.**

⚠️ **The ecosystem directories are marketing.** `madewithlaya.com` (115 builds,
**paid sponsor slots at $15**) and `laya.tools` both exist. Composition of those 115
"builds": Snake, Tetris, Breakout, Pac-Man, Gomoku, tic-tac-toe, Doom,
chrome-dino. **Almost all are Apple-Silicon MLX/MPS demos.** To its credit,
madewithlaya hosts benchmarks **critical of Laya**.

**Real integrations exist** (all third-party, all outside our target):
`@receptron/laya` (Node/TS via ONNX Runtime), `jev-nx` (Elixir), `laya-mcp`,
LangChain wrapper, MCP server, Docker, `laya-serve`, **CoreML port (3.7 ms on M5
Pro, 99.5% of ops on the Neural Engine)**, AXERA AX650/AX8850 edge NPUs <70 ms,
LiteRT/tflite, `laya-browser`.

🔴 **Zero production case studies. Zero named enterprise deployments. Every
headline latency figure is an M-series chip or an edge NPU — none is x86 Linux
CPU.**

**Lookalikes to avoid:** `openjev/openjev`, `jevlike`, `Jev-verdict-2.0`, `Nimble`,
`Decider`, `Kev-4B`, and the SEO sites `jevtypesafeai.com`, `jevtypesafe.site`,
`jevmodel.org`. **Only `typesafe.ai` and `laya.convaiinnovations.com` /
`convaiinnovations/laya` are real.**

---

## 5. Feasibility for our constraints

Constraints: **Linux laptop, CPU-only, ~50 ms, no GPU, offline, 55 classes,
calibrated confidence.**

### 🔴 Latency — hard fail

Laya's own BENCHMARKS.md (self-measured, p50, **x86 CPU**):

| Hardware | Checkpoint | 1 question |
|---|---|---|
| AMD EPYC 9R14, 4 cores | english (421M) | **580 ms** |
| " | multilingual (322M) | **193 ms** |
| " | typed-decisions (421M) | **584 ms** |
| Ryzen 9 6900HX, 8 threads | english | **329 ms** (p95 378) |
| " | 1 thread | 910 ms |
| Intel Arc B390 laptop CPU | english | **288 ms** |
| sysone-bench, 4 shared CPUs | english | **588 ms p50 / 1,364 ms p95** |

**The 193 ms figure is the multilingual checkpoint, not the one we'd use.** The
English ModernBERT-large checkpoint is **580 ms** on the same 4-core EPYC, **288–329
ms** on a modern laptop core.

**Is 288–580 ms usable in a voice loop? No** — 6–12× our entire budget, plus a
1.4 s p95 tail. And the repo's own warning: with default torch threading, one
3-question call took **9,396 ms** p50; pinning `set_num_threads(8)` +
`set_num_interop_threads(1)` recovered 12×. *"Batching questions saves little on
CPU."* **The 32.8 ms headline is Tesla T4 only.**

### ONNX without torch — partial yes, INT8 no

- ✅ `laya[onnx]` works CPU-only. **fp32 export is faithful: 100% agreement, 0.000
  drift.** This part is solid.
- ❌ **INT8 unusable** — shipped default (per-channel) **32% decision agreement**.
- ⚠️ `ONNXAgent` exists, but the Python package still pulls `transformers`/`torch`
  — the ONNX path is for dropping in a pre-exported graph, not a torch-free install.

### 🔴 Memory footprint — the delta is brutal

| | Params | fp32 | INT8 | vs ours |
|---|---|---|---|---|
| **Our BERT-Mini** (`bert_uncased_L-2_H-128_A-2`) | **4.4 M** | **17.6 MB** (measured on disk) | ~5 MB | 1× |
| Laya multilingual | 322 M | ~1.29 GB | — | **73×** |
| Laya English | **421 M** | **~1.68 GB** | broken | **~93×** |

Laya's own script peaked at **9.3 GiB RSS** with 5 checkpoints loaded. Our
`AGENTS.md` documents **104 MB idle RAM** as a headline achievement. **Laya alone
would be ~8× our entire idle footprint**, before Python/torch.

### Fine-tuning — doable, but we'd be doing it wrong

- Kaggle 2×T4 notebook (~4 h). Issue #741 gives the recipe.
- We would **skip RLCD** (plain soft CE beat it).
- We would **skip the ONNX INT8 path** (broken).
- We would need `head_max_len ≥ 512` + `max_len ≥ 1024` just to fit 55 options —
  **tripling the sequence length that is already our latency problem.**
- We would still have a **15–28% option-order flip rate** until #783 merges.
- We would need temperature fitting per (question-type, option-count) — which we
  already have.
- Our ~3,000 rows would fine-tune fine. **That is the one thing that works.**

### 🔴 Bottom line

**We would be spending 96× the memory and 20× the latency to roughly *match*, not
beat, a closed zero-shot API** — while still needing to fix its shipped bugs.

---

## 6. The alternative: fine-tune a standard small encoder

**This should be the whole conversation.** Our 3,108 rows / 55 intents is a
textbook in-domain intent-classification setup.

### Published accuracy, full-train fine-tuned, in-domain intent

| Model | Params | CLINC150 (150 cls) | Banking77 (77 cls) | HWU64 (64 cls) |
|---|---|---|---|---|
| USE-base | — | 81.6 | 75.2 | 83.4 |
| RASA (no BERT) | — | 68.3 | 76.9 | 78.9 |
| **DistilBERT-base** | 66 M | **85.7** | **79.2** | 87.4 |
| **BERT-base** | 110 M | **87.6** | **81.7** | 87.6 |
| **RoBERTa-base** | 125 M | **88.4** | **83.8** | 88.5 |
| BERT-large | 340 M | 89.5 | 83.9 | 89.2 |
| TOATOD small | — | **98.45** | **92.40** | 90.42 |
| SPACE 2.0 | — | 97.80 | 94.77 | 94.33 |

*(NAACL Industry 2021, difficult splits; Findings-ACL 2023, standard splits.
Solanki 2025 independently: RoBERTa 88.8% acc / 88.2% F1 on CLINC-150.)*

### Against the System One options on the same-shaped task (Banking77, 77 intents)

| System | Accuracy |
|---|---|
| **Jev zero-shot** | **79.2%** (nibzard, 3,080 official test msgs) / 79.7% (jevals) |
| Laya zero-shot | **0.425** (architectural ceiling) / 51.3% base checkpoint |
| Laya fine-tuned on 1,001 examples | 79.4% |
| **DistilBERT-base fine-tuned** | **79.2%** — *identical to Jev, at 1/6 the parameters, offline, on CPU* |
| BERT-base fine-tuned | ~93–94% on the standard Banking77 test set |

**Why we would beat all of them:** CLINC150 gives 100 train rows/class, Banking77
gives 130/class. We have ~56/class. But **our utterances are templated and narrow**
(`"open chrome then search for cats"`) versus diverse banking prose — far lower
label entropy — and **our regex tier already handles the easy majority**, so the
NLU only needs the long tail.

**Honest estimate: 85–93% held-out intent accuracy, likely higher on the
templated subset.**

### Size and latency

| Model | Params | fp32 | INT8 | CPU latency |
|---|---|---|---|---|
| **Our BERT-Mini (today)** | 4.4 M | 17.6 MB | ~5 MB | ~2–5 ms |
| **DeBERTa-v3-small + SiFT** | **22 M** | ~86 MB | **~22–25 MB** | **~5–15 ms @ seq 64** |
| all-MiniLM-L6-v2 | 22.7 M | 90 MB | **22 MB** | **20–50 ms** (measured, desktop) |
| Laya English | 421 M | 1.68 GB | broken | 288–580 ms |

🔑 **DeBERTa-v3-small+SiFT is the best size/quality point available.** 22 M
backbone, MNLI **88.8/88.5** — it actually **beats** the 44 M `deberta-v3-small`
(88.3/87.7) *while halving the backbone*, because SiFT shares the 128K-vocab
embedding (98 M params of pure overhead we would otherwise pay for nothing).

**Verdict: it dominates.** 20–100× faster, 20× smaller, fully offline, no
dependency on a 3-week-old repo, no waitlist, no vendor — and **we already own
the pipeline** (`server/nlu/train.py`, temperature calibration,
`evaluation_lock.json`).

---

## 7. Why dropping multilingual makes this easier

Per §6 of the executive summary, multilingual is out of scope. Consequences:

| Effect | Detail |
|---|---|
| **Removes the largest unmeasured risk** | **No verified Telugu benchmark exists for any sub-6B model.** Dropping the claim removes the gap rather than shipping around it. |
| **Unlocks the published literature** | English-only enables CLINC150 / Banking77 / HWU64 numbers to apply directly, instead of extrapolating from multilingual benchmarks |
| **Enables SiFT** | DeBERTa-v3-small+SiFT's shared 128K-vocab embedding trick is an English-vocab optimisation. With a 55-intent English label set we get the 22 M footprint. |
| **Removes the SiFT/multilingual tradeoff** | A multilingual-intent encoder needs a large vocab; an English-only one does not. This is where a large share of our RAM saving comes from. |
| **Wake word too** | livekit-wakeword's multilingual DET curves are worse because the frozen Google speech-embedding is English-dominant. English-only gets the good numbers. |

---

## 8. Local model landscape (English, 1–6 GB, CPU-viable)

### Available now

| Model | Params | GGUF/quant | Licence | Notes |
|---|---|---|---|---|
| **Qwen3.5-4B** | 4B (4.66B w/ 248K vocab) | **3.4 GB** Q4_K_M | Apache 2.0 | Hybrid Gated DeltaNet + Gated Attention, 75% linear attn, 32 layers, 1 MTP head. 262K ctx → 1M YaRN. MMLU-Pro 79.1%, GPQA-D 76.2% *(vendor)* |
| Qwen3.5-2B / 0.8B | 2B / 0.8B | — | Apache 2.0 | "small, fast, ideal for edge devices" *(vendor)* |
| **Gemma 4 E4B** | 4.5B eff / 8B total | **2.3 GB** quantized | **Apache 2.0** ← first Gemma under Apache | 128K ctx, text+image+**audio**. MMLU-Pro 69.4%, Tau2 42.2% |
| Gemma 4 E2B | 2.3B eff / 5.1B total | **0.8 GB** quantized | Apache 2.0 | MMLU-Pro 60.0% |
| **Nemotron 3 Nano 4B** | 3.97B | **2.8 GB** Q4_K_M | NVIDIA OML | Hybrid Mamba-2/Attention MoE. **18 t/s on Jetson Orin Nano 8GB, llama.cpp Q4_K_M** |
| **Granite 4.1 3B** | 3B | ~2 GB Q4 | Apache 2.0 | MMLU 67.02, BBH 75.83 |
| Phi-4-mini-instruct | 3.8B | **2.49 GB** Q4_K_M | **MIT** | MMLU 67.3. **No Phi-5 exists** — Phi-4 is still current. Feb-2025 vintage |
| SmolLM3-3B | 3B | — | Apache 2.0 (fully open: data + recipe) | 128K, `/think` + `/no_think`, **6 languages** |
| Ministral 3 3B | 3B | 3.0 GB | Apache 2.0 | text+image. Dec-2025 |
| gpt-oss-20b | 21B tot / 3.6B act | ~13 GB MXFP4 | Apache 2.0 | **Out of RAM budget** — MoE, all experts must be resident |

⚠️ **Do not consider Tiny Aya** despite it being the only small model with genuine
Telugu support: **CC-BY-NC-4.0 is non-commercial.** That was a hard blocker for a
product; it is now moot since multilingual is out of scope.

### CPU tok/s — what is and isn't verifiable

| Model / quant | tok/s | Hardware | Confidence |
|---|---|---|---|
| Nemotron 3 Nano 4B Q4_K_M | 18 | Jetson Orin Nano 8GB (ARM) | **High** — NVIDIA's own number |
| Qwen3.5-4B Q4_K_M | 22.1 | Apple M1 Pro (ARM) | Medium — third-party |
| Llama-3.2-1B Q8_0 | 68.1 | Apple M1 Pro | Medium |
| Llama-2-13B Q4_K_M | 8.12 tg / 54.2 pp | Ryzen 5900X, 12 threads | Medium — old model, cleanest x86 datapoint found |
| qwen3.5 (9.65B) Q4_K_M | 69.5 "CPU only" | **unspecified** | ❌ **Implausible. Do not cite.** |

🔴 **No trustworthy x86_64 CPU tok/s table exists for any 2026 small model.** SEO
listicles are largely fabricated or mis-scaled. **Run `llama-bench` on our actual
target hardware.**

### What's genuinely new in 2026

1. **Licence convergence to Apache 2.0.** Gemma 4 is the first Gemma under Apache.
   Qwen, Granite, SmolLM3, Ministral, gpt-oss all Apache. Phi is MIT. **18 months
   ago "Gemma" was a blocker.**
2. **Native function calling + structured JSON at the 2–4B tier.** In 2024 this
   was 7B+ territory.
3. **Gated DeltaNet / Mamba hybrids.** Much cheaper long-context prefill — directly
   relevant since our 55-label prompts are prefilled on every utterance.
4. **MTP drafters shipped for small models.** ⚠️ **But MTP likely does not help
   us** — on a dense Qwen3.6-27B it measured **0.58× (42% slower)** because dense
   decode is bandwidth-bound and MTP adds work. Only MoE benefited.

### Embedding models for on-device semantic matching

| Model | Params | Dim | Ctx | Licence | ML MTEB | EN MTEB |
|---|---|---|---|---|---|---|
| **ibm-granite/granite-embedding-97m-multilingual-r2** | **97M** | 384 | **32,768** | **Apache 2.0** | 59.6–60.3 | 50.1 |
| ibm-granite/granite-embedding-311m-multilingual-r2 | 311M | 768 (Matryoshka) | 32,768 | Apache 2.0 | 64.0–65.2 | 52.6 |
| Qwen3-Embedding-0.6B | 0.6B | 1024 | 32,768 | Apache 2.0 | — | 70.47 |
| bge-m3 | 568M | 1024 | 8,192 | MIT | 62.34 | — |
| intfloat/multilingual-e5-small | 96M | 384 | 512 | MIT | 50.9 | — |

**Recommended: `granite-embedding-97m-multilingual-r2`** — ModernBERT, 97M, 384-dim,
32,768 ctx, **Apache 2.0**, **pre-quantized ONNX available**, released 2026-04-29.
**97M is essentially our current BERT-Mini's footprint — a like-for-like swap.**
Higher MTEB multilingual retrieval than multilingual-e5-small at near-identical
throughput (2,894–3,379 vs 2,290–2,604 spans/s). Bonus: code retrieval 60.5,
LongEmbed 65.6.

⚠️ **IBM's own sources disagree** — HF card and blog say 60.3; the GitHub
comparison table says 59.6. Small enough not to change the ranking.

### 🔑 Immediate near-free win regardless of model choice

Per **Bekko-embedding-v1-a8m** (arXiv 2607.25180): a static vocabulary embedding
matrix dominates small encoder size — of a 140M-param mmBERT-small, **~98M params
are the vocabulary matrix**. **Int8-quantizing it cuts distribution size ~70% with
an average retrieval change of −0.0001.**

Our `nexus_nlu.onnx` has a 248K/262K-class vocabulary and a tiny transformer body
— the embedding matrix very likely dominates file size. **Quantize it to int8,
keep the transformer int8, keep the classifier head fp32. A day of work for a ~70%
file reduction.**

### Runtime

**llama.cpp 0.3.x** — 94.7k stars, the most widely deployed local runtime in
existence. We are on 0.2.0+/0.3.0+ (2026-08-21 / 2026-08-25).

CPU tuning: `--threads` = **physical** cores not hyperthreads; separate
`--threads-batch`; `--batch-size 512` + `--ubatch-size 256` on CPU;
`--flash-attn on`; **minimise `--ctx-size`** (prefill dominates our latency and
scales linearly with ctx).

**GGUF quantization guidance** (from llama.cpp's own `tools/quantize/README.md`,
Jul 2026):

| Quant | Speed | Quality | Use |
|---|---|---|---|
| Q2_K | fastest | lowest | experimentation |
| Q3_K_M | very fast | low | resource-constrained |
| **Q4_K_M** | **fast** | **good** | **recommended default** |
| Q5_K_M | moderate | very good | if it fits |
| Q6_K | slower | excellent | near-original |
| Q8_0 | slowest | highest | reference/eval |

Hard rules: **never quantize from an already-quantized file** (HF → F16 GGUF →
quantize); use `--imatrix`.

**Other engines:** ONNX Runtime is the right path for **encoder** models
(pre-quantized community builds exist). `candle` is Rust-native and a good fit for
our codebase, but no head-to-head CPU numbers are retrievable — measure ourselves.
**vLLM 0.6.3 cannot even load Gemma-2, Falcon, or OLMo-2** — do not use as a CPU
path.

---

## 9. The highest-leverage research result (applies regardless of model)

**Red Hat, 2026-06-02** — *"Improve vLLM Semantic Router accuracy with
fine-tuning."* Same base model, same router, **only the training objective
changed**: cross-entropy + autoencoder → **BatchAllTripletLoss with GROUP_BY_LABEL
sampling**, on a synthetic dataset with paraphrases, domain transfers, boundary
cases, **hard negatives, and typos**.

| Metric | Baseline | Fine-tuned | Δ |
|---|---|---|---|
| Routing accuracy (4 tiers) | 80.39% | **98.53%** | **+18.1 pp** |

> **This is the single most transferable number in this document.** 4 tiers → 55
> classes is harder, but the direction is unambiguous: **fine-tuning the encoder we
> already have, with a contrastive objective and hard negatives, dominates buying
> a bigger model.**

Supporting: **Zhang et al., EMNLP-I 2024** — adding an in-scope embedding
reconstruction (autoencoder) loss as a regulariser on top of cross-entropy
prevents in-scope embeddings from dispersing, giving **+1–4% AUPRC for
out-of-scope rejection at zero cost to in-scope accuracy**. That is exactly our
calibrated-confidence + OOS problem.

**DETER** (LREC-COLING 2024) — dual encoders + synthetic outlier generation +
threshold-based reclassification: +13%/+5% F1 (known/unknown) on CLINC-150,
+16%/+24% on Banking77.

### Three findings from the 41-model zero-shot study (arXiv 2607.27421)

1. **The most common error is a semantically overlapping label pair, not model
   weakness.** `play_music → music_query` was the #1 error for **21 of 41 models
   (51%)** and top-3 for 26/41, spanning every family and scale. **We have exactly
   this problem**: `browser_search` vs `browser_navigate`, `whatsapp_open` vs
   `whatsapp_search`, `list_prs` variants, `merge_pr` vs `close_pr`.
   **Label descriptions will move our accuracy more than any model swap.**

2. **Never let a reasoning model into a short-budget classification path.**
   DeepSeek-R1-Distill-Qwen-1.5B scored **0.000** on MASSIVE — its `<think>` trace
   consumed the entire 20-token budget before answering. The 8B version was fine.
   **Our Qwen2.5-0.5B brain must be pinned to non-thinking mode with a hard token cap.**

3. **ASR noise is the dominant robustness risk and is not predicted by clean
   accuracy.** Under typos: Qwen2.5-7B lost **1.3 pp**; Llama-3.1-8B 6.9 pp;
   Qwen2-7B **11.7 pp**; R1-Distill-Qwen-7B 25.9 pp. **Qwen2-7B had the *highest*
   clean accuracy (0.718) yet degraded ~9× more than Qwen2.5-7B.**

⚠️ Caveats on that study: evaluated on **vLLM 0.6.3** (a **2024** release) on a
**V100 fp16**. Not CPU latency. vLLM 0.6.3 **could not load** Gemma-2, Falcon, or
OLMo-2, so the 41-model cohort is effectively **pre-2025**. **There is no
equivalent systematic zero-shot intent-classification study covering Qwen3.5,
Gemma 4, Nemotron 3 Nano, or Granite 4.x.** Calibration is computed from free-text
logprobs, a *proxy* for label confidence. **On MASSIVE, all 10 pairwise
comparisons among the top-5 models are non-significant** (McNemar p = 0.14–0.34).

### Calibration layer for our cascade

- **UCCI** (arXiv 2605.18796) — token-level margin → **isotonic regression** →
  calibrated error probability → threshold by constrained cost minimization.
  **ECE = 0.03**. Small model 47.2 ms vs large 142.3 ms.
- **RACER** (arXiv 2603.06616, 2026-02-20) — post-hoc transformation into
  calibrated set prediction with finite-sample risk guarantees, **up to +4.0 pp
  absolute accuracy, no retraining**, supports abstention.

→ **We already ship `temperature_calibration.json`. A UCCI-style isotonic layer over
our existing regex → BERT → brain cascade is a drop-in and converts
hand-tuned thresholds into a measured risk-controlled policy.**

---

## 10. Recommendation

1. **Reject both.** Spend one sprint on the boring path.
2. **Fine-tune DeBERTa-v3-small + SiFT** (22 M, ~22 MB INT8) on our existing
   55-intent dataset. Expected **85–93%**, ~5–15 ms CPU, fits inside our existing
   `train.py` → ONNX → `evaluation_lock.json` pipeline with **no new runtime
   dependency**.
3. **Change the training objective before changing anything else.** Red Hat's
   +18.1 pp is the highest-leverage result available. BatchAllTripletLoss with
   GROUP_BY_LABEL + hard negatives from our existing confusion clusters.
4. **Add the Zhang et al. reconstruction loss** to our cross-entropy for OOS
   rejection — +1–4% AUPRC at no inference cost.
5. **Write explicit label descriptions into the encoder input.** 21/41 models made
   the identical overlapping-pair error. No model swap fixes it.
6. **Add a UCCI-style isotonic calibration layer** over the existing cascade.
7. **Quantize our embedding matrix to int8** — ~70% file reduction, −0.0001
   accuracy change. Do this regardless of model choice.
8. **Pin the admin brain to Qwen3.5-4B Q4_K_M (3.4 GB, Apache 2.0)**, non-thinking
   mode with a hard token cap. Runner-up: Gemma 4 E4B (first Gemma under Apache,
   native function calling) — but we would pay for a 305M audio encoder and
   150M vision encoder we will never use, and 128K ctx vs Qwen3.5's 262K.
9. **Steal exactly one idea from Jev: the risk-coverage curve.** The single most
   replicated positive finding across all five independent Jev evaluations:
   - jevals: at a 95%-accuracy target, Jev acts alone on **86%** of noul decisions
   - nibzard: Banking77 at ≤5% error → **51.2% coverage, 3.68% test error**
   - sysone-bench: **83% coverage @ 0.988 accuracy** vs Laya 69% @ 0.953
   - Arize: *"Threshold tuning changed the comparison enough to change which judge
     I would choose."*

   **We already ship `temperature_calibration.json`. Add a `coverage_at_error`
   table (threshold → % of commands auto-executed → observed error rate) to
   `nexus audit` and gate releases on it.** That is what makes a 55-class
   classifier safe to auto-execute, and it costs one day.
10. **Use `sysone-bench` as a free external oracle.** MIT, self-contained, and the
    only artifact pitting both candidates on byte-identical inputs with
    permutation tests.
11. **Do not port `laya-serve`, the MCP server, or the router.** Our 55 intents are
    fixed, so a plain 55-way softmax head is smaller, faster, and deterministic.
    The typed-decision abstraction solves a problem we do not have.
12. **Measure our current BERT-Mini first.** Our `AGENTS.md` notes the current NLU
    has *"low accuracy (5-9% confidence)"* from malformed labels. Before adopting
    anything, measure the current model on the locked test split **with the
    coverage-at-error table**. If it already clears 80% at 20% coverage, this
    entire investigation is moot — and the sprint should go to **data cleanup**,
    not model shopping.

---

## 11. What would change our mind

**On Laya:** #783 merged + a working INT8 path at >95% fp32 agreement + a
published CPU latency under 100 ms on a 4-core x86 host at 55 options + a held-out
55-intent number. **None of these exist today.**

**On Jev:** self-hosting or an on-prem binary. It does not exist and the company
shows no sign of shipping one. (And at 267–1017 ms p50 it would miss our budget
anyway.)

---

## 12. What could not be verified

- **x86_64 Linux tok/s** for Qwen3.5-4B, Gemma 4 E4B/E2B, Nemotron 3 Nano 4B,
  Granite 4.1 3B, or Ministral 3 3B. Every concrete number found is ARM or GPU.
- **GGUF sizes at Q8** for several models. Only Qwen3.5-4B (3.4 GB Q4_K_M),
  Nemotron 3 Nano 4B (2.8 GB), Phi-4-mini (2.49 GB), Gemma 4 12B (7.65 GB) confirmed.
- **Any systematic zero-shot intent-classification study covering 2026 models.**
- **candle vs llama.cpp CPU head-to-head numbers** (bench repo exists, results not
  retrievable).
- **Laya's acquisition curve** (GitHub API rate-limited on stargazers). 29.7k in 13
  days is launch-shaped and should be discounted.