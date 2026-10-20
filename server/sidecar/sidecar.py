"""
NEXUS WSS Bridge — FastAPI WebSocket sidecar (TEXT-ONLY protocol).

Bridges the thin client's persistent WebSocket to the Cloudflare Worker
for intent routing + API calls.

TEXT-ONLY: The client performs STT and TTS locally. Audio never crosses
the network. The client sends only transcribed text, and the server
returns only text responses.

Flow:
  1. Client connects to /ws.
  2. Client sends {type:"start", sessionId, userId, deviceId}.
  3. Client sends {type:"transcript", data:"check the 76 PR"}.
  4. This sidecar:
       a. Sends {type:"ack", data:"On it, sir."} immediately.
       b. Calls the Cloudflare Worker with transcript + user credentials.
       c. Sends {type:"result", data:"PR #76 is approved..."}.
       d. Sends {type:"done"}.
  5. Client may send {type:"cancel"} at any time for barge-in.

Binary frames are REJECTED — no audio is accepted on this endpoint.

OAuth endpoints (/oauth/*, /apikeys/*) let the client exchange authorization
codes for tokens, which are stored per-user and injected into Worker calls.

Run:
  uvicorn sidecar:app --host 0.0.0.0 --port 41098
  # Production: behind Caddy (TLS) proxying to localhost:41098
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import random
import uuid
from contextlib import suppress
from dataclasses import dataclass
from typing import Dict, Optional

from fastapi import FastAPI, Request, WebSocket, WebSocketDisconnect, status
from fastapi.responses import JSONResponse

from . import db
from .n8n_client import call_supervisor
from .oauth import router as oauth_router, get_valid_credentials

log = logging.getLogger("NEXUS.sidecar")
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")

# ---- Configuration (env-driven) ----
SIDECAR_TOKEN = os.getenv("NEXUS_SIDECAR_TOKEN", "")
MAX_TEXT_FRAME_BYTES = 64 * 1024  # 64 KB safety cap on text frames

# Acknowledgement phrases. The sidecar picks one randomly to avoid repetition.
# These are sent as text to the client, which speaks them locally via TTS.
_ACK_PHRASES = [
    "On it, sir.",
    "Right away, sir.",
    "Checking that now, sir.",
    "Working on it, sir.",
    "Let me look into that, sir.",
]

app = FastAPI(title="NEXUS WSS Bridge", version="0.3.0")
app.include_router(oauth_router)


@app.on_event("startup")
async def _startup() -> None:
    db.init_db()
    log.info("sidecar started (text-only protocol)")


# ---- Session registry ----
@dataclass
class Session:
    ws: WebSocket
    session_id: str
    user_id: str
    device_id: str
    cancelled: bool = False
    n8n_task: Optional[asyncio.Task] = None


SESSIONS: Dict[str, Session] = {}


# ---- Health ----
@app.get("/health")
async def health() -> JSONResponse:
    return JSONResponse({
        "ok": True,
        "sessions": len(SESSIONS),
        "protocol": "text-only",
    })


# ---- User registration (called by the installer.exe) ----
#
# When a user installs NEXUS and runs the setup wizard, the Tauri app
# calls this endpoint to register the new user + device with the server.
# The server returns the configuration the client needs to connect.
#
# This is the "handshake" between the installer and the main server.

@app.post("/api/register")
async def register_user(request: Request) -> JSONResponse:
    """
    Register a new user + device with the NEXUS server.

    Called by the NEXUS installer / setup wizard on first launch.

    Body:
    {
      "user_id": "user_a1b2c3d4...",     # auto-generated UUID on the client
      "device_id": "device_f6e5d4c3...",  # auto-generated UUID on the client
      "device_name": "Lakshya-ThinkPad",  # human-readable, optional
      "os": "windows" | "macos" | "linux"
    }

    Response (200):
    {
      "ok": true,
      "user_id": "user_a1b2c3d4...",
      "device_id": "device_f6e5d4c3...",
      "server_config": {
        "ws_url": "ws://100.71.60.31:41098/ws",
        "sidecar_url": "http://100.71.60.31:41098",
        "n8n_url": "http://100.71.60.31:5678"
      },
      "providers": {
        "google": {"configured": true, "scopes": "..."},
        "github": {"configured": true, "scopes": "..."}
      }
    }

    The client saves server_config to nexus-config.json and uses it
    for all subsequent WebSocket + OAuth connections.
    """
    try:
        body = await request.json()
    except Exception:
        return JSONResponse({"error": "invalid JSON"}, status_code=400)

    user_id = body.get("user_id", "")
    device_id = body.get("device_id", "")
    device_name = body.get("device_name", "")
    os_name = body.get("os", "")

    if not user_id or not device_id:
        return JSONResponse(
            {"error": "user_id and device_id are required"},
            status_code=400,
        )

    # Register the device in the database
    db.register_device(user_id, device_id)

    # Build the server config to return to the client
    # The WebSocket URL is how the NEXUS laptop connects to this sidecar
    # The sidecar URL is how the setup wizard reaches OAuth endpoints
    host = os.getenv("NEXUS_PUBLIC_HOST", "")
    if not host:
        # Fall back to the request's host header
        host = request.headers.get("host", "localhost:41098")
        # Strip the port for the hostname, we'll add our own
        host = host.split(":")[0]

    port = os.getenv("SIDECAR_PORT", "41098")
    ws_url = f"ws://{host}:{port}/ws"
    sidecar_url = f"http://{host}:{port}"

    # Check which OAuth providers are configured on this server
    from .oauth import GOOGLE_CLIENT_ID, GITHUB_CLIENT_ID, GOOGLE_SCOPES, GITHUB_SCOPES
    providers = {
        "google": {
            "configured": bool(GOOGLE_CLIENT_ID),
            "scopes": GOOGLE_SCOPES if GOOGLE_CLIENT_ID else "",
        },
        "github": {
            "configured": bool(GITHUB_CLIENT_ID),
            "scopes": GITHUB_SCOPES if GITHUB_CLIENT_ID else "",
        },
    }

    log.info(
        "user registered: user=%s device=%s name=%s os=%s",
        user_id, device_id, device_name, os_name,
    )

    return JSONResponse({
        "ok": True,
        "user_id": user_id,
        "device_id": device_id,
        "server_config": {
            "ws_url": ws_url,
            "sidecar_url": sidecar_url,
            "n8n_url": f"http://{host}:5678",
        },
        "providers": providers,
    })


# ---- WebSocket endpoint ----
@app.websocket("/ws")
async def ws_endpoint(ws: WebSocket) -> None:
    if SIDECAR_TOKEN:
        auth = ws.headers.get("authorization", "")
        if auth != f"Bearer {SIDECAR_TOKEN}":
            await ws.close(code=status.WS_1008_POLICY_VIOLATION, reason="unauthorized")
            return

    await ws.accept(subprotocol="NEXUS.v1")
    session: Optional[Session] = None
    try:
        while True:
            msg = await ws.receive()
            mtype = msg.get("type")
            if mtype == "websocket.disconnect":
                break
            if mtype != "websocket.receive":
                continue

            text = msg.get("text")
            if text is not None:
                # Reject oversized text frames.
                if len(text) > MAX_TEXT_FRAME_BYTES:
                    log.warning(
                        "session %s sent oversized text frame (%d bytes), rejecting",
                        session.session_id if session else "?",
                        len(text),
                    )
                    await _send_error(ws, "text frame too large")
                    continue
                session = await _handle_text(ws, text, session)
                continue

            # BINARY FRAMES ARE REJECTED — no audio is accepted.
            data = msg.get("bytes")
            if data is not None:
                log.warning(
                    "session %s sent binary frame (%d bytes) — REJECTED (text-only protocol)",
                    session.session_id if session else "?",
                    len(data),
                )
                await _send_error(ws, "binary frames not supported — text only")
                # Do NOT buffer or process the binary data.
                continue
    except WebSocketDisconnect:
        log.info("ws disconnected")
    except Exception:
        log.exception("ws loop error")
    finally:
        if session is not None:
            if session.n8n_task is not None and not session.n8n_task.done():
                session.n8n_task.cancel()
            SESSIONS.pop(session.session_id, None)


async def _handle_text(ws: WebSocket, raw: str, session: Optional[Session]) -> Optional[Session]:
    """Handle a JSON control frame from the client."""
    try:
        frame = json.loads(raw)
    except json.JSONDecodeError:
        log.warning("bad json frame: %r", raw[:200])
        return session

    ftype = frame.get("type")

    if ftype == "start":
        sid = frame.get("sessionId") or str(uuid.uuid4())
        sess = Session(
            ws=ws,
            session_id=sid,
            user_id=str(frame.get("userId", "")),
            device_id=str(frame.get("deviceId", "")),
        )
        SESSIONS[sid] = sess
        await _send_state(ws, "listening")
        return sess

    if ftype == "transcript":
        # Client has done STT locally and is sending the transcript TEXT.
        # No audio is involved — this is the only request payload.
        if session is not None and not session.cancelled:
            transcript_text = str(frame.get("data", "")).strip()
            if transcript_text:
                await _process_transcript(session, transcript_text)
            else:
                await _send_error(ws, "empty transcript")
        return session

    if ftype == "cancel":
        if session is not None:
            session.cancelled = True
            # Cancel any ongoing n8n task.
            if session.n8n_task is not None and not session.n8n_task.done():
                session.n8n_task.cancel()
            await _send_state(ws, "idle")
        return session

    log.debug("unknown control frame: %s", ftype)
    return session


async def _process_transcript(sess: Session, transcript: str) -> None:
    """
    Process a transcript: send ack, call n8n, send result, send done.

    The ack is sent immediately so the client can speak it locally while
    n8n processes the request. No TTS is done server-side — the client
    speaks all text locally via the Web Speech API.
    """
    if sess.cancelled:
        return

    await _send_state(sess.ws, "thinking")

    # 1. Get the user's credentials (OAuth tokens + API keys), refreshing if needed.
    try:
        credentials = await get_valid_credentials(sess.user_id)
    except Exception:
        log.exception("failed to load credentials for user %s", sess.user_id)
        credentials = {}

    # 2. Send acknowledgement text immediately.
    #    The client speaks this locally via Web Speech API.
    ack_text = random.choice(_ACK_PHRASES)
    if not sess.cancelled:
        with suppress(Exception):
            await sess.ws.send_text(json.dumps({"type": "ack", "data": ack_text}))

    # 3. Call the Cloudflare Worker with the transcript text.
    n8n_task = asyncio.create_task(
        call_supervisor(
            session_id=sess.session_id,
            user_id=sess.user_id,
            device_id=sess.device_id,
            transcript=transcript,
            credentials=credentials,
        )
    )
    sess.n8n_task = n8n_task

    # 4. Wait for the Worker result.
    if sess.cancelled:
        n8n_task.cancel()
        return

    try:
        result_text = await n8n_task
    except asyncio.CancelledError:
        return
    except Exception:
        log.exception("n8n task failed")
        result_text = "Sorry, I couldn't complete that request."
    finally:
        sess.n8n_task = None

    if sess.cancelled:
        return

    # 5. Send the result text to the client.
    #    The client speaks this locally via Web Speech API.
    if result_text:
        with suppress(Exception):
            await sess.ws.send_text(json.dumps({"type": "result", "data": result_text}))

    await _send_done(sess.ws)


async def _send_state(ws: WebSocket, state: str) -> None:
    with suppress(Exception):
        await ws.send_text(json.dumps({"type": "state", "state": state}))


async def _send_done(ws: WebSocket) -> None:
    with suppress(Exception):
        await ws.send_text(json.dumps({"type": "done"}))


async def _send_error(ws: WebSocket, message: str) -> None:
    with suppress(Exception):
        await ws.send_text(json.dumps({"type": "error", "message": message}))


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(
        "sidecar:app",
        host=os.getenv("SIDECAR_HOST", "0.0.0.0"),
        port=int(os.getenv("SIDECAR_PORT", "41098")),
        log_level="info",
    )
