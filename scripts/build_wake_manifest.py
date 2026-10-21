"""Build wake_word_data/manifest.jsonl with session-separated splits.

The trainer used to split by 80 ms window, so windows of one clip (and its
augmented copies) landed in both train and validation, and every reported
number was scored on training data. This manifest fixes the unit of
separation: a whole recording session (owner), voice (TTS) or generator
(procedural noise) belongs to exactly one split.

Splits:
  test        owner sessions never used to train the shipped model
              (wake_camp 2026-09-26 clips, the 2026-09-18 real background
              recordings) plus labeled live clips (P1, origin "live").
  validation  owner session 2026-09-23 and two held-out TTS voices.
  train       everything else.

Owner recordings share one speaker across splits on purpose: this is a
personal model for one voice, so sessions (not speakers) are the separation
unit for "owner" (see PERSONAL_SPEAKERS in wake_data_foundation.py).

Test clips that live outside wake_word_data (wake_camp/) are copied into
wake_word_data/real/<session>/ so manifest paths stay relative. Clips whose
audio hash also appears in a training folder are left out of test and
reported. Unlabeled clips go to needs_review.jsonl, never into a split.

Rows with origin "live" (added by `nexus wake review`) or "ambient" (added by
record_wake_ambient.py) are preserved.

Usage:
  python scripts/build_wake_manifest.py            # write manifest
  python scripts/build_wake_manifest.py --dry-run  # counts only
"""

import argparse
import datetime
import hashlib
import json
import re
import shutil
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "wake_word_data"
CAMP = ROOT / "wake_camp"
MANIFEST = DATA / "manifest.jsonl"
NEEDS_REVIEW = DATA / "needs_review.jsonl"

TEST_OWNER_SESSIONS = {"owner-2026-09-26", "room-2026-09-18"}
VALIDATION_OWNER_SESSIONS = {"owner-2026-09-23"}
VALIDATION_TTS_VOICES = {"en-GB-RyanNeural", "hi-IN-SwaraNeural"}

# wake_camp/raw/nexus/confusion: per-clip content from wake_camp/prompt_test.py
# and calibrate_nospeech.py. Clips not listed here are unlabeled.
CONFUSION_POSITIVE = {"NX01", "NX07", "NX19", "NX20", "NX30"}
CONFUSION_NEGATIVE = {
    "NX05": "lexus", "NX11": "lexus", "NX12": "lexus", "NX13": "lexus", "NX14": "lexus",
    "NX15": "lexus", "NX17": "lexus", "NX18": "lexus", "NX24": "lexus", "NX27": "lexus",
    "NX29": "lexus", "NX16": "access", "NX22": "process", "NX25": "alexis", "NX26": "letsjust",
}

PROCEDURAL_NEG = {
    "agc_stress", "clap", "cough", "gargle", "mic_testing", "shout", "sneeze",
    "throat_clear", "thump", "vocal_fry",
}


def sha256(path):
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def day(path):
    return datetime.datetime.fromtimestamp(path.stat().st_mtime).strftime("%Y-%m-%d")


def split_for(session):
    if session in TEST_OWNER_SESSIONS:
        return "test"
    if session in VALIDATION_OWNER_SESSIONS:
        return "validation"
    return "train"


def row(audio, label, *, phrase, speaker, session, group, split, origin, condition,
        environment="home", volume="normal", distance_cm=40, accent="en-IN",
        device_class="laptop-array"):
    return {
        "audio": audio.replace("\\", "/"),
        "label": label,
        "phrase": phrase,
        "speaker_id": speaker,
        "accent": accent,
        "device_class": device_class,
        "environment": environment,
        "distance_cm": distance_cm,
        "volume": volume,
        "session_id": session,
        "source_group_id": group,
        "split": split,
        "consent": True,
        "origin": origin,
        "condition": condition,
    }


def classify_training_file(folder, path):
    """Row for a file already inside wake_word_data/{positive,negative,background}."""
    name = path.name
    rel = f"{folder}/{name}"
    stem = re.sub(r"[_-]?\d+$", "", path.stem)

    if folder == "positive":
        if name.startswith("nexus_"):
            s = f"owner-{day(path)}"
            return row(rel, "positive", phrase="nexus", speaker="owner", session=s, group=s,
                       split=split_for(s), origin="human", condition="recorded")
        m = re.match(r"(?:neural_pos|synth_nexus)_([a-z]{2}-[A-Z]{2}-[A-Za-z]+Neural)", name)
        voice = m.group(1) if m else stem
        s = f"tts:{voice}"
        split = "validation" if voice in VALIDATION_TTS_VOICES else "train"
        return row(rel, "positive", phrase="nexus", speaker=s, session=s, group=s, split=split,
                   origin="tts", condition="tts", accent="mixed", device_class="synthetic")

    if folder == "negative":
        m = re.match(r"synth_([a-z]{2}-[A-Z]{2}-[A-Za-z]+Neural)_", name)
        if m:
            voice = m.group(1)
            s = f"tts:{voice}"
            split = "validation" if voice in VALIDATION_TTS_VOICES else "train"
            return row(rel, "negative", phrase="other", speaker=s, session=s, group=f"tts-neg:{voice}",
                       split=split, origin="tts", condition="tts-word", accent="mixed",
                       device_class="synthetic")
        if stem.startswith("synth_"):
            s = f"tts:{stem}"
            return row(rel, "negative", phrase="other", speaker=s, session=s, group=s, split="train",
                       origin="tts", condition="tts-word", accent="mixed", device_class="synthetic")
        if stem in PROCEDURAL_NEG:
            s = f"proc:{stem}"
            return row(rel, "negative", phrase="none", speaker="procedural", session=s, group=s,
                       split="train", origin="procedural", condition=stem, device_class="synthetic")
        # Human recordings (soundalikes, confusion words, phrases).
        s = f"owner-{day(path)}"
        return row(rel, "negative", phrase="other", speaker="owner", session=s, group=s,
                   split=split_for(s), origin="human", condition="spoken-word")

    # background
    if re.match(r"bg_\d+\.wav$", name):
        s = f"room-{day(path)}"
        return row(rel, "background", phrase="none", speaker="room", session=s, group=s,
                   split=split_for(s), origin="human", condition="room")
    s = f"proc:{stem}"
    return row(rel, "background", phrase="none", speaker="procedural", session=s, group=s,
               split="train", origin="procedural", condition=stem, device_class="synthetic")


