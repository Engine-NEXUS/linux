# NEXUS Live Mode — Feasibility & Architecture Research

**Date:** 2026-09-11
**Goal:** Always-listening, STT-only mode that controls the entire laptop via
multi-step voice commands (open app → navigate → type → send).

---

## 1. Executive Summary

**Is it possible? YES — fully achievable with the existing NEXUS stack.**

The research confirms that voice-controlled desktop automation is no longer
experimental. Multiple open-source projects (AnovaX, AURA, JARVIS, OpenDex,
Sai, Agent-Sam) and peer-reviewed papers (AnovaX arXiv:2607.15367, AURA
IJSRET 2026, AssistGUI CVPR 2024, UITron-Speech arXiv:2506.11127) demonstrate
working systems that do exactly what you want: open apps, navigate within
them, type text, click buttons, and send messages — all from continuous
voice input.

NEXUS already has 70% of the infrastructure:
- ✅ Wake word detection (OWW + ONNX)
- ✅ STT (Groq primary + Moonshine fallback)
- ✅ VAD (Silero)
- ✅ Intent parser (deterministic + BERT-Mini fallback)
- ✅ Orchestrator (routing, cancellation, state machine)
- ✅ App open/close (AppRegistry + ShellExecute)
- ✅ WhatsApp deep links
- ✅ Tauri/Rust backend (fast, native, cross-platform)

What's missing:
- ❌ Continuous-listening mode (no wake word required)
- ❌ Multi-step task planning (LLM tool-calling for sequences)
- ❌ Keyboard/mouse simulation (typing text, clicking, scrolling)
- ❌ Window focus management (bring app to foreground)
- ❌ UI element identification (find search box, find send button)
- ❌ Stateful conversation context (remember "open chat with mummy" → "type hi")

---

## 2. Existing Open-Source Projects (Analyzed)

### 2.1 AnovaX (arXiv:2607.15367, Aug 2026) — CLOSEST MATCH
- **Stack:** Python, Gemini LLM planner, pyautogui executor
- **Architecture:** Wake word → STT → LLM emits JSON plan of tool calls →
  multi-agent orchestrator dispatches to typed child agents (AppAgent,
  TypingAgent, BrowserAgent, +6 others) → adaptive recovery loop
- **Key insight:** LLM plans ONCE, Python orchestrator (not the LLM)
  decides which agent handles which tool, with TTL, retries, and locks
- **Recovery:** ReAct-style loop (max 6 turns, 20 agents) when a step fails
- **Safety:** Whitelist + denylist on every plan, recursive sub-plans too
- **Result:** Opens apps, types into them, runs searches, recovers from
  single-step failures — ~1,800 lines of Python
- **Relevance to NEXUS:** ★★★★★ — This is essentially what you want,
  but NEXUS would do it in Rust (faster, more reliable)

### 2.2 AURA (IJSRET Vol 12 Issue 3, 2026)
- **Stack:** Python, LLM intent understanding, desktop automation, TTS
- **Architecture:** Wake word → STT → LLM intent → command execution →
  adaptive voice feedback
- **Capabilities:** Manage applications, navigate websites, manipulate
  text, retrieve information, conversational assistance
- **Result:** "Interprets commands accurately, responds quickly, more
  user-friendly than traditional command-based systems"
- **Relevance:** ★★★★☆ — Validates the approach academically

### 2.3 JARVIS (aievolutionpl/jarvis)
- **Stack:** FastAPI + WebSocket backend, Vite/TypeScript/Three.js frontend
- **Capabilities:** Open apps, websites, files, folders, terminals,
  screenshots, clipboard, media, volume, lock screen
- **Action Guard:** Sensitive tool calls queued for confirmation
- **LLM providers:** Claude, OpenAI, Gemini, DeepSeek, Ollama
- **Relevance:** ★★★★☆ — Production-grade desktop control

### 2.4 OpenDex (wassgha/opendex)
- **Stack:** Voice-first agentic harness, J.A.R.V.I.S. theme
- **Key feature:** "Computer-use (off by default)" — screenshots desktop
  and drives mouse/keyboard. Every action goes through permission gate
  (Allow once / Always / Deny)
- **Voice:** Vosk wake, Whisper/Vosk STT, ElevenLabs/OS TTS
- **Relevance:** ★★★★☆ — Shows the permission/safety model

