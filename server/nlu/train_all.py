#!/usr/bin/env python3
"""
NEXUS NLU — Unified Training Script

One command to clean, generate, merge, repair, train, export, audit, and clean up the BERT-Mini NLU model.

Usage:
    python train_all.py              # full pipeline
    python train_all.py --clean-only  # just clean the dataset
    python train_all.py --skip-train   # everything except training (for quick data refresh)
    python train_all.py --keep-temp    # keep generated data and PyTorch checkpoint

Steps:
    1. Clean dataset (fix malformed labels, deduplicate)
    2. Generate live-mode training examples (type, press, send, browser, etc.)
    3. Merge and repair data, conflicts, slots, test coverage, and leakage
    4. Train BERT-Mini (google/bert_uncased_L-2_H-128_A-2)
    5. Export to ONNX (nexus_nlu.onnx) for fast CPU inference
    6. Copy model + tokenizer to src-tauri/resources/ for installer bundling
    7. Audit the trained dataset and ONNX model
    8. Delete temporary training files unless --keep-temp is supplied

Contributors: just run `nexus train` (which calls this script).
"""

import json
import os
import sys
import shutil
import subprocess
from pathlib import Path
from collections import Counter

# Force UTF-8 output to avoid cp1252 Unicode errors on Windows
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
sys.stderr.reconfigure(encoding='utf-8', errors='replace')

SCRIPT_DIR = Path(__file__).parent
DATASET_PATH = SCRIPT_DIR / "dataset.json"
NEW_EXAMPLES_PATH = SCRIPT_DIR / "new_examples.json"
RESOURCES_MODEL_DIR = SCRIPT_DIR.parent.parent / "src-tauri" / "resources" / "server" / "nlu" / "model"

# ─── Label mapping for malformed entries ──────────────────────────────────
LABEL_FIXES = {
    "OpenArchitect": "open_architect",
    'Unknown { raw: "Show me the PR list." }': "list_prs",
    'Unknown { raw: "Show me the PR list." }': "list_prs",
}


def step(msg):
    print(f"\n{'='*60}")
    print(f"  {msg}")
    print(f"{'='*60}")


def clean_dataset():
    """Fix malformed labels and deduplicate the dataset."""
    step("Step 1: Cleaning dataset")

    if not DATASET_PATH.exists():
        print(f"  ERROR: {DATASET_PATH} not found")
        sys.exit(1)

    with open(DATASET_PATH, 'r', encoding='utf-8') as f:
        data = json.load(f)

    train = data.get('train', data) if isinstance(data, dict) else data
    original_count = len(train)

    # Fix malformed labels
    fixed = 0
    for ex in train:
        intent = ex.get('intent', '')
        if intent in LABEL_FIXES:
            ex['intent'] = LABEL_FIXES[intent]
            fixed += 1
        # Also catch any non-lowercase intent that should be lowercase
        elif intent != intent.lower() and intent not in LABEL_FIXES:
            ex['intent'] = intent.lower()
            fixed += 1

    # Deduplicate by (text, intent)
    seen = set()
    deduped = []
    duplicates = 0
    for ex in train:
        key = (ex.get('text', '').lower().strip(), ex.get('intent', ''))
        if key in seen:
            duplicates += 1
            continue
        seen.add(key)
        deduped.append(ex)

    # Write back
    if isinstance(data, dict):
        data['train'] = deduped
    else:
        data = deduped

    with open(DATASET_PATH, 'w', encoding='utf-8') as f:
        json.dump(data, f, indent=2, ensure_ascii=False)

    # Report
    intents = Counter(ex.get('intent', '?') for ex in deduped)
    print(f"  Original examples: {original_count}")
    print(f"  Fixed labels: {fixed}")
    print(f"  Duplicates removed: {duplicates}")
    print(f"  Final examples: {len(deduped)}")
    print(f"  Unique intents: {len(intents)}")

    # Check for any remaining malformed labels
    malformed = [label for label in intents if label != label.lower() or 'raw' in label]
    if malformed:
        print(f"  WARNING: Still have malformed labels: {malformed}")
    else:
        print(f"  All labels clean")

    return len(deduped)


