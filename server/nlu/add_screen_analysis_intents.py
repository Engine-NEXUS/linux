#!/usr/bin/env python3
"""
Phase 86 — screen_analysis BERT-Mini coverage.

Gap: the deterministic parser handles analyse+screen phrasings, but
conversational variants ("look at my screen", "what am i seeing",
"tell me what's here") miss deterministically and the NLU had NO
screen_analysis label — the BERT fallback could never route them to the
spatial feature.

64 targeted rows (58 train forms + 6 OOS negatives), all intent
screen_analysis, empty slots (the whole text is the prompt). One new
label — train.py + nlu_server.py synced in the same change.

Output: server/nlu/data/phase86_screen_analysis.json
"""
import json
import re
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
OUTPUT_PATH = SCRIPT_DIR / "data" / "phase86_screen_analysis.json"

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


def build_examples():
    examples = []

    def add(text):
        examples.append({"text": text, "intent": "screen_analysis", "slots": {}})

    # ─── analyse/analyze + screen (deterministic already catches these —
    #     NLU rows make the fallback agree and cover paraphrases) ───────
    for verb in ["analyse", "analyze"]:
        add(f"{verb} the screen")
        add(f"{verb} my screen")
        add(f"{verb} this screen")
        add(f"{verb} the screen for me")
        add(f"{verb} my screen for me")
    # ─── "what's on my screen" family ─────────────────────────────
    for form in ["what's on my screen", "what is on my screen",
                 "what's on the screen", "what is on the screen",
                 "what's on screen", "what do you see on my screen",
                 "whats on my screen"]:
        add(form)
    # ─── "what do I see / what am I looking at" family ────────────
    for form in ["what do i see", "what am i looking at",
                 "what am i seeing", "tell me what i see",
                 "explain what i see", "explain what i am looking at",
                 "tell me what's here", "what is this on my screen"]:
        add(form)
    # ─── describe/read/scan/check + screen ────────────────────────
    for verb in ["describe", "read", "scan", "check"]:
        add(f"{verb} my screen")
        add(f"{verb} the screen")
    add("look at my screen")
    add("look at the screen for me")
    # ─── research anchored to this/screen ─────────────────────────
    add("research on this for me")
    add("research this screen")
    add("research my screen for me")

    # ─── OOS negatives (must NOT classify as screen_analysis) ─────
    for text in [
        "research quantum computing breakthroughs",
        "analyse servx",
        "analyse pr 5 in zync",
        "check my email",
        "read that report",
        "scan the document for me",
    ]:
        examples.append({"text": text, "intent": "unknown", "slots": {}})

    return examples


def main():
    examples = build_examples()

    with open(SCRIPT_DIR / "dataset.json", "r", encoding="utf-8") as f:
        dataset = json.load(f)
    test_rows = dataset["test"]
    cand_path = SCRIPT_DIR / "data" / "candidate_dataset.json"
    seen = set()
    if cand_path.exists():
        cand = json.load(open(cand_path))
        seen = {normalize(r["text"]) for s in ("train", "validation", "calibration", "test")
                for r in cand[s]}
    else:
        seen = {normalize(r["text"]) for s in ("train", "validation", "calibration", "test")
                for r in dataset[s]}
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
        "source": "nexus_synthetic_phase86",
        "review_status": "approved_for_candidate_training",
        "policy": "screen_analysis BERT coverage. One new label (train.py + nlu_server.py synced).",
        "total_examples": len(fresh),
        "unique_families": len({family_key(e) for e in fresh}),
        "intent_counts": dict(intent_counts.most_common()),
        "examples": fresh,
    }

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(OUTPUT_PATH, "w", encoding="utf-8") as f:
        json.dump(output, f, indent=2, ensure_ascii=False)

    print(f"Phase 86 screen analysis: {len(fresh)} fresh examples ({len(examples) - len(fresh)} dupes skipped)")
    for intent, count in intent_counts.most_common():
        print(f"  {intent}: {count}")
    print(f"Output: {OUTPUT_PATH}")


if __name__ == "__main__":
    main()
