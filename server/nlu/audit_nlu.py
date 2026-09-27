#!/usr/bin/env python3

import argparse
import asyncio
import json
import math
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
SERVER_DIR = SCRIPT_DIR.parent
ROOT_DIR = SERVER_DIR.parent
DATASET_PATH = SCRIPT_DIR / "dataset.json"
LABELS_PATH = SCRIPT_DIR / "model" / "labels.json"
MODEL_PATH = SCRIPT_DIR / "model" / "nexus_nlu.onnx"
REPORT_JSON_PATH = SCRIPT_DIR / "audit_report.json"
REPORT_MD_PATH = ROOT_DIR / "docs" / "research" / "nlu-model-data-audit-latest.md"

REQUIRED_SLOTS = {
    "open_app": {"app_name"},
    "open_url": {"url"},
    "close_app": {"app_name"},
    "whatsapp_chat": {"contact"},
    "search": {"query"},
    "analyse_repo": {"repo"},
    "analyse_pr": {"repo", "pr_number"},
    "merge_pr": {"repo", "pr_number"},
    "approve_pr": {"repo", "pr_number"},
    "close_pr": {"repo", "pr_number"},
    "get_pr": {"repo", "pr_number"},
    "create_pr": {"repo", "title", "head", "base"},
    "update_branch": {"repo", "pr_number"},
    "revert_pr": {"repo", "pr_number"},
    "list_pr_files": {"repo", "pr_number"},
    "comment_pr": {"repo", "pr_number", "body"},
    "add_collaborator": {"repo", "username"},
    "remove_collaborator": {"repo", "username"},
    "list_collaborators": {"repo"},
    "add_org_member": {"org", "username"},
    "remove_org_member": {"org", "username"},
    "list_org_members": {"org"},
    "delete_branch": {"repo", "branch"},
    "list_branches": {"repo"},
    "create_release": {"repo", "release_tag"},
    "list_releases": {"repo"},
    "list_workflows": {"repo"},
    "list_workflow_runs": {"repo"},
    "rerun_workflow": {"repo", "workflow_id"},
    "cancel_workflow": {"repo", "workflow_id"},
    "type_text": {"text"},
    "press_key": {"key"},
    "press_hotkey": {"keys"},
    "browser_navigate": {"url"},
    "browser_search": {"query"},
    "whatsapp_search": {"contact"},
    "focus_app": {"target"},
}

OPTIONAL_SLOT_INTENTS = {"analyse_latest_pr", "check_branch", "list_prs"}

TARGETED_CASES = [
    ("type hello world", "type_text"),
    ("please type hello world", "type_text"),
    ("press enter", "press_key"),
    ("hit the escape key", "press_key"),
    ("press control shift escape", "press_hotkey"),
    ("open a new tab", "browser_new_tab"),
    ("go to github.com", "browser_navigate"),
    ("search for rust in the browser", "browser_search"),
    ("open whatsapp", "whatsapp_open"),
    ("find mom in whatsapp", "whatsapp_search"),
    ("switch to chrome", "focus_app"),
    ("send it", "confirm_send"),
    ("never mind cancel that", "cancel_action"),
    ("stop music", "media_stop"),
    ("show me pull requests in nexus", "list_prs"),
    ("analyse the latest pull request in nexus", "analyse_latest_pr"),
    ("merge pull request 12 in nexus", "merge_pr"),
    ("write an email to john", "unknown"),
    ("what is the weather tomorrow", "unknown"),
    ("delete all my files", "unknown"),
    ("open my bank account", "unknown"),
    ("send my password to john", "unknown"),
]


def normalize_text(text):
    return " ".join(str(text).lower().strip().split())


def value_in_text(value, text):
    if isinstance(value, list):
        return all(value_in_text(item, text) for item in value)
    if value in (None, ""):
        return True
    normalized_value = normalize_text(value)
    normalized_text = normalize_text(text)
    if normalized_value in normalized_text:
        return True
    return all(part in normalized_text.split() for part in normalized_value.split())


