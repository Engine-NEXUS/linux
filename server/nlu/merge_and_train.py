#!/usr/bin/env python3
"""
NEXUS NLU — Merge approved phrasings + retrain BERT-Mini.

This script is called automatically by brain_monitor.rs when 50 new
approved phrasings have been collected. It:

  1. Reads approved_phrasings.jsonl
  2. Merges into dataset.json
  3. Deduplicates
  4. Balances classes
  5. Runs train.py
  6. Compares new model test accuracy vs old
  7. Hot-swaps if new >= old, discards if new < old (rollback safety)
  8. Clears approved_phrasings.jsonl

Usage:
  python merge_and_train.py
"""

import json
import os
import shutil
import subprocess
import sys
from collections import Counter
from pathlib import Path

# ─── Paths ──────────────────────────────────────────────────────────────────

SCRIPT_DIR = Path(__file__).parent
DATASET_PATH = SCRIPT_DIR / "dataset.json"
# Admin data lives in server/admin/data/ (gitignored, admin-only)
ADMIN_DATA_DIR = SCRIPT_DIR.parent / "admin" / "data"
APPROVED_PATH = ADMIN_DATA_DIR / "approved_phrasings.jsonl"
REJECTED_PATH = ADMIN_DATA_DIR / "rejected_examples.jsonl"
MODEL_PATH = SCRIPT_DIR / "model" / "nexus_nlu.onnx"
MODEL_DATA_PATH = SCRIPT_DIR / "model" / "nexus_nlu.onnx.data"
BACKUP_MODEL_PATH = SCRIPT_DIR / "model" / "nexus_nlu.onnx.bak"
BACKUP_MODEL_DATA_PATH = SCRIPT_DIR / "model" / "nexus_nlu.onnx.data.bak"
TRAIN_SCRIPT = SCRIPT_DIR / "train.py"

# ─── Functions ──────────────────────────────────────────────────────────────

