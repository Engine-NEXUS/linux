# 07 — Worker AI Usage Plan: Local vs Cloud vs Compulsory

> **Goal:** Classify every existing and future NEXUS feature by where it runs,
> what it costs, and whether the Cloudflare Worker (and its 10K neurons/day)
> is needed at all. The user's constraint: **don't eat the Worker limit
> when something can be done locally or through a free external provider.**

---

## 1. The Three Cost Tiers (Already in NEXUS)

NEXUS already has a 3-tier cost architecture. Every feature falls into one:

```
Tier 0 — FULLY LOCAL          Tier 1 — FREE EXTERNAL         Tier 2 — CLOUDFLARE WORKERS AI
─────────────────────────     ─────────────────────────     ─────────────────────────────────
0 cost, 0 internet            0 neurons, uses internet      Uses neurons ($5/mo plan)
0 ms latency                  ~200ms latency                ~500-2000ms latency

Wake word detection           Groq STT (cloud)              PR analysis (50-500 neurons)
Deterministic parser          Groq LLM (14,400/day)         Deep analysis (500 neurons)
Local commands (open/close)   Gemini LLM (1,500/day)        Intent classification (5 neurons)
Live mode (keyboard/mouse)    edge-tts (cloud TTS)          Summary generation (50 neurons)
Local STT (Moonshine)         Wikipedia/Wikidata search     Whisper STT fallback (neurons)
Local TTS (Piper)                                           Search synthesis (100 neurons)
NLU (BERT-Mini ONNX)
Brain (Qwen 0.5B)
MCP servers (local processes)
9Router (local gateway)
```

**The golden rule: Tier 0 first, Tier 1 second, Tier 2 last resort.**

NEXUS already follows this for AI calls (Gemini → Groq → Cloudflare cascade).
But the Worker is still used for things that could be done locally.

---

## 2. Existing Features — Full Classification

### Tier 0: Fully Local (No Worker, No Internet, No Neurons)

These features work **completely offline**. They never touch the Worker.

| Feature | Where | RAM | Latency | Worker Needed? |
|---------|-------|-----|---------|----------------|
| Wake word detection | `wakeword_oww.rs` (openWakeWord ONNX) | ~50 MB | <50ms | ❌ No |
| Deterministic intent parsing | `intent_parser.rs` (regex) | 0 MB | <1ms | ❌ No |
| Open app | `local_command.rs` (ShellExecute) | 0 MB | <5ms | ❌ No |
| Close app | `local_command.rs` (taskkill) | 0 MB | <5ms | ❌ No |
| Open URL | `local_command.rs` (ShellExecute) | 0 MB | <5ms | ❌ No |
| Media controls (play/pause/next/prev) | `local_command.rs` (keybd_event) | 0 MB | <5ms | ❌ No |
| Greetings ("hey nexus", "hello") | `intent_parser.rs` | 0 MB | <1ms | ❌ No |
| Open settings | `local_command.rs` | 0 MB | <5ms | ❌ No |
| WhatsApp deep link open | `local_command.rs` (wa.me URL) | 0 MB | <5ms | ❌ No |
| Live mode — type text | `live/commands/keyboard.rs` (enigo) | 0 MB | <10ms | ❌ No |
| Live mode — press keys/hotkeys | `live/commands/keyboard.rs` (enigo) | 0 MB | <5ms | ❌ No |
| Live mode — browser new tab/navigate | `live/commands/browser.rs` | 0 MB | <10ms | ❌ No |
| Live mode — window focus | `live/commands/window.rs` (AttachThreadInput) | 0 MB | <5ms | ❌ No |
| Live mode — WhatsApp send flow | `live/commands/whatsapp.rs` | 0 MB | <500ms | ❌ No |
| Live mode — safety/confirmation gates | `live/safety.rs` | 0 MB | <1ms | ❌ No |
| Local STT (Moonshine) | `server/stt_server.py` (port 39217) | ~150 MB | ~165ms | ❌ No |
| Local TTS (Piper) | `tts.rs` (offline voice) | ~80 MB | ~200ms | ❌ No |
| NLU fallback (BERT-Mini) | `server/nlu_server.py` (port 39218) | ~80 MB | ~20ms | ❌ No |
| Brain (Qwen 0.5B) | `server/brain_server.py` (port 39219) | ~500 MB | ~300ms | ❌ No |
| Acknowledgement phrases ("On it sir") | `orchestrator.rs` (cached audio) | 0 MB | <5ms | ❌ No |
| Contacts lookup | `contacts.json` (local file) | 0 MB | <1ms | ❌ No |
| State machine (live mode) | `live/state.rs` | 0 MB | <1ms | ❌ No |

