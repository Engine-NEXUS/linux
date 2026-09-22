# 12 — Command Center Architecture (n8n-Style Orchestration)

> **User's request:** "A main command center similar to the n8n flow.
> The main command center divides each task to its own sub-center,
> organizing the implementation backend into one state. Main center
> processes, decides which sub-center to send to, and the sub-center
> proceeds."

---

## 1. How n8n Works (The Pattern We're Borrowing)

n8n is a workflow automation engine. Its core architecture:

```
n8n Execution Engine:
  1. TRIGGER → A request enters (manual, webhook, schedule, sub-workflow)
  2. PLANNER → An AI Agent node reads the task and decides the plan
  3. ROUTER → The planner emits routing JSON: which workers to call
  4. WORKERS → Each "Execute Sub-workflow" node calls a specialist worker
  5. MERGE → Results from all workers are merged into one payload
  6. RESPONSE → The merged result is returned to the caller

Key n8n concepts we borrow:
  - Orchestrator-Worker pattern: one planner, many specialist workers
  - Sub-workflows: each worker is independently testable and swappable
  - Routing JSON: the planner decides at runtime which workers to call
  - State machine: every execution has a lifecycle (start → running → done/error)
  - Contracts: each worker validates inputs and returns {ok, data, error, metadata}
  - Run summary: the parent maintains a summary of which stage succeeded/failed
```

### The n8n Orchestrator-Worker Pattern (from production guidance)

```
1. A parent workflow owns the run ID and correlation ID
2. Each child workflow has ONE clear responsibility
3. Each child returns a predictable shape: {ok, data, error, metadata}
4. Each child validates its inputs at the start
5. The parent decides: retry, skip, or route to manual review on failure
6. Every external side effect writes a receipt before/after the action
7. Child workflows are independently testable with pinned input data
8. The parent maintains a run summary so you can see which stage failed
```

**This is exactly what NEXUS needs.** The current orchestrator (`orchestrator.rs`)
already does routing but it's flat — every subsystem is at the same level.
The new architecture adds a **hierarchical command center** with sub-centers.

---

## 2. The Current NEXUS Orchestrator (What Exists)

From `src-tauri/src/orchestrator.rs`:

```
Current architecture (FLAT):
  Transcript → Orchestrator → [LocalCommand | WorkerBackend | Architect | GitHub]
                                    ↓              ↓             ↓          ↓
                                 <5ms          3-6s           3-6s       1.6s

Problems with the current flat design:
  1. No sub-centers — everything is one level
  2. No multi-step planning — one request = one subsystem
  3. No task state — if a task has 5 steps, they're not tracked as one unit
  4. No chaining — can't do "search Amazon THEN order on Swiggy"
  5. No parallel execution — can't run two sub-centers at once
  6. No retry/skip/fallback — if a subsystem fails, the whole request fails
  7. No run summary — can't see which stage of a multi-step task succeeded
```

---

## 3. The New Command Center Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                    MAIN COMMAND CENTER (The Brain)                   │
│                                                                     │
│  Responsibilities:                                                   │
│  - Receive transcript from STT                                       │
│  - Classify intent (deterministic parser → NLU → brain)             │
│  - Plan the task (single step or multi-step)                         │
│  - Decide which sub-center(s) to invoke                              │
│  - Track task state (one unified state machine)                      │
│  - Emit events to frontend (state, loading, ack, result, done)       │
│  - Handle confirmation gates (ask before destructive actions)       │
│  - Merge results from multiple sub-centers                           │
│  - Maintain run summary (which stage succeeded/failed)               │
│  - Retry, skip, or route to fallback on sub-center failure           │
│                                                                     │
│  This is the NEXUS equivalent of n8n's WorkflowExecute + AI Agent    │
└─────────────────────────────────────────────────────────────────────┘
                              │
                    ┌─────────┼─────────┐
                    │         │         │
                    ▼         ▼         ▼
┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐
│  LOCAL   │ │ CLOUD    │ │ COMMERCE │ │ SOCIAL   │ │ SYSTEM   │
│  SUB-    │ │ AI SUB-  │ │ SUB-     │ │ SUB-     │ │ CONTROL  │
│  CENTER  │ │ CENTER   │ │ CENTER   │ │ CENTER   │ │ SUB-     │
│          │ │          │ │          │ │          │ │ CENTER   │
│ (Rust)   │ │(Worker + │ │(MCP      │ │(MCP      │ │(MCP +    │
│          │ │ 9Router) │ │ Jungle)  │ │ Jungle)  │ │ Rust)    │
└──────────┘ └──────────┘ └──────────┘ └──────────┘ └──────────┘
```

### The Five Sub-Centers

```
1. LOCAL SUB-CENTER (Rust, <5ms, no network)
   Handles: open/close apps, media controls, greetings, URLs, settings
   Tools: intent_parser.rs, command_executor.rs
   RAM: 0 MB
   Latency: <5ms

2. CLOUD AI SUB-CENTER (Worker + 9Router, 1-3s)
   Handles: general questions, reasoning, analysis, PR review, code analysis
   Tools: Worker (Cloudflare), 9Router (Groq/Gemini/Cerebras)
   RAM: 0 MB (cloud) or 500 MB (admin's 9Router)
   Latency: 968ms (9Router) to 3.5s (Worker AI)

3. COMMERCE SUB-CENTER (MCPJungle + MCP servers, 1-5s)
   Handles: Amazon, Swiggy, Zomato, Zepto, Blinkit, BookMyShow, District
   Tools: MCPJungle → MCP servers (Swiggy MCP, Amazon MCP, etc.)
   RAM: 50-100 MB (lazy MCP servers)
   Latency: 1-5s (depends on external API)

4. SOCIAL SUB-CENTER (MCPJungle + MCP servers, 1-3s)
   Handles: WhatsApp, Email, GitHub, YouTube, Spotify, Calendar, Twitter
   Tools: MCPJungle → MCP servers (WhatsApp MCP, Email MCP, etc.)
   RAM: 50-100 MB (lazy MCP servers)
   Latency: 1-3s

5. SYSTEM CONTROL SUB-CENTER (MCP + Rust, 0.1-2s)
   Handles: browser automation, keyboard/mouse, file operations, laptop control
   Tools: Playwright MCP, Lodestone MCP, live/ module (Rust)
   RAM: 50-100 MB (lazy)
   Latency: 0.1-2s
```

---

## 4. The Unified Task State Machine

Every task — whether single-step or multi-step — flows through one
unified state machine. This is the "one state" the user asked for.

```
                    ┌─────────┐
                    │  IDLE   │
                    └────┬────┘
                         │ transcript arrives
                         ▼
                    ┌─────────┐
                    │ PARSING │  ← deterministic parser → NLU → brain
                    └────┬────┘
                         │ intent classified
                         ▼
                    ┌─────────┐
                    │ PLANNING │  ← single step or multi-step plan
                    └────┬────┘
                         │ plan created (list of sub-center calls)
                         ▼
                    ┌─────────┐
                    │ROUTING  │  ← decide which sub-center(s) to invoke
                    └────┬────┘
                         │
              ┌──────────┼──────────┐
              │          │          │
              ▼          ▼          ▼
         ┌────────┐ ┌────────┐ ┌────────┐
         │EXECUT- │ │EXECUT- │ │EXECUT- │  ← sub-centers run (parallel or sequential)
         │ING(1)  │ │ING(2)  │ │ING(n)  │
         └───┬────┘ └───┬────┘ └───┬────┘
             │          │          │
             ▼          ▼          ▼
         ┌─────────────────────────────┐
         │      MERGING RESULTS         │  ← combine outputs from all sub-centers
         └──────────────┬──────────────┘
                        │
                 ┌──────┴──────┐
                 │             │
                 ▼             ▼
            ┌─────────┐   ┌─────────┐
            │CONFIRM? │   │  DONE    │
            │(gate)  │   └─────────┘
            └────┬────┘
                 │ user confirms
                 ▼
            ┌─────────┐
            │EXECUTING│  ← execute confirmed action
            │(final)  │
            └────┬────┘
                 ▼
            ┌─────────┐
            │  DONE   │
            └─────────┘

Error paths (from any state):
  → ERROR (sub-center failed, no fallback)
  → RETRY (sub-center failed, retry with different sub-center)
  → FALLBACK (sub-center failed, use Workers AI as last resort)
```

### Task State Object (The "One State")

```rust
struct TaskState {
    // Identity
    task_id: String,              // UUID for this task
    correlation_id: String,      // links multi-step tasks together

    // Lifecycle
    phase: TaskPhase,             // Idle, Parsing, Planning, Routing,
                                  // Executing, Merging, Confirming, Done, Error
    started_at: Instant,
    completed_at: Option<Instant>,

    // Input
    transcript: String,           // what the user said
    intent: Option<String>,       // classified intent
    slots: HashMap<String, String>, // extracted parameters

    // Plan
    plan: Vec<PlanStep>,          // ordered list of sub-center calls
    current_step: usize,         // which step we're on

    // Execution
    sub_center_results: Vec<SubCenterResult>, // results from each step
    pending_confirmation: Option<ConfirmationGate>,

    // Summary
    run_summary: RunSummary,      // which stages succeeded/failed
}

struct PlanStep {
    step_id: usize,
    sub_center: SubCenter,        // Local, CloudAI, Commerce, Social, SystemControl
    tool: String,                 // "swiggy_search", "amazon_product_details", etc.
    params: serde_json::Value,     // parameters for the tool
    depends_on: Vec<usize>,       // which previous steps must complete first
    optional: bool,                // if this fails, can we continue?
}

struct SubCenterResult {
    step_id: usize,
    sub_center: SubCenter,
    ok: bool,
    data: serde_json::Value,       // the result payload
    error: Option<String>,
    latency_ms: u64,
    receipt: Option<Receipt>,      // for external side effects
}

struct RunSummary {
    total_steps: usize,
    completed: usize,
    failed: usize,
    skipped: usize,
    retried: usize,
    stages: Vec<StageSummary>,    // per-step summary
}
```

---

## 5. How the Command Center Decides (Routing)

### The Routing Decision Tree

```
Transcript arrives → Main Command Center processes:

1. DETERMINISTIC PARSE (0.1ms, 0 RAM)
   "open chrome" → Local Sub-Center → execute immediately
   "pause music" → Local Sub-Center → execute immediately
   "hey nexus" → Local Sub-Center → greeting
   If matched: SKIP to execution. No planning needed.

2. NLU CLASSIFICATION (20ms, 80 MB, lazy)
   If deterministic fails → BERT-Mini classifies intent
   "order food" → intent=food_order, slots={platform: swiggy}
   "send whatsapp" → intent=send_message, slots={platform: whatsapp}
   If confidence > 0.7: route to the right sub-center

3. BRAIN PLANNING (300ms, 500 MB, admin only)
   If NLU confidence < 0.7 OR multi-step detected → brain plans
   "check movie tickets then order dinner" →
     plan = [
       {step 1: Commerce Sub-Center, bookmyshow_get_showtimes},
       {step 2: Commerce Sub-Center, swiggy_search_restaurants},
     ]
   "should I buy the Sony headphones?" →
     plan = [
       {step 1: Commerce Sub-Center, amazon_search},
       {step 2: Cloud AI Sub-Center, analyze_purchase_decision},
     ]

4. WORKER PLANNING (1s, 0 neurons, family members)
   If no local brain → Worker → Groq plans the task
   Same output as brain planning, just 700ms slower
```

### Routing Rules (Which Sub-Center)

```
Intent → Sub-Center mapping:

LOCAL (deterministic, <5ms):
  open_app, close_app, open_url, media_*, greeting, open_settings
  → Local Sub-Center (Rust, no network)

CLOUD AI (1-3s):
  general_question, reasoning, analysis, code_review, pr_analysis
  → Cloud AI Sub-Center (9Router → Groq/Gemini, or Worker)

COMMERCE (1-5s):
  food_order, grocery_order, product_search, movie_tickets, compare_prices
  → Commerce Sub-Center (MCPJungle → Swiggy/Amazon/Zepto MCP)

SOCIAL (1-3s):
  send_message, send_email, create_issue, search_youtube, play_music
  → Social Sub-Center (MCPJungle → WhatsApp/Email/GitHub/YouTube MCP)

SYSTEM CONTROL (0.1-2s):
  browser_automation, type_text, press_key, file_operation, window_focus
  → System Control Sub-Center (Playwright MCP + live/ module)
```

---

## 6. Example Flows

### Single-Step: "Open Chrome"

```
User: "open chrome"
  │
  ├── Main Command Center
  │   ├── Parse: "open" + "chrome" → intent=open_app, slot=app_name=chrome
  │   ├── Plan: [{step 1: Local Sub-Center, open_app, {app: "chrome"}}]
  │   ├── Route: → Local Sub-Center
  │   ├── Execute: open_app("chrome") → ok=true, latency=3ms
  │   ├── Merge: "Opened Chrome sir."
  │   └── Done
  │
  Total: 848ms (STT + parse + execute + TTS)
  Sub-centers used: 1 (Local)
  State transitions: Idle → Parsing → Planning → Routing → Executing → Merging → Done
```

### Multi-Step: "Check movie tickets for Goat, then order dinner from Swiggy"

```
User: "check movie tickets for goat then order dinner from swiggy"
  │
  ├── Main Command Center
  │   ├── Parse: multi-step detected (two intents: movie + food)
  │   ├── Plan: [
  │   │     {step 1: Commerce, bookmyshow_get_showtimes, {movie: "goat", city: "bangalore"}},
  │   │     {step 2: Commerce, swiggy_search_restaurants, {location: "koramangala"}},
  │   │   ]
  │   ├── Route: → Commerce Sub-Center (both steps)
  │   ├── Execute step 1: bookmyshow_get_showtimes →
  │   │     ok=true, data={theater: "PVR Forum", time: "7:30 PM", price: 350}
  │   ├── Execute step 2: swiggy_search_restaurants →
  │   │     ok=true, data={restaurants: ["Meghana Foods", "Paradise Biryani", ...]}
  │   ├── Merge: "Goat is playing at PVR Forum at 7:30 PM for ₹350.
  │   │           I also found 3 restaurants near you on Swiggy.
  │   │           Want to book the ticket first?"
  │   ├── Confirm: ask user (booking requires confirmation)
  │   └── Done (waiting for user response)
  │
  Total: ~3s (parallel sub-center calls)
  Sub-centers used: 1 (Commerce, two tools)
  State transitions: Idle → Parsing → Planning → Routing → Executing(1) → Executing(2) → Merging → Confirming → Done
```

### Cross-Center: "Search Sony headphones on Amazon, then ask if it's worth buying"

```
User: "search sony headphones on amazon then tell me if it's worth buying"
  │
  ├── Main Command Center
  │   ├── Parse: multi-step (commerce + reasoning)
  │   ├── Plan: [
  │   │     {step 1: Commerce, amazon_search, {query: "sony headphones"}},
  │   │     {step 2: Cloud AI, analyze_purchase, {product: step1.data, question: "worth buying?"}},
  │   │   ]
  │   ├── Route: → Commerce Sub-Center (step 1)
  │   ├── Execute step 1: amazon_search →
  │   │     ok=true, data={title: "Sony WH-1000XM5", price: 26990, rating: 4.5, image: "..."}
  │   ├── Route: → Cloud AI Sub-Center (step 2, depends on step 1)
  │   ├── Execute step 2: 9Router → Groq →
  │   │     "User asks if Sony WH-1000XM5 at ₹26,990 is worth buying.
  │   │      Context: 4.5 rating, Prime delivery."
  │   │     ok=true, data={answer: "₹26,990 is a good price..."}
  │   ├── Merge: "The Sony WH-1000XM5 is ₹26,990 on Amazon with 4.5 stars.
  │   │           That's a good price — it usually retails at ₹29,990.
  │   │           I'd say go for it sir."
  │   └── Done
  │
  Total: ~2.2s (sequential: commerce 1s + AI 1.2s)
  Sub-centers used: 2 (Commerce + Cloud AI)
  State transitions: Idle → Parsing → Planning → Routing → Executing(1) → Executing(2) → Merging → Done
```

### Parallel: "Send mom a WhatsApp message and start playing Spotify"

```
User: "send mom a whatsapp message saying i'll be late and play spotify"
  │
  ├── Main Command Center
  │   ├── Parse: multi-step (social + social, independent)
  │   ├── Plan: [
  │   │     {step 1: Social, whatsapp_send, {contact: "mom", text: "I'll be late"}},
  │   │     {step 2: Social, spotify_play, {playlist: "default"}},
  │   │   ]
  │   ├── Route: → Social Sub-Center (both steps, PARALLEL)
  │   ├── Execute step 1 & 2 in parallel:
  │   │     whatsapp_send → ok=true (after confirmation gate)
  │   │     spotify_play → ok=true, data={playing: "Daily Mix"}
  │   ├── Merge: "Message sent to mom, and Spotify is playing your Daily Mix sir."
  │   └── Done
  │
  Total: ~2s (parallel execution)
  Sub-centers used: 1 (Social, two tools, parallel)
```

---

## 7. Confirmation Gates (Safety Layer)

The Main Command Center enforces confirmation gates BEFORE any sub-center
executes a destructive action. This is independent of the brain — it's a
deterministic Rust policy.

```
Action Categories:

READ (no confirmation):
  amazon_search, swiggy_search, bookmyshow_get_showtimes
  youtube_search, email_list, github_list_prs
  → Execute immediately

WRITE (confirmation required):
  whatsapp_send, email_send, tweet_post
  swiggy_add_to_cart, amazon_add_to_cart
  github_merge_pr, github_close_pr
  → Ask user: "Should I send this message to mom?"
  → Wait for confirmation
  → Only then execute

DESTRUCTIVE (confirmation + details):
  swiggy_checkout, amazon_checkout, bookmyshow_book_ticket
  github_delete_branch, file_delete
  → Ask user with full details:
    "COD to home address. Cart total ₹120. Proceed to checkout?"
  → Wait for confirmation
  → Only then execute

BLOCKED (never execute):
  banking apps, password managers, crypto wallets
  → "I can't control that application sir."
```

### Confirmation Gate Implementation

```rust
struct ConfirmationGate {
    step_id: usize,
    action_category: ActionCategory,  // Read, Write, Destructive, Blocked
    prompt: String,                    // "Should I send this message to mom?"
    details: serde_json::Value,        // full details for destructive actions
    command: serde_json::Value,         // the serialized command to execute on confirm
    timeout_secs: u64,                  // auto-cancel after 30s
}

enum ActionCategory {
    Read,         // no confirmation
    Write,        // simple confirm
    Destructive,  // detailed confirm
    Blocked,      // never execute
}
```

---

## 8. Error Handling & Fallback (n8n-Style)

Following n8n's production guidance: the parent decides whether a failed
child stops the run, retries, skips, or routes to fallback.

```
Sub-center fails → Main Command Center decides:

1. RETRY with same sub-center
   - Network timeout on Swiggy MCP → retry once
   - If retry fails: go to fallback

2. RETRY with different sub-center
   - Commerce sub-center (Swiggy) fails → try Commerce (Zomato)
   - Cloud AI (9Router → Groq) fails → try Cloud AI (Worker → Gemini)

3. SKIP (if optional)
   - Step was "also check Blinkit prices" (optional) → skip, continue
   - Plan step marked optional=true → skip on failure

4. FALLBACK to Workers AI (last resort)
   - All external providers (Groq, Gemini, Cerebras) fail → Workers AI
   - Uses ~50-500 neurons depending on task

5. ERROR (no fallback available)
   - All options exhausted → report error to user
   - "I couldn't order from Swiggy sir. Want me to try Zomato instead?"
```

### Run Summary (Debugging)

```
After task completes (or fails), the run summary shows:

Task: "order my regular drink from swiggy instamart"
Task ID: a1b2c3d4
Correlation ID: e5f6g7h8

Stages:
  1. Commerce/Swiggy/search → OK (450ms) — found Coolberg Mojito
  2. Commerce/Swiggy/add_to_cart → OK (320ms) — added to cart
  3. Commerce/Swiggy/checkout → CONFIRMED — user confirmed COD
  4. Commerce/Swiggy/place_order → OK (1.2s) — order placed

Summary: 4/4 completed, 0 failed, 0 skipped
Total time: 2.17s
```

---

## 9. The Command Center vs Current Orchestrator

### What Changes

```
CURRENT (flat):
  Orchestrator → [LocalCommand | WorkerBackend | Architect | GitHub]
  - One request = one subsystem
  - No multi-step planning
  - No task state tracking
  - No sub-center abstraction
  - No parallel execution
  - No run summary

NEW (hierarchical, n8n-style):
  Command Center → [Local | CloudAI | Commerce | Social | SystemControl]
  - One request = one or more sub-center calls (a plan)
  - Multi-step planning with dependencies
  - Unified task state machine (one state for the whole task)
  - Sub-center abstraction (each sub-center manages its own tools)
  - Parallel execution for independent steps
  - Run summary for debugging
  - Confirmation gates (deterministic, not brain-dependent)
  - Error handling: retry, skip, fallback, error
```

### What Stays the Same

```
- The orchestrator's event system (state, loading, ack, result, done, error)
- The request ID and cancellation mechanism
- The deterministic parser (intent_parser.rs)
- The NLU server (BERT-Mini)
- The brain (Qwen 0.5B, admin only)
- The STT/TTS pipeline
- The Worker backend
- The existing sub-centers' internal logic (they just get wrapped)
```

---

## 10. Implementation Plan

### Phase 1: Wrap Existing Subsystems as Sub-Centers

```
Current → Sub-Center mapping:
  LocalCommand → Local Sub-Center (already works, just rename)
  WorkerBackend → Cloud AI Sub-Center (already works, add 9Router routing)
  Architect → Cloud AI Sub-Center (merge with WorkerBackend)
  GitHub → Social Sub-Center (move from top-level to sub-center)
  New: Commerce Sub-Center (MCPJungle + commerce MCPs)
  New: Social Sub-Center (MCPJungle + social MCPs)
  New: System Control Sub-Center (live/ + Playwright MCP)
```

### Phase 2: Add Task State Machine

```
- Replace the flat ActiveRequest with TaskState
- Add PlanStep struct (list of sub-center calls)
- Add SubCenterResult struct (results from each step)
- Add RunSummary struct (which stages succeeded/failed)
- Add ConfirmationGate struct (safety layer)
- Implement the state machine: Idle → Parsing → Planning → Routing →
  Executing → Merging → Confirming → Done
```

### Phase 3: Add Multi-Step Planning

```
- Brain (admin) or Worker (family) generates a plan (list of PlanSteps)
- Plan includes dependencies (step 2 depends on step 1)
- Plan includes optionality (step is optional=true/false)
- Command Center executes steps in order (or parallel if independent)
```

### Phase 4: Add Sub-Center Routing

```
- Intent → Sub-Center mapping (deterministic)
- Multi-intent detection (parse "then", "and", "also" as step separators)
- Parallel execution for independent steps
- Sequential execution for dependent steps
```

### Phase 5: Add Confirmation Gates

```
- Action category classification (Read, Write, Destructive, Blocked)
- Confirmation prompt generation
- Wait for user confirmation before executing destructive actions
- Timeout (auto-cancel after 30s)
```

### Phase 6: Add Error Handling & Fallback

```
- Retry logic (retry once with same sub-center)
- Alternative sub-center (Swiggy fails → try Zomato)
- Fallback to Workers AI (last resort)
- Skip optional steps on failure
- Run summary for debugging
```

---

## 11. RAM Impact

```
Current orchestrator: ~1 MB (just Rust state)
New command center:  ~5 MB (TaskState + plan + results + summary)

Sub-centers (lazy, only loaded when needed):
  Local:          0 MB (always loaded, Rust)
  Cloud AI:       0 MB (cloud) or 500 MB (admin's 9Router)
  Commerce:       50-100 MB (MCP servers, lazy)
  Social:         50-100 MB (MCP servers, lazy)
  System Control: 50-100 MB (Playwright MCP, lazy)

Total command center overhead: ~5 MB
Total with one sub-center active: ~55-105 MB
Total with brain + 9Router (admin): ~505-605 MB
```

---

## 12. Latency Impact

```
Single-step commands (no change):
  Current: 848ms (STT + parse + execute + TTS)
  New:    848ms (same — command center adds <1ms overhead)

Multi-step commands (NEW capability):
  "check movies then order food": ~3s (was impossible before)
  "search amazon then analyze": ~2.2s (was impossible before)
  "send whatsapp and play spotify": ~2s parallel (was impossible before)

The command center adds <1ms overhead for routing decisions.
The real latency comes from the sub-centers (network calls, MCP servers).
```

---

## 13. Summary — The Simple Version

### What the User Asked

> "A main command center similar to the n8n flow. The main command center
> divides each task to its own sub-center, organizing the implementation
> backend into one state. Main center processes, decides which sub-center
> to send to, and the sub-center proceeds."

### What We're Building

```
MAIN COMMAND CENTER (the brain/coordinator)
  ├── Receives transcript
  ├── Classifies intent (parser → NLU → brain)
  ├── Plans the task (single or multi-step)
  ├── Routes to sub-center(s)
  ├── Tracks one unified task state
  ├── Enforces confirmation gates
  ├── Merges results
  └── Handles errors (retry, skip, fallback)

FIVE SUB-CENTERS (the workers):
  1. LOCAL (Rust, <5ms) — apps, media, greetings
  2. CLOUD AI (Worker/9Router, 1-3s) — reasoning, analysis, Q&A
  3. COMMERCE (MCPJungle, 1-5s) — Amazon, Swiggy, Zepto, BookMyShow
  4. SOCIAL (MCPJungle, 1-3s) — WhatsApp, Email, GitHub, YouTube, Spotify
  5. SYSTEM CONTROL (MCP + Rust, 0.1-2s) — browser, keyboard, files
```

### The n8n Patterns We Borrow

```
1. Orchestrator-Worker: one planner, many specialist workers
2. Sub-workflows: each sub-center is independently testable
3. Routing JSON: planner decides at runtime which workers to call
4. State machine: every task has a lifecycle (start → running → done/error)
5. Contracts: each sub-center returns {ok, data, error, metadata}
6. Run summary: parent tracks which stage succeeded/failed
7. Error handling: retry, skip, fallback, or error
8. Parallel execution: independent steps run in parallel
```

### What Already Exists vs What's New

| Component | Status |
|-----------|--------|
| Orchestrator (routing, events, cancellation) | ✅ `orchestrator.rs` — working |
| Local sub-center (commands) | ✅ `commands.rs`, `intent_parser.rs` — working |
| Cloud AI sub-center (Worker) | ✅ `network.rs` — working |
| GitHub sub-center | ✅ `github_cmd.rs` — working |
| Task state machine | 🔲 New (replace ActiveRequest with TaskState) |
| Multi-step planning | 🔲 New (brain/Worker generates plan) |
| Sub-center abstraction | 🔲 New (wrap existing as sub-centers) |
| Commerce sub-center | 🔲 New (MCPJungle + commerce MCPs) |
| Social sub-center | 🔲 New (MCPJungle + social MCPs) |
| System control sub-center | 🔲 New (Playwright MCP + live/) |
| Confirmation gates | ✅ Partial (GitHub has it, needs generalization) |
| Error handling (retry/fallback) | 🔲 New |
| Run summary | 🔲 New |

### Cost

```
Command center overhead: ~5 MB RAM, <1ms latency
Sub-centers: lazy-loaded, 50-100 MB each when active
Total: $0 (no new cloud costs)
```

**The command center makes NEXUS capable of multi-step, multi-center
task execution — like n8n, but voice-first and local-first.**
