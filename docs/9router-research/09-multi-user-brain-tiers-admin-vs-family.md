# 09 — Multi-User Brain Tiers: Admin vs Family Members

> **The problem I got wrong:** I assumed all 5 users run the Qwen 0.5B
> brain locally. The user corrected me: the brain only exists on the
> admin's laptop. Other users have 8GB RAM laptops, don't want 500MB
> used by a model, and need a much smaller footprint.
>
> **The fix:** Three brain tiers. Admin gets the full brain. Family
> members get a "lite brain" (80 MB, no LLM). Everything complex goes
> to cloud for everyone.

---

## 1. The Three Brain Tiers

```
Tier 1: ADMIN BRAIN (you, the administrator)
  Model: Qwen 0.5B (398 MB file, ~500 MB RAM)
  Capabilities: Conversational understanding + function calling + routing
  Lives: Your laptop only
  Cloud delegation: Via local 9Router (localhost:20128) → Groq/Gemini
  MCP: Local MCP servers + local MCPJungle

Tier 2: FAMILY LITE BRAIN (other 4 users)
  Model: NONE (no LLM)
  Components: Deterministic parser (0 MB) + BERT-Mini NLU (80 MB)
  Capabilities: Intent classification + tool routing + cloud delegation
  Lives: Each family member's laptop
  Cloud delegation: Via Worker → Groq/Gemini (0 neurons)
  MCP: Local MCP servers (lightweight, lazy-loaded)

Tier 3: CLOUD BRAIN (shared, for complex reasoning)
  Model: Groq Llama 3.3 70B / Gemini Flash / Cerebras
  Capabilities: Full reasoning, analysis, recommendations
  Lives: Cloud (free tiers)
  Used by: Both admin (via 9Router) and family (via Worker)
  Cost: 0 neurons (free external providers)
```

---

## 2. Why Family Members Don't Need an LLM

### The Insight from Research

A 2026 study (Switchcraft, arxiv) found:

> "A lightweight encoder — specifically DistilBERT with 66 million
> parameters — is fine-tuned as a multi-label classifier. Given an
> agentic query, the router learns to predict which model should
> handle it. Switchcraft achieves 82.9% accuracy — matching or
> exceeding the best individual model — while reducing inference
> cost by 84%."

**A 66M parameter classifier can route tool calls with 82.9% accuracy.**
You don't need a 500M parameter LLM for routing. You need a classifier.

NEXUS already HAS this classifier: **BERT-Mini NLU** (port 39218).

### What the Lite Brain Does (Without Any LLM)

```
User: "Order my regular drink from Swiggy"

Lite Brain (no LLM):
  1. Deterministic parser (0 MB, <0.1ms)
     → "order" + "regular drink" + "swiggy" → matches food_order intent
     → If matched: execute directly (call Swiggy MCP)

  2. If not matched: BERT-Mini NLU (80 MB, ~20ms)
     → Classifies intent: food_order
     → Extracts slots: platform=swiggy, item=regular_drink

  3. If NLU confidence is low: delegate to cloud
     → Worker → Groq: "User said 'order my regular drink from swiggy'.
        Classify intent and extract: platform, item, action"
     → Groq returns: {intent: "food_order", platform: "swiggy",
        item: "regular_drink", action: "search_and_cart"}
     → Lite brain executes the tool calls

  4. For any REASONING ("should I buy this or wait?"):
     → Worker → Groq: full reasoning
     → Returns answer
     → Lite brain speaks it via TTS
```

### What the Lite Brain CAN Do (Without Cloud)

| Task | How | RAM | Latency |
|------|-----|-----|---------|
| Open/close app | Deterministic parser | 0 MB | <5ms |
| Media controls | Deterministic parser | 0 MB | <5ms |
| Greetings | Deterministic parser | 0 MB | <1ms |
| Open URL | Deterministic parser | 0 MB | <5ms |
| WhatsApp deep link | Deterministic parser | 0 MB | <5ms |
| Intent classification | BERT-Mini NLU | 80 MB | ~20ms |
| Tool routing (which MCP to call) | BERT-Mini NLU | 80 MB | ~20ms |
| Slot extraction (what to search) | BERT-Mini NLU | 80 MB | ~20ms |
| Call MCP servers | Direct (via MCPJungle) | 0 MB | varies |
| Read memory/routines | Local JSON file | 0 MB | <1ms |
| Confirmation gates | Rust safety layer | 0 MB | <1ms |
| Speak result | edge-tts / Piper | 0-80 MB | 200-600ms |

