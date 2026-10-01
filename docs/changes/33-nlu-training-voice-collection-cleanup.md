# NLU Training Pipeline + Voice Collection + Auto-Cleanup

**Date:** 2026-09-12
**Scope:** Complete retraining workflow for contributors — `nexus collect`, `nexus train`, `nexus build`
**Goal:** Enable any contributor to collect real voice samples and retrain the BERT-Mini NLU model with a single command, with automatic cleanup of temporary files.

---

## Summary

This change adds two new CLI commands (`nexus collect`, `nexus train`), expands the synthetic data generator, adds automatic cleanup of temporary files after training, and creates comprehensive documentation for the entire NLU training workflow.

| What | Before | After |
|------|--------|-------|
| Contributor retraining | Manual: 6 separate Python scripts | One command: `nexus train` |
| Voice sample collection | Did not exist | `nexus collect` — interactive, Groq auto-load |
| Temp file cleanup | Manual (17 MB left on disk) | Automatic (Step 7 of `nexus train`) |
| Synthetic data | ~478 live-mode examples | ~1097 live-mode examples |
| Negative examples | ~15 | ~58 (prevents intent confusion) |
| Documentation | Scattered across files | 1 detailed guide (1100+ lines) |

---

## 1. `nexus collect` — Interactive Voice Sample Collector

### New file: `scripts/collect_nlu_samples.py`

**Purpose:** Prompts the user with phrases to speak, records their voice, transcribes via Groq cloud (auto-loaded API key), and saves real transcripts as training data for BERT-Mini.

### How it works

```
User runs: nexus collect
    ↓
Shows phrase: "type hello world"
    ↓
User presses Enter (when ready, no rush)
    ↓
Countdown: 3...2...1...SPEAK! (big visible boxes)
    ↓
Records 2 seconds of audio (16kHz mono)
    ↓
Transcribes via Groq cloud (API key auto-loaded from NEXUS settings.json)
    ↓
Shows: Heard: "type hello world"
    ↓
Auto-saves to collected_samples.jsonl
    ↓
1.5s gap (breathe, prepare)
    ↓
Next phrase...
    ↓
User presses Ctrl+C to stop (all samples saved)
    ↓
User runs: nexus train
```

### Key design decisions

**Groq cloud as primary transcription (not STT server):**
- The STT server (port 39217) only runs when NEXUS is running
- Contributors may want to collect samples without starting NEXUS
- Groq key is auto-loaded from `%APPDATA%/com.nexus.assistant/settings.json` → `groqApiKey`
- No environment variable needed — if you've configured Groq in NEXUS, it works
- STT server is silent fallback (no error printed if not running)

**Enter to record (not continuous):**
- User requested Enter key for each sample
- No rush — take your time between samples
- 1.5s gap after each save (time to breathe)
- 10 samples per intent = 1 collection (default `--count 10`)

**2-second recording (not 4):**
- User requested shorter recording
- 2s is enough for most voice commands ("type hello world", "press enter")
- Faster collection cycle

**Countdown boxes (not inline text):**
- User requested visible countdown
- Big ASCII boxes on separate lines:
  ```
  ╔═══════╗
  ║   3   ║
  ╚═══════╝
  ```
- 0.8s between each number
- Much easier to follow than `3... 2... 1... SPEAK!`

**Ctrl+C to stop (not quit prompt):**
- Samples are saved immediately (not buffered)
- Nothing is lost on interrupt
- Clean exit message with summary

### 23 intents covered

