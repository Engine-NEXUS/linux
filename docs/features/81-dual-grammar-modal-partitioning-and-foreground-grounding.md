# Feature 81 — Dual-Grammar Modal Partitioning, Foreground Dynamic Grounding & Vision Screen Analysis

**Date:** 2026-10-01  
**Status:** IMPLEMENTED & VERIFIED (Rust 750/750 tests passing; Frontend 72/72 Vitest passing; Release Binary compiled)  
**Research Spec:** `docs/research/ghost-mode/06-dual-grammar-modal-partitioning-and-universal-grounding-architecture-2026-10-01.md`  
**Change Ledger:** `docs/changes/58-dual-grammar-modal-partitioning-unbreakable-typing-and-screen-analysis.md`  
**Related Features:** Feature 80 (`80-production-orb-creator-mcp-pipeline.md`), Feature 76 (`76-vision-mode-screen-email-scanning-and-proactive-watch-memory.md`), Change 57 (`57-ghost-fifo-queue-and-hotkey-realignment.md`)  

---

## 1. Executive Summary & Problem Definition

### 1.1 The Live Test Failures
During live Ghost Mode field testing, three critical interaction friction points were identified:

1. **Phonetic STT Corruption on Browser Search:**
   When the user spoke *"Search for almonds"*, Whisper's cloud/local STT transcribed the acoustic waveform as `'So it's for almonds.'`. The leading filler word filter stripped `"so"`, leaving `"it's for almonds"`. The existing parser only matched explicit search triggers (`"search"`, `"google"`, `"lookup"`), causing the utterance to fall through to `WorkerBackend` (cloud conversational chat). The browser search bar was never activated.

2. **Dictation Collisions with Lexical Command Keywords:**
   When dictating in messaging applications (e.g., WhatsApp, Telegram, Discord), natural sentences frequently contain command-reserved words:
   > *"type so it analysis for the servx right send it to as soon as possible"*
   
   Because `parse_analyse_command` and `parse_send_whatsapp_message` were evaluated higher in the intent pipeline than text input handling, the word `"analysis"` hijacked the intent parser into attempting a repository architecture scan, while words like `"send"` risked triggering premature message delivery. Furthermore, saying bare `'Type.'` fell to `WorkerBackend`.

3. **Global Multi-Modal Intent Ambiguity:**
   In Ghost Mode, the system's hot-mic loop previously could not distinguish whether a phrase like *"search on this repository"* or *"analyse this"* was intended for the active window or as an overarching system-level multi-modal request (VLM screen inspection, OCR, codebase exploration).

---

## 2. Architectural Solution: Dual-Grammar Modal Partitioning

To solve these collisions without introducing modal latency or locking the user into a rigid single-app container, NEXUS implements **Dual-Grammar Modal Partitioning with Foreground Dynamic Grounding**:

```
                                  [ Audio Stream ]
                                         │
                                [ Whisper STT Engine ]
                                         │
                              [ Raw Transcript Text ]
                                         │
                                         ▼
                     ┌───────────────────────────────────────┐
                     │ Intent Pipeline: parse_deterministic  │
                     └───────────────────────────────────────┘
                                         │
                 ┌───────────────────────┴───────────────────────┐
                 │                                               │
   [ Lexical Prefix: "type <text>" ]             [ Lexical Prefix: "Nexus <action>" ]
                 │                                               │
                 ▼                                               ▼
    ┌─────────────────────────────┐               ┌─────────────────────────────┐
    │   L1: Dictation Enclave     │               │   L2: Global Multi-Modal    │
    │  (Raw Keystroke Injection)  │               │      System Breakout        │
    └─────────────────────────────┘               └─────────────────────────────┘
                 │                                               │
                 │ (No command parsing)                          ├─ "Nexus analyse screen" -> VLM Scout
                 ▼                                               ├─ "Nexus search <query>" -> Web Engine
       [ Windows SendInput ]                                     └─ "Nexus explain..."     -> Vision OCR
