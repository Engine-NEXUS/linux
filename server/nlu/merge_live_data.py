#!/usr/bin/env python3
"""
NEXUS NLU — Merge new_examples.json into dataset.json.

Simple merge: load both, combine, deduplicate by (text, intent), write back.
This is a one-time script for adding the live-mode training data.

Usage:
  cd server/nlu
  python merge_live_data.py
"""

import json
from pathlib import Path

DATASET_PATH = Path(__file__).parent / "dataset.json"
NEW_DATA_PATH = Path(__file__).parent / "new_examples.json"

def main():
    # Load existing dataset
    with open(DATASET_PATH, "r", encoding="utf-8") as f:
        dataset = json.load(f)

    # Load new examples
    with open(NEW_DATA_PATH, "r", encoding="utf-8") as f:
        new_data = json.load(f)

    existing_train = dataset.get("train", [])
    existing_test = dataset.get("test", [])
    new_train = new_data.get("train", [])

    print(f"Existing train: {len(existing_train)}")
    print(f"Existing test:  {len(existing_test)}")
    print(f"New examples:   {len(new_train)}")

    # Deduplicate by (text, intent) — keep the first occurrence
    seen = set()
    merged_train = []

    # Add existing first (they have priority)
    for ex in existing_train:
        key = (ex["text"].lower().strip(), ex["intent"])
        if key not in seen:
            seen.add(key)
            merged_train.append(ex)

    # Add new examples
    added = 0
    for ex in new_train:
        key = (ex["text"].lower().strip(), ex["intent"])
        if key not in seen:
            seen.add(key)
            merged_train.append(ex)
            added += 1

    print(f"Actually added (after dedup): {added}")
    print(f"Merged train: {len(merged_train)}")

    # Write merged dataset
    dataset["train"] = merged_train
    with open(DATASET_PATH, "w", encoding="utf-8") as f:
        json.dump(dataset, f, indent=2, ensure_ascii=False)

    print(f"Written to {DATASET_PATH}")

    # Print intent distribution
    from collections import Counter
    counts = Counter(ex["intent"] for ex in merged_train)
    print(f"\nIntent distribution ({len(counts)} intents):")
    for intent, count in sorted(counts.items(), key=lambda x: -x[1]):
        print(f"  {count:4d}  {intent}")

if __name__ == "__main__":
    main()
