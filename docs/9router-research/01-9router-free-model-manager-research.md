# 9Router Free-Model Manager — Research Findings

**Date:** 2026-09-15
**Status:** Research complete — awaiting implementation plan
**Researcher:** Devin (GLM-5.2 High)
**Sources:** 9Router GitHub repo, DeepWiki, 9router.com, free-llm-api-hub, free-ai-apis, Perplexity master prompt, NEXUS codebase analysis

---

## 1. Executive Summary

This document captures the research into using **9Router** as NEXUS's free-model
manager — a system that discovers free-tier AI providers, authenticates with the
user's credentials, manages API keys (adds when free, deletes when exhausted),
and continuously researches new free-tier opportunities.

**Key finding:** 9Router already handles the *routing* layer (fallback chains,
quota tracking, OAuth, multi-account). What it does NOT handle is the
*discovery* and *self-healing* layer — auto-finding new free providers,
auto-deleting exhausted keys, and auto-researching replacements. That layer is
what NEXUS needs to build on top of 9Router.

**Critical constraint:** Automated account creation is against most providers'
Terms of Service. The self-healing system can rotate keys and switch providers,
but cannot create new accounts. The user must authenticate once per provider;
9Router manages the key from there.

---

## 2. 9Router — Verified Capabilities

### 2.1 What 9Router Is

9Router is a **local Next.js gateway** that runs on `localhost:20128`. It
provides a single OpenAI-compatible endpoint (`/v1/*`) and routes traffic
across multiple upstream providers with translation, fallback, token refresh,
and usage tracking.

- **Repo:** https://github.com/decolua/9router
- **License:** MIT
- **Install:** `npm install -g 9router`
- **Dashboard:** `http://localhost:20128/dashboard`
- **API:** `http://localhost:20128/v1`
- **Model discovery:** `GET http://localhost:20128/v1/models`

### 2.2 Verified Feature Matrix

| Feature | Status | Source |
|---------|--------|--------|
| 60+ providers, 100+ models | Confirmed | README, DeepWiki |
| 3-tier auto-fallback (Subscription -> Cheap -> Free) | Confirmed | README |
| Quota tracking + real-time usage + reset countdown | Confirmed | README, docs |
| Auto token refresh (OAuth lifecycle) | Confirmed | DeepWiki 5.4 |
| Multi-account per provider, round-robin | Confirmed | DeepWiki 5.5 |
| Credential validation (probes `/models`, checks 401/403) | Confirmed | DeepWiki 5.1 |
| OAuth flows (authorize, exchange, device-code, poll, import) | Confirmed | DeepWiki 9.7 |
| Provider categories: `free`, `freeTier`, `oauth`, `apikey`, `webCookie` | Confirmed | providers.js |
| Model discovery (`GET /v1/models` per provider) | Confirmed | API routes |
| Combos (chain providers into one virtual provider) | Confirmed | README |
| Format translation (OpenAI <-> Claude <-> Gemini) | Confirmed | ARCHITECTURE.md |
| RTK token saver (compress tool_result, save 20-40% tokens) | Confirmed | README |
| Cloud sync (optional multi-device state) | Confirmed | ARCHITECTURE.md |
| MITM proxy (intercept IDE traffic) | Confirmed | README |

### 2.3 Free Providers in 9Router (Verified)

| Provider | Category | Free Tier | Auth |
|----------|----------|-----------|------|
| Kiro AI | freeTier | 50 credits/month (Claude 4.5 + GLM-5 + MiniMax) | OAuth |
| OpenCode Free | free | No auth, auto-fetch models (list varies) | None |
| Vertex AI | freeTier | $300 GCP credits (Gemini 3 Pro + GLM-5 + DeepSeek) | Service account |

**Note:** iFlow, Qwen Code, and Gemini CLI free tiers were discontinued in 2026.
Kiro moved to a paid model in Sep 2025 — free tier now capped at 50 credits/month.

### 2.4 What 9Router Does NOT Do (The Gap)

