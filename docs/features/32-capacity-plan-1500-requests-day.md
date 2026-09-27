# NEXUS — 1,500 Requests/Day Capacity Plan (10 Users × 150 Each)

## Requirement

- **10 users**, each allowed **150 requests/day**
- Per user: 50 PR/repo analysis + 50 research + 50 architecture analysis
- **Total: 1,500 requests/day**
- Must work without issues, no downtime, minimal cost

---

## Current Architecture — 3 Request Types

### 1. PR/Repo Analysis (50/user/day = 500/day total)

**Current path:** Cloudflare Workers AI
- Model: GLM-4.7-flash (default) or GLM-5.3-flash (deep/large PR)
- Input: ~10K-500K tokens (PR diff + file context)
- Output: ~2,500-3,000 tokens (structured review)
- max_tokens: 3,000 (normal), 2,500 (deep)

**Neuron cost per call (GLM-4.7-flash):**
| Scenario | Input tokens | Output tokens | Neurons/call |
|----------|-------------|--------------|-------------|
| Small PR | 10,000 | 2,500 | 55 + 91 = **146** |
| Medium PR | 50,000 | 3,000 | 275 + 109 = **384** |
| Large PR | 200,000 | 3,000 | 1,100 + 109 = **1,209** |
| Huge PR (truncated) | 520,000 | 3,000 | 2,860 + 109 = **2,969** |

**500 PR analyses on Cloudflare:** ~146-384 neurons avg × 500 = **73,000-192,000 neurons**
**Free tier (10,000/day):** WAY OVER. Would cost ~$0.80-$2.11/day on Paid plan.

### 2. Research (50/user/day = 500/day total)

**Current path:** Gemini → Groq → Cloudflare cascade
- Primary: Gemini Flash Lite (external, free, 1,500 req/day)
- Fallback: Groq Qwen 27B (external, free, 14,400 req/day)
- Last resort: Cloudflare Llama 3.2 3B (~14 neurons/call)

**500 research calls:**
- If Gemini handles all: **0 Cloudflare neurons** (free external API)
- If 10% fall to Cloudflare: 50 × 14 = **700 neurons**

### 3. Architecture Mapper (50/user/day = 500/day total)

**Current path:** Cloudflare Workers AI (Mistral Small 3.1 24B)
- Phase 1 Enrichment: ~1,000 input + ~300 output tokens
- Impact Narration: ~500 input + ~150 output tokens

**Neuron cost per call:**
| Model | Phase 1 Enrichment | Impact Narration | Total/analysis |
|-------|-------------------|-----------------|---------------|
| Mistral Small 3.1 24B (current) | 47 neurons | 23 neurons | **70 neurons** |
| GLM-4.7-flash (recommended) | 16 neurons | 8 neurons | **24 neurons** |

**500 architecture analyses:**
- Mistral Small: 500 × 70 = **35,000 neurons** (OVER free tier)
- GLM-4.7-flash: 500 × 24 = **12,000 neurons** (slightly OVER free tier)
- GLM-4.7-flash (no impact narration): 500 × 16 = **8,000 neurons** (within free tier)

---

## The Problem

| Request type | 500 calls neurons | Fits 10K free? |
|-------------|-------------------|---------------|
| PR Analysis | 73,000-192,000 | NO (7-19x over) |
| Research | 0-700 | YES |
| Architecture (Mistral) | 35,000 | NO (3.5x over) |
| Architecture (GLM-4.7) | 8,000-12,000 | BARELY |
| **Total** | **108,000-227,000** | **NO** |

**Cloudflare free tier (10,000 neurons/day) cannot handle 1,500 requests/day.**
PR analysis alone is 7-19x over the free limit.

---

## The Solution: Multi-Provider Routing

### Strategy: Route each request type to the best FREE provider

