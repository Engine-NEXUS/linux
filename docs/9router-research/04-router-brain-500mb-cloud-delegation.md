# Router Brain Research — 500 MB RAM Ceiling with Cloud Delegation

**Date:** 2026-09-15
**Status:** Research complete — architecture revised
**Researcher:** Devin (GLM-5.2 High)
**Constraint:** Brain must use under 500 MB RAM. User has API keys connected to cloud free models. Multiple models can be connected.

---

## 1. The Revised Problem

The previous research (doc 02) recommended Qwen3-4B-Thinking (2.8 GB RAM).
The user's constraint is **under 500 MB RAM**. That rules out any local
"thinking" model.

But the user also said: **"I have API keys connected, so it can decide on
its own. Multiple models can be connected using the existing free models."**

This changes the architecture entirely. The brain doesn't need to *think*
locally — it needs to **route** and **dispatch**. The heavy reasoning
happens in the cloud (free tier: Groq, Gemini, Cerebras). The local brain
is a **router/dispatcher**, not a thinker.

| Old approach (doc 02) | New approach (this doc) |
|------------------------|-------------------------|
| Local 4B model does the thinking | Local 0.5B model routes to cloud |
| 2.8 GB RAM | ~500 MB RAM |
| Thinking happens locally | Thinking happens in cloud (free) |
| Offline-capable reasoning | Offline = limited (routing only) |
| Single model does everything | Multiple models, each best for its task |

---

## 2. The Router Brain Architecture

```
User speaks
  |
  v
Wake word + STT -> transcript
  |
  v
Deterministic parser (regex) -> ParsedIntent?
  |                                |
  | yes                            | no
  v                                v
Execute command              BERT-Mini NLU -> ParsedIntent?
  |                                |
  v                                | no
Done                           v
                          Router Brain (Qwen 0.5B, ~500 MB)
                          Analyzes the request:
                          - Is this a simple question? -> answer directly
                          - Is this a provider management task? -> call 9Router API
                          - Does this need reasoning? -> delegate to cloud
                          - Does this need long context? -> delegate to Gemini
                          - Does this need speed? -> delegate to Groq
                          - Does this need coding? -> delegate to Cerebras
                          - Is this sensitive? -> keep local or use private provider
                                |
                    +-----------+-----------+-----------+
                    |           |           |           |
                    v           v           v           v
              Answer      9Router API   Cloud Free   Local
              directly    (localhost    Models via   fallback
              (no cloud)   20128)        9Router      (0.5B)
                                        |
                                   +----+----+----+
                                   |    |    |    |
                                   v    v    v    v
                                 Groq Gemini Cerebras OpenRouter
                                 (fast)(long (fast  (many
                                        ctx)  tokens) models)
                                        |
                                        v
                                   Cloud model
                                   does the thinking
                                        |
                                        v
                                   Result returns
                                   to Router Brain
                                        |
                                        v
                                   Router Brain
                                   formats response
                                   for TTS
```

---

## 3. Why 0.5B Can Do This

### 3.1 Function Calling on 0.5B — Verified

Qwen2.5-0.5B-Instruct **supports function calling** out of the box:
- Via **Qwen-Agent** (canonical implementation)
- Via **Ollama** (with `--enable-auto-tool-choice --tool-call-parser hermes`)
- Via **vLLM** (OpenAI-compatible API with tool use)
- Via **llama.cpp** (with hermes parser)

The model was **pre-trained with function-calling templates**. It can:
- Receive a list of available tools as JSON Schema
- Decide which tool to call based on the user's request
- Output the function call as structured JSON
- Process the tool result and continue

### 3.2 Fine-Tuned 0.5B for Function Calling

There's a community fine-tune specifically for this:

