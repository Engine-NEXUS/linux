#!/usr/bin/env python3
"""
Publish a retrained NLU model for family devices.

The admin retrains BERT-Mini (nexus train / merge_and_train.py), which
produces server/nlu/model/nexus_nlu.onnx. This script ships that model
to family devices without a rebuild:

  1. Computes sha256 + size for each model file
  2. Uploads each file to the R2 bucket via `wrangler r2 object put`
  3. POSTs the manifest to the Worker's POST /models/nlu/publish
     (gated by NEXUS_ADMIN_TOKEN)

Family devices poll GET /models/nlu/latest on startup and download
changed files individually (see src-tauri/src/nlu_update.rs).

One-time setup (on the admin machine):
  npx wrangler r2 bucket create nexus-models
  # uncomment the [[r2_buckets]] MODELS binding in server/worker/wrangler.toml
  npx wrangler secret put NEXUS_ADMIN_TOKEN   # choose a strong token
  npx wrangler deploy

Usage:
  python publish_nlu.py --worker https://nexus-worker.example.workers.dev
  python publish_nlu.py --worker ... --token-file ../admin/data/admin_token.txt
  python publish_nlu.py --dry-run            # print manifest only

Admin token is read from (in order): --token, NEXUS_ADMIN_TOKEN env var,
--token-file.
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
MODEL_DIR = SCRIPT_DIR / "model"
R2_BUCKET = "nexus-models"
R2_PREFIX = "nlu/"

# Files the family-side updater knows how to fetch. Keep in sync with
# the whitelist in server/worker/src/index.ts (/models/nlu/download).
MODEL_FILES = [
    "nexus_nlu.onnx",
    "nexus_nlu.onnx.data",
    "labels.json",
    "temperature_calibration.json",
    "tokenizer/tokenizer.json",
    "tokenizer/tokenizer_config.json",
    "tokenizer/vocab.txt",
    "tokenizer/special_tokens_map.json",
]


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def collect_manifest() -> dict:
    files = {}
    for rel in MODEL_FILES:
        path = MODEL_DIR / rel
        if not path.exists():
            # onnx.data and calibration are optional on some exports;
            # skip missing files rather than fail the whole publish.
            print(f"  skip (missing): {rel}")
            continue
        files[rel] = {"sha256": sha256_file(path), "size": path.stat().st_size}
        print(f"  {rel}: {files[rel]['size'] / 1024:.0f} KB  sha256:{files[rel]['sha256'][:12]}...")
    if "nexus_nlu.onnx" not in files:
        sys.exit("ERROR: nexus_nlu.onnx not found — run train.py + export_onnx.py first")
    version = datetime.now(timezone.utc).strftime("%Y-%m-%d-%H%M")
    return {"version": version, "files": files}


def upload_to_r2(files: dict) -> None:
    for rel in files:
        src = MODEL_DIR / rel
        key = f"{R2_PREFIX}{rel}"
        cmd = [
            "npx", "wrangler", "r2", "object", "put",
            f"{R2_BUCKET}/{key}",
            "--file", str(src),
            "--remote",
        ]
        print(f"  r2 put {key} ...")
        result = subprocess.run(cmd, capture_output=True, text=True)
        if result.returncode != 0:
            sys.exit(f"ERROR: wrangler r2 put failed for {key}:\n{result.stderr.strip()}")


def publish_manifest(worker: str, token: str, manifest: dict) -> None:
    url = worker.rstrip("/") + "/models/nlu/publish"
    payload = json.dumps(manifest).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=payload,
        method="POST",
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {token}",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            body = json.loads(resp.read().decode("utf-8"))
            print(f"  publish: {body}")
    except urllib.error.HTTPError as e:
        sys.exit(f"ERROR: publish failed ({e.code}): {e.read().decode('utf-8', 'replace')}")
    except urllib.error.URLError as e:
        sys.exit(f"ERROR: publish failed: {e.reason}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--worker", required=True, help="Worker base URL")
    ap.add_argument("--token", help="NEXUS_ADMIN_TOKEN (else env var / --token-file)")
    ap.add_argument("--token-file", help="Path to a file containing the admin token")
    ap.add_argument("--dry-run", action="store_true", help="Build manifest only, no upload/publish")
    args = ap.parse_args()

    token = args.token or os.environ.get("NEXUS_ADMIN_TOKEN")
    if not token and args.token_file:
        token = Path(args.token_file).read_text(encoding="utf-8").strip()
    if not token and not args.dry_run:
        sys.exit("ERROR: admin token required (--token, NEXUS_ADMIN_TOKEN, or --token-file)")

    print("Collecting manifest...")
    manifest = collect_manifest()
    print(f"Version: {manifest['version']}  ({len(manifest['files'])} files)")

    if args.dry_run:
        print(json.dumps(manifest, indent=2))
        return

    print("Uploading to R2...")
    upload_to_r2(manifest["files"])

    print("Publishing manifest...")
    publish_manifest(args.worker, token, manifest)
    print("Done. Family devices will pick up the update on next launch.")


if __name__ == "__main__":
    main()
