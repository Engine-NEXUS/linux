# Targeted Intent Training, Category Drill-Down & MCP Data Promotion

**Date**: 2026-09-22  
**Status**: Implemented & Verified  
**Scope**: NLU Voice Collection CLI, Data Foundation, Step 3d Quarantine Repair, Intent Phrasings  

---

## 1. Executive Summary

This architecture milestone solves two core challenges in NEXUS NLU voice collection and training:
1. **Targeted Voice Training**: Enables granular intent and category training via CLI flags (`nexus collect --category github --intent create_pr` and `nexus collect --intent create_pr`) as well as an interactive category drill-down submenu. Phrase catalogs and regex slot extractors have been expanded to cover all 55 BERT-Mini intents + 2 live dictation modes (57 total intents).
2. **Zero-Quarantine MCP Data Foundation Promotion**: Diagnosed and resolved the root cause where newly added intents (`order_food`, `search_product`, `send_whatsapp_message`) had `0` trained rows in `nexus stats`. Staged examples and user voice recordings were promoted into `dataset.json` with family-isolated splits across `train`, `validation`, `calibration`, and `test` splits, re-minting cryptographic evaluation locks.

---

## 2. Targeted Intent Collection Architecture

### 2.1 CLI Flags & Scope Scenarios

| Command | Scope Behavior | Slot Extraction |
| :--- | :--- | :--- |
| `nexus collect --category github --intent create_pr` | Validates intent inside `github`, scopes to `create_pr` | `repo`, `title`, `head`, `base` |
| `nexus collect --intent order_food` | Auto-resolves category (`mcp`), scopes to `order_food` | `food_item`, `restaurant` |
| `nexus collect --category apps` | Scopes to all 6 apps intents, least-covered first | `app_name`, `url`, `query` |
| `nexus collect` (bare) | Interactive category menu with sub-intent drill-down | Full per-intent catalog |

### 2.2 Interactive Drill-Down Flow
```text
  What do you want to train?
    1. github     GitHub — PRs (merge/approve/close/list/create/comment), branch, actions, repo analysis
    2. mcp        Food / shopping / chat — order food, search Amazon, WhatsApp messages
    3. apps       Apps & system — open/close apps, settings, search, URL, architect
    4. messages   Messages & dictation — type text, confirmations, ghostwriter
    5. live       Live control — keys, hotkeys, browser tabs, URL navigation, focus
    6. media      Media & audio — play, pause, next, previous, stop
    7. random     Random mix — stratified across every tier (default)
  Pick [1-7] (Enter = random mix): 1

  Category 'github' contains 28 intents:
     1. merge_pr
     2. approve_pr
     ...
     6. create_pr
     ...
  Train all github intents (Enter), or pick specific [1-28 / name]: 6
  Scope: category 'github' -> intent 'create_pr'
```

---

## 3. Data Foundation Promotion & Quarantine Resolution

### 3.1 Root Cause Diagnosis
In `server/nlu/train_all.py` Step 3d (`enforce_split_hygiene()`):
```python
test_intents = {r.get("intent", "") for r in data.get("test", [])}
...
elif row.get("intent", "") not in test_intents:
    held.append(row)
```
Because `dataset.json["test"]` only contained the 52 legacy intents, any new intent lacked test evaluation rows. Step 3d moved 100% of their training rows into `phase11_new_intents_holding.jsonl`, leaving `0` dataset rows in `nexus stats`.

### 3.2 Formal Split Promotion
- Promoted **25 phrase families** for `order_food` (99 rows), **12 phrase families** for `search_product` (68 rows), and **12 phrase families** for `send_whatsapp_message` (127 rows).
- Allocated 2 distinct phrase families each to `test`, `validation`, and `calibration` splits, and the remaining 15+ families + all voice recordings to `train`.
- Verified 100% zero phrase-family overlap between splits via `prepare_evaluation_splits.validate_splits()`.
- Re-minted `split_lock.json` and `evaluation_lock.json` (481 total test rows).
- Validation passes cleanly via `python server/nlu/data_foundation.py validate`.

---

## 4. Verification & Status

1. **Rust Test Suite**: 520 / 520 passed (`cargo test --lib -- --test-threads=1`).
2. **Data Foundation**: `validate` passes with 481 evaluation rows and 0 errors.
3. **CLI Collection**: Verified category + intent combinations, single intent flags, and text-only non-interactive execution.
4. **Category Stats**: All MCP intents now show **32–107 dataset rows + 10 voice takes** and active `◐ Good` / `● Strong` mastery status in `nexus stats`.