**Total Tier 0 RAM: ~810 MB** (wake word + STT + TTS + NLU + brain, all lazy-loaded)

### Tier 1: Free External (Internet Needed, 0 Neurons)

These features use the internet but **never consume Cloudflare neurons**.
They use free external APIs (Groq, Gemini, edge-tts, Wikipedia).

| Feature | Where | External API | Free Limit | Worker Needed? | Neurons? |
|---------|-------|-------------|------------|----------------|----------|
| Cloud STT (Groq Whisper) | `stt_groq.rs` | Groq | 20 RPM, 2,000 RPD | ❌ No | 0 |
| Cloud TTS (edge-tts) | `tts.rs` | Microsoft Edge | Unlimited | ❌ No | 0 |
| Research/search synthesis | Worker → `external_llm.ts` | Gemini → Groq → CF | 1,500 + 14,400/day | ✅ Yes (router) | 0 (if Gemini/Groq work) |
| Architecture enrichment | Worker → `external_llm.ts` | Gemini → Groq → CF | 1,500 + 14,400/day | ✅ Yes (router) | 0 (if Gemini/Groq work) |
| Wikipedia/Wikidata search | Worker → `research.ts` | Wikipedia REST | Unlimited | ✅ Yes (router) | 0 |
| General Q&A | Worker → `external_llm.ts` | Gemini → Groq → CF | 1,500 + 14,400/day | ✅ Yes (router) | 0 (if Gemini/Groq work) |

**Key insight:** The Worker is used as a *router* for Tier 1, but it doesn't
consume neurons when Gemini or Groq succeed. Cloudflare Workers AI is only
the **fallback** when both free externals fail.

### Tier 2: Cloudflare Workers AI (Uses Neurons, $5/mo Plan)

These features **consume neurons** from the 10,000/day free allocation.

| Feature | Where | Model | Neurons/call | When Used | Avoidable? |
|---------|-------|-------|-------------|-----------|------------|
| Intent classification | Worker `classifyIntent()` | llama-3.2-1b | ~5 | Every Worker request | ⚠️ Partially — deterministic parser handles most locally |
| PR analysis (standard) | Worker `analysePr()` | glm-4.7-flash | ~50 | PR review | ⚠️ Could route to Gemini/Groq |
| PR analysis (deep) | Worker `analysePr()` | glm-5.3-flash | ~500 | Re-evaluation, large PR | ⚠️ Could route to Gemini |
| Summary generation | Worker `summarize()` | mistral-small | ~50 | Spoken summary | ⚠️ Could route to Groq |
| Search synthesis (fallback) | Worker `synthesize()` | llama-3.2-3b | ~100 | When Gemini+Groq fail | ✅ Only as last resort |
| Architecture enrichment (fallback) | Worker `architectEnrich()` | llama-3.2-3b | ~100 | When Gemini+Groq fail | ✅ Only as last resort |
| Whisper STT on Worker | Worker `/stt` | @cf/openai/whisper | ~50 | Audio upload to Worker | ✅ Use Groq STT instead |
| General Q&A (fallback) | Worker `general()` | llama-3.2-3b | ~100 | When Gemini+Groq fail | ✅ Only as last resort |

### Worker-Only (No Neurons, But Worker Required)

These features need the Worker's D1 database, OAuth, or KV — but **0 neurons**.

