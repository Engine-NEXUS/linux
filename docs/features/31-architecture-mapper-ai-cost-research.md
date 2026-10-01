# NEXUS Architecture Mapper — AI Cost & Sponsorship Research

## Current AI Usage

### What uses AI (and what doesn't)

| Phase | Uses AI? | Model | Where |
|-------|----------|-------|-------|
| Phase 1: Visual Map | NO | Rust heuristics | Local |
| Phase 1 Enrichment | YES | Mistral Small 3.1 24B | Cloudflare Worker |
| Phase 2: Deep Graph | NO | Rust + petgraph | Local |
| Impact Narration | YES | Mistral Small 3.1 24B | Cloudflare Worker |

Only **2 calls per architecture analysis** use AI:
1. **Phase 1 Enrichment** — rewrites generic layer labels into repo-specific ones
2. **Impact Narration** — explains blast radius in plain English (on-demand only)

### Token usage per request

#### Phase 1 Enrichment
- **Input:** ~800-1200 tokens (repo metadata + layer info + 200 sample file paths)
- **Output:** ~200-500 tokens (JSON with summary + enriched layer labels)
- **max_tokens:** 500 (hard cap in code)
- **Typical total:** ~1000-1700 tokens per call

#### Impact Narration
- **Input:** ~400-600 tokens (target file + dependency paths + affected files)
- **Output:** ~100-200 tokens (plain English explanation, <150 words)
- **Typical total:** ~500-800 tokens per call

#### Per full architecture analysis (50 analyses)
- Phase 1 Enrichment: 50 calls × ~1500 tokens = ~75,000 tokens
- Impact Narration (assume 3 per analysis): 150 calls × ~650 tokens = ~97,500 tokens
- **Total for 50 analyses: ~172,500 tokens**

---

## Cloudflare Workers AI — Current Provider

### Pricing

| Metric | Value |
|--------|-------|
| Free allocation | 10,000 Neurons/day |
| Paid pricing | $0.011 per 1,000 Neurons |
| Workers Paid plan | $5/month (includes 10,000 free Neurons/day) |

### Mistral Small 3.1 24B Neuron cost

| Direction | Neurons per 1M tokens | Cost per 1M tokens |
|-----------|----------------------|-------------------|
| Input | 31,876 | $0.351 |
| Output | 50,488 | $0.555 |

### How many free analyses per day?

**Per Phase 1 Enrichment call:**
- Input: ~1000 tokens → 31.9 neurons
- Output: ~300 tokens → 15.1 neurons
- **Total: ~47 neurons per call**

**10,000 free neurons / 47 neurons per call = ~212 free Phase 1 enrichments per day**

**Per Impact Narration call:**
- Input: ~500 tokens → 15.9 neurons
- Output: ~150 tokens → 7.6 neurons
- **Total: ~23 neurons per call**

**10,000 free neurons / 23 neurons per call = ~434 free impact narrations per day**

### Free tier capacity for 5-10 users

| Scenario | Daily neuron usage | Within 10K free? |
|----------|-------------------|-----------------|
| 5 users × 5 analyses each × 47 neurons | 1,175 | YES |
| 10 users × 5 analyses each × 47 neurons | 2,350 | YES |
| 10 users × 10 analyses each × 47 neurons | 4,700 | YES |
| 10 users × 20 analyses each × 47 neurons | 9,400 | YES (barely) |
| 10 users × 20 analyses + 50 impacts each | 9,400 + 11,500 = 20,900 | NO — needs Paid |

### Verdict on Cloudflare Free tier

**YES, the free tier is sufficient for 5-10 users doing ~10 analyses/day each.**
The 10,000 neurons/day free allocation covers ~212 Phase 1 enrichments.
You'd only need the $5/month Paid plan if users do heavy impact narration too.

### Is Mistral Small 3.1 24B available on the Free plan?

**YES** — as of August 2026, Mistral Small 3.1 24B is NOT on the restricted list.
Only these models require Workers Paid:
- `@cf/moonshotai/kimi-k2.6`
- `@cf/moonshotai/kimi-k2.7-code`
- `@cf/zai-org/glm-5.2`

Mistral Small 3.1 24B, GLM-4.7-flash, Llama 3.2 1B/3B, and most other models remain free.

