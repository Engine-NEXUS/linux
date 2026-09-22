#!/usr/bin/env python3

import argparse
import hashlib
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parent.parent
OWW_DIR = ROOT_DIR / "src-tauri" / "resources" / "oww"
MODEL_MANIFEST_PATH = OWW_DIR / "model_manifest.json"
WAKE_DATA_DIR = ROOT_DIR / "wake_word_data"
AUDIO_MANIFEST_PATH = WAKE_DATA_DIR / "manifest.jsonl"
SCHEMA_VERSION = 1
MODEL_FILES = ("melspectrogram.onnx", "embedding_model.onnx", "nexus.onnx")
REQUIRED_FIELDS = {
    "audio",
    "label",
    "phrase",
    "speaker_id",
    "accent",
    "device_class",
    "environment",
    "distance_cm",
    "volume",
    "session_id",
    "source_group_id",
    "split",
    "consent",
}
LABELS = {"positive", "negative", "background"}
SPLITS = {"train", "validation", "test"}
VOLUMES = {"whisper", "quiet", "normal", "loud"}


def file_sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def fingerprint_models():
    models = []
    for name in MODEL_FILES:
        path = OWW_DIR / name
        if not path.exists():
            raise FileNotFoundError(path)
        models.append({"file": name, "bytes": path.stat().st_size, "sha256": file_sha256(path)})
    return {
        "schema_version": SCHEMA_VERSION,
        "pipeline": "openwakeword-melspectrogram-embedding-classifier",
        "sample_rate_hz": 16000,
        "classifier_input": [1, 16, 96],
        "classifier_output": [1, 1],
        "runtime": {
            "wake_threshold": 0.35,
            "silence_rms_threshold": 0.002,
            "target_rms": 0.03,
            "max_gain": 20.0,
            "restart_grace_seconds": 10,
            "refractory_milliseconds": 3000
        },
        "models": models,
    }


def write_model_manifest():
    manifest = fingerprint_models()
    MODEL_MANIFEST_PATH.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"Fingerprint written: {MODEL_MANIFEST_PATH}")
    for model in manifest["models"]:
        print(f"{model['file']}: {model['bytes']} bytes {model['sha256']}")


def validate_model_manifest():
    errors = []
    if not MODEL_MANIFEST_PATH.exists():
        return [f"missing model manifest: {MODEL_MANIFEST_PATH}"]
    expected = json.loads(MODEL_MANIFEST_PATH.read_text(encoding="utf-8"))
    actual = fingerprint_models()
    for field in ("schema_version", "pipeline", "sample_rate_hz", "classifier_input", "classifier_output", "runtime", "models"):
        if expected.get(field) != actual.get(field):
            errors.append(f"wake model manifest mismatch: {field}")
    return errors


def validate_audio_manifest(require_data):
    errors = []
    counts = Counter()
    if not AUDIO_MANIFEST_PATH.exists():
        if require_data:
            return [f"missing audio manifest: {AUDIO_MANIFEST_PATH}"], counts
        return [], counts
    speakers_by_split = defaultdict(set)
    groups_by_split = defaultdict(set)
    sessions_by_split = defaultdict(set)
    seen_audio = set()
    with AUDIO_MANIFEST_PATH.open("r", encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, 1):
            if not line.strip():
                continue
            location = f"manifest.jsonl:{line_number}"
            try:
                row = json.loads(line)
            except json.JSONDecodeError as error:
                errors.append(f"{location} invalid JSON: {error.msg}")
                continue
            missing = sorted(REQUIRED_FIELDS - set(row))
            if missing:
                errors.append(f"{location} missing fields: {', '.join(missing)}")
                continue
            audio = str(row["audio"]).replace("\\", "/")
            if audio.startswith("/") or ".." in Path(audio).parts:
                errors.append(f"{location} audio path must be relative and contained")
            if audio in seen_audio:
                errors.append(f"{location} duplicates audio path: {audio}")
            seen_audio.add(audio)
            audio_path = WAKE_DATA_DIR / audio
            if not audio_path.is_file():
                errors.append(f"{location} missing audio file: {audio}")
            if row["label"] not in LABELS:
                errors.append(f"{location} invalid label: {row['label']}")
            if row["split"] not in SPLITS:
                errors.append(f"{location} invalid split: {row['split']}")
            if row["volume"] not in VOLUMES:
                errors.append(f"{location} invalid volume: {row['volume']}")
            if row["consent"] is not True:
                errors.append(f"{location} lacks explicit consent")
            if not isinstance(row["distance_cm"], (int, float)) or row["distance_cm"] < 0:
                errors.append(f"{location} has invalid distance_cm")
            speakers_by_split[row["speaker_id"]].add(row["split"])
            groups_by_split[row["source_group_id"]].add(row["split"])
            sessions_by_split[row["session_id"]].add(row["split"])
            counts[row["split"]] += 1
            counts[row["label"]] += 1
    for speaker, splits in speakers_by_split.items():
        if len(splits) > 1:
            errors.append(f"speaker {speaker} spans splits: {sorted(splits)}")
    for group, splits in groups_by_split.items():
        if len(splits) > 1:
            errors.append(f"source_group_id {group} spans splits: {sorted(splits)}")
    for session, splits in sessions_by_split.items():
        if len(splits) > 1:
            errors.append(f"session {session} spans splits: {sorted(splits)}")
    return errors, counts


def validate(require_data):
    errors = validate_model_manifest()
    audio_errors, counts = validate_audio_manifest(require_data)
    errors.extend(audio_errors)
    print(f"Audio manifest: {AUDIO_MANIFEST_PATH}")
    print(f"Audio counts: {dict(sorted(counts.items()))}")
    if errors:
        for error in errors:
            print(f"ERROR: {error}")
        return 1
    print("Wake data and model foundation validation passed")
    return 0


def main():
    parser = argparse.ArgumentParser(description="Fingerprint NEXUS wake models and validate leakage-safe audio manifests")
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("fingerprint-models")
    validate_parser = subparsers.add_parser("validate")
    validate_parser.add_argument("--require-data", action="store_true")
    args = parser.parse_args()
    if args.command == "fingerprint-models":
        write_model_manifest()
        return 0
    return validate(args.require_data)


if __name__ == "__main__":
    sys.exit(main())
