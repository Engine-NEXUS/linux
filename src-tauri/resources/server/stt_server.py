"""
Moonshine STT server for NEXUS — local fallback when Groq cloud is unavailable.

This server runs LOCALLY on the user's device (127.0.0.1:39217).
Audio is sent from the NEXUS Rust client to this local server, transcribed,
and only the resulting TEXT is returned.

Architecture:
  Primary:   Groq Whisper Large v3 Turbo (cloud, ~247ms, free tier)
  Fallback:  Moonshine Small Streaming (local, ~165ms CPU, 7.84% WER)

Moonshine is used instead of faster-whisper because:
  - Moonshine Small Streaming (123M params) has 7.84% WER vs Whisper tiny.en's ~18%
  - Moonshine is 10-100x faster than Whisper for real-time speech
  - Moonshine uses ONNX Runtime (same as our wake word engine)
  - Moonshine is designed for voice command recognition (low latency, streaming)

Requirements:
  pip install moonshine-voice fastapi uvicorn python-multipart

Run locally on the device:
  uvicorn stt_server:app --host 127.0.0.1 --port 39217

Environment:
  MOONSHINE_MODEL  — model architecture name (default: small_streaming)
                     Options: tiny_streaming, small_streaming, medium_streaming
  MOONSHINE_LANG   — language code (default: en)

Models (CPU, ONNX Runtime):
  - tiny_streaming:   34M params,  12.00% WER, ~69ms on Linux x86
  - small_streaming:  123M params,  7.84% WER, ~165ms on Linux x86  (default)
  - medium_streaming: 245M params,  6.65% WER, ~269ms on Linux x86
"""

from __future__ import annotations

import os
import logging
import time
import io
import struct
import wave
import array
import json

from fastapi import FastAPI, UploadFile, File, WebSocket, WebSocketDisconnect
from fastapi.responses import JSONResponse

log = logging.getLogger("NEXUS.stt")
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")

# ─── Model Configuration ───────────────────────────────────────────────────

# Default to small_streaming — best balance of accuracy (7.84% WER) and speed
# (~165ms on CPU). For maximum accuracy, set MOONSHINE_MODEL=medium_streaming
# (6.65% WER, ~269ms, ~300-600MB RAM).
MODEL_ARCH_NAME = os.getenv("MOONSHINE_MODEL", "small_streaming")
LANGUAGE = os.getenv("MOONSHINE_LANG", "en")

# Map model name strings to ModelArch enum values
_MODEL_ARCH_MAP = {
    "tiny_streaming": None,    # Will be set after import
    "small_streaming": None,
    "medium_streaming": None,
    "tiny": None,
    "base": None,
}

app = FastAPI(title="NEXUS STT (Moonshine)", version="0.3.0")

# ─── Model Loading ─────────────────────────────────────────────────────────

_transcriber = None
_model_path = None
_model_arch = None
_model_arch_name = MODEL_ARCH_NAME
# Why the model could not be loaded (missing python package, no network, …).
# Surfaced by /health and /transcribe so the Rust client can report the real
# reason instead of a bare "server did not become ready".
_load_error = None


def _load_model():
    """Load the Moonshine model. Called eagerly at startup."""
    global _transcriber, _model_path, _model_arch, _model_arch_name, _load_error

    try:
        from moonshine_voice import Transcriber, ModelArch, get_model_for_language
    except ImportError as e:
        # The most common failure on a fresh Linux box: the package is simply
        # not installed. Say exactly that — a raw traceback here used to look
        # like "NEXUS did not hear you".
        _load_error = (
            "moonshine-voice is not installed — "
            f"pip install moonshine-voice fastapi uvicorn python-multipart ({e})"
        )
        log.error("local STT unavailable: %s", _load_error)
        raise

    # Map string names to ModelArch enum
    arch_map = {
        "tiny_streaming": ModelArch.TINY_STREAMING,
        "small_streaming": ModelArch.SMALL_STREAMING,
        "medium_streaming": ModelArch.MEDIUM_STREAMING,
        "tiny": ModelArch.TINY,
        "base": ModelArch.BASE,
    }

    model_arch_enum = arch_map.get(MODEL_ARCH_NAME.lower())
    if model_arch_enum is None:
        log.warning(
            "Unknown MOONSHINE_MODEL='%s', defaulting to small_streaming",
            MODEL_ARCH_NAME,
        )
        model_arch_enum = ModelArch.SMALL_STREAMING
        _model_arch_name = "small_streaming"

    # Download model files if not cached (first run)
    log.info("loading moonshine model=%s language=%s", _model_arch_name, LANGUAGE)
    _load_start = time.monotonic()

    model_path, default_arch = get_model_for_language(LANGUAGE)

    _transcriber = Transcriber(
        model_path=model_path,
        model_arch=model_arch_enum,
    )

    log.info(
        "moonshine model loaded in %.1fs — ready for transcription",
        time.monotonic() - _load_start,
    )
    _model_path = model_path
    _model_arch = model_arch_enum
    _load_error = None