| Missing capability | Impact |
|--------------------|--------|
| Auto-discover new free providers from the internet | No self-healing discovery |
| Auto-delete exhausted keys and research replacements | No key lifecycle management |
| Auto-create accounts | Against ToS — user must authenticate manually |
| Classify providers as permanent-free vs trial-credit vs paid | No trust/classification system |
| Integrate with a "brain" (Qwen) for provider selection | No intelligent routing |
| Capability-based routing (coding vs reasoning vs research) | Only tier-based fallback |
| Privacy routing (sensitive data -> local only) | No data-sensitivity awareness |
| Provider health monitoring + quarantine | No automatic disabling of broken providers |
| Audit logging of provider changes | No change trail |

---

## 3. Free-Tier Provider Landscape (Verified, September 2026)

### 3.1 Permanent Free Tiers (No Card, Reset Daily/Monthly)

These are the **reliable backbone** — keys stay valid, limits reset on a
schedule. When exhausted, you wait for reset and rotate to another provider.

| Provider | Limit | Card? | Auth | Reset |
|----------|-------|-------|------|-------|
| Groq | 30 RPM, 14.4K TPM, ~14,400 RPD | No | API key | Daily (00:00 UTC) |
| Google Gemini | 15 RPM, 1,500 RPD, 1M TPD | No | API key | Daily |
| Cerebras | 30 RPM, 1M tokens/day | No | API key | Daily |
| Cloudflare Workers AI | 10K neurons/day | No | API key | Daily (00:00 UTC) |
| Mistral | 1B tokens/month | No (region-gated) | API key | Monthly |
| OpenRouter | 200 RPD (free models, 22+ available) | No | API key | Daily |
| GitHub Models | Free for GitHub users (GPT, Llama, Mistral) | No | OAuth | Per-session |
| NVIDIA NIM | 40 RPM, no daily token cap | Required | API key | Per-minute |
| Cohere | 1,000 calls/month (trial, not production) | No | API key | Monthly |

### 3.2 Trial Credits (One-Time, Exhaust Permanently)

These give a burst of free usage, then require payment. Great for testing,
bad for permanent infrastructure.

| Provider | Credit | Card? | Notes |
|----------|--------|-------|-------|
| Together AI | $5 free | No | One-time |
| Fireworks AI | $1 free | No | One-time, 10 RPM without card |
| SambaNova | Free tier | No | 100K tokens/day |
| Hyperbolic | Trial credit | No | One-time |
| Nebius | Trial credit | No | One-time |
| Novita AI | Metered access | No | Dynamic limits |
| DeepInfra | Trial credit | No | One-time |
| Hugging Face | $0.10/month | No | Enough to sample, not run a backend |

### 3.3 Critical Insight: Key Pooling Multiplies Throughput

Rate limits are **per-API-key, not per-account**. Multiple keys from the same
provider multiply effective throughput. This is the core mechanism for
self-healing:

- 3 Gemini keys = 4,500 RPD instead of 1,500
- 5 Groq keys = 72,000 RPD instead of 14,400

Existing projects that do this:
- **keymux** — OpenAI SDK wrapper, smart scheduling, avoids 429s proactively
- **FreeFlow LLM** — chains Groq + Gemini, auto-fallback on 429, multi-key rotation
- **KAME** — Key-Aware Management Engine, learns from every 429, respects retry-delay
- **QuotaSwitch** — multi-provider key vault, auto-rotation, config injection

### 3.4 Provider Classification Labels

For the NEXUS management layer, providers should be classified:

| Label | Meaning | Example |
|-------|---------|---------|
| `LOCAL_FREE` | Runs on user's machine, no API cost | Ollama, llama.cpp, whisper.cpp |
| `PERMANENT_FREE` | Free tier resets daily/monthly, key stays valid | Groq, Gemini, Cerebras |
| `FREE_TIER_LIMITED` | Free quota is finite or rate-limited | OpenRouter free models |
| `FREE_TRIAL` | Credits expire, one-time | Together AI $5, Fireworks $1 |
| `PAID` | Payment required | OpenAI, Anthropic |
| `UNKNOWN` | Pricing unclear, cannot verify | Random GitHub endpoints |
| `DISABLED` | Quarantined, broken or unsafe | Failed health check |

