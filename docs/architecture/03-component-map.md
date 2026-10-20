# NEXUS — Component Map

> Every source file, what it does, and what it talks to.
> Use this as the index when you need to find "where does X happen?"

---

## Rust Main Process (`src-tauri/src/`)

| File | Purpose | Key Functions / Exports | Talks To |
|------|---------|-------------------------|----------|
| `main.rs` | Binary entry point. Calls `lib::run()`. | `fn main()` | `lib.rs` |
| `lib.rs` | Tauri builder. Wires all plugins, modules, and the invoke handler. The "wiring diagram" of the app. | `pub fn run()`, `AppState` | All modules below; Tauri plugin registry |
| `window_manager.rs` | Overlay window: transparent, frameless, always-on-top, click-through. | `init()`, `set_click_through`, `show_overlay`, `hide_overlay` | Tauri window API, frontend `pointermove` |
| `hotkey.rs` | Global hotkey `Ctrl/Cmd+Shift+Space` → wake. | `init()` | `tauri-plugin-global-shortcut`, frontend `win.eval()` |
| `wakeword_oww.rs` | openWakeWord KWS engine + Tier 3 command classifiers. Pure Rust ONNX inference via `tract-onnx`. | `run()`, `set_meeting_state()` | `cpal` (audio), `tract-onnx` (models), `MeetingState`, frontend events |
| `wakeword.rs` | Legacy VAD+ASR fallback (sherpa-onnx). Only compiled when `wakeword-oww` feature is off. | `run()` | `sherpa-onnx`, `cpal` |
| `network.rs` | WSS bridge to the sidecar. Owns the single WebSocket session. | `open_session`, `send_transcript`, `cancel_session`, `close_session` | `tokio-tungstenite`, frontend `assistant:server` events |
| `sidecar_manager.rs` | Auto-spawns the Python FastAPI sidecar on startup. Finds Python, resolves the sidecar dir, waits for health. | `init()`, `shutdown()` | `pythonw.exe` / `python3`, TCP health probe |
| `meeting_detect.rs` | Meeting/privacy mode: WASAPI session detection (Windows) + process scan (cross-platform). Shared atomic state. | `MeetingState`, `run_detection_loop()`, `should_suppress_wake()` | Windows WASAPI COM, `sysinfo`, `MeetingState` atomics |
| `mic_permissions.rs` | WebView2 permission handler. Auto-approves mic/camera for NEXUS-owned origins only. | `init()` | `webview2_com`, WebView2 `PermissionRequested` event |
| `commands.rs` | IPC commands: setup window, server config, voice profile, meeting status, boot greeting. | `open_setup_window`, `save_server_config`, `get_server_config`, `frontend_ready`, `meeting_active`, `is_nexus_paused`, `meeting_status`, `set_meeting_detection`, `enroll_voice`, `delete_voice_profile`, `get_voice_profile_status` | `voice_profile`, `meeting_detect`, `sysinfo` |
| `command_executor.rs` | Local command execution: open app, open URL, search, Spotify, YouTube, GitHub, volume, screenshot, lock, browser keys. | `execute_command` (IPC), `Intent` enum | `app_registry`, `open::that`, OS-specific commands |
| `app_registry.rs` | Pre-indexed app launcher. Disk cache + in-memory HashMap + fuzzy match. Background refresh every 5 min. | `init()`, `lookup()`, `focus_existing()`, `launch()`, `record_usage()` | Windows Get-StartApps, macOS `/Applications`, Linux `.desktop` files (`app_registry_linux.rs`) |
| `stt.rs` | Local STT HTTP client. Sends PCM to `127.0.0.1:8000` and returns transcript text. | `transcribe_audio` (IPC), `stt_status` (IPC) | `reqwest` → local faster-whisper server |
| `voice_profile.rs` | Speaker verification: sherpa-onnx speaker embeddings, enrollment, threshold comparison. | `SpeakerVerifier`, `VoiceProfile`, `VoiceProfileStatus`, `SOUND_ALIKES`, `DEFAULT_THRESHOLD` | `sherpa-onnx`, disk (profile JSON) |
| `tray.rs` | System tray: show, pause/resume, settings, quit. | `setup()` | Tauri tray API, `MeetingState` |

---

## Frontend (`frontend/src/`)

