#!/usr/bin/env python3

import argparse
import hashlib
import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
ROOT_DIR = SCRIPT_DIR.parent.parent
DATASET_PATH = SCRIPT_DIR / "dataset.json"
LABELS_PATH = SCRIPT_DIR / "model" / "labels.json"
DATA_DIR = SCRIPT_DIR / "data"
REGISTRY_PATH = DATA_DIR / "source_registry.json"
EVALUATION_LOCK_PATH = DATA_DIR / "evaluation_lock.json"
EXTERNAL_EVALUATION_LOCK_PATH = DATA_DIR / "external_evaluation_lock.json"
SPLIT_LOCK_PATH = DATA_DIR / "split_lock.json"
STAGING_DIR = DATA_DIR / "staging"
SCHEMA_VERSION = 1
REQUIRED_FIELDS = {
    "text",
    "source",
    "source_record_id",
    "license",
    "language",
    "source_intent",
    "mapped_intent",
    "slots",
    "mapping_version",
    "review_status",
    "split_group",
}
REVIEW_STATUSES = {"pending", "approved", "rejected"}


def canonical_json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def sha256_value(value):
    return hashlib.sha256(canonical_json(value).encode("utf-8")).hexdigest()


def normalize_text(text):
    return " ".join(str(text).lower().strip().split())


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def file_sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_labels():
    labels = load_json(LABELS_PATH)
    intents = set(labels["intents"])
    slots = {label[2:] for label in labels["slots"] if label.startswith("B-")}
    return intents, slots


def test_payload(dataset):
    rows = []
    for row in dataset.get("test", []):
        rows.append({
            "text": normalize_text(row.get("text", "")),
            "intent": row.get("intent", ""),
            "slots": row.get("slots", {}),
        })
    return rows


def build_evaluation_lock(dataset):
    rows = test_payload(dataset)
    family_hashes = sorted({hashlib.sha256(row["text"].encode("utf-8")).hexdigest() for row in rows})
    return {
        "schema_version": SCHEMA_VERSION,
        "dataset": str(DATASET_PATH.relative_to(ROOT_DIR)).replace("\\", "/"),
        "test_examples": len(rows),
        "test_sha256": sha256_value(rows),
        "normalized_text_sha256": sha256_value(sorted(row["text"] for row in rows)),
        "family_hashes": family_hashes,
        "policy": "Evaluation rows are immutable during a training experiment. Update this lock only through an explicit reviewed freeze.",
    }


def validate_evaluation_lock(dataset):
    errors = []
    if not EVALUATION_LOCK_PATH.exists():
        return [f"missing evaluation lock: {EVALUATION_LOCK_PATH}"]
    expected = load_json(EVALUATION_LOCK_PATH)
    actual = build_evaluation_lock(dataset)
    for field in ("schema_version", "dataset", "test_examples", "test_sha256", "normalized_text_sha256", "family_hashes"):
        if expected.get(field) != actual.get(field):
            errors.append(f"evaluation lock mismatch: {field}")
    return errors


def validate_split_lock(dataset):
    if not SPLIT_LOCK_PATH.exists():
        return [f"missing split lock: {SPLIT_LOCK_PATH}"]
    from prepare_evaluation_splits import SPLIT_NAMES, build_lock, validate_splits

    splits = {**{name: dataset.get(name, []) for name in SPLIT_NAMES}, "quarantine": dataset.get("quarantine", [])}
    errors = validate_splits(splits)
    expected = load_json(SPLIT_LOCK_PATH)
    actual = build_lock(splits)
    if expected != actual:
        errors.append("split lock does not match dataset splits")
    return errors


def validate_external_evaluation():
    if not EXTERNAL_EVALUATION_LOCK_PATH.exists():
        return []
    errors = []
    lock = load_json(EXTERNAL_EVALUATION_LOCK_PATH)
    for benchmark in lock.get("benchmarks", []):
        path = ROOT_DIR / benchmark.get("path", "")
        if not benchmark.get("never_train"):
            errors.append(f"external benchmark {benchmark.get('id')} is not marked never_train")
            continue
        if not path.is_file():
            errors.append(f"missing external benchmark: {path}")
            continue
        lines = sum(1 for line in path.read_text(encoding="utf-8").splitlines() if line.strip())
        if lines != benchmark.get("rows"):
            errors.append(f"external benchmark row mismatch: {benchmark.get('id')}")
        if file_sha256(path) != benchmark.get("sha256"):
            errors.append(f"external benchmark hash mismatch: {benchmark.get('id')}")
    return errors