| Intent | Phrases | Example |
|--------|---------|---------|
| `type_text` | 15 | "type hello world" |
| `press_key` | 20 | "press enter" |
| `press_hotkey` | 20 | "press ctrl a" |
| `confirm_send` | 24 | "send it", "go ahead" |
| `cancel_action` | 25 | "stop", "cancel that" |
| `browser_new_tab` | 20 | "new tab" |
| `browser_navigate` | 20 | "go to github.com" |
| `browser_search` | 15 | "search for cats in browser" |
| `whatsapp_open` | 20 | "open whatsapp" |
| `whatsapp_search` | 20 | "find mom in whatsapp" |
| `focus_app` | 20 | "focus chrome" |
| `open_app` | 20 | "open chrome" |
| `close_app` | 20 | "close chrome" |
| `search` | 20 | "search for cats" |
| `open_settings` | 20 | "open settings" |
| `media_play_pause` | 20 | "play", "pause" |
| `media_next` | 20 | "next song" |
| `media_previous` | 19 | "previous" |
| `media_stop` | 20 | "stop music" |
| `greeting` | 20 | "hello", "hey nexus" |
| `analyse_repo` | 20 | "analyse the repo zync" |
| `list_prs` | 20 | "list prs" |
| `unknown` | 20 | "what is the meaning of life" |

### Output format

`server/admin/data/collected_samples.jsonl` (gitignored, admin-only):

```json
{"text": "type hello world", "intent": "type_text", "slots": {"text": "hello world"}, "source": "voice", "timestamp": 1789231579.45}
```

### Groq API key auto-load

The collector tries these sources in order:
1. `GROQ_API_KEY` environment variable
2. `%APPDATA%/com.nexus.assistant/settings.json` → `groqApiKey` (Windows)
3. `~/.config/com.nexus.assistant/settings.json` (Linux/macOS)
4. `server/admin/settings.json` (dev fallback)

No environment variable needed — if you've configured Groq in NEXUS settings, the collector finds it automatically.

---

## 2. `nexus train` — Full Training Pipeline (7 Steps)

### Updated file: `server/nlu/train_all.py`

**Purpose:** One command to clean, generate, merge, train, export, sync, and clean up.

### 7-step pipeline

```
Step 1: Clean dataset
  → Fix malformed labels (OpenArchitect → open_architect)
  → Convert to lowercase
  → Deduplicate by (text, intent)

Step 2: Generate synthetic examples
  → Run generate_live_data.py
  → Produces ~1097 new examples across 11 live-mode intents
  → Includes STT distortions, filler words, negative examples

Step 3: Merge all data
  → Merge generated examples into dataset.json
  → Merge collected voice samples from collected_samples.jsonl
  → Deduplicate everything

Step 3b: Re-clean after merge
  → Fix any new malformed labels
  → Remove duplicates introduced by merge

Step 4: Train BERT-Mini
  → Load google/bert_uncased_L-2_H-128_A-2 (4.4M params)
  → Fine-tune for 50 epochs on CPU
  → Joint intent classification + slot filling
  → Class-weighted loss to handle imbalance
  → Save best_model.pt (best validation accuracy)

Step 5: Export to ONNX
  → Load best_model.pt
  → Export to nexus_nlu.onnx (16.8 MB, max_length=64)
  → Save labels.json (intent + slot label lists)

Step 6: Sync to resources
  → Copy nexus_nlu.onnx → src-tauri/resources/server/nlu/model/
  → Copy labels.json → src-tauri/resources/server/nlu/model/
  → Copy tokenizer/ → src-tauri/resources/server/nlu/model/
  → Ready for `nexus build` to bundle into installer

Step 7: Clean up temporary files (save ~17 MB disk space)  ← NEW
  → Delete new_examples.json (150 KB, merged into dataset.json)
  → Delete model/best_model.pt (17 MB, ONNX is the production format)
  → Delete collected_samples.jsonl (merged into dataset.json)
  → Keep: dataset.json, nexus_nlu.onnx, labels.json, tokenizer/
```

### Step 7 — Auto-cleanup (NEW)

**Why:** After training + ONNX export + sync, these files are no longer needed:
- `new_examples.json` (150 KB) — generated synthetic data, already merged into `dataset.json`
- `model/best_model.pt` (17 MB) — PyTorch checkpoint, ONNX is the production format
- `collected_samples.jsonl` — voice samples, already merged into `dataset.json`

**Total freed:** ~17 MB per training run

