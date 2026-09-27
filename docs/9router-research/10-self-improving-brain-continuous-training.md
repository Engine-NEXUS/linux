# 10 — Self-Improving Brain: Continuous Training & Update Distribution

> **User's vision:** The admin's brain is always training and getting
> better. BERT-Mini NLU improves over time. Updates propagate to all
> family members automatically. Everything gets faster and more efficient.

---

## 1. What NEXUS Already Has (The Foundation)

### STT Self-Learning (Already Working)

NEXUS already has a self-learning system for STT corrections —
`src-tauri/src/stt_learning.rs`:

```
1. STT mishears "open zync" as "open zink" → parser fails
2. User repeats "open zync" → parser succeeds
3. System compares: "zink" vs "zync" at same position
4. After 3 consistent corrections → auto-apply = true
5. Next time STT says "zink" → auto-corrected to "zync"
6. Stored in: %APPDATA%/com.nexus.assistant/learned_corrections.json
7. RAM cost: ~1-10 KB (in-memory HashMap)
```

**This pattern already works.** We extend it to the brain and NLU.

### BERT-Mini Training Pipeline (Already Exists)

NEXUS already has a full training pipeline — `server/nlu/train.py`:

```
Model:     google/bert_uncased_L-2_H-128_A-2 (BERT-Mini, 4.4M params)
Intents:   58 (local commands, GitHub, live mode, analysis)
Slots:     33 BIO tag types
Dataset:   server/nlu/dataset.json (phrase-family-separated splits)
Training:  50 epochs, batch size 16, LR 5e-5
Export:    ONNX (opset 14, dynamic batch axis)
Bundling:  nexus.mjs syncs model to src-tauri/resources/server/nlu/model/

Commands:
  nexus train           # full pipeline (train + export + sync + audit)
  nexus train --clean-only
  nexus audit           # validate data + model quality
  nexus collect         # collect new voice samples
  nexus data nlu validate  # provenance + evaluation lock
```

**The infrastructure exists.** We need to make it continuous and distributed.

---

## 2. The Self-Improvement Loop (Three Layers)

```
Layer 1: CORRECTION LEARNING (instant, every command)
  "STT misheard" → "user corrected" → "learn the correction"
  Storage: local JSON file (~10 KB)
  RAM: ~10 KB
  Already exists: stt_learning.rs

Layer 2: NLU RETRAINING (weekly, on admin's laptop)
  "NLU misclassified" → "user corrected intent" → "add to dataset" → "retrain"
  Storage: dataset.json + nexus_nlu.onnx (~18 MB)
  RAM: ~80 MB (during inference, 0 during training)
  Already exists: train.py, needs automation

Layer 3: BRAIN PROMPT CORRECTION (daily, on admin's laptop)
  "Brain routed to wrong tool" → "user corrected routing" → "save correction"
  Storage: brain_corrections.json (~50 KB)
  RAM: ~50 KB
  New: extends stt_learning.rs pattern to brain routing
```

---

## 3. Layer 1: Correction Learning (Already Working — Extend to Brain)

### What Already Works (STT Corrections)

```
User says: "open zync"
STT hears: "open zink"
Parser fails → log_failed_transcript("open zink")

User repeats: "open zync"
STT hears: "open zync"
Parser succeeds → log_successful_transcript("open zync")

System compares:
  "open zink" vs "open zync"
  Diff at position 2: zink → zync
  Levenshtein distance: 2 (close enough)
  Context: "open" (word before)

After 3 times:
  auto_apply = true
  Next time STT says "zink" → auto-corrected to "zync"
```

### New: Brain Routing Corrections (Same Pattern)

```
User says: "order my regular drink from swiggy"
Brain routes to: Amazon MCP (wrong)
User says: "no, I said swiggy not amazon"
Brain re-routes to: Swiggy MCP (correct)

System logs:
  transcript: "order my regular drink from swiggy"
  wrong_route: "amazon_mcp"
  correct_route: "swiggy_mcp"
  context: "order", "drink", "regular"

After 3 times:
  auto_apply = true
  Next time brain sees "order" + "drink" → routes to Swiggy directly
  (without trying Amazon first)

Storage: %APPDATA%/com.nexus.assistant/brain_corrections.json
RAM: ~50 KB (in-memory HashMap)
```

