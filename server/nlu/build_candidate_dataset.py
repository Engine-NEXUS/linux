#!/usr/bin/env python3
"""
Phase 3/4 — Build a candidate NLU dataset for the OOS training experiment.

This script creates `server/nlu/data/candidate_dataset.json` by:
  1. Copying the locked train/validation/calibration/test splits verbatim.
  2. Appending the 85 reviewed and approved CLINC OOS training rows to the
     train split, mapped to the `unknown` intent with empty slots.
  3. Appending the Phase 4 expanded families (structurally diverse examples for
     13 weak GitHub intents + 80 additional OOS examples).
  4. Verifying that the locked validation/calibration/test hashes are unchanged.
  5. Verifying that no candidate row overlaps any active split by normalized text.

The production `dataset.json` is never modified. The candidate dataset is
written to a separate path and used only by the candidate training script.
"""
import hashlib
import json
import re
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
PRODUCTION_DATASET = SCRIPT_DIR / "dataset.json"
CANDIDATE_DATASET = SCRIPT_DIR / "data" / "candidate_dataset.json"
SPLIT_LOCK = SCRIPT_DIR / "data" / "split_lock.json"
REVIEWED_OOS_TRAIN = SCRIPT_DIR / "data" / "staging" / "clinc150_oos_train_reviewed.jsonl"
PHASE4_FAMILIES = SCRIPT_DIR / "data" / "phase4_expanded_families.json"
PHASE5_FAMILIES = SCRIPT_DIR / "data" / "phase5_verb_aligned_families.json"
PHASE6_FAMILIES = SCRIPT_DIR / "data" / "phase6_targeted_families.json"
PHASE7_FAMILIES = SCRIPT_DIR / "data" / "phase7_get_pr_closing_families.json"
PHASE8_FAMILIES = SCRIPT_DIR / "data" / "phase8_commerce_social.json"
PHASE9_FAMILIES = SCRIPT_DIR / "data" / "phase9_mcp_verbs.json"
PHASE10_FAMILIES = SCRIPT_DIR / "data" / "phase10_thin_topup.json"
PHASE11_FAMILIES = SCRIPT_DIR / "data" / "phase11_pr_verbs.json"

FILLERS = {"please", "could", "would", "you", "kindly", "can"}


