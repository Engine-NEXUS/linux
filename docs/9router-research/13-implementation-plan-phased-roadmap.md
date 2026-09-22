# 13 — Implementation Plan: Phased Roadmap

> **Goal:** Make NEXUS faster, more accurate, and capable of new
> features (commerce, social, multi-step) — incrementally, with each
> phase delivering real value before the next begins.

---

## What Already Exists (Don't Rebuild)

```
✅ STT: Groq Whisper Turbo (cloud, 247ms) + Moonshine Small (local, 165ms)
✅ STT cascade: Groq → local → Workers AI
✅ STT self-learning corrections (stt_learning.rs)
✅ Deterministic parser (intent_parser.rs, 58 intents, <5ms)
✅ NLU server (BERT-Mini ONNX, 80 MB, lazy)
✅ Orchestrator (orchestrator.rs, routing + events + cancellation)
✅ Local commands (open/close app, media, greetings, URLs, WhatsApp deep links)
✅ GitHub sub-command system (github_cmd.rs, merge/approve/close PR, etc.)
✅ Worker backend (Cloudflare, D1, OAuth, API key vault, quota)
✅ Worker LLM cascade (Gemini → Groq → Cloudflare Workers AI)
✅ TTS (Edge TTS primary, Piper/Kokoro fallback)
✅ Live mode (keyboard, browser, WhatsApp flow, window focus)
✅ Wake word (openWakeWord, ONNX)
✅ Confirmation gates (GitHub destructive operations)
```

## What's Missing (The Gaps)

```
🔲 9Router: Local AI gateway (route AI to Groq/Gemini directly, skip Worker)
🔲 MCP servers: Commerce (Swiggy, Amazon, Zepto) + Social (WhatsApp, Email)
🔲 MCPJungle: Unified MCP server manager
🔲 Moonshine Medium v2: Better local STT fallback (6.65% vs 7.84% WER)
🔲 Command center: Multi-step task planning + sub-center routing
🔲 Model distribution: Admin retrains NLU, family gets updates
🔲 Brain (Qwen 0.5B): Local reasoning model (admin only)
```

---

## Phase Priority (What Delivers the Most Value First)

```
Phase 1: 9ROUTER (biggest speed win — 3-7x faster AI)
Phase 2: MOONSHINE MEDIUM v2 (better STT accuracy, small effort)
Phase 3: MCP SERVERS (new capabilities — commerce + social)
Phase 4: COMMAND CENTER (multi-step task orchestration)
Phase 5: SELF-IMPROVEMENT (continuous training + model distribution)
Phase 6: BRAIN (Qwen 0.5B, admin-only local reasoning)
```

### Why This Order

```
Phase 1 (9Router) → Makes EVERYTHING faster immediately
  - AI responses: 3.5-6.5s → 968ms (3-7x faster)
  - Zero neurons (Groq/Gemini are free)
  - Biggest user-visible improvement
  - Moderate effort (new Rust module + Worker changes)

Phase 2 (Moonshine Medium v2) → Better STT accuracy
  - Local fallback WER: 7.84% → 6.65% (-15%)
  - Very low effort (change one config value)
  - Small but real improvement

Phase 3 (MCP servers) → New capabilities
  - Enables: order food, search Amazon, send WhatsApp, check movies
  - High effort (integrate MCPJungle + MCP servers)
  - High value (the features the user actually wants)

Phase 4 (Command center) → Multi-step tasks
  - Enables: "check movies then order dinner"
  - High effort (new task state machine)
  - Only useful AFTER Phase 3 (needs sub-centers to exist)

Phase 5 (Self-improvement) → Gets better over time
  - NLU retrains weekly, corrections propagate to family
  - Medium effort (automation + Worker R2 endpoints)
  - Long-term value, not immediate

Phase 6 (Brain) → Local reasoning
  - Qwen 0.5B on admin's laptop for conversational understanding
  - High effort (model integration, sidecar)
  - Only for admin — family uses Worker for reasoning
```

---

## Phase 1: 9Router — Local AI Gateway

### What It Does

9Router is a local gateway that routes AI reasoning requests directly
to free cloud providers (Groq, Gemini, Cerebras), bypassing the Worker
for AI calls. The Worker still handles identity, OAuth, and D1 — but
AI reasoning goes directly to the fastest free provider.

### Why It's First

```
Current:  Device → Worker (50ms) → Workers AI (500-2000ms) → Worker (50ms) → Device
          Total: 600-2100ms

With 9Router:  Device → localhost (1ms) → Groq (120ms) → localhost (1ms) → Device
               Total: 242ms

Speed improvement: 3-7x faster AI responses
Cost: $0 (Groq and Gemini are free)
Neurons: 0 (bypasses Workers AI for most requests)
```

### What Exists vs What's New

