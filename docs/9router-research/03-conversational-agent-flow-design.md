# Conversational Agent Flow Design — The 9Router Manager Experience

**Date:** 2026-09-15
**Status:** Research complete — design proposed
**Researcher:** Devin (GLM-5.2 High)
**Question:** How should the NEXUS assistant interact with the user about free tiers, API keys, reminders, and 9Router configuration — in a natural, conversational way?

---

## 1. The User's Vision

The user described this interaction pattern:

> "If the model tells me the free tier is there in this [provider] and asks
> me 'should I open it?' — if not, it will remind me again later. If I say
> yes, I have done and found the API key, I copy the API key and tell the
> model 'I have copied it, add in the 9router' — it says 'ok sir'."

This is a **proactive, conversational, stateful agent** — not a command
parser. The key characteristics:

1. **Proactive** — the assistant initiates ("I found a free tier at X")
2. **Asks permission** — "should I open it?"
3. **Respects "not now"** — saves a reminder for later
4. **Handles user actions** — user copies key, tells the assistant
5. **Confirms** — "ok sir, added to 9Router"
6. **Multi-turn** — spans multiple wake-word sessions
7. **Remembers context** — knows what was discussed before

---

## 2. Why This Needs a Thinking Brain

This flow cannot be done with regex, BERT-Mini, or the current 0.5B brain:

| Step | Why pattern-matching fails |
|------|---------------------------|
| "I found a free tier at cerebras" | No intent label for "user found free tier" |
| "should I open it?" | Requires understanding of context (what is "it"?) |
| "not now" | Requires state management (save reminder) |
| "I have copied it" | Requires context (what did the user copy?) |
| "add in the 9router" | Requires tool use (call 9Router API) |
| "ok sir" | Requires confirmation + TTS response |

Each step requires **understanding + state + action**, not classification.

---

## 3. The Complete Conversational Flow

### 3.1 Flow A: Discovery — New Free Tier Found

```
[Discovery scheduler finds a new free provider]

NEXUS: "Sir, I found a new free tier at Cerebras — 1 million tokens
        per day, no credit card needed. Would you like me to open the
        signup page?"

User:  "not now"

NEXUS: "No problem, sir. I'll remind you about it later."

[Brain saves reminder: cerebras, remind in 2 hours]

[...2 hours later, on next wake or proactively...]

NEXUS: "Sir, earlier I found a free tier at Cerebras. Would you like
        to set it up now?"

User:  "yes, open it"

NEXUS: "Opening the Cerebras signup page now, sir."

[Brain opens browser to https://cloud.cerebras.ai]

NEXUS: "Once you've signed up and got your API key, just say 'I have
        the key' and I'll add it to 9Router."

[...user signs up, copies API key...]

User:  "i have copied the api key"

NEXUS: "Let me grab that from your clipboard, sir."

[Brain reads clipboard, validates the key format]

NEXUS: "I see a Cerebras API key. Let me validate it and add it to
        9Router."

[Brain calls 9Router API to add the provider]

NEXUS: "Done, sir. Cerebras is now configured in 9Router. I ran a
        quick health check — it's responding in 420 milliseconds.
        You now have 1 million tokens per day available."

[Brain updates state: cerebras configured, healthy]
```

### 3.2 Flow B: Quota Exhausted — Auto-Rotation

```
[9Router reports Groq key 1 hit 429]

NEXUS: (silently rotates to Groq key 2, no user interaction needed)

[All Groq keys exhausted]

NEXUS: "Sir, your Groq quota is exhausted for today. I've switched
        to Gemini as the primary provider. Groq will reset at
        midnight UTC."

User:  "ok"

[Brain updates state: groq exhausted, gemini primary]
```

### 3.3 Flow C: Trial Credit Exhausted — Key Deletion

