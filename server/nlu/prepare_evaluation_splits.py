#!/usr/bin/env python3

import argparse
import hashlib
import json
import re
from collections import Counter, defaultdict
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
DATASET_PATH = SCRIPT_DIR / "dataset.json"
LOCK_PATH = SCRIPT_DIR / "data" / "split_lock.json"
READINESS_PATH = SCRIPT_DIR / "data" / "split_readiness.json"
SPLIT_NAMES = ("train", "validation", "calibration", "test")
FILLERS = {"please", "could", "would", "you", "kindly", "can"}


def normalize(text):
    return " ".join(str(text).lower().strip().split())


def family_text(row):
    text = normalize(row.get("text", ""))
    values = []
    for slot, value in sorted((row.get("slots") or {}).items()):
        slot_values = value if isinstance(value, list) else [value]
        for item in slot_values:
            item_text = normalize(item)
            if item_text:
                values.append((len(item_text), item_text, f"<{slot}>"))
    for _, value, replacement in sorted(values, reverse=True):
        text = text.replace(value, replacement)
    tokens = re.findall(r"<[a-z_]+>|[a-z]+|\d+", text)
    while tokens and tokens[0] in FILLERS:
        tokens.pop(0)
    tokens = ["<number>" if token.isdigit() else token for token in tokens]
    return " ".join(tokens)


def family_key(row):
    return f"{row.get('intent', '')}|{family_text(row)}"


