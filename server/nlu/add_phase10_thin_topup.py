#!/usr/bin/env python3
"""
Phase 10 — thin-intent top-up.

Brings every intent with <30 production rows to >=40 with template families
+ varied slot values. No new labels.

Targets (production counts): search:10, list_pr_files:7, create_pr:8,
remove_collaborator:8, update_branch:14, analyse_pr:15, close_app:16,
whatsapp_chat:17, analyse_repo:17, analyse_latest_pr:19, list_branches:19,
check_branch:23, open_url:23, list_workflows:25, list_workflow_runs:25,
comment_pr:26.

Output: server/nlu/data/phase10_thin_topup.json
"""
import json
import re
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
OUTPUT_PATH = SCRIPT_DIR / "data" / "phase10_thin_topup.json"

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


REPOS = ["servx", "zync", "nexus", "ledger-ai", "shopkart", "owner/project"]
APPS = ["calculator", "notepad", "paint", "vlc", "spotify", "cmd",
        "task manager", "photos", "calendar", "camera"]
CONTACTS = ["mom", "dad", "lakshya", "prem", "eesha", "brother"]
QUERIES = ["hotel deals", "weather today", "python tutorial", "news headlines",
           "cricket score", "train timings", "best laptop", "yoga videos"]
USERS = ["alex", "prem", "eesha", "ravi", "sana"]
BRANCHES = ["feature-login", "fix-crash", "dev", "release-2"]
URLS = ["github.com", "stackoverflow.com", "news.ycombinator.com"]