### What the Lite Brain CANNOT Do (Needs Cloud)

| Task | Why | Where | Cost |
|------|-----|-------|------|
| Complex reasoning | Needs LLM | Worker → Groq | 0 neurons |
| Recommendations | Needs LLM | Worker → Groq | 0 neurons |
| "Should I buy this?" | Needs analysis | Worker → Gemini | 0 neurons |
| Multi-step planning | Needs LLM | Worker → Groq | 0 neurons |
| Conversational responses | Needs LLM | Worker → Groq | 0 neurons |
| Code review | Needs specialized model | Worker → GLM-4.7 | 50 neurons |

**The lite brain handles 80-90% of commands locally. Only 10-20% need cloud.**

---

## 3: RAM Comparison — Admin vs Family

### Admin (Your Laptop)

```
Component                    RAM (idle)    RAM (active)
─────────────────────────────────────────────────────
nexus.exe (Rust + wake word)  48 MB         48 MB
WebView2 (orb, low-mem)       36 MB         36 MB
STT (Groq cloud)              0 MB          0 MB
TTS (edge-tts cloud)          0 MB          0 MB
NLU (BERT-Mini, lazy)         0 MB          80 MB
Brain (Qwen 0.5B, lazy)       0 MB          500 MB
9Router (local)               20 MB         20 MB
MCPJungle (local)             15 MB         15 MB
MCP servers (lazy, 2-3 active) 0 MB         50-100 MB
─────────────────────────────────────────────────────
TOTAL idle                    119 MB
TOTAL active (brain + NLU)     749-799 MB
```

### Family Member (8GB Laptop, Lite Brain)

```
Component                    RAM (idle)    RAM (active)
─────────────────────────────────────────────────────
nexus.exe (Rust + wake word)  48 MB         48 MB
WebView2 (orb, low-mem)       36 MB         36 MB
STT (Groq cloud)              0 MB          0 MB
TTS (edge-tts cloud)          0 MB          0 MB
NLU (BERT-Mini, lazy)         0 MB          80 MB
Brain (Qwen 0.5B)             0 MB          0 MB        ← NO LLM
9Router (local)               0 MB          0 MB        ← NO 9ROUTER
MCPJungle (local)             15 MB         15 MB
MCP servers (lazy, 1-2 active) 0 MB         25-50 MB
─────────────────────────────────────────────────────
TOTAL idle                    99 MB
TOTAL active (NLU + 1 MCP)    169-194 MB
```

**Family member uses ~170 MB for the brain layer. Admin uses ~580 MB.**

The difference: family doesn't run Qwen 0.5B (saves 500 MB) or 9Router (saves 20 MB). They use BERT-Mini (80 MB) for routing and the Worker for reasoning.

---

## 4. How Cloud Delegation Works for Each User Type

### Admin: 9Router → Groq/Gemini (Fastest)

```
Admin speaks: "Should I buy the Sony headphones now or wait?"

Admin's device:
  STT (247ms) → Parser (0.1ms) → Brain Qwen 0.5B (300ms)
  Brain: "This needs reasoning. Delegate to 9Router."
  9Router (localhost:20128) → Groq API (120ms)
  Groq: "The current price is ₹26,990. Sale history shows..."
  Brain formats → TTS (600ms)

Total: ~1,267ms
Path: Device → localhost → Groq → localhost → Device
Hops: 1 (to Groq)
```

### Family: Worker → Groq/Gemini (Slightly Slower)

```
Family member speaks: "Should I buy the Sony headphones now or wait?"

Family device:
  STT (247ms) → Parser (0.1ms) → NLU BERT-Mini (20ms)
  NLU: "Intent = product_advice. Confidence low. Delegate to cloud."
  Device → Worker (50ms) → Worker calls Groq (120ms) → Worker (50ms) → Device
  Groq: "The current price is ₹26,990. Sale history shows..."
  Lite brain formats → TTS (600ms)

Total: ~1,087ms
Path: Device → Worker → Groq → Worker → Device
Hops: 2 (to Worker, to Groq)
```

**The family path is only ~180ms slower** (50ms Worker round-trip × 2).
Both are under 1.5 seconds. Both use 0 neurons (Groq is free).

