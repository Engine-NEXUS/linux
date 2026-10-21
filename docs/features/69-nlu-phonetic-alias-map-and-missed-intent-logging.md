# Feature 69: NLU Phonetic Alias Map & Persistent Missed-Intent Logging

**Status:** BUILT & VERIFIED  
**Date:** 2026-09-25  
**Platform:** Windows 11  
**Architecture Layer:** Layer 3 (NLU Normalization & Audit Logging)  

---

## 1. Executive Summary

Voice assistants cannot rely exclusively on STT prompt seeding (Whisper caps prompts at ~244 tokens, and dictionary-stuffing increases Word Error Rate by 5–15%). Real-world vocal input produces natural phonetic variations, dialect accents, and acoustic confabulations (e.g. *"goes to mode"*, *"post modern"*, *"open what's up"*, *"vs coat"*).

This feature implements the **Layer 3 Defense-in-Depth Architecture**:
1. **Deterministic Phonetic Alias Normalizer**: High-speed (< 0.1ms) soundalike phrase mapping before intent parsing and within the frontend post-processor.
2. **Ghost Mode Acoustic Parity**: Immediate recognition of 15+ common STT soundalikes (*"goes to mode"*, *"goes to mold"*, *"post modern"*, *"postmodern"*, *"coast mode"*, *"gold mode"*, *"ghost mood"*, etc.) entering cursor control without false rejections.
3. **App & Command Soundalikes**: Automatic canonicalization of *"what's up"* → WhatsApp, *"vs coat"* → VS Code, *"spot if I"* → Spotify, *"this cord"* → Discord, *"u tube"* → YouTube, *"note pad"* → Notepad, *"stand down"* → stop.
4. **Persistent Missed-Intent Logging**: Every unmatched transcript that escapes deterministic/ML intent parsing is automatically recorded to `%APPDATA%/com.nexus.assistant/missed_intents.jsonl` with millisecond timestamps, reasons, and subsystem origins, establishing an empirical data flywheel for continuous vocabulary expansion.

---

## 2. Technical Architecture

```
[Spoken Utterance] ──> [Groq / Whisper STT]
                               │
                               ▼ Transcript (e.g. "goes to mode", "open what's up")
                    ┌──────────────────────────────────────────────┐
                    │       Layer 3: Phonetic Alias Normalizer     │
                    │   • Exact phrase map ("goes to mode" → ghost)│
                    │   • Contextual app soundalikes (open, on, in)│
                    │   • STOP_PHRASES expansion (stand down, etc.)│
                    └──────────────────────────────────────────────┘
                               │
                               ▼ Normalized ("ghost mode", "open whatsapp")
                    ┌──────────────────────────────────────────────┐
                    │             Intent Parser Dispatch           │
                    │  Matched  ──────> Execute (Local / Ghost)    │
                    │  Unmatched ─────> Missed-Intent Logger       │
                    └──────────────────────────────────────────────┘
                               │
                               ▼
               %APPDATA%/com.nexus.assistant/missed_intents.jsonl
               (Auto-rotating JSONL audit trail, 5MB cap)
```

---

## 3. Implementation Details

### A. Intent Parser Normalization (`src-tauri/src/intent_parser.rs`)
- Added `normalize_phonetic_mishearings(text: &str) -> String` called at the entry of `parse_deterministic` after whitespace/punctuation stripping.
- Expanded `parse_ghost_control_entry` TRIGGERS table with `"activate ghost mode"`, `"activate ghost"`, `"turn on ghost mode"`, and full acoustic soundalike phrases.
- Expanded `STOP_PHRASES` in `src-tauri/src/ghost.rs` with `"stand down"`, `"stop please"`, `"exit ghost"`, `"stop ghost"`, `"exit ghost mode"`, `"leave ghost mode"`.

### B. Frontend Audio Post-Processing (`frontend/src/audio/recorder.ts`)
- Added regex-based acoustic soundalike normalizer in `correctSttTranscript(transcript: string)` matching ghost mode confabulations and app prefixes (`open`, `launch`, `start`, `close`, `on`, `in`, `to`, `via`).

### C. Missed-Intent Logger (`src-tauri/src/missed_intent_logger.rs`)
- Thread-safe append to `%APPDATA%/com.nexus.assistant/missed_intents.jsonl`.
- Structure:
  ```json
  {
    "timestamp_ms": 1727280000000,
    "datetime": "21:00:00 UTC",
    "transcript": "unrecognized query",
    "source": "orchestrator",
    "reason": "deterministic_miss"
  }
  ```
- Registered Tauri command `get_missed_intents(limit: Option<usize>)` for runtime inspection and tooling.
- Auto-rotates to `.old.jsonl` when exceeding 5 MB.

---

## 4. Verification & Benchmarks

1. **Rust Intent Parser Unit Tests**: 173/173 tests passed (`cargo test --lib intent_parser -- --test-threads=1`).
2. **Phonetic Soundalike Battery**:
   - `goes to mode` → `EnterGhostControl` (Pass)
   - `goes to mold` → `EnterGhostControl` (Pass)
   - `post modern` → `EnterGhostControl` (Pass)
   - `postmodern` → `EnterGhostControl` (Pass)
   - `activate ghost mode` → `EnterGhostControl` (Pass)
   - `open what's up` → `OpenApp("whatsapp")` (Pass)
   - `open vs coat` → `OpenApp("vs code")` (Pass)
   - `open spot if I` → `OpenApp("spotify")` (Pass)
3. **Logger Unit Tests**: 1/1 test passed (`cargo test --lib missed_intent_logger`).
4. **Frontend Unit Tests**: 33/33 tests passed (`npm --prefix frontend test`).
