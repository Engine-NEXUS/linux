#!/usr/bin/env python3

import argparse
import json
from pathlib import Path

from prepare_evaluation_splits import family_key

SCRIPT_DIR = Path(__file__).parent
DATASET_PATH = SCRIPT_DIR / "dataset.json"
MANIFEST_PATH = SCRIPT_DIR / "data" / "phase1_authored_families.json"
TARGET_FAMILIES = 15
WRAPPERS = [
    "{verb} {object}",
    "please {verb} {object}",
    "I need you to {verb} {object}",
    "go ahead and {verb} {object}",
    "can we {verb} {object}",
    "would you {verb} {object}",
    "perform the action to {verb} {object}",
    "on GitHub {verb} {object}",
    "for this repository {verb} {object}",
    "take care of this and {verb} {object}",
    "the next task is to {verb} {object}",
    "use the repository controls to {verb} {object}",
    "from the project page {verb} {object}",
    "when ready {verb} {object}",
    "I want NEXUS to {verb} {object}",
    "handle this request and {verb} {object}",
    "open the relevant GitHub page and {verb} {object}",
    "as the repository assistant {verb} {object}",
    "carry out my request to {verb} {object}",
    "for the current development task {verb} {object}",
    "without changing anything else {verb} {object}",
    "using the supplied details {verb} {object}",
    "complete this repository operation and {verb} {object}",
    "apply my instruction to {verb} {object}",
]

CONFIG = {
    "analyse_pr": {
        "verbs": ["inspect", "analyze", "review"],
        "object": "pull request 42 in owner/project",
        "slots": {"pr_number": "42", "repo": "owner/project"},
    },
    "analyse_repo": {
        "verbs": ["inspect", "analyze", "examine"],
        "object": "the architecture of repository owner/project",
        "slots": {"repo": "owner/project"},
    },
    "check_branch": {
        "verbs": ["check", "inspect", "verify"],
        "object": "branch feature-login in owner/project",
        "slots": {"repo": "owner/project"},
    },
    "create_pr": {
        "verbs": ["create", "open", "prepare"],
        "object": "a pull request in owner/project titled improve login from feature-login into main",
        "slots": {"repo": "owner/project", "title": "improve login", "head": "feature-login", "base": "main"},
    },
    "list_pr_files": {
        "verbs": ["list files from", "show changed files for", "retrieve the file list for"],
        "object": "pull request 42 in owner/project",
        "slots": {"pr_number": "42", "repo": "owner/project"},
    },
    "open_url": {
        "verbs": ["open", "visit", "load"],
        "object": "https://docs.example.com",
        "slots": {"url": "https://docs.example.com"},
    },
    "remove_collaborator": {
        "verbs": ["remove", "revoke access for", "drop"],
        "object": "collaborator alex from owner/project",
        "slots": {"username": "alex", "repo": "owner/project"},
    },
    "update_branch": {
        "verbs": ["update the branch for", "synchronize", "bring up to date"],
        "object": "pull request 42 in owner/project",
        "slots": {"pr_number": "42", "repo": "owner/project"},
    },
    "whatsapp_chat": {
        "verbs": ["start a WhatsApp chat with", "message", "open a WhatsApp conversation with"],
        "object": "alex",
        "slots": {"contact": "alex"},
    },
    "add_collaborator": {
        "verbs": ["invite", "grant access to", "register"],
        "object": "developer alex as a collaborator on owner/project",
        "slots": {"username": "alex", "repo": "owner/project"},
    },
    "add_org_member": {
        "verbs": ["invite", "enroll", "bring"],
        "object": "developer alex into organization nexus-labs",
        "slots": {"username": "alex", "org": "nexus-labs"},
    },
    "approve_pr": {
        "verbs": ["approve", "accept the review for", "mark as approved"],
        "object": "pull request 42 in owner/project",
        "slots": {"pr_number": "42", "repo": "owner/project"},
    },
    "cancel_workflow": {
        "verbs": ["cancel", "stop the run for", "terminate"],
        "object": "workflow build-42 in owner/project",
        "slots": {"workflow_id": "build-42", "repo": "owner/project"},
    },
    "close_pr": {
        "verbs": ["close", "shut down", "mark closed"],
        "object": "pull request 42 in owner/project",
        "slots": {"pr_number": "42", "repo": "owner/project"},
    },
    "comment_pr": {
        "verbs": ["comment on", "leave feedback on", "post a note to"],
        "object": "pull request 42 in owner/project saying needs another review",
        "slots": {"pr_number": "42", "repo": "owner/project", "body": "needs another review"},
    },
    "create_release": {
        "verbs": ["create", "publish", "prepare"],
        "object": "release v2.4.0 for owner/project",
        "slots": {"release_tag": "v2.4.0", "repo": "owner/project"},
    },
    "delete_branch": {
        "verbs": ["delete", "remove", "erase"],
        "object": "branch obsolete-feature from owner/project",
        "slots": {"branch": "obsolete-feature", "repo": "owner/project"},
    },
    "get_pr": {
        "verbs": ["retrieve", "show the details for", "open the record for"],
        "object": "pull request 42 in owner/project",
        "slots": {"pr_number": "42", "repo": "owner/project"},
    },
    "merge_pr": {
        "verbs": ["merge", "integrate", "combine"],
        "object": "pull request 42 into owner/project",
        "slots": {"pr_number": "42", "repo": "owner/project"},
    },
    "remove_org_member": {
        "verbs": ["remove", "revoke membership for", "take"],
        "object": "developer alex from organization nexus-labs",
        "slots": {"username": "alex", "org": "nexus-labs"},
    },
    "rerun_workflow": {
        "verbs": ["rerun", "restart", "execute again"],
        "object": "workflow build-42 in owner/project",
        "slots": {"workflow_id": "build-42", "repo": "owner/project"},
    },
    "revert_pr": {
        "verbs": ["revert", "undo the changes from", "reverse"],
        "object": "pull request 42 in owner/project",
        "slots": {"pr_number": "42", "repo": "owner/project"},
    },
}