```

### 2.1 Grammar Partition 1: Unbreakable Typing Enclave (`L1`)
Any phrase matching `^type\s+(.+)$` or `^type$` is immediately captured by `parse_type_dictation_command` at the **very top** of `parse_deterministic` before any domain parsers (such as `parse_analyse_command`, `parse_github_command`, `parse_mail_command`) run.

- **Explicit Dictation Toggles:** If the text following `"type"` matches a dictation state change (*"type whatever I say"*, *"type line by line"*, *"stop typing"*), it toggles atomic dictation state (`DICTATION_ACTIVE`).
- **Literal Payload Typing:** If dictation state is already active or a literal phrase follows (*"type so it analysis for the servx..."*), the entire payload following `"type "` is extracted verbatim without any keyword filtering, and typed directly into the active foreground window via native Windows `SendInput`.
- **Bare `"type"` Fallback:** Uttering bare `"type"` or `"start typing"` enables continuous line-by-line dictation and speaks the pre-cached confirmation *"Dictation active, sir."* (<5ms).

### 2.2 Grammar Partition 2: Prefix-Gated Global Breakout (`L2`)
System-level, out-of-context operations are gated behind the explicit `"Nexus"` prefix:
- `"Nexus analyse the screen"` / `"Nexus explain what is on my screen"`: Triggers instantaneous desktop screenshot capture, grid encoding, and Groq Llama-4-Scout VLM query.
- `"Nexus search <query>"`: Bypasses window-specific browser controls to execute a system-wide web knowledge search.
- `"Nexus check repository <target>"`: Invokes the architecture mapper and codebase indexer.

When the `"Nexus"` prefix is absent in Ghost Mode, commands are strictly **locally grounded** to the active foreground process.

---

## 3. Dynamic Foreground Grounding (Universal Windows Support)

NEXUS does not hardcode browser-only control. The orchestrator and input engine inspect the active foreground window (`GetForegroundWindow()`, `GetWindowTextW()`) to execute context-appropriate actions:

| Foreground App | Local Utterance | Resolved Action | Sub-5ms Hotkey / Native API |
|---|---|---|---|
| **Brave / Chrome / Edge** | *"Search for almonds"* / *"So it's for almonds"* | Address Bar Search | `Ctrl+L` $\to$ `SendInput("almonds")` $\to$ `Enter` |
| **Brave / Chrome / Edge** | *"Open result 2"* / *"Open the 2 in the result"* | Numbered Result Click | Regex ordinal parsing $\to$ Screen OCR/DOM coordinate click |
| **Brave / Chrome / Edge** | *"Shift to tab 2"* | Tab Navigation | `Ctrl+2` |
| **Brave / Chrome / Edge** | *"New tab"* | Tab Creation | `Ctrl+T` |
| **WhatsApp / Discord / Slack** | *"Type so it analysis for the servx..."* | Direct Keystroke Typing | `keyboard::type_text(...)` |
| **Any Window** | *"Nexus analyse screen"* | VLM Multi-Modal Scan | `vision::capture_gridded_jpeg_base64()` $\to$ Groq Scout |

---

## 4. Numbered Search Result Grounding & Screen Click Expansion

Users naturally refer to search results using cardinals, ordinals, or colloquial phrasing. To handle this without fragile DOM bindings, `screen::parse_ordinal` and `intent_parser::parse_live_command` were expanded:

### 4.1 Word Number and Ordinal Parsing
[`src-tauri/src/screen.rs`](file:///c:/PROJECTS/ULTRON/src-tauri/src/screen.rs) maps textual numbers directly to numeric indices:
- `"first"` / `"1st"` / `"one"` $\to$ `1`
- `"second"` / `"2nd"` / `"two"` $\to$ `2`
- `"third"` / `"3rd"` / `"three"` $\to$ `3`
- `"fourth"` / `"4th"` / `"four"` $\to$ `4`
- Up to index 10.

### 4.2 Coloquial Utterance Matchers
[`src-tauri/src/intent_parser.rs`](file:///c:/PROJECTS/ULTRON/src-tauri/src/intent_parser.rs) maps the following patterns to `ParsedIntent::ScreenClick`:
- `"open result <N>"`
- `"open the <N> in the result"` / `"open the <N> in results"`
- `"open the <N>th result"`
- `"click result <N>"` / `"click the <N>nd result"`
- `"select result <N>"` / `"choose result <N>"`

---

## 5. Verification & Test Gate Results

All changes underwent rigorous unit and integration testing:

```
running 750 tests
test intent_parser::tests::test_parse_type_dictation_raw_payload ... ok
test intent_parser::tests::test_parse_type_dictation_bare_starts ... ok
test intent_parser::tests::test_parse_screen_analysis_intents ... ok
test intent_parser::tests::test_parse_search_soundalikes_resilience ... ok
test intent_parser::tests::test_parse_numbered_result_clicks ... ok
test screen::tests::test_parse_ordinal_words_and_digits ... ok
...
test result: ok. 750 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

- **Rust Test Suite:** 750/750 library tests passing (`cargo test --lib -- --test-threads=1`).
- **Frontend Vitest Suite:** 72/72 tests passing (`npm test -- --run`).
- **Release Build:** `nexus.exe` freshly compiled with custom protocol support (50.9 MB).