```
EXISTS (Worker side):
  ✅ external_llm.ts: callGemini(), callGroq(), callCloudflare()
  ✅ Cascade: Gemini → Groq → Cloudflare
  ✅ API key management (env.GEMINI_API_KEY, env.GROQ_API_KEY)

EXISTS (Rust side):
  ✅ stt_groq.rs: Already calls Groq directly for STT
  ✅ commands.rs: read_groq_api_key() — Groq key in settings
  ✅ network.rs: HTTP client for Worker calls

NEW (what we build):
  🔲 src-tauri/src/router.rs: 9Router module
  🔲 Provider routing logic (which provider for which task)
  🔲 Local API key storage (Gemini, Cerebras keys in settings)
  🔲 Fallback chain (9Router → Worker → Workers AI)
  🔲 Settings UI for adding provider API keys
```

### Implementation Steps

```
Step 1.1: Create router.rs — the 9Router module
  - Provider enum: Groq, Gemini, Cerebras, Worker, WorkersAI
  - Route function: task_type → best provider
  - Call function: send prompt to provider, get response
  - Fallback: if provider fails, try next in chain
  - Caching: simple in-memory cache for repeated questions

Step 1.2: Add provider API keys to settings
  - groqApiKey (already exists for STT — reuse for LLM too)
  - geminiApiKey (new)
  - cerebrasApiKey (new, optional)
  - Settings UI: add fields for Gemini and Cerebras keys

Step 1.3: Route AI requests through 9Router instead of Worker
  - In orchestrator.rs, WorkerBackend path:
    - BEFORE: send transcript to Worker, Worker calls Gemini/Groq/CF
    - AFTER: 9Router tries Groq/Gemini directly (local)
    - FALLBACK: if 9Router fails, send to Worker (old path)
  - Worker still handles: PR analysis (needs GitHub token), 
    deep analysis (needs GLM models), research (needs search)

Step 1.4: Add provider health checks
  - Ping each provider at startup
  - Mark unavailable providers as "down"
  - Skip down providers in routing
  - Re-check every 5 minutes

Step 1.5: Add usage tracking (local)
  - Track requests per provider per day
  - Show in settings: "Groq: 45/14400 today, Gemini: 12/1500 today"
  - Warn when approaching free tier limits
```

### Files to Create/Modify

```
CREATE:
  src-tauri/src/router.rs          (~300 lines, new 9Router module)

MODIFY:
  src-tauri/src/commands.rs        (add Gemini/Cerebras key settings)
  src-tauri/src/orchestrator.rs    (route AI through 9Router)
  src-tauri/src/lib.rs             (register router module)
  src/Settings.svelte              (add provider key fields)
```

### Expected Outcome

```
BEFORE: "What's the capital of France?" → 3.5-6.5s (Worker → Workers AI)
AFTER:  "What's the capital of France?" → 968ms (9Router → Groq)

BEFORE: "Analyse PR 42" → 3.5s (Worker → GLM-4.7, stays same)
AFTER:  "Analyse PR 42" → 3.5s (still via Worker, needs GitHub token)

BEFORE: General questions → 4.5-9.5s (Worker → Workers AI)
AFTER:  General questions → 968ms-1.2s (9Router → Groq/Gemini)
```

### Effort

```
Time: ~2-3 days
Risk: Low (additive — Worker fallback still works)
RAM: +20 MB (9Router process)
Speed: 3-7x faster for general AI questions
```

---

## Phase 2: Moonshine Medium v2 — Better Local STT

### What It Does

Replace Moonshine Small Streaming (123M, 7.84% WER) with Moonshine
Medium v2 (245M, 6.65% WER) as the local STT fallback.

### Why It's Second

```
Effort: Very low (change one config value)
Risk: Very low (Moonshine v2 is backward compatible)
Benefit: -15% WER on local STT fallback (7.84% → 6.65%)
RAM: +100 MB (only when local STT is active, lazy)
```

### Implementation Steps

```
Step 2.1: Update MOONSHINE_MODEL env var
  - Current: "small_streaming" (123M)
  - New: "medium_streaming" (245M) for admin
  - Keep "small_streaming" for family members (8GB laptops)

Step 2.2: Test accuracy
  - Run a few commands with local STT only
  - Verify WER improvement
  - Verify RAM stays within budget

Step 2.3: Make it configurable per device
  - Admin settings: "medium_streaming"
  - Family settings: "small_streaming" (default)
  - User can override in settings
```

### Files to Modify

```
MODIFY:
  server/stt_server.py    (change default model)
  src-tauri/src/lazy_stt.rs (pass model from settings)
  src-tauri/src/commands.rs (add moonshine_model setting)
```

### Effort

```
Time: ~2 hours
Risk: Very low
Speed: Same latency (269ms vs 165ms — negligible)
Accuracy: -15% WER on local fallback
```

---

