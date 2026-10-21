"""
ElevenLabs multi-context TTS client.

Uses the multi-context WebSocket API to manage independent speech contexts
over a single persistent connection. This enables:
  - Immediate acknowledgement ("On it, sir.") while n8n processes the request.
  - Final result speech after n8n returns.
  - Barge-in: close the current context to stop speech instantly.

Protocol: wss://api.evenlabs.io/v1/text-to-speech/{voice_id}/multi-stream-input
  - Send initialization message with xi-api-key + voice_settings.
  - Create contexts by sending text with a context_id.
  - Flush at sentence boundaries for progressive audio.
  - Close context to stop generation (barge-in).
  - Audio arrives as base64-encoded PCM chunks (pcm_16000 format).

The client maintains ONE persistent WebSocket to ElevenLabs for the process
lifetime. Each speech turn creates a new context, so concurrent turns are
isolated.
"""

from __future__ import annotations

import os
import json
import uuid
import asyncio
import logging
import base64
from typing import AsyncIterator, Optional

import websockets

log = logging.getLogger("NEXUS.sidecar.tts")

ELEVENLABS_API_KEY = os.getenv("ELEVENLABS_API_KEY", "")
ELEVENLABS_VOICE_ID = os.getenv("ELEVENLABS_VOICE_ID", "")
ELEVENLABS_MODEL = os.getenv("ELEVENLABS_MODEL", "eleven_flash_v2_5")

# Multi-context WebSocket URL.
_WS_URL = (
    f"wss://api.elevenlabs.io/v1/text-to-speech/{ELEVENLABS_VOICE_ID}"
    f"/multi-stream-input?model_id={ELEVENLABS_MODEL}&output_format=pcm_16000"
)

# Default voice settings (can be overridden per-context).
_DEFAULT_VOICE_SETTINGS = {
    "stability": 0.5,
    "similarity_boost": 0.8,
    "style": 0.0,
    "use_speaker_boost": True,
}

# Chunk schedule for streaming: flush at these character thresholds.
# Smaller = lower latency, slightly less natural prosody.
_DEFAULT_CHUNK_SCHEDULE = [120, 160, 250, 290]


