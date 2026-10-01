"""
RapidOCR server for NEXUS — local text grounding tier for the screen agent.

This server runs LOCALLY on the user's device (127.0.0.1:39220).
A screenshot JPEG is sent from the NEXUS Rust client, OCR'd on CPU via
RapidOCR (PP-OCRv4 ONNX, ~100-300ms), and text boxes are returned.

Grounding order (see docs/mcp/09-screen-vision-control.md):
  1. UIA tree (Windows, exact, ~5ms) / hotkeys for tabs (instant)
  2. THIS SERVER — OCR text boxes for canvas/custom UI (~300ms, free)
  3. Vision LLM grid fallback (~1-3s, free-tier quota)

Sync note: production builds bundle src-tauri/resources/server/ocr_server.py
(a checked-in copy of this file) via tauri.conf.json resources. Keep both
identical — see server/stt_server.py precedent.

Requirements:
  pip install rapidocr-onnxruntime opencv-python-headless fastapi uvicorn

Run locally on the device:
  uvicorn ocr_server:app --host 127.0.0.1 --port 39220

Environment:
  OCR_PORT — override the listen port (default: 39220)
"""

from __future__ import annotations

import logging
import os
import time

import cv2
import numpy as np
from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse

log = logging.getLogger("NEXUS.ocr")
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")

PORT = int(os.getenv("OCR_PORT", "39220"))

app = FastAPI(title="NEXUS OCR (RapidOCR)", version="0.1.0")

_engine = None


def _get_engine():
    """Lazily create the RapidOCR engine (downloads ~15MB ONNX on first use)."""
    global _engine
    if _engine is None:
        from rapidocr_onnxruntime import RapidOCR

        t0 = time.monotonic()
        _engine = RapidOCR()
        log.info("RapidOCR engine ready in %.1fs", time.monotonic() - t0)
    return _engine


@app.get("/health")
async def health() -> JSONResponse:
    return JSONResponse({"status": "ok", "service": "nexus-ocr"})


@app.post("/ocr")
async def ocr(request: Request) -> JSONResponse:
    """OCR a raw JPEG/PNG image body. Returns axis-aligned text boxes in
    ORIGINAL image pixels: {"boxes": [{text, x, y, w, h, conf}]}."""
    t0 = time.monotonic()
    body = await request.body()
    if not body:
        return JSONResponse({"error": "empty image"}, status_code=400)
    img = cv2.imdecode(np.frombuffer(body, dtype=np.uint8), cv2.IMREAD_COLOR)
    if img is None:
        return JSONResponse({"error": "could not decode image"}, status_code=400)
    try:
        result, _ = _get_engine()(img)
    except Exception as e:  # model download failure, OOM, ...
        log.exception("OCR failed")
        return JSONResponse({"error": f"ocr failed: {e}"}, status_code=500)
    boxes = []
    for item in result or []:
        try:
            pts, text, conf = item[0], str(item[1]), float(item[2])
        except (IndexError, TypeError, ValueError):
            continue
        xs = [p[0] for p in pts]
        ys = [p[1] for p in pts]
        x0, y0, x1, y1 = int(min(xs)), int(min(ys)), int(max(xs)), int(max(ys))
        if x1 <= x0 or y1 <= y0 or not text.strip():
            continue
        boxes.append(
            {"text": text.strip(), "x": x0, "y": y0, "w": x1 - x0, "h": y1 - y0,
             "conf": round(conf, 3)}
        )
    ms = int((time.monotonic() - t0) * 1000)
    log.info("OCR: %d bytes -> %d boxes in %dms", len(body), len(boxes), ms)
    return JSONResponse({"boxes": boxes, "latency_ms": ms})