## Phase 3: MCP Servers — Commerce + Social

### What It Does

Integrate MCPJungle (MCP server manager) and connect MCP servers for
commerce (Swiggy, Amazon, Zepto, BookMyShow) and social (WhatsApp,
Email, GitHub, YouTube, Spotify).

### Why It's Third

```
Enables: The features the user actually wants (order food, send WhatsApp, etc.)
Effort: High (integrate MCPJungle + multiple MCP servers)
Risk: Medium (MCP servers are community-maintained, may need debugging)
Value: Very high (new capabilities)
```

### Implementation Steps

```
Step 3.1: Integrate MCPJungle
  - Install MCPJungle (Go binary, ~15 MB)
  - Configure MCPJungle to manage MCP servers
  - Expose MCPJungle API to NEXUS (localhost:8765)
  - Tool group management (brain sees only relevant tools)

Step 3.2: Add commerce MCP servers
  - Swiggy MCP (food + Instamart + Dineout)
  - Amazon MCP (search, product details, cart, checkout)
  - Zepto MCP (groceries)
  - BookMyShow MCP (movie tickets)
  - Each: OAuth/OTP login, stored locally

Step 3.3: Add social MCP servers
  - WhatsApp MCP (QR login, send/reply/react)
  - Email MCP (IMAP/SMTP, send allowlist)
  - GitHub MCP (already have github_cmd.rs — bridge to MCP)
  - YouTube MCP (search, transcripts)
  - Spotify MCP (search, playlist, playback)

Step 3.4: Add intent mappings
  - New intents in intent_parser.rs:
    - food_order, grocery_order, product_search, movie_tickets
    - send_message, send_email, search_video, play_music
  - New NLU training examples for these intents
  - Route to Commerce/Social sub-centers

Step 3.5: Add confirmation gates
  - Read actions (search, list): no confirmation
  - Write actions (send, add to cart): confirmation required
  - Destructive actions (checkout, book): confirmation + details
  - Generalize the existing GitHub confirmation pattern

Step 3.6: Add MCP tool calling from orchestrator
  - Orchestrator routes to MCPJungle
  - MCPJungle routes to the right MCP server
  - MCP server executes the tool
  - Result flows back through MCPJungle → orchestrator → TTS
```

### Files to Create/Modify

```
CREATE:
  src-tauri/src/mcp_client.rs      (~200 lines, MCPJungle client)
  src-tauri/src/mcp_tools.rs        (~150 lines, tool definitions)
  server/mcp/mcpjungle.toml         (MCPJungle config)

MODIFY:
  src-tauri/src/orchestrator.rs    (add Commerce + Social sub-centers)
  src-tauri/src/intent_parser.rs   (new intents for commerce/social)
  server/nlu/dataset.json           (new training examples)
  server/nlu/train.py              (new intents in INTENTS list)
```

### Expected Outcome

```
NEW: "order my regular drink from swiggy" → Swiggy MCP → cart → confirm → order
NEW: "search sony headphones on amazon" → Amazon MCP → results with images
NEW: "send mom a whatsapp message" → WhatsApp MCP → confirm → send
NEW: "check movie tickets for goat" → BookMyShow MCP → showtimes
```

### Effort

```
Time: ~1-2 weeks
Risk: Medium (MCP servers vary in maturity)
RAM: +50-100 MB (lazy MCP servers)
```

---

## Phase 4: Command Center — Multi-Step Orchestration

### What It Does

Upgrade the flat orchestrator to a hierarchical command center with
sub-centers, multi-step planning, parallel execution, and error handling.

### Why It's Fourth

```
Only useful AFTER Phase 3 (needs sub-centers to exist)
Enables: "check movies then order dinner" (multi-step)
Effort: High (new task state machine)
Risk: Medium (refactoring core orchestrator)
```

### Implementation Steps

```
Step 4.1: Add TaskState (replace flat ActiveRequest)
  - TaskState struct with plan, steps, results, summary
  - State machine: Idle → Parsing → Planning → Routing → Executing → Merging → Done

Step 4.2: Add multi-step planning
  - Brain (admin) or Worker (family) generates a plan
  - Plan = list of PlanSteps with dependencies
  - Detect multi-step: "then", "and", "also" as step separators

Step 4.3: Add sub-center routing
  - 5 sub-centers: Local, CloudAI, Commerce, Social, SystemControl
  - Route based on intent → sub-center mapping
  - Parallel execution for independent steps

Step 4.4: Add error handling
  - Retry with same sub-center
  - Retry with alternative (Swiggy → Zomato)
  - Skip optional steps
  - Fallback to Workers AI
  - Run summary for debugging

Step 4.5: Generalize confirmation gates
  - Action categories: Read, Write, Destructive, Blocked
  - Apply to all sub-centers (not just GitHub)
```

### Files to Create/Modify