| Feature | Where | What It Uses | Neurons? | Avoidable? |
|---------|-------|-------------|----------|------------|
| User/device registration | Worker `POST /api/register` | D1 database | 0 | ❌ No — needs central DB |
| OAuth URL generation | Worker `GET /oauth/auth-url` | OAuth config | 0 | ❌ No — needs OAuth secrets |
| OAuth code exchange | Worker `POST /oauth/exchange` | Google/GitHub OAuth | 0 | ❌ No — needs client secrets |
| OAuth status check | Worker `GET /oauth/status` | D1 database | 0 | ❌ No — needs central DB |
| OAuth disconnect | Worker `DELETE /oauth/disconnect` | D1 database | 0 | ❌ No |
| API key add/remove/list | Worker `/apikeys/*` | D1 + Fernet encryption | 0 | ⚠️ Could be local-only |
| GitHub token fetch | Worker `GET /oauth/github-token` | D1 (cached token) | 0 | ⚠️ Could cache locally |
| Quota tracking | Worker `quota.ts` | D1 `usage_log` | 0 | ⚠️ Could be local |
| Health check | Worker `GET /health` | None | 0 | ❌ No — needed for diagnostics |
| Cache (KV/D1) | Worker `cache.ts` | KV namespace | 0 | ⚠️ Could be local file cache |

---

## 3. The Neuron Budget — Current vs Optimized

### Current Budget (10,000 neurons/day free on $5 plan)

```
Per-user daily limit:  8,000 neurons
Global hard reject:   9,500 neurons
Global warn:          8,000 neurons (switch to cheap model)

Per-user request limit: 150 requests/day
Deep analysis limit:   15/day
Search limit:           50/day
```

### Where Neurons Go Today

```
Intent classification:  150 × 5  = 750 neurons  (EVERY Worker request)
PR analysis (standard): 50 × 50  = 2,500 neurons
PR analysis (deep):     15 × 500 = 7,500 neurons
Summary:                50 × 50  = 2,500 neurons
Search fallback:        10 × 100 = 1,000 neurons  (when Gemini/Groq fail)
Architect fallback:     10 × 100 = 1,000 neurons  (when Gemini/Groq fail)
─────────────────────────────────────────────────────────────────
Total per user/day:               ~15,250 neurons  ← EXCEEDS 10K!
```

**Problem:** A single heavy user can blow through the 10K daily budget.
With 5 users, it's impossible without optimization.

### Optimized Budget (After This Plan)

```
Intent classification:  0  (deterministic parser + local NLU handle it)
PR analysis:            0  (route to Gemini/Groq, Worker just forwards)
Summary:                0  (route to Groq)
Search fallback:        10 × 100 = 1,000 neurons  (ONLY when externals fail)
Architect fallback:     10 × 100 = 1,000 neurons  (ONLY when externals fail)
General Q&A fallback:   10 × 100 = 1,000 neurons  (ONLY when externals fail)
─────────────────────────────────────────────────────────────────
Total per user/day:               ~3,000 neurons
Total for 5 users/day:            ~15,000 neurons  ← Still over 10K
```

**With 9Router offload** (route AI to Groq/Gemini/Cerebras directly from local,
bypassing the Worker entirely for AI):

```
Worker neurons used:    0  (Worker only for OAuth, D1, API keys)
9Router handles:        All AI calls → Groq/Gemini/Cerebras (free, external)
─────────────────────────────────────────────────────────────────
Total Worker neurons/day: 0
```

**This is the target: Worker for identity/database, 9Router for AI.**

---

## 4. Feature Classification Matrix

### A. Features That Don't Need the Worker AT ALL

These can work with the Worker completely offline/dead.