---

## 4. NEXUS Current State — Gap Analysis

### 4.1 What NEXUS Already Has

| Component | Location | Status |
|-----------|----------|--------|
| LLM fallback chains (hardcoded) | `server/worker/src/models.ts` | 4 chains: analysis, deep, summary, search |
| LLM cascade (Gemini -> Groq -> Cloudflare) | `server/worker/src/external_llm.ts` | Working, free providers only |
| Per-user quota tracking | `server/worker/src/quota.ts` | D1 `usage_log`, daily limits |
| Global neuron budget | `server/worker/src/quota.ts` | Warn at 8K, hard reject at 9.5K |
| Health endpoint | Worker `GET /health` | Minimal liveness check |
| Free providers in use | Gemini, Groq, Cloudflare AI | All free-tier, no paid |
| API key storage | `settings.json` (plaintext) | Groq + Gemini keys |
| Brain server (Qwen 0.5B) | Port 39219, admin-only | Intent classification only |
| Brain monitor | `brain_monitor.rs` | Tracks intent success/failure |

### 4.2 What NEXUS Is Missing (vs. the Perplexity Plan)

| Perplexity component | NEXUS status | Priority |
|---------------------|---------------|----------|
| 9Router integration | None | High |
| Dynamic provider discovery | None | High |
| Provider classification (FREE/PAID/UNKNOWN) | None | High |
| Budget modes (zero_cost / free_first) | None | Medium |
| Capability-based routing (coding vs reasoning) | None | Medium |
| Local LLM integration (Ollama) | None in LLM path | High |
| OS keychain credential storage | None (plaintext) | High |
| Provider health checks (6 levels) | None | Medium |
| Capability scoring / benchmarking | None | Low |
| Privacy routing (PUBLIC/SENSITIVE) | None | Medium |
| Provider quarantine | None | Medium |
| Automatic 9Router configuration | None | High |
| Local model installer | None | Low |
| Audit logging | None | Low |

### 4.3 Architecture Decision: 9Router's Role

Based on the user's clarification:

> "9Router's only purpose is to set and find and research all API keys which
> are temporary free or permanent free. If the limit has been exhausted it
> will delete that API key and do keen research for any platform that gives
> update of free tier. It uses my credential to authenticate and login. It
> has nothing to do with the Worker. It is a manager that has all the
> control of every feature, credential, authentication, and also the brain."

**9Router is the credential + API-key manager and researcher.** It is
separate from the Cloudflare Worker. The Worker stays as-is. 9Router
manages:
1. All provider credentials (OAuth + API keys)
2. Free-tier discovery and research
3. Key lifecycle (add -> monitor -> delete when exhausted -> find replacement)
4. The brain (Qwen) for intelligent provider selection
5. Eventually: routing coding requests through free providers

The Worker continues to handle:
- PR analysis, GitHub operations, research, general Q&A
- Its own hardcoded fallback chains (Gemini -> Groq -> Cloudflare)
- Its own quota tracking (D1 `usage_log`)

**Future integration:** The Worker could optionally query 9Router's
`/v1/models` endpoint to discover which providers are currently healthy,
but this is a Phase 2 concern.

---

## 5. The Self-Healing Key Lifecycle

### 5.1 The Flow

```
Discovery Scheduler (every 12 hours)
  |
  v
Scan curated free-provider lists
  |
  v
For each new provider found:
  +-- Classify (PERMANENT_FREE / FREE_TRIAL / UNKNOWN)
  +-- Check if already configured in 9Router
  +-- If not configured -> present to user ("Free tier found at X, should I open it?")
  +-- User authenticates -> key added to 9Router
  +-- Health check -> validate key works
  |
  v
For each configured provider:
  +-- Monitor quota (9Router tracks this)
  +-- On 429/quota-exhausted:
  |   +-- If PERMANENT_FREE -> mark "waiting for reset", rotate to next provider
  |   +-- If FREE_TRIAL -> delete key, mark provider exhausted, research replacement
  +-- On repeated failures -> quarantine provider
  |
  v
Update fallback chain (9Router combos)
  |
  v
Write audit record
```