```
CREATE:
  src-tauri/src/task_state.rs      (~400 lines, TaskState + PlanStep + RunSummary)
  src-tauri/src/command_center.rs  (~300 lines, new orchestrator logic)

MODIFY:
  src-tauri/src/orchestrator.rs    (wrap existing as sub-centers)
  src-tauri/src/lib.rs              (register new modules)
```

### Effort

```
Time: ~1 week
Risk: Medium (refactoring core, but Worker fallback still works)
RAM: +5 MB (task state)
```

---

## Phase 5: Self-Improvement — Continuous Training

### What It Does

Admin's laptop retrains BERT-Mini NLU weekly using collected corrections.
Updated models propagate to family members via Worker R2.

### Why It's Fifth

```
Long-term value (gets better over time)
Not immediately visible (takes weeks to show improvement)
Medium effort (automation + Worker endpoints)
```

### Implementation Steps

```
Step 5.1: Auto-collect corrections
  - Extend stt_learning.rs pattern to NLU and brain routing
  - Log misclassifications + user corrections
  - Store in correction JSON files

Step 5.2: Weekly auto-retrain
  - Cron job (Sunday 3 AM) on admin's laptop
  - Collect corrections → add to dataset.json
  - Retrain BERT-Mini (nexus train)
  - Validate: new model must beat old model
  - Export ONNX

Step 5.3: Model distribution
  - Worker R2 storage for model versions
  - New endpoints: GET /models/version, GET /models/nlu/:version
  - Family members check for updates on startup
  - Download + verify checksum + swap on idle

Step 5.4: Admin dashboard
  - Review corrections (approve/reject)
  - Trigger retrain manually
  - Push update to family
  - Rollback to previous version
```

### Effort

```
Time: ~1 week
Risk: Low (additive, doesn't change existing flow)
RAM: +500 MB during 5-min weekly retrain (admin only)
```

---

## Phase 6: Brain — Qwen 0.5B Local Reasoning

### What It Does

Add Qwen 0.5B as a local reasoning model on the admin's laptop. Handles
conversational understanding, function calling, and multi-step planning.

### Why It's Last

```
Only for admin (family uses Worker for reasoning)
High effort (model integration, sidecar, prompt engineering)
Not needed until Phase 4 (command center needs a planner)
```

### Implementation Steps

```
Step 6.1: Integrate Qwen 0.5B
  - Download model (~398 MB)
  - Run as sidecar (llama.cpp or similar)
  - Expose on localhost:20129
  - Lazy load (only when needed, killed after 5 min idle)

Step 6.2: Wire brain into orchestrator
  - Brain handles: intent classification (when NLU confidence low)
  - Brain handles: multi-step planning (generates PlanStep list)
  - Brain handles: conversational responses
  - Fallback: if brain unavailable, use Worker → Groq

Step 6.3: Add brain corrections
  - Log routing mistakes
  - Inject corrections into brain's system prompt
  - Auto-apply after 3 consistent corrections
```

### Effort

```
Time: ~1 week
Risk: Medium (model integration, prompt engineering)
RAM: +500 MB (admin only, lazy)
```

---

## Summary: The Full Roadmap

```
Phase 1: 9ROUTER                    2-3 days    3-7x faster AI
Phase 2: MOONSHINE MEDIUM v2        2 hours     -15% STT WER
Phase 3: MCP SERVERS               1-2 weeks   New capabilities
Phase 4: COMMAND CENTER            1 week      Multi-step tasks
Phase 5: SELF-IMPROVEMENT          1 week      Gets better over time
Phase 6: BRAIN (Qwen 0.5B)         1 week      Local reasoning (admin)

Total: ~5-7 weeks for full vision
```

### Priority Matrix

| Phase | Speed | Accuracy | New Features | Effort | Do First? |
|-------|-------|----------|-------------|--------|-----------|
| 1. 9Router | **3-7x** | Same | Same | Medium | **YES** |
| 2. Moonshine v2 | Same | **-15%** | Same | Very Low | **YES** |
| 3. MCP servers | Same | Same | **Huge** | High | After 1-2 |
| 4. Command center | Same | Same | Multi-step | High | After 3 |
| 5. Self-improvement | Same | **+over time** | Same | Medium | After 3-4 |
| 6. Brain | Same | Same | Conversational | High | Last |

### What to Start With

**Phase 1 (9Router) + Phase 2 (Moonshine v2) together.**

- Phase 1 is the biggest speed win (3-7x faster AI)
- Phase 2 is the easiest accuracy win (2 hours, -15% WER)
- Both are low-risk and additive (Worker fallback still works)
- Together they take ~3 days
- Together they make NEXUS noticeably faster AND more accurate

**Then Phase 3 (MCP servers) for the new features the user wants.**

Shall I start implementing Phase 1 (9Router)?
