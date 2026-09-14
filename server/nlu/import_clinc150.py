#!/usr/bin/env python3

import argparse
import hashlib
import json
import urllib.request
import zipfile
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
DATA_DIR = SCRIPT_DIR / "data"
DOWNLOAD_DIR = DATA_DIR / "downloads"
STAGING_DIR = DATA_DIR / "staging"
EVALUATION_DIR = DATA_DIR / "evaluation"
ZIP_PATH = DOWNLOAD_DIR / "clinc150.zip"
TRAIN_PATH = STAGING_DIR / "clinc150_oos_train.jsonl"
BENCHMARK_PATH = EVALUATION_DIR / "clinc150_oos_test.jsonl"
ATTRIBUTION_PATH = DATA_DIR / "clinc150_attribution.json"
EXTERNAL_EVALUATION_LOCK_PATH = DATA_DIR / "external_evaluation_lock.json"
SOURCE_URL = "https://archive.ics.uci.edu/static/public/570/clinc150.zip"
SOURCE_DOI = "10.24432/C5MP58"
LICENSE = "CC-BY-4.0"
NEXUS_DATASET_PATH = SCRIPT_DIR / "dataset.json"


def file_sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download():
    DOWNLOAD_DIR.mkdir(parents=True, exist_ok=True)
    urllib.request.urlretrieve(SOURCE_URL, ZIP_PATH)
    print(f"Downloaded {ZIP_PATH} ({ZIP_PATH.stat().st_size} bytes)")


def locate_full_dataset(archive):
    names = [name for name in archive.namelist() if name.endswith("data_full.json") and not name.startswith("__MACOSX/")]
    if len(names) != 1:
        raise ValueError(f"expected one data_full.json, found {names}")
    return names[0]


def staging_row(text, source_record_id):
    return {
        "text": text,
        "source": "clinc150",
        "source_record_id": source_record_id,
        "license": LICENSE,
        "language": "en-US",
        "source_intent": "oos",
        "mapped_intent": "unknown",
        "slots": {},
        "mapping_version": "clinc-oos-v1",
        "review_status": "pending",
        "split_group": f"clinc150:{source_record_id}",
        "split": "train",
    }


def benchmark_row(text, source_record_id):
    return {
        "text": text,
        "expected_intent": "unknown",
        "review_status": "pending_nexus_mapping",
        "source": "clinc150",
        "source_record_id": source_record_id,
        "license": LICENSE,
        "split_group": f"clinc150:{source_record_id}",
        "never_train": True,
    }


def extract_rows(values, expected_label, split):
    rows = []
    for index, value in enumerate(values):
        if not isinstance(value, list) or len(value) != 2:
            raise ValueError(f"invalid {split} row {index}")
        text, label = value
        if label != expected_label:
            raise ValueError(f"unexpected {split} label at {index}: {label}")
        text = " ".join(str(text).strip().split())
        if not text:
            raise ValueError(f"empty {split} text at {index}")
        rows.append((text, f"{split}:{index}"))
    return rows


def write_jsonl(path, rows):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n" for row in rows), encoding="utf-8")


def import_data():
    if not ZIP_PATH.exists():
        download()
    with zipfile.ZipFile(ZIP_PATH) as archive:
        dataset_name = locate_full_dataset(archive)
        dataset_bytes = archive.read(dataset_name)
    dataset = json.loads(dataset_bytes)
    train = extract_rows(dataset["oos_train"], "oos", "oos_train")
    test = extract_rows(dataset["oos_test"], "oos", "oos_test")
    nexus_dataset = json.loads(NEXUS_DATASET_PATH.read_text(encoding="utf-8"))
    nexus_texts = {" ".join(row["text"].lower().split()) for row in nexus_dataset.get("train", []) + nexus_dataset.get("test", [])}
    original_train_count = len(train)
    original_test_count = len(test)
    train = [(text, source_id) for text, source_id in train if " ".join(text.lower().split()) not in nexus_texts]
    test = [(text, source_id) for text, source_id in test if " ".join(text.lower().split()) not in nexus_texts]
    train_texts = {text.lower() for text, _ in train}
    test_texts = {text.lower() for text, _ in test}
    overlap = sorted(train_texts & test_texts)
    if overlap:
        raise ValueError(f"CLINC OOS train/test overlap: {overlap[:5]}")
    write_jsonl(TRAIN_PATH, [staging_row(text, source_id) for text, source_id in train])
    write_jsonl(BENCHMARK_PATH, [benchmark_row(text, source_id) for text, source_id in test])
    attribution = {
        "source": "CLINC150",
        "source_url": SOURCE_URL,
        "doi": SOURCE_DOI,
        "license": LICENSE,
        "citation": "CLINC150 [Dataset]. (2020). UCI Machine Learning Repository. https://doi.org/10.24432/C5MP58.",
        "source_zip_sha256": file_sha256(ZIP_PATH),
        "source_dataset_member": dataset_name,
        "source_dataset_sha256": hashlib.sha256(dataset_bytes).hexdigest(),
        "retained": {
            "oos_train_staged_pending_review": len(train),
            "oos_test_never_train_benchmark": len(test),
        },
        "nexus_exact_text_overlaps_discarded": {
            "oos_train": original_train_count - len(train),
            "oos_test": original_test_count - len(test),
        },
        "discarded": "All 150 in-scope classes, validation rows, alternate dataset versions, nonessential source fields, and exact NEXUS dataset overlaps",
    }
    ATTRIBUTION_PATH.write_text(json.dumps(attribution, indent=2) + "\n", encoding="utf-8")
    external_lock = {
        "schema_version": 1,
        "benchmarks": [{
            "id": "clinc150_oos_test",
            "path": "server/nlu/data/evaluation/clinc150_oos_test.jsonl",
            "rows": len(test),
            "sha256": file_sha256(BENCHMARK_PATH),
            "never_train": True,
            "source_dataset_sha256": attribution["source_dataset_sha256"],
        }],
    }
    EXTERNAL_EVALUATION_LOCK_PATH.write_text(json.dumps(external_lock, indent=2) + "\n", encoding="utf-8")
    print(f"Staged OOS train rows: {len(train)}")
    print(f"Frozen never-train OOS benchmark rows: {len(test)}")
    print(f"Source ZIP SHA-256: {attribution['source_zip_sha256']}")


def main():
    parser = argparse.ArgumentParser(description="Import only CLINC150 OOS records from the official UCI distribution")
    parser.add_argument("--download-only", action="store_true")
    parser.add_argument("--refresh", action="store_true")
    args = parser.parse_args()
    if args.refresh or not ZIP_PATH.exists():
        download()
    if not args.download_only:
        import_data()


if __name__ == "__main__":
    main()
