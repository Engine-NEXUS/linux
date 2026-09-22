#!/usr/bin/env python3
"""
NEXUS NLU — Promote MCP Intents into dataset.json with Strict Evaluation Locks

Promotes `order_food`, `search_product`, and `send_whatsapp_message` into
`dataset.json` with cleanly isolated, non-overlapping phrase families across
`train`, `validation`, `calibration`, and `test` splits.

Updates:
  - server/nlu/dataset.json
  - server/nlu/data/split_lock.json
  - server/nlu/data/evaluation_lock.json
"""

import json
import re
import sys
from collections import defaultdict, Counter
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
DATASET_PATH = SCRIPT_DIR / "dataset.json"
SPLIT_LOCK_PATH = SCRIPT_DIR / "data" / "split_lock.json"
EVAL_LOCK_PATH = SCRIPT_DIR / "data" / "evaluation_lock.json"
HOLDING_PATH = SCRIPT_DIR / "data" / "phase11_new_intents_holding.jsonl"
PHASE8_PATH = SCRIPT_DIR / "data" / "phase8_commerce_social.json"

sys.path.insert(0, str(SCRIPT_DIR))
from prepare_evaluation_splits import (
    family_key,
    normalize,
    SPLIT_NAMES,
    validate_splits,
    build_lock,
)
from data_foundation import build_evaluation_lock, validate as validate_foundation

DISHES = [
    "pizza", "biryani", "burger", "dosa", "noodles", "paneer tikka",
    "chicken curry", "sushi", "pasta", "sandwich", "salad", "fried rice",
    "tacos", "ice cream", "coffee", "thali", "soup", "dim sum"
]
RESTAURANTS = [
    "dominos", "meghana foods", "paradise", "mcdonalds", "pizza hut",
    "subway", "empire", "kfc", "behrouz biryani", "starbucks", "taco bell",
    "barbeque nation"
]
PRODUCTS = [
    "sony headphones", "wireless earbuds", "laptop", "keyboard", "gaming mouse",
    "monitor", "phone case", "mechanical keyboard", "ssd", "webcam",
    "power bank", "usb cable", "charger", "desk lamp", "backpack",
    "smart watch", "tablet", "hard drive"
]
CONTACTS = [
    "mom", "dad", "lakshya", "brother", "sister", "boss", "prem", "eesha",
    "rahul", "priya", "amit", "neha"
]
MESSAGES = [
    "i'll be late", "on my way", "call me back", "happy birthday",
    "meeting moved to 3", "found it", "almost there", "good night",
    "see you soon", "got the files", "where are you", "thank you"
]


