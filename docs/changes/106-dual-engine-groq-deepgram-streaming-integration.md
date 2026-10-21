# 106 — Dual-Engine Hybrid STT: Groq LPU Fast-Path & Optional Deepgram Nova-2 Streaming

**Date**: 2026-10-07  
**Status**: Verified & Integrated  
**Target Systems**: `src-tauri/src/stt_deepgram.rs`, `src-tauri/src/stt_stream.rs`, `src-tauri/src/stt.rs`, `src-tauri/src/api_keys.rs`, `src-tauri/src/commands.rs`, `frontend/src/stage/LiveCaption.tsx`  

---

## 1. Problem Statement & Motivation

Following the completion of **Phase 1** (150ms-240ms adaptive VAD endpointing + HTTP/2 connection pooling) and **Phase 2** (Rust intent pre-routing on capture thread + zero-IPC bypass):
1. **Default Zero-Cost Baseline**: Groq's Whisper Large v3 Turbo provides ultra-fast transcription (~50ms inference on LPUs) at $0 cost and zero local model RAM, but Groq's public API does not support token-by-token WebSocket audio streaming.
2. **Real-Time Word Hypotheses Requirement**: To drive [`LiveCaption.tsx`](file:///c:/PROJECTS/ULTRON/frontend/src/stage/LiveCaption.tsx) with token-level partial transcripts while a user is still speaking mid-sentence, a cloud WebSocket provider was required that does not increase local client RAM (unlike local models like Whisper.cpp which add 200MB–1GB RAM).
3. **Seamless Dual-Engine Operation**: Users should have the best of both worlds:
   - **Primary ($0 / Zero RAM)**: Groq Whisper Turbo LPU engine handles all standard and Ghost Mode commands without any paid API keys.
   - **Optional Enhancement**: When a Deepgram API key is present in Command Hub, NEXUS streams 20ms PCM audio frames directly to Deepgram Nova-2 WebSockets, emitting live partial words to `stt:partial`. If absent or failed, it seamlessly falls back to the Groq LPU pipeline.

---

## 2. Implementation Details

### A. Deepgram Nova-2 Engine Integration (`src-tauri/src/stt_deepgram.rs`)
1. Implemented `transcribe_with_deepgram(samples, api_key, client)`:
   - Formats 16kHz PCM16 audio into WAV in memory.
   - Submits to `https://api.deepgram.com/v1/listen?model=nova-2&smart_format=true&punctuate=true`.
   - Returns sanitized transcription text with zero local neural RAM consumption.
2. Added unit tests for response deserialization and empty channel payloads.

### B. Cloud WebSocket Streaming Bridge (`src-tauri/src/stt_stream.rs`)
1. In `stt_stream_start`:
   - Checks `crate::commands::read_api_key(&app, "deepgram")`.
   - When configured, connects via TLS WebSockets to:
     `wss://api.deepgram.com/v1/listen?model=nova-2&smart_format=true&encoding=linear16&sample_rate=16000&channels=1&interim_results=true`.
   - Dispatches interim transcripts to `app.emit("stt:partial", { text })`, driving [`LiveCaption.tsx`](file:///c:/PROJECTS/ULTRON/frontend/src/stage/LiveCaption.tsx) with a 5-word sliding FIFO window in real time.
2. In `stt_stream_stop`:
   - Sends empty binary frame (`Message::Binary(vec![].into())`) to signal clean EOF to Deepgram without dropping sockets.
   - Falls back gracefully to local stream or silent no-op when no key is present.

### C. Automatic Cascade Routing (`src-tauri/src/stt.rs`)
1. In `transcribe_samples`:
   - If `deepgram_key` is present, routes to `stt_deepgram::transcribe_with_deepgram` (marking STT path 8).
   - If Deepgram request fails or no key exists, routes immediately to `stt_groq::transcribe_with_groq` (path 1).
   - If Groq fails, falls back to local faster-whisper (path 2/3).
2. Made `pcm_to_wav` `pub(crate)` for zero-copy reuse across providers.

### D. Keychain & Command Hub API Keys Wiring (`src-tauri/src/api_keys.rs` & `src-tauri/src/commands.rs`)
1. Registered `("deepgram", "Deepgram", "deepgramApiKey")` in `KEY_SERVICES`. Automatically exposes Deepgram in Command Hub's API Keys settings.
2. Updated `apply_key_field` to map `"deepgram"` to `"deepgram_api_key"`.
3. Added `deepgram_api_key` to `NexusSettings`, `default()`, `save_settings`, `read_api_key`, `export_settings`, and `import_settings`.

---

## 3. Verification & Performance

- **Rust Backend**:
  - `cargo check --features custom-protocol,admin-brain`: clean (**0 warnings, 0 errors**).
  - `cargo test --lib -- stt_deepgram`: 2/2 passed.
  - `cargo test --lib -- api_keys`: 7/7 passed.
  - `cargo test --lib -- stt`: 31/31 passed.
  - `cargo test --lib -- wakeword`: 58/58 passed.
  - `cargo test --lib -- intent_parser`: 197/197 passed.
- **Frontend**:
  - `npx tsc --noEmit`: clean (0 errors).
  - `npm test -- --run`: 200/200 passed.
  - `npm run build`: built in 8.17s.
- **Release Executable**:
  - Fresh binary produced at `src-tauri/target/release/nexus.exe` (96.1 MB).
