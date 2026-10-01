#!/usr/bin/env python3
"""
NEXUS NLU — Expand list_prs training data.

This script:
  1. Removes all closed/merged PR examples from list_prs (open-only by default)
  2. Removes all "owner/repo" literal examples (replace with real repos or empty)
  3. Adds ~370 new open-only possibilities across 20 categories
  4. Deduplicates
  5. Saves the expanded dataset

Usage:
  python expand_list_prs.py
"""

import json
import re
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
DATASET_PATH = SCRIPT_DIR / "dataset.json"

# ─── Step 1: New list_prs examples (open-only) ──────────────────────────────

NEW_EXAMPLES = [
    # ─── 1. Direct verb + PR(s) — the core commands ───────────────────────
    "list prs", "list pr", "list the prs", "list all prs", "list open prs",
    "show prs", "show pr", "show the prs", "show all prs", "show open prs",
    "get prs", "get pr", "get the prs", "get me prs", "get me the prs",
    "fetch prs", "fetch the prs", "pull up prs", "pull up the prs",
    "bring up prs", "bring up the prs", "display prs", "display the prs",
    "open prs", "open the prs", "view prs", "view the prs",
    "see prs", "see the prs", "check prs", "check the prs", "check open prs",

    # ─── 2. "Show me" / "Give me" / "Get me" — polite/conversational ──────
    "show me the prs", "show me prs", "show me all prs", "show me open prs",
    "show me the pr list", "show me my prs",
    "give me the prs", "give me prs", "give me all prs", "give me open prs",
    "give me the pr list",
    "get me the prs", "get me all prs", "get me the pr list",
    "bring me the prs", "fetch me the prs", "pull up the prs for me",

    # ─── 3. "PR list" as a noun ───────────────────────────────────────────
    "pr list", "the pr list", "show pr list", "show the pr list",
    "open pr list", "open the pr list", "get pr list", "get the pr list",
    "pull up pr list", "display pr list", "bring up pr list", "view pr list",
    "show me the pr list", "give me the pr list", "get me the pr list",
    "fetch the pr list",

    # ─── 4. Latest / recent / new — time-based ────────────────────────────
    "latest prs", "latest pr", "the latest prs", "show latest prs",
    "show me the latest prs", "show the latest pr", "get latest prs",
    "get the latest prs", "fetch latest prs", "list latest prs",
    "what are the latest prs", "what's the latest pr", "what was the latest pr",
    "recent prs", "recent pr", "the recent prs", "show recent prs",
    "show me recent prs", "get recent prs", "list recent prs",
    "what are the recent prs",
    "new prs", "show new prs", "any new prs", "are there new prs",
    "show me new prs", "list new prs", "what new prs are there",
    "latest pull requests", "show latest pull requests",
    "recent pull requests", "show recent pull requests",
    "give me the latest pr", "show me the latest pr",

    # ─── 5. Questions — "what" / "any" / "how many" ───────────────────────
    "what prs are open", "what are the open prs", "what prs do i have",
    "what are my prs", "what's in my prs",
    "any prs", "any open prs", "are there any prs", "are there any open prs",
    "do i have any prs", "do we have any prs",
    "how many prs are open", "how many open prs", "how many prs",
    "how many prs are there",
    "what prs need review", "what prs are pending",

    # ─── 6. "Pull request(s)" — full form ────────────────────────────────
    "list pull requests", "show pull requests", "show me pull requests",
    "show the pull requests", "get pull requests", "fetch pull requests",
    "list all pull requests", "show all pull requests",
    "show open pull requests",
    "latest pull requests", "recent pull requests",
    "show me the latest pull requests",
    "what pull requests are open", "any pull requests",
    "are there any pull requests",
    "pull request list", "show pull request list",
    "the pull request list", "open pull request list",

    # ─── 7. With repository specified (real repo names, NOT owner/repo) ──
    "list prs in zync", "list prs in ultron", "show prs in zync",
    "show me prs in zync", "show prs for zync", "show prs for ultron",
    "get prs in zync", "fetch prs in zync",
    "list open prs in zync", "show open prs in zync",
    "what prs are open in zync", "any prs in zync",
    "are there any prs in zync", "show me the prs in zync",
    "pull requests in zync", "show pull requests in zync",
    "latest prs in zync", "recent prs in zync",
    "list prs for the zync repo", "show prs for the ultron repo",
    "list prs in servx", "show prs in servx", "show me prs in servx",
    "list prs in nexus", "show prs in nexus",

    # ─── 8. Account-wide / "my" / "all my repos" ──────────────────────────
    "show all my prs", "show me all my prs", "list all my prs",
    "get all my prs", "fetch all my prs",
    "show prs across all my repos", "show prs from all my repos",
    "show prs from every repo", "list prs from all repos",
    "show me prs from all repositories",
    "what prs are open across my repos",
    "show all open prs", "show me all open prs across my repos",
    "list all open prs everywhere", "show me everything that's open",

    # ─── 9. State-specific (OPEN only — closed/merged removed) ────────────
    "show open prs", "list open prs", "show me open prs",
    "get open prs", "any open prs", "are there open prs",
    "show me the open ones", "which prs are open",
    "show me what's open",
    "show me all the open prs", "list all the open prs",
    "what's open", "show me what's open right now",
    "show all open pull requests", "list all open pull requests",

    # ─── 10. Filler word prefixes (STT often adds these) ──────────────────
    "so list prs", "so show me the prs", "and list prs",
    "and show me the prs", "but show me the prs", "then list prs",
    "then show me the prs", "hey list prs", "hey show me the prs",
    "please list prs", "please show me the prs",
    "ok list prs", "ok show me the prs", "now list prs",
    "now show me the prs", "um show me the prs", "uh show me the prs",
    "well show me the prs", "let me see the prs", "let me see prs",
    "can you list prs", "can you show me the prs",
    "could you list prs", "could you show me the prs",

    # ─── 11. STT mishearings — "PR" → sounds like ─────────────────────────
    "pee ars", "pee ar", "peer", "peers", "pear", "pears",
    "pay are", "pay ars", "p r", "p r s",
    "pee are", "pee are es", "pee are s",
    "pi ar", "pi ars", "b r", "b r s",
    "bee ar", "bee ars", "dee ar", "dee ars",
    "tea ar", "tea ars", "free ar", "free ars",
    "key ar", "key ars", "me ar", "me ars",
    "she ar", "she ars", "we ar", "we ars",
    "show me the pee ars", "show pee ars", "list pee ars",
    "show me the peers", "show peers", "list peers",
    "show me the pears", "show pears",
    "show me the p r s", "show p r s",
    "pee are list", "show me the pee are list",
    "show me the pee ar list", "pee ar list",

    # ─── 12. STT mishearings — "pull request" → sounds like ───────────────
    "pool request", "pool requests", "pole request", "pole requests",
    "bull request", "bull requests", "full request", "full requests",
    "pull re quest", "pool re quest", "poor request", "poor requests",
    "poll request", "poll requests", "paul request", "paul requests",
    "pulled request", "pulled requests",
    "show pool requests", "show me the pool requests",
    "list pool requests", "show pool request",
    "show me the pool request list", "pool request list",
    "show me the pole requests", "show pole requests",
    "show me the bull requests", "show bull requests",

    # ─── 13. STT mishearings — "list" → sounds like ───────────────────────
    "least prs", "last prs", "lust prs", "lost prs", "lift prs",
    "left prs", "lease prs", "leet prs", "loose prs",
    "least the prs", "show me the least prs",

    # ─── 14. STT mishearings — "show" → sounds like ───────────────────────
    "so prs", "so me the prs", "shoe prs", "shoe me the prs",
    "sho prs", "sho me the prs", "shew prs", "shew me the prs",
    "slow prs", "sew prs", "so me the pr list",

    # ─── 15. STT mishearings — "latest" / "recent" → sounds like ──────────
    "lay test prs", "late prs", "lay test peers",
    "lady prs", "lately prs",
    "recent peers", "reece and prs", "recently prs",
    "reason prs", "season prs",
    "show me the lay test prs", "show me the late prs",
    "show me the lady prs", "show me the recently prs",

    # ─── 16. Casual / shorthand / abbreviated ─────────────────────────────
    "prs", "the prs", "my prs", "open prs", "all prs",
    "just prs", "give me prs", "show prs please", "prs please",
    "the pr list", "pr list", "my pr list", "open pr list",
    "show me prs please", "just show me prs",

    # ─── 17. Contextual / work-related phrasings ──────────────────────────
    "what needs review", "what's waiting for review", "what's pending review",
    "show me what needs review", "show me pending reviews",
    "show me review requests",
    "what's in the queue", "show me the queue", "show me the pr queue",
    "what's in my queue", "show me my review queue",
    "what's waiting on me", "what's on my plate",
    "anything waiting for me", "anything needing review",
    "anything needs my attention", "show me what needs my attention",

    # ─── 18. Combined / complex sentences ─────────────────────────────────
    "show me the pr list for zync", "show me all the open prs in zync",
    "what are the latest open prs", "show me the latest open prs across all repos",
    "give me a list of all open prs",
    "can you show me all the prs that are open",
    "show me all the pull requests that are open right now",
    "what pull requests are currently open",
    "list all the prs that are open in my repos",

    # ─── 19. Indian accent / pronunciation variations ─────────────────────
    "sho me de prs", "sho me de pr list", "giv me de prs",
    "list de prs", "sho de prs", "wat prs are open",
    "sho me latest pr", "sho me de latest prs",
    "sho me de open prs", "list de open prs",

    # ─── 20. Wrong-but-close commands (user says wrong thing) ─────────────
    "show me the bp list", "show me the pv list", "show me the pt list",
    "show me the qr list", "show me the tr list",
    "show me the bt list", "show me the pd list",
    "show me the br list", "show me the fr list",
]