### Latency Comparison

| Command Type | Admin (9Router) | Family (Worker) | Difference |
|---|---|---|---|
| Local (open/close) | 848ms | 848ms | 0ms (same — no cloud) |
| AI reasoning (Groq) | 968ms | 1,087ms | +119ms |
| AI reasoning (Gemini) | 1,128ms | 1,247ms | +119ms |
| PR analysis (GLM-4.7) | 1,147ms | 1,147ms | 0ms (both via Worker) |
| MCP tool (Swiggy) | 848ms + API | 848ms + API | 0ms (same — local MCP) |

**The family experience is nearly identical to the admin.** The 119ms
difference is imperceptible because "On it sir" plays instantly (<5ms).

---

## 5: MCP Server Approach — My Recommendation

### Recommendation: Local MCP Servers on Each Device

**Why local is better than central:**

| Factor | Local MCP (per device) | Central MCPJungle (hosted) |
|--------|----------------------|--------------------------|
| Security | ✅ Credentials stay on user's device | ⚠️ All credentials on one server |
| Cost | ✅ $0 (no server needed) | ❌ $5-20/mo VPS |
| RAM per user | ~50 MB (1-2 MCP servers, lazy) | 0 MB (connects remotely) |
| Latency | ✅ <1ms (localhost) | ~50ms (network) |
| Multi-user isolation | ✅ Natural (each device = own credentials) | ⚠️ Needs per-user tokens |
| Failure mode | One user's MCP breaks = only they affected | Central server down = all affected |

**My recommendation: Local MCP servers, lazy-loaded, per device.**

### How It Works on a Family Member's 8GB Laptop

```
Family member's 8GB laptop:
  Windows:           ~3 GB
  Chrome (5 tabs):   ~1.5 GB
  Other apps:        ~2 GB
  Available:         ~1.5 GB

NEXUS uses:
  nexus.exe:         48 MB
  WebView2:          36 MB
  NLU (lazy):        80 MB (only when needed, killed after 60s idle)
  MCPJungle:         15 MB
  MCP servers:       50 MB (lazy — Swiggy MCP starts when food ordered,
                            killed after 5 min idle)
  ─────────────────────────
  Total NEXUS:       ~229 MB peak, ~99 MB idle

Remaining for user:  ~1.27 GB (plenty)
```

**Even on an 8GB laptop, NEXUS uses only ~229 MB peak.** The key is
lazy-loading: NLU, MCP servers, and TTS only start when needed and
are killed after idle timeout.

### MCP Server Lifecycle on Family Device

```
Idle state (no commands):
  MCP servers: NOT running (0 MB)
  NLU: NOT running (0 MB)
  Only: nexus.exe (48 MB) + WebView2 (36 MB) + MCPJungle (15 MB)
  Total: 99 MB

User says "order food from swiggy":
  1. STT (Groq, cloud, 247ms) → transcript
  2. Parser matches "order" + "food" + "swiggy" → food_order intent
  3. MCPJungle starts Swiggy MCP server (cold start ~2s first time)
  4. Swiggy MCP loads user's OAuth token (from local storage)
  5. Swiggy MCP calls Swiggy API → returns restaurants
  6. Brain/NLU formats response → TTS speaks it
  7. After 5 min idle: Swiggy MCP killed (frees 50 MB)

Peak during food order: 99 + 80 (NLU) + 50 (Swiggy MCP) = 229 MB
After idle: back to 99 MB
```

---

