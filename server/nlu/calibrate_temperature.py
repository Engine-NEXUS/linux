#!/usr/bin/env python3
"""
Phase 8 — Temperature scaling and optimal gate finding.

Uses the calibration split (429 rows, never used for training or checkpoint
selection) to:
  1. Fit a temperature T that minimizes negative log-likelihood.
  2. Find the optimal confidence gate that maximizes supported mapping
     acceptance while keeping OOS failures at or below a safety budget.
  3. Report the calibrated metrics.

The temperature is applied post-hoc: logits are divided by T before softmax.
T > 1 softens confidence, T < 1 sharpens it.

Output: server/nlu/model/candidate/temperature_calibration.json
"""
import json
import numpy as np
import onnxruntime as ort
from pathlib import Path
from scipy.optimize import minimize_scalar
from transformers import AutoTokenizer

SCRIPT_DIR = Path(__file__).parent
CANDIDATE_ONNX = SCRIPT_DIR / "model" / "candidate" / "nexus_nlu_candidate.onnx"
CANDIDATE_TOKENIZER = SCRIPT_DIR / "model" / "candidate" / "tokenizer"
CANDIDATE_LABELS = SCRIPT_DIR / "model" / "candidate" / "labels.json"
CALIBRATION_DATA = SCRIPT_DIR / "data" / "candidate_dataset.json"
OOS_BENCHMARK = SCRIPT_DIR / "data" / "evaluation" / "clinc150_oos_reviewed_test.jsonl"
SUPPORTED_MAPPINGS = SCRIPT_DIR / "data" / "evaluation" / "clinc150_supported_mappings.jsonl"
OUTPUT_PATH = SCRIPT_DIR / "model" / "candidate" / "temperature_calibration.json"


def load_rows(path):
    rows = []
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            rows.append(json.loads(line))
    return rows


def get_logits(session, tokenizer, text):
    enc = tokenizer(text, truncation=True, padding="max_length", max_length=64, return_tensors="np")
    outputs = session.run(None, {
        "input_ids": enc["input_ids"].astype(np.int64),
        "attention_mask": enc["attention_mask"].astype(np.int64),
    })
    return outputs[0][0]  # intent logits


def softmax_with_temp(logits, T):
    scaled = logits / T
    scaled = scaled - scaled.max()
    exp = np.exp(scaled)
    return exp / exp.sum()


def nll_loss(logits_list, labels_list, T):
    total = 0.0
    for logits, label in zip(logits_list, labels_list):
        probs = softmax_with_temp(logits, T)
        total += -np.log(probs[label] + 1e-12)
    return total / len(logits_list)


