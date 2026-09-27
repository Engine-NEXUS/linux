#!/usr/bin/env python3
"""
Phase 8 — Commerce + Social MCP intents.

Adds training examples for the 3 new MCP-backed intents added to
intent_parser.rs (Phase 3) and routed to Subsystem::Mcp:

  - order_food            (slots: food_item?, restaurant?)
  - search_product        (slots: query)
  - send_whatsapp_message (slots: contact, message)

These intents currently only have deterministic regex coverage. Without
NLU examples, phrasings the regex misses fall to `unknown` and land on
the Worker — these examples give BERT-Mini fallback coverage.

Also adds OOS negatives for plausible near-misses:
  - "order a book" is NOT order_food unless a food/dish word is present
    (book on amazon → search_product territory)
  - "send a message" without whatsapp context stays generic

Output: server/nlu/data/phase8_commerce_social.json
"""
import json
import re
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
OUTPUT_PATH = SCRIPT_DIR / "data" / "phase8_commerce_social.json"

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


DISHES = ["pizza", "biryani", "burger", "dosa", "noodles", "paneer tikka",
          "chicken curry", "sushi", "pasta", "sandwich"]
RESTAURANTS = ["dominos", "meghana foods", "paradise", "mcdonalds",
               "pizza hut", "subway", "empire", "kfc"]
PRODUCTS = ["sony headphones", "wireless earbuds", "laptop", "keyboard",
            "gaming mouse", "monitor", "phone case", "mechanical keyboard",
            "ssd", "webcam"]
CONTACTS = ["mom", "dad", "lakshya", "brother", "sister", "boss",
            "prem", "eesha"]
MESSAGES = ["i'll be late", "on my way", "call me back", "happy birthday",
            "meeting moved to 3", "found it", "almost there", "good night"]