| File | Purpose | Key Exports | Talks To |
|------|---------|-------------|----------|
| `main.tsx` | Entry point. Wires wake handler, Tier 3 listener, boot greeting, cancel handler, mic stream management. | `window.__NEXUS_WAKE__`, `window.__NEXUS_CANCEL__`, `window.__NEXUS_RELEASE_MIC__`, `window.__NEXUS_GET_MIC_STREAM__`, `greet()` | All audio modules, `store/assistant`, Tauri IPC |
| `App.tsx` | Root React component. Avatar + visibility + auto-hide + click-through toggle. | `App` (default) | `store/assistant`, `avatar/Avatar`, Tauri IPC |
| `store/assistant.ts` | Zustand store: state machine + transcript + visibility. | `useAssistant`, `transition()` | — (pure state) |
| `audio/recorder.ts` | Mic capture via `ScriptProcessorNode`. Buffers Float32, downsamples to 16 kHz, sends to local STT, parses intent, routes to backend. | `startRecording()`, `captureUntilSilence()`, `finishCapture()`, `abortCapture()`, `getRecordingContext()` | `audio/stt`, `audio/ttsPlayer`, `net/wsBridge`, `intent/parser` |
| `audio/vad.ts` | Silero VAD (ONNX Runtime Web) + RMS fallback. Detects speech/silence, calls `finishCaptureFromVad()`. | `startVad()`, `stopVad()`, `preloadSileroVad()` | `@ricky0123/vad-web`, `audio/recorder` |
| `audio/stt.ts` | Frontend wrapper for the `transcribe_audio` IPC command. | `transcribeAudio()` | Tauri IPC → `stt.rs` |
| `audio/ttsPlayer.ts` | Local TTS via Web Speech API. Meeting-aware suppression. Emits `tts-started`/`tts-ended` events. | `speak()`, `stopTts()`, `ttsAvailable()` | `speechSynthesis`, Tauri IPC (`meeting_active`), Tauri events |
| `audio/paramCapture.ts` | 3-second parameter capture for Tier 3 parameterized commands. | `captureParameter()` | `window.__NEXUS_GET_MIC_STREAM__`, `audio/stt` |
| `net/wsBridge.ts` | WebSocket bridge facade. Open/close/send + server event handler. Retry with exponential backoff. | `openSession()`, `sendTranscript()`, `cancelSession()`, `closeSession()`, `hasSession()` | Tauri IPC → `network.rs`, `store/assistant`, `audio/ttsPlayer` |
| `intent/parser.ts` | Local intent parser: regex + Double Metaphone phonetic correction. | `parseIntent()`, `Intent` type | — (pure logic) |
| `overlay/clickThrough.ts` | Region-aware click-through via `document.elementFromPoint`. | — | Tauri IPC → `window_manager::set_click_through` |
| `avatar/Avatar.tsx` | Rive state machine avatar (or Lottie fallback). | `Avatar` | `store/assistant` |
| `setup/main.tsx` | Setup window entry point. | — | `setup/SetupApp` |
| `setup/SetupApp.tsx` | Setup UI: server URL, Google/GitHub OAuth, API keys, voice enrollment. | `SetupApp` | `setup/oauth`, `setup/VoiceEnrollment`, Tauri IPC |
| `setup/oauth.ts` | OAuth2 PKCE client. Generates verifier/challenge, opens browser, handles deep-link redirect, exchanges code. | `connectOAuth()`, `disconnectOAuth()`, `getOAuthStatus()`, `addApiKey()`, `removeApiKey()`, `listApiKeys()`, `setSidecarBaseUrl()` | Sidecar HTTP endpoints, Tauri deep-link events, `@tauri-apps/plugin-shell` |
| `setup/VoiceEnrollment.tsx` | Voice enrollment UI: records 5 clips, sends to Rust for embedding extraction. | `VoiceEnrollment` | Tauri IPC → `commands::enroll_voice` |

---

## Python Sidecar (`server/sidecar/`)

| File | Purpose | Key Exports | Talks To |
|------|---------|-------------|----------|
| `sidecar.py` | FastAPI app. WebSocket endpoint `/ws` (text-only), health `/health`. Session registry. Ack + n8n call + result flow. | `app`, `Session`, `ws_endpoint()`, `_process_transcript()` | `db`, `n8n_client`, `oauth` |
| `oauth.py` | OAuth2 token exchange/refresh + API key CRUD + device registration + config check. APIRouter. | `router`, `get_valid_credentials()`, `_exchange_google()`, `_exchange_github()`, `_refresh_google()` | `db`, `httpx` (Google/GitHub token endpoints) |
| `db.py` | SQLite database for OAuth tokens, API keys (Fernet-encrypted), device registration. | `init_db()`, `store_oauth_token()`, `get_oauth_token()`, `store_api_key()`, `get_api_key()`, `register_device()`, `validate_device()` | SQLite, `cryptography.fernet` |
| `n8n_client.py` | n8n supervisor HTTP client. Streaming SSE or JSON. Accumulates token deltas. | `call_supervisor()` | `httpx` → n8n webhook |
| `tts.py` | (Placeholder for server-side TTS — currently unused, TTS is client-side) | — | — |
| `__init__.py` | Package marker. Makes `sidecar` importable as a package. | — | — |

---

## Configuration & Build

| File | Purpose |
|------|---------|
| `src-tauri/tauri.conf.json` | Tauri config: windows (main + setup), transparent overlay, plugins, bundle, CSP, deep-link scheme |
| `src-tauri/Cargo.toml` | Rust dependencies + feature flags (`wakeword-oww`, `mock-wake`, etc.) |
| `src-tauri/capabilities/main.json` | Least-privilege capability scopes for the main window |
| `frontend/package.json` | Frontend dependencies (React, Vite, zustand, vad-web, etc.) |
| `frontend/vite.config.ts` | Vite build config + `viteStaticCopy` for VAD model files |
| `frontend/.env.local` | Build-time fallbacks: `VITE_SERVER_URL`, `VITE_DEVICE_TOKEN` |
| `server/sidecar/.env` | Sidecar secrets: `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET`, `NEXUS_ENCRYPTION_KEY`, `N8N_SUPERVISOR_URL`, etc. |
| `command_intents.json` | 39 command definitions for Tier 3 classifiers (phrase → model_file → intent) |

---

## ONNX Models (`src-tauri/resources/oww/`)

| File | Size | Role |
|------|------|------|
| `nexus.onnx` | ~790 KB | Custom-trained wake word classifier ("NEXUS") |
| `melspectrogram.onnx` | ~1.1 MB | Pre-trained mel spectrogram extractor (shared) |
| `embedding_model.onnx` | ~1.3 MB | Pre-trained embedding extractor (shared) |
| `commands/*.onnx` | ~800 KB each | Tier 3 command classifiers (one per command phrase) |
| `commands/command_intents.json` | — | Intent mapping (phrase → action + target) |

---

## Training Notebooks

| File | Purpose |
|------|---------|
| `train_nexus_oww.ipynb` | Trains the `nexus.onnx` wake word classifier (Colab, T4 GPU, ~1 hour) |
| `train_nexus_commands.ipynb` | Trains Tier 3 command classifiers (Colab, checkpoints to Google Drive) |