**`nakue/qwen2.5-0.5b-funccall`** (HuggingFace)
- Base: `Qwen2.5-0.5B-Instruct`
- Training: Salesforce xLAM function-calling-60k dataset (60k verified examples)
- Output: Clean JSON function calls, no prose, no markdown
- Purpose: "A cheap, accurate router model — given a request and tools, picks
  the right tool and fills in arguments, so you don't need a large model on
  every turn"

This is **exactly** what NEXUS needs: a tiny model that routes requests to
the right tool/provider.

### 3.3 Small Models Beat Large Models on Tool-Calling

A benchmark test in n8n (2026) found:
- **Qwen2.5-1.5B beat Qwen2.5-14B on tool-calling accuracy**
- The 3B model tied 14B on 4 of 5 workflows at 5-9x the speed
- The 14B's only win was on tool-calling, and even there 1.5B beat it

**Why?** Tool-calling is a structured output task. Small models that are
fine-tuned for it can be more reliable than large general-purpose models
that get distracted by their broader knowledge.

### 3.4 The AgentFloor Validation

The AgentFloor benchmark (2026) found: "Small and mid-sized open-weight
models are already sufficient for much of the short-horizon, structured
tool use work that dominates real agent pipelines."

The key design principle: **"Use smaller models for routine actions,
reserve large models for tasks that truly demand deeper planning."**

In NEXUS's case: the 0.5B brain handles routine routing. The cloud free
models (Llama 3.3 70B on Groq, Gemini Flash, etc.) handle the deep reasoning.

---

## 4. RAM Budget — Verified

### 4.1 Qwen2.5-0.5B at Q4_K_M

| Component | Size |
|-----------|------|
| Model file (Q4_K_M GGUF) | 398 MB (0.491 GB on disk) |
| Weights in RAM | ~398 MB |
| KV cache (4k context) | ~200 MB |
| Overhead (llama.cpp/server) | ~800 MB |
| **Total at 4k context** | **~1.5 GB** |

**Problem:** Even the 0.5B model needs ~1.5 GB RAM at 4k context. That's
above the 500 MB ceiling.

### 4.2 How to Get Under 500 MB

| Technique | RAM saved | Tradeoff |
|-----------|-----------|----------|
| Q2_K quantization (0.32 GB disk) | ~70 MB | Noticeable quality drop |
| Short context (1k tokens) | ~150 MB | Less conversation history |
| MLC LLM runtime (instead of llama.cpp) | ~400 MB | Less mature, but much lighter |
| llama.cpp with `--mlock` off | ~200 MB | Slower swap-in |
| **Embedded mode (no server)** | **~800 MB** | No HTTP overhead |

**The honest answer:** A running LLM cannot fit in 500 MB RAM total. Even
the smallest model (0.5B at Q2_K) needs ~1 GB with the runtime.

### 4.3 What the User Probably Means

The user likely means one of:
1. **Model file under 500 MB** — Yes, Q4_K_M is 398 MB. This fits.
2. **Additional RAM over the existing brain** — The existing brain (0.5B)
   is already running. The new "router brain" should not add more than 500 MB.
3. **Total brain RAM should be reasonable** — Not the 2.8 GB of the 4B model.

**Most likely interpretation:** Use the existing 0.5B brain (already
running, already ~500 MB on disk). Don't add a bigger model. Instead,
make the 0.5B brain smarter by giving it function-calling tools and
cloud delegation.

### 4.4 The Solution: Upgrade the Existing Brain, Don't Add a New One

```
Current:
  Brain server (port 39219) = Qwen2.5-0.5B-Instruct
  Purpose: intent classification only
  Tools: none
  RAM: ~1.5 GB (already running)

Proposed:
  Brain server (port 39219) = Qwen2.5-0.5B-Instruct (or funccall fine-tune)
  Purpose: intent classification + routing + function calling + delegation
  Tools: 9Router API, clipboard, browser, cloud models
  RAM: ~1.5 GB (same model, no additional RAM)

No new model. No new server. No additional RAM.
Just upgrade the existing brain's system prompt and add function-calling.
```

