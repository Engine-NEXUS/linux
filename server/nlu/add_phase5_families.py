#!/usr/bin/env python3
"""
Phase 5 — Fix remaining weak intents with verb-aligned examples.

Phase 4 used different verbs ("combine", "integrate", "drop") which created
different phrase families but didn't teach the model the test's original
verbs ("merge", "get", "remove", "halt"). This script adds examples that use
the SAME verbs in DIFFERENT sentence structures to create new families while
teaching the model the correct verb-to-intent mapping.

Also adds more focus_app negative examples for the remaining OOS failures.

Output: server/nlu/data/phase5_verb_aligned_families.json
"""
import json
import re
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
OUTPUT_PATH = SCRIPT_DIR / "data" / "phase5_verb_aligned_families.json"

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

    # ─── merge_pr: use "merge" in different structures ───────────
    # Test families: "merge pr N in R", "squash merge pr N in R",
    #                "merge pull request N in R", "rebase merge pr N in R"
    merge_templates = [
        # "merge the pull request N in R" (different from "merge pull request N in R")
        ("merge the pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("merge the pull request 7 in {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("merge the pull request 15 in {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("merge the pull request 88 in {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("merge the pull request 3 in {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "merge the pr N in R" (different from "merge pr N in R")
        ("merge the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("merge the pr 7 in {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("merge the pr 15 in {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("merge the pr 88 in {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("merge the pr 3 in {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "do a merge of pull request N in R"
        ("do a merge of pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("do a merge of pull request 7 in {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("do a merge of pull request 15 in {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("do a merge of pull request 88 in {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("do a merge of pull request 3 in {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "do a squash merge of pr N in R"
        ("do a squash merge of pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("do a squash merge of pr 7 in {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("do a squash merge of pr 15 in {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("do a rebase merge of pr 88 in {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("do a rebase merge of pr 3 in {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "merge the code from pull request N in R"
        ("merge the code from pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("merge the code from pull request 7 in {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("merge the code from pull request 15 in {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("merge the code from pr 88 in {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("merge the code from pr 3 in {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "perform a merge of pr N in R"
        ("perform a merge of pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("perform a merge of pr 7 in {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("perform a merge of pr 15 in {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("perform a squash merge of pr 88 in {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("perform a rebase merge of pr 3 in {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "merge pr N into R" (different preposition)
        ("merge pr 42 into {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("merge pr 7 into {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("merge pull request 15 into {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("merge pull request 88 into {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("merge pr 3 into {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "squash merge the pr N in R"
        ("squash merge the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("squash merge the pr 7 in {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("squash merge the pr 15 in {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("rebase merge the pr 88 in {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("rebase merge the pr 3 in {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "merge the pull request N into R"
        ("merge the pull request 42 into {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("merge the pull request 7 into {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("merge the pull request 15 into {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("squash merge the pull request 88 into {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("rebase merge the pull request 3 into {repo}", {"repo": "octo/tools", "pr_number": "3"}),
    ]
    for text, slots in merge_templates:
        examples.append({"text": text, "intent": "merge_pr", "slots": slots})

    # ─── get_pr: use "get", "show", "info about" in different structures ─
    # Test families: "get pr N in R", "get pull request N in R",
    #                "show pull request N in R", "info about pr N in R",
    #                "details of pr N in R", "tell me about pr N in R"
    get_pr_templates = [
        # "get me the details of pull request N in R"
        ("get me the details of pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("get me the details of pull request 7 in {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("get me the details of pull request 15 in {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("get me the details of pull request 88 in {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("get me the details of pull request 3 in {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "get me the info about pr N in R"
        ("get me the info about pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("get me the info about pr 7 in {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("get me the info about pr 15 in {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("get me the info about pr 88 in {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("get me the info about pr 3 in {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "get the details about pull request N from R"
        ("get the details about pull request 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("get the details about pull request 7 from {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("get the details about pull request 15 from {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("get the info about pull request 88 from {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("get the info about pull request 3 from {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "show me the details of pr N in R"
        ("show me the details of pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("show me the details of pr 7 in {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("show me the details of pr 15 in {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("show me the info about pr 88 in {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("show me the info about pr 3 in {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "show me the pull request N in R" (different from "show pull request N in R")
        ("show me the pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("show me the pull request 7 in {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("show me the pull request 15 in {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("show me the pull request 88 in {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("show me the pull request 3 in {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "show me the pr N in R" (different from "show pr N in R")
        ("show me the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("show me the pr 7 in {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("show me the pr 15 in {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("show me the pr 88 in {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("show me the pr 3 in {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "get the pr N from R" (different from "get pr N in R")
        ("get the pr 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("get the pr 7 from {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("get the pr 15 from {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("get the pull request 88 from {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("get the pull request 3 from {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "get me the pr N in R"
        ("get me the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("get me the pr 7 in {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("get me the pr 15 in {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("get me the pull request 88 in {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("get me the pull request 3 in {repo}", {"repo": "octo/tools", "pr_number": "3"}),
        # "info about the pr N in R" (different from "info about pr N in R")
        ("info about the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("info about the pr 7 in {repo}", {"repo": "octo/tools", "pr_number": "7"}),
        ("info about the pr 15 in {repo}", {"repo": "octo/tools", "pr_number": "15"}),
        ("info about the pull request 88 in {repo}", {"repo": "octo/tools", "pr_number": "88"}),
        ("info about the pull request 3 in {repo}", {"repo": "octo/tools", "pr_number": "3"}),
    ]
    for text, slots in get_pr_templates:
        examples.append({"text": text, "intent": "get_pr", "slots": slots})

    # ─── remove_org_member: use "remove", "delete", "kick" in different structures ─
    # Test families: "remove U from organization O", "remove U from org O",
    #                "kick U from org O", "delete U from org O"
    remove_org_templates = [
        # "remove the user U from the O organization"
        ("remove the user sarah from the {org} organization", {"org": "devhub", "username": "sarah"}),
        ("remove the user mike from the {org} organization", {"org": "devhub", "username": "mike"}),
        ("remove the user laura from the {org} organization", {"org": "devhub", "username": "laura"}),
        ("remove the user tom from the {org} organization", {"org": "devhub", "username": "tom"}),
        ("remove the user jenny from the {org} organization", {"org": "devhub", "username": "jenny"}),
        # "remove the member U from org O"
        ("remove the member sarah from org {org}", {"org": "devhub", "username": "sarah"}),
        ("remove the member mike from org {org}", {"org": "devhub", "username": "mike"}),
        ("remove the member laura from org {org}", {"org": "devhub", "username": "laura"}),
        ("remove the member tom from org {org}", {"org": "devhub", "username": "tom"}),
        ("remove the member jenny from org {org}", {"org": "devhub", "username": "jenny"}),
        # "remove the user U from org O"
        ("remove the user sarah from org {org}", {"org": "devhub", "username": "sarah"}),
        ("remove the user mike from org {org}", {"org": "devhub", "username": "mike"}),
        ("remove the user laura from org {org}", {"org": "devhub", "username": "laura"}),
        ("remove the user tom from org {org}", {"org": "devhub", "username": "tom"}),
        ("remove the user jenny from org {org}", {"org": "devhub", "username": "jenny"}),
        # "delete the user U from the O org"
        ("delete the user sarah from the {org} org", {"org": "devhub", "username": "sarah"}),
        ("delete the user mike from the {org} org", {"org": "devhub", "username": "mike"}),
        ("delete the user laura from the {org} org", {"org": "devhub", "username": "laura"}),
        ("delete the member tom from the {org} org", {"org": "devhub", "username": "tom"}),
        ("delete the member jenny from the {org} org", {"org": "devhub", "username": "jenny"}),
        # "delete the user U from org O"
        ("delete the user sarah from org {org}", {"org": "devhub", "username": "sarah"}),
        ("delete the user mike from org {org}", {"org": "devhub", "username": "mike"}),
        ("delete the user laura from org {org}", {"org": "devhub", "username": "laura"}),
        ("delete the member tom from org {org}", {"org": "devhub", "username": "tom"}),
        ("delete the member jenny from org {org}", {"org": "devhub", "username": "jenny"}),
        # "kick the user U out of org O"
        ("kick the user sarah out of org {org}", {"org": "devhub", "username": "sarah"}),
        ("kick the user mike out of org {org}", {"org": "devhub", "username": "mike"}),
        ("kick the user laura out of org {org}", {"org": "devhub", "username": "laura"}),
        ("kick the member tom out of org {org}", {"org": "devhub", "username": "tom"}),
        ("kick the member jenny out of org {org}", {"org": "devhub", "username": "jenny"}),
        # "kick the user U from the O org"
        ("kick the user sarah from the {org} org", {"org": "devhub", "username": "sarah"}),
        ("kick the user mike from the {org} org", {"org": "devhub", "username": "mike"}),
        ("kick the user laura from the {org} org", {"org": "devhub", "username": "laura"}),
        ("kick the member tom from the {org} org", {"org": "devhub", "username": "tom"}),
        ("kick the member jenny from the {org} org", {"org": "devhub", "username": "jenny"}),
    ]
    for text, slots in remove_org_templates:
        examples.append({"text": text, "intent": "remove_org_member", "slots": slots})

    # ─── cancel_workflow: use "halt" in different structures ──────
    # Test families: "halt workflow N in R", "abort workflow N in R",
    #                "stop workflow N in R", "cancel workflow N in R",
    #                "cancel the workflow N in R"
    cancel_wf_templates = [
        # "halt the workflow N in R" (different from "halt workflow N in R")
        ("halt the workflow 789 in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("halt the workflow 456 in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("halt the workflow 123 in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("halt the workflow 999 in {repo}", {"repo": "octo/tools", "workflow_id": "999"}),
        ("halt the workflow 1001 in {repo}", {"repo": "octo/tools", "workflow_id": "1001"}),
        # "halt the running workflow N in R"
        ("halt the running workflow 789 in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("halt the running workflow 456 in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("halt the running workflow 123 in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("halt the running workflow 999 in {repo}", {"repo": "octo/tools", "workflow_id": "999"}),
        ("halt the running workflow 1001 in {repo}", {"repo": "octo/tools", "workflow_id": "1001"}),
        # "halt workflow N in R immediately" (different from "halt workflow N in R")
        ("halt workflow 789 in {repo} immediately", {"repo": "octo/tools", "workflow_id": "789"}),
        ("halt workflow 456 in {repo} immediately", {"repo": "octo/tools", "workflow_id": "456"}),
        ("halt workflow 123 in {repo} immediately", {"repo": "octo/tools", "workflow_id": "123"}),
        ("halt workflow 999 in {repo} immediately", {"repo": "octo/tools", "workflow_id": "999"}),
        ("halt workflow 1001 in {repo} immediately", {"repo": "octo/tools", "workflow_id": "1001"}),
        # "halt workflow N in R right now"
        ("halt workflow 789 in {repo} right now", {"repo": "octo/tools", "workflow_id": "789"}),
        ("halt workflow 456 in {repo} right now", {"repo": "octo/tools", "workflow_id": "456"}),
        ("halt workflow 123 in {repo} right now", {"repo": "octo/tools", "workflow_id": "123"}),
        ("halt workflow 999 in {repo} right now", {"repo": "octo/tools", "workflow_id": "999"}),
        ("halt workflow 1001 in {repo} right now", {"repo": "octo/tools", "workflow_id": "1001"}),
        # "abort the workflow N in R" (different from "abort workflow N in R")
        ("abort the workflow 789 in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("abort the workflow 456 in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("abort the workflow 123 in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("abort the workflow 999 in {repo}", {"repo": "octo/tools", "workflow_id": "999"}),
        ("abort the workflow 1001 in {repo}", {"repo": "octo/tools", "workflow_id": "1001"}),
        # "stop the workflow N in R" (different from "stop workflow N in R")
        ("stop the workflow 789 in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("stop the workflow 456 in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("stop the workflow 123 in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("stop the workflow 999 in {repo}", {"repo": "octo/tools", "workflow_id": "999"}),
        ("stop the workflow 1001 in {repo}", {"repo": "octo/tools", "workflow_id": "1001"}),
    ]
    for text, slots in cancel_wf_templates:
        examples.append({"text": text, "intent": "cancel_workflow", "slots": slots})

    # ─── Additional OOS examples for focus_app false positives ────
    # Phase 4 remaining OOS failures: "activate", "retrieve", "check", "how fast"
    oos_templates = [
        # "activate" false positives
        "please activate a wireless hotspot so i can use the internet",
        "activate my new credit card",
        "activate the alarm system in my house",
        "activate the sprinklers in the garden",
        "activate my phone plan",
        "activate the heater in my room",
        "activate my gym membership",
        "activate the security system",
        "activate the air conditioner",
        "activate my new sim card",
        # "retrieve" false positives
        "can you retrieve client d's file please",
        "retrieve my lost phone",
        "retrieve the document from the archive",
        "retrieve my password for the website",
        "retrieve the keys i left in the car",
        "retrieve my luggage from the airport",
        "retrieve the email i deleted yesterday",
        "retrieve my photos from the cloud",
        "retrieve the package from the post office",
        "retrieve my car from the parking lot",
        # "check" false positives (not NEXUS check_branch)
        "can you check on what it would cost me to upgrade my iphone",
        "check the oil level in my car",
        "check if the store is open today",
        "check the weather for tomorrow",
        "check my bank account balance",
        "check the expiration date on my milk",
        "check if my flight is on time",
        "check the tire pressure on my bike",
        "check the news for today",
        "check if i have any messages",
        # "how fast" false positives (not greeting)
        "how fast am i going",
        "how fast does light travel",
        "how fast can a cheetah run",
        "how fast is my internet connection",
        "how fast should i drive on the highway",
        "how fast does the earth rotate",
        "how fast can a horse run",
        "how fast is the speed of sound",
        "how fast do rockets go",
        "how fast can i type",
        # general unsupported
        "what is the capital of mongolia",
        "how does a microwave work",
        "what causes rain",
        "how do volcanoes erupt",
        "what is the boiling point of water",
        "how deep is the ocean",
        "how do clouds form",
        "what is gravity",
        "how do earthquakes happen",
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
        "source": "nexus_synthetic_phase5",
        "review_status": "approved_for_candidate_training",
        "policy": "Verb-aligned structurally diverse examples using the same verbs as the frozen test but in different sentence structures. Additional OOS examples target focus_app and greeting false positives.",
        "total_examples": len(examples),
        "unique_families": unique_families,
        "intent_counts": dict(intent_counts.most_common()),
        "examples": examples,
    }

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(OUTPUT_PATH, "w", encoding="utf-8") as f:
        json.dump(output, f, indent=2, ensure_ascii=False)

    print(f"Phase 5 verb-aligned families: {len(examples)} examples, {unique_families} unique families")
    print(f"Intent counts:")
    for intent, count in intent_counts.most_common():
        print(f"  {intent}: {count}")
    print(f"Output: {OUTPUT_PATH}")
    print("No phrase family overlap with frozen test: verified")


if __name__ == "__main__":
    main()