```
[Together AI $5 credit exhausted — 402 Payment Required]

NEXUS: "Sir, your Together AI trial credits are used up. I've
        removed the key from 9Router and switched to Cerebras.
        Would you like me to look for new free providers to replace
        Together AI?"

User:  "yes, find some"

NEXUS: "On it, sir. I'll scan for new free-tier providers and let
        you know what I find."

[Discovery scheduler runs, finds 2 new providers]

NEXUS: "Sir, I found two new free options: SambaNova (100K tokens/day)
        and Novita AI (metered access). SambaNova looks best for your
        usage. Want me to open the signup page?"

[...continues like Flow A...]
```

### 3.4 Flow D: Provider Broken — Quarantine

```
[OpenCode Free endpoint returns 503 for 3 consecutive checks]

NEXUS: "Sir, OpenCode Free has been unresponsive for the last few
        hours. I've quarantined it and switched to your local model
        as fallback. I'll check OpenCode again in 6 hours and
        re-enable it if it recovers."

User:  "ok"

[Brain quarantines OpenCode, sets recheck timer]
```

### 3.5 Flow E: User Asks for Status

```
User:  "hey nexus, what's my model status?"

NEXUS: "Here's your current setup, sir:

        Local models: Ollama with Qwen 2.5 3B — ready.
        Cloud providers:
        - Groq: healthy, 12,000 requests remaining today.
        - Gemini: healthy, 800 requests remaining.
        - Cerebras: healthy, 950,000 tokens remaining.
        - Together AI: exhausted, key removed.

        9Router combos active:
        - jarvis-general: Cerebras -> Gemini -> Groq -> Local
        - jarvis-coding: Groq -> Cerebras -> Local

        Budget: zero-cost mode, all paid providers blocked."

[Brain reads state file, formats summary, speaks it]
```

### 3.6 Flow F: User Manually Adds a Provider

```
User:  "hey nexus, i found a free api at siliconflow, add it"

NEXUS: "SiliconFlow — I see they offer free tier with 1,000 RPM.
        Do you have an API key, sir?"

User:  "yes, i copied it"

NEXUS: "Let me grab that from your clipboard."

[Brain reads clipboard, validates key format]

NEXUS: "I see a SiliconFlow API key. Validating now."

[Brain calls SiliconFlow /v1/models to validate]

NEXUS: "Key is valid, sir. I can see 15 models available including
        Qwen 2.5 7B and DeepSeek V3. Adding to 9Router now."

[Brain calls 9Router API to add provider]

NEXUS: "Done, sir. SiliconFlow is configured in 9Router. I've added
        it to your jarvis-general combo as a fallback after Cerebras."

[Brain updates state: siliconflow configured, healthy]
```

---

## 4. State Machine

### 4.1 Conversation States

```
IDLE
  |
  | [discovery finds new provider]
  v
PROPOSING_PROVIDER  -----> user says "not now" -----> REMINDER_SET -----> IDLE
  |
  | [user says "yes, open it"]
  v
WAITING_FOR_KEY  -----> timeout (5 min) -----> IDLE (with reminder)
  |
  | [user says "i have the key" / "i copied it"]
  v
VALIDATING_KEY  -----> key invalid -----> WAITING_FOR_KEY (retry)
  |
  | [key valid]
  v
ADDING_TO_9ROUTER  -----> 9Router API error -----> IDLE (with error)
  |
  | [success]
  v
HEALTH_CHECK  -----> health check fails -----> IDLE (with warning)
  |
  | [health check passes]
  v
CONFIRMING  -----> "ok sir, added to 9Router" -----> IDLE
```

### 4.2 State File