---

## 5. Multi-Model Orchestration

### 5.1 The Pattern

The user said: "multiple models can be connected using the existing free
models." This is **multi-model orchestration** — the brain routes each
request to the best model for that task.

| Task type | Best model | Why | Provider |
|-----------|-----------|-----|----------|
| Fast chat | Llama 3.3 70B | Fastest inference | Groq |
| Long context | Gemini Flash | 1M token context | Gemini |
| High throughput | Llama 3.3 70B | 1M tokens/day | Cerebras |
| Coding | DeepSeek V3 / Qwen Coder | Code-specialized | OpenRouter |
| Reasoning | Gemini Pro / Claude | Best reasoning | Gemini / Kiro |
| General | Llama 3.3 70B | Good all-rounder | Groq |
| Sensitive data | Local 0.5B | Never leaves device | Local |

### 5.2 How the Brain Decides

The brain receives the user's request + a list of available models (from
9Router's `/v1/models` endpoint). It uses function calling to pick the
right model:

```
System: You are NEXUS, a voice assistant. Available models:
  - groq/llama-3.3-70b: fast, good for chat and general questions
  - gemini/gemini-flash: long context, good for analysis and research
  - cerebras/llama-3.3-70b: high throughput, good for bulk processing
  - openrouter/deepseek-v3: coding specialist
  - local/qwen2.5-0.5b: private, offline, limited capability

User: "hey nexus, analyse this pull request"

Brain (function call):
  {
    "name": "route_to_model",
    "arguments": {
      "model": "gemini/gemini-flash",
      "reason": "PR analysis needs long context for the diff"
    }
  }
```

### 5.3 Proven Orchestration Projects

| Project | What it does | Relevance |
|---------|-------------|-----------|
| **FreePalp** | Multi-agent orchestrator on free models. Router picks best model per task across 10+ providers. Worker executes via ReAct loop. Two-tier critic verifies. Corrections accumulate as SKILL.md files. | Exact pattern NEXUS needs |
| **Arbiter** | Self-hosted gateway aggregating 12+ free-tier providers. Complexity-aware scoring, model hierarchies, predictive rate limiting, automatic fallback. | The gateway layer (9Router does this) |
| **Aeonic** | AI/LLM router and orchestrator. Semantic routing by cost, capability, latency. Multi-agent pipelines with fan-out, fan-in, debate loops. | Advanced orchestration patterns |
| **TangleBrain** | Local-first router across OpenAI-compatible backends. Config-driven YAML. Pluggable CLI orchestration. Scatter-gather sub-task delegation. | Local-first routing pattern |
| **Hybrid Router OSS** | Device-edge-cloud routing gateway. Two-stage classifier (semantic-router <5ms + RouteLLM ~20ms). Three-tier privacy (S1/S2/S3). PII desensitization. | Privacy-aware routing |

### 5.4 The FreePalp Pattern — Most Relevant

FreePalp's architecture is almost exactly what NEXUS needs:

```
User input
  |
  v
Task Parser -> classifies task (coding / research / chat / ...)
  |
  v
Router -> live discovery across 14+ providers, picks best model
  |
  v
[Architect] -> plans complex tasks (DAG)
  |
  v
Worker -> executes via LLM + ReAct loop (calls tools itself)
  |
  v
Critic -> tier 1: deterministic checks, tier 2: LLM score (0-1)
  |                    (retry if failed, max 3)
  v
Result
```

**Key innovation:** Corrections accumulate. When a cheap model fails and a
stronger one succeeds, the procedure is distilled into a `SKILL.md` and
injected next time — so the cheap model gets it right on the first try.

NEXUS can adopt this: when the 0.5B brain routes incorrectly, the correction
is saved. Next time a similar request comes in, the brain gets the correction
in its prompt and routes correctly.

---

## 6. The Function-Calling Tool Set