def normalize_slots(slots):
    normalized = {}
    for key, value in (slots or {}).items():
        if isinstance(value, list):
            values = [normalize_text(item).strip(".,") for item in value if normalize_text(item)]
            if values:
                normalized[key] = values
        elif value not in (None, ""):
            normalized[key] = normalize_text(value).replace(" / ", "/").replace(" - ", "-").replace(" . ", ".").strip(".,")
    return normalized


def entropy(counts):
    total = sum(counts.values())
    if not total:
        return 0.0
    result = 0.0
    for count in counts.values():
        probability = count / total
        result -= probability * math.log2(probability)
    return result


def audit_split(name, rows, intents, allowed_slots):
    counts = Counter(row.get("intent") for row in rows)
    malformed = []
    unknown_labels = []
    unknown_slots = []
    missing_required = []
    slot_value_absent = []
    empty_text = []
    exact_rows = set()
    exact_duplicates = 0
    by_text = defaultdict(set)

    for index, row in enumerate(rows):
        text = row.get("text", "")
        intent = row.get("intent", "")
        slots = row.get("slots", {})
        if not isinstance(text, str) or not text.strip():
            empty_text.append(index)
        if intent not in intents:
            unknown_labels.append({"index": index, "intent": intent, "text": text})
        if intent != intent.lower() or not re.fullmatch(r"[a-z][a-z0-9_]*", intent or ""):
            malformed.append({"index": index, "intent": intent, "text": text})
        row_key = (normalize_text(text), intent, json.dumps(slots, sort_keys=True, ensure_ascii=False))
        if row_key in exact_rows:
            exact_duplicates += 1
        exact_rows.add(row_key)
        by_text[normalize_text(text)].add(intent)
        if not isinstance(slots, dict):
            unknown_slots.append({"index": index, "slot": "<non-object>", "value": slots, "text": text})
            slots = {}
        for slot, value in slots.items():
            if slot not in allowed_slots:
                unknown_slots.append({"index": index, "slot": slot, "value": value, "text": text})
            if not value_in_text(value, text):
                slot_value_absent.append({"index": index, "intent": intent, "slot": slot, "value": value, "text": text})
        required = REQUIRED_SLOTS.get(intent, set())
        absent_required = sorted(slot for slot in required if slots.get(slot) in (None, "", []))
        if absent_required:
            missing_required.append({"index": index, "intent": intent, "missing": absent_required, "text": text})

    conflicts = [
        {"text": text, "intents": sorted(labels)}
        for text, labels in by_text.items()
        if text and len(labels) > 1
    ]
    return {
        "name": name,
        "examples": len(rows),
        "intent_count": len(counts),
        "intent_distribution": dict(sorted(counts.items())),
        "missing_intents": sorted(set(intents) - set(counts)),
        "unknown_labels": unknown_labels,
        "malformed_labels": malformed,
        "exact_duplicate_rows": exact_duplicates,
        "same_text_intent_conflicts": conflicts,
        "unknown_slots": unknown_slots,
        "missing_required_slots": missing_required,
        "slot_values_absent_from_text": slot_value_absent,
        "empty_text": empty_text,
        "min_examples_per_intent": min(counts.values()) if counts else 0,
        "max_examples_per_intent": max(counts.values()) if counts else 0,
        "imbalance_ratio": round(max(counts.values()) / min(counts.values()), 3) if counts and min(counts.values()) else None,
        "intent_entropy_bits": round(entropy(counts), 4),
    }