### New: NLU Intent Corrections (Same Pattern)

```
User says: "hey nexus, what's the weather"
NLU classifies: search (wrong — should be weather_query)
User says: "no, I want the weather"
NLU re-classifies: weather_query (if NLU has this intent)
  OR
  User corrects: "it's a weather question, not a search"

System logs:
  transcript: "what's the weather"
  wrong_intent: "search"
  correct_intent: "weather_query"
  confidence: 0.45 (low — that's why it was wrong)

After 3 times:
  Add to dataset.json as a new training example
  Flag for next retraining cycle
```

---

## 4. Layer 2: NLU Continuous Retraining

### The Weekly Retraining Cycle (Admin's Laptop)

```
Every Sunday at 3 AM (admin's laptop, idle time):

1. Collect corrections from the past week
   Source: brain_corrections.json + nlu_corrections.json
   Count: typically 5-20 new examples per week

2. Add to dataset.json
   - Validate format (intent + slots + text)
   - Check for duplicates
   - Add to "train" split (not test — test is locked)
   - Run: nexus data nlu validate

3. Retrain BERT-Mini
   Command: nexus train
   Time: ~5-10 minutes (50 epochs, 4.4M params, CPU)
   RAM: ~500 MB during training (PyTorch + dataset)
   Result: new nexus_nlu.onnx + new labels.json + new tokenizer

4. Validate new model
   Command: nexus audit
   Checks:
     - Test accuracy >= previous model's accuracy
     - No intent regression (each intent's F1 >= previous)
     - No slot leakage
     - ONNX export verified

5. If validation passes:
   - Replace local model
   - Upload to Worker D1 (for distribution to family members)
   - Log: "NLU model updated: v2.3.1, accuracy 87.3% (+0.4%)"

6. If validation fails:
   - Keep old model
   - Log: "NLU retrain failed: test accuracy dropped 2.1%"
   - Alert admin: "Sir, the weekly NLU retrain didn't improve accuracy.
     I'm keeping the current model."
```

### Why Weekly, Not Daily?

```
Daily retraining:
  - Only 1-3 new examples per day → too few to improve
  - Training takes 5-10 minutes → disruptive
  - Risk of overfitting to a single day's corrections

Weekly retraining:
  - 5-20 new examples per week → enough to improve
  - Training at 3 AM → no disruption
  - Validation gate prevents regression
  - Admin can review before distribution
```

### What Makes BERT-Mini Better Over Time

```
Week 1:  2,438 examples, 58 intents, accuracy 82%
Week 4:  2,498 examples (+60), 58 intents, accuracy 83.5%
Week 8:  2,578 examples (+80), 58 intents, accuracy 85.1%
Week 12: 2,680 examples (+102), 58 intents, accuracy 86.7%
Week 24: 2,950 examples (+270), 58 intents, accuracy 89.2%

Each week:
  - New phrasings learned ("grab food" = food_order)
  - New app names added ("figma" = open_app)
  - New MCP tool mappings ("order groceries" = swiggy_instamart)
  - Accent/voice adaptations (admin's specific pronunciation)
  - STT mishearing patterns (learned corrections feed back)
```

---

## 5. Layer 3: Brain Prompt Correction (The FreePalp Pattern)

### How the Brain Gets Smarter Without Retraining

The Qwen 0.5B brain doesn't get retrained (too expensive, too slow).
Instead, it gets **prompt corrections** — examples injected into its
system prompt that teach it the right routing.

```
brain_corrections.json:
{
  "corrections": [
    {
      "pattern": "order * drink *",
      "wrong_route": "amazon_mcp",
      "correct_route": "swiggy_instamart_mcp",
      "count": 5,
      "auto_apply": true
    },
    {
      "pattern": "check * movie *",
      "wrong_route": "general_qa",
      "correct_route": "bookmyshow_mcp",
      "count": 3,
      "auto_apply": true
    },
    {
      "pattern": "send * whatsapp *",
      "wrong_route": "general_qa",
      "correct_route": "whatsapp_mcp",
      "count": 8,
      "auto_apply": true
    }
  ]
}
```