def _get_transcriber():
    """Get the loaded transcriber instance."""
    global _load_error
    if _transcriber is None:
        try:
            _load_model()
        except Exception as e:
            if _load_error is None:
                _load_error = str(e)
            raise
    return _transcriber


# ─── Endpoints ─────────────────────────────────────────────────────────────


@app.get("/health")
async def health() -> JSONResponse:
    # A server that cannot transcribe is NOT healthy. Answering 503 makes the
    # Rust client stop waiting for it and report the real reason, instead of
    # burning the full 20 s startup budget on a sidecar that will never work.
    if _transcriber is None and _load_error is not None:
        return JSONResponse(
            {
                "ok": False,
                "model": f"moonshine-{_model_arch_name}",
                "language": LANGUAGE,
                "engine": "moonshine-voice (ONNX Runtime)",
                "error": _load_error,
            },
            status_code=503,
        )
    return JSONResponse({
        "ok": True,
        "model": f"moonshine-{_model_arch_name}",
        "language": LANGUAGE,
        "engine": "moonshine-voice (ONNX Runtime)",
    })


@app.post("/transcribe")
async def transcribe(audio: UploadFile = File(...)) -> JSONResponse:
    """Transcribe raw audio bytes. Returns {"text": "transcript"}.

    Accepts WAV (16kHz, mono, 16-bit PCM) or raw 16-bit LE mono PCM.
    """
    audio_bytes = await audio.read()
    if not audio_bytes:
        return JSONResponse({"text": ""}, status_code=400)

    # Check if the bytes start with a RIFF/WAV header. If not, assume raw
    # 16-bit LE mono PCM at 16kHz and wrap it in a WAV header.
    is_wav = len(audio_bytes) >= 12 and audio_bytes[:4] == b"RIFF" and audio_bytes[8:12] == b"WAVE"
    if not is_wav:
        sample_rate = 16000
        num_channels = 1
        bits_per_sample = 16
        data_len = len(audio_bytes)
        header = struct.pack(
            "<4sI4s4sIHHIIHH4sI",
            b"RIFF",
            36 + data_len,
            b"WAVE",
            b"fmt ",
            16,  # fmt chunk size
            1,   # PCM
            num_channels,
            sample_rate,
            sample_rate * num_channels * bits_per_sample // 8,  # byte rate
            num_channels * bits_per_sample // 8,  # block align
            bits_per_sample,
            b"data",
            data_len,
        )
        audio_bytes = header + audio_bytes
        log.info("wrapped raw PCM (%d bytes) in WAV header", data_len)

    # Parse WAV to extract raw float samples for Moonshine
    try:
        audio_data, sample_rate = _wav_to_float_samples(audio_bytes)
    except Exception as e:
        log.error("WAV parsing failed: %s", e)
        return JSONResponse({"text": "", "error": str(e)}, status_code=500)

    if len(audio_data) == 0:
        return JSONResponse({"text": ""})

    # Transcribe using Moonshine
    try:
        transcriber = _get_transcriber()
    except Exception as e:
        reason = _load_error or str(e)
        log.error("local STT unavailable: %s", reason)
        return JSONResponse({"text": "", "error": reason}, status_code=503)
    _transcribe_start = time.monotonic()
    try:
        transcript = transcriber.transcribe_without_streaming(
            audio_data=audio_data,
            sample_rate=sample_rate,
        )
        # Join all transcript lines
        text = " ".join(line.text for line in transcript.lines).strip()
    except Exception as e:
        log.error("transcription failed: %s", e)
        return JSONResponse({"text": "", "error": str(e)}, status_code=500)

    _elapsed = time.monotonic() - _transcribe_start
    log.info(
        "transcribed %d samples → %d chars in %.2fs: '%s'",
        len(audio_data), len(text), _elapsed, text,
    )
    return JSONResponse({"text": text})