def generate_live_data():
    """Run generate_live_data.py to create new training examples."""
    step("Step 2: Generating live-mode training examples")

    script = SCRIPT_DIR / "generate_live_data.py"
    if not script.exists():
        print(f"  WARNING: {script} not found, skipping generation")
        return 0

    result = subprocess.run(
        [sys.executable, str(script)],
        cwd=str(SCRIPT_DIR),
        capture_output=True,
        text=True,
        encoding='utf-8',
        errors='replace',
    )
    print(result.stdout)
    if result.returncode != 0:
        print(f"  STDERR: {result.stderr}")
        print(f"  WARNING: Generation had errors, continuing with existing data")

    # Count new examples
    if NEW_EXAMPLES_PATH.exists():
        with open(NEW_EXAMPLES_PATH, 'r', encoding='utf-8') as f:
            new_data = json.load(f)
        count = len(new_data) if isinstance(new_data, list) else len(new_data.get('train', []))
        print(f"  Generated: {count} new examples")
        return count
    return 0


def merge_live_data():
    """Run merge_live_data.py to merge new examples into dataset.json."""
    step("Step 3: Merging new examples into dataset")

    script = SCRIPT_DIR / "merge_live_data.py"
    if not script.exists():
        print(f"  WARNING: {script} not found, skipping merge")
        return

    # Only merge if new_examples.json exists and has data
    if NEW_EXAMPLES_PATH.exists():
        result = subprocess.run(
            [sys.executable, str(script)],
            cwd=str(SCRIPT_DIR),
            capture_output=True,
            text=True,
            encoding='utf-8',
            errors='replace',
        )
        print(result.stdout)
        if result.returncode != 0:
            print(f"  STDERR: {result.stderr}")
    else:
        print(f"  No generated examples to merge, skipping")

    # Also merge collected voice samples (from collect_nlu_samples.py)
    collected_path = SCRIPT_DIR.parent / "admin" / "data" / "collected_samples.jsonl"
    if collected_path.exists():
        print(f"  Merging collected voice samples from {collected_path}")
        merge_collected_samples(collected_path)
    else:
        print(f"  No collected voice samples found (run 'nexus collect' to add real voice data)")


def merge_collected_samples(collected_path):
    """Merge collected voice samples into dataset.json."""
    # Read collected samples
    collected = []
    with open(collected_path, 'r', encoding='utf-8') as f:
        for line in f:
            line = line.strip()
            if line:
                try:
                    ex = json.loads(line)
                    collected.append({
                        "text": ex["text"],
                        "intent": ex["intent"],
                        "slots": ex.get("slots", {}),
                    })
                except (json.JSONDecodeError, KeyError):
                    continue

    if not collected:
        print(f"  No valid samples in collected file")
        return

    # Read existing dataset
    with open(DATASET_PATH, 'r', encoding='utf-8') as f:
        data = json.load(f)

    train = data.get('train', data) if isinstance(data, dict) else data

    # Deduplicate against existing data
    existing_keys = set()
    for ex in train:
        key = (ex.get('text', '').lower().strip(), ex.get('intent', ''))
        existing_keys.add(key)

    added = 0
    for ex in collected:
        key = (ex['text'].lower().strip(), ex['intent'])
        if key not in existing_keys:
            train.append(ex)
            existing_keys.add(key)
            added += 1

    # Write back
    if isinstance(data, dict):
        data['train'] = train
    else:
        data = train

    with open(DATASET_PATH, 'w', encoding='utf-8') as f:
        json.dump(data, f, indent=2, ensure_ascii=False)

    print(f"  Added {added} new voice samples (out of {len(collected)} collected)")
    print(f"  Skipped {len(collected) - added} duplicates")