```json
{
  "conversation_state": {
    "current_state": "IDLE",
    "pending_provider": null,
    "pending_action": null,
    "state_entered_at": "2026-09-15T14:30:00Z"
  },
  "reminders": [
    {
      "id": "rem_001",
      "type": "free_tier_reminder",
      "provider": "cerebras",
      "message": "You found a free tier at Cerebras. Want to add it?",
      "created_at": "2026-09-15T14:30:00Z",
      "remind_at": "2026-09-15T16:30:00Z",
      "status": "pending",
      "retry_count": 0
    }
  ],
  "providers": {
    "groq": {
      "status": "healthy",
      "keys": 2,
      "quota_remaining": 12000,
      "quota_reset_at": "2026-09-16T00:00:00Z",
      "classification": "PERMANENT_FREE",
      "last_health_check": "2026-09-15T14:25:00Z"
    },
    "cerebras": {
      "status": "healthy",
      "keys": 1,
      "quota_remaining": 950000,
      "quota_reset_at": "2026-09-16T00:00:00Z",
      "classification": "PERMANENT_FREE",
      "last_health_check": "2026-09-15T14:25:00Z"
    }
  },
  "combos": {
    "jarvis-general": ["cerebras", "gemini", "groq", "local"],
    "jarvis-coding": ["groq", "cerebras", "local"]
  },
  "audit_log": [
    {
      "timestamp": "2026-09-15T14:30:00Z",
      "action": "provider_added",
      "provider": "cerebras",
      "details": "Added via user clipboard, key validated, health check passed"
    }
  ]
}
```

---

## 5. The Brain's System Prompt

The Thinking Brain needs a system prompt that defines its role, available
tools, and behavior:

```
You are NEXUS, a voice assistant with a 9Router free-model manager.
Your job is to help the user discover, configure, and maintain free AI
model providers through 9Router.

You can:
- Check 9Router's current providers and models (GET /v1/models)
- Add a provider to 9Router (POST /api/providers)
- Validate an API key (POST /api/providers/{id}/validate)
- Run a health check (POST /api/providers/{id}/health-check)
- Remove a provider (DELETE /api/providers/{id})
- Read the user's clipboard (to get API keys they've copied)
- Open a URL in the browser (to take the user to signup pages)
- Set reminders for later
- Check provider quota and status

Rules:
- Always ask before opening a browser URL
- Always confirm before adding or removing a provider from 9Router
- Never spend money without explicit user approval
- Never send sensitive data (passwords, payment info) to any model
- If a provider is exhausted, rotate to the next one silently
- If all cloud providers fail, fall back to the local model
- Keep responses concise — this is a voice assistant, not a chatbot
- Address the user as "sir"
- Use thinking mode for complex requests (provider selection, planning)
- Use non-thinking mode for simple confirmations and status checks

Current state:
{brain_state_json}
```

---

## 6. Clipboard Integration

### 6.1 The Flow

When the user says "I have the key" or "I copied it":

1. Brain asks Rust to read the clipboard (via `arboard` crate, already a
   dependency)
2. Rust returns the clipboard contents to the brain
3. Brain validates the key format (length, prefix, character set)
4. If valid, brain proceeds to add it to 9Router
5. If invalid, brain asks the user to re-copy

### 6.2 Key Format Validation

| Provider | Key format | Example |
|----------|-----------|---------|
| Groq | `gsk_` + 52 chars | `gsk_abc123...` |
| Gemini | `AIza` + 35 chars | `AIzaSyA...` |
| Cerebras | 40 char hex | `abc123def456...` |
| OpenRouter | `sk-or-v1-` + 57 chars | `sk-or-v1-abc...` |
| Together AI | 64 char hex | `abc123def456...` |

The brain can detect the provider from the key prefix and auto-classify.

### 6.3 Security

- Clipboard is read ONLY when the user explicitly says "I have the key"
- The key is never logged, never stored in plaintext, never sent to a model
- The key is passed directly to 9Router's API (which stores it securely)
- After adding to 9Router, the clipboard is cleared

---

## 7. Proactive Behavior

### 7.1 When NEXUS Initiates

NEXUS should proactively reach out in these situations:

| Trigger | Action | Timing |
|---------|--------|--------|
| Discovery finds new free provider | Propose to user | Next wake word |
| Provider quota running low | Warn user | At 80% usage |
| Provider quota exhausted | Notify + rotate | Immediately |
| Trial credit about to expire | Warn user | 24h before expiry |
| Provider goes offline | Notify + quarantine | After 3 failed checks |
| Provider recovers | Notify + re-enable | After 3 successful checks |
| Reminder fires | Remind user | At remind_at time |
| 9Router not running | Notify | On startup |

### 7.2 How Proactive Messages Work