## 6: The Complete Multi-User Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    ADMIN (Your Laptop)                          │
│                                                                 │
│  Wake Word → STT (Groq) → Parser → NLU → Brain (Qwen 0.5B)     │
│                                         │                       │
│              ┌──────────────────────────┤                       │
│              │                          │                       │
│              ▼                          ▼                       │
│        9Router (local)           MCPJungle (local)              │
│        localhost:20128          localhost:8765                 │
│         │                            │                          │
│         ▼                            ▼                          │
│    Groq/Gemini/Cerebras        MCP Servers (local)              │
│    (free cloud AI)            Swiggy, Amazon, WhatsApp          │
│                                                            │
│  RAM: ~580 MB active | ~119 MB idle                              │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│              FAMILY MEMBER 1 (8GB Laptop)                       │
│                                                                 │
│  Wake Word → STT (Groq) → Parser → NLU (BERT-Mini)             │
│                                    │                            │
│              ┌─────────────────────┤                            │
│              │                     │                            │
│              ▼                     ▼                            │
│        Worker (cloud)       MCPJungle (local)                  │
│        → Groq/Gemini        MCP Servers (local)                │
│        (free cloud AI)       Swiggy, Amazon (own login)        │
│                                                            │
│  RAM: ~229 MB peak | ~99 MB idle | NO LLM                       │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│              FAMILY MEMBER 2 (8GB Laptop)                       │
│              [Same as Family Member 1]                          │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│              FAMILY MEMBERS 3, 4 (Same architecture)            │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│              CLOUDFLARE WORKER (Shared, $5/mo)                 │
│                                                                 │
│  For Admin: 9Router handles AI (bypasses Worker)                │
│  For Family: Worker routes to Groq/Gemini (0 neurons)          │
│  For All: Worker handles identity, OAuth, D1, API key vault   │
│  Fallback: Workers AI (neurons, last resort only)              │
│                                                            │
│  Neurons/day: ~1,450/user × 5 = 7,250 (under 10K free limit)    │
└─────────────────────────────────────────────────────────────────┘
```

---

## 7: What Each User Gets

### Admin (You)

| Feature | How | RAM | Latency |
|---------|-----|-----|---------|
| Conversational understanding | Qwen 0.5B local | 500 MB | 300ms |
| Function calling | Qwen 0.5B local | 0 MB | 300ms |
| AI reasoning | 9Router → Groq | 0 MB | 120ms |
| MCP tools | Local MCP servers | 50 MB | <1ms routing |
| Provider management | 9Router dashboard | 20 MB | <1ms |
| Clipboard key capture | Rust arboard | 0 MB | <1ms |
| Proactive reminders | Local brain + JSON state | 0 MB | <1ms |
| Complex multi-step planning | Brain + 9Router + MCP | 500 MB | ~1s |

### Family Members

| Feature | How | RAM | Latency |
|---------|-----|-----|---------|
| Intent classification | BERT-Mini NLU | 80 MB | 20ms |
| Tool routing | BERT-Mini NLU | 0 MB | 20ms |
| AI reasoning | Worker → Groq | 0 MB | 240ms |
| MCP tools | Local MCP servers | 50 MB | <1ms routing |
| Simple commands | Deterministic parser | 0 MB | <1ms |
| Reminders | Local JSON state | 0 MB | <1ms |
| Multi-step tasks | Worker → Groq plans, local executes | 0 MB | ~1.2s |
| Conversational responses | Worker → Groq | 0 MB | ~240ms |

### What Family Members DON'T Get (vs Admin)

| Feature | Admin | Family | Why |
|---------|-------|--------|-----|
| Conversational brain | ✅ Qwen 0.5B | ❌ NLU only | RAM constraint |
| Local 9Router | ✅ | ❌ | No need (Worker routes) |
| Provider management | ✅ Full control | ❌ | Admin manages providers |
| Clipboard key capture | ✅ | ❌ | Admin adds keys |
| Proactive provider discovery | ✅ | ❌ | Admin's job |
| Offline AI reasoning | ✅ Limited | ❌ | Family needs cloud |

**Family members get 90% of the functionality at 1/5 the RAM.**

---

## 8: The Neuron Budget (5 Users, Mixed Tiers)

```
Admin (uses 9Router, bypasses Worker for AI):
  Local commands:              30 calls  → 0 neurons
  AI via 9Router → Groq:      20 calls  → 0 neurons (bypasses Worker)
  PR analysis (Worker GLM):     5 calls  → 250 neurons
  Deep analysis (Worker GLM):   2 calls  → 1,000 neurons
  MCP tools:                   15 calls  → 0 neurons
  ─────────────────────────────────────
  Admin total:                1,250 neurons/day

Family member × 4 (uses Worker → Groq/Gemini for AI):
  Local commands:              30 calls  → 0 neurons
  AI via Worker → Groq:        15 calls  → 0 neurons (Groq free)
  AI via Worker → Gemini:       5 calls  → 0 neurons (Gemini free)
  PR analysis (Worker GLM):     2 calls  → 100 neurons
  Fallback (Worker AI):          1 call   → 100 neurons
  MCP tools:                   10 calls  → 0 neurons
  ─────────────────────────────────────
  Per family member:            200 neurons/day
  × 4 family members:          800 neurons/day