def generate_synthetic_mcp_examples():
    examples = []

    # ─── order_food ──────────────────────────────────────────────
    for d in DISHES[:8]:
        examples.append({"text": f"order {d}", "intent": "order_food", "slots": {"food_item": d}})
    for d in DISHES[:6]:
        for r in RESTAURANTS[:3]:
            examples.append({"text": f"order {d} from {r}", "intent": "order_food", "slots": {"food_item": d, "restaurant": r}})
    for d in DISHES[:6]:
        examples.append({"text": f"get me {d}", "intent": "order_food", "slots": {"food_item": d}})
    for d in DISHES[:6]:
        for r in RESTAURANTS[:2]:
            examples.append({"text": f"get {d} from {r}", "intent": "order_food", "slots": {"food_item": d, "restaurant": r}})
    for d in DISHES[6:12]:
        examples.append({"text": f"i want to order {d}", "intent": "order_food", "slots": {"food_item": d}})
    for d in DISHES[6:12]:
        examples.append({"text": f"i am craving {d}", "intent": "order_food", "slots": {"food_item": d}})
    for d in DISHES[6:10]:
        for r in RESTAURANTS[:2]:
            examples.append({"text": f"i want {d} from {r}", "intent": "order_food", "slots": {"food_item": d, "restaurant": r}})
    for d in DISHES[:6]:
        examples.append({"text": f"bring me {d}", "intent": "order_food", "slots": {"food_item": d}})
    for d in DISHES[:4]:
        for r in RESTAURANTS[:2]:
            examples.append({"text": f"bring me {d} from {r}", "intent": "order_food", "slots": {"food_item": d, "restaurant": r}})
    for d in DISHES[10:14]:
        examples.append({"text": f"fetch {d}", "intent": "order_food", "slots": {"food_item": d}})
    for text in [
        "order food", "order some food", "get food", "get me some food",
        "order food from swiggy", "get food from swiggy", "order lunch",
        "order dinner", "order breakfast", "order groceries from instamart"
    ]:
        examples.append({"text": text, "intent": "order_food", "slots": {}})

    # ─── search_product ──────────────────────────────────────────
    for p in PRODUCTS[:8]:
        examples.append({"text": f"search for {p} on amazon", "intent": "search_product", "slots": {"query": p}})
    for p in PRODUCTS[:6]:
        examples.append({"text": f"find {p} on amazon", "intent": "search_product", "slots": {"query": p}})
    for p in PRODUCTS[:6]:
        examples.append({"text": f"amazon search for {p}", "intent": "search_product", "slots": {"query": p}})
    for p in PRODUCTS[6:12]:
        examples.append({"text": f"search amazon for {p}", "intent": "search_product", "slots": {"query": p}})
    for p in PRODUCTS[:6]:
        examples.append({"text": f"look for {p} on amazon", "intent": "search_product", "slots": {"query": p}})
    for p in PRODUCTS[:6]:
        examples.append({"text": f"check the price of {p} on amazon", "intent": "search_product", "slots": {"query": p}})
    for p in PRODUCTS[6:12]:
        examples.append({"text": f"buy {p} on amazon", "intent": "search_product", "slots": {"query": p}})
    for p in PRODUCTS[6:10]:
        examples.append({"text": f"shop for {p} on amazon", "intent": "search_product", "slots": {"query": p}})
    for p in PRODUCTS[10:14]:
        examples.append({"text": f"find price of {p} on amazon", "intent": "search_product", "slots": {"query": p}})
    for p in PRODUCTS[10:14]:
        examples.append({"text": f"look up {p} on amazon", "intent": "search_product", "slots": {"query": p}})
    for p in PRODUCTS[12:16]:
        examples.append({"text": f"find me {p} on amazon", "intent": "search_product", "slots": {"query": p}})

    # ─── send_whatsapp_message ───────────────────────────────────
    for c in CONTACTS[:4]:
        for m in MESSAGES[:3]:
            examples.append({"text": f"send {c} a whatsapp message saying {m}", "intent": "send_whatsapp_message", "slots": {"contact": c, "message": m}})
    for c in CONTACTS[4:8]:
        for m in MESSAGES[:3]:
            examples.append({"text": f"whatsapp {c} saying {m}", "intent": "send_whatsapp_message", "slots": {"contact": c, "message": m}})
    for c in CONTACTS[:4]:
        for m in MESSAGES[3:6]:
            examples.append({"text": f"message {c} on whatsapp saying {m}", "intent": "send_whatsapp_message", "slots": {"contact": c, "message": m}})
    for c in CONTACTS[4:8]:
        for m in MESSAGES[3:6]:
            examples.append({"text": f"text {c} on whatsapp saying {m}", "intent": "send_whatsapp_message", "slots": {"contact": c, "message": m}})
    for c in CONTACTS[:4]:
        for m in MESSAGES[6:9]:
            examples.append({"text": f"tell {c} on whatsapp saying {m}", "intent": "send_whatsapp_message", "slots": {"contact": c, "message": m}})
    for c in CONTACTS[8:12]:
        for m in MESSAGES[:3]:
            examples.append({"text": f"ping {c} on whatsapp saying {m}", "intent": "send_whatsapp_message", "slots": {"contact": c, "message": m}})
    for c in CONTACTS[:4]:
        for m in MESSAGES[9:12]:
            examples.append({"text": f"send a whatsapp message to {c} saying {m}", "intent": "send_whatsapp_message", "slots": {"contact": c, "message": m}})
    for c in CONTACTS[4:8]:
        for m in MESSAGES[6:9]:
            examples.append({"text": f"send a message to {c} on whatsapp saying {m}", "intent": "send_whatsapp_message", "slots": {"contact": c, "message": m}})
    for c in CONTACTS[8:12]:
        for m in MESSAGES[3:6]:
            examples.append({"text": f"drop {c} a message on whatsapp saying {m}", "intent": "send_whatsapp_message", "slots": {"contact": c, "message": m}})
    for c in CONTACTS[:4]:
        for m in MESSAGES[:3]:
            examples.append({"text": f"send {c} a message saying {m}", "intent": "send_whatsapp_message", "slots": {"contact": c, "message": m}})

    return examples