**Files kept:**
| File | Size | Why |
|------|------|-----|
| `dataset.json` | ~500 KB | Merged dataset (needed for future retraining) |
| `nexus_nlu.onnx` | ~17 MB | Production ONNX model (used by NLU server) |
| `labels.json` | ~2 KB | Intent + slot label lists |
| `tokenizer/` | ~1 MB | BERT-Mini tokenizer files |
| `approved_phrasings.jsonl` | varies | Brain-collected data (admin-only) |
| `rejected_examples.jsonl` | varies | Brain-collected rejections (admin-only) |

**`--keep-temp` flag:** For debugging, keeps all temp files:
```bash
nexus train --keep-temp    # keeps best_model.pt + new_examples.json + collected_samples.jsonl
```

### Merge of collected voice samples (NEW)

`train_all.py` now reads `server/admin/data/collected_samples.jsonl` during Step 3 and merges the voice-collected samples into `dataset.json`:

```python
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
```

---

## 3. Expanded Synthetic Data Generator

### Updated file: `server/nlu/generate_live_data.py`

**Expansion:** From ~478 to ~1097 generated examples across 11 live-mode intents.

### What was expanded

| Generator | Before | After | Expansion |
|-----------|--------|-------|-----------|
| `gen_type_text()` | ~40 examples | ~120 examples | More text samples, STT distortions, filler words |
| `gen_press_key()` | ~35 examples | ~110 examples | More keys, phrasings ("hit", "tap", "press the X key") |
| `gen_press_hotkey()` | ~35 examples | ~110 examples | More combos, STT distortions |
| `gen_confirm_send()` | ~20 examples | ~70 examples | 24→70 phrasings ("ship it", "fire it off", "make it so") |
| `gen_cancel_action()` | ~15 examples | ~70 examples | 25→70 phrasings ("halt", "cease", "belay that") |
| `gen_browser_new_tab()` | ~15 examples | ~60 examples | More phrasings, STT distortions |
| `gen_browser_navigate()` | ~40 examples | ~90 examples | More URL patterns, verbs ("pull up", "fetch", "render") |
| `gen_browser_search()` | ~40 examples | ~90 examples | More search engines, query patterns |
| `gen_whatsapp_open()` | ~15 examples | ~60 examples | More STT distortions ("whats app", "what sap") |
| `gen_whatsapp_search()` | ~35 examples | ~90 examples | More contact patterns, verbs ("message", "text") |
| `gen_focus_app()` | ~30 examples | ~90 examples | More apps, verbs ("activate", "raise", "foreground") |
| `gen_negative_examples()` | ~15 examples | ~58 examples | Prevents intent confusion (see below) |

### Negative examples (prevents intent confusion)

Added 58 negative examples that teach the model to distinguish between similar commands:

| Confusion | Negative Example |
|-----------|-----------------|
| `type_text` vs `open_app` | "type notepad" → type_text (not open_app) |
| `press_key` vs `open_app` | "press chrome" → press_key (not open_app) |
| `confirm_send` vs `search` | "send cats" → confirm_send (not search) |
| `cancel_action` vs `media_stop` | "stop" → cancel_action, "stop music" → media_stop |
| `browser_new_tab` vs `open_app` | "new tab" → browser_new_tab (not open_app) |
| `browser_navigate` vs `open_app` | "go to github.com" → navigate, "go to github" → open_app |
| `browser_search` vs `search` | "search for cats in browser" → browser_search, "search for cats" → search |
| `focus_app` vs `open_app` | "focus chrome" → focus_app, "open chrome" → open_app |
| `press_key` vs `press_hotkey` | "press enter" → press_key, "press ctrl enter" → press_hotkey |
| `cancel_action` vs `close_app` | "cancel" → cancel_action, "close chrome" → close_app |

### STT distortions added

