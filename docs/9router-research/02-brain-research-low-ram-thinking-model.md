# Brain Research — Low-RAM Thinking Models for NEXUS

**Date:** 2026-09-15
**Status:** Research complete
**Researcher:** Devin (GLM-5.2 High)
**Question:** What is the best low-RAM model that can actually *understand and think* (not just pattern-match) for NEXUS's conversational agent brain?

---

## 1. The Problem

NEXUS currently has a "brain" — a Qwen2.5-0.5B-Instruct model (398 MB GGUF)
running on port 39219. But it's used only for **intent classification** —
mapping transcripts to one of 52 intent labels. It doesn't "think." It
pattern-matches.

The user's vision requires a brain that can:

1. **Understand** — parse natural language that doesn't match any regex or
   intent label (e.g., "hey nexus, i found a free tier at this site, can you
   add it?")
2. **Think** — reason about what to do next (e.g., "the user said they found
   a free tier, I should ask for the API key, validate it, and add it to
   9Router")
3. **Plan** — break a request into steps (e.g., "first I'll check if this
   provider is already configured, then I'll validate the key, then I'll add
   it to 9Router, then I'll run a health check")
4. **Converse** — multi-turn dialogue with state (e.g., "should I open it?"
   -> user says "not now" -> "ok, I'll remind you later" -> later: "hey,
   remember that free tier I found?")
5. **Remember** — track pending actions, reminders, and context across turns

This is fundamentally different from intent classification. Intent
classification is: `text -> label`. The brain needs to be:
`text + context + state -> action + response + new_state`.

---

## 2. What "Understanding" Means for Small Models

### 2.1 Pattern-Matching vs Understanding

| Approach | How it works | Example | RAM |
|----------|-------------|---------|-----|
| Regex/keyword matching | `if "open" in text: return open_app` | "open youtube" -> open_app | 0 MB |
| BERT-Mini classification | Embedding -> classifier -> label | "analyse pr 5" -> github_analyse | 18 MB |
| Small LLM (0.5B) | Token prediction with instruction following | "the user wants me to find free models, I should search the provider list" | 398 MB |
| Small LLM with thinking (0.6B-4B) | Chain-of-thought reasoning before output | "The user said 'not now'. This means they're not ready. I should save this as a pending reminder and check back later. I'll set a reminder for 2 hours from now." | 500 MB - 2.5 GB |

### 2.2 The Key Insight: Thinking Mode

**Qwen3** (released 2025) introduced a breakthrough: **thinking mode** in
small models. A single model can switch between:
- **Non-thinking mode** — fast, direct responses (like Qwen2.5-Instruct)
- **Thinking mode** — chain-of-thought reasoning before answering (like QwQ-32B)

This means a 0.6B or 4B model can *actually reason* about complex requests,
not just pattern-match. The thinking happens internally (the model generates
a `

Sir, Cerebras is a solid choice — 1 million tokens per day, no credit
card needed, and it's one of the fastest inference providers available.
Let me check if it's already configured in your 9Router.

[calls 9Router API: GET /v1/models]

It's not configured yet. Would you like me to open the Cerebras signup
page so you can get an API key?
```

### 5.2 Non-Thinking Mode Example

User says: "what time is it?"

```
It's 3:42 PM, sir.
```

(Direct response, no thinking block, <100ms)

### 5.3 Mode Switching

The brain automatically selects the mode:
- **Non-thinking** for: simple questions, time, weather, basic commands,
  intent classification
- **Thinking** for: multi-step requests, provider management, planning,
  anything requiring reasoning over state

This can be controlled by the system prompt:
```
For simple questions, respond directly.
For complex requests that require planning or multiple steps, use
thinking mode to reason before responding.
```

---

## 6. Conversational State Management

### 6.1 The Problem

The brain needs to maintain state across turns:
- Pending reminders ("remind me about the Cerebras free tier later")
- Provider status ("I found 3 new free providers, you said no to 2, yes to 1")
- Context ("we were talking about adding Cerebras, the user hasn't responded yet")

### 6.2 The Solution: JSON State File

The brain maintains a state file at
`%APPDATA%/com.nexus.assistant/brain_state.json`:

```json
{
  "pending_reminders": [
    {
      "id": "rem_001",
      "type": "free_tier_reminder",
      "provider": "cerebras",
      "message": "You found a free tier at Cerebras. Want to add it?",
      "created_at": "2026-09-15T14:30:00Z",
      "remind_at": "2026-09-15T16:30:00Z",
      "status": "pending"
    }
  ],
  "conversation_context": {
    "current_topic": "cerebras_setup",
    "last_action": "asked_to_open_signup",
    "waiting_for": "user_response"
  },
  "provider_status": {
    "groq": { "status": "healthy", "keys": 2, "quota_remaining": 12000 },
    "gemini": { "status": "healthy", "keys": 1, "quota_remaining": 800 },
    "cerebras": { "status": "not_configured", "keys": 0 }
  }
}
```

### 6.3 Memory Without LLM Tokens

Research found **EdgeMem** — a 0-LLM-token long-term memory system for edge
devices. It builds local structured indexes from dialogue turns and routes
questions through timeline and graph signals without calling the LLM for
every retrieval. This is perfect for NEXUS's low-RAM constraint:

- Conversational history stored as JSON (no LLM summarization)
- Retrieval is deterministic (timeline + graph, no embedding needed)
- LLM is called only for final generation or evaluation
- Designed for edge constraints and privacy-first deployment

---

## 7. Integration with NEXUS

### 7.1 Current Brain (Port 39219)

The current brain server (`server/admin/brain_server.py`) runs
Qwen2.5-0.5B-Instruct and provides:
- `/classify` — intent classification
- `/generate_phrasings` — for BERT-Mini training
- `/pronunciation_map` — pronunciation learning
- `/health` — liveness check

### 7.2 Proposed: Thinking Brain (Port 39220)

A new brain server running Qwen3-4B-Thinking-2507:

```
Port 39220 (separate from Fast Brain 39219)

Endpoints:
  /think          — process a request with thinking mode
  /converse       — multi-turn dialogue with state
  /plan           — break a request into steps
  /select_provider — choose the best provider for a task
  /state          — get/update conversation state
  /reminders      — list/add/clear pending reminders
  /health         — liveness check
```

### 7.3 Lazy Loading

```
Idle state:
  - Fast Brain (0.5B) loaded: ~500 MB
  - Thinking Brain (4B) NOT loaded: 0 MB
  - Total brain RAM: ~500 MB

Complex request received:
  - Thinking Brain loads: ~2s (cold start)
  - RAM: ~2.8 GB
  - Processes request with thinking mode
  - Stays loaded for 10 min

After 10 min idle:
  - Thinking Brain unloads
  - RAM back to ~500 MB
```

### 7.4 When to Use Which Brain

| Trigger | Brain | Mode |
|---------|-------|------|
| Wake word + command | Fast Brain | Non-thinking |
| Deterministic parser fails | Fast Brain | Non-thinking |
| BERT-Mini fails | Fast Brain | Non-thinking |
| User asks about free models | Thinking Brain | Thinking |
| User says "find me free providers" | Thinking Brain | Thinking |
| User says "should I add this?" | Thinking Brain | Thinking |
| User says "remind me later" | Thinking Brain | Non-thinking |
| Reminder fires | Thinking Brain | Thinking |
| Provider exhausted, need replacement | Thinking Brain | Thinking |
| Simple question ("what time is it?") | Fast Brain | Non-thinking |

---

## 8. Model Download and Storage

### 8.1 Qwen3-4B-Thinking-2507

- **HuggingFace:** `Qwen/Qwen3-4B-Thinking-2507`
- **Format:** GGUF Q4_K_M (recommended for CPU)
- **Size:** ~2.5 GB on disk
- **License:** Apache 2.0
- **Download:** Via Ollama (`ollama pull qwen3:4b-thinking`) or direct GGUF

### 8.2 Storage Path

```
src-tauri/resources/brain/
  qwen3-4b-thinking-2507-q4_k_m.gguf    (~2.5 GB)
```

**Note:** This is large for a bundled resource. Options:
1. Bundle in installer (adds 2.5 GB to installer size)
2. Download on first use (saves installer size, needs internet on first run)
3. Use Ollama if installed (no bundling needed, Ollama manages the model)

**Recommendation:** Option 3 (use Ollama if installed) with Option 2 as
fallback. This keeps the installer small and leverages Ollama's model
management.

---

## 9. Benchmark References

### 9.1 AgentFloor Benchmark (2026)

A 30-task benchmark organized as a six-tier capability ladder. Key finding:
"Small and mid-sized open-weight models are already sufficient for much of
the short-horizon, structured tool use work that dominates real agent
pipelines." The gap appears on long-horizon planning tasks where frontier
models still hold an advantage.

**Design principle:** "Use smaller open-weight models for the broad base
of routine actions, and reserve large frontier models for the narrower
class of tasks that truly demand deeper planning."

This validates NEXUS's two-tier approach: Fast Brain for routine, Thinking
Brain for complex.

### 9.2 Instruction-Followed Function Calling (IFFC)

Research showing that small models achieve better function-calling accuracy
in instruction-following contexts (standard user-assistant interactions)
rather than tool-calling contexts. The IFFC framework delegates function-calling
logic to a dedicated smaller model.

**Relevance:** NEXUS's brain should frame tool calls as instructions, not
as raw function-calling. "I need to check 9Router's model list" is better
than `tool_call: get_models()`.

### 9.3 Local-LLM-Benchmark (CPU-only, production)

Benchmarked 5 models on Intel Xeon E-2224 (no GPU) for agent capability:

| Model | Verdict |
|-------|---------|
| gemma3:1b | Chat-only — no tool support |
| llama3.2:3b | Tool calls work; verbose reasoning hits time budgets |
| **qwen2.5:3b** | **Selected — passed all three tests** |
| gemma4:e4b | Cold-load timeouts — too heavy for 4-core CPU |
| qwen3:14b | Hardware ceiling exceeded |

**Note:** This benchmark predates Qwen3-4B-Thinking. The 4B-Thinking model
should be tested similarly, but its thinking mode + tool use scores suggest
it will outperform the 3B on agent tasks.

---

## 10. Summary

| Question | Answer |
|----------|--------|
| Can a small model actually "think"? | **Yes** — Qwen3's thinking mode enables chain-of-thought reasoning in 0.6B-4B models |
| What's the best low-RAM thinking model? | **Qwen3-4B-Thinking-2507** (2.8 GB RAM, BFCL 71.2, TAU2 53.5) |
| Is it too much RAM? | **No** — lazy-loaded, only ~2.8 GB when actively thinking, 0 at idle |
| Can it do tool use? | **Yes** — BFCL-v3 score 71.2, better than many 7B models |
| Can it do multi-turn dialogue? | **Yes** — TAU2 agent scores confirm multi-step capability |
| How does it integrate with NEXUS? | **Two-tier:** Fast Brain (0.5B, always loaded) + Thinking Brain (4B, lazy) |
| What about memory/state? | **JSON state file + EdgeMem pattern** (0-LLM-token retrieval) |

---

## 11. Next Steps

1. Test Qwen3-4B-Thinking-2507 on NEXUS's actual hardware (CPU-only laptop)
2. Build the Thinking Brain server (port 39220) with thinking/non-thinking
   mode switching
3. Implement conversational state management (JSON state file)
4. Wire the brain into the 9Router management flow (see
   [03-conversational-agent-flow-design.md](./03-conversational-agent-flow-design.md))
5. Benchmark real-world latency for the conversational flows described in
   the user's vision