def camp_rows(training_hashes, dry_run, report):
    """Test rows for wake_camp recordings (copied into wake_word_data/real/)."""
    rows, review = [], []
    raw = CAMP / "raw"
    sources = sorted(raw.rglob("*.wav")) + sorted((CAMP / "retrain_pack" / "eval-negatives").glob("*.wav"))
    seen = set()
    for src in sources:
        digest = sha256(src)
        if digest in seen:
            report["camp_duplicate_within_camp"] += 1
            continue
        seen.add(digest)
        if digest in training_hashes:
            report["camp_excluded_also_in_training"] += 1
            report.setdefault("camp_excluded_files", []).append(str(src.relative_to(ROOT)))
            continue
        parts = src.relative_to(CAMP).parts
        if parts[0] == "raw" and parts[1] in ("hey_nexus", "nexus"):
            phrase, condition, clip = parts[1], parts[2], src.stem
            session = "owner-2026-09-26" if day(src) == "2026-09-26" else f"owner-{day(src)}"
            dest_rel = f"real/{session}/{phrase}/{condition}/{src.name}"
            label = "positive"
            word = phrase
            if condition == "confusion":
                if clip in CONFUSION_POSITIVE:
                    word = "nexus"
                elif clip in CONFUSION_NEGATIVE:
                    label, word = "negative", CONFUSION_NEGATIVE[clip]
                else:
                    review.append({"audio": str(src.relative_to(ROOT)).replace("\\", "/"),
                                   "reason": "confusion clip without a confirmed word"})
                    continue
            volume = condition if condition in ("quiet", "loud") else "normal"
            distance = 250 if condition == "far" else 40
            r = row(dest_rel, label, phrase=word, speaker="owner", session=session, group=session,
                    split=split_for(session), origin="human", condition=condition,
                    volume=volume, distance_cm=distance)
        else:
            # Real background recordings (tv, conversation, room tone, household).
            session = f"room-{day(src)}"
            dest_rel = f"real/{session}/background/{src.name}"
            r = row(dest_rel, "background", phrase="none", speaker="room", session=session,
                    group=session, split=split_for(session), origin="human",
                    condition=src.stem.split("_")[0] if "_" in src.stem else src.stem)
        if r["split"] != "test":
            report["camp_not_test_session"] += 1
        dest = DATA / dest_rel
        if not dry_run and not dest.exists():
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dest)
        rows.append(r)
    return rows, review


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    report = Counter()
    rows = []
    training_hashes = set()
    for folder in ("positive", "negative", "background"):
        for path in sorted((DATA / folder).glob("*.wav")):
            training_hashes.add(sha256(path))
            rows.append(classify_training_file(folder, path))

    camp, review = camp_rows(training_hashes, args.dry_run, report)
    rows.extend(camp)

    # Preserve labeled live clips (`nexus wake review`) and ambient recordings.
    if MANIFEST.exists():
        for line in MANIFEST.read_text(encoding="utf-8").splitlines():
            if line.strip():
                r = json.loads(line)
                if r.get("origin") in ("live", "ambient"):
                    rows.append(r)
                    report[f"{r['origin']}_rows_kept"] += 1

    counts = Counter((r["split"], r["label"], r["origin"]) for r in rows)
    print("split       label       origin       count")
    for (split, label, origin), n in sorted(counts.items()):
        print(f"{split:<11} {label:<11} {origin:<12} {n}")
    for k, v in sorted(report.items()):
        if not isinstance(v, list):
            print(f"{k}: {v}")
    if report.get("camp_excluded_files"):
        print("excluded (same audio as a training file):", *report["camp_excluded_files"][:10], sep="\n  ")
    print(f"needs review (unlabeled): {len(review)}")

    if args.dry_run:
        return
    with MANIFEST.open("w", encoding="utf-8") as f:
        for r in rows:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")
    with NEEDS_REVIEW.open("w", encoding="utf-8") as f:
        for r in review:
            f.write(json.dumps(r) + "\n")
    print(f"wrote {MANIFEST.relative_to(ROOT)} ({len(rows)} rows), {NEEDS_REVIEW.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