```
PR Analysis (500/day)    → Gemini (1,500/day free) + Groq fallback
Research (500/day)       → Gemini/Groq cascade (already done)
Architecture (500/day)   → Cloudflare GLM-4.7-flash (10K free neurons)
```

### Provider capacity

| Provider | Free limit | Capacity for NEXUS |
|----------|-----------|-------------------|
| Gemini Flash Lite | 1,500 req/day | 1,000 PR + research calls |
| Groq Llama 3.3 70B | 1,000 req/day (70B) | Fallback for PR analysis |
| Groq Llama 3.1 8B | 14,400 req/day | Fallback for everything |
| Cloudflare GLM-4.7-flash | 10,000 neurons/day | ~625 architecture calls |
| Cerebras | 1M tokens/day | Additional fallback |
| Mistral Experiment | 1B tokens/month | Additional fallback |

### Routing plan

#### PR/Repo Analysis → Gemini + Groq (0 Cloudflare neurons)

| Priority | Provider | Model | Free capacity | Why |
|----------|----------|-------|--------------|-----|
| 1st | Gemini | gemini-flash-lite-latest | 1,500/day | 1M context, handles large PRs |
| 2nd | Groq | llama-3.3-70b-versatile | 1,000/day | 128K context, best quality |
| 3rd | Groq | llama-3.1-8b-instant | 14,400/day | Fast, smaller context |
| 4th | Cloudflare | GLM-4.7-flash | 10K neurons | Last resort (costs neurons) |

**500 PR analyses:**
- Gemini handles ~400 (80%): **0 neurons**
- Groq handles ~80 (16%): **0 neurons**
- Groq 8B handles ~15 (3%): **0 neurons**
- Cloudflare handles ~5 (1%): ~730 neurons
- **Total Cloudflare cost: ~730 neurons**

#### Research → Gemini + Groq (already done, 0 Cloudflare neurons)

Already routed through `synthesizeWithCascade()` in `external_llm.ts`:
- Gemini primary (1,500/day)
- Groq fallback (14,400/day)
- Cloudflare last resort

**500 research calls: ~0-700 Cloudflare neurons**

#### Architecture Mapper → Cloudflare GLM-4.7-flash

| Priority | Provider | Model | Cost | Why |
|----------|----------|-------|------|-----|
| 1st | Cloudflare | GLM-4.7-flash | 16 neurons/call | Best code understanding, cheapest |
| 2nd | Cloudflare | Llama 3.2 3B | 14 neurons/call | Fallback |
| 3rd | Gemini | gemini-flash-lite | 0 neurons | External free fallback |

**500 architecture analyses:**
- Phase 1 Enrichment only (no impact narration per analysis): 500 × 16 = **8,000 neurons**
- With 1 impact narration per analysis: 500 × 24 = **12,000 neurons** (over by 2,000)
- With Gemini fallback for overflow: 8,000 Cloudflare + Gemini handles rest = **8,000 neurons**

---

## Final Capacity Calculation

| Request type | Volume | Provider | Cloudflare neurons | External calls |
|-------------|--------|----------|-------------------|---------------|
| PR Analysis | 500/day | Gemini + Groq | ~730 | 495 Gemini/Groq |
| Research | 500/day | Gemini + Groq | ~700 | 495 Gemini/Groq |
| Architecture | 500/day | Cloudflare GLM-4.7 | ~8,000 | 0 |
| **Total** | **1,500/day** | | **~9,430** | **990 external** |

**Total Cloudflare neurons: ~9,430 / 10,000 free daily allocation**
**Status: FITS within free tier with ~570 neurons to spare**

### External API usage

| Provider | Calls/day | Free limit | Utilization |
|----------|----------|-----------|------------|
| Gemini | ~990 | 1,500/day | 66% |
| Groq 70B | ~80 | 1,000/day | 8% |
| Groq 8B | ~15 | 14,400/day | 0.1% |

### Headroom

