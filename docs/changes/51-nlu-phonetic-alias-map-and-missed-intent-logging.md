# Change 51: NLU Phonetic Alias Map & Persistent Missed-Intent Logging

**Date:** 2026-09-25  
**Domain:** NLU, Intent Parsing, Telemetry & Logging  

---

## 1. Summary of Changes

### `src-tauri/src/intent_parser.rs`
- **`normalize_phonetic_mishearings()`**: Added phonetic normalizer before deterministic intent matching. Handles exact short soundalikes (*"goes to mode"*, *"post modern"*, *"open what's up"*, *"vs coat"*, *"stand down"*) and contextual multi-word app phrases.
- **`parse_ghost_control_entry()`**: Added extended triggers (`"activate ghost mode"`, `"activate ghost"`, `"turn on ghost mode"`) and soundalike phrases (*"goes to mode"*, *"goes to mold"*, *"post modern"*, etc.).
- **Unit Tests**: Added `test_ghost_mode_phonetic_soundalikes` and `test_app_phonetic_mishearings`; all 173/173 tests pass.

### `src-tauri/src/missed_intent_logger.rs` (NEW)
- Implemented persistent JSONL logger writing to `%APPDATA%/com.nexus.assistant/missed_intents.jsonl`.
- Logs timestamp, ISO datetime, transcript, source (`orchestrator` / `stt_learning`), and failure reason.
- Implemented auto-rotation at 5 MB.
- Added `get_missed_intents` IPC command.

### `src-tauri/src/orchestrator.rs`
- Hooked `missed_intent_logger::log_missed_intent` into `process_transcript` when deterministic/ML parsing produces `ParsedIntent::Unknown`.

### `src-tauri/src/stt_learning.rs`
- Hooked `missed_intent_logger::log_missed_intent` into `log_failure` to preserve all unparsed transcripts to disk.

### `src-tauri/src/ghost.rs`
- Added `"stand down"`, `"stop please"`, `"exit ghost"`, `"stop ghost"`, `"exit ghost mode"`, `"leave ghost mode"`, `"exit goes to mode"` to `STOP_PHRASES`.

### `frontend/src/audio/recorder.ts`
- Added phonetic mishearing regexes to `correctSttTranscript(transcript)` covering ghost mode soundalikes, app command soundalikes, and voice stop words.

---

## 2. Verification

- Rust tests: 173/173 intent parser tests pass (`cargo test --lib intent_parser -- --test-threads=1`)
- Logger tests: 1/1 test passes (`cargo test --lib missed_intent_logger`)
- Frontend tests: 33/33 tests pass (`npm --prefix frontend test`)