def validate_registry():
    errors = []
    if not REGISTRY_PATH.exists():
        return [f"missing source registry: {REGISTRY_PATH}"]
    registry = load_json(REGISTRY_PATH)
    if registry.get("schema_version") != SCHEMA_VERSION:
        errors.append("source registry schema_version is unsupported")
    source_ids = set()
    for index, source in enumerate(registry.get("sources", [])):
        source_id = source.get("id", "")
        if not re.fullmatch(r"[a-z][a-z0-9_]*", source_id):
            errors.append(f"source[{index}] has invalid id")
        if source_id in source_ids:
            errors.append(f"duplicate source id: {source_id}")
        source_ids.add(source_id)
        if not source.get("license") or not source.get("allowed_use"):
            errors.append(f"source {source_id or index} lacks license or allowed_use")
    return errors, source_ids


def iter_staging_rows():
    if not STAGING_DIR.exists():
        return
    for path in sorted(STAGING_DIR.glob("*.jsonl")):
        if path.name == "clinc150_oos_train.jsonl" and (STAGING_DIR / "clinc150_oos_train_reviewed.jsonl").exists():
            continue
        with path.open("r", encoding="utf-8") as handle:
            for line_number, line in enumerate(handle, 1):
                if line.strip():
                    yield path, line_number, json.loads(line)


def validate_staging(source_ids, intents, allowed_slots, dataset):
    errors = []
    counts = Counter()
    seen_record_ids = set()
    train_texts = {normalize_text(row.get("text", "")) for row in dataset.get("train", [])}
    evaluation_texts = {
        normalize_text(row.get("text", ""))
        for split in ("validation", "calibration", "test")
        for row in dataset.get(split, [])
    }
    groups_by_split = defaultdict(set)
    for path, line_number, row in iter_staging_rows() or []:
        location = f"{path.name}:{line_number}"
        missing = sorted(REQUIRED_FIELDS - set(row))
        if missing:
            errors.append(f"{location} missing fields: {', '.join(missing)}")
            continue
        text = normalize_text(row["text"])
        if not text:
            errors.append(f"{location} has empty text")
        if row["source"] not in source_ids:
            errors.append(f"{location} references unknown source: {row['source']}")
        record_key = (row["source"], str(row["source_record_id"]))
        if record_key in seen_record_ids:
            errors.append(f"{location} duplicates source record id")
        seen_record_ids.add(record_key)
        if row["mapped_intent"] not in intents:
            errors.append(f"{location} has unknown mapped intent: {row['mapped_intent']}")
        if row["review_status"] not in REVIEW_STATUSES:
            errors.append(f"{location} has invalid review_status")
        if not isinstance(row["slots"], dict):
            errors.append(f"{location} slots must be an object")
        else:
            unknown_slots = sorted(set(row["slots"]) - allowed_slots)
            if unknown_slots:
                errors.append(f"{location} has unknown slots: {', '.join(unknown_slots)}")
        if not row["split_group"]:
            errors.append(f"{location} has empty split_group")
        split = row.get("split", "train")
        groups_by_split[row["split_group"]].add(split)
        if row["review_status"] == "approved" and text in evaluation_texts:
            errors.append(f"{location} approved row leaks into validation, calibration, or final-test text")
        if row["review_status"] == "approved" and text in train_texts:
            counts["existing_train_text"] += 1
        counts[row["review_status"]] += 1
    for group, splits in groups_by_split.items():
        if len(splits) > 1:
            errors.append(f"split_group {group} spans splits: {sorted(splits)}")
    return errors, counts


def freeze_evaluation():
    dataset = load_json(DATASET_PATH)
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    lock = build_evaluation_lock(dataset)
    EVALUATION_LOCK_PATH.write_text(json.dumps(lock, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"Frozen {lock['test_examples']} evaluation rows: {lock['test_sha256']}")


def validate():
    dataset = load_json(DATASET_PATH)
    intents, slots = load_labels()
    registry_result = validate_registry()
    registry_errors, source_ids = registry_result if isinstance(registry_result, tuple) else (registry_result, set())
    lock_errors = validate_evaluation_lock(dataset)
    external_errors = validate_external_evaluation()
    split_errors = validate_split_lock(dataset)
    staging_errors, counts = validate_staging(source_ids, intents, slots, dataset)
    errors = registry_errors + lock_errors + external_errors + split_errors + staging_errors
    print(f"Evaluation rows: {len(dataset.get('test', []))}")
    print(f"Registered sources: {len(source_ids)}")
    print(f"Staging rows: {sum(counts.values()) - counts['existing_train_text']}")
    print(f"Staging review counts: {dict(sorted((k, v) for k, v in counts.items() if k != 'existing_train_text'))}")
    if errors:
        for error in errors:
            print(f"ERROR: {error}")
        return 1
    print("NLU data foundation validation passed")
    return 0


def main():
    parser = argparse.ArgumentParser(description="Validate NEXUS NLU provenance and frozen evaluation controls")
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("validate")
    subparsers.add_parser("freeze-evaluation")
    args = parser.parse_args()
    if args.command == "freeze-evaluation":
        freeze_evaluation()
        return 0
    return validate()


if __name__ == "__main__":
    sys.exit(main())
