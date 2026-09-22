#!/usr/bin/env python3
"""
Phase 9 — MCP verb alternates + STT confusables.

Expands the 3 MCP intents (order_food, search_product, send_whatsapp_message)
along the two axes the gap sweep flagged:

  Tier 1 (verb alternates): craving/fetch/bring/hungry, check price/buy/hunt
    down, tell/text/ping/forward — forms the deterministic regex misses.
  Tier 2 (STT confusables): swigy/dominose/amazone/whatsup/mommy — labeled
    with the CORRECT intent so the model learns through mishearings.
  Extra slot values: more Indian dishes (vada pav, chole, thali, idli,
    samosa), more contacts.

No new labels (stays 55 intents) — no train.py/nlu_server.py sync needed.
OOS negatives for near-misses.

Output: server/nlu/data/phase9_mcp_verbs.json
"""
import json
import re
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
OUTPUT_PATH = SCRIPT_DIR / "data" / "phase9_mcp_verbs.json"

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


DISHES = ["vada pav", "chole bhature", "thali", "idli", "samosa",
          "fried rice", "hakka noodles", "tacos", "shawarma", "cake"]
RESTAURANTS = ["behrouz", "faasos", "oven story", "mandar", "a2b"]
PRODUCTS = ["power bank", "earphones", "backpack", "water bottle",
            "desk lamp", "notebook"]
CONTACTS = ["amma", "nanna", "akka", "anna", "thanmayee", "lakshya"]
MESSAGES = ["reached home", "starting now", "happy diwali", "all the best",
            "take care", "call when free"]


def build_examples():
    examples = []

    # ─── order_food: new verbs ──────────────────────────────────
    for dish in DISHES:
        examples.append({"text": f"i want {dish}", "intent": "order_food",
                         "slots": {"food_item": dish}})
        examples.append({"text": f"get me {dish}", "intent": "order_food",
                         "slots": {"food_item": dish}})
        examples.append({"text": f"i am craving {dish}", "intent": "order_food",
                         "slots": {"food_item": dish}})
        examples.append({"text": f"bring me {dish}", "intent": "order_food",
                         "slots": {"food_item": dish}})
    for dish in DISHES[:5]:
        for rest in RESTAURANTS[:3]:
            examples.append({"text": f"fetch {dish} from {rest}",
                             "intent": "order_food",
                             "slots": {"food_item": dish, "restaurant": rest}})
    # confusables (correct label despite mishearing)
    for bad, good in [("swigy", "swiggy"), ("swigi", "swiggy")]:
        examples.append({"text": f"order food from {bad}", "intent": "order_food",
                         "slots": {"restaurant": good}})
    for bad in ["dominose", "pizzahut"]:
        examples.append({"text": f"order pizza from {bad}", "intent": "order_food",
                         "slots": {"food_item": "pizza", "restaurant": bad}})

    # ─── search_product: new verbs ──────────────────────────────
    for prod in PRODUCTS:
        examples.append({"text": f"check the price of {prod} on amazon",
                         "intent": "search_product", "slots": {"query": prod}})
        examples.append({"text": f"buy {prod} on amazon",
                         "intent": "search_product", "slots": {"query": prod}})
        examples.append({"text": f"look up {prod} on amazon",
                         "intent": "search_product", "slots": {"query": prod}})
    for prod in PRODUCTS[:3]:
        examples.append({"text": f"hunt down {prod} on amazon",
                         "intent": "search_product", "slots": {"query": prod}})
    for bad in ["amazone", "amazan"]:
        examples.append({"text": f"search for laptop on {bad}",
                         "intent": "search_product", "slots": {"query": "laptop"}})

    # ─── send_whatsapp_message: new verbs ───────────────────────
    for contact in CONTACTS:
        for msg in MESSAGES[:3]:
            examples.append({"text": f"tell {contact} {msg} on whatsapp",
                             "intent": "send_whatsapp_message",
                             "slots": {"contact": contact, "message": msg}})
            examples.append({"text": f"text {contact} saying {msg}",
                             "intent": "send_whatsapp_message",
                             "slots": {"contact": contact, "message": msg}})
    for contact in CONTACTS[:3]:
        for msg in MESSAGES[3:]:
            examples.append({"text": f"ping {contact} saying {msg}",
                             "intent": "send_whatsapp_message",
                             "slots": {"contact": contact, "message": msg}})
            examples.append({"text": f"forward this to {contact} saying {msg}",
                             "intent": "send_whatsapp_message",
                             "slots": {"contact": contact, "message": msg}})
    # confusables
    for bad in ["whatsup", "watsapp", "vatsap"]:
        examples.append({"text": f"send mom a {bad} message saying hello",
                         "intent": "send_whatsapp_message",
                         "slots": {"contact": "mom", "message": "hello"}})
    for bad, good in [("mommy", "mom"), ("momy", "mom")]:
        examples.append({"text": f"whatsapp {bad} saying on my way",
                         "intent": "send_whatsapp_message",
                         "slots": {"contact": good, "message": "on my way"}})

    # ─── OOS negatives ──────────────────────────────────────────
    for text in [
        "order a book on amazon",
        "send the file to rahul",
        "what is on the tv menu tonight",
        "forward the mail to accounts",
        "check the price of bitcoin",
    ]:
        examples.append({"text": text, "intent": "unknown", "slots": {}})

    return examples


def main():
    examples = build_examples()

    with open(SCRIPT_DIR / "dataset.json", "r", encoding="utf-8") as f:
        dataset = json.load(f)
    test_rows = dataset["test"]
    cand = json.load(open(SCRIPT_DIR / "data" / "candidate_dataset.json"))
    seen = {normalize(r["text"]) for s in ("train", "validation", "calibration", "test")
            for r in cand[s]}
    test_families = {family_key(r) for r in test_rows}

    fresh = []
    for ex in examples:
        if normalize(ex["text"]) in seen:
            continue
        if family_key(ex) in test_families:
            raise ValueError(f"shares family with frozen test: {ex['text']}")
        seen.add(normalize(ex["text"]))
        fresh.append(ex)

    from collections import Counter
    intent_counts = Counter(e["intent"] for e in fresh)

    output = {
        "schema_version": 1,
        "source": "nexus_synthetic_phase9",
        "review_status": "approved_for_candidate_training",
        "policy": "MCP verb alternates + STT confusables for order_food, search_product, send_whatsapp_message. No new labels.",
        "total_examples": len(fresh),
        "unique_families": len({family_key(e) for e in fresh}),
        "intent_counts": dict(intent_counts.most_common()),
        "examples": fresh,
    }

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(OUTPUT_PATH, "w", encoding="utf-8") as f:
        json.dump(output, f, indent=2, ensure_ascii=False)

    print(f"Phase 9 MCP verbs: {len(fresh)} fresh examples ({len(examples) - len(fresh)} dupes skipped)")
    for intent, count in intent_counts.most_common():
        print(f"  {intent}: {count}")
    print(f"Output: {OUTPUT_PATH}")


if __name__ == "__main__":
    main()