class ElevenLabsTTS:
    """
    Persistent multi-context TTS client.

    Usage:
        tts = ElevenLabsTTS()
        await tts.connect()

        # Speak an ack phrase immediately
        async for pcm_chunk in tts.speak("On it, sir."):
            send_to_client(pcm_chunk)

        # Speak the final result (flushed at sentence boundaries)
        async for pcm_chunk in tts.speak(result_text, flush_sentences=True):
            send_to_client(pcm_chunk)

        # Barge-in: stop current speech
        await tts.stop_current()

    The WebSocket stays open across multiple speak() calls.
    """

    def __init__(self) -> None:
        self._ws: Optional[websockets.WebSocketClientProtocol] = None
        self._connect_lock = asyncio.Lock()
        self._active_context: Optional[str] = None
        self._audio_queue: asyncio.Queue[Optional[bytes]] = asyncio.Queue()
        self._reader_task: Optional[asyncio.Task] = None

    async def connect(self) -> None:
        """Open the persistent WebSocket connection. Safe to call multiple times."""
        async with self._connect_lock:
            if self._ws is not None and not self._ws.closed:
                return
            if not ELEVENLABS_API_KEY or not ELEVENLABS_VOICE_ID:
                raise RuntimeError("ELEVENLABS_API_KEY and ELEVENLABS_VOICE_ID must be set")

            log.info("connecting to ElevenLabs TTS WebSocket (voice=%s, model=%s)", ELEVENLABS_VOICE_ID, ELEVENLABS_MODEL)
            self._ws = await websockets.connect(
                _WS_URL,
                additional_headers={"xi-api-key": ELEVENLABS_API_KEY},
            )
            # Start the reader task that pumps audio chunks into the queue.
            self._reader_task = asyncio.create_task(self._read_loop())
            log.info("ElevenLabs TTS connected")

    async def disconnect(self) -> None:
        """Close the WebSocket and stop the reader."""
        if self._reader_task:
            self._reader_task.cancel()
            self._reader_task = None
        if self._ws and not self._ws.closed:
            await self._ws.close()
        self._ws = None

    async def _read_loop(self) -> None:
        """Background task: read audio chunks from ElevenLabs and enqueue them."""
        try:
            async for raw in self._ws:
                try:
                    msg = json.loads(raw)
                except (json.JSONDecodeError, TypeError):
                    continue

                mtype = msg.get("type") or msg.get("audio")

                # Audio chunk: base64-encoded PCM.
                if mtype == "audio" or "audio" in msg:
                    audio_b64 = msg.get("audio_base_64") or msg.get("audio")
                    if audio_b64:
                        pcm = base64.b64decode(audio_b64)
                        await self._audio_queue.put(pcm)

                # Context finished — signal end of this speech turn.
                elif mtype == "flush_done" or msg.get("flush_done"):
                    context_id = msg.get("context_id")
                    if context_id == self._active_context or self._active_context is None:
                        await self._audio_queue.put(None)  # sentinel: no more audio

                # Error from ElevenLabs.
                elif mtype == "error":
                    log.error("ElevenLabs error: %s", msg.get("message", msg))
                    await self._audio_queue.put(None)

        except asyncio.CancelledError:
            pass
        except Exception:
            log.exception("ElevenLabs reader loop crashed")
            await self._audio_queue.put(None)

    async def speak(
        self,
        text: str,
        context_id: Optional[str] = None,
        flush_sentences: bool = False,
    ) -> AsyncIterator[bytes]:
        """
        Speak `text` and yield raw 16-bit PCM mono 16kHz chunks.

        Args:
            text: The text to synthesize.
            context_id: Optional context ID. If None, a new one is generated.
            flush_sentences: If True, flush after each sentence boundary
                             for progressive playback of long text.

        Yields:
            Raw PCM bytes (16-bit LE mono 16kHz), ready for the client player.
        """
        await self.connect()

        ctx = context_id or f"ctx_{uuid.uuid4().hex[:12]}"
        self._active_context = ctx

        # Drain any leftover audio from a previous turn.
        while not self._audio_queue.empty():
            try:
                self._audio_queue.get_nowait()
            except asyncio.QueueEmpty:
                break

        # Initialize the context with voice settings (first message).
        init_msg = {
            "text": " ",  # space — required for context init
            "context_id": ctx,
            "voice_settings": _DEFAULT_VOICE_SETTINGS,
            "generation_config": {"chunk_length_schedule": _DEFAULT_CHUNK_SCHEDULE},
        }
        await self._ws.send(json.dumps(init_msg))

        if flush_sentences:
            # Send text sentence-by-sentence, flushing after each.
            for sentence in _split_sentences(text):
                if not sentence.strip():
                    continue
                await self._ws.send(json.dumps({
                    "text": sentence,
                    "context_id": ctx,
                }))
                # Flush to get audio for this sentence immediately.
                await self._ws.send(json.dumps({
                    "text": "",
                    "context_id": ctx,
                    "flush": True,
                }))
        else:
            # Send all text at once, then flush.
            await self._ws.send(json.dumps({"text": text, "context_id": ctx}))
            await self._ws.send(json.dumps({"text": "", "context_id": ctx, "flush": True}))

        # Yield audio chunks until the flush_done sentinel (None) arrives.
        while True:
            chunk = await self._audio_queue.get()
            if chunk is None:
                break
            yield chunk

        self._active_context = None

    async def stop_current(self) -> None:
        """Barge-in: close the active context to stop speech immediately."""
        if self._ws is None or self._ws.closed:
            return
        if self._active_context:
            try:
                await self._ws.send(json.dumps({
                    "context_id": self._active_context,
                    "close_context": True,
                }))
            except Exception:
                log.warning("failed to close TTS context")
        self._active_context = None
        # Drain the queue so the next speak() starts clean.
        while not self._audio_queue.empty():
            try:
                self._audio_queue.get_nowait()
            except asyncio.QueueEmpty:
                break


def _split_sentences(text: str) -> list[str]:
    """Split text into sentences at common boundaries. Keeps the delimiter."""
    import re
    # Split on . ! ? followed by space or end, keeping the delimiter.
    parts = re.split(r'(?<=[.!?])\s+', text.strip())
    return [p for p in parts if p.strip()]


# ---- Module-level singleton for the process lifetime ----
_tts: Optional[ElevenLabsTTS] = None


def get_tts() -> ElevenLabsTTS:
    global _tts
    if _tts is None:
        _tts = ElevenLabsTTS()
    return _tts


async def shutdown_tts() -> None:
    global _tts
    if _tts is not None:
        await _tts.disconnect()
        _tts = None
