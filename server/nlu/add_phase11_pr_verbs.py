#!/usr/bin/env python3
"""
Phase 11 — list_prs "pull requests" noun + trailing-tolerance top-up.

Gap: the deterministic list_prs pattern only accepted "prs" and anchored at
$ — so "show me the pull requests" never matched deterministically and
"show me the pull requests and all" confused NLU too (list_prs had 64 train
rows but almost no "pull requests"-noun or trailing-"and all" forms).

50 targeted rows (45 train forms + 5 state variants), all intent list_prs,
slot repo only. No new labels — no train.py/nlu_server.py sync needed.
OOS negatives for near-misses ("pull the door", "request a refund", ...).

Output: server/nlu/data/phase11_pr_verbs.json
"""
import json
import re
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
OUTPUT_PATH = SCRIPT_DIR / "data" / "phase11_pr_verbs.json"

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


REPOS = ["zync", "servx", "nexus", "shopkart"]


def build_examples():
    examples = []

    def add(text, repo=""):
        examples.append({"text": text, "intent": "list_prs",
                         "slots": {"repo": repo}})

    # ─── "pull requests" noun × verbs (bare, no repo) ─────────────
    for verb in ["show me", "show", "give me", "pull up", "display",
                 "fetch me", "get me", "bring me"]:
        add(f"{verb} the pull requests")
    # ─── trailing tolerance ("and all" / "all of them" / "everything") ──
    for tail in ["and all", "and all of them", "all of them", "everything"]:
        add(f"show me the pull requests {tail}")
        add(f"show me the prs {tail}")
    for tail in ["and all", "everything"]:
        add(f"list the pull requests {tail}")
        add(f"give me the pull requests {tail}")
    # ─── "pull request list" noun ─────────────────────────────────
    for verb in ["show me", "pull up", "open", "display"]:
        add(f"{verb} the pull request list")
    # ─── with repos ───────────────────────────────────────────────
    for repo in REPOS:
        add(f"show me the pull requests in {repo}", repo)
        add(f"list all the pull requests in {repo}", repo)
    add(f"show me the pull requests in {REPOS[0]} and all", REPOS[0])
    add(f"show the prs in {REPOS[1]} and all of them", REPOS[1])
    # ─── state variants with the new noun ─────────────────────────
    add("show me the open pull requests")
    add("show me the closed pull requests")
    add("show me all the pull requests")
    add("show me the merged pull requests")
    add(f"list the open pull requests in {REPOS[2]}", REPOS[2])

    # ─── OOS negatives ────────────────────────────────────────────
    for text in [
        "pull the door open",
        "request a refund for my order",
        "pull up a chair",
        "request time off tomorrow",
        "the printer is pulling the paper crooked",
        "can you pull over here",
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
        "source": "nexus_synthetic_phase11",
        "review_status": "approved_for_candidate_training",
        "policy": "list_prs pull-requests noun + trailing-tolerance top-up. No new labels.",
        "total_examples": len(fresh),
        "unique_families": len({family_key(e) for e in fresh}),
        "intent_counts": dict(intent_counts.most_common()),
        "examples": fresh,
    }

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(OUTPUT_PATH, "w", encoding="utf-8") as f:
        json.dump(output, f, indent=2, ensure_ascii=False)

    print(f"Phase 11 PR verbs: {len(fresh)} fresh examples ({len(examples) - len(fresh)} dupes skipped)")
    for intent, count in intent_counts.most_common():
        print(f"  {intent}: {count}")
    print(f"Output: {OUTPUT_PATH}")


if __name__ == "__main__":
    main()