| Correct | STT Distortion | Intent |
|---------|---------------|--------|
| "send" | "sand", "sent", "cent" | confirm_send |
| "stop" | "stoop", "stomp", "stow" | cancel_action |
| "tab" | "tap", "tad", "tan" | browser_new_tab |
| "for" | "four" | browser_search, whatsapp_search |
| "up" | "app" | browser_search |
| "google" | "goggle" | browser_search |
| "whatsapp" | "whats app", "what sap", "what's app", "watsap" | whatsapp_open |
| "to" | "too" | browser_navigate, focus_app |
| "with" | "wit" | whatsapp_search |
| "message" | "massage" | whatsapp_search |

### Filler words added

"um", "uh", "like", "hey nexus", "nexus", "ok", "okay", "so", "and", "now", "actually", "hmm"

---

## 4. `nexus build` — Installer Bundling

### Updated file: `nexus.mjs`

**`syncNluModel()`** runs before every build:
1. Copies `nexus_nlu.onnx` from `server/nlu/model/` to `src-tauri/resources/server/nlu/model/`
2. Copies `labels.json`
3. Copies `tokenizer/` directory
4. Tauri bundles everything in `resources/` into the installer

### `tauri.conf.json` resources

```json
"resources": [
    "resources/server/nlu/model/nexus_nlu.onnx",
    "resources/server/nlu/model/nexus_nlu.onnx.data",
    "resources/server/nlu/model/labels.json",
    "resources/server/nlu/model/tokenizer/*",
    ...
]
```

At runtime, the NLU server reads the model from:
```
exe_dir/resources/server/nlu/model/nexus_nlu.onnx
```

---

## 5. CLI Commands

### `nexus.mjs` — New commands

| Command | Function | Purpose |
|---------|----------|---------|
| `nexus collect` | `cmdCollect()` | Interactive voice sample collector |
| `nexus train` | `cmdTrain()` | Full retraining pipeline (7 steps) |

### `nexus collect` flags

```bash
nexus collect                    # all 23 intents, 10 samples each
nexus collect --intent type_text # specific intent only
nexus collect --count 20         # 20 samples per intent
nexus collect --list             # list all intents
nexus collect --text-only        # no microphone (save phrases directly)
```

### `nexus train` flags

```bash
nexus train              # full pipeline (clean → generate → merge → train → export → sync → cleanup)
nexus train --clean-only # just clean the dataset
nexus train --skip-train # everything except training (data refresh)
nexus train --keep-temp  # keep temp files for debugging (~17 MB)
```

### Help text added

```
Commands:
  train    Retrain the BERT-Mini NLU model (clean → generate → train → export)
  collect  Collect real voice samples for NLU training (speak phrases → transcribe → save)

Examples:
  nexus train       # retrain the NLU model with new data
  nexus collect     # speak phrases → save real voice samples for training
```

---

## 6. Documentation

### New file: `docs/features/51-nlu-training-and-voice-collection.md`

**1100+ lines** covering:

1. Overview — Why voice collection matters
2. Architecture — Full ASCII diagram
3. `nexus collect` — Interactive mode, Groq auto-load, countdown, flags
4. `nexus train` — 7-step pipeline, hyperparameters, cleanup
5. `nexus build` — Installer bundling
6. BERT-Mini Model — Architecture, ONNX export
7. Dataset Format — JSON schema
8. Intent Catalog — All 52 intents with slots and examples
9. Slot Types — All 45 BIO tags
10. File Map — Every script, data file, gitignore status, deleted-after-train status
11. Data Flow Diagram — ASCII flow
12. End-to-End Workflow — Contributor, admin, quick iteration
13. Advanced Usage — Adding intents, slot types, custom phrases
14. Troubleshooting — Every common error with fixes
15. Admin-Only Brain Integration — Auto-collection, brain retraining
16. API Reference — Python API, HTTP endpoints, ports

### Updated files

| File | Change |
|------|--------|
| `docs/features/README.md` | Added entry #51 |
| `docs/README.md` | Added entry #51 |
| `docs/changes/CHANGELOG.md` | Added this change entry |
| `AGENTS.md` | Added `nexus train` + `nexus collect` documentation |

---

## 7. Files Changed

### New files