| # | Feature | How | Status |
|---|---------|-----|--------|
| 1 | Wake word | Local ONNX | ✅ Exists |
| 2 | STT | Groq direct (bypass Worker) or local Moonshine | ✅ Exists |
| 3 | TTS | edge-tts direct or local Piper | ✅ Exists |
| 4 | Open/close app | Rust ShellExecute | ✅ Exists |
| 5 | Open URL | Rust ShellExecute | ✅ Exists |
| 6 | Media controls | Rust keybd_event | ✅ Exists |
| 7 | Greetings | Rust regex | ✅ Exists |
| 8 | Settings open | Rust | ✅ Exists |
| 9 | WhatsApp deep link | Rust wa.me URL | ✅ Exists |
| 10 | Live mode (keyboard) | Rust enigo | ✅ Exists |
| 11 | Live mode (browser) | Rust | ✅ Exists |
| 12 | Live mode (window focus) | Rust AttachThreadInput | ✅ Exists |
| 13 | Live mode (WhatsApp flow) | Rust | ✅ Exists |
| 14 | Deterministic intent parsing | Rust regex | ✅ Exists |
| 15 | NLU fallback | Local BERT-Mini ONNX | ✅ Exists |
| 16 | Brain (intent classification) | Local Qwen 0.5B | ✅ Exists |
| 17 | Contacts lookup | Local JSON file | ✅ Exists |
| 18 | Clipboard read/write | Rust arboard | ✅ Exists |
| 19 | State machine | Rust | ✅ Exists |
| 20 | Safety/confirmation gates | Rust | ✅ Exists |
| 21 | MCP servers (future) | Local processes | 🔲 Planned |
| 22 | 9Router (future) | Local gateway | 🔲 Planned |
| 23 | MCPJungle (future) | Local Go binary | 🔲 Planned |
| 24 | Amazon search (future) | Local MCP server → Amazon | 🔲 Planned |
| 25 | Swiggy/Zepto/Blinkit (future) | Local MCP servers → APIs | 🔲 Planned |
| 26 | WhatsApp messaging (future) | Local MCP server | 🔲 Planned |
| 27 | YouTube search (future) | Local MCP server | 🔲 Planned |
| 28 | Email (future) | Local MCP server | 🔲 Planned |
| 29 | GitHub operations (future) | Local octocrab (already in Rust) | 🔲 Partial |
| 30 | Calendar (future) | Local MCP server | 🔲 Planned |
| 31 | Spotify (future) | Local MCP server | 🔲 Planned |
| 32 | Browser automation (future) | Local Playwright MCP | 🔲 Planned |
| 33 | Memory/routines (future) | Local JSON state file | 🔲 Planned |
| 34 | Reminders (future) | Local JSON state file | 🔲 Planned |
| 35 | Product comparison (future) | Local MCP → QuickCommerce API | 🔲 Planned |
| 36 | Movie tickets (future) | Local MCP → BookMyShow/District | 🔲 Planned |
| 37 | Spline 3D (future) | Local MCP → Spline desktop | 🔲 Planned |
| 38 | Whole-laptop control (future) | Local Playwright + Lodestone MCP | 🔲 Planned |

### B. Features That Need the Worker (No Neurons)

These need the Worker's D1 database, OAuth secrets, or central storage.
They consume **0 neurons** but require the Worker to be online.

| # | Feature | Why Worker Needed | Neurons | Status |
|---|---------|-------------------|---------|--------|
| 1 | User registration | Central D1 database | 0 | ✅ Exists |
| 2 | Device registration | Central D1 database | 0 | ✅ Exists |
| 3 | Google OAuth (Gmail, Calendar) | OAuth client secret | 0 | ✅ Exists |
| 4 | GitHub OAuth | OAuth client secret | 0 | ✅ Exists |
| 5 | OAuth status/disconnect | D1 database | 0 | ✅ Exists |
| 6 | API key storage (encrypted) | D1 + Fernet | 0 | ✅ Exists |
| 7 | GitHub token cache | D1 database | 0 | ✅ Exists |
| 8 | Quota tracking | D1 `usage_log` | 0 | ✅ Exists |
| 9 | Health check | Worker endpoint | 0 | ✅ Exists |
| 10 | Multi-user identity (future) | D1 per-user records | 0 | 🔲 Planned |
| 11 | Per-user credential vault (future) | D1 + encryption | 0 | 🔲 Planned |
| 12 | Audit log (future) | D1 database | 0 | 🔲 Planned |
| 13 | Cross-device sync (future) | D1 database | 0 | 🔲 Planned |
| 14 | Shared MCP access control (future) | D1 + MCPJungle tokens | 0 | 🔲 Planned |

### C. Features That Need Workers AI (Neurons) — Compulsory

These **must** use Cloudflare Workers AI. There is no free external alternative,
or the alternative is impractical.