def build_examples():
    examples = []

    # ─── order_food ──────────────────────────────────────────────
    # Family A: "order <dish>" (no restaurant)
    for dish in DISHES:
        examples.append({
            "text": f"order {dish}",
            "intent": "order_food",
            "slots": {"food_item": dish},
        })
    # Family B: "order <dish> from <restaurant>"
    for dish in DISHES[:6]:
        for rest in RESTAURANTS[:4]:
            examples.append({
                "text": f"order {dish} from {rest}",
                "intent": "order_food",
                "slots": {"food_item": dish, "restaurant": rest},
            })
    # Family C: "get me <dish>" / "i want <dish>"
    for dish in DISHES[:6]:
        examples.append({
            "text": f"get me {dish}",
            "intent": "order_food",
            "slots": {"food_item": dish},
        })
    for dish in DISHES[6:]:
        examples.append({
            "text": f"i want to order {dish}",
            "intent": "order_food",
            "slots": {"food_item": dish},
        })
    # Family D: "order food" / "get food from swiggy" (generic, no slots)
    for text in [
        "order food", "order some food", "get food", "get me some food",
        "order food from swiggy", "get food from swiggy",
        "order lunch", "order dinner", "i'm hungry order something",
        "order groceries from instamart",
    ]:
        examples.append({"text": text, "intent": "order_food", "slots": {}})

    # ─── search_product ──────────────────────────────────────────
    # Family A: "search for <product> on amazon"
    for prod in PRODUCTS:
        examples.append({
            "text": f"search for {prod} on amazon",
            "intent": "search_product",
            "slots": {"query": prod},
        })
    # Family B: "find <product> on amazon"
    for prod in PRODUCTS[:6]:
        examples.append({
            "text": f"find {prod} on amazon",
            "intent": "search_product",
            "slots": {"query": prod},
        })
    # Family C: "amazon search for <product>" / "search amazon for <product>"
    for prod in PRODUCTS[:6]:
        examples.append({
            "text": f"amazon search for {prod}",
            "intent": "search_product",
            "slots": {"query": prod},
        })
    for prod in PRODUCTS[6:]:
        examples.append({
            "text": f"search amazon for {prod}",
            "intent": "search_product",
            "slots": {"query": prod},
        })
    # Family D: "look for <product> on amazon" / "check price of <product>"
    for prod in PRODUCTS[:4]:
        examples.append({
            "text": f"look for {prod} on amazon",
            "intent": "search_product",
            "slots": {"query": prod},
        })
        examples.append({
            "text": f"check the price of {prod} on amazon",
            "intent": "search_product",
            "slots": {"query": prod},
        })

    # ─── send_whatsapp_message ───────────────────────────────────
    # Family A: "send <contact> a whatsapp message saying <message>"
    for contact in CONTACTS[:5]:
        for msg in MESSAGES[:4]:
            examples.append({
                "text": f"send {contact} a whatsapp message saying {msg}",
                "intent": "send_whatsapp_message",
                "slots": {"contact": contact, "message": msg},
            })
    # Family B: "whatsapp <contact> saying <message>"
    for contact in CONTACTS[:5]:
        for msg in MESSAGES[4:]:
            examples.append({
                "text": f"whatsapp {contact} saying {msg}",
                "intent": "send_whatsapp_message",
                "slots": {"contact": contact, "message": msg},
            })
    # Family C: "message <contact> on whatsapp saying <message>"
    for contact in CONTACTS[5:]:
        for msg in MESSAGES[:3]:
            examples.append({
                "text": f"message {contact} on whatsapp saying {msg}",
                "intent": "send_whatsapp_message",
                "slots": {"contact": contact, "message": msg},
            })
    # Family D: "send <contact> a message saying <message>" (no whatsapp word)
    for contact in CONTACTS[:4]:
        for msg in MESSAGES[:3]:
            examples.append({
                "text": f"send {contact} a message saying {msg}",
                "intent": "send_whatsapp_message",
                "slots": {"contact": contact, "message": msg},
            })

    # ─── OOS negatives (stay unknown / generic) ──────────────────
    oos_templates = [
        # "order" without food context is NOT order_food
        "order a new book",
        "order the results alphabetically",
        "out of order",
        "what order should i watch the movies in",
        # "send a message" without contact+saying stays unknown
        "how do i send a message on slack",
        "send it back",
        "send the file",
        # "search for" without amazon stays generic search — already covered
        # by 'search' intent examples, add ambiguous ones as unknown:
        "what's on the menu",
        "where can i buy groceries near me",
    ]
    for text in oos_templates:
        examples.append({"text": text, "intent": "unknown", "slots": {}})

    return examples


def verify_no_test_family_overlap(examples, test_rows):
    test_families = {family_key(r) for r in test_rows}
    new_families = set()
    for ex in examples:
        fk = family_key(ex)
        if fk in test_families:
            raise ValueError(f"new example shares family with frozen test: {ex['text']} -> {fk}")
        new_families.add(fk)
    return len(new_families)


def main():
    examples = build_examples()

    with open(SCRIPT_DIR / "dataset.json", "r", encoding="utf-8") as f:
        dataset = json.load(f)
    test_rows = dataset["test"]

    unique_families = verify_no_test_family_overlap(examples, test_rows)

    from collections import Counter
    intent_counts = Counter(e["intent"] for e in examples)

    output = {
        "schema_version": 1,
        "source": "nexus_synthetic_phase8",
        "review_status": "approved_for_candidate_training",
        "policy": "order_food, search_product, send_whatsapp_message intent families for MCP sub-center routing. OOS negatives for near-miss phrasings.",
        "total_examples": len(examples),
        "unique_families": unique_families,
        "intent_counts": dict(intent_counts.most_common()),
        "examples": examples,
    }

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(OUTPUT_PATH, "w", encoding="utf-8") as f:
        json.dump(output, f, indent=2, ensure_ascii=False)

    print(f"Phase 8 commerce+social: {len(examples)} examples, {unique_families} unique families")
    print("Intent counts:")
    for intent, count in intent_counts.most_common():
        print(f"  {intent}: {count}")
    print(f"Output: {OUTPUT_PATH}")
    print("No phrase family overlap with frozen test: verified")


if __name__ == "__main__":
    main()
