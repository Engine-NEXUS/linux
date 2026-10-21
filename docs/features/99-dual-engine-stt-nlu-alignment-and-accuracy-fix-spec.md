# Feature Spec 99: Dual-Engine STT ↔ BERT-Mini NLU Alignment & 50% Accuracy Fix

**Target Systems**: `src-tauri/src/stt_groq.rs`, `src-tauri/src/stt_deepgram.rs`, `src-tauri/src/intent_parser.rs`, `src-tauri/src/nlu_client.rs`, `src-tauri/src/stt_learning.rs`  
**Related Docs**: `docs/research/stt/05-dual-engine-stt-nlu-alignment-and-50pct-accuracy-fix-2026-10-07.md`  
**Status**: Ready for Implementation by Muse Spark

---

## 1. Executive Summary

Voice command recognition in NEXUS currently suffers from a ~50% mishearing/failure rate. This is not purely an audio model issue; it is a cumulative cascading failure across 5 distinct pipeline layers:
1. **ASR proper noun / homophone errors**: Whisper Large v3 Turbo mishears domain keywords (e.g. "ghost mode" → "post mode", "zync" → "sink", "servx" → "cervix").
2. **Missing phonetic normalizations**: `normalize_phonetic_mishearings()` catches ~200 fixed phrases, but lacks newer additions like "ghost right", "memory odd it", etc.
3. **BERT-Mini confident hallucinations**: BERT-Mini classifies an unmatched or slightly misheard phrase into an incorrect intent (e.g. "So you have to list" → `MediaPlayPause` at 0.90 confidence), preventing fallback to the brain or Cloudflare Worker.
4. **Deterministic parser gaps**: Strict regexes fail on natural phrasing variations.
5. **Acoustic energy / silence pacing**: Short words get clipped if trailing silence or energy RMS gates reject early buffers.

---

## 2. Implementation Blueprint for Muse Spark

### Phase 1: Expand Whisper Decoder Vocabulary (`src-tauri/src/stt_groq.rs`)
In `src-tauri/src/stt_groq.rs`, update `NEXUS_VOCABULARY` (line 29) to bias Whisper Large v3 Turbo towards all domain verbs, entities, and actions up to the ~244-token limit:
```rust
pub const NEXUS_VOCABULARY: &str = "Ghost mode. Ghost control. Exit ghost mode. \
Open WhatsApp. Open Chrome. Open VS Code. Open Spotify. Open Discord. \
Open Brave. Open YouTube. Open settings. Open architecture mapper. \
Analyse Servx. Analyse Zync. GitHub pull request. Merge pull request. \
List pull requests. Search for. Play pause. Next track. Previous track. \
Memory audit. Forget everything. Take the mouse. Control my cursor. \
Type this. Press enter. Click the button. Search the web. \
Ghostwriter. Take a letter. Order food. Send message. \
Deepgram Groq Supabase Eesha Prem Lakshya Congi Shopkart NEXUS \
open close stop cancel navigate analyse search type click press scroll.";
```

### Phase 2: Add Deepgram Keyterms Boosting (`src-tauri/src/stt_deepgram.rs`)
When Deepgram Nova-2 is active (via `deepgramApiKey`), append query parameter `&keyterm=NEXUS&keyterm=ghost+mode&keyterm=servx&keyterm=zync&keyterm=whatsapp` to boost beam search probabilities on domain terms by +30–40%.

### Phase 3: Expand Phonetic Mishearing Normalizations (`src-tauri/src/intent_parser.rs`)
In `normalize_phonetic_mishearings()` (~L4399):
Add common mishearings identified from usage:
- `"ghost right"`, `"goes mode"`, `"post mode"` → `"ghost mode"`
- `"memory odd it"`, `"memory or dit"`, `"memory audit"` → `"memory audit"`
- `"forget every thing"` → `"forget everything"`
- `"what's up message"`, `"what sap message"` → `"whatsapp message"`
- `"open brave browser"` → `"open brave"`
- `"architecture diagram"` → `"open architecture mapper"`

### Phase 4: Guard BERT-Mini Against Confident Hallucinations (`src-tauri/src/nlu_client.rs`)
In `nlu_client::parse_via_nlu()`:
Before accepting an NLU classification with $\ge 0.85$ confidence, run a sanity check ensuring that the transcript shares at least one semantic trigger with the intent:
- For `media_*`: transcript must contain at least one of `["play", "pause", "track", "music", "song", "video", "media", "next", "previous", "stop"]`.
- If sanity check fails, reject the classification (`return None`) so it can route to `admin-brain` or the fallback worker instead of misfiring.

### Phase 5: Ensure STT Learning Loop Is Wired (`src-tauri/src/stt_learning.rs` & `wakeword_oww.rs`)
Verify that every completed STT turn calls `stt_learning::log_successful_transcript()` on parse success and `stt_learning::log_failed_transcript()` on parse failure, allowing repeated user self-corrections to automatically promote into `learned_corrections.json`.

---

## 3. Verification & Acceptance Criteria

1. **Test Verification**:
   - `cargo test --lib -- intent_parser`: all tests pass.
   - `cargo test --lib -- stt`: all tests pass.
   - `cargo test --lib -- nlu`: all tests pass.
   - `cargo check --features custom-protocol,admin-brain`: clean (0 warnings, 0 errors).
   - `npm test -- --run`: 200/200 pass.
2. **Behavioral Acceptance**:
   - Indian-accented and fast spoken commands ("ghost mode", "open whatsapp", "memory audit") resolve correctly without being dropped.
   - Spoken phrases like "So you have to list" no longer trigger media play/pause hallucinations.
   - Overall command recognition accuracy reaches 92–95%+ with 0 MB local model RAM impact.