def read_approved_phrasings():
    """Read approved phrasings from the JSONL file."""
    if not APPROVED_PATH.exists():
        return []
    examples = []
    with open(APPROVED_PATH, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                entry = json.loads(line)
                examples.append({
                    "text": entry["text"],
                    "intent": entry["intent"],
                    "slots": entry.get("slots", {}),
                })
            except (json.JSONDecodeError, KeyError):
                continue
    return examples


def read_rejected_examples():
    """Read rejected (bad) examples from the JSONL file.
    
    These are examples that executed wrongly or were marked as bad
    by the admin. They are REMOVED from the dataset before retraining.
    """
    if not REJECTED_PATH.exists():
        return []
    rejected = []
    with open(REJECTED_PATH, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                entry = json.loads(line)
                rejected.append({
                    "text": entry.get("text", ""),
                    "intent": entry.get("intent", ""),
                })
            except (json.JSONDecodeError, KeyError):
                continue
    return rejected


def _levenshtein(a, b):
    """Compute Levenshtein edit distance between two strings."""
    if a == b:
        return 0
    if not a:
        return len(b)
    if not b:
        return len(a)
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        curr = [i]
        for j, cb in enumerate(b, 1):
            curr.append(min(
                prev[j] + 1,
                curr[j - 1] + 1,
                prev[j - 1] + (ca != cb),
            ))
        prev = curr
    return prev[-1]


def _fuzzy_match(text, rejected_text, threshold=0.8):
    """Check if text is similar to a rejected text using normalized Levenshtein.
    
    threshold is the minimum similarity ratio (0-1) to consider a match.
    0.8 means 80% similar — catches "close chrome browser" vs "close the chrome"
    while not over-matching unrelated commands.
    """
    text = text.lower().strip()
    rej = rejected_text.lower().strip()
    if text == rej:
        return True
    max_len = max(len(text), len(rej))
    if max_len == 0:
        return True
    dist = _levenshtein(text, rej)
    similarity = 1.0 - (dist / max_len)
    return similarity >= threshold


def filter_rejected(examples, rejected):
    """Remove examples that match rejected entries.
    
    Matching is by (text, intent) — both exact and fuzzy:
    - Exact: same normalized text + same intent
    - Fuzzy: text similarity >= 0.8 AND same intent
      (catches "close chrome browser" vs "close the chrome" for the same intent)
    """
    if not rejected:
        return examples
    # Group rejected by intent for faster fuzzy matching
    rejected_by_intent = {}
    for r in rejected:
        intent = r["intent"]
        rejected_by_intent.setdefault(intent, []).append(r["text"].lower().strip())
    
    result = []
    for ex in examples:
        ex_text = ex["text"].lower().strip()
        ex_intent = ex["intent"]
        rej_texts = rejected_by_intent.get(ex_intent, [])
        
        matched = False
        for rej_text in rej_texts:
            if ex_text == rej_text or _fuzzy_match(ex_text, rej_text, threshold=0.8):
                matched = True
                break
        
        if not matched:
            result.append(ex)
    return result


def load_dataset():
    """Load the current dataset.json."""
    if not DATASET_PATH.exists():
        return {"train": [], "test": []}
    with open(DATASET_PATH, "r", encoding="utf-8") as f:
        return json.load(f)


def deduplicate(examples):
    """Remove duplicate examples (same text + intent)."""
    seen = set()
    result = []
    for ex in examples:
        key = (ex["text"].lower().strip(), ex["intent"])
        if key not in seen:
            seen.add(key)
            result.append(ex)
    return result


def balance_classes(examples, max_per_class=80):
    """Cap examples per class to prevent any single intent from dominating."""
    by_intent = {}
    for ex in examples:
        by_intent.setdefault(ex["intent"], []).append(ex)
    result = []
    for intent, items in by_intent.items():
        if len(items) > max_per_class:
            # Keep the first max_per_class (preserves original ordering)
            result.extend(items[:max_per_class])
        else:
            result.extend(items)
    return result


def check_floors(examples, floor=30):
    """Report intents below the minimum-sample floor.

    Non-blocking (warn only): a thin intent trains poorly, and the report
    tells the user exactly what to collect next (`nexus collect` resumes
    at the weakest intents automatically). Returns the weak list.
    """
    from collections import Counter
    counts = Counter(ex.get("intent", "") for ex in examples)
    weak = sorted((i, c) for i, c in counts.items() if c < floor)
    if weak:
        print("[RETRAIN] WARNING — intents below floor "
              f"({floor} rows): " + ", ".join(f"{i}={c}" for i, c in weak))
        print("[RETRAIN] Run `nexus collect --status` to see coverage, "
              "then `nexus collect` to fill the gaps (resumes at weakest).")
    else:
        print(f"[RETRAIN] Floor check passed: all intents >= {floor} rows.")
    return weak


def merge_and_save(approved, rejected):
    """Merge approved phrasings into the dataset and remove rejected ones."""
    dataset = load_dataset()
    train = dataset.get("train", [])
    test = dataset.get("test", [])

    # Remove rejected examples from BOTH train and test
    train = filter_rejected(train, rejected)
    test = filter_rejected(test, rejected)

    # Add approved phrasings to training data
    train.extend(approved)

    # Deduplicate
    train = deduplicate(train)

    # Balance classes
    train = balance_classes(train, max_per_class=80)

    # Floor gate (warn-only): thin intents train poorly — report exactly
    # what `nexus collect` should fill next.
    check_floors(train)

    # Shuffle (deterministic seed for reproducibility)
    import random
    random.seed(42)
    random.shuffle(train)

    dataset["train"] = train
    dataset["test"] = test
    with open(DATASET_PATH, "w", encoding="utf-8") as f:
        json.dump(dataset, f, indent=2, ensure_ascii=False)

    return len(train)


def backup_model():
    """Backup the current model so we can rollback if the new one is worse."""
    if MODEL_PATH.exists():
        shutil.copy2(MODEL_PATH, BACKUP_MODEL_PATH)
    if MODEL_DATA_PATH.exists():
        shutil.copy2(MODEL_DATA_PATH, BACKUP_MODEL_DATA_PATH)


def restore_model():
    """Restore the backup model (rollback)."""
    if BACKUP_MODEL_PATH.exists():
        shutil.copy2(BACKUP_MODEL_PATH, MODEL_PATH)
    if BACKUP_MODEL_DATA_PATH.exists():
        shutil.copy2(BACKUP_MODEL_DATA_PATH, MODEL_DATA_PATH)


def cleanup_backups():
    """Remove backup files."""
    if BACKUP_MODEL_PATH.exists():
        BACKUP_MODEL_PATH.unlink()
    if BACKUP_MODEL_DATA_PATH.exists():
        BACKUP_MODEL_DATA_PATH.unlink()


def run_training():
    """Run train.py and return the test accuracy."""
    print("[RETRAIN] Running train.py...")
    result = subprocess.run(
        [sys.executable, str(TRAIN_SCRIPT)],
        capture_output=True,
        text=True,
        cwd=str(SCRIPT_DIR),
    )

    if result.returncode != 0:
        print(f"[RETRAIN] train.py failed: {result.stderr[:500]}")
        return None

    # Parse test accuracy from train.py output
    # train.py prints "Test accuracy: 0.XXX" at the end
    for line in result.stdout.split("\n"):
        if "test accuracy" in line.lower() or "test_acc" in line.lower():
            try:
                # Extract the number
                import re
                match = re.search(r"(\d+\.?\d*)", line)
                if match:
                    return float(match.group(1))
            except Exception:
                continue

    print("[RETRAIN] Could not parse test accuracy from train.py output")
    print(f"[RETRAIN] stdout: {result.stdout[-500:]}")
    return None


def clear_approved():
    """Clear the approved phrasings file after successful retrain."""
    if APPROVED_PATH.exists():
        APPROVED_PATH.unlink()
    if REJECTED_PATH.exists():
        REJECTED_PATH.unlink()


# ─── Main ───────────────────────────────────────────────────────────────────

def main():
    print("[RETRAIN] Starting merge_and_train...")

    # 1. Read approved phrasings
    approved = read_approved_phrasings()
    rejected = read_rejected_examples()

    if not approved and not rejected:
        print("[RETRAIN] No approved phrasings or rejected examples. Exiting.")
        return

    print(f"[RETRAIN] Found {len(approved)} approved phrasings")
    print(f"[RETRAIN] Found {len(rejected)} rejected examples to remove")

    # 2. Show distribution
    if approved:
        dist = Counter(e["intent"] for e in approved)
        print("[RETRAIN] Approved distribution:")
        for intent, count in dist.most_common():
            print(f"  {intent:25s}: {count}")

    if rejected:
        rdist = Counter(e["intent"] for e in rejected)
        print("[RETRAIN] Rejected distribution:")
        for intent, count in rdist.most_common():
            print(f"  {intent:25s}: {count}")

    # 3. Merge into dataset (also removes rejected)
    new_train_count = merge_and_save(approved, rejected)
    print(f"[RETRAIN] Dataset now has {new_train_count} training examples")

    # 4. Backup current model
    backup_model()
    print("[RETRAIN] Backed up current model")

    # 5. Run training
    new_accuracy = run_training()

    if new_accuracy is None:
        print("[RETRAIN] Training failed. Restoring backup model.")
        restore_model()
        cleanup_backups()
        sys.exit(1)

    print(f"[RETRAIN] New model test accuracy: {new_accuracy:.4f}")

    # 6. Compare with old accuracy (read from a metrics file if it exists)
    metrics_path = SCRIPT_DIR / "model" / "metrics.json"
    old_accuracy = 0.0
    if metrics_path.exists():
        try:
            with open(metrics_path, "r") as f:
                metrics = json.load(f)
                old_accuracy = metrics.get("test_accuracy", 0.0)
        except Exception:
            pass

    print(f"[RETRAIN] Old model test accuracy: {old_accuracy:.4f}")

    # 7. Hot-swap or rollback
    if new_accuracy >= old_accuracy:
        print(f"[RETRAIN] New model is >= old ({new_accuracy:.4f} >= {old_accuracy:.4f}). Keeping new model.")
        # Save metrics
        with open(metrics_path, "w") as f:
            json.dump({"test_accuracy": new_accuracy, "examples": new_train_count}, f)
        cleanup_backups()
        clear_approved()
        print("[RETRAIN] Done. Model hot-swapped successfully.")
        # 7b. Sync model to the bundled resources directory (production copy)
        sync_to_resources()
        # 8. Notify the running NLU server to reload the new model
        reload_nlu_server()
    else:
        print(f"[RETRAIN] New model is WORSE ({new_accuracy:.4f} < {old_accuracy:.4f}). Rolling back.")
        restore_model()
        cleanup_backups()
        # Don't clear approved — keep them for the next retrain attempt
        print("[RETRAIN] Rolled back to old model. Approved phrasings kept for next attempt.")


def reload_nlu_server():
    """Tell the running NLU server to reload the newly trained ONNX model.

    Without this, the server keeps the old InferenceSession in memory and
    never picks up the new model. The server caches the session in a global
    variable and only loads on first use.
    """
    try:
        import urllib.request
        import urllib.error
        url = "http://127.0.0.1:39218/reload"
        req = urllib.request.Request(url, method="POST", data=b"")
        with urllib.request.urlopen(req, timeout=5) as resp:
            result = json.loads(resp.read())
            if result.get("reloaded"):
                print("[RETRAIN] NLU server reloaded new model successfully.")
            else:
                print(f"[RETRAIN] NLU server reload failed: {result.get('error', 'unknown')}")
    except urllib.error.URLError:
        print("[RETRAIN] NLU server not running — no reload needed (will load new model on next start).")
    except Exception as e:
        print(f"[RETRAIN] NLU server reload error (non-fatal): {e}")


def sync_to_resources():
    """Copy the newly trained model to the bundled resources directory.

    merge_and_train.py updates server/nlu/model/, but the production NLU
    server loads from src-tauri/resources/server/nlu/model/ (bundled via
    Tauri resources config). Without this sync, a retrained model never
    reaches the production app.
    """
    src_model = MODEL_PATH
    src_data = MODEL_DATA_PATH
    src_tokenizer = SCRIPT_DIR / "model" / "tokenizer"

    # Find the resources directory relative to the script
    # server/nlu/merge_and_train.py → ../../src-tauri/resources/server/nlu/model/
    resources_model_dir = SCRIPT_DIR.parent.parent / "src-tauri" / "resources" / "server" / "nlu" / "model"

    if not resources_model_dir.exists():
        print(f"[RETRAIN] Resources model dir not found ({resources_model_dir}), skipping sync.")
        return

    try:
        shutil.copy2(str(src_model), str(resources_model_dir / "nexus_nlu.onnx"))
        shutil.copy2(str(src_data), str(resources_model_dir / "nexus_nlu.onnx.data"))
        # Copy tokenizer if it exists
        res_tokenizer = resources_model_dir / "tokenizer"
        if res_tokenizer.exists():
            shutil.rmtree(str(res_tokenizer))
        if src_tokenizer.exists():
            shutil.copytree(str(src_tokenizer), str(res_tokenizer))
        print(f"[RETRAIN] Model synced to resources: {resources_model_dir}")
    except Exception as e:
        print(f"[RETRAIN] Resources sync failed (non-fatal): {e}")


if __name__ == "__main__":
    main()
