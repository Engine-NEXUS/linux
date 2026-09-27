# NEXUS Live Mode — Deep Source Analysis & Reusable Components

**Date:** 2026-09-11 (Part 2)
**Goal:** Identify exact files, functions, and patterns from open-source
projects that NEXUS can directly reuse or adapt for faster implementation.

---

## 1. Summary of Findings

After reading the actual source code of 8 open-source projects and 4 Rust
crates, here is what we can directly reuse:

### Direct Reuse (copy patterns, no rewrite needed)
1. **OpenDex `open` skill** — cross-platform app launch (cmd/open/gtk-launch)
2. **OpenDex `computer` skill** — nut.js tool schema for click/type/scroll/keys
3. **OpenDex permission model** — `sensitive` + `optIn` flags per skill
4. **Ari `BaseCommand` + `CommandRegistry`** — priority-ordered command matching
5. **Ari `SystemCommand`** — scheduled shutdown/restart with natural language time parsing
6. **SAM ScreenParser `uia_to_type()`** — UIA control type → semantic type mapping
7. **enigo `keyboard.rs` example** — exact Rust pattern for text + key combos
8. **uiautomation-rs `uia_notepad` sample** — find window, send text, send keys

### Adapt (translate Python → Rust)
1. **AnovaX plan schema** — JSON tool-call plan format (translate to Rust serde)
2. **AnovaX safety filter** — whitelist + denylist per plan
3. **SAM ScreenParser two-table output** — semantic table for planner, coordinate
   table for executor (model never sees pixels)

### Use as Reference Architecture
1. **OpenDex skill module structure** — meta.ts + skill.ts + view.tsx pattern
2. **AnovaX typed executors** — AppAgent, TypingAgent, BrowserAgent with TTL
3. **JARVIS action guard** — queue sensitive tool calls for confirmation

---

## 2. OpenDex — Direct Reusable Patterns

### 2.1 Cross-Platform App Launch (TypeScript → Rust port)

Source: `src/skills/open/skill.ts`

```typescript
function launchApp(name: string) {
  if (process.platform === "darwin")      cmd = "open",    args = ["-a", name];
  else if (process.platform === "win32")   cmd = "cmd",     args = ["/c", "start", "", name];
  else                                     cmd = "gtk-launch", args = [name];
  const child = spawn(cmd, args, { stdio: "ignore", detached: true });
  child.unref();
  setTimeout(() => settle({ ok: true }), 150);  // optimistic resolve
}
```

**NEXUS already has this** in `command_executor.rs::resolve_and_open_app()`
via ShellExecuteW. OpenDex's pattern is simpler but less robust (no AppRegistry).
**Verdict:** NEXUS's existing implementation is BETTER. Keep it.

### 2.2 Computer Control Tool Schema (THE key pattern)

Source: `src/skills/computer/skill.ts` + `meta.ts`

OpenDex defines 8 tools with zod schemas:

```typescript
TOOLS = {
  captureScreen, click, moveMouse, drag,
  typeText, pressKeys, scroll, wait
}
```

Each tool has:
- `name` — string identifier
- `description` — when to use it (for LLM)
- `inputSchema` — zod validation
- `summarize` — human-readable summary for permission prompt
- `execute` — the actual implementation
- `toModelOutput` — transform result for LLM (text + screenshot)

**Key design: pasteText via clipboard** (avoids autocomplete corruption):
```typescript
async function pasteText(text: string) {
  const prev = clipboard.readText();
  clipboard.writeText(text);
  const mod = process.platform === "darwin" ? Key.LeftCmd : Key.LeftControl;
  await keyboard.pressKey(mod, Key.V);
  await keyboard.releaseKey(Key.V, mod);
  await delay(120);
  clipboard.writeText(prev);  // restore
}
```