### 5.2 Key Rotation Strategy

```
Provider A (Groq key 1) -> 429
  |
  v
Rotate to Provider A (Groq key 2) -> 429
  |
  v
Rotate to Provider A (Groq key 3) -> success
  |
  v
If all keys exhausted -> switch to Provider B (Gemini)
  |
  v
If all Gemini keys exhausted -> switch to Provider C (Cerebras)
  |
  v
If all cloud providers exhausted -> switch to local model (Ollama)
  |
  v
If no local model -> tell user clearly
```

### 5.3 Discovery Sources

| Source | Trust | Auto-enable |
|--------|-------|-------------|
| 9Router's own provider registry | High | Yes |
| free-llm-api-hub (github.com/pacocartones) | Medium | No (review) |
| free-ai-apis (github.com/OuterSpacee) | Medium | No (review) |
| Provider official docs | High | Yes |
| Random GitHub repos | Low | No (sandbox) |
| Social media links | Very low | No |

---

## 6. Honest Constraints

### 6.1 What Can Be Guaranteed

- The software scans configured sources
- The software validates provider metadata
- The software tests credentials before use
- The software removes invalid configurations
- The software prevents routing to providers outside the approved budget
- The software maintains a working fallback plan
- The software never silently spends money (with strict budget mode)

### 6.2 What Cannot Be Guaranteed

- Third-party providers remain free forever
- Third-party providers remain online
- Third-party providers keep the same quota
- Third-party providers keep the same model
- Third-party providers permit automation
- Perfect model intelligence

### 6.3 The Closest Practical Solution

**Local-first inference + automatic free-provider discovery + 9Router
configuration automation + strict budget enforcement + capability testing +
fallback combos + provider quarantine + continuous health monitoring.**

---

## 7. Recommended Provider Stack for NEXUS

### 7.1 Tier 1: Local (Permanent, Offline-Capable)

| Component | Tool | RAM | Why |
|-----------|------|-----|-----|
| LLM | Ollama (qwen2.5:3b or qwen3:4b) | 2-4 GB | Best small model for tool use |
| STT | faster-whisper / Moonshine | 128-340 MB | Already integrated |
| TTS | Kokoro / Piper | 350 MB | Already integrated |
| NLU | BERT-Mini ONNX | 18 MB | Already integrated |
| Brain | Qwen 0.5B/1.5B GGUF | 398 MB - 1 GB | Already integrated (0.5B) |

### 7.2 Tier 2: Permanent Free Cloud (Reset Daily)

| Provider | Use Case | Limit |
|----------|----------|-------|
| Groq | Fast inference, coding | 14,400 RPD |
| Gemini | Long context, reasoning | 1,500 RPD, 1M TPD |
| Cerebras | Fastest token throughput | 1M tokens/day |
| Cloudflare AI | Edge inference | 10K neurons/day |

### 7.3 Tier 3: Free-Tier Aggregators

| Provider | Use Case | Limit |
|----------|----------|-------|
| OpenRouter | 22+ free models, one endpoint | 200 RPD |
| GitHub Models | GPT-style models free | Per-session |

### 7.4 Tier 4: Trial Credits (Use Once)

| Provider | Credit | When to Use |
|----------|--------|-------------|
| Together AI | $5 | Burst testing |
| Fireworks AI | $1 | Quick experiments |

---

## 8. Next Steps

This document is the research foundation. The next documents cover:

1. **Brain research** — which low-RAM model can actually "understand and think"
   (not just pattern-match) for the conversational agent flow
2. **Conversational flow design** — how the assistant interacts with the user
   about free tiers, keys, reminders, and confirmations

See:
- [02-brain-research-low-ram-thinking-model.md](./02-brain-research-low-ram-thinking-model.md)
- [03-conversational-agent-flow-design.md](./03-conversational-agent-flow-design.md)