### How It Works

```
User says: "order my regular drink"
Brain's system prompt includes:
  "When user says 'order * drink', route to swiggy_instamart_mcp"
Brain routes to: Swiggy Instamart MCP (correct, first try)

Without the correction:
  Brain might route to: Amazon MCP (wrong)
  User corrects: "no, swiggy"
  Brain learns: add to brain_corrections.json
  Next time: routes correctly
```

### RAM Impact

```
brain_corrections.json: ~50 KB (typically 20-100 corrections)
Loaded into memory at brain startup: ~50 KB HashMap
Injected into system prompt: ~200 tokens (out of 4096 context)
RAM cost: negligible (~50 KB)
```

---

## 6. Update Distribution to Family Members

### The Distribution Architecture

```
Admin's Laptop (training hub)
  │
  ├── Weekly: Retrains BERT-Mini
  │   ├── New nexus_nlu.onnx (~18 MB)
  │   ├── New labels.json (~2 KB)
  │   ├── New brain_corrections.json (~50 KB)
  │   └── New learned_corrections.json (~10 KB)
  │
  ├── Uploads to Worker D1
  │   ├── POST /models/nlu (ONNX binary, versioned)
  │   ├── POST /models/corrections (JSON, versioned)
  │   └── POST /models/brain-corrections (JSON, versioned)
  │
  └── Worker stores in R2 (free 10 GB)
      ├── nlu/v2.3.1/nexus_nlu.onnx
      ├── nlu/v2.3.1/labels.json
      ├── corrections/v2.3.1/learned_corrections.json
      └── corrections/v2.3.1/brain_corrections.json

Family Members' Laptops (update clients)
  │
  ├── On startup: GET /models/version
  │   └── Compare local version with server version
  │
  ├── If update available:
  │   ├── Download in background (~18 MB, ~5 seconds)
  │   ├── Verify checksum (SHA-256)
  │   ├── Stage in temp directory
  │   ├── On next idle: swap model files
  │   └── Restart NLU server (15s cooldown)
  │
  └── If no update: continue with current model
```

### Worker Endpoints (New, 0 Neurons)

```
GET  /models/version          → { nlu_version, corrections_version }
GET  /models/nlu/:version     → ONNX binary (from R2)
GET  /models/nlu/:version/meta → { labels.json, tokenizer, checksum }
GET  /models/corrections/:version → JSON (learned + brain corrections)
POST /models/nlu              → Admin uploads new model (auth required)
POST /models/corrections      → Admin uploads new corrections (auth required)

Storage: Cloudflare R2 (free 10 GB)
Cost: 0 neurons (just D1 + R2 reads/writes)
RAM: 0 (serverless, just file serving)
```

### Update Flow (Family Member's Perspective)

```
Family member opens NEXUS at 9 AM:

1. NEXUS checks: GET /models/version
   Server says: nlu_version = "2.3.1"
   Local says:   nlu_version = "2.3.0"
   → Update available!

2. NEXUS downloads in background:
   GET /models/nlu/2.3.1/nexus_nlu.onnx (18 MB, ~3s)
   GET /models/nlu/2.3.1/labels.json (2 KB)
   GET /models/corrections/2.3.1/learned_corrections.json (10 KB)
   GET /models/corrections/2.3.1/brain_corrections.json (50 KB)
   Total: ~18 MB, ~5 seconds

3. Verify checksum:
   SHA-256 matches → proceed
   SHA-256 mismatch → abort, keep old model

4. Stage in temp:
   %APPDATA%/com.nexus.assistant/nlu_staging/
   ├── nexus_nlu.onnx
   ├── labels.json
   └── tokenizer/

5. Wait for idle (no active commands):
   → Swap: move staging files to active directory
   → Kill old NLU server (if running)
   → New NLU server will start on next unparseable command

6. User sees: "NEXUS updated: NLU model v2.3.1 (87.3% accuracy, +0.4%)"
   (Optional: admin can disable this notification)

Total time: ~5 seconds download + instant swap
User disruption: 0 (happens in background, swap on idle)
```

### Version Tracking