@app.websocket("/stream")
async def stream(websocket: WebSocket) -> None:
    """Live partial-transcript stream (plan Phase 4 — additive, best-effort).

    Runs ENTIRELY PARALLEL to /transcribe: this is for the on-screen live
    caption only (the user's own words growing while they still talk) and
    never feeds intent parsing/NLU/brain — that stays the batch /transcribe
    path, untouched. A dropped or failed connection here must never affect
    command execution; the Rust client treats every error as a silent no-op.

    Protocol: client sends raw 16-bit LE mono PCM @ 16kHz binary frames
    (same convention as /transcribe's raw-PCM branch); server pushes
    {"text": "<growing line>"} whenever Moonshine's LineTextChanged fires
    (the full current line, not a delta — Moonshine's own semantics).
    A text frame {"cmd": "stop"} or a client disconnect ends the stream.

    Uses a FRESH `Stream` per connection (one per NEXUS turn) off the
    existing global transcriber (reuses the already-loaded model — never
    loads a second one), so no text state bleeds between turns.
    """
    await websocket.accept()
    try:
        transcriber = _get_transcriber()
    except Exception as e:
        reason = _load_error or str(e)
        log.error("stream: local STT unavailable: %s", reason)
        try:
            await websocket.close(code=1011)
        except Exception:
            pass
        return
    stream_handle = transcriber.create_stream()
    pending_texts: list[str] = []

    def _on_event(event) -> None:
        from moonshine_voice import LineTextChanged
        if isinstance(event, LineTextChanged):
            pending_texts.append(event.line.text)

    stream_handle.add_listener(_on_event)
    stream_handle.start()
    log.info("stream: client connected")
    try:
        while True:
            message = await websocket.receive()
            if message.get("type") == "websocket.disconnect":
                break
            audio_bytes = message.get("bytes")
            if audio_bytes:
                try:
                    samples = array.array("h", audio_bytes)
                    float_samples = [s / 32768.0 for s in samples]
                    stream_handle.add_audio(float_samples, sample_rate=16000)
                except Exception as e:
                    log.warning("stream: add_audio failed — %s", e)
                for text in pending_texts:
                    await websocket.send_json({"text": text})
                pending_texts.clear()
                continue
            text_msg = message.get("text")
            if text_msg:
                try:
                    payload = json.loads(text_msg)
                except Exception:
                    payload = {}
                if payload.get("cmd") == "stop":
                    break
    except WebSocketDisconnect:
        log.info("stream: client disconnected")
    except Exception as e:
        log.warning("stream: unexpected error — %s", e)
    finally:
        try:
            # stop() transcribes whatever audio is left and fires the final
            # LineTextChanged/LineCompleted events into pending_texts via
            # _on_event — without flushing those here, the LAST (often most
            # complete) partial would be silently dropped.
            stream_handle.stop()
        except Exception:
            pass
        try:
            for text in pending_texts:
                await websocket.send_json({"text": text})
        except Exception:
            pass
        try:
            await websocket.close()
        except Exception:
            pass
        log.info("stream: closed")


def _wav_to_float_samples(wav_bytes: bytes):
    """Parse WAV bytes and return (float_samples, sample_rate).

    Converts 16-bit signed PCM to float32 in range [-1.0, 1.0].
    """
    wf = wave.open(io.BytesIO(wav_bytes), "rb")
    sample_rate = wf.getframerate()
    n_channels = wf.getnchannels()
    sample_width = wf.getsampwidth()
    n_frames = wf.getnframes()
    raw = wf.readframes(n_frames)
    wf.close()

    if sample_width == 2:
        # 16-bit signed PCM → float32
        import array
        samples = array.array("h", raw)
        if n_channels > 1:
            # Downmix to mono by averaging channels
            mono = []
            for i in range(0, len(samples), n_channels):
                chunk = samples[i:i + n_channels]
                mono.append(sum(chunk) // n_channels)
            samples = array.array("h", mono)
        float_samples = [s / 32768.0 for s in samples]
    elif sample_width == 4:
        # 32-bit signed PCM → float32
        import array
        samples = array.array("i", raw)
        if n_channels > 1:
            mono = []
            for i in range(0, len(samples), n_channels):
                chunk = samples[i:i + n_channels]
                mono.append(sum(chunk) // n_channels)
            samples = array.array("i", mono)
        float_samples = [s / 2147483648.0 for s in samples]
    else:
        raise ValueError(f"Unsupported sample width: {sample_width} bytes")

    return float_samples, sample_rate


# ─── Startup ───────────────────────────────────────────────────────────────


@app.on_event("startup")
async def _startup():
    """Eagerly load the model at startup so the first transcription is fast.

    A missing dependency must NOT kill the server: the Rust client polls
    /health, and a dead process looks like "no STT at all". Stay up and
    report the reason through /health (503) instead.
    """
    global _load_error
    try:
        _load_model()
    except Exception as e:
        if _load_error is None:
            _load_error = str(e)
        log.error("local STT unavailable at startup: %s", e)


if __name__ == "__main__":
    import uvicorn
    port = int(os.environ.get("NEXUS_STT_PORT", "39217"))
    uvicorn.run(app, host="127.0.0.1", port=port, log_level="warning")