| File | Lines | Purpose |
|------|-------|---------|
| `scripts/collect_nlu_samples.py` | 950 | Interactive voice sample collector |
| `docs/features/51-nlu-training-and-voice-collection.md` | 1100+ | Complete training guide |
| `docs/changes/33-nlu-training-voice-collection-cleanup.md` | 350+ | This change log |

### Updated files

| File | Change |
|------|--------|
| `server/nlu/train_all.py` | Added Step 7 cleanup, `merge_collected_samples()`, `--keep-temp` flag |
| `server/nlu/generate_live_data.py` | Expanded all generators (478 → 1097 examples), added negative examples |
| `nexus.mjs` | Added `cmdCollect()`, `cmdTrain()`, help text, switch cases |
| `AGENTS.md` | Added `nexus train` + `nexus collect` documentation |
| `docs/features/README.md` | Added entry #51 |
| `docs/README.md` | Added entry #51 |
| `docs/changes/CHANGELOG.md` | Added this change entry |

---

## 8. Testing

### Verified

- `nexus collect --list` — lists all 23 intents with phrase counts
- `nexus collect --text-only --yes` — saves samples to JSONL correctly
- `nexus collect --intent type_text --count 3 --text-only --yes` — specific intent works
- Groq API key auto-load — loads from `%APPDATA%/com.nexus.assistant/settings.json`
- `nexus train --skip-train` — dataset goes from 2438 → 3440 examples
- `cleanup_temp_files()` — deletes 16.9 MB (new_examples.json + best_model.pt)
- `nexus help` — shows `train` and `collect` commands
- Dataset cleaning — fixes malformed labels (OpenArchitect → open_architect)

### Not yet tested (requires user interaction)

- `nexus collect` with real microphone (needs user to speak)
- `nexus train` full pipeline (needs ~5-15 min CPU training)
- `nexus build` with new model (needs full build)

---

## 9. End-to-End Workflow

### For a new contributor

```bash
# 1. Clone and setup
git clone <repo-url>
cd ULTRON
nexus install

# 2. Collect voice samples (10-30 min)
#    Press Enter → 3...2...1...SPEAK! → 2s record → save
#    Groq key auto-loaded from NEXUS settings
#    Ctrl+C to stop anytime
nexus collect

# 3. Retrain model (5-15 min)
#    Auto-deletes temp files (~17 MB freed)
nexus train

# 4. Build installer (5 min)
nexus build

# 5. Test
nexus start
# Say: "type hello world", "press enter", "open whatsapp"
```

### For quick iteration (no voice collection)

```bash
nexus train    # regenerate synthetic data + retrain + cleanup
nexus build    # bundle into installer
```

### For debugging (keep temp files)

```bash
nexus train --keep-temp    # keeps best_model.pt + new_examples.json
nexus build
# Inspect best_model.pt or new_examples.json
# Run nexus train (without --keep-temp) to clean up
```

---

## 10. Disk Space Impact

### After `nexus collect`

| File | Size | Gitignored? |
|------|------|-------------|
| `server/admin/data/collected_samples.jsonl` | ~50 KB (230 samples) | Yes (admin-only) |

### After `nexus train` (with cleanup)

| File | Size | Kept? |
|------|------|-------|
| `server/nlu/dataset.json` | ~500 KB | Yes |
| `server/nlu/model/nexus_nlu.onnx` | ~17 MB | Yes |
| `server/nlu/model/labels.json` | ~2 KB | Yes |
| `server/nlu/model/tokenizer/` | ~1 MB | Yes |
| `src-tauri/resources/server/nlu/model/` | ~18 MB | Yes (committed) |
| `server/nlu/new_examples.json` | ~150 KB | **Deleted** |
| `server/nlu/model/best_model.pt` | ~17 MB | **Deleted** |
| `server/admin/data/collected_samples.jsonl` | ~50 KB | **Deleted** |

**Total kept:** ~36 MB (dataset + ONNX + labels + tokenizer + resources copy)
**Total freed:** ~17 MB (temp files deleted)

### After `nexus train --keep-temp`

Same as above but temp files are kept (+17 MB).