```
D1 table: nlu_models
  ├── version (TEXT, e.g. "2.3.1")
  ├── uploaded_by (TEXT, admin user_id)
  ├── uploaded_at (TIMESTAMP)
  ├── accuracy (REAL, e.g. 0.873)
  ├── intent_count (INTEGER, e.g. 58)
  ├── example_count (INTEGER, e.g. 2498)
  ├── checksum (TEXT, SHA-256)
  └── r2_key (TEXT, e.g. "nlu/v2.3.1/nexus_nlu.onnx")

D1 table: correction_versions
  ├── version (TEXT)
  ├── corrections_count (INTEGER)
  ├── brain_corrections_count (INTEGER)
  ├── checksum (TEXT)
  └── r2_key (TEXT)
```

---

## 7. What Gets Better Over Time

### BERT-Mini NLU (Retrained Weekly)

```
What improves:
  - New intents recognized ("order groceries" → swiggy_instamart)
  - New phrasings understood ("grab food" = "order food")
  - New app names added ("figma", "notion", "linear")
  - New MCP tool mappings ("check movies" → bookmyshow)
  - Accent adaptation (admin's pronunciation patterns)
  - STT mishearing compensation (learned corrections feed back)
  - Confidence improves (fewer "unknown" classifications)

What doesn't change:
  - Model architecture (always BERT-Mini, 4.4M params)
  - Model size (always ~18 MB ONNX)
  - RAM usage (always ~80 MB)
  - Latency (always ~20ms)
  - Intent count stays at 58 (unless admin adds new intents)
```

### Brain Corrections (Updated Daily)

```
What improves:
  - Tool routing accuracy (brain picks right MCP first try)
  - Provider selection (brain picks right cloud model for the task)
  - Response phrasing (brain learns preferred response style)
  - Confirmation timing (brain learns when to ask vs. proceed)

What doesn't change:
  - Brain model (Qwen 0.5B, no retraining)
  - Brain RAM (always ~500 MB)
  - Brain latency (always ~300ms)
```

### STT Corrections (Updated Every Command)

```
What improves:
  - Transcription accuracy (misheard words auto-corrected)
  - Accent adaptation (admin's specific pronunciation)
  - App name recognition ("zync" → "zync" not "zink")
  - Technical term recognition (repo names, PR numbers)

What doesn't change:
  - STT model (Groq Whisper, no retraining)
  - STT RAM (0 MB, cloud)
  - STT latency (~247ms)
```

---

## 8. The Complete Self-Improvement Timeline

```
Day 1 (Admin installs NEXUS):
  NLU: 2,438 examples, 82% accuracy
  Brain: 0 corrections
  STT: 0 corrections

Week 1:
  NLU: 2,438 examples (no retrain yet)
  Brain: 5-10 routing corrections learned
  STT: 10-20 word corrections learned
  User experience: "getting to know my voice"

Week 4 (first retrain):
  NLU: 2,498 examples, 83.5% accuracy (+1.5%)
  Brain: 20-40 routing corrections
  STT: 40-80 word corrections
  Family members: receive NLU v2.3.1
  User experience: "noticeably better at understanding me"

Week 12:
  NLU: 2,680 examples, 86.7% accuracy (+4.7%)
  Brain: 60-100 routing corrections
  STT: 120-240 word corrections
  Family members: receive NLU v2.6.0
  User experience: "rarely mishears me now"

Week 24:
  NLU: 2,950 examples, 89.2% accuracy (+7.2%)
  Brain: 100-200 routing corrections
  STT: 200-400 word corrections
  Family members: receive NLU v2.12.0
  User experience: "feels like it knows me"

Week 52 (1 year):
  NLU: 4,000+ examples, 93%+ accuracy
  Brain: 300+ routing corrections (covers most patterns)
  STT: 500+ word corrections (covers admin's vocabulary)
  Family members: receive NLU v3.5.0
  User experience: "rarely makes mistakes"
```

---

## 9. Admin's Control Over Training

### The Admin Dashboard (9Router + Training)