def audit_dataset():
    data = json.loads(DATASET_PATH.read_text(encoding="utf-8"))
    labels = json.loads(LABELS_PATH.read_text(encoding="utf-8"))
    intents = labels["intents"]
    allowed_slots = {tag[2:] for tag in labels["slots"] if tag.startswith("B-")}
    train = data.get("train", [])
    test = data.get("test", [])
    train_audit = audit_split("train", train, intents, allowed_slots)
    test_audit = audit_split("test", test, intents, allowed_slots)
    train_text = defaultdict(set)
    for row in train:
        train_text[normalize_text(row.get("text", ""))].add(row.get("intent", ""))
    overlap = []
    for index, row in enumerate(test):
        text = normalize_text(row.get("text", ""))
        if text in train_text:
            overlap.append({
                "test_index": index,
                "text": row.get("text", ""),
                "test_intent": row.get("intent", ""),
                "train_intents": sorted(train_text[text]),
            })
    return {
        "dataset_path": str(DATASET_PATH),
        "labels_path": str(LABELS_PATH),
        "defined_intents": len(intents),
        "defined_slots": len(allowed_slots),
        "train": train_audit,
        "test": test_audit,
        "train_test_exact_text_overlap": overlap,
    }


async def audit_model(dataset_report):
    if not MODEL_PATH.exists():
        return {"available": False, "reason": f"missing {MODEL_PATH}"}
    sys.path.insert(0, str(SERVER_DIR))
    import nlu_server

    nlu_server.get_session()
    nlu_server.get_tokenizer()
    data = json.loads(DATASET_PATH.read_text(encoding="utf-8"))
    test = data.get("test", [])
    predictions = []
    for row in test:
        result = await nlu_server.parse(nlu_server.ParseRequest(text=row["text"]))
        predictions.append((row, result))

    correct = sum(row["intent"] == result.intent for row, result in predictions)
    confusion = Counter(
        (row["intent"], result.intent)
        for row, result in predictions
        if row["intent"] != result.intent
    )
    per_intent = defaultdict(lambda: {"correct": 0, "total": 0, "confidences": []})
    slot_gold = 0
    slot_exact = 0
    empty_gold_clean = 0
    empty_gold_total = 0
    false_slot_examples = []
    slot_error_examples = []

    for row, result in predictions:
        stats = per_intent[row["intent"]]
        stats["total"] += 1
        stats["correct"] += int(row["intent"] == result.intent)
        stats["confidences"].append(result.confidence)
        gold_slots = normalize_slots(row.get("slots"))
        predicted_slots = normalize_slots(result.slots)
        if gold_slots:
            slot_gold += 1
            slot_exact += int(gold_slots == predicted_slots)
            if gold_slots != predicted_slots and len(slot_error_examples) < 100:
                slot_error_examples.append({
                    "text": row["text"],
                    "intent": row["intent"],
                    "gold": gold_slots,
                    "predicted": predicted_slots,
                })
        else:
            empty_gold_total += 1
            empty_gold_clean += int(not predicted_slots)
            if predicted_slots and len(false_slot_examples) < 100:
                false_slot_examples.append({
                    "text": row["text"],
                    "intent": row["intent"],
                    "predicted": predicted_slots,
                })

    per_intent_output = {}
    for intent, stats in per_intent.items():
        per_intent_output[intent] = {
            "correct": stats["correct"],
            "total": stats["total"],
            "accuracy": round(stats["correct"] / stats["total"], 4),
            "average_confidence": round(sum(stats["confidences"]) / stats["total"], 4),
        }

    targeted = []
    for text, expected in TARGETED_CASES:
        result = await nlu_server.parse(nlu_server.ParseRequest(text=text))
        targeted.append({
            "text": text,
            "expected": expected,
            "predicted": result.intent,
            "confidence": round(result.confidence, 4),
            "slots": result.slots,
            "correct": result.intent == expected,
        })

    return {
        "available": True,
        "model_path": str(MODEL_PATH),
        "test_intent_accuracy": round(correct / len(test), 4) if test else None,
        "test_correct": correct,
        "test_total": len(test),
        "per_intent": dict(sorted(per_intent_output.items())),
        "top_confusions": [
            {"gold": gold, "predicted": predicted, "count": count}
            for (gold, predicted), count in confusion.most_common(30)
        ],
        "nonempty_gold_slot_exact_match": round(slot_exact / slot_gold, 4) if slot_gold else None,
        "nonempty_gold_slot_exact_count": slot_exact,
        "nonempty_gold_slot_total": slot_gold,
        "empty_gold_without_false_slots": round(empty_gold_clean / empty_gold_total, 4) if empty_gold_total else None,
        "empty_gold_clean_count": empty_gold_clean,
        "empty_gold_total": empty_gold_total,
        "slot_error_examples": slot_error_examples,
        "false_slot_examples": false_slot_examples,
        "targeted_cases": targeted,
        "targeted_accuracy": round(sum(item["correct"] for item in targeted) / len(targeted), 4),
    }