def build_examples():
    ex = []
    A = ex.append

    # search (+30) — avoid test families: search for|search|google|find|find me|look for|look up <query>
    for q in QUERIES:
        A({"text": f"hunt down {q}", "intent": "search", "slots": {"query": q}})
    for q in QUERIES[:8]:
        A({"text": f"dig up {q}", "intent": "search", "slots": {"query": q}})
    for q in QUERIES[:8]:
        A({"text": f"show me results for {q}", "intent": "search", "slots": {"query": q}})
    for q in QUERIES[:6]:
        A({"text": f"i need details about {q}", "intent": "search", "slots": {"query": q}})

    # close_app (+30) — avoid: close|kill|quit|shut|shut down <app>
    for a in APPS:
        A({"text": f"exit {a}", "intent": "close_app", "slots": {"app_name": a}})
        A({"text": f"terminate {a}", "intent": "close_app", "slots": {"app_name": a}})
        A({"text": f"force close {a}", "intent": "close_app", "slots": {"app_name": a}})

    # whatsapp_chat (+25) — avoid: chat with|message|open my chat|send message to|send whatsapp to
    for c in CONTACTS:
        A({"text": f"open chat with {c}", "intent": "whatsapp_chat", "slots": {"contact": c}})
        A({"text": f"talk to {c} on whatsapp", "intent": "whatsapp_chat", "slots": {"contact": c}})
        A({"text": f"ping {c} on whatsapp", "intent": "whatsapp_chat", "slots": {"contact": c}})
    for c in CONTACTS[:7]:
        A({"text": f"get me {c} on whatsapp", "intent": "whatsapp_chat", "slots": {"contact": c}})

    # analyse_repo (+25) — avoid: analyse|analyze <repo> repo|repository variants
    for r in REPOS:
        A({"text": f"scan {r} codebase", "intent": "analyse_repo", "slots": {"repo": r}})
        A({"text": f"map the architecture of {r}", "intent": "analyse_repo", "slots": {"repo": r}})
        A({"text": f"give me an overview of {r}", "intent": "analyse_repo", "slots": {"repo": r}})
    for r in REPOS[:7]:
        A({"text": f"inspect the {r} project", "intent": "analyse_repo", "slots": {"repo": r}})

    # analyse_pr (+25) — avoid: analyse|analyze pr|pull request <n> in|<repo> forms
    for n, r in zip([5, 12, 23, 42, 7, 99], REPOS):
        A({"text": f"review pr {n} in {r}", "intent": "analyse_pr",
           "slots": {"pr_number": str(n), "repo": r}})
        A({"text": f"deep analyse pr {n} {r}", "intent": "analyse_pr",
           "slots": {"pr_number": str(n), "repo": r}})
        A({"text": f"check pr {n} in {r}", "intent": "analyse_pr",
           "slots": {"pr_number": str(n), "repo": r}})
    for n in [3, 8, 15, 27, 31, 55, 71]:
        A({"text": f"critique pull request {n} in servx", "intent": "analyse_pr",
           "slots": {"pr_number": str(n), "repo": "servx"}})

    # analyse_latest_pr (+22) — avoid: analyse latest|newest|the latest pr|pull request in <repo>, pr in <repo>
    for r in REPOS:
        A({"text": f"show me the current pr in {r}", "intent": "analyse_latest_pr", "slots": {"repo": r}})
        A({"text": f"what is the newest pull request in {r}", "intent": "analyse_latest_pr", "slots": {"repo": r}})
    for u in USERS:
        A({"text": f"recent pr by {u} in servx", "intent": "analyse_latest_pr",
           "slots": {"repo": "servx", "author": u}})
    for r in REPOS[:5]:
        A({"text": f"get the active pr in {r}", "intent": "analyse_latest_pr", "slots": {"repo": r}})

    # list_pr_files (+33) — avoid: list pr files|show files|show pr files|what files changed
    for n, r in zip([42, 7, 19, 101, 3, 55], REPOS):
        A({"text": f"list files in pr {n} in {r}", "intent": "list_pr_files",
           "slots": {"pr_number": str(n), "repo": r}})
        A({"text": f"which files did pr {n} touch in {r}", "intent": "list_pr_files",
           "slots": {"pr_number": str(n), "repo": r}})
        A({"text": f"diff files for pr {n} in {r}", "intent": "list_pr_files",
           "slots": {"pr_number": str(n), "repo": r}})
    for n in [11, 22, 33, 44, 66, 77, 88, 9, 13, 17, 21, 25, 29, 35, 41]:
        A({"text": f"files changed in pr {n} servx", "intent": "list_pr_files",
           "slots": {"pr_number": str(n), "repo": "servx"}})

    # create_pr (+32) — avoid: [and|so] create pr|pull request <title> from <head> to <base> in <repo>
    for i, (t, h, b) in enumerate([
        ("improve login", "feature-login", "main"),
        ("fix crash on start", "fix-crash", "dev"),
        ("add dark mode", "feat-dark", "main"),
        ("update readme", "docs-update", "main"),
        ("payment flow", "feat-pay", "release-2"),
        ("speed up search", "perf-search", "main")]):
        r = REPOS[i % len(REPOS)]
        A({"text": f"open a pr in {r} {t} from {h} to {b}",
           "intent": "create_pr",
           "slots": {"repo": r, "title": t, "head": h, "base": b}})
        A({"text": f"new pull request in {r} {t} {h} into {b}", "intent": "create_pr",
           "slots": {"repo": r, "title": t, "head": h, "base": b}})
    for t in ["refactor auth", "bump deps", "fix typo", "add tests", "new landing",
              "cache layer", "api v2", "mobile layout", "sso login", "export csv",
              "fix flaky test", "perf pass", "i18n strings", "health check",
              "docker setup", "ci pipeline", "search filters", "profile page",
              "notifications", "audit log"]:
        A({"text": f"raise pr {t} from dev to main in servx", "intent": "create_pr",
           "slots": {"repo": "servx", "title": t, "head": "dev", "base": "main"}})

    # remove_collaborator (+32) — avoid: remove <u> as collaborator from|remove <u> from
    for u in USERS:
        A({"text": f"drop {u} from servx", "intent": "remove_collaborator",
           "slots": {"username": u, "repo": "servx"}})
        A({"text": f"revoke {u} access to owner/project", "intent": "remove_collaborator",
           "slots": {"username": u, "repo": "owner/project"}})
        A({"text": f"kick {u} from zync", "intent": "remove_collaborator",
           "slots": {"username": u, "repo": "zync"}})
    for u in ["kiran", "divya", "arjun", "meera", "vikram", "anaya", "kabir",
              "ishita", "rohan", "neha", "farhan", "priya", "aman", "zoya",
              "nikhil", "tara", "dev"]:
        A({"text": f"delete {u} from collaborators of shopkart", "intent": "remove_collaborator",
           "slots": {"username": u, "repo": "shopkart"}})

    # update_branch (+26) — avoid: so|sync|update branch for [pr] <n> in <repo>
    for n, r in zip([42, 7, 15, 99, 3, 21], REPOS):
        A({"text": f"update the branch for pr {n} in {r}", "intent": "update_branch",
           "slots": {"pr_number": str(n), "repo": r}})
        A({"text": f"sync pr {n} with main in {r}", "intent": "update_branch",
           "slots": {"pr_number": str(n), "repo": r}})
    for n in [5, 9, 13, 17, 25, 29, 33, 37, 45, 51, 60, 70, 80, 90]:
        A({"text": f"bring pr {n} up to date in servx", "intent": "update_branch",
           "slots": {"pr_number": str(n), "repo": "servx"}})

    # list_branches (+21) — avoid: [and|so] list all|show|get branches|what branches are in
    for r in REPOS:
        A({"text": f"list branches in {r}", "intent": "list_branches", "slots": {"repo": r}})
        A({"text": f"show all branches of {r}", "intent": "list_branches", "slots": {"repo": r}})
    for r in REPOS[:6]:
        A({"text": f"branch list for {r}", "intent": "list_branches", "slots": {"repo": r}})
    for b in BRANCHES[:3]:
        A({"text": f"does branch {b} exist in servx", "intent": "list_branches",
           "slots": {"repo": "servx", "branch": b}})

    # check_branch (+18) — avoid: check latest|the latest|new|newest|recent|show latest|what is latest
    for r in REPOS:
        A({"text": f"check the newest branch in {r}", "intent": "check_branch", "slots": {"repo": r}})
        A({"text": f"recently created branch in {r}", "intent": "check_branch", "slots": {"repo": r}})
    for u in USERS[:6]:
        A({"text": f"fresh branches by {u} in servx", "intent": "check_branch",
           "slots": {"repo": "servx", "author": u}})

    # open_url (+17) — avoid: go to|open <url>
    for u in URLS:
        A({"text": f"open {u} in browser", "intent": "open_url", "slots": {"url": u, "target": u}})
        A({"text": f"launch {u} website", "intent": "open_url", "slots": {"url": u, "target": u}})
        A({"text": f"browse to {u}", "intent": "open_url", "slots": {"url": u, "target": u}})
    for u in ["mail.google.com", "calendar.google.com", "drive.google.com",
              "maps.google.com", "youtube.com", "linkedin.com", "github.com",
              "amazon.in"]:
        A({"text": f"visit {u}", "intent": "open_url", "slots": {"url": u, "target": u}})

    # list_workflows (+15) / list_workflow_runs (+15) — avoid listed action/run families
    for r in REPOS:
        A({"text": f"show ci pipelines in {r}", "intent": "list_workflows", "slots": {"repo": r}})
        A({"text": f"ci workflows in {r}", "intent": "list_workflows", "slots": {"repo": r}})
    for r in REPOS[:3]:
        A({"text": f"actions list for {r}", "intent": "list_workflows", "slots": {"repo": r}})
    for r in REPOS:
        A({"text": f"show recent ci runs in {r}", "intent": "list_workflow_runs", "slots": {"repo": r}})
        A({"text": f"build history of {r}", "intent": "list_workflow_runs", "slots": {"repo": r}})
    for r in REPOS[:3]:
        A({"text": f"did the last build pass in {r}", "intent": "list_workflow_runs", "slots": {"repo": r}})

    # comment_pr (+14) — avoid: comment on|comment pr <n> in <repo> <body>
    bodies = ["looks good", "needs another review", "fix the typo", "add tests please"]
    for b in bodies:
        A({"text": f"comment on pr 42 in servx saying {b}", "intent": "comment_pr",
           "slots": {"pr_number": "42", "repo": "servx", "body": b}})
    for n, b in list(zip([7, 15, 23, 31, 55, 8, 12, 18, 26, 39], bodies * 3))[:10]:
        A({"text": f"leave a comment on pr {n} in zync saying {b}", "intent": "comment_pr",
           "slots": {"pr_number": str(n), "repo": "zync", "body": b}})

    return ex


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
        "source": "nexus_synthetic_phase10",
        "review_status": "approved_for_candidate_training",
        "policy": "Thin-intent top-up to >=40 rows. No new labels.",
        "total_examples": len(fresh),
        "unique_families": len({family_key(e) for e in fresh}),
        "intent_counts": dict(intent_counts.most_common()),
        "examples": fresh,
    }

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(OUTPUT_PATH, "w", encoding="utf-8") as f:
        json.dump(output, f, indent=2, ensure_ascii=False)

    print(f"Phase 10 thin top-up: {len(fresh)} fresh ({len(examples) - len(fresh)} dupes skipped)")
    for intent, count in intent_counts.most_common():
        print(f"  {intent}: {count}")
    print(f"Output: {OUTPUT_PATH}")


if __name__ == "__main__":
    main()
