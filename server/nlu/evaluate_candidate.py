#!/usr/bin/env python3
"""
Phase 3 — Evaluate the candidate ONNX model against all benchmarks.

Loads the candidate ONNX from `model/candidate/` directly (no server needed)
and evaluates against:
  1. Frozen final test (452 rows from candidate_dataset.json)
  2. Reviewed CLINC OOS benchmark (867 never-train rows)
  3. Supported mapping benchmark (5 rows)

Produces a JSON report at `model/candidate/candidate_evaluation_report.json`.
The production model is never loaded or modified.
"""
import json
import sys
from collections import Counter
from pathlib import Path

import numpy as np
import onnxruntime as ort
from transformers import AutoTokenizer

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

SCRIPT_DIR = Path(__file__).parent
CANDIDATE_DIR = SCRIPT_DIR / "model" / "candidate"
CANDIDATE_ONNX = CANDIDATE_DIR / "nexus_nlu_candidate.onnx"
CANDIDATE_TOKENIZER = CANDIDATE_DIR / "tokenizer"
CANDIDATE_LABELS = CANDIDATE_DIR / "labels.json"
CANDIDATE_DATASET = SCRIPT_DIR / "data" / "candidate_dataset.json"
OOS_BENCHMARK = SCRIPT_DIR / "data" / "evaluation" / "clinc150_oos_reviewed_test.jsonl"
SUPPORTED_BENCHMARK = SCRIPT_DIR / "data" / "evaluation" / "clinc150_supported_mappings.jsonl"
REPORT_PATH = CANDIDATE_DIR / "candidate_evaluation_report.json"

MAX_LEN = 64


def load_labels():
    with open(CANDIDATE_LABELS, "r") as f:
        labels = json.load(f)
    return labels["intents"], labels["slots"]


def softmax(logits):
    logits = logits - np.max(logits, axis=-1, keepdims=True)
    exp = np.exp(logits)
    return exp / np.sum(exp, axis=-1, keepdims=True)


def predict(session, tokenizer, intents, text):
    encoding = tokenizer(text, truncation=True, padding="max_length", max_length=MAX_LEN, return_tensors="np")
    input_ids = encoding["input_ids"].astype(np.int64)
    attention_mask = encoding["attention_mask"].astype(np.int64)
    outputs = session.run(None, {"input_ids": input_ids, "attention_mask": attention_mask})
    intent_logits = outputs[0][0]
    intent_probs = softmax(intent_logits)
    pred_id = int(np.argmax(intent_logits))
    confidence = float(intent_probs[pred_id])
    return intents[pred_id], confidence


def load_jsonl(path):
    rows = []
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    return rows


def evaluate_benchmark(session, tokenizer, intents, rows, threshold, expected_field="expected_intent"):
    raw_correct = 0
    gated_correct = 0
    predicted = Counter()
    failures = []
    for row in rows:
        text = row["text"]
        expected = row[expected_field]
        pred_intent, conf = predict(session, tokenizer, intents, text)
        raw_match = pred_intent == expected
        if expected == "unknown":
            gated_match = raw_match or conf < threshold
        else:
            gated_match = raw_match and conf >= threshold
        raw_correct += int(raw_match)
        gated_correct += int(gated_match)
        predicted[pred_intent] += 1
        if not gated_match:
            failures.append({
                "text": text, "expected": expected,
                "predicted": pred_intent, "confidence": round(conf, 4),
            })
    n = len(rows)
    return {
        "examples": n,
        "raw_accuracy": round(raw_correct / n, 4) if n else 0.0,
        "gated_accuracy": round(gated_correct / n, 4) if n else 0.0,
        "gated_failures": len(failures),
        "predicted_intents": dict(predicted.most_common()),
        "highest_confidence_failures": sorted(failures, key=lambda x: x["confidence"], reverse=True)[:50],
    }


def evaluate_final_test(session, tokenizer, intents, test_data):
    correct = 0
    total = len(test_data)
    per_intent = Counter()
    per_intent_correct = Counter()
    slot_total = 0
    for row in test_data:
        text = row["text"]
        expected = row["intent"]
        pred, _ = predict(session, tokenizer, intents, text)
        per_intent[expected] += 1
        if pred == expected:
            correct += 1
            per_intent_correct[expected] += 1
    per_intent_report = {}
    for intent in sorted(per_intent):
        per_intent_report[intent] = {
            "total": per_intent[intent],
            "correct": per_intent_correct[intent],
            "recall": round(per_intent_correct[intent] / per_intent[intent], 4) if per_intent[intent] else 0.0,
        }
    return {
        "examples": total,
        "accuracy": round(correct / total, 4) if total else 0.0,
        "per_intent": per_intent_report,
    }


def main():
    if not CANDIDATE_ONNX.exists():
        print(f"ERROR: {CANDIDATE_ONNX} not found. Run train_candidate.py first.", file=sys.stderr)
        return 1

    print("Loading candidate ONNX model...")
    session = ort.InferenceSession(str(CANDIDATE_ONNX))
    intents, slots = load_labels()
    tokenizer = AutoTokenizer.from_pretrained(str(CANDIDATE_TOKENIZER))
    print(f"  Intents: {len(intents)}, Slots: {len(slots)}")

    threshold = 0.85

    # 1. Frozen final test
    print("\nEvaluating candidate on frozen final test (452 rows)...")
    with open(CANDIDATE_DATASET, "r", encoding="utf-8") as f:
        dataset = json.load(f)
    test_result = evaluate_final_test(session, tokenizer, intents, dataset["test"])
    print(f"  Intent accuracy: {test_result['accuracy']:.4f}")

    # 2. Reviewed OOS benchmark
    print("\nEvaluating candidate on reviewed CLINC OOS benchmark (867 rows)...")
    oos_rows = load_jsonl(OOS_BENCHMARK)
    oos_result = evaluate_benchmark(session, tokenizer, intents, oos_rows, threshold)
    print(f"  Raw accuracy: {oos_result['raw_accuracy']:.4f}")
    print(f"  Gated accuracy (0.85): {oos_result['gated_accuracy']:.4f}")
    print(f"  Gated failures: {oos_result['gated_failures']}")

    # 3. Supported mappings
    print("\nEvaluating candidate on supported mappings (5 rows)...")
    sup_rows = load_jsonl(SUPPORTED_BENCHMARK)
    sup_result = evaluate_benchmark(session, tokenizer, intents, sup_rows, threshold)
    print(f"  Raw accuracy: {sup_result['raw_accuracy']:.4f}")
    print(f"  Gated accuracy (0.85): {sup_result['gated_accuracy']:.4f}")
    print(f"  Gated failures: {sup_result['gated_failures']}")

    report = {
        "candidate_onnx": str(CANDIDATE_ONNX.relative_to(SCRIPT_DIR.parent.parent)).replace("\\", "/"),
        "confidence_gate": threshold,
        "final_test": test_result,
        "reviewed_oos_benchmark": oos_result,
        "supported_mappings": sup_result,
    }
    with open(REPORT_PATH, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2, ensure_ascii=False)
    print(f"\nCandidate evaluation report: {REPORT_PATH}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