def severity_summary(dataset, model):
    issues = []
    train = dataset["train"]
    test = dataset["test"]
    if test["missing_intents"]:
        issues.append(("critical", "Test coverage", f"{len(test['missing_intents'])} defined intents are absent from the test set"))
    if dataset["train_test_exact_text_overlap"]:
        issues.append(("critical", "Evaluation leakage", f"{len(dataset['train_test_exact_text_overlap'])} test texts also appear in training"))
    if train["same_text_intent_conflicts"]:
        issues.append(("critical", "Conflicting labels", f"{len(train['same_text_intent_conflicts'])} normalized texts have multiple intent labels"))
    if train["unknown_slots"]:
        issues.append(("high", "Invalid slot names", f"{len(train['unknown_slots'])} slot annotations use undefined slot names"))
    if train["slot_values_absent_from_text"]:
        issues.append(("high", "Unalignable slots", f"{len(train['slot_values_absent_from_text'])} slot values cannot be aligned to text"))
    if model.get("available"):
        slot_score = model.get("nonempty_gold_slot_exact_match")
        if slot_score is not None and slot_score < 0.8:
            issues.append(("critical", "Slot extraction", f"Exact match is {slot_score:.1%} on test examples with slots"))
        if model.get("targeted_accuracy", 1) < 0.95:
            issues.append(("high", "Targeted safety/OOS", f"Targeted accuracy is {model['targeted_accuracy']:.1%}"))
    return [{"severity": severity, "area": area, "finding": finding} for severity, area, finding in issues]


