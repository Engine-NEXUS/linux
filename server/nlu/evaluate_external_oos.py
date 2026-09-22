#!/usr/bin/env python3

import argparse
import asyncio
import json
import sys
from collections import Counter
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
SERVER_DIR = SCRIPT_DIR.parent
DATA_DIR = SCRIPT_DIR / "data"
DEFAULT_BENCHMARK = DATA_DIR / "evaluation" / "clinc150_oos_reviewed_test.jsonl"
DEFAULT_REPORT = DATA_DIR / "clinc150_oos_reviewed_baseline.json"


def load_rows(path):
    rows = []
    with path.open("r", encoding="utf-8") as handle:
        for line in handle:
            if line.strip():
                row = json.loads(line)
                if not row.get("never_train"):
                    raise ValueError("benchmark contains a trainable row")
                if row.get("review_status") != "approved":
                    raise ValueError("benchmark contains a row without approved review status")
                if not row.get("expected_intent"):
                    raise ValueError("benchmark row lacks expected_intent")
                rows.append(row)
    return rows


async def evaluate(benchmark, report_path, threshold):
    sys.path.insert(0, str(SERVER_DIR))
    import nlu_server

    nlu_server.get_session()
    nlu_server.get_tokenizer()
    rows = load_rows(benchmark)
    raw_correct = 0
    gated_correct = 0
    predicted_intents = Counter()
    failures = []
    for row in rows:
        result = await nlu_server.parse(nlu_server.ParseRequest(text=row["text"]))
        expected = row["expected_intent"]
        raw_match = result.intent == expected
        if expected == "unknown":
            gated_match = raw_match or result.confidence < threshold
        else:
            gated_match = raw_match and result.confidence >= threshold
        raw_correct += int(raw_match)
        gated_correct += int(gated_match)
        predicted_intents[result.intent] += 1
        if not gated_match:
            failures.append({
                "text": row["text"],
                "expected": expected,
                "predicted": result.intent,
                "confidence": round(result.confidence, 4),
            })
    report = {
        "benchmark": str(benchmark.relative_to(SCRIPT_DIR.parent.parent)).replace("\\", "/"),
        "examples": len(rows),
        "review_status": "approved_nexus_capability_v1",
        "confidence_gate": threshold,
        "raw_accuracy": round(raw_correct / len(rows), 4),
        "gated_accuracy": round(gated_correct / len(rows), 4),
        "gated_failure_count": len(failures),
        "predicted_intents": dict(predicted_intents.most_common()),
        "highest_confidence_failures": sorted(failures, key=lambda item: item["confidence"], reverse=True)[:100],
    }
    report_path.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"Reviewed benchmark examples: {len(rows)}")
    print(f"Raw accuracy: {report['raw_accuracy']:.2%}")
    print(f"Accuracy with {threshold:.2f} confidence gate: {report['gated_accuracy']:.2%}")
    print(f"Gated failures: {len(failures)}")
    print(f"Report: {report_path}")


def main():
    parser = argparse.ArgumentParser(description="Evaluate NEXUS on a reviewed never-train external benchmark")
    parser.add_argument("--threshold", type=float, default=0.85)
    parser.add_argument("--benchmark", type=Path, default=DEFAULT_BENCHMARK)
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    args = parser.parse_args()
    if not 0.0 <= args.threshold <= 1.0:
        parser.error("threshold must be between 0 and 1")
    asyncio.run(evaluate(args.benchmark.resolve(), args.report.resolve(), args.threshold))


if __name__ == "__main__":
    main()