### 2.5 Sai (GodlyDonuts/sai) — macOS
- **Architecture:** Wake word → stream speech → transcribe → route
  (simple command vs visual task) → plan one action → click/type/scroll/
  hotkey → capture fresh screen → verify → continue or done
- **Key insight:** "Plan, act, verify" loop — one action at a time,
  re-screenshot after each action, verify what changed
- **Relevance:** ★★★★☆ — The verify-after-each-action pattern is critical

### 2.6 Agent-Sam (Sabir-Ali-Mondal) — Windows
- **Stack:** Local 4B model (KoboldCpp), SAM ScreenParser (RapidOCR +
  Windows UIA), Telegram interface
- **Key insight:** "The model is the dispatcher. The skills are the
  workers. The parser is the eyes. The clicker is the hands. None of
  them are allowed to do each other's job."
- **Screen parsing:** Two-table output — semantic table for planning
  (id, text, type — NO pixels), coordinate table for executor (id →
  bounds, center). Model never sees pixels, can't hallucinate coordinates.
- **Relevance:** ★★★★★ — The separation of concerns is exactly right
  for NEXUS's deterministic-first philosophy

### 2.7 NOVA (Epicmanpreet01/NOVA)
- **Stack:** Python, Eel + Web frontend, Whisper STT, Kokoro TTS
- **Upgrade path:** Replaced speech_recognition + pyttsx3 with Whisper +
  Kokoro "without rewriting the command pipeline" — same pattern NEXUS
  already followed (Moonshine replaced faster-whisper)
- **Relevance:** ★★★☆☆ — Validates NEXUS's existing upgrade path

---

## 3. Research Papers (Analyzed)

### 3.1 AnovaX (arXiv:2607.15367, 2026)
**"A Local, Multi-Agent Voice Assistant with LLM Planning, Typed
Executors, and Adaptive Recovery"**

Key findings:
1. **LLM plans once, Python orchestrates execution** — the LLM never
   touches the keyboard. This prevents hallucinated actions.
2. **Typed executors with TTL and retries** — each tool (AppAgent,
   TypingAgent, BrowserAgent) has its own timeout and retry policy.
3. **Adaptive recovery** — when a step fails, a ReAct-style loop takes
   over (max 6 turns), with speculative parallelism for read-only tools.
4. **Two-stage safety** — prompt-level rules + Python whitelist/denylist
   on every plan, including recursive sub-plans.
5. **Limitations documented:**
   - "pyautogui is still fragile on Windows" — coordinate-based clicking
     breaks when windows move or DPI changes
   - "The executor is still blind" — no visual verification of results
   - "The recovery loop can chase a bad idea for six turns"
   - "The safety filter is a blacklist" — can't anticipate every risk

### 3.2 AURA (IJSRET, 2026)
**"An LLM-Driven Voice Interface For Desktop Automation"**

Validates that LLM + STT + desktop automation produces "accurate command
interpretation, quick response, and better user-friendliness than
traditional command-based systems."

### 3.3 AssistGUI (CVPR 2024)
**"Task-Oriented PC Graphical User Interface Automation"**

Benchmark of 100 tasks across 9 Windows apps. Multi-agent collaboration
framework (task decomposition, GUI parsing, action generation, reflection).
**Critical finding: best model achieves only 46% success rate** — GUI
automation is HARD. This means NEXUS must use deterministic paths wherever
possible (deep links, keyboard shortcuts, UI Automation API) rather than
pure vision-based clicking.

### 3.4 UITron-Speech (arXiv:2506.11127, 2025)
**"Towards Automated GUI Agents Based on Speech Instructions"**

First end-to-end GUI agent processing speech instructions + screenshots
directly. Uses synthesized speech instruction datasets + mixed-modality
training. **Relevance:** Shows that speech → GUI action is feasible
without a separate STT step, but requires significant training data.

### 3.5 OSWorld / OSWorld 2.0 (2024/2026)
**Benchmark for computer-use agents on real desktop environments**

Key findings:
- OSWorld 1.0: 369 tasks, GPT-4 scored 12.2%, best agent 41.4%
- OSWorld 2.0: 108 long-horizon workflows, Claude Opus 4.8 scores 20.6%
  at 500 steps, median task takes humans ~1.6 hours
- **Agents fail on: constraint tracking, mid-task information, asking
  vs guessing, verification** — NOT on basic GUI control
- OSWorld-Human: when efficiency is measured, success drops from 41.4%
  to 9.6% — agents waste 66% of steps on looping