```
Admin can see:
  ├── Current NLU model: v2.3.1, 87.3% accuracy, 2,498 examples
  ├── Pending corrections: 12 new (waiting for Sunday retrain)
  ├── Brain corrections: 47 active, 3 pending
  ├── STT corrections: 156 active, 8 pending
  ├── Family members: 4 connected, all on v2.3.1
  ├── Last retrain: Sunday 3 AM, +0.4% accuracy
  └── Next retrain: Sunday 3 AM

Admin can:
  ├── "Retrain now" → trigger immediate retrain
  ├── "Review corrections" → see what was learned, approve/reject
  ├── "Push update to family" → force distribution (not waiting for Sunday)
  ├── "Rollback model" → revert to previous version if regression
  ├── "Add training example" → manually add to dataset
  ├── "Export dataset" → download dataset.json for backup
  └── "Disable auto-retrain" → switch to manual training only
```

### Safety Gates

```
1. Retrain validation:
   - New model must beat old model on test set
   - No intent regression (each intent F1 >= previous)
   - If fails: keep old model, alert admin

2. Correction review:
   - Admin can review all corrections before they're used
   - Can reject bad corrections ("no, that was a different command")
   - Rejected corrections don't go into the dataset

3. Rollback:
   - Worker keeps last 5 model versions in R2
   - Admin can rollback to any previous version
   - Family members receive rollback on next check

4. Family opt-out:
   - Each family member can disable auto-updates
   - They stay on their current model until they manually update
   - Useful if a new model performs worse for their voice
```

---

## 10. RAM and Cost Impact

### Admin's Laptop (Training Hub)

```
Normal operation (no training):
  NLU: 80 MB (inference only)
  Brain: 500 MB
  Corrections: 60 KB (in-memory)
  Total: ~580 MB

During weekly retrain (Sunday 3 AM):
  PyTorch: ~500 MB (training)
  Dataset: ~50 MB (in memory)
  NLU inference: 80 MB (still serving)
  Total: ~1,130 MB (for 5-10 minutes, then back to 580 MB)

The retrain happens at 3 AM when the laptop is idle.
No user-visible impact.
```

### Family Members' Laptops (Update Clients)

```
Normal operation:
  NLU: 80 MB (inference only)
  Corrections: 60 KB (in-memory)
  Total: ~80 MB (brain layer)

During update (once per week):
  Download: 18 MB (5 seconds, background)
  Verify: ~1 second (SHA-256)
  Swap: ~1 second (file move)
  NLU restart: 15 seconds (cooldown)
  Total disruption: 0 (happens on idle, user doesn't notice)

No training happens on family devices.
No PyTorch needed on family devices.
No extra RAM during update.
```

### Worker Cost

```
R2 storage: ~18 MB per version × 5 versions = 90 MB (free, 10 GB limit)
D1 reads: 1 per family member per startup (version check)
D1 writes: 1 per retrain (admin upload)
Neurons: 0 (no AI calls for distribution)
Cost: $0 (included in $5/month plan)
```

---