def repair_dataset():
    """Repair confirmed conflicts, leakage, invalid slots, and missing test coverage."""
    step("Step 3b: Repairing dataset conflicts and evaluation coverage")
    script = SCRIPT_DIR / "repair_nlu_data.py"
    result = subprocess.run(
        [sys.executable, str(script), "--apply"],
        cwd=str(SCRIPT_DIR),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    print(result.stdout)
    if result.returncode != 0:
        print(result.stderr)
        print("  ERROR: Dataset repair failed")
        sys.exit(1)


def train_model():
    """Run train.py to train the BERT-Mini model."""
    step("Step 4: Training BERT-Mini model")

    script = SCRIPT_DIR / "train.py"
    if not script.exists():
        print(f"  ERROR: {script} not found")
        sys.exit(1)

    # Run training (stream output so user sees progress)
    result = subprocess.run(
        [sys.executable, str(script)],
        cwd=str(SCRIPT_DIR),
    )
    if result.returncode != 0:
        print(f"\n  ERROR: Training failed (exit code {result.returncode})")
        sys.exit(1)

    # Verify model was created
    model_path = SCRIPT_DIR / "model" / "best_model.pt"
    if model_path.exists():
        size_mb = model_path.stat().st_size / 1024 / 1024
        print(f"\n  Model saved: {model_path} ({size_mb:.1f} MB)")
    else:
        print(f"\n  ERROR: Model not found at {model_path}")
        sys.exit(1)


def export_onnx():
    """Run export_onnx.py to export the trained model to ONNX format."""
    step("Step 5: Exporting to ONNX")

    script = SCRIPT_DIR / "export_onnx.py"
    if not script.exists():
        print(f"  ERROR: {script} not found")
        sys.exit(1)

    result = subprocess.run(
        [sys.executable, str(script)],
        cwd=str(SCRIPT_DIR),
        capture_output=True,
        text=True,
        encoding='utf-8',
        errors='replace',
    )
    print(result.stdout)
    if result.returncode != 0:
        print(f"  STDERR: {result.stderr}")
        print(f"  ERROR: ONNX export failed")
        sys.exit(1)

    onnx_path = SCRIPT_DIR / "model" / "nexus_nlu.onnx"
    if onnx_path.exists():
        size_mb = onnx_path.stat().st_size / 1024 / 1024
        print(f"  ONNX exported: {onnx_path} ({size_mb:.1f} MB)")
    else:
        print(f"  ERROR: ONNX file not found")
        sys.exit(1)


def sync_to_resources():
    """Copy the trained model + tokenizer to src-tauri/resources/ for installer bundling."""
    step("Step 6: Syncing model to resources (for installer)")

    src_dir = SCRIPT_DIR / "model"
    if not src_dir.exists():
        print(f"  ERROR: {src_dir} not found")
        sys.exit(1)

    RESOURCES_MODEL_DIR.mkdir(parents=True, exist_ok=True)

    files_to_copy = ["nexus_nlu.onnx", "nexus_nlu.onnx.data", "labels.json"]
    copied = 0
    for fname in files_to_copy:
        src = src_dir / fname
        if src.exists():
            shutil.copy2(src, RESOURCES_MODEL_DIR / fname)
            copied += 1
            print(f"  Copied: {fname}")

    # Copy tokenizer dir
    src_tok = src_dir / "tokenizer"
    dst_tok = RESOURCES_MODEL_DIR / "tokenizer"
    if src_tok.exists():
        dst_tok.mkdir(parents=True, exist_ok=True)
        for f in src_tok.iterdir():
            shutil.copy2(f, dst_tok / f.name)
            copied += 1
        print(f"  Copied: tokenizer/ ({len(list(src_tok.iterdir()))} files)")

    if copied == 0:
        print(f"  WARNING: No files copied — model may not be trained yet")
    else:
        print(f"  Synced {copied} file(s) to {RESOURCES_MODEL_DIR}")
        print(f"  Run 'nexus build' to bundle into the installer")


def audit_trained_model():
    """Run the structural and ONNX quality audit after training."""
    step("Step 7: Auditing trained dataset and ONNX model")
    script = SCRIPT_DIR / "audit_nlu.py"
    result = subprocess.run([sys.executable, str(script)], cwd=str(SCRIPT_DIR.parent.parent))
    if result.returncode != 0:
        print("  ERROR: Post-training audit failed")
        sys.exit(1)


def cleanup_temp_files():
    """Delete temporary files created during training to save disk space.

    After training + ONNX export + sync, these files are no longer needed:
      - new_examples.json       (~150 KB)  — generated, merged into dataset.json
      - model/best_model.pt     (~17 MB)   — PyTorch checkpoint, ONNX is the production format
      - collected_samples.jsonl (~varies)  — voice samples, merged into dataset.json

    Files that are KEPT:
      - dataset.json             — the merged dataset (for future retraining)
      - model/nexus_nlu.onnx     — the production ONNX model
      - model/labels.json        — intent + slot label lists
      - model/tokenizer/         — BERT-Mini tokenizer files
      - approved_phrasings.jsonl — brain-collected data (admin-only, for brain retraining)
      - rejected_examples.jsonl  — brain-collected rejections (admin-only)
    """
    step("Step 8: Cleaning up temporary files")

    files_to_delete = [
        ("new_examples.json", NEW_EXAMPLES_PATH),
        ("model/best_model.pt", SCRIPT_DIR / "model" / "best_model.pt"),
        ("collected_samples.jsonl", SCRIPT_DIR.parent / "admin" / "data" / "collected_samples.jsonl"),
    ]

    total_freed = 0
    deleted = 0
    for name, path in files_to_delete:
        if path.exists():
            size = path.stat().st_size
            path.unlink()
            size_kb = size / 1024
            if size_kb > 1024:
                print(f"  Deleted: {name} ({size_kb / 1024:.1f} MB)")
            else:
                print(f"  Deleted: {name} ({size_kb:.0f} KB)")
            total_freed += size
            deleted += 1
        else:
            print(f"  Skip: {name} (not found)")

    if total_freed > 0:
        if total_freed > 1024 * 1024:
            print(f"  Total freed: {total_freed / 1024 / 1024:.1f} MB")
        else:
            print(f"  Total freed: {total_freed / 1024:.0f} KB")
    else:
        print(f"  Nothing to clean")

    print(f"  Kept: dataset.json, nexus_nlu.onnx, labels.json, tokenizer/")


def main():
    skip_train = "--skip-train" in sys.argv
    clean_only = "--clean-only" in sys.argv
    keep_temp = "--keep-temp" in sys.argv

    print("\n" + "=" * 60)
    print("  NEXUS NLU Training Pipeline")
    print("  BERT-Mini (google/bert_uncased_L-2_H-128_A-2)")
    print("=" * 60)

    # Step 1: Clean dataset
    clean_dataset()

    if clean_only:
        print("\n  --clean-only specified, exiting after cleanup.")
        return

    # Step 2: Generate live-mode examples
    generate_live_data()

    # Step 3: Merge into dataset
    merge_live_data()

    # Repair conflicts, invalid annotations, test gaps, and split leakage
    repair_dataset()

    # Re-clean after merge and repair
    step("Step 3c: Re-cleaning after merge and repair")
    clean_dataset()

    if skip_train:
        print("\n  --skip-train specified, exiting before training.")
        print("  Run 'python train_all.py' (without --skip-train) to train.")
        return

    # Step 4: Train
    train_model()

    # Step 5: Export ONNX
    export_onnx()

    # Step 6: Sync to resources
    sync_to_resources()

    # Step 7: Audit the trained model
    audit_trained_model()

    # Step 8: Clean up temporary files (save disk space)
    if keep_temp:
        step("Step 8: Skipping cleanup (--keep-temp)")
        print("  Keeping: new_examples.json, best_model.pt, collected_samples.jsonl")
        print("  These take ~17 MB. Run 'nexus train' without --keep-temp to clean up.")
    else:
        cleanup_temp_files()

    print("\n" + "=" * 60)
    print("  Training complete!")
    print("=" * 60)
    print("  Next steps:")
    print("    1. Run 'nexus build' to bundle the new model into the installer")
    print("    2. Run 'nexus start' to test with the new model")
    print("=" * 60 + "\n")


if __name__ == "__main__":
    main()