**Implication for NEXUS:** Keep tasks SHORT and DETERMINISTIC. Don't try
to build a general-purpose computer-use agent. Build a voice-driven
automation tool with known, tested paths for common operations.

### 3.6 JARVIS-CWTS (JETIR, 2026)
**"Accessibility Using Groq"**

22-module Windows voice assistant using Groq LLaMA 3.3 70B for
"Confidence-Weighted Tool Selection." 97% pass rate on 78 automated
cases, 91% accuracy on 312 voice commands (39 points better than
keyword matching). **Validates LLM-based tool calling for desktop
control.**

---

## 4. Technical Methods Required

### 4.1 Continuous Listening (STT-only, no wake word)

**Current NEXUS flow:**
```
Wake word → STT capture (5-10s) → transcribe → parse → execute → TTS
```

**Live mode flow:**
```
VAD detects speech → stream to STT → endpoint detection →
transcript → parse → execute → (no TTS unless needed) →
immediately resume listening
```

**Required changes:**
1. **VAD-driven endpointing** — NEXUS already has Silero VAD. Instead of
   wake-word-triggered capture, run VAD continuously. When speech starts,
   begin capturing. When silence detected (500-1500ms), endpoint and
   send to STT.
2. **Streaming STT** — Groq Whisper is batch (sends full WAV). For live
   mode, either:
   - **Option A (simpler):** Keep batch STT but with VAD-segmented audio.
     Each VAD segment → Groq API call. Latency = STT time (~247ms) +
     segment duration. Works but has per-segment API latency.
   - **Option B (better):** Use Moonshine streaming mode locally.
     Moonshine is designed for streaming, sub-200ms latency, runs on-device.
     No API calls, no rate limits, fully private.
   - **Option C (best):** Hybrid — Moonshine for instant command
     recognition, Groq for complex/long utterances. NEXUS already has
     this fallback architecture.
3. **No TTS by default** — Execute silently. Only speak when:
   - User asks a question ("what time is it")
   - Confirmation needed ("send this message to mummy?")
   - Error occurred ("WhatsApp not found")
   - User explicitly says "tell me" or "speak"

**RAM impact:** Moonshine streaming adds ~150-300MB (already loaded
after first transcription). VAD is ~20MB. Total live-mode overhead:
minimal since STT is already lazy-loaded.

### 4.2 Multi-Step Task Planning

**The problem:** "open whatsapp → open chat with mummy → type hi what
are u doing → send" is FOUR separate commands. The user says them
sequentially, expecting the system to maintain context.

**Two approaches:**

#### Approach A: Stateful Command Sequence (RECOMMENDED for NEXUS)
```
User: "open whatsapp"
  → NEXUS opens WhatsApp, responds "WhatsApp open"
  → NEXUS enters "whatsapp_open" state

User: "open chat with mummy"
  → NEXUS looks up "mummy" in contacts
  → Types contact name in WhatsApp search box
  → Clicks the matching chat
  → NEXUS enters "whatsapp_chat_active" state with contact=mummy

User: "type hi what are u doing"
  → NEXUS types "hi what are u doing" into the message box
  → NEXUS enters "text_typed" state with text="hi what are u doing"

User: "send"
  → NEXUS presses Enter
  → NEXUS clears state, returns to idle
```

**Implementation:** A finite state machine in Rust. States track what
app is focused, what chat is open, what text is typed. Each voice
command transitions the state machine. This is DETERMINISTIC, no LLM
needed for the sequence — only for parsing each individual command.

#### Approach B: LLM Plan-and-Execute (AnovaX style)
```
User: "open whatsapp, open chat with mummy, type hi what are u doing, and send"
  → LLM generates JSON plan:
    [
      {"tool": "open_app", "target": "whatsapp"},
      {"tool": "wait", "ms": 2000},
      {"tool": "whatsapp_search_contact", "name": "mummy"},
      {"tool": "wait", "ms": 500},
      {"tool": "type_text", "text": "hi what are u doing"},
      {"tool": "press_key", "key": "enter"}
    ]
  → Orchestrator executes each step sequentially
  → If a step fails, recovery loop
```

**Implementation:** Send transcript to Worker's LLM with a tool schema.
Worker returns JSON plan. Rust executes each step. More flexible but
requires LLM call per complex command and is less predictable.

**Recommendation:** Use BOTH. Approach A for common sequences (WhatsApp,
browser search, email). Approach B for novel multi-step commands the
state machine doesn't recognize.