**Why this matters for NEXUS:** Typing "hi what are u doing" character-by-
character into WhatsApp can trigger autocomplete suggestions that corrupt
the text. Clipboard paste is instant and reliable. NEXUS should use this
pattern in Rust (via `clipboard-win` crate or `arboard`).

### 2.3 Permission Model

Source: `src/skills/types.ts`

```typescript
interface SkillMeta {
  sensitive: boolean;  // each tool call gated behind permission prompt
  optIn?: boolean;     // skill is OFF unless user explicitly enables
  imageResults?: boolean;  // tools return images (screenshots)
}
type PermissionRequester = (
  skillId: string, label: string, detail: string
) => Promise<boolean>;
```

**For NEXUS:** Add `sensitive: bool` to each intent type. Sensitive intents
(send message, delete file, purchase) require a confirmation event to the
frontend before execution. This maps directly to the existing `OrchestratorEvent::Confirm` variant.

### 2.4 Adaptive Screenshot Cadence

Source: `src/skills/computer/skill.ts`

```typescript
// Keystroke actions (typeText, pressKeys) default to NOT capturing
// Clicks/scrolls capture by default since they change the screen
async function finishAction(message: string, wantShot: boolean) {
  if (!wantShot) return { ok: true, message };
  const shot = await shoot();
  if (lastSentSig && !framesDiffer(lastSentSig, shot.signature)) {
    return { ok: true, message: `${message} (no visible change)` };
  }
  // ...
}
```