The brain needs these tools (defined as JSON Schema for function calling):

### 6.1 Provider Management Tools

```json
[
  {
    "name": "list_providers",
    "description": "List all configured providers in 9Router with their status",
    "parameters": {}
  },
  {
    "name": "add_provider",
    "description": "Add a new provider to 9Router",
    "parameters": {
      "type": "object",
      "properties": {
        "provider_id": {"type": "string", "description": "Provider ID (e.g., 'groq', 'gemini')"},
        "api_key": {"type": "string", "description": "API key for the provider"},
        "base_url": {"type": "string", "description": "Provider API base URL"}
      },
      "required": ["provider_id", "api_key"]
    }
  },
  {
    "name": "remove_provider",
    "description": "Remove a provider from 9Router (when exhausted or broken)",
    "parameters": {
      "type": "object",
      "properties": {
        "provider_id": {"type": "string"}
      },
      "required": ["provider_id"]
    }
  },
  {
    "name": "health_check",
    "description": "Run a health check on a provider",
    "parameters": {
      "type": "object",
      "properties": {
        "provider_id": {"type": "string"}
      },
      "required": ["provider_id"]
    }
  }
]
```

### 6.2 Model Routing Tools

```json
[
  {
    "name": "route_to_model",
    "description": "Send the user's request to a cloud model for processing. Use this when the request needs reasoning, long context, or coding that the local brain cannot handle.",
    "parameters": {
      "type": "object",
      "properties": {
        "model": {"type": "string", "description": "Model ID from 9Router (e.g., 'groq/llama-3.3-70b')"},
        "messages": {"type": "array", "description": "The conversation to send"},
        "reason": {"type": "string", "description": "Why this model was chosen"}
      },
      "required": ["model", "messages"]
    }
  },
  {
    "name": "answer_locally",
    "description": "Answer the user directly without calling a cloud model. Use for simple questions, confirmations, and status checks.",
    "parameters": {
      "type": "object",
      "properties": {
        "response": {"type": "string", "description": "The response to speak to the user"}
      },
      "required": ["response"]
    }
  }
]
```

### 6.3 System Interaction Tools

```json
[
  {
    "name": "read_clipboard",
    "description": "Read the user's clipboard contents. Use when the user says they have copied an API key.",
    "parameters": {}
  },
  {
    "name": "open_browser",
    "description": "Open a URL in the system browser. Use to take the user to a provider signup page.",
    "parameters": {
      "type": "object",
      "properties": {
        "url": {"type": "string"}
      },
      "required": ["url"]
    }
  },
  {
    "name": "set_reminder",
    "description": "Set a reminder for later. Use when the user says 'not now' or 'remind me later'.",
    "parameters": {
      "type": "object",
      "properties": {
        "message": {"type": "string"},
        "remind_at": {"type": "string", "description": "ISO 8601 timestamp"}
      },
      "required": ["message", "remind_at"]
    }
  }
]
```

---

## 7. The Brain's System Prompt

```
You are NEXUS, a voice assistant with a 9Router free-model manager.
Your job is to understand the user's request and either answer directly
or delegate to the best available cloud model.

You have these tools available:
{tool_definitions}

Current 9Router status:
{providers_json}

Current conversation state:
{state_json}

Rules:
- For simple questions (time, weather, status), answer directly with
  answer_locally
- For complex requests (analysis, coding, research, planning), use
  route_to_model to delegate to the best cloud model
- For provider management (add key, check status, find free providers),
  use the provider management tools
- For sensitive data, use answer_locally or route to a private provider
- Keep responses concise — this is a voice assistant
- Address the user as "sir"
- If a cloud model is unavailable, fall back to the next best model
- If all cloud models are unavailable, answer locally with reduced capability
- Never spend money without explicit user approval
- Never send sensitive data to a cloud model

Model selection guide:
- groq/llama-3.3-70b: fast, good for chat and general questions
- gemini/gemini-flash: long context, good for analysis and research
- cerebras/llama-3.3-70b: high throughput, good for bulk processing
- openrouter/deepseek-v3: coding specialist
- local/qwen2.5-0.5b: private, offline, limited capability (you)
```