NEXUS doesn't interrupt the user randomly. Proactive messages are delivered
on the **next wake-word activation**:

```
[User says "NEXUS"]

NEXUS: "Sir, I have a reminder: you wanted to set up the Cerebras free
        tier. Want to do that now, or is there something else?"

User:  "do it later"

NEXUS: "Sure, sir. What can I help you with?"

[...normal interaction...]

[Brain updates reminder: remind_at += 2 hours]
```

### 7.3 Reminder Scheduling

Reminders use a backoff schedule:
- First reminder: 2 hours after initial "not now"
- Second reminder: 4 hours after first reminder
- Third reminder: 8 hours after second
- After 3 reminders: stop reminding, keep in state file for manual review

---

## 8. 9Router API Integration

### 8.1 The Brain Calls 9Router

The brain server (Python, port 39220) makes HTTP calls to 9Router
(localhost:20128):

| Action | 9Router API | Brain method |
|--------|-------------|-------------|
| List current providers | `GET /v1/models` | `list_providers()` |
| Add a provider | `POST /api/providers` | `add_provider(id, key, base_url)` |
| Validate a key | `POST /api/providers/{id}/validate` | `validate_key(id)` |
| Health check | `POST /api/providers/{id}/health-check` | `health_check(id)` |
| Remove a provider | `DELETE /api/providers/{id}` | `remove_provider(id)` |
| Create a combo | `POST /api/combos` | `create_combo(name, providers)` |
| Get usage | `GET /api/usage` | `get_usage()` |

### 8.2 Error Handling

```
9Router not running?
  -> Brain tells user: "9Router isn't running, sir. Start it with
     '9router' in your terminal."
  -> Brain retries 9Router health check every 60s
  -> Falls back to direct provider calls (bypassing 9Router)

9Router API error?
  -> Brain logs the error
  -> Tells user: "I had trouble configuring 9Router, sir. Let me try
     again."
  -> Retries up to 3 times
  -> If still failing, saves the configuration for manual addition
```

---

## 9. Voice Response Design

### 9.1 Tone and Style

NEXUS speaks like a competent butler:
- Concise (voice, not text — keep it short)
- Addresses user as "sir"
- Confirms actions briefly ("Done, sir.")
- Explains problems simply ("Your Groq quota is used up for today.")
- Offers next steps ("Want me to look for alternatives?")

### 9.2 Example Responses

| Situation | Response |
|-----------|----------|
| Provider added | "Done, sir. Cerebras is configured in 9Router." |
| Provider exhausted | "Sir, Groq is used up for today. I've switched to Gemini." |
| Key invalid | "That key doesn't look right, sir. Can you re-copy it?" |
| Discovery found new provider | "Sir, I found a new free option at SambaNova. Want me to open the signup page?" |
| Reminder | "Sir, earlier you wanted to set up Cerebras. Still want to do that?" |
| Status check | "You have 3 providers healthy, 1 exhausted. Groq resets at midnight." |
| All providers down | "All cloud providers are unavailable, sir. I'm using your local model. Quality may be reduced." |

---

## 10. Integration with NEXUS Architecture

### 10.1 Where the Brain Sits

```
User speaks
  |
  v
Wake word (OWW KWS)
  |
  v
STT (Groq/Moonshine) -> transcript
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
                          Fast Brain (0.5B) -> ParsedIntent?
                                  |
                                  | no
                                  v
                          Thinking Brain (4B) -> action + response
                                  |
                                  v
                          Execute action (9Router API, browser, etc.)
                                  |
                                  v
                          TTS speaks response
```

### 10.2 New Tauri Commands

| Command | Purpose |
|---------|---------|
| `brain_think` | Send a request to the Thinking Brain |
| `brain_converse` | Multi-turn dialogue with state |
| `brain_get_state` | Get current conversation state |
| `brain_set_reminder` | Set a reminder |
| `brain_clear_reminder` | Clear a reminder |
| `read_clipboard` | Read clipboard contents (for API key capture) |
| `open_browser` | Open a URL in the system browser |
| `nine_router_status` | Check if 9Router is running |
| `nine_router_add_provider` | Add a provider to 9Router |
| `nine_router_list_models` | List 9Router's configured models |
| `nine_router_health_check` | Run a health check on a provider |