### 4.3 Keyboard/Mouse Simulation (Typing Text)

**Rust-native options:**

| Library | Platform | Method | Best For |
|---------|----------|--------|----------|
| `enigo` | Win/Mac/Linux | SendInput/CGEvent/XTest | Cross-platform text + keys |
| `uiautomation-rs` | Windows only | UI Automation API | Finding elements, clicking |
| `windows` crate | Windows only | SendInput directly | Maximum control, no deps |
| `ghost-hands` | Win/Mac/Linux | SendInput/CGEvent/XTest | Production-tested, IME-aware |

**Recommended: `enigo` for NEXUS**
- Cross-platform (NEXUS targets Win/Mac/Linux)
- Simple API: `enigo.text("hello")`, `enigo.key(Key::Enter, Click)`
- Active maintenance, 9K+ stars
- Already used by many Rust automation projects

**For Windows-specific element finding: `uiautomation-rs`**
- Find search box by name/class
- `element.send_text("query")` — types into specific element
- `element.click()` — clicks specific element
- More reliable than coordinate-based clicking

**Typing text into WhatsApp:**
```rust
// 1. Focus WhatsApp window (already have SetForegroundWindow)
// 2. Find the message input box via UI Automation
let automation = UIAutomation::new()?;
let matcher = automation.create_matcher()
    .from(focused_element)
    .contains_name("Type a message")
    .timeout(2000);
let message_box = matcher.find_first()?;
// 3. Type text
message_box.send_text("hi what are u doing")?;
// 4. Press Enter
enigo.key(Key::Return, Click)?;
```

### 4.4 Window Focus Management

**Current NEXUS:** Opens apps via ShellExecuteW but doesn't manage focus.

**Required:** After opening an app, wait for it to appear, then bring
to foreground. Windows' `SetForegroundWindow` has a known restriction:
background processes can't steal focus. The workaround (used by
`ghost-hands` and `nuphus-mcp`):

```rust
// Attach thread input to foreground thread, then set foreground
let fg = GetForegroundWindow();
let fg_thread = GetWindowThreadProcessId(fg, None);
let this_thread = GetCurrentThreadId();
AttachThreadInput(this_thread, fg_thread, true);
SetForegroundWindow(hwnd);
BringWindowToTop(hwnd);
AttachThreadInput(this_thread, fg_thread, false);
```

### 4.5 UI Element Identification

**Three approaches, in order of reliability:**

1. **UI Automation API (most reliable)** — Query the accessibility tree
   to find elements by name, class, or control type. Works with most
   apps that have proper accessibility support. `uiautomation-rs` wraps
   this.

2. **Keyboard shortcuts (most portable)** — Tab through focusable
   elements, type into whatever has focus. E.g., in WhatsApp: Ctrl+T
   opens a new chat search, type the name, press Enter, type message,
   press Enter. No element finding needed.

3. **Screenshot + vision model (least reliable, most flexible)** —
   OmniParser V2 (Microsoft) tokenizes screenshots into clickable
   elements. 39.6% accuracy on ScreenSpot Pro. Use only as last resort.

**Recommendation for NEXUS:** Use keyboard shortcuts + UI Automation.
Skip vision models entirely — they're too unreliable (46% success per
AssistGUI benchmark) and too heavy (need GPU for real-time).

### 4.6 WhatsApp Automation Specifically

**Current NEXUS:** Opens `whatsapp://send?phone=NUMBER` deep link. This
opens a chat but doesn't type or send.

**For full WhatsApp control:**

| Method | Pros | Cons |
|--------|------|------|
| Deep links (`whatsapp://`) | Simple, no automation | Can't type/send |
| WhatsApp Web + Selenium | Full control | Needs browser, slow, fragile |
| Desktop app + UI Automation | Native, fast | WhatsApp Desktop UI changes |
| Desktop app + keyboard shortcuts | Simple, reliable | App-specific key sequences |

**Recommended:** Desktop app + keyboard shortcuts:
```
1. Open WhatsApp Desktop (ShellExecuteW or deep link)
2. Wait 2s for app to load
3. Ctrl+T or Ctrl+F → search bar opens
4. Type contact name → press Enter → chat opens
5. Type message text → press Enter → sent
```

This requires NO element finding, NO Selenium, NO browser. Just
keyboard simulation into the focused app. Fragile only if WhatsApp
changes their keyboard shortcuts (rare).