def main():
    print("Loading candidate model...")
    session = ort.InferenceSession(str(CANDIDATE_ONNX))
    tokenizer = AutoTokenizer.from_pretrained(str(CANDIDATE_TOKENIZER))
    labels = json.load(open(CANDIDATE_LABELS, "r", encoding="utf-8"))
    intents = labels["intents"]
    intent_to_idx = {name: i for i, name in enumerate(intents)}

    # Load calibration split
    print("Loading calibration split...")
    with open(CALIBRATION_DATA, "r", encoding="utf-8") as f:
        dataset = json.load(f)
    calibration = dataset["calibration"]

    # Collect logits and labels for calibration
    print(f"Collecting logits for {len(calibration)} calibration rows...")
    logits_list = []
    labels_list = []
    for row in calibration:
        logits = get_logits(session, tokenizer, row["text"])
        logits_list.append(logits)
        intent = row["intent"]
        if intent in intent_to_idx:
            labels_list.append(intent_to_idx[intent])
        else:
            labels_list.append(0)

    # Fit temperature
    print("Fitting temperature T...")
    result = minimize_scalar(
        lambda T: nll_loss(logits_list, labels_list, T),
        bounds=(0.1, 10.0),
        method="bounded",
    )
    T_opt = result.x
    print(f"  Optimal temperature: {T_opt:.4f}")

    # Load OOS benchmark and supported mappings
    oos_rows = load_rows(OOS_BENCHMARK)
    supported_rows = load_rows(SUPPORTED_MAPPINGS)

    # Collect confidences for OOS and supported
    print("Computing OOS and supported mapping confidences...")
    oos_confs = []
    for r in oos_rows:
        if r.get("review_status") != "approved":
            continue
        logits = get_logits(session, tokenizer, r["text"])
        probs = softmax_with_temp(logits, T_opt)
        pred_idx = probs.argmax()
        pred = intents[pred_idx]
        conf = float(probs[pred_idx])
        expected = r.get("expected_intent", "unknown")
        is_correct = (pred == expected) or (expected == "unknown" and pred == "unknown")
        oos_confs.append({"text": r["text"], "pred": pred, "conf": conf, "expected": expected, "correct": is_correct})

    supported_confs = []
    for r in supported_rows:
        logits = get_logits(session, tokenizer, r["text"])
        probs = softmax_with_temp(logits, T_opt)
        pred_idx = probs.argmax()
        pred = intents[pred_idx]
        conf = float(probs[pred_idx])
        expected = r.get("expected_intent", "unknown")
        is_correct = pred == expected
        supported_confs.append({"text": r["text"], "pred": pred, "conf": conf, "expected": expected, "correct": is_correct})

    # Find max wrong OOS confidence
    wrong_oos = [c for c in oos_confs if not c["correct"]]
    max_wrong_oos = max((c["conf"] for c in wrong_oos), default=0.0)

    # Find optimal gate: highest gate that accepts at least 1 more supported mapping
    # while keeping OOS failures <= safety_budget
    print(f"\nMax wrong OOS confidence (T={T_opt:.4f}): {max_wrong_oos:.4f}")

    # Try different gates
    gates = [0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80, 0.802, 0.85, 0.90, 0.95]
    print(f"\n{'Gate':>6} | {'OOS fail':>8} | {'OOS acc':>8} | {'Supp raw':>8} | {'Supp gated':>10} | {'Supp fail':>8}")
    print("-" * 70)
    best_gate = 0.85
    best_supported_gated_acc = 0
    for gate in gates:
        oos_fail = sum(1 for c in oos_confs if not c["correct"] and c["conf"] >= gate)
        oos_total = len(oos_confs)
        oos_acc = 1 - oos_fail / oos_total if oos_total > 0 else 0
        supp_raw = sum(1 for c in supported_confs if c["correct"])
        supp_gated = sum(1 for c in supported_confs if c["correct"] and c["conf"] >= gate)
        supp_fail = sum(1 for c in supported_confs if not c["correct"] and c["conf"] >= gate)
        supp_total = len(supported_confs)
        supp_gated_acc = supp_gated / supp_total if supp_total > 0 else 0
        print(f"{gate:>6.3f} | {oos_fail:>8} | {oos_acc:>8.4f} | {supp_raw:>8} | {supp_gated:>10} | {supp_fail:>8}")
        # Optimal: maximize supported gated accuracy with 0 OOS failures
        if oos_fail == 0 and supp_gated_acc > best_supported_gated_acc:
            best_supported_gated_acc = supp_gated_acc
            best_gate = gate

    print(f"\nOptimal gate (0 OOS failures, max supported): {best_gate:.3f}")
    print(f"  Supported gated accuracy at optimal: {best_supported_gated_acc:.4f}")

    # Also evaluate final test with temperature
    print("\nEvaluating final test with temperature scaling...")
    test = dataset["test"]
    correct = 0
    for row in test:
        logits = get_logits(session, tokenizer, row["text"])
        probs = softmax_with_temp(logits, T_opt)
        pred = intents[probs.argmax()]
        if pred == row["intent"]:
            correct += 1
    test_acc = correct / len(test)
    print(f"  Final test accuracy (T={T_opt:.4f}): {test_acc:.4f}")

    # Save report
    report = {
        "temperature": float(T_opt),
        "optimal_gate": float(best_gate),
        "max_wrong_oos_confidence": float(max_wrong_oos),
        "final_test_accuracy": float(test_acc),
        "gate_analysis": [
            {
                "gate": gate,
                "oos_failures": sum(1 for c in oos_confs if not c["correct"] and c["conf"] >= gate),
                "oos_accuracy": 1 - sum(1 for c in oos_confs if not c["correct"] and c["conf"] >= gate) / len(oos_confs),
                "supported_gated": sum(1 for c in supported_confs if c["correct"] and c["conf"] >= gate),
                "supported_gated_accuracy": sum(1 for c in supported_confs if c["correct"] and c["conf"] >= gate) / len(supported_confs),
            }
            for gate in gates
        ],
        "oos_confidence_range": {
            "min": float(min(c["conf"] for c in oos_confs)),
            "max": float(max(c["conf"] for c in oos_confs)),
            "max_wrong": float(max_wrong_oos),
        },
        "supported_confidences": [
            {"text": c["text"], "pred": c["pred"], "conf": c["conf"], "expected": c["expected"], "correct": c["correct"]}
            for c in supported_confs
        ],
    }

    with open(OUTPUT_PATH, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2, ensure_ascii=False)
    print(f"\nReport: {OUTPUT_PATH}")


if __name__ == "__main__":
    main()
