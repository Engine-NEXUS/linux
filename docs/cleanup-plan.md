# NEXUS — Repo Cleanup & Size Reduction Plan

> **Goal:** Remove unnecessary files, delete training data after model training, and reduce the tracked repo size.
>
> **Current tracked size:** 318.7 MB (769 files)
> **Target tracked size:** ~150 MB (remove ~170 MB of dead/duplicate weight)
>
> **Date:** 2026-09-07

---

## Table of Contents

1. [Current Disk Usage Breakdown](#1-current-disk-usage-breakdown)
2. [What's Tracked vs Gitignored](#2-whats-tracked-vs-gitignored)
3. [Cleanup Categories](#3-cleanup-categories)
4. [Phase 1: Delete Wake Word Augmented Data](#phase-1-delete-wake-word-augmented-data)
5. [Phase 2: Delete Root-Level Scratch Files](#phase-2-delete-root-level-scratch-files)
6. [Phase 3: Remove Sherpa Resources (Unused in Default Build)](#phase-3-remove-sherpa-resources)
7. [Phase 4: Remove Duplicate NLU Model](#phase-4-remove-duplicate-nlu-model)
8. [Phase 5: Remove Duplicate Server Scripts](#phase-5-remove-duplicate-server-scripts)
9. [Phase 6: Clean Build Artifacts](#phase-6-clean-build-artifacts)
10. [Phase 7: Post-Training Data Deletion Policy](#phase-7-post-training-data-deletion-policy)
11. [Summary of Savings](#summary-of-savings)
12. [Files to KEEP (Do NOT Delete)](#files-to-keep)

---

## 1. Current Disk Usage Breakdown

### On-disk sizes (including untracked + build artifacts)

| Directory | Size | Files | Status |
|-----------|------|-------|--------|
| `src-tauri/target/` | 38,836 MB | 57,499 | Build artifacts (gitignored) |
| `server/` | 742 MB | 2,396 | Mixed (admin 469 MB gitignored, worker node_modules 204 MB gitignored) |
| `wake_word_data/` | 690 MB | 10,236 | Gitignored (673 MB is augmented, regenerable) |
| `frontend/node_modules/` | 255 MB | 7,831 | Gitignored |
| `src-tauri/resources/` | 139 MB | 387 | Tracked (includes 34 MB sherpa + 60 MB piper) |
| `frontend/dist/` | 4.6 MB | 41 | Gitignored |
| `docs/` | 1.7 MB | 173 | Tracked |
| `scripts/` | 0.1 MB | 35 | Tracked |
| Root scratch files | ~0.6 MB | ~30 | Mostly gitignored |

### Git-tracked size: 318.7 MB (769 files)

| Tracked path | Size | Notes |
|-------------|------|-------|
| `src-tauri/resources/piper/en_US-amy-medium.onnx` | 60.3 MB | TTS fallback model — KEEP |
| `server/nlu/model/nexus_nlu.onnx` | 17.6 MB | DUPLICATE of resources copy |
| `server/nlu/model/nexus_nlu.onnx.data` | 17.5 MB | DUPLICATE of resources copy |
| `src-tauri/resources/sherpa/speaker_model.onnx` | 28.2 MB | Unused (feature not in default build) |
| `src-tauri/resources/sherpa/encoder-*.int8.onnx` | 4.8 MB | Unused (sherpa KWS, not default) |
| `src-tauri/resources/sherpa/silero_vad.onnx` | 0.6 MB | Unused (sherpa VAD, not default) |
| `src-tauri/resources/sherpa/decoder-*.int8.onnx` | 0.3 MB | Unused (sherpa KWS, not default) |
| `src-tauri/resources/sherpa/joiner-*.int8.onnx` | 0.2 MB | Unused (sherpa KWS, not default) |
| `src-tauri/resources/server/nlu/model/nexus_nlu.onnx` | 17.6 MB | Production NLU model — KEEP |
| `src-tauri/resources/server/nlu/model/nexus_nlu.onnx.data` | 17.5 MB | Production NLU model data — KEEP |
| `src-tauri/resources/server/nlu/model/tokenizer/tokenizer.json` | 0.7 MB | NLU tokenizer — KEEP |
| `src-tauri/resources/espeak-ng-data/` | 7.6 MB | Espeak phoneme data — KEEP |
| `src-tauri/resources/oww/` | 2.9 MB | Wake word models — KEEP |
| `server/nlu/model/tokenizer/tokenizer.json` | 0.7 MB | DUPLICATE |
| `server/worker/` (src only) | 0.2 MB | Cloudflare Worker — KEEP |
| `docs/` | 1.7 MB | Documentation — KEEP |
| `frontend/src/` | 0.5 MB | Frontend source — KEEP |

---

## 2. What's Tracked vs Gitignored

### Tracked in git (769 files, 318.7 MB)

- `src-tauri/src/` — Rust source code
- `src-tauri/resources/` — Bundled models (piper, oww, sherpa, espeak, server)
- `server/nlu/` — NLU model + training scripts (68 MB, DUPLICATE of resources)
- `server/worker/` — Cloudflare Worker source (src only, not node_modules)
- `server/sidecar/` — Python sidecar source
- `server/nlu_server.py`, `server/stt_server.py` — Dev copies (nlu is DUPLICATE)
- `frontend/src/` — Frontend source
- `docs/` — Documentation
- `scripts/` — Utility scripts
- `AGENTS.md`, `README.md`, `QUICKSTART.md` — Project docs
- `CHANGELOG_PREM22K.md`, `newStep3.txt` — Scratch (should be removed)

### Gitignored (95 entries)

- `wake_word_data/` — Real voice samples (privacy)
- `wake_word_data.zip` — Colab upload zip
- `server/admin/` — Admin-only brain (469 MB, Qwen model)
- `server/worker/node_modules/` — Worker dependencies (204 MB)
- `frontend/node_modules/` — Frontend dependencies (255 MB)
- `frontend/dist/` — Build output
- `src-tauri/target/` — Rust build artifacts (38.8 GB!)
- `*.ipynb` — Training notebooks
- `training/` — Training artifacts
- Root scratch files (`test_*.py`, `*.cjs`, `*.wav`, logs, etc.)

---

## 3. Cleanup Categories

### Category A: Safe to delete immediately (no impact on app)

| Item | Size | Reason |
|------|------|-------|
| `wake_word_data/augmented/` | 673 MB | Regenerable from originals via `augment_wake_samples.py` |
| Root `*.wav` files (6 files) | 0.6 MB | Old TTS test clips, not used by app |
| Root `test_*.py` files (8 files) | 0.03 MB | Scratch test scripts, gitignored |
| Root `*.cjs` files (7 files) | 0.01 MB | Scratch debug scripts, gitignored |
| Root `brain_stdout.txt`, `brain_stderr.txt` | 0.01 MB | Log files, gitignored |
| Root `newStep3.txt` | 0.01 MB | Scratch, tracked but should be removed |
| `CHANGELOG_PREM22K.md` | 0.01 MB | Old changelog, tracked but should be removed |
| `scripts/check_sizes.ps1`, `check_sizes_detail.ps1`, `check_tracked.ps1` | 0.01 MB | Temp scripts from this session |

### Category B: Remove from git tracking (keep local if needed)

| Item | Tracked Size | Reason |
|------|-------------|--------|
| `src-tauri/resources/sherpa/` | 34.1 MB | `wakeword-sherpa` feature NOT in default build. Sherpa-onnx not even in Cargo.toml dependencies. All sherpa code is behind `#[cfg(feature = "wakeword-sherpa")]`. These resources are dead weight in the default build. |
| `server/nlu/model/` | 35.8 MB | EXACT DUPLICATE of `src-tauri/resources/server/nlu/model/` (verified by MD5 hash). The production copy in `resources/` is what gets bundled. |
| `server/nlu_server.py` | 0.01 MB | EXACT DUPLICATE of `src-tauri/resources/server/nlu_server.py` (verified by MD5 hash). |

### Category C: Delete after model training (Phase 7)

| Item | Size | Condition |
|------|------|-----------|
| `wake_word_data/positive/` | 6 MB | Delete after v3 model is trained and validated |
| `wake_word_data/negative/` | 5.7 MB | Delete after v3 model is trained and validated |
| `wake_word_data/free/` | 3.7 MB | Delete after v3 model is trained and validated |
| `wake_word_data/background/` | 1.2 MB | Delete after v3 model is trained and validated |
| `wake_word_data.zip` | 10.8 MB | Delete after Colab upload completes |
| `train_nexus_oww_v2.ipynb` | 0.05 MB | Delete after v3 model is trained (v3 notebook supersedes) |

### Category D: Build artifacts (already gitignored, just clean disk)

| Item | Size | Command |
|------|------|---------|
| `src-tauri/target/` | 38.8 GB | `cargo clean` |
| `frontend/node_modules/` | 255 MB | `npm --prefix frontend prune` |
| `server/worker/node_modules/` | 204 MB | `npm --prefix server/worker prune` |
| `frontend/dist/` | 4.6 MB | Delete, regenerated by `npm run build` |

### Category E: Keep (do NOT delete)

| Item | Size | Reason |
|------|------|-------|
| `src-tauri/resources/piper/en_US-amy-medium.onnx` | 60.3 MB | TTS fallback engine, bundled in app |
| `src-tauri/resources/oww/*.onnx` | 2.9 MB | Wake word models (mel + embedding + classifier) |
| `src-tauri/resources/espeak-ng-data/` | 7.6 MB | Phoneme data for espeak-rs (used by Piper TTS) |
| `src-tauri/resources/server/nlu/model/` | 35.8 MB | Production NLU model, bundled in app |
| `src-tauri/resources/server/stt_server.py` | 0.01 MB | Production STT server, bundled in app |
| `src-tauri/resources/server/nlu_server.py` | 0.01 MB | Production NLU server, bundled in app |
| `server/worker/src/` | 0.2 MB | Cloudflare Worker source |
| `server/sidecar/` | 0.1 MB | Python sidecar source |
| `server/stt_server.py` | 0.01 MB | Dev version (different from production copy) |
| `server/nlu/train.py`, `seed_dataset.py`, etc. | 0.05 MB | NLU training scripts (needed for retraining) |
| `docs/` | 1.7 MB | All documentation |
| `scripts/` | 0.1 MB | Utility scripts (record_wake_samples, augment, etc.) |
| `frontend/src/` | 0.5 MB | Frontend source |
| `src-tauri/src/` | — | Rust source |

---

## Phase 1: Delete Wake Word Augmented Data

**Rationale:** The augmented data (673 MB, 10,013 files) is 100% regenerable from the 223 original recordings using `scripts/augment_wake_samples.py`. Keeping it wastes disk space. The originals (16.6 MB) are kept for future re-augmentation.

**Action:**
```bash
# Delete augmented data (regenerable)
rm -rf C:\PROJECTS\ULTRON\wake_word_data\augmented
```

**Savings:** 673 MB disk space
**Risk:** None — run `python scripts/augment_wake_samples.py` to regenerate

---

## Phase 2: Delete Root-Level Scratch Files

**Rationale:** These are one-off test scripts and debug files from previous development sessions. They are already gitignored (except `CHANGELOG_PREM22K.md` and `newStep3.txt` which are tracked but should be removed).

**Files to delete (gitignored, disk only):**
```
live_mic_5s.wav
mic_test_dev21.wav
test_tts_hey_nexus.wav
test_tts_nexus.wav
test_tts_nexus_please.wav
test_tts_nexus_wake_up.wav
test_tts_ok_nexus.wav
gen_tts.py
list_apis.py
list_devices.py
test_all_devices.py
test_apis.py
test_dev21.py
test_live_mic.py
test_mel_compare.py
test_mic_freq.py
test_wake_model.py
test_wasapi_native.py
check-css-var.cjs
deep-debug.cjs
reshow.cjs
show-blur-test.cjs
show-blur-test2.cjs
show-blur-test3.cjs
show-blur-test4.cjs
show-demo.cjs
brain_stderr.txt
brain_stdout.txt
demo-v3.png
nexus-stderr.log
nexus-stdout.log
nexus_stderr.log
nexus_stderr_test.log
nexus_stdout.log
nexus_stdout_test.log
```

**Files to `git rm` (tracked):**
```
CHANGELOG_PREM22K.md
newStep3.txt
```

**Temp scripts from this session (delete after cleanup):**
```
scripts/check_sizes.ps1
scripts/check_sizes_detail.ps1
scripts/check_tracked.ps1
```

**Savings:** ~1 MB disk + removes 2 tracked scratch files

---

## Phase 3: Remove Sherpa Resources

**Rationale:** The `wakeword-sherpa` feature is NOT in the default build. Sherpa-onnx is not even listed as a dependency in `Cargo.toml` — it's only mentioned in comments. All sherpa-related Rust code is behind `#[cfg(feature = "wakeword-sherpa")]`. The resources are dead weight in every default build and clone.

**Verification:**
- `Cargo.toml` line 135: `default = ["wakeword-oww"]` (no sherpa)
- `Cargo.toml` line 154: `wakeword-oww = []` (default)
- No `wakeword-sherpa = [...]` feature definition exists in Cargo.toml
- All sherpa code: `#[cfg(feature = "wakeword-sherpa")]`
- `tauri.conf.json` line 33: `"resources/sherpa/speaker_model.onnx"` — bundled but never loaded

**Action:**
```bash
# Remove from git tracking
git rm -r src-tauri/resources/sherpa/

# Remove from tauri.conf.json bundle list
# Delete line: "resources/sherpa/speaker_model.onnx",
```

**Update `tauri.conf.json`:** Remove the sherpa resource line from the bundle list.

**Update `.gitignore`:** Add `src-tauri/resources/sherpa/` so it stays local if someone needs it.

**Savings:** 34.1 MB tracked size, 34.1 MB smaller app bundle

**Risk:** If someone wants to use `wakeword-sherpa` feature, they need to download sherpa models separately. This is acceptable — the default and recommended wake engine is openWakeWord.

---

## Phase 4: Remove Duplicate NLU Model

**Rationale:** The NLU model at `server/nlu/model/` is an EXACT DUPLICATE of `src-tauri/resources/server/nlu/model/` (verified by MD5 hash: `EF39BF3D0B911C8045984CA1EF477504` for both `.onnx` files, `19A40B9245716493E0DE697CC515CD69` for both `.onnx.data` files). The production copy in `resources/` is what gets bundled in the app. The `server/nlu/` copy is only used for local dev/testing, but since the production copy exists, the dev server can point to it.

**Files to `git rm`:**
```
server/nlu/model/nexus_nlu.onnx          (17.6 MB)
server/nlu/model/nexus_nlu.onnx.data     (17.5 MB)
server/nlu/model/tokenizer/tokenizer.json (0.7 MB)
server/nlu/model/tokenizer/tokenizer_config.json
server/nlu/model/tokenizer/special_tokens_map.json
server/nlu/model/tokenizer/vocab.txt
```

**Keep in `server/nlu/`:**
```
server/nlu/train.py           — NLU training script
server/nlu/seed_dataset.py    — Dataset seeding
server/nlu/merge_and_train.py — Merge + train
server/nlu/dataset.json       — Training dataset
server/nlu/requirements.txt   — Python deps
server/nlu/requirements-train.txt
```

**Update `server/nlu_server.py`** (or the lazy_nlu.rs path) to point to `src-tauri/resources/server/nlu/model/` instead of `server/nlu/model/` for dev mode.

**Savings:** 35.8 MB tracked size

**Risk:** Dev NLU server needs path update. The production path (in `resources/`) is unchanged.

---

## Phase 5: Remove Duplicate Server Scripts

**Rationale:** `server/nlu_server.py` is an EXACT DUPLICATE of `src-tauri/resources/server/nlu_server.py` (MD5: `957208DDCC2427514DA07E743F37ACB4` for both). However, `server/stt_server.py` is DIFFERENT from `src-tauri/resources/server/stt_server.py` (different MD5 hashes), so the dev copy should be kept.

**Action:**
```bash
git rm server/nlu_server.py
```

**Keep:** `server/stt_server.py` (dev version, different from production)

**Savings:** 0.01 MB tracked size (negligible, but removes confusion)

---

## Phase 6: Clean Build Artifacts

**Rationale:** 38.8 GB of Rust build artifacts is excessive. These are fully regenerable via `cargo build`.

**Action:**
```bash
# Clean Rust build artifacts (frees 38.8 GB)
cd C:\PROJECTS\ULTRON\src-tauri
cargo clean

# Optional: prune npm node_modules (frees 459 MB)
# Only do this if you're not actively developing
npm --prefix C:\PROJECTS\ULTRON\frontend prune --omit=dev
npm --prefix C:\PROJECTS\ULTRON\server\worker prune --omit=dev
```

**Savings:** 38.8 GB disk (cargo clean), 459 MB disk (npm prune)
**Risk:** Next build will be slower (cold compile). Only run `cargo clean` if you're not actively building.

---

## Phase 7: Post-Training Data Deletion Policy

**Principle:** Once a model is trained and the ONNX file is exported, the training data is no longer needed on disk. The model file is the only artifact that matters. Training data can always be re-collected or re-generated.

### 7.1 Wake Word Data Deletion Schedule

| Stage | When | What to Delete | What to Keep |
|-------|------|---------------|--------------|
| 1. Pre-training | Now | Augmented data (673 MB) | Originals (16.6 MB) |
| 2. During training | Colab upload done | `wake_word_data.zip` (10.8 MB) | Originals |
| 3. Post-training | Model validated | Originals (16.6 MB) | Only `nexus.onnx` (415 KB or 2 MB) |
| 4. Future retraining | When collecting new data | Old originals | New originals + old model |

### 7.2 NLU Data Deletion

The NLU training data (`server/nlu/dataset.json`, 367 KB) is small and useful for future retraining. **Keep it.**

The NLU model (`server/nlu/model/`, 35.8 MB) is a duplicate — **delete the duplicate** (Phase 4). The production copy in `src-tauri/resources/server/nlu/model/` is kept.

### 7.3 TTS Model Deletion

The Piper TTS model (`src-tauri/resources/piper/en_US-amy-medium.onnx`, 60.3 MB) is the largest tracked file. It is actively used as the TTS fallback when edge-tts (cloud) is unavailable. **Keep it** unless you want to remove the local TTS fallback entirely.

If you decide to remove Piper TTS fallback:
1. Remove `piper-rs` from `Cargo.toml`
2. Remove `src-tauri/src/tts_piper.rs`
3. Remove piper references from `tts.rs`
4. `git rm src-tauri/resources/piper/`
5. Remove piper from `tauri.conf.json` bundle list
6. **Savings:** 60.3 MB tracked + 60.3 MB smaller app

### 7.4 General Rule

> **After any model is trained and the ONNX is validated, delete:**
> - All training data (originals + augmented)
> - All training notebooks (`.ipynb`)
> - All intermediate checkpoints (`.pt`, `.pth`)
> - All feature caches (`.npy`)
>
> **Keep only:**
> - The final `.onnx` model file (in `src-tauri/resources/`)
> - The training script/notebook (for reproducibility, in git)
> - The data collection script (for future re-collection, in `scripts/`)

---

## Summary of Savings

### Immediate (Phases 1-5)

| Phase | Action | Disk Saved | Tracked Size Saved |
|-------|--------|-----------|-------------------|
| 1 | Delete augmented wake word data | 673 MB | 0 (already gitignored) |
| 2 | Delete root scratch files | 1 MB | 0.02 MB (2 tracked files) |
| 3 | Remove sherpa resources from git | 34 MB | 34 MB |
| 4 | Remove duplicate NLU model from git | 36 MB | 36 MB |
| 5 | Remove duplicate nlu_server.py | 0.01 MB | 0.01 MB |
| **Total** | | **~744 MB disk** | **~70 MB tracked** |

### After build clean (Phase 6)

| Action | Disk Saved |
|--------|-----------|
| `cargo clean` | 38,836 MB |
| `npm prune` (optional) | 459 MB |
| **Total** | **~39.3 GB disk** |

### After model training (Phase 7)

| Action | Disk Saved |
|--------|-----------|
| Delete wake word originals | 16.6 MB |
| Delete wake_word_data.zip | 10.8 MB |
| Delete v2 training notebook | 0.05 MB |
| **Total** | **~27.5 MB disk** |

### Grand Total

| Category | Disk | Tracked Repo |
|----------|------|-------------|
| Immediate cleanup | 744 MB | 70 MB |
| Build clean | 39.3 GB | 0 |
| Post-training | 27.5 MB | 0.05 MB |
| **Grand Total** | **~40 GB** | **~70 MB** |

**Tracked repo size:** 318.7 MB → **~249 MB** (after Phases 2-5)
**Or ~189 MB** if Piper TTS fallback is also removed.

---

## Files to KEEP (Do NOT Delete)

### Critical app resources (bundled in the app)

```
src-tauri/resources/oww/melspectrogram.onnx      1.0 MB  (wake word mel model)
src-tauri/resources/oww/embedding_model.onnx     1.3 MB  (wake word embedding model)
src-tauri/resources/oww/nexus.onnx               0.4 MB  (wake word classifier)
src-tauri/resources/piper/en_US-amy-medium.onnx 60.3 MB  (TTS fallback)
src-tauri/resources/piper/en_US-amy-medium.onnx.json
src-tauri/resources/espeak-ng-data/              7.6 MB  (phoneme data)
src-tauri/resources/server/stt_server.py                 (STT server)
src-tauri/resources/server/nlu_server.py                (NLU server)
src-tauri/resources/server/nlu/model/           35.8 MB  (NLU model)
src-tauri/resources/server/nlu/requirements.txt
```

### Source code

```
src-tauri/src/             (Rust source)
frontend/src/              (Frontend source)
server/worker/src/         (Cloudflare Worker source)
server/sidecar/            (Python sidecar source)
server/stt_server.py       (dev STT server, different from production)
server/nlu/train.py        (NLU training script)
server/nlu/seed_dataset.py (NLU dataset seeding)
server/nlu/dataset.json    (NLU training data, 367 KB)
```

### Documentation

```
docs/                      (1.7 MB, all documentation)
AGENTS.md                  (project notes)
README.md
QUICKSTART.md
```

### Scripts

```
scripts/record_wake_samples.py    (wake word recorder)
scripts/augment_wake_samples.py   (augmentation pipeline)
scripts/test_mic_live.py          (mic health check)
scripts/run.ps1                   (unified console launcher)
scripts/restart_mic.ps1           (Intel SST restart)
# ... other scripts
```

### Config

```
.gitignore
.gitattributes
src-tauri/Cargo.toml
src-tauri/tauri.conf.json
frontend/package.json
frontend/vite.config.ts
server/worker/wrangler.toml
server/worker/package.json
nexus.mjs                      (unified CLI)
```

---

## Execution Checklist

### Phase 1: Delete augmented data
- [ ] `rm -rf wake_word_data/augmented`
- [ ] Verify originals still exist: `wake_word_data/{positive,negative,free,background}/`

### Phase 2: Delete root scratch files
- [ ] Delete all root `*.wav` files
- [ ] Delete all root `test_*.py` files
- [ ] Delete all root `*.cjs` files
- [ ] Delete root `*.txt` log files
- [ ] Delete `demo-v3.png`
- [ ] `git rm CHANGELOG_PREM22K.md newStep3.txt`
- [ ] Delete temp check scripts

### Phase 3: Remove sherpa
- [ ] `git rm -r src-tauri/resources/sherpa/`
- [ ] Edit `tauri.conf.json`: remove `"resources/sherpa/speaker_model.onnx",`
- [ ] Add `src-tauri/resources/sherpa/` to `.gitignore`

### Phase 4: Remove duplicate NLU model
- [ ] `git rm -r server/nlu/model/`
- [ ] Verify `src-tauri/resources/server/nlu/model/` still exists
- [ ] Update `server/nlu_server.py` or `lazy_nlu.rs` dev path if needed

### Phase 5: Remove duplicate server script
- [ ] `git rm server/nlu_server.py`
- [ ] Verify `src-tauri/resources/server/nlu_server.py` still exists

### Phase 6: Clean build artifacts (optional)
- [ ] `cargo clean` (frees 38.8 GB)
- [ ] `npm --prefix frontend prune --omit=dev` (optional)

### Phase 7: Post-training cleanup (after v3 model is trained)
- [ ] Verify `nexus_v3.onnx` is downloaded and placed in `src-tauri/resources/oww/nexus.onnx`
- [ ] Verify model loads: `cargo test --features wakeword-oww`
- [ ] Verify live detection works
- [ ] Delete `wake_word_data/positive/`
- [ ] Delete `wake_word_data/negative/`
- [ ] Delete `wake_word_data/free/`
- [ ] Delete `wake_word_data/background/`
- [ ] Delete `wake_word_data.zip`
- [ ] Delete `train_nexus_oww_v2.ipynb` (superseded by v3)
- [ ] Keep `train_nexus_oww_v3.ipynb` (for future retraining)
- [ ] Keep `scripts/record_wake_samples.py` and `scripts/augment_wake_samples.py`

### Final verification
- [ ] `cargo check` passes
- [ ] `npm run build` passes
- [ ] `cargo test` passes
- [ ] App launches and wake word works
- [ ] `git status` shows clean working tree