**For NEXUS:** When executing multi-step plans, don't screenshot after every
step. Only capture after clicks/scrolls (which change UI state), not after
typing (which usually doesn't need visual verification).

---

## 3. Ari VoiceCommand — Command Registry Pattern

### 3.1 Priority-Based Command Matching

Source: `commands/base_command.py` + `command_registry.py`

```python
class BaseCommand(ABC):
    priority: int = 50  # lower = matched first
    @abstractmethod
    def matches(self, text: str) -> bool: ...
    @abstractmethod
    def execute(self, text: str) -> CommandResult: ...

class CommandRegistry:
    def __init__(self):
        commands = [LearningCommand(...), YoutubeCommand(...), TimerCommand(...),
                    WeatherCommand(...), VolumeCommand(...), SystemCommand(...),
                    TimeCommand(...), CalculatorCommand(...), MemoryCommand(...),
                    AICommand(...)]
        self.commands = sorted(commands, key=lambda c: c.priority)

    def execute(self, text: str) -> CommandResult:
        for command in self.commands:
            if command.matches(text):
                return command.execute(text)
        return CommandResult(success=False)
```

**NEXUS already does this** in `intent_parser.rs::parse_deterministic()` with
regex patterns. But Ari's pattern is more extensible — each command is a
self-contained class with its own `matches()` logic.

**Improvement for NEXUS:** Refactor `intent_parser.rs` from a giant function
with if/else chains into a trait-based registry:

```rust
trait LiveCommand: Send + Sync {
    fn priority(&self) -> u32;
    fn matches(&self, text: &str) -> bool;
    fn execute(&self, text: &str) -> CommandResult;
}

struct CommandRegistry { commands: Vec<Box<dyn LiveCommand>> }

impl CommandRegistry {
    fn execute(&self, text: &str) -> CommandResult {
        for cmd in &self.commands {
            if cmd.matches(text) {
                return cmd.execute(text);
            }
        }
        CommandResult::unknown(text)
    }
}
```

This makes adding new live-mode commands trivial — just implement the trait.

### 3.2 Natural Language Time Parsing

Source: `commands/system_command.py::_parse_scheduled_time()`

```python
# Relative: "in 2 hours", "in 30 minutes", "in 10 seconds"
relative_patterns = [
    (r'(\d+)\s*시간\s*(?:후|뒤)', 3600),   # Korean: hours
    (r'(\d+)\s*분\s*(?:후|뒤)', 60),       # Korean: minutes
    (r'(\d+)\s*초\s*(?:후|뒤)', 1),        # Korean: seconds
]
# Absolute: "at 3pm", "at 14:30"
m = re.search(r'(오전|오후)?\s*(\d{1,2})시(?:\s*(\d{1,2})분)?', normalized)
```

**For NEXUS:** Add English time parsing for "shutdown in 30 minutes",
"restart at 3pm", "set alarm for 6:30 am". Currently NEXUS has placeholder
`set_timer` and `set_alarm` that don't actually parse time.

---

## 4. SAM ScreenParser — UI Element Detection

### 4.1 UIA Control Type → Semantic Type Mapping

Source: `CodeBase/test_screen.py::uia_to_type()`

```python
def uia_to_type(control_type):
    t = control_type.lower()
    if t in ('button', 'menuitem', 'menu', 'splitbutton'): return 'button'
    if t == 'edit': return 'input'
    if t in ('listitem', 'treeitem'): return 'sidebar_item'
    if t in ('tabitem', 'tab'): return 'tab'
    if t in ('checkbox',): return 'checkbox'
    if t in ('radiobutton',): return 'radio'
    if t in ('combobox',): return 'dropdown'
    if t in ('slider',): return 'slider'
    if t in ('hyperlink',): return 'link'
    return None
```

**For NEXUS:** This exact mapping works with `uiautomation-rs` in Rust.
When looking for "the search box" in an app, NEXUS can:
1. Walk the UIA tree from the focused window
2. Filter for `control_type == "Edit"` (input fields)
3. Match by name/placeholder text
4. Click + type into it

### 4.2 Two-Table Output (Critical Safety Pattern)

Source: `CodeBase/test_screen.py::compact_for_llm()`

```python
# Semantic table (for LLM planner) — NO coordinates
{
  "_guide": "If element has no 'type', parser couldn't identify it...",
  "elements": [
    { "id": 3, "text": "File", "type": "menu" },
    { "id": 18, "text": "Folder" }  # type omitted = unknown
  ]
}
# Coordinate table (for executor) — has pixels
{ "id": 3, "bounds": [10, 30, 50, 60], "center": [30, 45] }
```

**Why this is critical:** The LLM NEVER sees pixel coordinates. It picks
an element by ID. The executor resolves the ID to coordinates. This makes
it impossible for the LLM to hallucinate a click on a wrong location.

**For NEXUS:** If we ever add LLM-driven GUI automation, use this two-table
pattern. The LLM says "click element 3", and Rust resolves element 3 to
its actual screen position via the cached UIA tree.

---

## 5. AnovaX — Plan Schema & Safety Filter

### 5.1 JSON Plan Schema

Source: AnovaX paper, Section 3.2

```json
{
  "goal": "open whatsapp and send hi to mummy",
  "steps": [
    {"tool": "open_app", "target": "whatsapp"},
    {"tool": "wait", "ms": 2000},
    {"tool": "whatsapp_search_contact", "name": "mummy"},
    {"tool": "wait", "ms": 500},
    {"tool": "type_text", "text": "hi"},
    {"tool": "press_key", "key": "enter", "confirm": true}
  ]
}
```

Each step has:
- `tool` — which executor handles it
- Tool-specific parameters
- `confirm` — optional, requires user confirmation before execution
- `wait` — built-in pause for app loading

**For NEXUS:** Define this as a Rust serde struct:

```rust
#[derive(Deserialize)]
struct PlanStep {
    tool: String,
    #[serde(flatten)]
    params: serde_json::Value,
    #[serde(default)]
    wait_ms: u64,
    #[serde(default)]
    confirm: bool,
}
#[derive(Deserialize)]
struct Plan {
    goal: String,
    steps: Vec<PlanStep>,
}
```

### 5.2 Two-Stage Safety Filter

Source: AnovaX paper, Section 3.3

**Stage 1: Prompt-level rules** (in the LLM system prompt):
- "Never type into password fields"
- "Never click delete/confirm purchase without user approval"
- "Never navigate to banking sites"

**Stage 2: Code-level whitelist + denylist** (in Rust, runs on every plan):

```rust
const ALLOWED_TOOLS: &[&str] = &[
    "open_app", "close_app", "type_text", "press_key",
    "click_element", "scroll", "wait", "open_url",
    "whatsapp_search_contact", "browser_search",
];
const DENIED_TARGETS: &[&str] = &[
    "1password", "bitwarden", "keychain", "lastpass",
    "bank", "paypal", "venmo", "cashapp",
];
fn safety_check(plan: &Plan) -> Result<(), String> {
    for step in &plan.steps {
        if !ALLOWED_TOOLS.contains(&step.tool.as_str()) {
            return Err(format!("Tool '{}' not allowed", step.tool));
        }
        if let Some(target) = step.params.get("target").and_then(|t| t.as_str()) {
            let lower = target.to_lowercase();
            for denied in DENIED_TARGETS {
                if lower.contains(denied) {
                    return Err(format!("Target '{}' is blocked", target));
                }
            }
        }
    }
    Ok(())
}
```

---

## 6. enigo (Rust) — Exact Code Patterns

### 6.1 Type Text + Key Combinations

Source: `examples/keyboard.rs`

```rust
use enigo::{Direction::{Click, Press, Release}, Enigo, Key, Keyboard};

let mut enigo = Enigo::new(&Settings::default()).unwrap();

// Type text (handles Unicode including emoji)
enigo.text("Hello World! ❤️").unwrap();

// Ctrl+A (select all)
enigo.key(Key::Control, Press).unwrap();
enigo.key(Key::Unicode('a'), Click).unwrap();
enigo.key(Key::Control, Release).unwrap();
```

**For NEXUS's `type_text` command:**
```rust
fn live_type_text(text: &str) -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("enigo init: {e}"))?;
    enigo.text(text)
        .map_err(|e| format!("type failed: {e}"))
}
```

### 6.2 Key Mapping (from OpenDex's `keyFromToken`)

OpenDex maps string key names to nut.js Key enum. NEXUS needs the same
for voice commands like "press enter", "press escape", "press control A":

```rust
fn parse_key(token: &str) -> Option<enigo::Key> {
    let t = token.trim().to_lowercase();
    let map: &[(&str, Key)] = &[
        ("enter", Key::Return), ("return", Key::Return),
        ("tab", Key::Tab), ("escape", Key::Escape), ("esc", Key::Escape),
        ("space", Key::Space), ("backspace", Key::Backspace),
        ("delete", Key::Delete), ("up", Key::Up), ("down", Key::Down),
        ("left", Key::Left), ("right", Key::Right),
        ("home", Key::Home), ("end", Key::End),
        ("ctrl", Key::Control), ("control", Key::Control),
        ("alt", Key::Alt), ("shift", Key::Shift),
    ];
    for (name, key) in map {
        if t == *name { return Some(key.clone()); }
    }
    if t.len() == 1 {
        if let Some(c) = t.chars().next() {
            return Some(Key::Unicode(c));
        }
    }
    None
}
```

---

## 7. uiautomation-rs — Find Element + Type

### 7.1 Open Notepad + Type Text (Official Sample)

Source: `samples/uia_notepad/src/main.rs`

```rust
use uiautomation::actions::Window;
use uiautomation::controls::WindowControl;
use uiautomation::core::UIAutomation;
use uiautomation::inputs::Keyboard;
use uiautomation::processes::Process;

// 1. Launch notepad
Process::create("notepad.exe").unwrap();

// 2. Find the notepad window
let automation = UIAutomation::new().unwrap();
let root = automation.get_root_element().unwrap();
let matcher = automation.create_matcher()
    .from(root)
    .timeout(10000)
    .classname("Notepad");
let notepad = matcher.find_first().unwrap();

// 3. Type text (multiple methods)
notepad.send_text_by_clipboard("From clipboard.\n").unwrap();
notepad.send_keys("Hello, Rust UIAutomation!", 10).unwrap();
notepad.send_text("\r\n{Win}D.", 10).unwrap();

// 4. Key combos
let kb = Keyboard::new().interval(10).ignore_parse_err(true);
kb.send_keys(" {None} (Keys).").unwrap();
notepad.hold_send_keys("{Ctrl}{Shift}", "{Left}{Left}", 50).unwrap();

// 5. Window control
let window: WindowControl = notepad.try_into().unwrap();
window.maximize().unwrap();
```

**For NEXUS's WhatsApp flow:**
```rust
fn whatsapp_find_and_type(contact: &str, message: &str) -> Result<(), String> {
    let automation = UIAutomation::new().map_err(|e| e.to_string())?;

    // Find WhatsApp window
    let matcher = automation.create_matcher()
        .contains_name("WhatsApp")
        .timeout(5000);
    let wa_window = matcher.find_first().map_err(|e| e.to_string())?;

    // Find the search box (Ctrl+F opens it in WhatsApp Desktop)
    let kb = Keyboard::new().interval(10);
    kb.send_keys("{Ctrl}f").map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(500));

    // Type contact name
    wa_window.send_text(contact).map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(500));

    // Press Enter to open the chat
    kb.send_keys("{Enter}").map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(500));

    // Type the message
    wa_window.send_text(message).map_err(|e| e.to_string())?;

    // Press Enter to send (REQUIRES CONFIRMATION)
    kb.send_keys("{Enter}").map_err(|e| e.to_string())?;

    Ok(())
}
```

### 7.2 UIMatcher for Finding Elements

Source: `uiautomation::core::UIMatcher`

```rust
let matcher = automation.create_matcher()
    .from(focused_element)
    .contains_name("Type a message")  // WhatsApp's input placeholder
    .timeout(2000);
let message_box = matcher.find_first()?;
message_box.send_text("hi what are u doing")?;
```

---

## 8. ghost-hands — Window Focus Trick

Source: `src/window.rs`

The critical `AttachThreadInput` workaround for Windows' foreground lock:

```rust
fn windows_focus(hwnd: HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::Win32::System::Threading::*;

    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);

        // SetForegroundWindow is rejected for background processes.
        // Attach our input queue to the foreground thread's to bypass this.
        let fg = GetForegroundWindow();
        let mut fg_pid = 0u32;
        let fg_thread = GetWindowThreadProcessId(fg, Some(&mut fg_pid));
        let this_thread = GetCurrentThreadId();

        let attached = fg_thread != 0
            && fg_thread != this_thread
            && AttachThreadInput(this_thread, fg_thread, true).as_bool();

        let _ = SetForegroundWindow(hwnd);
        let _ = BringWindowToTop(hwnd);

        if attached {
            let _ = AttachThreadInput(this_thread, fg_thread, false);
        }
        GetForegroundWindow() == hwnd
    }
}
```

**For NEXUS:** This is REQUIRED for any window focus operation. Without it,
`SetForegroundWindow` silently fails when NEXUS is in the background.

---

## 9. Concrete Implementation: Reusable Module Structure

Based on all the research, here's the proposed file structure for NEXUS
live mode, with each file mapped to its open-source inspiration:

```
src-tauri/src/live/
├── mod.rs                  # Module root, re-exports
├── state.rs                # LiveModeState state machine
│                           # Inspired by: AnovaX typed executors
├── command_registry.rs     # Priority-ordered command matching
│                           # Inspired by: Ari CommandRegistry
├── commands/
│   ├── mod.rs
│   ├── app_control.rs      # open_app, close_app, focus_app
│   │                       # Reuses: existing command_executor.rs
│   ├── keyboard.rs         # type_text, press_key, press_hotkey
│   │                       # Uses: enigo crate
│   ├── mouse.rs            # click, scroll (future)
│   │                       # Uses: enigo crate
│   ├── window.rs           # find_window, focus_window, maximize
│   │                       # Uses: windows crate + AttachThreadInput trick
│   │                       # Inspired by: ghost-hands window.rs
│   ├── uia.rs              # find_element, click_element, type_into
│   │                       # Uses: uiautomation-rs
│   │                       # Inspired by: SAM ScreenParser + uia_notepad sample
│   ├── whatsapp.rs         # whatsapp_open, whatsapp_search, whatsapp_send
│   │                       # Uses: keyboard shortcuts + UIA
│   ├── browser.rs          # browser_search, browser_navigate, browser_new_tab
│   │                       # Reuses: existing open_url/open_search
│   └── system.rs           # shutdown, restart, lock, volume
│                           # Inspired by: Ari SystemCommand
├── planner.rs             # LLM plan-and-execute (JSON plan → steps)
│                           # Inspired by: AnovaX plan schema
├── safety.rs              # Whitelist + denylist + confirmation gates
│                           # Inspired by: AnovaX safety filter + OpenDex permissions
└── executor.rs             # Sequential step executor with verification
                                # Inspired by: AnovaX orchestrator
```

---

## 10. Dependency Summary

### Rust Crates to Add

```toml
# Cargo.toml
[dependencies]
enigo = "0.3"                    # Cross-platform keyboard/mouse
uiautomation = { version = "0.25", features = ["input", "control"] }
                                 # Windows UI Automation (element finding)
arboard = "3"                    # Cross-platform clipboard (for pasteText pattern)
```

**Note:** `enigo` and `uiautomation` are both well-maintained, MIT/Apache
licensed, and have no heavy dependencies. `arboard` is used for the
clipboard-paste pattern from OpenDex (avoids autocomplete corruption).

### No New Python Dependencies

All live-mode functionality is in Rust. No Python sidecar needed for
desktop control. The existing NLU server (BERT-Mini) remains for intent
classification fallback only.

### No New Cloud Dependencies

All live-mode functionality is local. No API calls for desktop control.
The Worker is only used for research/PR analysis as before.

---

## 11. What We Can Build in Phase 1 (Immediate, High-Impact)

Based on the reusable patterns identified, here's what we can build
IMMEDIATELY with minimal new code:

### 11.1 Keyboard Simulation (1 file, ~100 lines)

```rust
// src-tauri/src/live/commands/keyboard.rs
use enigo::{Enigo, Key, Keyboard, Settings, Direction::{Click, Press, Release}};

pub fn type_text(text: &str) -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("enigo init: {e}"))?;
    enigo.text(text).map_err(|e| format!("type: {e}"))
}

pub fn press_key(key: &str) -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("enigo init: {e}"))?;
    let k = parse_key(key).ok_or(format!("unknown key: {key}"))?;
    enigo.key(k, Click).map_err(|e| format!("press: {e}"))
}

pub fn press_hotkey(keys: &[&str]) -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("enigo init: {e}"))?;
    // Press all modifiers
    for k in &keys[..keys.len()-1] {
        let key = parse_key(k).ok_or(format!("unknown key: {k}"))?;
        enigo.key(key, Press).map_err(|e| format!("press: {e}"))?;
    }
    // Click the last key
    let last = parse_key(keys[keys.len()-1])
        .ok_or(format!("unknown key: {}", keys[keys.len()-1]))?;
    enigo.key(last, Click).map_err(|e| format!("click: {e}"))?;
    // Release modifiers in reverse
    for k in &keys[..keys.len()-1].iter().rev() {
        let key = parse_key(k).ok_or(format!("unknown key: {k}"))?;
        enigo.key(key, Release).map_err(|e| format!("release: {e}"))?;
    }
    Ok(())
}
```

### 11.2 WhatsApp Send (1 file, ~80 lines)

```rust
// src-tauri/src/live/commands/whatsapp.rs
use std::time::Duration;
use std::thread;

pub fn whatsapp_send(contact: &str, message: &str) -> Result<(), String> {
    // 1. Open WhatsApp (reuse existing deep link)
    crate::command_executor::whatsapp_chat(contact)?;

    // 2. Wait for app to load
    thread::sleep(Duration::from_millis(2000));

    // 3. Type the message
    super::keyboard::type_text(message)?;

    // 4. Wait a bit
    thread::sleep(Duration::from_millis(300));

    // 5. Send (REQUIRES CONFIRMATION — handled by safety.rs)
    super::keyboard::press_key("enter")?;

    Ok(())
}
```

### 11.3 State Machine (1 file, ~60 lines)

```rust
// src-tauri/src/live/state.rs
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum LiveState {
    Idle,
    AppOpen { app: String },
    ChatActive { app: String, contact: String },
    TextTyped { app: String, contact: String, text: String },
}

pub struct LiveContext {
    pub state: LiveState,
    pub last_action: Instant,
    pub timeout: Duration,
}

impl LiveContext {
    pub fn new() -> Self {
        Self {
            state: LiveState::Idle,
            last_action: Instant::now(),
            timeout: Duration::from_secs(30),
        }
    }

    pub fn is_expired(&self) -> bool {
        self.last_action.elapsed() > self.timeout
    }

    pub fn reset(&mut self) {
        self.state = LiveState::Idle;
    }

    pub fn transition(&mut self, new_state: LiveState) {
        tracing::info!("live: {:?} → {:?}", self.state, new_state);
        self.state = new_state;
        self.last_action = Instant::now();
    }
}
```

---

## 12. Comparison: NEXUS vs. Open-Source Projects

| Feature | NEXUS (current) | AnovaX | OpenDex | Ari | SAM |
|---------|----------------|--------|---------|-----|-----|
| Wake word | ✅ OWW | ✅ | ✅ Vosk | ✅ | ❌ |
| STT | ✅ Groq+Moonshine | ✅ Whisper | ✅ Whisper/Vosk | ✅ Whisper | ❌ |
| VAD | ✅ Silero | ❌ | ❌ | ❌ | ❌ |
| Intent parser | ✅ Deterministic+BERT | ✅ LLM only | ✅ LLM only | ✅ Regex | ✅ LLM |
| Open app | ✅ AppRegistry | ✅ pyautogui | ✅ shell | ✅ | ❌ |
| Type text | ❌ | ✅ pyautogui | ✅ nut.js | ❌ | ❌ |
| Press keys | ❌ | ✅ | ✅ nut.js | ❌ | ❌ |
| Find UI elements | ❌ | ❌ (blind) | ✅ screenshots | ❌ | ✅ UIA+OCR |
| Window focus | ❌ | ❌ | ❌ | ❌ | ❌ |
| Multi-step plans | ❌ | ✅ JSON plan | ✅ LLM tools | ❌ | ✅ dispatch |
| State machine | ❌ | ❌ | ❌ | ❌ | ❌ |
| Safety filter | ❌ | ✅ whitelist | ✅ permissions | ✅ guard | ✅ id-only |
| Confirmation gates | ✅ GitHub only | ✅ | ✅ | ✅ | ❌ |
| TTS | ✅ Kokoro | ✅ | ✅ | ✅ CosyVoice | ❌ |
| Cross-platform | ✅ Win/Mac/Linux | ❌ Win | ✅ | ❌ Win | ❌ Win |
| Language | Rust | Python | TypeScript | Python | Python |

**NEXUS's unique advantages:**
1. **Rust core** — faster, more reliable than Python alternatives
2. **Deterministic parser first** — no LLM call for known commands
3. **Silero VAD** — best-in-class speech segmentation
4. **AppRegistry** — fastest app resolution (1ms vs 1.5s)
5. **Cross-platform** — most projects are Windows or macOS only

**What NEXUS needs to add (from the table above):**
1. Type text / press keys (from enigo)
2. Find UI elements (from uiautomation-rs)
3. Window focus (from ghost-hands pattern)
4. Multi-step plans (from AnovaX schema)
5. State machine (NEW — no project has this)
6. Safety filter (from AnovaX + OpenDex)

---

## 13. Recommended Implementation Order

Based on reusability and impact:

### Step 1: Keyboard Simulation (Day 1)
- Add `enigo` dependency
- Create `src-tauri/src/live/commands/keyboard.rs`
- Add intents: `type_text`, `press_key`, `press_hotkey`
- Test: type into Notepad

### Step 2: WhatsApp Full Flow (Day 2)
- Create `src-tauri/src/live/commands/whatsapp.rs`
- Implement: open → search contact → type → send (with confirmation)
- Test: "open whatsapp" → "chat with mummy" → "type hi" → "send"

### Step 3: State Machine (Day 3)
- Create `src-tauri/src/live/state.rs`
- Track context across sequential commands
- Auto-reset after 30s silence
- Test: multi-command sequences

### Step 4: Browser Navigation (Day 3)
- Extend existing `open_url`/`open_search`
- Add: "open new tab", "go to wikipedia", "search for X"
- Use keyboard shortcuts (Ctrl+T, Ctrl+L, type, Enter)

### Step 5: Window Focus (Day 4)
- Add `windows` crate functions (already a dependency)
- Implement `AttachThreadInput` focus trick
- Test: focus WhatsApp after opening it

### Step 6: UI Automation (Day 5-6)
- Add `uiautomation` dependency
- Create `src-tauri/src/live/commands/uia.rs`
- Find elements by name, click, type into them
- Test: find WhatsApp search box, type contact name

### Step 7: Safety Layer (Day 7)
- Create `src-tauri/src/live/safety.rs`
- Whitelist of allowed tools
- Denylist of forbidden targets (banking, password managers)
- Confirmation gates for send/delete/purchase
- Test: blocked actions, confirmation flow

### Step 8: Continuous Listening Mode (Day 8-9)
- Bypass wake word when live mode is on
- VAD-driven endpointing (already have Silero)
- STT → parse → execute → resume VAD
- Visual indicator (orb color change)
- Test: 10 consecutive commands without wake word

### Step 9: LLM Plan-and-Execute (Day 10-12, optional)
- Add tool schema to Worker's LLM endpoint
- Worker returns JSON plan for complex commands
- Rust executes plan with safety checks
- Test: novel multi-step commands

**Total: 7-9 days for core functionality, 10-12 with LLM planning.**

---

## 14. Key Takeaways

1. **Don't reinvent the wheel.** Every piece of functionality we need
   has working open-source code we can adapt.

2. **Rust is an advantage, not a constraint.** `enigo` and `uiautomation-rs`
   provide the same capabilities as Python's pyautogui/pywinauto, but
   compiled and cross-platform.

3. **The two-table pattern from SAM ScreenParser is critical for safety.**
   If we ever use LLM for GUI control, the LLM should never see pixel
   coordinates. It picks element IDs; Rust resolves them.

4. **Clipboard paste > character typing.** OpenDex's `pasteText` pattern
   avoids autocomplete corruption. Use `arboard` crate in Rust.

5. **The AttachThreadInput trick is mandatory on Windows.** Without it,
   `SetForegroundWindow` silently fails from a background process.

6. **NEXUS's deterministic-first philosophy is validated by all research.**
   Agent-Sam's "model is dispatcher, skills are workers" and AnovaX's
   "LLM plans once, Python orchestrates" both match NEXUS's existing
   architecture. We just need to add the "workers" (keyboard, mouse, UIA).

7. **The state machine is NEXUS's unique contribution.** No open-source
   project tracks context across sequential voice commands. They all
   either do single commands (Ari, OpenDex) or full LLM plans (AnovaX).
   NEXUS's state machine approach is simpler and more predictable.