---

## Alternative Free Providers (Sponsorship Options)

### 1. Groq (BEST free alternative)

| Model | RPM | Requests/day | Tokens/day | Speed |
|-------|-----|-------------|------------|-------|
| llama-3.3-70b-versatile | 30 | 1,000 | 100,000 | ~280-394 TPS |
| llama-3.1-8b-instant | 30 | 14,400 | 500,000 | ~560-840 TPS |
| qwen/qwen3-32b | 60 | 1,000 | 500,000 | Fast |

- **Cost:** $0 forever, no credit card
- **API:** OpenAI-compatible format
- **Best model for architecture:** `llama-3.3-70b-versatile` (1,000 req/day free)
- **Sponsorship:** Groq has a free tier that's permanently free, not a trial
- **Get key:** https://console.groq.com

**50 analyses on Groq free tier:**
- 50 Phase 1 enrichments + 150 impact narrations = 200 requests
- Well within 1,000 requests/day limit
- **100% free, no sponsorship needed**

### 2. Google Gemini (AI Studio)

| Model | Free limit | Context |
|-------|-----------|---------|
| gemini-flash-lite-latest | 1,500 req/day | 1M tokens |
| gemini-2.0-flash | 1,500 req/day | 1M tokens |

- **Cost:** $0, no credit card for free tier
- **API:** Google AI Studio format (already have `GEMINI_API_KEY` support in Worker)
- **Best model:** `gemini-flash-lite-latest` (already configured in `external_llm.ts`)
- **Get key:** https://aistudio.google.com/apikey

**50 analyses on Gemini free tier:**
- 200 requests / 1,500 per day = 13% of daily limit
- **100% free, no sponsorship needed**

### 3. Cerebras

| Model | Free limit | Speed |
|-------|-----------|-------|
| llama-3.1-8b | 30 RPM, 1M tokens/day | Ultra fast (~1000+ TPS) |

- **Cost:** $0, no credit card
- **API:** OpenAI-compatible
- **Get key:** https://cloud.cerebras.ai
- **Limitation:** 8K context cap on free tier

### 4. Mistral La Plateforme (direct)

| Tier | Limit |
|------|-------|
| Free "Experiment" | 2 RPM, 500K TPM, 1B tokens/month |

- **Cost:** $0
- **API:** Mistral native format
- **Get key:** https://console.mistral.ai
- **1 billion tokens/month free** — enough for ~50,000 architecture analyses

### 5. OpenRouter (free models)

| Tier | Limit |
|------|-------|
| Free models (`:free` suffix) | ~50 req/day (no credits) or ~1,000/day (≥$10 credit) |
| Free models available | Llama 3.3 70B, Mistral 7B, Gemma 2 9B, etc. |

- **Cost:** $0 for `:free` models
- **API:** OpenAI-compatible
- **Get key:** https://openrouter.ai/keys

### 6. Together AI

| Tier | Limit |
|------|-------|
| Free signup credits | $25 free credits |
| Mistral Small 3.1 24B | $0.10/1M input, $0.30/1M output |

- **Cost:** $25 free credits at signup
- **50 analyses cost:** ~172,500 tokens × $0.20/1M avg = ~$0.03
- **$25 credit covers:** ~800,000 analyses

### 7. NVIDIA NIM

| Tier | Limit |
|------|-------|
| Free build credits | Many models free, no daily cap |

- **Cost:** $0
- **API:** OpenAI-compatible
- **Get key:** https://build.nvidia.com
- **Models:** 100+ models including Llama 3.3 70B, Mistral, etc.

---

## Recommendation: Multi-Provider Fallback Strategy

### Best approach for NEXUS (5-10 users, 50 analyses)

**Primary: Cloudflare Workers AI (already configured)**
- 10,000 free neurons/day = ~212 analyses
- No changes needed — already works
- Mistral Small 3.1 24B is free tier eligible

**Fallback 1: Groq (add as fallback)**
- 1,000 free requests/day on llama-3.3-70b
- Already have `GROQ_MODEL = "qwen/qwen3.8-27b"` in `external_llm.ts`
- Just need to wire it as a fallback in the enrichment chain