def row_key(row):
    return json.dumps({
        "text": normalize_text(row.get("text", "")),
        "intent": row.get("intent", ""),
        "slots": row.get("slots", {}),
    }, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def sha256_json_rows(rows):
    payload = "\n".join(row_key(row) for row in rows)
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def normalize_text(text):
    return " ".join(str(text).lower().strip().split())


def family_text(row):
    text = normalize_text(row.get("text", ""))
    values = []
    for slot, value in sorted((row.get("slots") or {}).items()):
        slot_values = value if isinstance(value, list) else [value]
        for item in slot_values:
            item_text = normalize_text(item)
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


def load_reviewed_oos():
    rows = []
    with REVIEWED_OOS_TRAIN.open("r", encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            row = json.loads(line)
            if row.get("review_status") != "approved":
                raise ValueError(f"staging row is not approved: {row.get('source_record_id')}")
            if row.get("mapped_intent") != "unknown":
                raise ValueError(f"staging row is not mapped to unknown: {row.get('source_record_id')}")
            if row.get("split") != "train":
                raise ValueError(f"staging row is not marked train: {row.get('source_record_id')}")
            rows.append({
                "text": row["text"],
                "intent": "unknown",
                "slots": {},
                "source": "clinc150",
                "source_record_id": row.get("source_record_id"),
                "review_status": "approved",
            })
    return rows


def verify_locked_splits(dataset, lock):
    # Only validation/calibration/test must remain unchanged; train is
    # intentionally extended with the reviewed OOS candidates.
    splits = ("validation", "calibration", "test")
    for name in splits:
        expected = lock["splits"][name]["sha256"]
        actual = sha256_json_rows(dataset[name])
        if actual != expected:
            raise ValueError(
                f"split {name} hash mismatch after candidate build: "
                f"expected {expected} got {actual}"
            )


def verify_no_overlap(candidate_rows, active_rows):
    active_texts = {normalize_text(row["text"]) for row in active_rows}
    for row in candidate_rows:
        text = normalize_text(row["text"])
        if text in active_texts:
            raise ValueError(f"candidate OOS row overlaps active split: {row['text']}")


def load_phase4_families():
    if not PHASE4_FAMILIES.exists():
        raise FileNotFoundError(f"Phase 4 families not found: {PHASE4_FAMILIES}")
    with PHASE4_FAMILIES.open("r", encoding="utf-8") as handle:
        data = json.load(handle)
    return [
        {"text": ex["text"], "intent": ex["intent"], "slots": ex.get("slots", {})}
        for ex in data["examples"]
    ]


def load_phase5_families():
    if not PHASE5_FAMILIES.exists():
        raise FileNotFoundError(f"Phase 5 families not found: {PHASE5_FAMILIES}")
    with PHASE5_FAMILIES.open("r", encoding="utf-8") as handle:
        data = json.load(handle)
    return [
        {"text": ex["text"], "intent": ex["intent"], "slots": ex.get("slots", {})}
        for ex in data["examples"]
    ]


def load_phase6_families():
    if not PHASE6_FAMILIES.exists():
        raise FileNotFoundError(f"Phase 6 families not found: {PHASE6_FAMILIES}")
    with PHASE6_FAMILIES.open("r", encoding="utf-8") as handle:
        data = json.load(handle)
    return [
        {"text": ex["text"], "intent": ex["intent"], "slots": ex.get("slots", {})}
        for ex in data["examples"]
    ]


def load_phase7_families():
    if not PHASE7_FAMILIES.exists():
        raise FileNotFoundError(f"Phase 7 families not found: {PHASE7_FAMILIES}")
    with PHASE7_FAMILIES.open("r", encoding="utf-8") as handle:
        data = json.load(handle)
    return [
        {"text": ex["text"], "intent": ex["intent"], "slots": ex.get("slots", {})}
        for ex in data["examples"]
    ]


def load_phase8_families():
    if not PHASE8_FAMILIES.exists():
        raise FileNotFoundError(f"Phase 8 families not found: {PHASE8_FAMILIES}")
    with PHASE8_FAMILIES.open("r", encoding="utf-8") as handle:
        data = json.load(handle)
    return [
        {"text": ex["text"], "intent": ex["intent"], "slots": ex.get("slots", {})}
        for ex in data["examples"]
    ]


def load_phase9_families():
    if not PHASE9_FAMILIES.exists():
        raise FileNotFoundError(f"Phase 9 families not found: {PHASE9_FAMILIES}")
    with PHASE9_FAMILIES.open("r", encoding="utf-8") as handle:
        data = json.load(handle)
    return [
        {"text": ex["text"], "intent": ex["intent"], "slots": ex.get("slots", {})}
        for ex in data["examples"]
    ]


def load_phase10_families():
    if not PHASE10_FAMILIES.exists():
        raise FileNotFoundError(f"Phase 10 families not found: {PHASE10_FAMILIES}")
    with PHASE10_FAMILIES.open("r", encoding="utf-8") as handle:
        data = json.load(handle)
    return [
        {"text": ex["text"], "intent": ex["intent"], "slots": ex.get("slots", {})}
        for ex in data["examples"]
    ]


def load_phase11_families():
    if not PHASE11_FAMILIES.exists():
        raise FileNotFoundError(f"Phase 11 families not found: {PHASE11_FAMILIES}")
    with PHASE11_FAMILIES.open("r", encoding="utf-8") as handle:
        data = json.load(handle)
    return [
        {"text": ex["text"], "intent": ex["intent"], "slots": ex.get("slots", {})}
        for ex in data["examples"]
    ]


def verify_no_family_overlap(new_rows, test_rows):
    test_families = {family_key(r) for r in test_rows}
    for row in new_rows:
        fk = family_key(row)
        if fk in test_families:
            raise ValueError(f"new row shares phrase family with frozen test: {row['text']} -> {fk}")


def main():
    if not PRODUCTION_DATASET.exists():
        print(f"ERROR: {PRODUCTION_DATASET} not found", file=sys.stderr)
        return 1
    if not REVIEWED_OOS_TRAIN.exists():
        print(f"ERROR: {REVIEWED_OOS_TRAIN} not found", file=sys.stderr)
        return 1
    if not SPLIT_LOCK.exists():
        print(f"ERROR: {SPLIT_LOCK} not found", file=sys.stderr)
        return 1
    if not PHASE4_FAMILIES.exists():
        print(f"ERROR: {PHASE4_FAMILIES} not found", file=sys.stderr)
        return 1
    if not PHASE5_FAMILIES.exists():
        print(f"ERROR: {PHASE5_FAMILIES} not found", file=sys.stderr)
        return 1
    if not PHASE6_FAMILIES.exists():
        print(f"ERROR: {PHASE6_FAMILIES} not found", file=sys.stderr)
        return 1
    if not PHASE7_FAMILIES.exists():
        print(f"ERROR: {PHASE7_FAMILIES} not found", file=sys.stderr)
        return 1
    if not PHASE8_FAMILIES.exists():
        print(f"ERROR: {PHASE8_FAMILIES} not found", file=sys.stderr)
        return 1
    if not PHASE9_FAMILIES.exists():
        print(f"ERROR: {PHASE9_FAMILIES} not found", file=sys.stderr)
        return 1
    if not PHASE10_FAMILIES.exists():
        print(f"ERROR: {PHASE10_FAMILIES} not found", file=sys.stderr)
        return 1

    with PRODUCTION_DATASET.open("r", encoding="utf-8") as handle:
        production = json.load(handle)
    with SPLIT_LOCK.open("r", encoding="utf-8") as handle:
        lock = json.load(handle)

    reviewed = load_reviewed_oos()
    print(f"Reviewed OOS training candidates: {len(reviewed)}")

    phase4 = load_phase4_families()
    print(f"Phase 4 expanded families: {len(phase4)}")

    phase5 = load_phase5_families()
    print(f"Phase 5 verb-aligned families: {len(phase5)}")

    phase6 = load_phase6_families()
    print(f"Phase 6 targeted families: {len(phase6)}")

    phase7 = load_phase7_families()
    print(f"Phase 7 get_pr closing families: {len(phase7)}")

    phase8 = load_phase8_families()
    print(f"Phase 8 commerce+social families: {len(phase8)}")

    phase9 = load_phase9_families()
    print(f"Phase 9 MCP verb alternates: {len(phase9)}")

    phase10 = load_phase10_families()
    print(f"Phase 10 thin top-up: {len(phase10)}")

    phase11 = load_phase11_families()
    print(f"Phase 11 PR verbs: {len(phase11)}")

    candidate = {
        "train": list(production["train"]),
        "validation": list(production["validation"]),
        "calibration": list(production["calibration"]),
        "test": list(production["test"]),
    }
    if "quarantine" in production:
        candidate["quarantine"] = list(production["quarantine"])

    active_rows = (
        candidate["train"] + candidate["validation"]
        + candidate["calibration"] + candidate["test"]
    )
    active_texts = {normalize_text(row["text"]) for row in active_rows}

    def dedupe_new(rows, label):
        """Skip rows already present in active splits (idempotent rebuild).

        Production train already contains the CLINC OOS + phase4-7 merges,
        so re-adding them would duplicate rows. Only genuinely new texts
        are appended; skipped counts are logged for auditability.
        """
        new_rows = [r for r in rows if normalize_text(r["text"]) not in active_texts]
        skipped = len(rows) - len(new_rows)
        if skipped:
            print(f"  (skipped {skipped} {label} rows already in active splits)")
        return new_rows

    reviewed_new = dedupe_new(reviewed, "CLINC OOS")
    phase4_new = dedupe_new(phase4, "Phase 4")
    phase5_new = dedupe_new(phase5, "Phase 5")
    phase6_new = dedupe_new(phase6, "Phase 6")
    phase7_new = dedupe_new(phase7, "Phase 7")
    phase8_new = dedupe_new(phase8, "Phase 8")
    phase9_new = dedupe_new(phase9, "Phase 9")
    phase10_new = dedupe_new(phase10, "Phase 10")
    phase11_new = dedupe_new(phase11, "Phase 11")
    verify_no_family_overlap(
        reviewed_new + phase4_new + phase5_new + phase6_new + phase7_new + phase8_new + phase9_new + phase10_new + phase11_new,
        candidate["test"],
    )

    candidate["train"].extend(reviewed_new)
    candidate["train"].extend(phase4_new)
    candidate["train"].extend(phase5_new)
    candidate["train"].extend(phase6_new)
    candidate["train"].extend(phase7_new)
    candidate["train"].extend(phase8_new)
    candidate["train"].extend(phase9_new)
    candidate["train"].extend(phase10_new)
    candidate["train"].extend(phase11_new)

    verify_locked_splits(candidate, lock)

    CANDIDATE_DATASET.parent.mkdir(parents=True, exist_ok=True)
    with CANDIDATE_DATASET.open("w", encoding="utf-8") as handle:
        json.dump(candidate, handle, indent=2, ensure_ascii=False)

    train_hash = sha256_json_rows(candidate["train"])
    print(f"Candidate train rows: {len(candidate['train'])} (was {len(production['train'])})")
    print(f"  + {len(reviewed)} CLINC OOS rows")
    print(f"  + {len(phase4)} Phase 4 expanded families")
    print(f"  + {len(phase5)} Phase 5 verb-aligned families")
    print(f"  + {len(phase6)} Phase 6 targeted families")
    print(f"  + {len(phase7)} Phase 7 get_pr closing families")
    print(f"  + {len(phase8)} Phase 8 commerce+social families")
    print(f"  + {len(phase9)} Phase 9 MCP verb alternates")
    print(f"  + {len(phase10)} Phase 10 thin top-up")
    print(f"  + {len(phase11)} Phase 11 PR verbs")
    print(f"Candidate train sha256: {train_hash}")
    print(f"Validation rows: {len(candidate['validation'])} (unchanged)")
    print(f"Calibration rows: {len(candidate['calibration'])} (unchanged)")
    print(f"Test rows: {len(candidate['test'])} (unchanged)")
    print(f"Candidate dataset: {CANDIDATE_DATASET}")
    print("Locked split hashes verified: validation/calibration/test unchanged")
    print("Phrase family overlap with frozen test: verified (none)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
