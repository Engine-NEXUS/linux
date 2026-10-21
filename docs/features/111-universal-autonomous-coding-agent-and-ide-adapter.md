# Feature 111: Universal Autonomous Coding Agent & AI IDE Adapter

## 1. Executive Summary & User Experience

Modern software engineering relies on an ecosystem of specialized AI coding agents:
- **Autonomous CLI Agents**: [Google Antigravity (`agy`)](https://github.com), [Anthropic Claude Code (`claude`)](https://github.com/anthropics/claude-code), [Aider (`aider`)](https://github.com/Aider-AI/aider), Codex, and Kilo.
- **AI-Powered IDEs**: Cursor, Trae, OpenCode, Windsurf, and VS Code.

Instead of manually writing prompts and copying context, the user asks NEXUS:
- *"Have Claude Code build the authentication module from Eesah's chat."*
- *"Use Antigravity to create a website for Eesah."*
- *"Prompt Cursor composer to write the integration tests."*
- *"Have Trae fix the broken build."*

NEXUS synthesizes a structured prompt, selects the correct agent runner, focuses the workspace, and initiates execution.

---

## 2. System Architecture & Lifecycle

```mermaid
flowchart TD
    Voice["User Voice Command"] --> Parser["intent_parser.rs"]
    Parser --> Intent["ParsedIntent::CodingAgentTask / CodeProject"]
    Intent --> Dispatcher["coder::agents::dispatch_agent_task"]

    Dispatcher --> TypeCheck{"Agent Modality?"}

    TypeCheck -->|CLI Agent| CLI["Headless CLI Runner"]
    CLI --> RunAgy["agy run \"{structured_prompt}\""]
    CLI --> RunClaude["claude -p \"{structured_prompt}\""]
    CLI --> RunAider["aider --message \"{structured_prompt}\""]

    TypeCheck -->|AI IDE| IDE["IDE Desktop Adapter"]
    IDE --> Focus["Win32 Focus & Bring Window to Front"]
    Focus --> Shortcut["Send Shortcut Router Chord (Ctrl+I / Ctrl+L)"]
    Shortcut --> Inject["Inject Synthesized Prompt via Win32 SendInput"]
```

---

## 3. Supported Agents & Resolution Precedence

Implemented in `src-tauri/src/coder/agents.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodingAgent {
    Antigravity,
    Claude,
    Aider,
    Codex,
    Kilo,
    Cursor,
    Trae,
    OpenCode,
    Windsurf,
    VSCode,
}
```

### Substring Resolution Precedence
Because phrases like *"claude code"* contain the word *"code"*, naive substring matching would falsely resolve to VS Code (`code.cmd`).
The resolver strictly checks names in order of specificity:
1. `antigravity` / `agy` $\rightarrow$ `CodingAgent::Antigravity`
2. `claude` / `claude code` $\rightarrow$ `CodingAgent::Claude`
3. `aider` $\rightarrow$ `CodingAgent::Aider`
4. `cursor` $\rightarrow$ `CodingAgent::Cursor`
5. `trae` $\rightarrow$ `CodingAgent::Trae`
6. `opencode` $\rightarrow$ `CodingAgent::OpenCode`
7. `windsurf` $\rightarrow$ `CodingAgent::Windsurf`
8. `vscode` / `code` $\rightarrow$ `CodingAgent::VSCode`

### Modality Partitioning
- **`agent.is_cli()`**: Returns `true` for Antigravity, Claude, Aider, Codex, Kilo. Dispatches background subprocesses with non-blocking stdout capture.
- **`agent.is_ide()`**: Returns `true` for Cursor, Trae, OpenCode, Windsurf, VS Code. Executes GUI focus and shortcut activation.

---

## 4. Structured Prompt Synthesis

Raw voice queries are unstructured (e.g. *"make eesah's website"*). Spawning an autonomous agent with raw speech leads to ambiguous outputs.

`coder::agents::build_autonomous_prompt(raw_task: &str, context: Option<&str>)` synthesizes an enterprise-grade agent prompt:

```markdown
# TASK SPECIFICATION
Implement the following objective with strict adherence to existing project patterns:
> {raw_task}

## RELEVANT CONTEXT & REQUIREMENTS
- Specification Source: {context}
- Verification: Run unit tests and lint checks before completing the turn.
- Quality: Ensure zero regressions and follow idiomatic coding practices.
```

---

## 5. IDE Desktop Automation (Shortcut Injection)

When dispatching to an IDE (e.g. Cursor Composer or Trae):
1. **Window Focus**: Queries `EnumWindows` to match process handle (`Cursor.exe`, `Trae.exe`).
2. **Bring to Front**: Calls Win32 `SetForegroundWindow` and `ShowWindow(SW_RESTORE)`.
3. **Trigger AI Composer**:
   - Cursor: Fires <kbd>Ctrl+I</kbd> (Composer) or <kbd>Ctrl+L</kbd> (Chat).
   - Trae: Fires <kbd>Ctrl+U</kbd> or <kbd>Ctrl+I</kbd>.
   - VS Code: Fires <kbd>Ctrl+I</kbd> (GitHub Copilot inline) or <kbd>Ctrl+Shift+I</kbd>.
4. **Prompt Injection**: Injects the synthesized prompt using the layout-independent Unicode input pipeline in `src-tauri/src/shortcuts/keys.rs`.

---

## 6. Failure Modes & Edge Cases

| Failure Mode | Mitigation |
|---|---|
| Requested CLI tool not installed in PATH | Checks executable existence (`where.exe agy`, `where.exe claude`). If missing, suggests alternative installed agents (e.g. *"Claude Code is not installed in your PATH. Would you like me to use Antigravity instead?"*). |
| Target IDE window is minimized or closed | Spawns the IDE binary directly in the project directory (`cursor <dir>`, `code <dir>`). |
| Ambiguous agent name | Defaults to user's configured preferred agent in Command Hub (`defaultCodingAgent`, default: `Antigravity`). |

---

## 7. Verification & Test Suite

1. **Agent Resolution & Precedence Test**:
   ```bash
   cargo test --lib -- coder::agents::tests::test_coding_agent_resolution
   ```
2. **Autonomous Prompt Synthesis Test**:
   ```bash
   cargo test --lib -- coder::agents::tests::test_build_autonomous_prompt
   ```
3. **Intent Parsing Test**:
   ```bash
   cargo test --lib -- intent_parser::power_and_session_tests::test_coding_command_parsing
   ```