## 11. The Complete Self-Improvement Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    ADMIN (Training Hub)                          │
│                                                                 │
│  Every command:                                                 │
│    STT correction → learned_corrections.json (10 KB)           │
│    Brain correction → brain_corrections.json (50 KB)           │
│    NLU correction → nlu_corrections.json (20 KB)               │
│                                                                 │
│  Every Sunday 3 AM:                                             │
│    Collect corrections → add to dataset.json                    │
│    Retrain BERT-Mini → new nexus_nlu.onnx (18 MB)              │
│    Validate → accuracy must improve                             │
│    Upload to Worker R2 → nlu/v2.x.x/nexus_nlu.onnx              │
│    Upload corrections → corrections/v2.x.x/*.json              │
│                                                                 │
│  Admin dashboard:                                               │
│    Review corrections, approve/reject, rollback                │
│                                                                 │
│  RAM: 580 MB normal, 1,130 MB during 5-min retrain              │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ Upload (Sunday 3 AM)
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│              CLOUDFLARE WORKER + R2 (Distribution)              │
│                                                                 │
│  R2 storage:                                                    │
│    nlu/v2.3.1/nexus_nlu.onnx (18 MB)                            │
│    nlu/v2.3.1/labels.json (2 KB)                                │
│    corrections/v2.3.1/learned_corrections.json (10 KB)          │
│    corrections/v2.3.1/brain_corrections.json (50 KB)           │
│    (last 5 versions kept for rollback)                         │
│                                                                 │
│  D1 database:                                                   │
│    nlu_models table (version, accuracy, checksum)              │
│    correction_versions table (version, counts, checksum)       │
│                                                                 │
│  Endpoints (0 neurons):                                          │
│    GET /models/version → version check                          │
│    GET /models/nlu/:version → download ONNX                     │
│    GET /models/corrections/:version → download JSON             │
│    POST /models/nlu → admin upload (auth required)             │
│                                                                 │
│  Cost: $0 (R2 free 10 GB, D1 free 5 GB)                         │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ Download (on startup, background)
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│              FAMILY MEMBERS (Update Clients)                     │
│                                                                 │
│  On startup:                                                    │
│    GET /models/version → compare with local                    │
│    If update: download 18 MB (5s, background)                  │
│    Verify SHA-256 → stage in temp → swap on idle               │
│                                                                 │
│  During use:                                                    │
│    NLU: 80 MB (latest model, same as admin's)                  │
│    Corrections: 60 KB (latest, same as admin's)                │
│    Brain: 0 MB (no LLM, uses Worker for reasoning)             │
│                                                                 │
│  No training. No PyTorch. No extra RAM.                         │
│  Just download and swap.                                        │
│                                                                 │
│  RAM: 80 MB (NLU + corrections)                                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## 12. Summary — The Simple Version

### What the User Asked

> "The admin brain always trained and make the bert mini better and
> updates the family members have them updated and making it faster
> and efficient"

### What Happens

```
EVERY COMMAND (all users):
  STT mishears → user corrects → system learns (10 KB, instant)
  Brain routes wrong → user corrects → system learns (50 KB, instant)
  NLU misclassifies → user corrects → flagged for retraining

EVERY SUNDAY 3 AM (admin's laptop):
  Collect week's corrections → add to dataset
  Retrain BERT-Mini (5-10 min, 500 MB PyTorch)
  Validate: new model must beat old model
  Upload to Worker R2 (18 MB)

EVERY STARTUP (family members):
  Check for update → download 18 MB (5s, background)
  Verify checksum → swap on idle
  Now running the same improved model as admin

RESULT:
  Week 1: 82% accuracy
  Week 12: 87% accuracy
  Week 24: 89% accuracy
  Week 52: 93% accuracy
  Gets better every week, for everyone, automatically.
```

### What Already Exists

| Component | Status |
|-----------|--------|
| STT self-learning | ✅ `stt_learning.rs` — fully working |
| BERT-Mini training pipeline | ✅ `train.py` — fully working |
| ONNX export | ✅ `export_onnx.py` — fully working |
| `nexus train` command | ✅ Full pipeline with audit |
| `nexus collect` command | ✅ Voice sample collection |
| `nexus audit` command | ✅ Model validation |
| Dataset management | ✅ Phrase-family-separated splits |
| Worker D1 database | ✅ Already exists |
| Worker R2 storage | 🔲 Needs setup (free, 10 GB) |
| Brain routing corrections | 🔲 New (extends stt_learning pattern) |
| NLU correction logging | 🔲 New (extends stt_learning pattern) |
| Auto-retrain scheduler | 🔲 New (cron-style, Sunday 3 AM) |
| Model distribution endpoints | 🔲 New (Worker routes, 0 neurons) |
| Family update client | 🔲 New (version check + download + swap) |
| Admin training dashboard | 🔲 New (review + approve + rollback) |

### Cost

```
Training: $0 (admin's laptop, idle time)
Storage: $0 (R2 free 10 GB, model is 18 MB)
Distribution: $0 (Worker, 0 neurons)
Family updates: $0 (download 18 MB once per week)
Total: $0 (included in $5/month Cloudflare plan)
```

### RAM Impact

```
Admin: 580 MB normal + 550 MB during 5-min weekly retrain = 1,130 MB peak
Family: 80 MB (no change — just updated model, same size)
```

**The system gets smarter every week, for everyone, at zero cost.**