def main():
    print("Promoting MCP intents into dataset.json...")

    with open(DATASET_PATH, "r", encoding="utf-8") as f:
        dataset = json.load(f)

    # 1. Gather all candidates
    synthetic = generate_synthetic_mcp_examples()

    holding_rows = []
    if HOLDING_PATH.exists():
        for line in HOLDING_PATH.read_text(encoding="utf-8").splitlines():
            if line.strip():
                try:
                    holding_rows.append(json.loads(line))
                except json.JSONDecodeError:
                    pass

    intents_to_promote = ["order_food", "search_product", "send_whatsapp_message"]
    all_candidates = synthetic + [r for r in holding_rows if r.get("intent") in intents_to_promote]

    # Deduplicate
    seen_texts = set()
    unique_candidates = []
    for r in all_candidates:
        t = normalize(r.get("text", ""))
        it = r.get("intent", "")
        k = (t, it)
        if k not in seen_texts:
            seen_texts.add(k)
            unique_candidates.append(r)

    by_intent_family = defaultdict(lambda: defaultdict(list))
    for r in unique_candidates:
        it = r.get("intent")
        if it in intents_to_promote:
            fk = family_key(r)
            by_intent_family[it][fk].append(r)

    # 2. Allocate non-overlapping families
    splits_added = {"test": [], "validation": [], "calibration": [], "train": []}

    for it in sorted(intents_to_promote):
        fams = sorted(by_intent_family[it].keys())
        print(f"  {it}: {len(fams)} families, {sum(len(by_intent_family[it][f]) for f in fams)} rows")

        # Pick 2 families for test, 2 for validation, 2 for calibration, remainder for train
        test_fams = fams[:2]
        val_fams = fams[2:4]
        cal_fams = fams[4:6]
        train_fams = fams[6:]

        for f in test_fams:
            splits_added["test"].extend(by_intent_family[it][f])
        for f in val_fams:
            splits_added["validation"].extend(by_intent_family[it][f])
        for f in cal_fams:
            splits_added["calibration"].extend(by_intent_family[it][f])
        for f in train_fams:
            splits_added["train"].extend(by_intent_family[it][f])

    print("  Added counts by split:", {k: len(v) for k, v in splits_added.items()})

    # 3. Construct new dataset
    new_dataset = {}
    for split in SPLIT_NAMES:
        # Keep existing rows for other intents
        existing = [r for r in dataset.get(split, []) if r.get("intent") not in intents_to_promote]
        new_dataset[split] = existing + splits_added[split]

    new_dataset["quarantine"] = list(dataset.get("quarantine", []))

    # 4. Validate split isolation
    errors = validate_splits(new_dataset)
    if errors:
        print(f"ERROR: Split validation failed: {errors}")
        sys.exit(1)

    print("  Split validation passed (zero phrase-family cross-leakage)")

    # 5. Write dataset.json
    with open(DATASET_PATH, "w", encoding="utf-8") as f:
        json.dump(new_dataset, f, indent=2, ensure_ascii=False)
    print(f"  Saved {DATASET_PATH}")

    # 6. Update split_lock.json
    lock = build_lock(new_dataset)
    with open(SPLIT_LOCK_PATH, "w", encoding="utf-8") as f:
        json.dump(lock, f, indent=2, ensure_ascii=False)
    print(f"  Updated {SPLIT_LOCK_PATH}")

    # 7. Update evaluation_lock.json
    eval_lock = build_evaluation_lock(new_dataset)
    with open(EVAL_LOCK_PATH, "w", encoding="utf-8") as f:
        json.dump(eval_lock, f, indent=2, ensure_ascii=False)
    print(f"  Updated {EVAL_LOCK_PATH}")

    # 8. Run data_foundation validation
    rc = validate_foundation()
    if rc != 0:
        print("ERROR: data_foundation validation failed")
        sys.exit(rc)

    print("[OK] MCP intents successfully promoted into dataset.json with full evaluation lock!")


if __name__ == "__main__":
    main()