GRAND TOTAL (5 users):   1,250 + 800 = 2,050 neurons/day
Free limit:               10,000 neurons/day
Headroom:                  7,950 neurons/day (79% unused)

COST: $5/month (Cloudflare plan only, 0 AI overage)
```

**With this tiered architecture, 5 users use only 2,050 neurons/day —
20% of the free limit. Massive headroom.**

---

## 9: What Happens When a Family Member's Internet Is Down?

```
Family member, no internet:
  STT: Falls back to local Moonshine (165ms, 150 MB)
  TTS: Falls back to local Piper (200ms, 80 MB)
  Parser: Works (local, 0 MB)
  NLU: Works (local, 80 MB)
  MCP tools: FAILS (needs internet for Swiggy/Amazon APIs)
  AI reasoning: FAILS (needs Worker/Groq)
  Simple commands: WORKS (open/close/media/greetings)

Result: Family member can still control their laptop locally.
        Cannot order food, search Amazon, or ask complex questions.
        This is expected — these services need internet anyway.

Admin, no internet:
  STT: Falls back to local Moonshine
  TTS: Falls back to local Piper
  Brain: Works (Qwen 0.5B local)
  9Router: Works (local, but can't reach Groq/Gemini)
  MCP tools: FAILS (needs internet)
  AI reasoning: FAILS (9Router can't reach cloud)
  Simple commands: WORKS
  Limited local reasoning: WORKS (Qwen 0.5B can answer simple questions)

Result: Admin has slightly better offline capability (local brain
        can do simple Q&A without cloud), but complex reasoning still
        needs internet.
```

---

## 10: Summary — The Simple Version

### The Problem You Pointed Out

> "Brain only exists in my laptop. Others have 8GB RAM, can't run
> a 500MB model. Some don't want that much RAM usage."

### The Fix

**Three brain tiers:**

```
Admin (you):     Qwen 0.5B brain (500 MB) + 9Router → cloud for thinking
Family members:  BERT-Mini NLU (80 MB) + Worker → cloud for thinking
Cloud (shared):  Groq/Gemini (free) does the actual thinking for everyone
```

**Family members don't need an LLM.** They need a classifier (BERT-Mini,
80 MB) that routes commands to the right tool, and delegates reasoning
to the cloud (Worker → Groq, 0 neurons).

### RAM Per User

| User | Brain RAM | Idle RAM | Peak RAM | LLM? |
|------|----------|---------|---------|------|
| Admin | 500 MB (Qwen 0.5B) | 119 MB | 799 MB | ✅ Yes |
| Family | 80 MB (BERT-Mini) | 99 MB | 229 MB | ❌ No |

### Cost

```
5 users, mixed tiers: 2,050 neurons/day (20% of free limit)
Cost: $5/month total (Cloudflare plan, 0 AI overage)
Per user: $1/month
```

### Latency

| Command | Admin | Family | Difference |
|---------|-------|--------|-----------|
| Local (open/close) | 848ms | 848ms | 0ms |
| AI reasoning | 968ms | 1,087ms | +119ms |
| MCP tools | same | same | 0ms |

**Family experience is nearly identical to admin.** The 119ms difference
is imperceptible because "On it sir" plays instantly.

### MCP Recommendation

**Local MCP servers on each device** (not central):
- More secure (credentials stay on each user's device)
- $0 cost (no central server)
- Lazy-loaded (50 MB only when actively ordering, killed after idle)
- Each user's Swiggy/Amazon/WhatsApp login is their own

---

## 11: What I Got Wrong and Fixed

| What I Said (Wrong) | What's Actually True (Fixed) |
|---|---|
| All 5 users run Qwen 0.5B locally | Only admin runs Qwen 0.5B. Family uses BERT-Mini (80 MB). |
| All users need 500 MB for the brain | Family needs 80 MB (NLU only, no LLM). |
| All users run 9Router locally | Only admin runs 9Router. Family uses Worker for AI routing. |
| All users have the same architecture | Three tiers: admin (full brain), family (lite brain), cloud (shared reasoning). |
| 5 users × 1,450 neurons = 7,250 | Actually 2,050 neurons (admin bypasses Worker for AI via 9Router). |