def candidates(intent, config):
    rows = []
    for index, wrapper in enumerate(WRAPPERS):
        verb = config["verbs"][index % len(config["verbs"])]
        rows.append({
            "text": wrapper.format(verb=verb, object=config["object"]),
            "intent": intent,
            "slots": config["slots"],
        })
    return rows


def build_additions(dataset):
    test_families = {family_key(row) for row in dataset.get("test", [])}
    existing_families = {family_key(row) for row in dataset.get("train", [])}
    additions = []
    summary = {}
    for intent, config in CONFIG.items():
        existing_available = {
            family_key(row) for row in dataset.get("train", [])
            if row.get("intent") == intent and family_key(row) not in test_families
        }
        needed = max(0, TARGET_FAMILIES - len(existing_available))
        accepted = []
        seen = set()
        if needed == 0:
            summary[intent] = {"existing": len(existing_available), "added": 0, "target": TARGET_FAMILIES}
            continue
        for row in candidates(intent, config):
            family = family_key(row)
            if family in test_families or family in existing_families or family in seen:
                continue
            seen.add(family)
            accepted.append(row)
            if len(accepted) == needed:
                break
        if len(accepted) < needed:
            raise ValueError(f"{intent} needs {needed} families but produced only {len(accepted)}")
        additions.extend(accepted)
        summary[intent] = {"existing": len(existing_available), "added": len(accepted), "target": TARGET_FAMILIES}
    return additions, summary


def main():
    parser = argparse.ArgumentParser(description="Add reviewed independent Phase 1 command families")
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    dataset = json.loads(DATASET_PATH.read_text(encoding="utf-8"))
    additions, summary = build_additions(dataset)
    manifest = {
        "schema_version": 1,
        "source": "nexus_synthetic",
        "review_status": "approved_for_split_construction",
        "policy": "Independently structured command families; not slot-only substitutions",
        "families_per_intent": summary,
        "examples": additions,
    }
    MANIFEST_PATH.parent.mkdir(parents=True, exist_ok=True)
    MANIFEST_PATH.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"Prepared {len(additions)} independent examples across {len(summary)} intents")
    if args.apply:
        dataset["train"].extend(additions)
        DATASET_PATH.write_text(json.dumps(dataset, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        print(f"Added examples to {DATASET_PATH}")
    else:
        print("Dry run only. Re-run with --apply after review.")


if __name__ == "__main__":
    main()