def main():
    # Load dataset
    with open(DATASET_PATH, "r", encoding="utf-8") as f:
        dataset = json.load(f)

    train = dataset.get("train", [])
    test = dataset.get("test", [])

    # ─── Step 1: Clean existing list_prs examples ─────────────────────────
    # Remove: closed, merged, and "owner/repo" literal examples
    cleaned_train = []
    removed_count = 0
    for ex in train:
        if ex["intent"] == "list_prs":
            text_lower = ex["text"].lower()
            # Remove closed/merged examples
            if "closed" in text_lower or "merged" in text_lower:
                removed_count += 1
                continue
            # Remove "owner/repo" literal examples
            if "owner/repo" in text_lower:
                removed_count += 1
                continue
            # Remove "myorg/myrepo" literal examples
            if "myorg/myrepo" in text_lower:
                removed_count += 1
                continue
        cleaned_train.append(ex)

    print(f"[CLEAN] Removed {removed_count} bad list_prs examples")
    remaining_list_prs = sum(1 for e in cleaned_train if e["intent"] == "list_prs")
    print(f"[CLEAN] Remaining list_prs examples: {remaining_list_prs}")

    # Also clean test set
    cleaned_test = []
    for ex in test:
        if ex["intent"] == "list_prs":
            text_lower = ex["text"].lower()
            if "closed" in text_lower or "merged" in text_lower:
                continue
            if "owner/repo" in text_lower or "myorg/myrepo" in text_lower:
                continue
        cleaned_test.append(ex)

    # ─── Step 2: Add new examples ─────────────────────────────────────────
    new_examples = []
    for text in NEW_EXAMPLES:
        new_examples.append({
            "text": text,
            "intent": "list_prs",
            "slots": {"repo": ""},
        })

    # Deduplicate new examples
    seen = set()
    unique_new = []
    for ex in new_examples:
        key = (ex["text"].lower().strip(), ex["intent"])
        if key not in seen:
            seen.add(key)
            unique_new.append(ex)

    print(f"[ADD] {len(unique_new)} new unique list_prs examples")

    # ─── Step 3: Merge new examples into train ────────────────────────────
    # Check for duplicates against existing train
    existing_texts = {(e["text"].lower().strip(), e["intent"]) for e in cleaned_train}
    truly_new = []
    for ex in unique_new:
        key = (ex["text"].lower().strip(), ex["intent"])
        if key not in existing_texts:
            truly_new.append(ex)
            existing_texts.add(key)

    print(f"[MERGE] {len(truly_new)} truly new examples (not already in dataset)")

    cleaned_train.extend(truly_new)

    # ─── Step 4: Save ─────────────────────────────────────────────────────
    dataset["train"] = cleaned_train
    dataset["test"] = cleaned_test

    with open(DATASET_PATH, "w", encoding="utf-8") as f:
        json.dump(dataset, f, indent=2, ensure_ascii=False)

    final_count = sum(1 for e in cleaned_train if e["intent"] == "list_prs")
    total_count = len(cleaned_train)
    print(f"\n[DONE] Dataset saved.")
    print(f"  list_prs examples: {remaining_list_prs} → {final_count}")
    print(f"  Total train examples: {total_count}")
    print(f"  Total test examples: {len(cleaned_test)}")


if __name__ == "__main__":
    main()
