#!/usr/bin/env python3

import hashlib
import json
import re
from collections import Counter
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
DATA_DIR = SCRIPT_DIR / "data"
RAW_TRAIN_PATH = DATA_DIR / "staging" / "clinc150_oos_train.jsonl"
RAW_TEST_PATH = DATA_DIR / "evaluation" / "clinc150_oos_test.jsonl"
REVIEWED_TRAIN_PATH = DATA_DIR / "staging" / "clinc150_oos_train_reviewed.jsonl"
REVIEWED_TEST_PATH = DATA_DIR / "evaluation" / "clinc150_oos_reviewed_test.jsonl"
MAPPED_TEST_PATH = DATA_DIR / "evaluation" / "clinc150_supported_mappings.jsonl"
EXCLUDED_PATH = DATA_DIR / "clinc150_review_excluded.jsonl"
REPORT_PATH = DATA_DIR / "clinc150_review_report.json"
LOCK_PATH = DATA_DIR / "external_evaluation_lock.json"

SUPPORTED_RULES = [
    ("browser_search", re.compile(r"^(?:please )?(?:search (?:the )?web|search online|google)\b", re.I)),
    ("browser_search", re.compile(r"\b(?:find|look up)\b.*\b(?:on|using) (?:the )?(?:web|internet|google)\b", re.I)),
    ("browser_new_tab", re.compile(r"^(?:please )?(?:open|create|start) (?:a |another )?(?:new |blank )?(?:browser )?tab\b", re.I)),
    ("media_next", re.compile(r"^(?:please )?(?:play |go to )?(?:the )?next (?:song|track)\b", re.I)),
    ("media_previous", re.compile(r"^(?:please )?(?:play |go to )?(?:the )?(?:previous|last) (?:song|track)\b", re.I)),
    ("media_play_pause", re.compile(r"^(?:please )?(?:play|pause|resume) (?:the )?(?:music|song|track|audio)\b", re.I)),
]

AMBIGUOUS_TERMS = re.compile(
    r"\b(?:open|close|launch|start|press|hit|type|write|enter|search|find|google|message|send|play|pause|resume|next|previous|tab|browser|website|url|app|application|whatsapp|repository|repo|pull request|workflow|branch|release|collaborator|organization|github)\b",
    re.I,
)
SENSITIVE_TERMS = re.compile(
    r"\b(?:password|passcode|pin|credit card|debit card|bank|account number|social security|secret|credential|wallet|crypto|payment|transfer money)\b",
    re.I,
)


def read_jsonl(path):
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def write_jsonl(path, rows):
    path.write_text("".join(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n" for row in rows), encoding="utf-8")


def file_sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def review_text(text):
    if SENSITIVE_TERMS.search(text):
        return "approved_unknown", "unknown", "unsupported sensitive request"
    for intent, pattern in SUPPORTED_RULES:
        if pattern.search(text):
            return "mapped_supported", intent, pattern.pattern
    if AMBIGUOUS_TERMS.search(text):
        return "excluded_ambiguous", None, "contains capability-adjacent language without an exact supported mapping"
    return "approved_unknown", "unknown", "outside the typed NEXUS capability set"


def reviewed_train_row(row, decision, mapped_intent, reason):
    result = dict(row)
    result["review_status"] = "approved" if decision == "approved_unknown" else "rejected"
    result["mapped_intent"] = mapped_intent or "unknown"
    result["mapping_version"] = "clinc-nexus-capability-v1"
    result["review_decision"] = decision
    result["review_reason"] = reason
    return result


def reviewed_test_row(row, decision, mapped_intent, reason):
    return {
        **row,
        "expected_intent": mapped_intent or "unknown",
        "review_status": "approved" if decision != "excluded_ambiguous" else "rejected",
        "review_decision": decision,
        "review_reason": reason,
        "mapping_version": "clinc-nexus-capability-v1",
    }


def review():
    raw_train = read_jsonl(RAW_TRAIN_PATH)
    raw_test = read_jsonl(RAW_TEST_PATH)
    approved_train = []
    approved_test = []
    mapped_test = []
    excluded = []
    counts = Counter()

    for split, rows in (("train", raw_train), ("test", raw_test)):
        for row in rows:
            decision, mapped_intent, reason = review_text(row["text"])
            counts[f"{split}_{decision}"] += 1
            if split == "train":
                reviewed = reviewed_train_row(row, decision, mapped_intent, reason)
                if decision == "approved_unknown":
                    approved_train.append(reviewed)
                else:
                    excluded.append({**reviewed, "source_split": split})
            else:
                reviewed = reviewed_test_row(row, decision, mapped_intent, reason)
                if decision == "approved_unknown":
                    approved_test.append(reviewed)
                elif decision == "mapped_supported":
                    mapped_test.append(reviewed)
                else:
                    excluded.append({**reviewed, "source_split": split})

    write_jsonl(REVIEWED_TRAIN_PATH, approved_train)
    write_jsonl(REVIEWED_TEST_PATH, approved_test)
    write_jsonl(MAPPED_TEST_PATH, mapped_test)
    write_jsonl(EXCLUDED_PATH, excluded)

    lock = json.loads(LOCK_PATH.read_text(encoding="utf-8"))
    lock["benchmarks"] = [item for item in lock.get("benchmarks", []) if item["id"] == "clinc150_oos_test"]
    lock["benchmarks"].extend([
        {
            "id": "clinc150_oos_reviewed_test",
            "path": "server/nlu/data/evaluation/clinc150_oos_reviewed_test.jsonl",
            "rows": len(approved_test),
            "sha256": file_sha256(REVIEWED_TEST_PATH),
            "never_train": True,
            "mapping_version": "clinc-nexus-capability-v1",
        },
        {
            "id": "clinc150_supported_mappings",
            "path": "server/nlu/data/evaluation/clinc150_supported_mappings.jsonl",
            "rows": len(mapped_test),
            "sha256": file_sha256(MAPPED_TEST_PATH),
            "never_train": True,
            "mapping_version": "clinc-nexus-capability-v1",
        },
    ])
    LOCK_PATH.write_text(json.dumps(lock, indent=2) + "\n", encoding="utf-8")

    report = {
        "schema_version": 1,
        "mapping_version": "clinc-nexus-capability-v1",
        "policy": {
            "supported": "Only narrow anchored rules representing typed NEXUS capabilities",
            "unknown": "No capability-adjacent term, or an unsupported sensitive request",
            "excluded": "Capability-adjacent but not safe to map automatically",
        },
        "input": {"train": len(raw_train), "test": len(raw_test)},
        "counts": dict(sorted(counts.items())),
        "output": {
            "approved_unknown_train": len(approved_train),
            "approved_unknown_test": len(approved_test),
            "mapped_supported_test": len(mapped_test),
            "excluded_or_ambiguous": len(excluded),
        },
        "hashes": {
            "reviewed_train": file_sha256(REVIEWED_TRAIN_PATH),
            "reviewed_test": file_sha256(REVIEWED_TEST_PATH),
            "mapped_test": file_sha256(MAPPED_TEST_PATH),
            "excluded": file_sha256(EXCLUDED_PATH),
        },
    }
    REPORT_PATH.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    review()