| Metric | Value |
|--------|-------|
| Cloudflare neurons used | 9,430 / 10,000 (94%) |
| Cloudflare neurons spare | 570 |
| Gemini requests used | 990 / 1,500 (66%) |
| Gemini requests spare | 510 |
| Groq requests used | 95 / 15,400 (0.6%) |
| Total spare capacity | ~570 neurons + 510 Gemini + 15,305 Groq |

---

## What Needs to Change in the Code

### Change 1: Route PR Analysis through external LLM cascade

**File:** `server/worker/src/index.ts` — `handleGitHubAnalyse()`

Currently calls `env.AI.run(ANALYSIS_MODEL)` directly.
Change to try Gemini first, then Groq, then Cloudflare:

```
1. Build the analysis prompt (same as now)
2. Try Gemini (1M context, handles large PRs)
3. If Gemini fails → try Groq Llama 3.3 70B (128K context)
4. If Groq fails → Cloudflare GLM-4.7-flash (131K context, costs neurons)
5. If context > 128K and Gemini fails → truncate to 128K for Groq
```

### Change 2: Switch architecture mapper to GLM-4.7-flash

**File:** `server/worker/src/index.ts` — `handlePhase1Enrichment()`

Change `SUMMARY_MODEL` → `ANALYSIS_MODEL` (GLM-4.7-flash).
3x cheaper, better at code tasks.

### Change 3: Add Gemini fallback for architecture enrichment

**File:** `server/worker/src/index.ts` — `handlePhase1Enrichment()`

If Cloudflare GLM-4.7-flash fails or neurons are low, fall back to Gemini.

### Change 4: Update quota limits

**File:** `server/worker/src/quota.ts`

```typescript
export const LIMITS = {
  requests_per_day: 150,        // was 500
  ai_neurons_per_day: 1000,     // was 3000 (most calls go external now)
  deep_calls_per_day: 15,       // was 10
  search_calls_per_day: 50,     // was 100
};
```

---

## Cost Summary

| Plan | Monthly cost | Capacity |
|------|-------------|----------|
| **Free tier only** | **$0** | 1,500 req/day (10 users × 150) |
| With Cloudflare Paid ($5/mo) | $5/mo | 1,500 req/day + paid models + 10K free neurons |
| If all on Cloudflare Paid | ~$24-64/mo | 1,500 req/day (no external APIs) |

**Recommended: $0/month (free tier only)**

The key insight: **PR analysis is the expensive part, but Gemini's free tier (1,500 req/day, 1M context) can handle it at zero cost.** Cloudflare's 10,000 free neurons are reserved for the architecture mapper where GLM-4.7-flash excels at code understanding.

---

## Risk Mitigation

| Risk | Mitigation |
|------|-----------|
| Gemini rate limit hit | Groq fallback (15,400 req/day free) |
| Groq rate limit hit | Cloudflare fallback (10K neurons) |
| Cloudflare neurons exhausted | Gemini handles architecture too |
| All providers down | Cache last-known results in D1 |
| Large PR > 1M tokens | Truncate to 1M (Gemini limit) |
| Gemini API key not set | Fall back to Cloudflare (Paid plan $5/mo) |

### If you want guaranteed uptime without external dependencies:

**Cloudflare Workers Paid plan ($5/month)** gives you:
- 10,000 free neurons/day (same as free)
- Paid models (GLM-5.3-flash, Kimi K2.6)
- $0.011/1,000 neurons after free allocation
- 1,500 req/day entirely on Cloudflare: ~$0.80-$2.11/day = **$24-64/month**

### Best value: Free tier + external APIs = $0/month

| Component | Cost | Capacity |
|-----------|------|----------|
| Cloudflare Free | $0 | 10K neurons/day (architecture) |
| Gemini Free | $0 | 1,500 req/day (PR + research) |
| Groq Free | $0 | 15,400 req/day (fallback) |
| **Total** | **$0/mo** | **1,500 req/day for 10 users** |
