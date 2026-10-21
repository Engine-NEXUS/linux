"""Record real ambient audio on THIS microphone for the wake false-fire gate.

Wake accuracy is only meaningful against hours of the sound your PC actually
hears: TV, YouTube and podcasts (English, Hindi, Telugu), music, conversation,
typing, room tone with the fan running, and NEXUS's own voice through the
speakers. `nexus wake replay` counts stage-1 false fires per hour on these
recordings. The release gate needs at least 10 hours.

What it does:
  - Records the default input device at its native rate and channel count
    (the replay harness then resamples and downmixes exactly like the app).
  - Writes 60-second WAV chunks to wake_word_data/real/ambient-<date>-<category>/.
  - Appends one manifest row per chunk (split "test", label "background",
    origin "ambient", condition = category) to wake_word_data/manifest.jsonl.
    `build_wake_manifest.py` keeps these rows when it rebuilds.

Rules while recording: do NOT say "nexus" (or let the TV say it). If a real
"nexus" slips in, the replay lists it as a false fire with its timestamp;
delete that chunk and its manifest row.

PRIVACY: records whatever the microphone hears, including other people. It
asks for typed confirmation, writes only inside wake_word_data/ (git-ignored),
and never uploads anything. Do not record people who have not agreed.

Usage:
  python scripts/record_wake_ambient.py --category tv --minutes 60
  python scripts/record_wake_ambient.py --list          # hours recorded per category

Requires: pip install sounddevice numpy scipy
"""

import argparse
import datetime
import json
import sys
import time
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "wake_word_data"
MANIFEST = DATA / "manifest.jsonl"

CATEGORIES = [
    "tv", "youtube", "podcast", "music", "conversation", "phone-call", "typing",
    "room", "kitchen", "tts-echo", "hindi", "telugu", "other",
]
TARGET_HOURS = 10.0


def list_hours():
    hours = defaultdict(float)
    if MANIFEST.exists():
        for line in MANIFEST.read_text(encoding="utf-8").splitlines():
            if line.strip():
                r = json.loads(line)
                if r.get("origin") == "ambient":
                    hours[r.get("condition", "?")] += float(r.get("duration_s", 0)) / 3600
    total = sum(hours.values())
    for c in sorted(hours):
        print(f"  {c:<14} {hours[c]:6.2f} h")
    print(f"  {'total':<14} {total:6.2f} h  (gate needs >= {TARGET_HOURS:.0f} h)")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--category", choices=CATEGORIES)
    ap.add_argument("--minutes", type=float, default=60)
    ap.add_argument("--chunk-seconds", type=int, default=60)
    ap.add_argument("--list", action="store_true", help="show hours recorded per category")
    a = ap.parse_args()

    if a.list:
        list_hours()
        return 0
    if not a.category:
        ap.error("--category is required (or use --list)")

    try:
        import numpy as np
        import sounddevice as sd
        from scipy.io import wavfile
    except ImportError as e:
        print(f"missing dependency: {e}\n  pip install sounddevice numpy scipy")
        return 2

    dev = sd.query_devices(kind="input")
    sr = int(dev["default_samplerate"])
    channels = max(1, min(2, int(dev["max_input_channels"])))
    today = datetime.date.today().isoformat()
    session = f"ambient-{today}-{a.category}"
    out = DATA / "real" / session
    print(f'Device: {dev["name"]} ({sr} Hz, {channels} ch)')
    print(f"Category: {a.category}, {a.minutes:.0f} min -> {out.relative_to(ROOT)}")
    print('Do not say "nexus" while recording.')
    ans = input("Type RECORD to start: ").strip()
    if ans != "RECORD":
        print("cancelled - nothing recorded")
        return 1

    out.mkdir(parents=True, exist_ok=True)
    end = time.time() + a.minutes * 60
    n = 0
    print("recording... Ctrl+C stops early (the current chunk is discarded)")
    try:
        while time.time() < end:
            secs = min(a.chunk_seconds, max(1, int(end - time.time())))
            buf = sd.rec(int(secs * sr), samplerate=sr, channels=channels, dtype="int16")
            sd.wait()
            name = f"{a.category}_{int(time.time())}_{n:04d}.wav"
            wavfile.write(out / name, sr, np.asarray(buf))
            row = {
                "audio": f"real/{session}/{name}",
                "label": "background",
                "phrase": "none",
                "speaker_id": "room",
                "accent": "mixed",
                "device_class": "laptop-array",
                "environment": "home",
                "distance_cm": 0,
                "volume": "normal",
                "session_id": session,
                "source_group_id": session,
                "split": "test",
                "consent": True,
                "origin": "ambient",
                "condition": a.category,
                "duration_s": secs,
            }
            with MANIFEST.open("a", encoding="utf-8") as f:
                f.write(json.dumps(row) + "\n")
            n += 1
            if n % 10 == 0:
                print(f"  {n * a.chunk_seconds / 60:.0f} min recorded")
    except KeyboardInterrupt:
        print("\nstopped")
    print(f"{n} chunks saved in {out.relative_to(ROOT)}")
    list_hours()
    return 0


if __name__ == "__main__":
    sys.exit(main())