**Fallback 2: Google Gemini (already configured)**
- 1,500 free requests/day
- Already have `GEMINI_API_KEY` support in `external_llm.ts`
- `gemini-flash-lite-latest` is already set up

### Cost for 50 analyses with current setup

| Provider | Cost for 50 analyses | Notes |
|----------|---------------------|-------|
| Cloudflare Free | **$0** | Within 10K neurons/day |
| Cloudflare Paid ($5/mo) | **$5/month** | 10K free + $0.011/1K neurons after |
| Groq Free | **$0** | 1,000 req/day free |
| Gemini Free | **$0** | 1,500 req/day free |
| Together AI | **$0** (from $25 credit) | $25 covers ~800K analyses |

### Monthly cost projection for 5-10 users

| Usage pattern | Cloudflare Free | Cloudflare Paid | With Groq fallback |
|---------------|----------------|-----------------|-------------------|
| Light (5 users, 3 analyses/day) | $0 | $5/mo | $0 |
| Medium (10 users, 5 analyses/day) | $0 | $5/mo | $0 |
| Heavy (10 users, 15 analyses/day) | $0-3/mo | $5-8/mo | $0 |
| Extreme (10 users, 30 analyses/day) | $5-10/mo | $10-15/mo | $0-2/mo |

---

## Sponsorship Programs

### 1. Cloudflare Developer Sponsorship
- **Cloudflare for Open Source:** Free Workers Paid plan for qualifying open source projects
- **Apply at:** https://www.cloudflare.com/lp/opensource/
- **What you get:** Workers Paid ($5/mo value) + 10K free neurons/day + paid-tier models
- **Requirements:** Public repo, active community, non-commercial or open source

### 2. Groq for Open Source
- No formal sponsorship program, but free tier is permanently free
- 1,000 requests/day on Llama 3.3 70B is enough for 50 analyses
- No application needed — just sign up

### 3. Google for Startups / AI Studio
- **Google AI Studio:** Free Gemini API, no application needed
- **Google for Startups Cloud Program:** Up to $200K in Google Cloud credits over 2 years
- **Apply at:** https://cloud.google.com/startup
- **Requirements:** Early-stage startup, < $10M funding

### 4. Together AI Research Credits
- **Together AI Research Grants:** Free credits for research/open source
- **Apply at:** research@together.ai
- **What you get:** $25-$500 in API credits
- **Requirements:** Research project or open source contribution

### 5. NVIDIA Developer Program
- **Free NIM access:** No application, just sign up
- **NVIDIA Inception Program:** Free credits for startups
- **Apply at:** https://www.nvidia.com/en-us/startups/
- **What you get:** $100K+ in cloud credits over 2 years

### 6. Mistral AI Research Program
- **Free Experiment tier:** 1B tokens/month, no application
- **Research grants:** Contact Mistral for larger allocations
- **Apply at:** https://console.mistral.ai

### 7. OpenRouter Free Tier
- 23+ free models (`:free` suffix)
- No application, no credit card
- ~50 requests/day on free tier (without credits)

---

## Final Recommendation

### For 50 architecture analyses with 5-10 users:

**You don't need sponsorship. The free tiers are more than enough.**

| Provider | Free capacity | Analyses covered |
|----------|--------------|-----------------|
| Cloudflare (current) | 212/day | 212 analyses |
| Groq (fallback) | 1,000/day | 1,000 analyses |
| Gemini (fallback) | 1,500/day | 1,500 analyses |
| **Combined** | **2,712/day** | **2,712 analyses** |

You could run **50 analyses/day for free** using just Cloudflare's free tier.
With Groq + Gemini as fallbacks, you have 2,712 analyses/day capacity — all free.

### If you want to apply for sponsorship anyway:

1. **Cloudflare Open Source Sponsorship** — gets you Workers Paid ($5/mo) for free
2. **Google for Startups** — $200K in cloud credits (if you're a startup)
3. **NVIDIA Inception** — $100K+ in credits (if you're a startup)

### Action items to maximize free capacity:

1. ✅ Cloudflare Workers AI — already configured, free tier sufficient
2. Add Groq as fallback for Phase 1 enrichment (external_llm.ts already has Groq support)
3. ✅ Google Gemini — already configured as fallback in external_llm.ts
4. Consider applying for Cloudflare Open Source sponsorship for the Paid plan