def row_key(row):
    return json.dumps({
        "text": normalize(row.get("text", "")),
        "intent": row.get("intent", ""),
        "slots": row.get("slots", {}),
    }, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def split_hash(rows):
    payload = "\n".join(row_key(row) for row in rows)
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def choose_groups(rows, target_count, excluded_families):
    groups = defaultdict(list)
    for row in rows:
        groups[family_key(row)].append(row)
    candidates = [
        (hashlib.sha256(key.encode("utf-8")).hexdigest(), key, values)
        for key, values in groups.items()
        if key not in excluded_families
    ]
    selected = []
    selected_families = set()
    for _, key, values in sorted(candidates):
        if len(selected) >= target_count:
            break
        selected.extend(values)
        selected_families.add(key)
    return selected, selected_families


def readiness_report(dataset):
    test_families = {family_key(row) for row in dataset.get("test", [])}
    by_intent = defaultdict(list)
    for row in dataset.get("train", []):
        by_intent[row["intent"]].append(row)
    intents = {}
    for intent, rows in sorted(by_intent.items()):
        families = {family_key(row) for row in rows}
        available = families - test_families
        target = max(5, round(len(rows) * 0.1))
        intents[intent] = {
            "train_rows": len(rows),
            "train_families": len(families),
            "families_overlapping_test": len(families & test_families),
            "families_available_for_validation_and_calibration": len(available),
            "target_rows_per_split": target,
            "ready": len(available) >= 10,
        }
    blocked = [intent for intent, result in intents.items() if not result["ready"]]
    return {
        "schema_version": 1,
        "family_algorithm": "intent-plus-slot-masked-normalized-text-v1",
        "frozen_test_rows": len(dataset.get("test", [])),
        "frozen_test_families": len(test_families),
        "ready_intents": len(intents) - len(blocked),
        "defined_intents": len(intents),
        "blocked_intents": blocked,
        "intents": intents,
    }


def build_splits(dataset):
    if dataset.get("validation") or dataset.get("calibration"):
        return {**{name: list(dataset.get(name, [])) for name in SPLIT_NAMES}, "quarantine": list(dataset.get("quarantine", []))}
    original_train = list(dataset.get("train", []))
    test = list(dataset.get("test", []))
    test_families = {family_key(row) for row in test}
    by_intent = defaultdict(list)
    for row in original_train:
        by_intent[row["intent"]].append(row)
    validation = []
    calibration = []
    selected_families = set()
    for intent, rows in sorted(by_intent.items()):
        target = max(5, round(len(rows) * 0.1))
        available_families = {family_key(row) for row in rows} - test_families
        if len(available_families) < 2:
            raise ValueError(f"intent {intent} lacks two non-test phrase families")
        val_rows, val_families = choose_groups(rows, target, test_families | selected_families)
        cal_rows, cal_families = choose_groups(rows, target, test_families | selected_families | val_families)
        if not val_rows or not cal_rows:
            raise ValueError(f"intent {intent} cannot populate validation and calibration")
        validation.extend(val_rows)
        calibration.extend(cal_rows)
        selected_families.update(val_families | cal_families)
    selected_rows = {row_key(row) for row in validation + calibration}
    quarantine = [row for row in original_train if family_key(row) in test_families]
    train = [
        row for row in original_train
        if row_key(row) not in selected_rows and family_key(row) not in test_families
    ]
    return {"train": train, "validation": validation, "calibration": calibration, "test": test, "quarantine": quarantine}


def validate_splits(splits):
    errors = []
    texts_by_split = {}
    families_by_split = {}
    rows_by_split = {}
    for name in SPLIT_NAMES:
        rows = splits.get(name, [])
        rows_by_split[name] = {row_key(row) for row in rows}
        texts_by_split[name] = {normalize(row.get("text", "")) for row in rows}
        families_by_split[name] = {family_key(row) for row in rows}
        if not rows:
            errors.append(f"split {name} is empty")
    for index, left in enumerate(SPLIT_NAMES):
        for right in SPLIT_NAMES[index + 1:]:
            row_overlap = rows_by_split[left] & rows_by_split[right]
            text_overlap = texts_by_split[left] & texts_by_split[right]
            family_overlap = families_by_split[left] & families_by_split[right]
            if row_overlap:
                errors.append(f"{left}/{right} exact-row overlap: {len(row_overlap)}")
            if text_overlap:
                errors.append(f"{left}/{right} normalized-text overlap: {len(text_overlap)}")
            if family_overlap:
                errors.append(f"{left}/{right} phrase-family overlap: {len(family_overlap)}")
    intents = {row["intent"] for rows in splits.values() for row in rows}
    for name in SPLIT_NAMES:
        counts = Counter(row["intent"] for row in splits[name])
        missing = sorted(intents - set(counts))
        if missing:
            errors.append(f"split {name} missing intents: {missing}")
    return errors


def build_lock(splits):
    return {
        "schema_version": 1,
        "family_algorithm": "intent-plus-slot-masked-normalized-text-v1",
        "splits": {
            name: {
                "rows": len(splits[name]),
                "sha256": split_hash(splits[name]),
                "families": len({family_key(row) for row in splits[name]}),
                "intents": len({row["intent"] for row in splits[name]}),
            }
            for name in SPLIT_NAMES
        },
        "quarantine": {
            "rows": len(splits.get("quarantine", [])),
            "sha256": split_hash(splits.get("quarantine", [])),
            "reason": "Phrase family overlaps frozen final test; excluded from optimization and model selection"
        },
    }


def main():
    parser = argparse.ArgumentParser(description="Create deterministic phrase-family-separated NLU splits")
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    dataset = json.loads(DATASET_PATH.read_text(encoding="utf-8"))
    if not dataset.get("validation") and not dataset.get("calibration"):
        readiness = readiness_report(dataset)
        READINESS_PATH.parent.mkdir(parents=True, exist_ok=True)
        READINESS_PATH.write_text(json.dumps(readiness, indent=2) + "\n", encoding="utf-8")
        if readiness["blocked_intents"]:
            print(json.dumps(readiness, indent=2))
            print(f"ERROR: {len(readiness['blocked_intents'])} intents lack ten phrase families independent from the frozen test set")
            print(f"Readiness report: {READINESS_PATH}")
            raise SystemExit(2)
    splits = build_splits(dataset)
    errors = validate_splits(splits)
    lock = build_lock(splits)
    print(json.dumps(lock, indent=2))
    if errors:
        for error in errors:
            print(f"ERROR: {error}")
        raise SystemExit(1)
    if args.apply:
        output = dict(dataset)
        output.update(splits)
        DATASET_PATH.write_text(json.dumps(output, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        LOCK_PATH.parent.mkdir(parents=True, exist_ok=True)
        LOCK_PATH.write_text(json.dumps(lock, indent=2) + "\n", encoding="utf-8")
        print(f"Wrote {DATASET_PATH} and {LOCK_PATH}")
    else:
        print("Dry run only. Re-run with --apply after review.")


if __name__ == "__main__":
    main()
