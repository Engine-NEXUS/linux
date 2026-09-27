"""
Vision OCR accuracy harness — Phase-3c locator verification.

Generates synthetic UI screenshots with text at KNOWN positions, runs them
through the real OCR server (server/ocr_server.py over HTTP, same path the
Rust client uses), and measures the snap-tier hit rate:

  hit = an OCR box scores screen::score_match(query) >= 0.5 AND its center
        lands within TOL_PX of the ground-truth center.

Target: >= 90% hits (the Phase-3c acceptance bar for OCR snapping).

Usage:
  python scripts/vision_accuracy.py [--images 12] [--port 39220]

The harness spawns its own ocr_server on the given port (or reuses a
running one) and kills what it spawned.
"""

from __future__ import annotations

import argparse
import io
import math
import random
import subprocess
import sys
import time
import urllib.request

sys.path.insert(0, "server")

WORDS = [
    "Save", "Cancel", "Settings", "Search", "Download", "Documents",
    "Sign in", "Learn more", "Apply", "Close window", "Open file",
    "Address bar", "New tab", "Play video", "Volume", "Next",
]
TOL_PX = 30.0
PASS_RATE = 0.90


def norm(s: str) -> str:
    return " ".join("".join(c.lower() if c.isalnum() else " " for c in s).split())


def score_match(query: str, name: str) -> float:
    """Port of screen::score_match (must stay in sync with the Rust side)."""
    q, n = norm(query), norm(name)
    if not q or not n:
        return 0.0
    if q == n:
        return 1.0
    if q in n or n in q:
        return 0.8
    # Space-insensitive containment (OCR drops spaces: "Closewindow").
    nq, nn = q.replace(" ", ""), n.replace(" ", "")
    if nq in nn or nn in nq:
        return 0.75
    qw, nw = q.split(" "), n.split(" ")
    hits = sum(1 for w in qw if w in nw)
    if hits == len(qw):
        return 0.7
    return 0.4 if hits else 0.0


def load_font():
    from PIL import ImageFont

    for path in [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "/System/Library/Fonts/SFNSDisplay.ttf",
    ]:
        try:
            return ImageFont.truetype(path, 22)
        except Exception:
            continue
    return ImageFont.load_default()


def make_image(rng: random.Random, idx: int):
    from PIL import Image, ImageDraw

    W, H = 1280, 720
    font = load_font()
    img = Image.new("RGB", (W, H), (245, 245, 245))
    d = ImageDraw.Draw(img)
    placed = []
    taken = []
    for _ in range(6):
        word = rng.choice(WORDS)
        # Rejection-sample a non-overlapping slot (padded by 20px).
        for _ in range(40):
            x = rng.randint(40, W - 340)
            y = rng.randint(40, H - 100)
            bx0, by0, bx1, by1 = d.textbbox((x, y), word, font=font)
            if all(bx1 + 36 < tx0 or tx1 + 36 < bx0 or by1 + 36 < ty0 or ty1 + 36 < by0
                   for (tx0, ty0, tx1, ty1) in taken):
                break
        else:
            continue
        d.text((x, y), word, fill=(20, 20, 20), font=font)
        # Exact ground truth from PIL itself (not an approximation).
        bx0, by0, bx1, by1 = d.textbbox((x, y), word, font=font)
        taken.append((bx0, by0, bx1, by1))
        placed.append((word, (bx0 + bx1) / 2, (by0 + by1) / 2))
    buf = io.BytesIO()
    img.save(buf, format="JPEG", quality=80)
    return buf.getvalue(), placed


def merge_lines(boxes):
    """Mirror of ocr::merge_lines (Rust) — must stay in sync."""
    boxes = sorted(boxes, key=lambda b: (b["y"], b["x"]))
    lines = []
    for b in boxes:
        placed = False
        for line in lines:
            top = min(m["y"] for m in line)
            bot = max(m["y"] + m["h"] for m in line)
            slack = max(b["h"] // 4, 2)
            if b["y"] < bot + slack and b["y"] + b["h"] > top - slack:
                line.append(b)
                placed = True
                break
        if not placed:
            lines.append([b])
    out = []
    for line in sorted(lines, key=lambda l: (l[0]["y"], l[0]["x"])):
        line = sorted(line, key=lambda b: b["x"])
        gap_limit = max(max(m["h"] for m in line), 8)
        run, prev_right = [], -10 ** 9
        for b in line:
            if run and b["x"] - prev_right > gap_limit:
                out.append(_fuse(run))
                run = []
            prev_right = b["x"] + b["w"]
            run.append(b)
        if run:
            out.append(_fuse(run))
    return out


def _fuse(run):
    return {
        "text": " ".join(b["text"] for b in run),
        "x": min(b["x"] for b in run),
        "y": min(b["y"] for b in run),
        "w": max(b["x"] + b["w"] for b in run) - min(b["x"] for b in run),
        "h": max(b["y"] + b["h"] for b in run) - min(b["y"] for b in run),
        "conf": min(b["conf"] for b in run),
    }


def post_ocr(port: int, jpeg: bytes):
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}/ocr", data=jpeg,
        headers={"Content-Type": "image/jpeg"}, method="POST",
    )
    with urllib.request.urlopen(req, timeout=60) as r:
        import json
        return json.load(r)["boxes"]


def wait_healthy(port: int, timeout: float = 90.0) -> bool:
    t0 = time.monotonic()
    while time.monotonic() - t0 < timeout:
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=3) as r:
                if r.status == 200:
                    return True
        except Exception:
            pass
        time.sleep(1.0)
    return False


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--images", type=int, default=12)
    ap.add_argument("--port", type=int, default=39220)
    args = ap.parse_args()

    child = None
    if not wait_healthy(args.port, timeout=3.0):
        env = {"OCR_PORT": str(args.port)}
        import os
        child = subprocess.Popen(
            [sys.executable, "-m", "uvicorn", "ocr_server:app",
             "--host", "127.0.0.1", "--port", str(args.port)],
            cwd="server", env={**os.environ, **env},
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        if not wait_healthy(args.port):
            print("FAIL: ocr server did not start")
            child.terminate()
            return 2

    rng = random.Random(20260927)
    hits, total = 0, 0
    try:
        for i in range(args.images):
            jpeg, placed = make_image(rng, i)
            boxes = merge_lines(post_ocr(args.port, jpeg))
            for word, gx, gy in placed:
                total += 1
                best = None
                for b in boxes:
                    s = score_match(word, b["text"])
                    if s < 0.5:
                        continue
                    cx, cy = b["x"] + b["w"] / 2, b["y"] + b["h"] / 2
                    d = math.hypot(cx - gx, cy - gy)
                    if best is None or (s, -d) > (best[0], -best[1]):
                        best = (s, d)
                if best is not None and best[1] <= TOL_PX:
                    hits += 1
                else:
                    print(f"  miss: {word!r} truth=({gx:.0f},{gy:.0f})")
    finally:
        if child is not None:
            child.terminate()

    rate = hits / total if total else 0.0
    print(f"OCR snap harness: {hits}/{total} = {rate:.1%} (bar: {PASS_RATE:.0%})")
    return 0 if rate >= PASS_RATE else 1


if __name__ == "__main__":
    raise SystemExit(main())