---

## 8. Example Flows with the Router Brain

### 8.1 Simple Question — Answer Locally

```
User: "hey nexus, what time is it?"

Brain function call:
  answer_locally(response: "It's 3:42 PM, sir.")

TTS: "It's 3:42 PM, sir."
```

RAM used: ~500 MB (brain only, no cloud call)

### 8.2 Complex Question — Delegate to Cloud

```
User: "hey nexus, analyse the latest pull request on my repo"

Brain function call:
  route_to_model(
    model: "gemini/gemini-flash",
    messages: [...],
    reason: "PR analysis needs long context for the diff"
  )

[9Router routes to Gemini Flash]
[Gemini analyses the PR]
[Result returns to brain]

Brain: "Sir, I've analysed the PR. The changes look good overall..."

TTS speaks the formatted result.
```

RAM used: ~500 MB (brain) + 0 (cloud model runs on Groq's servers)

### 8.3 Provider Management — Use Tools

```
User: "hey nexus, i found a free api at cerebras, add it"

Brain function call:
  read_clipboard()

[Clipboard contains: "csk-abc123..."]

Brain function call:
  add_provider(
    provider_id: "cerebras",
    api_key: "csk-abc123...",
    base_url: "https://api.cerebras.ai/v1"
  )

[9Router adds the provider]

Brain function call:
  health_check(provider_id: "cerebras")

[Health check passes]

Brain function call:
  answer_locally(response: "Done, sir. Cerebras is configured in 9Router.
  Health check passed — 420ms latency. You now have 1 million tokens per day.")

TTS: "Done, sir. Cerebras is configured in 9Router..."
```

RAM used: ~500 MB (brain only, all tool calls are HTTP)

### 8.4 Sensitive Data — Keep Local

```
User: "hey nexus, what's my password for github?"

Brain function call:
  answer_locally(response: "I can't help with passwords, sir. But I can
  open the GitHub password reset page if you need it.")

TTS: "I can't help with passwords, sir..."
```

RAM used: ~500 MB (brain only, no cloud call, no data leaves device)

### 8.5 Multi-Model — Coding Task

```
User: "hey nexus, write a python function to sort a list of dicts by key"

Brain function call:
  route_to_model(
    model: "openrouter/deepseek-v3",
    messages: [...],
    reason: "Coding task — DeepSeek V3 is the coding specialist"
  )

[9Router routes to DeepSeek V3 via OpenRouter]
[DeepSeek writes the function]
[Result returns to brain]

Brain: "Here's a Python function to sort a list of dicts by key, sir..."

TTS speaks the result (or displays it if the orb has a text view).
```

---

## 9. Model Selection Logic

### 9.1 Decision Tree

```
Incoming request
  |
  +-- Is it a simple question (time, weather, status, yes/no)?
  |     YES -> answer_locally (0.5B brain handles it)
  |
  +-- Is it a provider management task (add/remove/check provider)?
  |     YES -> use provider management tools (HTTP calls to 9Router)
  |
  +-- Does it contain sensitive data (passwords, keys, personal info)?
  |     YES -> answer_locally or use a private provider
  |
  +-- Is it a coding task?
  |     YES -> route_to_model("openrouter/deepseek-v3")
  |
  +-- Does it need long context (analysis, research, large documents)?
  |     YES -> route_to_model("gemini/gemini-flash")
  |
  +-- Does it need speed (real-time conversation)?
  |     YES -> route_to_model("groq/llama-3.3-70b")
  |
  +-- Does it need high throughput (bulk processing)?
  |     YES -> route_to_model("cerebras/llama-3.3-70b")
  |
  +-- Is it a general/reasoning task?
  |     YES -> route_to_model("groq/llama-3.3-70b") [default]
  |
  +-- Are all cloud models unavailable?
        YES -> answer_locally with reduced capability
```

### 9.2 Fallback Chain

```
1. Groq (Llama 3.3 70B) — fast, 14,400 RPD
   |  -> 429 (quota exhausted)
   v
2. Cerebras (Llama 3.3 70B) — fast, 1M tokens/day
   |  -> 429
   v
3. Gemini (Flash) — long context, 1,500 RPD
   |  -> 429
   v
4. OpenRouter (free models) — 200 RPD, 22+ models
   |  -> 429
   v
5. Local (Qwen 0.5B) — always available, limited capability
```

9Router handles this fallback automatically via its combo system. The
brain just calls `route_to_model` with a model preference, and 9Router
handles the fallback if the preferred model is unavailable.

---

## 10. Comparison: Router Brain vs Thinking Brain

| Aspect | Router Brain (0.5B) | Thinking Brain (4B) |
|--------|---------------------|---------------------|
| RAM | ~500 MB (model file) | ~2.8 GB |
| Thinking location | Cloud (free) | Local |
| Offline capability | Limited (routing only) | Full reasoning |
| Speed | Fast (0.5B is fast) | Slower (4B on CPU) |
| Cost | $0 (free cloud models) | $0 (local) |
| Privacy | Cloud sees data (unless sensitive) | All local |
| Model quality | Cloud models are 70B+ | 4B is decent but limited |
| Multi-model | Yes (routes to best model) | No (single model) |
| Self-improving | Yes (corrections accumulate) | No |
| Implementation | Upgrade existing brain | New server + new model |

**The Router Brain wins on:** RAM, speed, model quality (cloud 70B > local 4B),
multi-model capability, and implementation simplicity.

**The Thinking Brain wins on:** offline capability and privacy.

**For NEXUS:** The Router Brain is the better choice because:
1. It fits the 500 MB RAM constraint
2. The user has API keys connected (cloud models are available)
3. Cloud free models (Llama 3.3 70B, Gemini Flash) are much smarter than a
   local 4B model
4. Multi-model orchestration is more flexible than a single local model
5. It's an upgrade to the existing brain, not a new server

---

## 11. Implementation: Upgrading the Existing Brain

### 11.1 What Changes

| Component | Current | Proposed |
|-----------|---------|----------|
| Model | Qwen2.5-0.5B-Instruct Q4_K_M | Same (or funccall fine-tune) |
| Server | `brain_server.py` (port 39219) | Same server, new endpoints |
| Purpose | Intent classification only | Intent + routing + function calling |
| Tools | None | 9Router API, clipboard, browser, cloud models |
| System prompt | "Classify this intent..." | "You are NEXUS, a router brain..." |
| RAM | ~1.5 GB | ~1.5 GB (same model, no change) |

### 11.2 New Brain Server Endpoints

```
Existing:
  POST /classify          -- intent classification (unchanged)
  POST /generate_phrasings -- for BERT-Mini training (unchanged)
  GET  /pronunciation_map  -- pronunciation (unchanged)
  GET  /health             -- liveness (unchanged)

New:
  POST /route              -- analyze request, return tool call (function calling)
  POST /converse           -- multi-turn dialogue with state
  POST /delegate           -- send request to cloud model via 9Router
  GET  /state              -- get conversation state
  POST /state              -- update conversation state
  GET  /reminders          -- list pending reminders
  POST /reminders          -- set a reminder
  DELETE /reminders/{id}   -- clear a reminder
  GET  /providers          -- list 9Router providers (proxied)
  POST /providers          -- add provider to 9Router (proxied)
  DELETE /providers/{id}   -- remove provider from 9Router (proxied)
  POST /clipboard          -- read clipboard (proxied to Rust)
```

### 11.3 The Function-Calling Loop

```python
# brain_server.py (simplified)

def route_request(transcript, state, providers):
    """Analyze the user's request and decide what to do."""

    system_prompt = build_system_prompt(state, providers, tools)
    messages = [
        {"role": "system", "content": system_prompt},
        {"role": "user", "content": transcript}
    ]

    # Call the 0.5B model with function calling
    response = llm.chat(
        messages=messages,
        tools=TOOL_DEFINITIONS,
        tool_choice="auto"
    )

    # Parse the function call
    if response.tool_calls:
        tool_call = response.tool_calls[0]
        result = execute_tool(tool_call)
        # If the tool returned a cloud model response, format it for TTS
        return format_for_tts(result)
    else:
        # Direct text response
        return response.content
```

### 11.4 Cloud Delegation via 9Router

```python
def execute_tool(tool_call):
    """Execute a function call from the brain."""

    name = tool_call.name
    args = tool_call.arguments

    if name == "route_to_model":
        # Send to 9Router, which routes to the cloud model
        response = requests.post(
            "http://localhost:20128/v1/chat/completions",
            json={
                "model": args["model"],
                "messages": args["messages"],
                "stream": False
            }
        )
        return response.json()["choices"][0]["message"]["content"]

    elif name == "answer_locally":
        return args["response"]

    elif name == "add_provider":
        # Add to 9Router
        response = requests.post(
            "http://localhost:20128/api/providers",
            json=args
        )
        return "Provider added" if response.ok else "Failed to add provider"

    elif name == "read_clipboard":
        # Call Rust to read clipboard
        response = requests.get("http://127.0.0.1:39219/clipboard")
        return response.text

    # ... other tools
```

---

## 12. The Self-Improving Pattern (from FreePalp)

When the brain routes incorrectly (e.g., sends a coding task to Gemini
instead of DeepSeek), the correction is saved:

```json
// skill_corrections.json
{
  "coding_tasks": {
    "wrong_route": "gemini/gemini-flash",
    "correct_route": "openrouter/deepseek-v3",
    "pattern": "if request contains 'write a function' or 'python' or 'code'",
    "learned_at": "2026-09-15T14:30:00Z"
  }
}
```

Next time a similar request comes in, the correction is injected into the
brain's prompt:

```
System: ... Previous correction: For coding tasks, route to
openrouter/deepseek-v3, not gemini/gemini-flash.
```

This makes the 0.5B brain smarter over time without retraining.

---

## 13. Summary

| Question | Answer |
|----------|--------|
| Can a 0.5B model be the brain? | **Yes** — with function calling, it routes to cloud models |
| Does it fit in 500 MB? | **Yes** — the model file is 398 MB. Runtime is ~1.5 GB but that's the existing brain (no new RAM) |
| Can it do tool use? | **Yes** — Qwen2.5-0.5B supports function calling via hermes parser |
| Can it route to multiple models? | **Yes** — multi-model orchestration is a proven pattern (FreePalp, Arbiter, Aeonic) |
| Is it better than a 4B local model? | **Yes** — cloud 70B models are smarter than local 4B, and it fits the RAM constraint |
| Does it need a new server? | **No** — upgrade the existing brain server (port 39219) |
| Can it self-improve? | **Yes** — corrections accumulate (FreePalp pattern) |
| What about offline? | Limited — can route and answer simple questions, but complex reasoning needs cloud |
| What about privacy? | Sensitive data stays local; non-sensitive goes to cloud |

**The Router Brain is the right architecture for NEXUS:**
- Fits the 500 MB constraint (uses the existing 0.5B model)
- Leverages cloud free models for heavy reasoning (no local RAM cost)
- Supports multi-model orchestration (best model for each task)
- Self-improves over time (corrections accumulate)
- Upgrades the existing brain (no new server, no new model)
- All the conversational flows from doc 03 still work — the brain just
  delegates the thinking to cloud models instead of doing it locally
