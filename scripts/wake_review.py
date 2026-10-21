"""Review live wake clips and turn them into labeled training/test data.

With "Save wake clips" on (Command Hub, setting `wakeClipCapture`), the app
saves ~3.5 s of audio around every wake fire, rejected candidate and near-miss
to %APPDATA%/com.nexus.assistant/wake_clips/ as <id>.wav + <id>.json. This
tool plays each clip and asks what it was:

  y  = I said "nexus"            -> positive
  n  = not "nexus" (false wake)   -> negative
  r  = replay      s = skip (keep for later)      d = delete      q = quit

Labeled clips are moved into wake_word_data/real/live-<date>/ and appended to
wake_word_data/manifest.jsonl (origin "live"). Each recording day is one
session and goes wholly to one split: test until the live test set holds
--test-target clips, train after that (the choice is remembered per day in
wake_word_data/live_sessions.json so a day never crosses splits).

Sidecar (written by src-tauri/src/wake_clips.rs):
  {"version":1, "id", "ts_ms", "kind": fire|verify_reject|speaker_reject|dropped|near_miss,
   "prob", "transcript", "rms", "tts_playing", "hint": likely_wake|likely_false|likely_miss|null,
   "sample_rate", "pre_ms"}

Everything stays on this PC. Usage:
  python scripts/wake_review.py                 # review oldest first
  python scripts/wake_review.py --kind near_miss
  python scripts/wake_review.py --stats
"""

import argparse
import datetime
import json
import os
import shutil
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "wake_word_data"
MANIFEST = DATA / "manifest.jsonl"
SESSIONS = DATA / "live_sessions.json"
CLIPS = Path(os.path.expandvars(r"%APPDATA%")) / "com.nexus.assistant" / "wake_clips"

HINT_TEXT = {
    "likely_wake": "probably a real wake (a command followed)",
    "likely_false": "probably a FALSE wake (dismissed / no command)",
    "likely_miss": "probably a MISSED wake (you repeated nexus or used the hotkey)",
}


def load_sessions():
    if SESSIONS.exists():
        return json.loads(SESSIONS.read_text(encoding="utf-8"))
    return {}


def live_test_count():
    n = 0
    if MANIFEST.exists():
        for line in MANIFEST.read_text(encoding="utf-8").splitlines():
            if line.strip():
                r = json.loads(line)
                if r.get("origin") == "live" and r.get("split") == "test":
                    n += 1
    return n


def split_for_day(day, sessions, test_target):
    if day not in sessions:
        sessions[day] = "test" if live_test_count() < test_target else "train"
        SESSIONS.write_text(json.dumps(sessions, indent=2), encoding="utf-8")
    return sessions[day]


def play(path):
    try:
        import winsound
        winsound.PlaySound(str(path), winsound.SND_FILENAME)
    except Exception as e:  # non-Windows or unsupported format
        print(f"  (could not play: {e}; open {path} manually)")


def pending(kind=None):
    if not CLIPS.exists():
        return []
    items = []
    for meta_path in sorted(CLIPS.glob("*.json")):
        wav = meta_path.with_suffix(".wav")
        if not wav.exists():
            continue
        try:
            meta = json.loads(meta_path.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            continue
        if kind and meta.get("kind") != kind:
            continue
        items.append((meta.get("ts_ms", 0), wav, meta_path, meta))
    items.sort(key=lambda x: x[0])
    return items


def label_clip(wav, meta_path, meta, label, sessions, test_target):
    ts = datetime.datetime.fromtimestamp(meta.get("ts_ms", 0) / 1000)
    day = ts.strftime("%Y-%m-%d")
    session = f"live-{day}"
    split = split_for_day(day, sessions, test_target)
    dest_rel = f"real/{session}/{'positive' if label == 'positive' else 'negative'}/{wav.name}"
    dest = DATA / dest_rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.move(str(wav), dest)
    shutil.move(str(meta_path), dest.with_suffix(".json"))
    row = {
        "audio": dest_rel,
        "label": label,
        "phrase": "nexus" if label == "positive" else "other",
        "speaker_id": "owner",
        "accent": "en-IN",
        "device_class": "laptop-array",
        "environment": "home",
        "distance_cm": 60,
        "volume": "normal",
        "session_id": session,
        "source_group_id": session,
        "split": split,
        "consent": True,
        "origin": "live",
        "condition": f"live-{meta.get('kind', 'clip')}" + ("-tts" if meta.get("tts_playing") else ""),
        "stage1_prob": meta.get("prob"),
    }
    with MANIFEST.open("a", encoding="utf-8") as f:
        f.write(json.dumps(row) + "\n")
    return split


def stats():
    items = pending()
    print(f"Pending clips in {CLIPS}: {len(items)}")
    for (kind, hint), n in sorted(Counter((m.get("kind"), m.get("hint")) for *_, m in items).items(), key=str):
        print(f"  {kind:<15} hint={hint}: {n}")
    labeled = Counter()
    if MANIFEST.exists():
        for line in MANIFEST.read_text(encoding="utf-8").splitlines():
            if line.strip():
                r = json.loads(line)
                if r.get("origin") == "live":
                    labeled[(r["split"], r["label"])] += 1
    print("Labeled live clips:", dict(labeled) or "none yet")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--kind", help="only this clip kind (fire, near_miss, ...)")
    ap.add_argument("--stats", action="store_true")
    ap.add_argument("--test-target", type=int, default=300,
                    help="live clips to collect in the test split before new days go to train")
    a = ap.parse_args()
    if a.stats:
        stats()
        return 0

    items = pending(a.kind)
    if not items:
        print(f"No clips to review in {CLIPS}. Turn on 'Save wake clips' in Command Hub and use NEXUS normally.")
        return 0
    sessions = load_sessions()
    done = Counter()
    print(f"{len(items)} clips to review. y=nexus  n=not nexus  r=replay  s=skip  d=delete  q=quit\n")
    for i, (_, wav, meta_path, meta) in enumerate(items, 1):
        hint = HINT_TEXT.get(meta.get("hint"), "")
        print(f"[{i}/{len(items)}] {meta.get('kind')}  stage-1 prob {meta.get('prob', 0):.2f}"
              f"{'  (NEXUS was speaking)' if meta.get('tts_playing') else ''}")
        if meta.get("transcript"):
            print(f"  heard: \"{meta['transcript']}\"")
        if hint:
            print(f"  hint: {hint}")
        play(wav)
        while True:
            ans = input("  > ").strip().lower()
            if ans == "r":
                play(wav)
                continue
            if ans in ("y", "n"):
                label = "positive" if ans == "y" else "negative"
                split = label_clip(wav, meta_path, meta, label, sessions, a.test_target)
                done[(label, split)] += 1
            elif ans == "d":
                wav.unlink(missing_ok=True)
                meta_path.unlink(missing_ok=True)
                done["deleted"] += 1
            elif ans == "s":
                done["skipped"] += 1
            elif ans == "q":
                print(dict(done))
                return 0
            else:
                print("  y / n / r / s / d / q")
                continue
            break
    print(dict(done))
    return 0


if __name__ == "__main__":
    sys.exit(main())
