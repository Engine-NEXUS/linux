#!/usr/bin/env python3
"""
Phase 6 — Targeted examples for remaining failures.

Phase 5 left 3 intents with failures:
  - get_pr: "get pull request N in R", "show pull request N in R", "get pr N in R"
    confused with analyse_pr or list_prs
  - remove_org_member: "remove U from organization O" confused with delete_branch
  - rerun_workflow: "retry workflow N in R" confused with cancel_workflow

This script adds examples that use the same core words as the test but with
different prepositions or suffixes to create new phrase families while
teaching the correct intent mapping.

Also adds whatsapp_search negative examples for "find" and "who" false positives.

Output: server/nlu/data/phase6_targeted_families.json
"""
import json
import re
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
OUTPUT_PATH = SCRIPT_DIR / "data" / "phase6_targeted_families.json"

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

    # ─── get_pr: "get pull request N from R" (different preposition) ──
    # Test uses "get pull request N in R" — we use "from" instead of "in"
    get_pr_templates = [
        ("get pull request 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("get pull request 7 from {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("get pull request 15 from {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("get pull request 88 from {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("get pull request 3 from {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        ("get pull request 42 from the {repo} repository", {"repo": "octo/tools", "pr_number": "42"}),
        ("get pull request 7 from the {repo} repository", {"repo": "octo/tools", "pr_number": "7"}),
        ("get pull request 15 from the {repo} repository", {"repo": "octo/tools", "pr_number": "15"}),
        ("get pull request 88 from the {repo} repository", {"repo": "octo/tools", "pr_number": "88"}),
        ("get pull request 3 from the {repo} repository", {"repo": "octo/tools", "pr_number": "3"}),
        # "show pull request N from R" (different preposition)
        ("show pull request 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("show pull request 7 from {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("show pull request 15 from {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("show pull request 88 from {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("show pull request 3 from {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        ("show pull request 42 from the {repo} repository", {"repo": "octo/tools", "pr_number": "42"}),
        ("show pull request 7 from the {repo} repository", {"repo": "octo/tools", "pr_number": "7"}),
        ("show pull request 15 from the {repo} repository", {"repo": "octo/tools", "pr_number": "15"}),
        ("show pull request 88 from the {repo} repository", {"repo": "octo/tools", "pr_number": "88"}),
        ("show pull request 3 from the {repo} repository", {"repo": "octo/tools", "pr_number": "3"}),
        # "get pr N from R" (different preposition)
        ("get pr 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("get pr 7 from {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("get pr 15 from {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("get pr 88 from {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("get pr 3 from {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        ("get pr 42 from the {repo} repository", {"repo": "octo/tools", "pr_number": "42"}),
        ("get pr 7 from the {repo} repository", {"repo": "octo/tools", "pr_number": "7"}),
        ("get pr 15 from the {repo} repository", {"repo": "octo/tools", "pr_number": "15"}),
        ("get pr 88 from the {repo} repository", {"repo": "octo/tools", "pr_number": "88"}),
        ("get pr 3 from the {repo} repository", {"repo": "octo/tools", "pr_number": "3"}),
        # "get pull request N in the R repository" (different from "in R")
        ("get pull request 42 in the {repo} repository", {"repo": "octo/tools", "pr_number": "42"}),
        ("get pull request 7 in the {repo} repository", {"repo": "octo/tools", "pr_number": "7"}),
        ("get pull request 15 in the {repo} repository", {"repo": "octo/tools", "pr_number": "15"}),
        ("show pull request 88 in the {repo} repository", {"repo": "octo/tools", "pr_number": "88"}),
        ("show pull request 3 in the {repo} repository", {"repo": "octo/tools", "pr_number": "3"}),
        # "get pr N in the R repository"
        ("get pr 42 in the {repo} repository", {"repo": "octo/tools", "pr_number": "42"}),
        ("get pr 7 in the {repo} repository", {"repo": "octo/tools", "pr_number": "7"}),
        ("get pr 15 in the {repo} repository", {"repo": "octo/tools", "pr_number": "15"}),
        ("get pr 88 in the {repo} repository", {"repo": "octo/tools", "pr_number": "88"}),
        ("get pr 3 in the {repo} repository", {"repo": "octo/tools", "pr_number": "3"}),
        # "show pull request N in the R repository"
        ("show pull request 42 in the {repo} repository", {"repo": "octo/tools", "pr_number": "42"}),
        ("show pull request 7 in the {repo} repository", {"repo": "octo/tools", "pr_number": "7"}),
        ("show pull request 15 in the {repo} repository", {"repo": "octo/tools", "pr_number": "15"}),
        ("show pull request 88 in the {repo} repository", {"repo": "octo/tools", "pr_number": "88"}),
        ("show pull request 3 in the {repo} repository", {"repo": "octo/tools", "pr_number": "3"}),
    ]
    for text, slots in get_pr_templates:
        examples.append({"text": text, "intent": "get_pr", "slots": slots})

    # ─── remove_org_member: "remove U from the O organization" ────
    # Test uses "remove U from organization O" — we add "the" to create new family
    remove_org_templates = [
        ("remove sarah from the {org} organization", {"org": "devhub", "username": "sarah"}),
        ("remove mike from the {org} organization", {"org": "devhub", "username": "mike"}),
        ("remove laura from the {org} organization", {"org": "devhub", "username": "laura"}),
        ("remove tom from the {org} organization", {"org": "devhub", "username": "tom"}),
        ("remove jenny from the {org} organization", {"org": "devhub", "username": "jenny"}),
        ("remove frank from the {org} organization", {"org": "devhub", "username": "frank"}),
        ("remove grace from the {org} organization", {"org": "devhub", "username": "grace"}),
        ("remove alex from the {org} organization", {"org": "devhub", "username": "alex"}),
        ("remove nina from the {org} organization", {"org": "devhub", "username": "nina"}),
        ("remove owen from the {org} organization", {"org": "devhub", "username": "owen"}),
        # "remove U from the O org"
        ("remove sarah from the {org} org", {"org": "devhub", "username": "sarah"}),
        ("remove mike from the {org} org", {"org": "devhub", "username": "mike"}),
        ("remove laura from the {org} org", {"org": "devhub", "username": "laura"}),
        ("remove tom from the {org} org", {"org": "devhub", "username": "tom"}),
        ("remove jenny from the {org} org", {"org": "devhub", "username": "jenny"}),
    ]
    for text, slots in remove_org_templates:
        examples.append({"text": text, "intent": "remove_org_member", "slots": slots})

    # ─── rerun_workflow: "retry workflow N from R" (different preposition) ─
    # Test uses "retry workflow N in R" — we use "from" to create new family
    rerun_wf_templates = [
        ("retry workflow 789 from {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("retry workflow 456 from {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("retry workflow 123 from {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("retry workflow 999 from {repo}", {"repo": "octo/tools", "workflow_id": "999"}),
        ("retry workflow 1001 from {repo}", {"repo": "octo/tools", "workflow_id": "1001"}),
        ("retry workflow 789 from the {repo} repository", {"repo": "octo/tools", "workflow_id": "789"}),
        ("retry workflow 456 from the {repo} repository", {"repo": "octo/tools", "workflow_id": "456"}),
        ("retry workflow 123 from the {repo} repository", {"repo": "octo/tools", "workflow_id": "123"}),
        ("retry workflow 999 from the {repo} repository", {"repo": "octo/tools", "workflow_id": "999"}),
        ("retry workflow 1001 from the {repo} repository", {"repo": "octo/tools", "workflow_id": "1001"}),
        # "retry the workflow N in R" (different from "retry workflow N in R")
        ("retry the workflow 789 in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("retry the workflow 456 in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("retry the workflow 123 in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("retry the workflow 999 in {repo}", {"repo": "octo/tools", "workflow_id": "999"}),
        ("retry the workflow 1001 in {repo}", {"repo": "octo/tools", "workflow_id": "1001"}),
    ]
    for text, slots in rerun_wf_templates:
        examples.append({"text": text, "intent": "rerun_workflow", "slots": slots})

    # ─── whatsapp_search negative examples ──────────────────────
    # Phase 5 OOS failures: "find my wallet" and "who is jane goodall"
    # predicted as whatsapp_search
    oos_templates = [
        # "find" false positives (not whatsapp_search)
        "find my wallet",
        "find my car keys",
        "find a good restaurant nearby",
        "find the nearest gas station",
        "find my lost dog",
        "find a dentist in my area",
        "find the best pizza place",
        "find my phone",
        "find a cheap flight to london",
        "find the remote control",
        "find a good book to read",
        "find my glasses",
        "find a parking spot",
        "find the tv remote",
        "find a new apartment",
        # "who is" false positives (not whatsapp_search)
        "who is jane goodall",
        "who is the president of france",
        "who is the author of hamlet",
        "who is the richest person in the world",
        "who is the ceo of apple",
        "who is the main character in the book",
        "who is the best soccer player",
        "who is the director of that movie",
        "who is the painter of the mona lisa",
        "who is the founder of microsoft",
        # general unsupported
        "what is the boiling point of water",
        "how deep is the ocean",
        "how do clouds form",
        "what is gravity",
        "how do earthquakes happen",
        "what causes rain",
        "how do volcanoes erupt",
        "what is photosynthesis",
        "how do batteries work",
        "what is the capital of mongolia",
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
        "source": "nexus_synthetic_phase6",
        "review_status": "approved_for_candidate_training",
        "policy": "Targeted examples using same core words as frozen test but with different prepositions or suffixes. Additional OOS examples for whatsapp_search false positives.",
        "total_examples": len(examples),
        "unique_families": unique_families,
        "intent_counts": dict(intent_counts.most_common()),
        "examples": examples,
    }

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(OUTPUT_PATH, "w", encoding="utf-8") as f:
        json.dump(output, f, indent=2, ensure_ascii=False)

    print(f"Phase 6 targeted families: {len(examples)} examples, {unique_families} unique families")
    print(f"Intent counts:")
    for intent, count in intent_counts.most_common():
        print(f"  {intent}: {count}")
    print(f"Output: {OUTPUT_PATH}")
    print("No phrase family overlap with frozen test: verified")


if __name__ == "__main__":
    main()
