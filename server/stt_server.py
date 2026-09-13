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

from fastapi import FastAPI, UploadFile, File
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


def _load_model():
    """Load the Moonshine model. Called eagerly at startup."""
    global _transcriber, _model_path, _model_arch, _model_arch_name

    from moonshine_voice import Transcriber, ModelArch, get_model_for_language

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


def _get_transcriber():
    """Get the loaded transcriber instance."""
    if _transcriber is None:
        _load_model()
    return _transcriber


# ─── Endpoints ─────────────────────────────────────────────────────────────


@app.get("/health")
async def health() -> JSONResponse:
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
    transcriber = _get_transcriber()
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
    """Eagerly load the model at startup so the first transcription is fast."""
    _load_model()


if __name__ == "__main__":
    import uvicorn
    port = int(os.environ.get("NEXUS_STT_PORT", "39217"))
    uvicorn.run(app, host="127.0.0.1", port=port, log_level="warning")
