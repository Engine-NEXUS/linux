"""
RapidOCR server for NEXUS — local text grounding tier for the screen agent.

This server runs LOCALLY on the user's device (127.0.0.1:39220).
A screenshot JPEG is sent from the NEXUS Rust client, OCR'd on CPU via
RapidOCR (PP-OCRv6-tiny ONNX, ~1.5s on a 1080p screenshot), and text boxes
are returned.

Grounding order (see docs/mcp/09-screen-vision-control.md):
  1. UIA tree (Windows, exact, ~5ms) / hotkeys for tabs (instant)
  2. THIS SERVER — OCR text boxes for canvas/custom UI (~1.5s, free)
  3. Vision LLM grid fallback (~1-3s, free-tier quota)

Sync note: production builds bundle src-tauri/resources/server/ocr_server.py
(a checked-in copy of this file) via tauri.conf.json resources. Keep both
identical — see server/stt_server.py precedent.

Requirements:
  pip install "rapidocr==3.9.2" onnxruntime opencv-python-headless fastapi uvicorn

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
    """Lazily create the RapidOCR engine.

    Uses `rapidocr` 3.x, which bundles PP-OCRv6 det/rec ONNX models inside the
    wheel. The previous `rapidocr-onnxruntime` 1.4.4 shipped PP-OCRv4 and
    downloaded its weights on first use, so OCR was dead until the machine had
    network access. The models are now local to the install.

    `model_type=TINY` is deliberate, not a shortcut. Upstream defaults to
    `small`, which is the *slower* option on the low-power x86 laptops NEXUS
    targets. Measured on an i3-1215U over a 1920x1080 UI screenshot with 12 text
    regions (best of 3, warm):

        PP-OCRv4 small  (old default) ....  4138 ms
        PP-OCRv6 small  (upstream default)  5248 ms   <- 27% SLOWER than v4
        PP-OCRv6 tiny   (this config) ......  1493 ms   <- 2.8x faster than v4

    All three found the same 12 regions. The tiny detector also recovered a
    rotated/slanted label that PP-OCRv4's angle classifier handles worse, which
    matches PP-OCRv6's advertised gain on non-axis-aligned text.

    NOTE: the old "~100-300ms" figure in this file's header was aspirational and
    never true on this hardware. If it is ever needed for real, the fix is a
    GPU/Vulkan execution provider, not a smaller model.
    """
    global _engine
    if _engine is None:
        from rapidocr import RapidOCR
        from rapidocr.utils.typings import ModelType

        t0 = time.monotonic()
        _engine = RapidOCR(
            params={
                "Det.model_type": ModelType.TINY,
                "Rec.model_type": ModelType.TINY,
            }
        )
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
        out = _get_engine()(img)
    except Exception as e:  # model load failure, OOM, ...
        log.exception("OCR failed")
        return JSONResponse({"error": f"ocr failed: {e}"}, status_code=500)

    # rapidocr 3.x returns a RapidOCROutput dataclass with parallel
    # boxes/txts/scores fields, NOT the legacy `(result, elapse)` tuple that
    # rapidocr-onnxruntime 1.x returned. The dataclass is not iterable, so it
    # cannot be unpacked or looped directly.
    #
    # `boxes` is a numpy array, so the None-checks below are explicit rather
    # than `or []` — truthiness on a multi-element ndarray raises
    # "truth value of an array is ambiguous".
    raw_boxes = getattr(out, "boxes", None)
    raw_txts = getattr(out, "txts", None)
    raw_scores = getattr(out, "scores", None)
    boxes = []
    if raw_boxes is None or raw_txts is None or raw_scores is None:
        log.info("OCR: engine returned no detections")
        return JSONResponse({"boxes": [], "latency_ms": int((time.monotonic() - t0) * 1000)})
    for pts, text, conf in zip(raw_boxes, raw_txts, raw_scores):
        try:
            text, conf = str(text), float(conf)
            xs = [float(p[0]) for p in pts]
            ys = [float(p[1]) for p in pts]
            x0, y0 = int(min(xs)), int(min(ys))
            x1, y1 = int(max(xs)), int(max(ys))
        except (IndexError, TypeError, ValueError):
            continue
        if x1 <= x0 or y1 <= y0 or not text.strip():
            continue
        boxes.append(
            {"text": text.strip(), "x": x0, "y": y0, "w": x1 - x0, "h": y1 - y0,
             "conf": round(conf, 3)}
        )
    ms = int((time.monotonic() - t0) * 1000)
    log.info(
        "OCR: %d bytes -> %d boxes in %dms (engine %.0fms)",
        len(body), len(boxes), ms,
        float(getattr(out, "elapse", 0.0) or 0.0) * 1000,
    )
    return JSONResponse({"boxes": boxes, "latency_ms": ms})