### 10.3 New Rust Modules

| Module | Purpose |
|--------|---------|
| `src-tauri/src/brain_thinking.rs` | Thinking Brain client (port 39220) |
| `src-tauri/src/nine_router.rs` | 9Router API client |
| `src-tauri/src/clipboard.rs` | Clipboard reader (using `arboard`) |
| `src-tauri/src/conversation_state.rs` | Conversation state + reminders |

### 10.4 New Python Server

| File | Purpose |
|------|---------|
| `server/admin/thinking_brain_server.py` | Qwen3-4B-Thinking server (port 39220) |

---

## 11. Testing the Flow

### 11.1 Test Scenario 1: Full Discovery-to-Setup

```
1. Start 9Router (npm install -g 9router && 9router)
2. Start NEXUS
3. Say: "hey nexus, find me free model providers"
4. NEXUS should: scan discovery sources, propose providers
5. Say: "yes, open cerebras"
6. NEXUS should: open browser to cerebras.ai
7. Sign up, copy API key
8. Say: "i have the key"
9. NEXUS should: read clipboard, validate, add to 9Router, confirm
10. Say: "what's my status"
11. NEXUS should: speak the current provider status
```

### 11.2 Test Scenario 2: Reminder Flow

```
1. Say: "hey nexus, find free providers"
2. NEXUS proposes a provider
3. Say: "not now"
4. NEXUS says: "I'll remind you later"
5. Wait 2 hours (or speed up with test mode)
6. Say: "hey nexus"
7. NEXUS should: remind about the pending provider
```

### 11.3 Test Scenario 3: Quota Exhaustion

```
1. Use up Groq quota (send 14,400 requests)
2. NEXUS should: detect 429, rotate to next key or provider
3. Say: "what happened to groq"
4. NEXUS should: explain quota exhaustion + rotation
```

---

## 12. Open Questions for Further Research

1. **9Router API stability** — The 9Router API endpoints (`/api/providers`,
   `/api/combos`) need to be verified for programmatic access (not just
   dashboard use). Are they documented and stable?

2. **Clipboard security** — Reading the clipboard is sensitive. Should we
   require the user to explicitly confirm before reading, or is "I have the
   key" sufficient consent?

3. **Thinking Brain latency** — Qwen3-4B-Thinking on CPU may take 2-5 seconds
   for complex reasoning. Is this acceptable for a voice assistant? Should
   we show a "thinking..." indicator?

4. **9Router as coding gateway** — The user mentioned using 9Router for
   coding in the future. How should coding requests be routed? Through
   the Worker, or directly through 9Router's `/v1` endpoint?

5. **Model download strategy** — The 4B-Thinking model is 2.5 GB. Should
   we bundle it, download on first use, or require Ollama?

6. **Multi-device sync** — 9Router has optional cloud sync. Should NEXUS
   sync provider configurations across devices?

---

## 13. Summary

The conversational agent flow transforms NEXUS from a command parser into
a **proactive, stateful, conversational assistant** that:

1. **Discovers** free providers automatically
2. **Proposes** them to the user conversationally
3. **Remembers** when the user says "not now" and reminds later
4. **Captures** API keys from the clipboard when the user is ready
5. **Validates** and **adds** keys to 9Router automatically
6. **Confirms** with a brief voice response ("ok sir")
7. **Monitors** provider health and **rotates** when exhausted
8. **Reports** status on request

This requires the **Thinking Brain** (Qwen3-4B-Thinking-2507) for
understanding, planning, and tool use, plus a **conversation state file**
for multi-turn memory, plus **9Router API integration** for provider
management.

The design is **local-first** (brain runs locally, state is local, 9Router
is local), **privacy-preserving** (keys never leave the device, clipboard
is read only on explicit request), and **low-RAM** (Thinking Brain is
lazy-loaded, only ~2.8 GB when actively reasoning).
