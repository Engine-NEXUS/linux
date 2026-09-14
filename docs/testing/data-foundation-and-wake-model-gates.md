# NEXUS Data Foundation and Wake-Model Gates

**Date:** 2026-09-14  
**Status:** Milestone 1 implementation  
**Scope:** NLU source provenance, frozen evaluation controls, wake-audio split integrity, and deployed wake-model fingerprints

## Purpose

External data must not enter BERT-Mini training until its origin, license, mapping, review status, and split family are validated. Wake-word candidates must not be compared against mutable or speaker-leaked audio splits. These controls run before acquisition or retraining.

## NLU controls

The source registry is `server/nlu/data/source_registry.json`. Every staged record requires:

```json
{
  "text": "open another browser tab",
  "source": "massive_en",
  "source_record_id": "en-US:12345",
  "license": "CC-BY-4.0",
  "language": "en-US",
  "source_intent": "create_or_open",
  "mapped_intent": "browser_new_tab",
  "slots": {},
  "mapping_version": "1",
  "review_status": "pending",
  "split_group": "massive:12345"
}
```

Staged source payloads remain in the ignored `server/nlu/data/staging/` directory. Validation rejects unknown sources, labels, and slots; duplicate source IDs; invalid review states; split-family leakage; and approved text that overlaps the frozen evaluation set.

The current 452-row evaluation split is locked by normalized-content hashes in `server/nlu/data/evaluation_lock.json`. Updating that lock is an explicit reviewed action, not part of normal training.

```powershell
nexus data nlu validate
nexus data nlu freeze-evaluation
```

`nexus train` runs NLU foundation validation before modifying or training the dataset.

## Wake controls

The deployed wake pipeline is fingerprinted in `src-tauri/resources/oww/model_manifest.json`. It records SHA-256 and byte size for:

- `melspectrogram.onnx`
- `embedding_model.onnx`
- `nexus.onnx`

It also records the runtime operating point currently implemented in Rust:

- wake threshold `0.35`
- silence RMS threshold `0.002`
- AGC target RMS `0.03`
- maximum gain `20x`
- restart grace `10s`
- refractory period `3000ms`

```powershell
nexus data wake validate
nexus data wake fingerprint
```

Fingerprint updates are explicit. A model replacement causes validation to fail until the candidate has been reviewed and the manifest intentionally regenerated.

## Wake-audio manifest

Local consented audio remains under the ignored `wake_word_data/` directory. When data exists, `wake_word_data/manifest.jsonl` must describe every original or augmented clip:

```json
{
  "audio": "positive/speaker-001/nexus-001.wav",
  "label": "positive",
  "phrase": "nexus",
  "speaker_id": "speaker-001",
  "accent": "en-IN",
  "device_class": "laptop_array",
  "environment": "quiet_room",
  "distance_cm": 50,
  "volume": "normal",
  "session_id": "session-001",
  "source_group_id": "recording-001",
  "split": "train",
  "consent": true
}
```

Validation rejects:

- missing audio or metadata;
- duplicate audio paths;
- invalid labels, splits, volumes, or distances;
- records without explicit consent;
- a speaker spanning train, validation, and test;
- a recording session spanning splits;
- originals and their augmented derivatives spanning splits.

Use strict mode once collection begins:

```powershell
python scripts/wake_data_foundation.py validate --require-data
```

## Current verified baseline

The deployed classifier is `415224` bytes. Historical documents mentioning a `790682`-byte classifier describe an older artifact. The v3 notebook and local wake recordings referenced by earlier planning documents are not present in this workspace and must be restored or recreated before v3 training.

## Three-pass cross-check

Milestone 1 is accepted only after all three passes succeed:

1. Python unit checks for deterministic locks, evaluation-leak rejection, speaker-leak rejection, and model fingerprint repeatability.
2. Direct NLU and wake foundation CLI validation, including the existing structural NLU audit.
3. Unified `nexus data` command validation plus Rust wake-model tests and repository-sensitive-data review.

No external import or model retraining should begin until all three passes are green.

## Milestone 2 CLINC150 OOS staging

The importer downloads the official UCI archive and keeps only the fields NEXUS needs:

```powershell
nexus data nlu import-clinc
python server/nlu/evaluate_external_oos.py
```

The retained artifacts are:

- 100 OOS training records in ignored staging, all `pending` review;
- 1,000 OOS test records in a committed, hash-locked, `never_train` benchmark;
- source URL, DOI, CC BY 4.0 attribution, archive hash, and source-member hash;
- a provisional current-model report.

All 150 CLINC in-scope classes, validation rows, alternate dataset versions, and nonessential source fields are discarded. The original CLINC `oos` designation is not automatically authoritative for NEXUS because some records may describe supported NEXUS actions such as browser search. The benchmark remains `pending_nexus_mapping` until those overlaps are reviewed. No CLINC row is merged into `dataset.json`, and no model is retrained at this stage.
