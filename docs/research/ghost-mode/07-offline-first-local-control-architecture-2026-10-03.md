# 07 — Offline-First Local Control Architecture: Removing Cloud/Quota Dependency from Laptop Control

**Date:** 2026-10-03
**Status:** Research complete, implementation plan proposed, not yet built.
**Trigger (user, verbatim intent):** Ghost Mode's basic laptop-control verbs
(open / close / delete / search / type) should never depend on a cloud model,
an API key, or a request quota, and should never add network latency —
"controlling the laptop doesn't need internet." Asked for an industry survey
(open-source repos + papers) before committing to a plan.

Companion docs: `01-clicky-and-grounding.md`, `02-costs-and-limits.md` (prior
grounding-cost research this builds on), `06-dual-grammar-modal-partitioning...md`,
`docs/9router-research/02-brain-research-low-ram-thinking-model.md`.

---

## 0. What's already true vs. what needs fixing

Important framing before the plan: **NEXUS does not need a new model trained
from zero.** It already ships two local models, bundled in the installer,
that run with zero internet once installed:

| Model | Size | Format | Runs via | Status today |
|---|---|---|---|---|
| BERT-Mini NLU classifier | ~18 MB ONNX | `server/nlu/model/nexus_nlu.onnx` | Python FastAPI sidecar on :39218 (lazy-spawned) | Works, but network+process overhead, 85% confidence floor, and its output is thrown away for system-control intents (§3.1) |
| Qwen2.5-0.5B-Instruct | ~400 MB GGUF (Q4_K_M) | `server/admin/model/*.gguf` | `llama-cpp-python` sidecar on :39219 (lazy-spawned) | Works, fully local, but **admin-only gated** and not grammar-constrained (§3.3) |

So "train and import a model" is mostly **already done**. The actual gap is
architectural: the pipeline still defaults to the cloud Worker for anything
these two local models don't explicitly claim, and two of the wiring steps
that would let them claim more were left half-finished. That's fixable
without new training data or a new model.

---

## 1. Industry survey

### 1.1 Offline intent parsing for device control is an old, solved problem

[Rhasspy](https://github.com/rhasspy/rhasspy) and [Snips NLU](https://arxiv.org/pdf/1805.10190)
(Snips Voice Platform, arXiv:1805.10190) are the reference prior art for
exactly this use case: fully offline voice command parsing for "open/close/
turn on/play/search"-style device control, no cloud, no API key, tuned for
constrained vocabularies rather than open-domain conversation. Snips' own
paper reports competitive intent-accuracy against cloud NLU engines (Google/
Amazon/Microsoft/api.ai) at the time, running entirely on-device. The design
principle that mattered: **small, closed vocabulary + deterministic slot
grammar beats a general LLM for this narrow task, both in latency and in
reliability.** NEXUS's two-tier design (deterministic regex → BERT-Mini) is
already the same shape as Snips' (rule templates → lightweight intent
classifier) — the industry didn't abandon this approach, it's just not
talked about as much because it isn't "LLM" branding.

### 1.2 On-device function-calling models confirm small models are enough for this task

[Octopus v2](https://arxiv.org/pdf/2404.01744) (Stanford, arXiv:2404.01744):
a 2B on-device model (Gemma-2B fine-tune) fine-tuned with "functional
tokens" for function-calling beats GPT-4 on accuracy *and* latency for
function calls, cuts context length 95%, completes a call in 1.1–1.7s on a
phone CPU. The relevant finding isn't "go get Octopus v2" (it's a Stanford
research checkpoint, not a drop-in product) — it's the validated principle:
**fine-tuning a small model on a closed, known set of callable actions
reliably beats prompting a bigger general model**, and it is explicitly
positioned for "reduced latency, offline operation, lower cost, improved
data security." That is precisely the shape of "open/close/search/type."

### 1.3 Desktop GUI agents: the industry's own answer is "UIA first, vision last"

