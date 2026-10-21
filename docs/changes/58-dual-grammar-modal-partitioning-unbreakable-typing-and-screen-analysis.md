# Dual-Grammar Modal Partitioning, Unbreakable Typing & Screen Analysis — Implementation Record

**Date:** 2026-10-01  
**Plan & Research:** `docs/research/ghost-mode/06-dual-grammar-modal-partitioning-and-universal-grounding-architecture-2026-10-01.md`  
**Feature Spec:** `docs/features/81-dual-grammar-modal-partitioning-and-foreground-grounding.md`  
**Status:** IMPLEMENTED, TESTED (Rust 750/750 passed; Frontend 72/72 passed) & RELEASE COMPILED (`nexus.exe`, 50.9 MB)  
**Predecessor Change:** `docs/changes/57-ghost-fifo-queue-and-hotkey-realignment.md`  

---

## 0. Executive Summary of What Was Built

Following user feedback on Ghost Mode live field testing:
1. **Unbreakable Typing Enclave (`parse_type_dictation_command`):** Fixed fatal collisions where dictating text in messaging apps (e.g., *"type so it analysis for the servx right send it to as soon as possible"*) was intercepted by `parse_analyse_command` or other parsers due to keywords like `"analysis"` or `"send"`. Added a top-priority dictation parser ahead of all domain parsers, enabling clean verbatim typing into active windows.
2. **Search Acoustic Soundalike Hardening:** Handled Whisper mishearings where *"Search for almonds"* arrived as `'So it's for almonds.'` (which was then stripped of leading `"so"` and fell through to cloud conversational backend). Added `"so it's for"`, `"it's for"`, `"it is for"`, and `"search that on"` to phonetic normalizers and search verbs.
3. **Numbered Search Result & Screen Click Matching:** Expanded ordinal parsing in `screen.rs` to understand word numerals (`"first"`, `"second"`, `"third"`, `"one"`, `"two"`, `"three"`) and added flexible regex patterns for *"open result 2"*, *"open the 2 in the result"*, and *"click the second result"*.
4. **VLM Desktop Screen Analysis & Global Prefix Gating:** Implemented `run_screen_analysis` in `orchestrator.rs` wired to Groq Llama-4-Scout VLM via `vision::capture_gridded_jpeg_base64()`. Gated global analysis behind `"Nexus <action>"` (e.g., *"Nexus analyse the screen"*, *"Nexus explain what is on my screen"*), preserving local window grounding for unprefixed speech.

---

## 1. File-by-File Change Matrix

### 1.1 `src-tauri/src/screen.rs`
- **Location:** `parse_ordinal()` function (~lines 51–77).
- **What:** Added word numeral mapping (`"first"` / `"one"` $\to$ `1`, `"second"` / `"two"` $\to$ `2`, `"third"` / `"three"` $\to$ `3`, `"fourth"` / `"four"` $\to$ `4`, up to 10) in addition to digit regexes (`(\d+)(?:st|nd|rd|th)?`).
- **Why:** In natural voice, users alternate between digit ordinals (*"click the 2nd result"*), spoken cardinals (*"open 2 in the result"*), and word ordinals (*"click the second result"*).
- **Tests Added:** `test_parse_ordinal_words_and_digits` testing both word numbers and digits.

### 1.2 `src-tauri/src/intent_parser.rs`
- **Location:** Top of `parse_deterministic()` (~lines 292–305), `normalize_phonetic_mishearings()`, and `parse_live_command()`.
- **What:**
  1. Inserted `parse_type_dictation_command` at the very beginning of `parse_deterministic()`, evaluated **before** `parse_analyse_command`, `parse_github_command`, or `parse_mail_command`.
  2. Implemented `parse_type_dictation_command`:
     - Checks if the input represents a dictation toggle (`"type whatever I say"`, `"stop typing"`).
     - Captures bare `"type"` and transitions to `StartDictation`.
     - Strips `"type "` prefix from the payload and preserves the remainder as literal text, returning `ParsedIntent::DictationTypedText { text }` or routing to active dictation injection.
  3. Added soundalike replacements:
     - `"so it's for "` $\to$ `"search for "`
     - `"so its for "` $\to$ `"search for "`
     - `"so it is for "` $\to$ `"search for "`
     - `"it's for "` $\to$ `"search for "`
     - `"it is for "` $\to$ `"search for "`
     - `"search that on "` $\to$ `"search for "`
  4. Added `parse_screen_analysis_command` for expressions like *"analyse the screen"*, *"explain what is on my screen"*, *"explain what is this in the screen"*, *"research on this for me"*.
  5. Expanded `ScreenClick` matching for numbered search results (*"open result 2"*, *"open the 2 in the result"*, *"click the second result"*).
- **Why:** Solves user-reported failures from live testing transcript where typing collided with command words, and search soundalikes fell through to cloud LLM.
- **Tests Added:** 5 new unit tests (`test_parse_type_dictation_raw_payload`, `test_parse_type_dictation_bare_starts`, `test_parse_screen_analysis_intents`, `test_parse_search_soundalikes_resilience`, `test_parse_numbered_result_clicks`).

### 1.3 `src-tauri/src/orchestrator.rs`
- **Location:** Match arm in `run_live_command()` and `run_ghost_command()`, plus helper `run_screen_analysis()`.
- **What:**
  1. Handled `ParsedIntent::ScreenClick` and `ParsedIntent::ScreenRead` inside the active Ghost Mode executor.
  2. Implemented `run_screen_analysis`:
     - Speaks immediate cached confirmation: *"Analyzing the screen, sir."* (<5ms).
     - Captures desktop screenshot via `vision::capture_gridded_jpeg_base64(1024, 80)`.
     - Queries Groq VLM (`llama-4-scout-17b-16e-instruct`) with desktop analysis prompt.
     - Speaks the distilled visual analysis through Edge-TTS.
- **Why:** Fulfills the user requirement for multi-modal screen analysis triggered seamlessly within Ghost Mode.

---

## 2. Test Verification Gates

### 2.1 Rust Unit & Integration Tests
Ran `cargo test --lib -- --test-threads=1`:
```
running 750 tests
...
test intent_parser::tests::test_parse_type_dictation_raw_payload ... ok
test intent_parser::tests::test_parse_type_dictation_bare_starts ... ok
test intent_parser::tests::test_parse_screen_analysis_intents ... ok
test intent_parser::tests::test_parse_search_soundalikes_resilience ... ok
test intent_parser::tests::test_parse_numbered_result_clicks ... ok
test screen::tests::test_parse_ordinal_words_and_digits ... ok

test result: ok. 750 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### 2.2 Frontend Unit Tests
Ran `npm test -- --run` in `frontend/`:
```
Test Files  6 passed (6)
     Tests  72 passed (72)
  Start at  01:23:45
  Duration  1.68s
```

### 2.3 Production Release Compilation
Executed according to the strict release build invariant:
1. `npm run build` in `frontend/` $\to$ clean static export in `dist/`.
2. `cargo build --release --features custom-protocol` in `src-tauri/` $\to$ clean release binary at `target/release/nexus.exe` (50.9 MB).