---

## 5. Proposed Architecture for NEXUS Live Mode

```
                    ┌─────────────────────────┐
                    │   Continuous VAD Loop    │
                    │   (Silero, always on)    │
                    └────────────┬────────────┘
                                 │ speech detected
                                 ▼
                    ┌─────────────────────────┐
                    │   STT (Moonshine stream   │
                    │   or Groq batch fallback) │
                    └────────────┬────────────┘
                                 │ transcript
                                 ▼
                    ┌─────────────────────────┐
                    │   Intent Parser          │
                    │   (deterministic + BERT)  │
                    └────────────┬────────────┘
                                 │ parsed intent
                    ┌────────────┴────────────┐
                    │                         │
                    ▼                         ▼
          ┌──────────────┐          ┌──────────────┐
          │ Simple Command│          │ Multi-Step    │
          │ (single action)│         │ (state machine│
          │               │         │ or LLM plan)  │
          └───────┬──────┘          └───────┬──────┘
                  │                         │
                  ▼                         ▼
          ┌──────────────────────────────────────┐
          │        Action Executor (Rust)         │
          │                                       │
          │  ┌─────────┐ ┌─────────┐ ┌─────────┐  │
          │  │ App Mgr │ │ Keyboard│ │ Window  │  │
          │  │ (open/  │ │ /Mouse  │ │ Focus   │  │
          │  │ close) │ │ (enigo) │ │ (Win32) │  │
          │  └─────────┘ └─────────┘ └─────────┘  │
          │                                       │
          │  ┌─────────┐ ┌─────────┐ ┌─────────┐  │
          │  │ UI Auto │ │ Deep    │ │ State   │  │
          │  │ (find   │ │ Links   │ │ Machine │  │
          │  │ elements)│ │ (URLs)  │ │ (context)│ │
          │  └─────────┘ └─────────┘ └─────────┘  │
          └──────────────────┬───────────────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │  Resume VAD     │
                    │  (immediately)  │
                    └─────────────────┘
                             │
                    ┌────────┴────────┐
                    │ TTS only if:     │
                    │ - Question asked │
                    │ - Confirmation   │
                    │ - Error occurred │
                    └─────────────────┘
```

### 5.1 State Machine for Sequential Commands

```rust
enum LiveModeState {
    Idle,
    AppOpen { app: String, hwnd: HWND },
    ChatActive { app: String, contact: String },
    TextTyped { app: String, contact: String, text: String },
    BrowserSearch { query: String },
    // ... extensible
}

struct LiveModeContext {
    state: LiveModeState,
    last_action_timestamp: Instant,
    // Auto-reset to Idle after 30s of silence
    timeout: Duration,
}
```

### 5.2 New Rust Dependencies

```toml
# Cargo.toml additions
[dependencies]
enigo = "0.3"           # Cross-platform keyboard/mouse simulation
uiautomation = "0.25"   # Windows UI Automation (element finding)
# windows crate already in dependencies for Win32 calls
```

### 5.3 New Tauri Commands

```rust
#[tauri::command]
async fn live_mode_type_text(text: String) -> Result<(), String>;

#[tauri::command]
async fn live_mode_press_key(key: String) -> Result<(), String>;

#[tauri::command]
async fn live_mode_focus_app(app: String) -> Result<(), String>;

#[tauri::command]
async fn live_mode_find_and_click(element_name: String) -> Result<(), String>;

#[tauri::command]
async fn live_mode_whatsapp_send(contact: String, message: String) -> Result<(), String>;

#[tauri::command]
async fn live_mode_browser_search_and_navigate(query: String) -> Result<(), String>;
```

---

## 6. Safety & Security Considerations

Based on Anthropic's Computer Use safety guidelines and the SynapseKit
safety framework:

### 6.1 Mandatory Confirmations
- **Sending messages** — Always confirm before pressing Send
- **Deleting files** — Always confirm
- **Financial actions** — Always confirm (purchases, transfers)
- **Password entry** — Never type passwords automatically

### 6.2 Forbidden Actions
- Don't type into password fields
- Don't interact with banking/financial apps
- Don't accept cookies/terms of service automatically
- Don't click "delete" or "confirm purchase" without explicit user yes

### 6.3 Prompt Injection Defense
- Screen content can contain malicious instructions (ads, web pages)
- NEXUS should NOT read screen content as commands
- Only the user's voice transcript drives actions
- If using vision models for element finding, sanitize the extracted
  text before passing to LLM