[Microsoft UFO²](https://arxiv.org/html/2504.14603v1) ("The Desktop
AgentOS", MIT-licensed, github.com/microsoft/UFO) is the most directly
comparable open-source project to Ghost Mode: a multi-agent Windows desktop
controller that **fuses Windows UI Automation (UIA) with vision-based
parsing only as a fallback**, and — the detail most relevant here — prefers
**native app APIs/COM calls over simulated clicks whenever one exists**
("exporting a spreadsheet or formatting text are reduced from a multi-step
GUI dance to a single atomic function call"), batching multiple predicted
steps into one LLM call (they report up to 51% fewer LLM queries this way).
This independently confirms NEXUS's existing tier order (registry launch →
UIA → vision-last) is the right one; the improvement UFO² has that NEXUS
doesn't yet is **native-API-over-simulated-input** for the apps NEXUS
already special-cases (file delete, WhatsApp, browser).

### 1.4 Local vision grounding exists and is already scoped in this repo's own prior research

`02-costs-and-limits.md` (this folder) already identified
[OmniParser](https://github.com/microsoft/OmniParser) (Microsoft, MIT,
arXiv:2408.00203) as the free/local alternative to the cloud Groq/Gemini
vision calls in `vision.rs`, and deliberately deferred it as "last resort,
measure don't assume." Current state (verified in this pass):
**OmniParser V2** (Feb 2025 release, MIT, runs via Docker or
[locally](https://www.analyticsvidhya.com/blog/2025/02/run-omniparser-v2-locally/))
cut latency 60% vs V1 using a fine-tuned YOLOv8 detector + Florence-2
captioner — fully local, no API key, ~1-3GB RAM resident while active,
free. Two newer open-weight alternatives that are smaller and may be a
better fit for a CPU laptop than OmniParser's two-stage pipeline:
- [ShowUI](https://huggingface.co/showlab/ShowUI-2B) — 2B params, 75%
  zero-shot ScreenSpot grounding accuracy, **beats CogAgent (18B) and
  SeeClick (9.6B)** despite being far smaller.
- [OS-Atlas-Base-4B](https://huggingface.co/OS-Copilot/OS-Atlas-Base-4B) —
  cross-platform (Windows included), 13M-element training corpus, the
  7B variant scores 82.47% on ScreenSpot.

Groq's vision models are confirmed dead in this codebase already
(`vision.rs:349`, 404s since 2026-10-01) — so the *current* fallback is
Gemini-only at 500 req/day free. A local 2-4B grounding model removes that
cap and the network hop entirely, at the cost of ~2-4GB RAM while resident
(same lazy-load/kill-idle pattern already used for STT/NLU/brain sidecars).

### 1.5 Reliability: grammar-constrained decoding solves "the model hallucinates a bad action"

[llama.cpp's GBNF grammar system](https://deepwiki.com/ggml-org/llama.cpp/8.1-grammar-and-structured-output)
masks the token sampler so the model is **physically unable** to emit a
token that would break a formal grammar — converting tool/JSON schemas into
GBNF guarantees syntactically and semantically valid output. `llama-cpp-python`
(already NEXUS's exact runtime for the Qwen brain, `server/admin/brain_server.py:146`)
supports this natively. This is the fix for a problem AGENTS.md already
documents happening live: *"Qwen/BERT are overconfident on
out-of-distribution garbage transcripts... 'You feel it, no?' → OpenArchitect
@0.99."* Constrain the brain's grammar to **only** the known action schema
(one of: open_app, close_app, delete_file, search, type_text, focus_app,
click_element, …) and it becomes structurally impossible for it to emit
anything else, regardless of how garbled the input is.

### 1.6 Running the NLU model in-process in Rust (no Python, no sidecar, no network)

NEXUS already proves this works: the wake-word model runs in-process via
`tract-onnx` (`Cargo.toml:51`), not a Python sidecar — that's exactly why
wake-word detection is instant. The NLU model (BERT-Mini, 18MB) is the same
shape of model and could be loaded the same way. Measured numbers for this
exact migration pattern (Python→Rust ONNX in-process):
cold-start **3-5s → 200-500ms**, memory **~2GB → ~100MB**, and BERT
throughput on CPU **~100/s (Python) → ~400/s (Rust ONNX Runtime)**. The
practical implication for NEXUS: classification that currently requires a
lazy-spawned Python process + an HTTP round-trip to :39218 becomes a
plain in-process function call, on the order of **single-digit
milliseconds**, with **zero cold-start wait on first use after boot.**
(`ort` — the actively-maintained Microsoft ONNX Runtime binding — is the
stronger choice for this over `tract` for transformer models specifically;
`tract` stays ideal for the wake-word CNN it's already running, since it's
pure-Rust with no native ORT dependency to ship.)

---

## 2. Where NEXUS already matches the state of the art — don't touch these

- **Tier order** (registry launch → UIA bounds → vision last) matches
  UFO²'s own published doctrine exactly. Keep it.
- **Lazy-spawn, kill-idle sidecars** for STT/NLU/brain already follow the
  "don't pay RAM for a model you're not using" rule the OmniParser/vision
  research in this repo already adopted. Keep the pattern; just change
  *what* runs in-process vs. sidecar (§4).
- **UIA-first element resolution with password-field exclusion** (`mouse.rs`,
  `screen.rs`) is correct and free — this is not where the problem is.
- **Deterministic regex tier 0** (<1ms) is the right foundation; it just
  needs more coverage and a safety net under it instead of a cloud call
  (§4).

## 3. The three concrete gaps causing cloud/quota dependency today

(Full detail already in the previous turn's codebase analysis — summarized
here for plan traceability.)

1. **`route_intent` defaults every `NluResult` (i.e. everything the local
   BERT-Mini model successfully classified) to `Subsystem::WorkerBackend`**
   (`orchestrator.rs:551`) unless a hand-written special case exists earlier.
   Local classification happens, then its answer is discarded for anything
   not on a short hand-maintained allowlist.
2. **`SystemCenter`/`BrowserCenter` (the modules meant to execute
   classified local actions — clicking, focusing, window control) have no
   `execute()` method and are never called from the orchestrator.** They
   validate slots and produce a spoken confirmation line, then nothing
   happens — confirmed dead code outside their own unit tests.
3. **The Qwen brain is gated to `is_admin()` + the `admin-brain` compile
   feature**, and its output isn't grammar-constrained, so even on an admin
   build it's one bad transcript away from hallucinating a wrong action
   (per the documented `OpenArchitect @0.99` incident).

None of these require a new model. All three are wiring/gating fixes.

## 4. Target architecture: fully offline control path

```
transcript
   │
   ▼
Tier 0 — deterministic regex (intent_parser.rs)         <1ms    no model
   │ miss
   ▼
Tier 1 — in-process BERT-Mini via `ort` (NEW: no sidecar) ~2-8ms  local, no network
   │ miss / low confidence
   ▼
Tier 2 — in-process Qwen2.5-0.5B, GBNF-grammar-constrained to the
         known action schema (NEW: un-gated from admin-only,         50-300ms  local, no network
         grammar guarantees valid output even on garbage input)
   │ miss (grammar legitimately returns "not a command")
   ▼
Tier 3 — Cloudflare Worker / cloud LLM                                600ms-2s  ONLY for
         (open-domain knowledge questions, PR/GitHub analysis —               non-control
          things that genuinely need the internet)                           requests
```

Execution side (unchanged tiers, just now actually reachable from Tier 1/2):

```
classified action (open/close/delete/search/type/focus/click)
   │
   ▼
Registry launch (app_registry.rs)         ~0.1-1ms   known apps
   │ miss
   ▼
UIA resolve (screen.rs/mouse.rs)          ~80ms      named elements
   │ miss
   ▼
Local vision grounding (NEW: OmniParser V2 or ShowUI/OS-Atlas-4B,
   lazy-loaded, kill-idle — replaces Groq/Gemini cloud call)    2-5s CPU   no API key,
                                                                           no quota
```

With this shape, **every laptop-control verb the user named (open, close,
delete, search, type) resolves and executes without touching the network
at all**, and the only thing that ever calls the cloud is a genuinely
open-domain question — exactly the split the user asked for.

## 5. Phased implementation plan

| Phase | What | Why this order | Risk |
|---|---|---|---|
| **P0** | Commit current uncommitted Ghost Mode work in checkpointed slices (prerequisite, not optional — see prior turn's finding #5) | Nothing below should be built on an unrecoverable 345-file working tree | None — pure safety |
| **P1** | Port BERT-Mini to in-process `ort` inference; delete the Python NLU sidecar + lazy_nlu spawn/HTTP path | Kills the biggest latency source (process spawn + HTTP) for zero behavior change — purely a transport swap | Low — same model, same ONNX file, new caller |
| **P2** | Add `execute()` to `SubCenter`, wire `SystemCenter`/`BrowserCenter` into real dispatch; flip `route_intent`'s `NluResult` default so local-system-shaped intents (`system_*`, `browser_*`, `whatsapp_*`, `focus_app`) resolve locally instead of defaulting to Worker | Makes the already-classified intents from P1 actually *do* something instead of being thrown away | Medium — touches the main dispatch switch; needs the existing test suite (center.rs, orchestrator.rs) extended per new arm |
| **P3** | Un-gate the Qwen brain from `is_admin()` for the fixed set of control verbs only (open/close/delete/search/type/focus/click); add GBNF grammar constraining output to that exact action schema | Gives the system a second, still-local, reliability net under BERT-Mini for phrasing BERT-Mini misses, without the hallucination risk that kept it admin-only | Medium — grammar authoring + schema versioning; keep admin-only gate for the *conversational/planning* brain use cases (9router-research/02), only lift it for this bounded control schema |
| **P4** | Replace Groq/Gemini vision fallback with a local grounding model (OmniParser V2, or ShowUI-2B/OS-Atlas-4B if a 50-screenshot local eval favors them per the existing doctrine in `02-costs-and-limits.md`) | Closes the last network dependency (vision-on-UIA-miss); optional/lower priority since UIA already covers most real commands | Medium-high — new resident model, RAM budget, needs the lazy-load/kill-idle pattern already used elsewhere |
| **P5** | Extend deterministic Tier-0 coverage using phonetic-alias normalization (already proven elsewhere in this codebase) for the ghost entry/stop/control phrase lists, so fewer phrases even reach Tier 1 | Cheap, compounding win; every phrase caught here never pays any model cost at all | Low |

P1+P2 together are the direct, load-bearing answer to "make it execute in
milliseconds without burning API quota" — they should ship first and
together, since P2 is what makes P1's local classification actually matter.
P3 is the "train/import a model" ask made concrete without training anything
new. P4 is the only phase that touches vision/cursor grounding specifically,
and per this repo's own existing doctrine, only worth doing after measuring
how often UIA alone already satisfies real usage (it already is the common
case for ghost mode today).

## 6. What legitimately keeps needing the internet (and why that's correct, not a compromise)

- Open-domain knowledge questions ("what's the capital of France", general
  chat) — there is no offline model small enough to replace a cloud LLM here
  without a large quality drop; Tier 3 exists for exactly this and nothing
  else once P2 lands.
- GitHub/PR analysis, Google (Gmail/Calendar) — these need the actual
  remote APIs regardless of model location; not a "thinking" dependency,
  a data dependency.
- Nothing about laptop control (open/close/delete/search/type/click/focus)
  needs to be in this list after P1-P3 ship.

## 7. Sources

- Snips Voice Platform (arXiv:1805.10190) — https://arxiv.org/pdf/1805.10190
- Rhasspy — https://github.com/rhasspy/rhasspy
- Octopus v2: On-device language model for super agent (arXiv:2404.01744) — https://arxiv.org/pdf/2404.01744
- UFO²: The Desktop AgentOS (arXiv:2504.14603) — https://arxiv.org/html/2504.14603v1 / https://github.com/microsoft/UFO
- OmniParser / OmniParser V2 (Microsoft, MIT) — https://github.com/microsoft/OmniParser ; https://www.microsoft.com/en-us/research/articles/omniparser-v2-turning-any-llm-into-a-computer-use-agent/ ; https://www.analyticsvidhya.com/blog/2025/02/run-omniparser-v2-locally/
- ShowUI-2B — https://huggingface.co/showlab/ShowUI-2B
- OS-Atlas (arXiv:2410.23218) — https://arxiv.org/html/2410.23218v1 ; https://huggingface.co/OS-Copilot/OS-Atlas-Base-4B
- llama.cpp grammar / GBNF structured output — https://deepwiki.com/ggml-org/llama.cpp/8.1-grammar-and-structured-output
- ONNX Runtime in Rust vs Python latency/memory benchmarks — https://dasroot.net/posts/2026/03/onnx-runtime-rust-ml-inference-optimization/ ; https://www.prismnews.com/hobbies/rust-programming/rust-developers-can-now-run-bert-yolo-and-llama-via-onnx