| # | Feature | Why Compulsory | Neurons | Status |
|---|---------|---------------|---------|--------|
| 1 | Fallback when Gemini+Groq fail | Last resort AI | 100/call | ✅ Exists |
| 2 | Whisper STT on Worker (audio upload) | No Groq key configured | ~50/call | ✅ Exists |
| 3 | Intent classification (Worker-side) | When local parser+NLU fail | 5/call | ✅ Exists |

**That's it. Only 3 features truly need neurons, and all are fallbacks.**

### D. Features That COULD Use Neurons But Shouldn't

These currently use neurons but **can and should be redirected** to free externals.

| # | Feature | Current | Should Be | Neurons Saved |
|---|---------|---------|-----------|---------------|
| 1 | PR analysis (standard) | glm-4.7-flash (50) | Gemini/Groq via 9Router | 50/call |
| 2 | PR analysis (deep) | glm-5.3-flash (500) | Gemini 1M context | 500/call |
| 3 | Summary generation | mistral-small (50) | Groq Llama 3.3 70B | 50/call |
| 4 | Search synthesis | llama-3.2-3b (100) | Already cascades to Gemini/Groq | 100/call (when they work) |
| 5 | Architecture enrichment | llama-3.2-3b (100) | Already cascades to Gemini/Groq | 100/call (when they work) |
| 6 | General Q&A | llama-3.2-3b (100) | Already cascades to Gemini/Groq | 100/call (when they work) |
| 7 | Intent classification (Worker) | llama-3.2-1b (5) | Local deterministic + NLU | 5/call |

---

## 5. The Target Architecture

```
                         ┌─────────────────────┐
                         │   USER'S DEVICE      │
                         │  (each of 5 users)   │
                         │                     │
   Voice ──► Wake Word ──►│                     │
                         │  ┌───────────────┐   │
                         │  │ Deterministic  │   │ ◄── Tier 0 (0 cost)
                         │  │ Parser (regex) │   │
                         │  └───────┬───────┘   │
                         │          │ no match   │
                         │  ┌───────▼───────┐   │
                         │  │ NLU (BERT-Mini)│   │ ◄── Tier 0 (0 cost)
                         │  └───────┬───────┘   │
                         │          │ no match   │
                         │  ┌───────▼───────┐   │
                         │  │ Brain (0.5B)  │   │ ◄── Tier 0 (0 cost)
                         │  │ + function    │   │
                         │  │   calling     │   │
                         │  └──┬───┬───┬───┘   │
                         │     │   │   │       │
              ┌──────────┼─────┘   │   └───────┼──────────┐
              │          │         │           │          │
              ▼          ▼         ▼           ▼          ▼
         ┌─────────┐ ┌────────┐ ┌───────┐ ┌────────┐ ┌─────────┐
         │ MCP     │ │ 9Router│ │ Local │ │Worker  │ │ MCP     │
         │ Servers │ │ (local)│ │ State │ │ (D1)   │ │Jungle   │
         │(local)  │ │        │ │ (JSON)│ │        │ │(local)  │
         └────┬────┘ └───┬────┘ └───────┘ └───┬────┘ └────┬────┘
              │          │                     │           │
              │          │ Tier 1 (0 neurons) │           │ Tier 2 (neurons)
              │          ▼                     │           │
              │     ┌─────────┐                │           │
              │     │ Groq    │ ◄── 14,400/day │           │
              │     │ Gemini  │ ◄── 1,500/day  │           │
              │     │ Cerebras│ ◄── 1M tok/day │           │
              │     └─────────┘                │           │
              │                                ▼           │
              │                    ┌─────────────────────┐ │
              │                    │ Cloudflare Workers  │ │
              │                    │ AI (FALLBACK ONLY)  │ │
              │                    │ 10K neurons/day     │ │
              │                    └─────────────────────┘ │
              │                                            │
              ▼                                            ▼
    ┌──────────────────────────────────────────────────────┐
    │              EXTERNAL SERVICES                        │
    │  Amazon, Swiggy, Zepto, Blinkit, WhatsApp,           │
    │  YouTube, Gmail, GitHub, Spotify, BookMyShow...      │
    │  (via MCP servers, user's own accounts)              │
    └──────────────────────────────────────────────────────┘
```

