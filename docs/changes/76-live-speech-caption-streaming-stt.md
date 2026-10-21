# Live-Speech Caption — Streaming STT (Phase 4, Claude session, 2026-10-04)

**Plan:** `C:\Users\Chitkul Lakshya\.claude\plans\c-users-chitkul-lakshya-downloads-phone-idempotent-snail.md` (Wakeup Orb Redesign; 5 phases). Phases 1-3 ([doc 73](73-window-consolidation-orb-into-stage.md), [74](74-orb-shape-color-and-glitch-redesign.md), [75](75-response-caption-word-by-word-above-orb.md)) landed first. This is **Phase 4 — the user's own words, growing above the orb while they're still talking**, via a genuinely new capability: a parallel streaming-STT path.

**Hard constraint honored**: the existing batch STT pipeline (`recorder.ts` → `stt.ts` → Rust `transcribe_audio` → Groq/Moonshine → intent parsing/NLU/brain) is untouched. This phase is purely additive and degrades silently to "no live caption, batch transcript arrives normally" on any failure — verified live, not just assumed (see §4).

## 1. Python — `server/stt_server.py`'s new `/stream` WebSocket endpoint

Verified the actual installed `moonshine_voice` package source (not just the plan's description) before writing anything: `Transcriber.create_stream(update_interval=...)` → `Stream`, fed via `stream.add_audio(float_samples, sample_rate=16000)`, which internally calls `update_transcription()` once enough audio has accumulated and fires listener callbacks (`LineStarted`/`LineTextChanged`/`LineCompleted`/...) synchronously within that call. `stream.add_listener(callable)` accepts a plain function — no `TranscriptEventListener` subclass needed.

- New `@app.websocket("/stream")` endpoint: accepts the connection, creates a **fresh `Stream`** off the already-loaded global `_transcriber` (never loads a second model), registers a listener that collects `LineTextChanged` text into a list, then on each incoming **binary** frame (raw 16-bit LE mono PCM @ 16kHz — same convention as `/transcribe`'s raw-PCM branch) calls `stream.add_audio(...)` and flushes any newly-collected text to the client as `{"text": "<growing line>"}`. A text frame `{"cmd":"stop"}` or a client disconnect ends the loop.
- **Bug found and fixed while testing**: the `finally` block called `stream_handle.stop()` (which transcribes whatever audio is left and fires the final events) but never sent the resulting `pending_texts` to the client before closing — the last, usually most-complete line was silently dropped. Fixed by flushing `pending_texts` after `stop()`, before `websocket.close()`.
- `nexus.mjs`'s `checkMoonshine()`: added a **separate** check/install for the `websockets` package. Neither `fastapi` nor bare `uvicorn` depends on it, but uvicorn needs it (or `wsproto`) to serve WebSocket routes at all — and the original install line only ran once, on a machine with no `moonshine_voice` yet, so an *existing* install (this user's own machine, and presumably most return users) would never have picked it up. Checked separately so `nexus setup` backfills it even when `moonshine_voice` is already present.

## 2. Rust — `stt_stream.rs` (new module)

- `tokio-tungstenite` + `futures-util` added as **direct** dependencies — both were already locked transitive dependencies of `edge-tts-rust` (its own WSS client to Microsoft's endpoint), confirmed via `Cargo.lock` before adding, so this is zero new download/compile surface despite being "a new third-party surface" in API terms.
- Three commands, matching the plan's naming: `stt_stream_start` (connects to `ws://127.0.0.1:39217/stream` — the same hardcoded host/port `stt.rs::STT_URL` already uses for the batch path — splits the socket, stores the write half in a `static Lazy<Mutex<Option<SplitSink<...>>>>`, spawns a background task that re-emits every `{"text":...}` message as a `stt:partial` Tauri event), `stt_stream_push_chunk` (sends one chunk of raw PCM as a binary WS frame; silently no-ops if nothing is connected), `stt_stream_stop` (sends `{"cmd":"stop"}`, closes, clears state).
- Every failure path is swallowed exactly as the plan required: a connect failure in `stt_stream_start` is logged at `debug` level and leaves the sink `None` — the command itself **always returns `Ok(())`**, so a dead server can never surface as a frontend-visible error. A send failure in `push_chunk` just clears the sink (stop retrying this turn).

## 3. Frontend

- `recorder.ts`: `startRecording()` fires `stt_stream_start` (fire-and-forget); `stopRecording()` fires `stt_stream_stop` (fire-and-forget) — since every recording-ending path (`finishCapture`, both `abortCapture` branches) already funnels through `stopRecording()`, one edit covers all of them. The existing `onaudioprocess` handler gets a **second**, non-blocking push: `downsampleAndConvert(new Float32Array(input), nativeSampleRate, 16000)` → `invoke("stt_stream_push_chunk", ...)`, wrapped in `.catch(() => {})`. The original `floatBuffer.push(...)` line driving the batch path is untouched.
- `frontend/src/stage/LiveCaption.tsx` (new): listens to `stt:partial` (wholesale-replaces its text each time — Moonshine's `LineTextChanged` gives the full growing line, not a delta) and clears on `stt:transcript` (turn end — the real batch result arrived). Same positioning formula as `ResponseCaption` (self-contained `stage:orb_rect` listener, fixed gap above the orb) — the two share a CSS class (`.response-caption`) since they're temporally mutually exclusive (one only has text while listening, the other only while speaking) and never compete for the same slot. A `.live-caption` modifier dims it slightly (opacity 0.82) so it still reads as distinct from the assistant's own reply.
- Mounted in `stage/main.tsx` alongside `ResponseCaption`.

## 4. Verification (live, not just code review)

- **Ran the real Python server** and drove its `/stream` endpoint with an actual WebSocket test client (scratchpad, not committed) pushing **real synthesized speech** (`edge-tts` generated a 9s multi-sentence clip — the moonshine package's own bundled `success.wav` turned out to be a ~1s UI chime, not speech, so an earlier attempt with it correctly produced zero partials). Result — the endpoint genuinely works:
  ```
  PARTIAL: 'Good morning.'
  PARTIAL: 'Good morning, sir.'
  PARTIAL: 'You have three meetings today and there'
  PARTIAL: 'You have three meetings today, and the weather looks clear with a high of 72.'
  ```
  (This also caught and confirmed the fix for the dropped-final-line bug in §1 — before the fix, the last/most-complete line never arrived.)
- **Killed the server entirely** (confirmed via `Get-CimInstance Win32_Process` + `Stop-Process`, not just a port probe) and reran the same client: connection raises `ConnectionRefusedError` immediately (well under a few seconds, no hang) — the same error class `tokio_tungstenite::connect_async` surfaces to `stt_stream_start`, which the Rust code already explicitly catches (`Err(e) => { ...; *SINK.lock().await = None; }`) without ever returning an `Err` from the command itself. This is the plan's explicit "kill the stream and confirm zero impact" check, done against the real failure mode rather than assumed.
- `cargo check --lib` clean; `cargo test --lib` **870/870** (no new Rust unit tests — this module is thin I/O glue with no pure logic to test, consistent with other command-wrapper modules in this codebase). `npx tsc --noEmit` clean, `npm run build` clean, `npx vitest run` **154/154**.
- **Not done**: a full live run through the actual Tauri app (real mic, real wake word, real orb) — deliberately not attempted, since that means launching the background assistant with global hotkeys and live mic capture on the user's real machine, which earlier phases' verification notes already flagged as something to leave for the user rather than trigger autonomously.

## 5. Not done / next

Phase 5 (cleanup — the dead `.transcript, .caption { display: none; }` CSS rule, a final `styles.css` split, an AGENTS.md entry marking the Single-Stage Shell migration complete) is the last phase of the plan.