def render_markdown(report):
    dataset = report["dataset"]
    model = report["model"]
    lines = [
        "# NEXUS NLU Model and Dataset Audit",
        "",
        "**Generated by:** `python server/nlu/audit_nlu.py`  ",
        f"**Dataset:** `{dataset['dataset_path']}`  ",
        f"**Model:** `{model.get('model_path', 'not available')}`",
        "",
        "## Executive findings",
        "",
    ]
    for issue in report["issues"]:
        lines.append(f"- **{issue['severity'].upper()} — {issue['area']}:** {issue['finding']}.")
    lines.extend([
        "",
        "## Dataset summary",
        "",
        "| Metric | Train | Test |",
        "|---|---:|---:|",
        f"| Examples | {dataset['train']['examples']} | {dataset['test']['examples']} |",
        f"| Intents represented | {dataset['train']['intent_count']} | {dataset['test']['intent_count']} |",
        f"| Missing defined intents | {len(dataset['train']['missing_intents'])} | {len(dataset['test']['missing_intents'])} |",
        f"| Same-text conflicting labels | {len(dataset['train']['same_text_intent_conflicts'])} | {len(dataset['test']['same_text_intent_conflicts'])} |",
        f"| Unknown slot annotations | {len(dataset['train']['unknown_slots'])} | {len(dataset['test']['unknown_slots'])} |",
        f"| Slot values absent from text | {len(dataset['train']['slot_values_absent_from_text'])} | {len(dataset['test']['slot_values_absent_from_text'])} |",
        "",
        f"Train/test exact-text overlap: **{len(dataset['train_test_exact_text_overlap'])}**.",
        "",
        "### Missing test intents",
        "",
        ", ".join(f"`{intent}`" for intent in dataset["test"]["missing_intents"]) or "None",
        "",
        "### Training label conflicts",
        "",
    ])
    for conflict in dataset["train"]["same_text_intent_conflicts"]:
        lines.append(f"- `{conflict['text']}` → {', '.join(conflict['intents'])}")
    lines.extend(["", "## Model summary", ""])
    if model.get("available"):
        lines.extend([
            f"- Intent accuracy on current test set: **{model['test_intent_accuracy']:.2%}** ({model['test_correct']}/{model['test_total']})",
            f"- Exact slot match on non-empty slot examples: **{model['nonempty_gold_slot_exact_match']:.2%}** ({model['nonempty_gold_slot_exact_count']}/{model['nonempty_gold_slot_total']})",
            f"- Empty-slot examples without false predicted slots: **{model['empty_gold_without_false_slots']:.2%}** ({model['empty_gold_clean_count']}/{model['empty_gold_total']})",
            f"- Targeted command/OOS accuracy: **{model['targeted_accuracy']:.2%}**",
            "",
            "The test score measures the curated dataset, not unrestricted production speech. Treat targeted OOS and real-ASR suites as separate promotion gates.",
            "",
            "### Targeted failures",
            "",
        ])
        for case in model["targeted_cases"]:
            if not case["correct"]:
                lines.append(f"- `{case['text']}`: expected `{case['expected']}`, predicted `{case['predicted']}` ({case['confidence']:.1%})")
    else:
        lines.append(f"Model audit unavailable: {model.get('reason', 'unknown reason')}")
    lines.extend([
        "",
        "## Next remediation order",
        "",
        "1. Expand real ASR transcript tests and safety/OOS tests.",
        "2. Improve low-coverage live intents, especially hotkeys and spoken-dot browser navigation.",
        "3. Add calibrated rejection before uncertain NLU predictions can execute.",
        "4. Collect and review first-party voice transcripts rather than auto-saving bad STT output.",
        "5. Import reviewed subsets of MASSIVE, CLINC150 OOS, SLURP text, and NL2Bash after provenance tooling exists.",
        "6. Retrain, rerun this audit, and promote only when all safety gates pass.",
        "",
        "The machine-readable details are in `server/nlu/audit_report.json`.",
        "",
    ])
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description="Audit the NEXUS NLU dataset and ONNX model")
    parser.add_argument("--dataset-only", action="store_true", help="Skip ONNX inference")
    parser.add_argument("--json", type=Path, default=REPORT_JSON_PATH, help="JSON report output path")
    parser.add_argument("--markdown", type=Path, default=REPORT_MD_PATH, help="Markdown report output path")
    args = parser.parse_args()

    dataset = audit_dataset()
    model = {"available": False, "reason": "skipped by --dataset-only"}
    if not args.dataset_only:
        model = asyncio.run(audit_model(dataset))
    report = {"dataset": dataset, "model": model}
    report["issues"] = severity_summary(dataset, model)

    args.json.parent.mkdir(parents=True, exist_ok=True)
    args.markdown.parent.mkdir(parents=True, exist_ok=True)
    args.json.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    args.markdown.write_text(render_markdown(report), encoding="utf-8")

    print(f"Train examples: {dataset['train']['examples']}")
    print(f"Test examples: {dataset['test']['examples']}")
    print(f"Test missing intents: {len(dataset['test']['missing_intents'])}")
    print(f"Train label conflicts: {len(dataset['train']['same_text_intent_conflicts'])}")
    print(f"Train/test overlap: {len(dataset['train_test_exact_text_overlap'])}")
    print(f"Invalid train slot names: {len(dataset['train']['unknown_slots'])}")
    print(f"Unalignable train slot values: {len(dataset['train']['slot_values_absent_from_text'])}")
    if model.get("available"):
        print(f"Test intent accuracy: {model['test_intent_accuracy']:.2%}")
        print(f"Non-empty slot exact match: {model['nonempty_gold_slot_exact_match']:.2%}")
        print(f"Targeted accuracy: {model['targeted_accuracy']:.2%}")
    print(f"JSON report: {args.json}")
    print(f"Markdown report: {args.markdown}")


if __name__ == "__main__":
    main()