### The Flow

```
1. User speaks
2. Wake word (local) → STT (Groq free or local) → transcript
3. Deterministic parser (local, <1ms) → matches? → execute locally
4. NLU BERT-Mini (local, ~20ms) → matches? → execute locally
5. Brain Qwen 0.5B (local, ~300ms) → classifies task type
6. Brain decides:
   a. Simple/local → execute locally (Tier 0)
   b. Needs AI reasoning → 9Router → Groq/Gemini/Cerebras (Tier 1, 0 neurons)
   c. Needs MCP tool → MCPJungle → MCP server → external service (Tier 0)
   d. Needs database/OAuth → Worker D1 (Tier 0 neurons)
   e. Everything failed → Worker → Cloudflare Workers AI (Tier 2, neurons)
```

---

## 6. What's Compulsory (Must Use Worker)

These features **cannot work without the Worker** — they need a central
database, OAuth secrets, or server-side encryption.

| Feature | Why Compulsory | Neurons |
|---------|----------------|---------|
| **User identity** | Central D1 database needed for 5 users | 0 |
| **Device registration** | Central D1 database | 0 |
| **Google OAuth** | Client secret must stay server-side | 0 |
| **GitHub OAuth** | Client secret must stay server-side | 0 |
| **API key vault** | Fernet encryption key stays server-side | 0 |
| **Cross-device sync** | Central D1 database | 0 |
| **Audit log** | Central D1 database | 0 |
| **Workers AI fallback** | When all free externals fail | 100/call |

**Only the last one uses neurons. Everything else is 0-neuron D1/OAuth work.**

---

## 7. What's Optional (Can Bypass Worker)

These features **can work without the Worker** — they should be designed
to run locally and only use the Worker when explicitly needed.

| Feature | Local Alternative | When to Use Worker |
|---------|------------------|-------------------|
| STT | Groq direct (client-side) or Moonshine | Never need Worker |
| TTS | edge-tts direct or Piper | Never need Worker |
| Intent parsing | Deterministic + NLU (local) | Never need Worker |
| AI reasoning | 9Router → Groq/Gemini/Cerebras | Only as last-resort fallback |
| GitHub operations | octocrab (Rust, local) | Only for token fetch |
| MCP tool calls | Local MCP servers | Never need Worker |
| Shopping/food | Local MCP servers → external APIs | Never need Worker |
| Memory/routines | Local JSON state file | Sync via Worker (optional) |
| Reminders | Local JSON state file | Sync via Worker (optional) |
| WhatsApp | Local MCP server | Never need Worker |
| YouTube | Local MCP server | Never need Worker |
| Email | Local MCP server (Gmail OAuth) | OAuth via Worker |
| Calendar | Local MCP server (Google OAuth) | OAuth via Worker |
| Spotify | Local MCP server | Never need Worker |
| Browser automation | Local Playwright MCP | Never need Worker |
| Spline 3D | Local MCP → Spline desktop | Never need Worker |

---

## 8. Future Features — Classification

### Future Features That Don't Need the Worker

| Feature | How | Cost |
|---------|-----|------|
| 9Router provider discovery | Local scan of provider registries | $0 |
| 9Router key rotation | Local 9Router API | $0 |
| 9Router health monitoring | Local 9Router API | $0 |
| MCPJungle management | Local Go binary | $0 |
| Amazon product search | Local MCP → Amazon scraping | $0 |
| Amazon cart/checkout | Local MCP → browser | $0 |
| Swiggy food order | Local MCP → Swiggy API | $0 |
| Swiggy Instamart | Local MCP → Swiggy API | $0 |
| Zomato order | Local MCP → Zomato API | $0 |
| Zepto order | Local MCP → Zepto API | $0 |
| Blinkit order | Local MCP → Blinkit API | $0 |
| BigBasket search | Local MCP → QuickCommerce API | $0 |
| BookMyShow search | Local MCP → web scraping | $0 |
| District movies/events | Local MCP → District API | $0 |
| QuickCommerce comparison | Local MCP → QuickCommerce API | $0 |
| WhatsApp messaging | Local MCP → WhatsApp Web | $0 |
| YouTube search | Local MCP → YouTube internal API | $0 |
| GitHub operations | Local octocrab (already in Rust) | $0 |
| Spotify control | Local MCP → Spotify API | $0 (Premium for playback) |
| Browser automation | Local Playwright MCP | $0 |
| Spline 3D design | Local MCP → Spline desktop | $0 |
| Whole-laptop control | Local Playwright + Lodestone MCP | $0 |
| Memory/routines | Local JSON state file | $0 |
| Reminders | Local JSON state file | $0 |
| Clipboard integration | Rust arboard (already exists) | $0 |
| Confirmation gates | Rust safety layer (already exists) | $0 |
| Multi-step task planning | Local brain + 9Router → cloud free models | $0 |
| Provider research | Local scan + 9Router | $0 |
| Key validation | Local 9Router API | $0 |
| Quota tracking | Local 9Router dashboard | $0 |

