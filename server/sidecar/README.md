# NEXUS WSS Bridge (sidecar)

Bridges the thin client's persistent WebSocket to:
- **STT** (faster-whisper) for speech-to-text
- **n8n** supervisor webhook for intent routing + canvas execution
- **ElevenLabs** multi-context TTS for voice output (ack + final result)
- **OAuth** token exchange for Google / GitHub / API keys

## Architecture

```
Thin Client ──WSS──▶ sidecar (/ws)
                         │
                         ├──HTTP POST──▶ STT (faster-whisper, GPU)
                         │   transcript ◀────────────┘
                         │
                         ├──WSS──▶ ElevenLabs TTS (multi-context)
                         │   "On it, sir." (ack, ~75ms)     ◀── immediate
                         │   final result (sentence flush)  ◀── after n8n
                         │   PCM chunks back to client
                         │
                         └──HTTP POST (streaming)──▶ n8n /supervisor
                                    result ◀──────────────────┘
                         │
                         ▼
                    {type:tts_chunk} + {type:done} frames to client
```

## OAuth flow

```
Client setup page → "Connect Google"
  → sidecar /oauth/auth-url returns Google OAuth URL (with PKCE challenge)
  → client opens system browser → user logs into Google
  → Google redirects to NEXUS://oauth/callback?code=XXX
  → client catches deep link, sends code + verifier to sidecar /oauth/exchange
  → sidecar exchanges code for tokens (using client secret)
  → tokens stored in SQLite, keyed by user_id
  → when n8n canvas needs Google access, sidecar injects tokens into payload
```

## Run

```bash
pip install -r requirements.txt
uvicorn sidecar:app --host 0.0.0.0 --port 41098
# Production: behind Caddy (TLS) proxying to localhost:41098
```

### Environment variables

| Var | Default | Purpose |
|---|---|---|
| `STT_URL` | `http://localhost:8000/transcribe` | faster-whisper endpoint |
| `N8N_SUPERVISOR_URL` | `http://localhost:5678/webhook/supervisor` | n8n webhook (non-streaming) |
| `N8N_STREAM_URL` | `http://localhost:5678/webhook-stream/supervisor` | n8n webhook (streaming) |
| `N8N_API_TOKEN` | (empty) | n8n auth token (if supervisor uses Auth Check) |
| `ELEVENLABS_API_KEY` | (required) | ElevenLabs API key |
| `ELEVENLABS_VOICE_ID` | (required) | ElevenLabs voice ID |
| `ELEVENLABS_MODEL` | `eleven_flash_v2_5` | ElevenLabs TTS model |
| `GOOGLE_CLIENT_ID` | (empty) | Google OAuth client ID |
| `GOOGLE_CLIENT_SECRET` | (empty) | Google OAuth client secret (server-side only) |
| `GITHUB_CLIENT_ID` | (empty) | GitHub OAuth client ID |
| `GITHUB_CLIENT_SECRET` | (empty) | GitHub OAuth client secret (server-side only) |
| `OAUTH_REDIRECT_URI` | `NEXUS://oauth/callback` | OAuth redirect URI (deep link) |
| `NEXUS_SIDECAR_TOKEN` | (empty = no gate) | optional bearer gate on /ws |
| `NEXUS_DB_PATH` | `NEXUS_credentials.db` | SQLite database path |
| `NEXUS_ENCRYPTION_KEY` | (empty = ephemeral) | Fernet key for encrypting API keys at rest |
| `SIDECAR_HOST` / `SIDECAR_PORT` | `0.0.0.0` / `41098` | bind address |

## Protocol (client ↔ sidecar)

Client → sidecar:
- `{type:"start", sessionId, userId, deviceId}` — open a session
- binary frames — raw 16k mono PCM audio
- `{type:"end_audio"}` — VAD silence; flush to STT + n8n
- `{type:"cancel"}` — abort the current turn (barge-in)

Sidecar → client:
- `{type:"state", state:"listening"|"thinking"|"speaking"|"idle"}`
- `{type:"transcript", data:"..."}` — the transcribed user speech
- `{type:"ack", data:"On it, sir."}` — acknowledgement text (for transcript display)
- `{type:"result", data:"..."}` — final result text from n8n (for transcript display)
- `{type:"tts_chunk", data:"<base64 PCM>"}` — 16-bit 16kHz mono PCM audio
- `{type:"done"}` — turn complete
- `{type:"error", message:"..."}`

## HTTP endpoints

| Method | Path | Purpose |
|---|---|---|
| GET | `/health` | Health check (includes TTS config status) |
| GET | `/config/check` | Report which OAuth providers are configured (no secrets) |
| GET | `/oauth/auth-url?provider=google&user_id=X&code_challenge=Y` | Get OAuth authorization URL |
| POST | `/oauth/exchange` | Exchange auth code for tokens |
| POST | `/oauth/refresh` | Refresh an expired token |
| GET | `/oauth/status?user_id=X` | Check connected providers |
| DELETE | `/oauth/disconnect` | Remove a provider's tokens |
| POST | `/device/register` | Register a new device for a user |
| GET | `/device/validate?user_id=X&device_id=Y` | Validate a device registration |
| POST | `/apikeys/add` | Store an API key (Claude, Devin, etc.) |
| DELETE | `/apikeys/remove` | Remove an API key |
| GET | `/apikeys/list?user_id=X` | List stored API key providers |

## Files

| File | Purpose |
|---|---|
| `sidecar.py` | Main FastAPI app, WebSocket handler, session management |
| `tts.py` | ElevenLabs multi-context TTS WebSocket client |
| `oauth.py` | OAuth exchange + token management + API key storage |
| `db.py` | SQLite database for tokens, API keys, device registration |
| `n8n_client.py` | n8n supervisor webhook client (streaming + non-streaming) |

## Multi-process scaling

The session map is in-memory. For >1 sidecar process, replace `SESSIONS` with
Redis pub/sub keyed by `sessionId`. A single process handles ~hundreds of
concurrent sessions comfortably for the 5-user target.