### 6.4 Permission Model (from OpenDex)
- **Allow once** — execute this action, ask again next time
- **Always allow** — remember for this app/action type
- **Deny** — refuse and explain why
- Store preferences in settings, editable by user

---

## 7. Implementation Roadmap

### Phase 1: Continuous Listening Mode (1-2 days)
- [ ] Add `live_mode: bool` to settings
- [ ] When live mode is on, bypass wake word, run VAD continuously
- [ ] VAD speech endpoint → STT → parse → execute → resume VAD
- [ ] No TTS by default (silent execution)
- [ ] Visual indicator (orb color change) when live mode is active

### Phase 2: Keyboard/Mouse Simulation (1-2 days)
- [ ] Add `enigo` dependency
- [ ] Implement `type_text`, `press_key`, `press_hotkey` commands
- [ ] Implement window focus management (AttachThreadInput trick)
- [ ] Add intent parser patterns: "type X", "press enter", "press escape"

### Phase 3: Stateful Command Sequences (2-3 days)
- [ ] Implement `LiveModeContext` state machine
- [ ] WhatsApp sequence: open → search contact → type message → send
- [ ] Browser sequence: open browser → open new tab → type URL/search → enter
- [ ] Auto-timeout: reset to Idle after 30s of silence
- [ ] Add intent parser patterns: "send", "go", "enter", "submit"

### Phase 4: UI Automation Integration (2-3 days)
- [ ] Add `uiautomation-rs` dependency (Windows)
- [ ] Find elements by name for apps that need it
- [ ] Fallback to keyboard shortcuts when UI Automation fails
- [ ] Test with WhatsApp Desktop, Brave, Chrome, Notepad

### Phase 5: LLM Plan-and-Execute (3-4 days)
- [ ] Add tool schema to Worker's LLM endpoint
- [ ] Worker returns JSON plan for complex multi-step commands
- [ ] Rust executes plan steps sequentially with verification
- [ ] Recovery loop for failed steps (max 3 retries)

### Phase 6: Safety & Polish (2-3 days)
- [ ] Confirmation prompts for send/delete/purchase actions
- [ ] Forbidden app list (banking, password managers)
- [ ] Permission model (allow once / always / deny)
- [ ] Visual feedback for each step (orb state changes)
- [ ] Emergency stop ("stop" or "cancel" aborts immediately)

**Total estimated effort: 11-17 days** for a production-ready live mode.

---

## 8. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| UI changes break automation | Medium | High | Use keyboard shortcuts, not coordinates |
| STT mishears commands | Medium | Medium | Fuzzy matching + confirmation for critical actions |
| App doesn't respond to focus | Low | High | AttachThreadInput workaround |
| WhatsApp UI changes | Low | Medium | Keyboard shortcuts are stable; deep links as fallback |
| Prompt injection from screen | Low | High | Don't read screen as commands; voice-only input |
| RAM increase from live mode | Low | Low | Moonshine already loaded; VAD is lightweight |
| Mic driver issues (Intel SST) | High | High | Existing silence recovery thread handles this |
| False VAD triggers | Medium | Low | Confidence threshold + min speech duration |

---

## 9. Conclusion

**Live mode is fully feasible.** The technology exists, is proven by
multiple open-source projects and research papers, and NEXUS already has
the core infrastructure. The main work is:

1. **Continuous VAD → STT loop** (bypassing wake word)
2. **Keyboard/mouse simulation** via `enigo` (cross-platform)
3. **Stateful command sequences** via a Rust state machine
4. **UI element finding** via `uiautomation-rs` (Windows) + keyboard
   shortcut fallbacks
5. **Safety layer** for destructive/irreversible actions

The deterministic-first approach NEXUS already uses (parse before LLM,
execute known paths first) is the RIGHT architecture for this. Pure
LLM-driven computer-use agents achieve only 12-46% success rates on
benchmarks. NEXUS's approach of deterministic commands + LLM fallback
will be significantly more reliable for the specific use cases you want
(WhatsApp, browser, app management).

**Key principle from the research:** "The model is the dispatcher. The
skills are the workers. The parser is the eyes. The clicker is the
hands. None of them are allowed to do each other's job." — Agent-Sam

This matches NEXUS's existing philosophy: deterministic parser first,
LLM only when needed, execution always in Rust.