### Future Features That Need the Worker (0 Neurons)

| Feature | Why | Cost |
|---------|-----|------|
| Multi-user identity | D1 per-user records | $0 (D1 free) |
| Per-user credential vault | D1 + Fernet encryption | $0 (D1 free) |
| Cross-device memory sync | D1 database | $0 (D1 free) |
| Cross-device reminder sync | D1 database | $0 (D1 free) |
| Audit log | D1 database | $0 (D1 free) |
| MCPJungle access tokens | D1 + MCPJungle enterprise mode | $0 |
| Per-user MCP permissions | D1 database | $0 (D1 free) |
| Gmail OAuth | Worker holds client secret | $0 |
| Google Calendar OAuth | Worker holds client secret | $0 |
| GitHub OAuth | Worker holds client secret | $0 |

### Future Features That Need Neurons (Last Resort Only)

| Feature | When | Neurons |
|---------|------|---------|
| AI fallback | Gemini + Groq + Cerebras all fail | ~100/call |
| Deep PR analysis | Gemini 1M context insufficient | ~500/call |
| Whisper STT | No Groq key, no local STT | ~50/call |

---

## 9. The 5-User Neuron Budget (Optimized)

### Scenario: 5 users, moderate usage, 9Router offload

```
Per user per day:
  Local commands (open/close/media):     30 calls  → 0 neurons
  STT (Groq direct):                      50 calls  → 0 neurons
  TTS (edge-tts):                        50 calls  → 0 neurons
  AI reasoning (9Router → Groq/Gemini):  20 calls  → 0 neurons
  MCP tool calls (shopping/food/etc):    15 calls  → 0 neurons
  GitHub operations (local octocrab):    5 calls   → 0 neurons
  Worker D1 (OAuth, keys, quota):        10 calls  → 0 neurons
  Worker AI fallback (all externals dead): 2 calls → 200 neurons

Per user per day: 200 neurons
5 users per day:  1,000 neurons  ← WELL UNDER 10,000 free limit
```

### Scenario: 5 users, heavy usage, no 9Router

```
Per user per day:
  Worker AI calls (no 9Router, Worker handles all AI): 50 calls
  Average neurons per call: 100
  Per user per day: 5,000 neurons
  5 users per day: 25,000 neurons  ← EXCEEDS 10K, would cost ~$0.16/day extra
```

### Scenario: 5 users, light usage, no 9Router

```
Per user per day:
  Worker AI calls: 20 calls
  Average neurons per call: 100
  Per user per day: 2,000 neurons
  5 users per day: 10,000 neurons  ← EXACTLY at free limit (risky)
```

### Conclusion

**With 9Router offload: 5 users cost ~1,000 neurons/day = $0/month**
**Without 9Router: 5 users cost ~10,000-25,000 neurons/day = $0-$1.65/month**

**9Router is the key to staying 100% free with 5 users.**

---

## 10. Implementation Priority

### Phase 1: Optimize Existing (0 new features, just redirect)

1. **Route PR analysis to 9Router** instead of Worker AI
   - Worker forwards to 9Router → 9Router → Groq/Gemini
   - Saves 50-500 neurons per PR analysis

2. **Route summary to 9Router** instead of Worker AI
   - Worker forwards to 9Router → Groq
   - Saves 50 neurons per summary

3. **Skip Worker intent classification** when deterministic parser matches
   - Already partially done (keyword fallback first)
   - Make it 100% — never call Worker if local parser succeeds

4. **Skip Worker for STT** — always use Groq direct or local
   - Worker Whisper should never be reached

### Phase 2: Add 9Router (local AI gateway)

5. **Install 9Router locally** (port 20128)
6. **Configure free providers** (Groq, Gemini, Cerebras, OpenRouter)
7. **Point brain at 9Router** instead of Worker for AI calls
8. **Worker becomes identity/database only** — 0 neurons in normal operation

### Phase 3: Add MCP Layer (local tools)

9. **Install MCPJungle** (local Go binary)
10. **Register MCP servers** (WhatsApp, YouTube, Amazon, Swiggy, etc.)
11. **Wire brain function-calling to MCPJungle**
12. **Add confirmation gates** for destructive MCP actions

### Phase 4: Add Commerce/Life Features

13. **Amazon MCP** — search, images, prices, cart
14. **Swiggy/Zepto/Blinkit MCP** — food, groceries
15. **BookMyShow/District MCP** — movies, events
16. **Memory/routines** — "order my regular drink"
17. **Multi-step planning** — brain chains MCP calls

### Phase 5: Multi-User

18. **Per-user identity in D1** (Worker, 0 neurons)
19. **Per-user credential vault** (Worker D1 + Fernet, 0 neurons)
20. **MCPJungle enterprise mode** — per-user access tokens
21. **Cross-device sync** — reminders/memory via D1 (0 neurons)

---

## 11. The Guarantee

### What's 100% Free (0 neurons, 0 cost)

- All local commands (open/close/media/greetings)
- All live mode (keyboard, browser, WhatsApp flow)
- STT (Groq free tier or local Moonshine)
- TTS (edge-tts or local Piper)
- Intent parsing (deterministic + NLU)
- Brain (Qwen 0.5B local)
- 9Router (local, open source)
- MCPJungle (local, open source)
- All MCP servers (local, open source)
- All shopping/food/movie features (via MCP, user's own accounts)
- AI reasoning (Groq 14,400/day + Gemini 1,500/day + Cerebras 1M tokens/day)
- Worker D1/OAuth/API keys (0 neurons)
- 5 users (with 9Router offload: ~1,000 neurons/day, well under 10K)

### What Costs Money (Unavoidable)

- Cloudflare Workers Paid plan: **$5/month** (required for D1, KV, custom domain)
- Workers AI overage (only if 9Router fails AND free externals fail): **~$0.011/1K neurons**
- Spotify Premium (for playback control): **₹119/month** (optional, search is free)
- Real purchases (Amazon, Swiggy, Zepto, Blinkit, BookMyShow): **actual product cost**
- Delivery fees, taxes, surge pricing: **charged by the platform**

### What's 100% Free with 9Router

```
Monthly cost for 5 users:
  Cloudflare Workers Paid:  $5.00
  Workers AI overage:       $0.00 (with 9Router, ~1K neurons/day << 10K free)
  9Router:                 $0.00 (open source, local)
  MCPJungle:                $0.00 (open source, local)
  All MCP servers:          $0.00 (open source, local)
  Groq/Gemini/Cerebras:     $0.00 (free tiers)
  ─────────────────────────────────
  Total:                   $5.00/month for 5 users
  Per user:                 $1.00/month
```

**Without 9Router (Worker handles all AI):**
```
Monthly cost for 5 users:
  Cloudflare Workers Paid:  $5.00
  Workers AI overage:       $0-$1.65/month (depends on usage)
  ─────────────────────────────────
  Total:                   $5.00-$6.65/month for 5 users
  Per user:                 $1.00-$1.33/month
```

---

## 12. Summary — The One-Line Answer

**The Worker is compulsory only for identity, OAuth, and database — 0 neurons.
All AI reasoning should go through 9Router to free external providers — 0 neurons.
Cloudflare Workers AI is the last-resort fallback only — minimal neurons.
With 9Router, 5 users cost $5/month total (just the Cloudflare plan, 0 AI overage).**
