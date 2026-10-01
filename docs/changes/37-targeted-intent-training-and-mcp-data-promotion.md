# Change 37 — Targeted Intent Training, Category Drill-Down & MCP Split Promotion

**Date**: 2026-09-22  
**Type**: Feature / Data Engineering / Fix  
**Files Modified**:
- `scripts/collect_nlu_samples.py`
- `scripts/nlu_stats.py`
- `server/nlu/dataset.json`
- `server/nlu/data/split_lock.json`
- `server/nlu/data/evaluation_lock.json`
- `server/nlu/promote_mcp_intents.py` (new)

---

## 1. Summary of Changes

- **CLI Flag Expansion (`scripts/collect_nlu_samples.py`)**:
  - Added `-i` / `--intent` and `-c` / `--category` flags with combined validation.
  - Added interactive category drill-down submenu to allow picking specific intents right from terminal menu.
  - Added phrase catalogs for all 23 missing intents (PRs, workflows, releases, collaborators, orgs, open_url, open_architect), covering all 57 intents.
  - Implemented regex slot extractors for all intents (`create_pr`, `comment_pr`, `analyse_pr`, `add_collaborator`, `delete_branch`, etc.).
- **Data Foundation Promotion & Step 3d Fix (`server/nlu/`)**:
  - Identified Step 3d quarantine cause (missing test rows for new intents).
  - Promoted `order_food`, `search_product`, and `send_whatsapp_message` with isolated phrase families into `dataset.json`.
  - Re-minted `split_lock.json` and `evaluation_lock.json` (481 evaluation rows).
  - Validated zero family cross-split leakage.
- **Stats Alignment (`scripts/nlu_stats.py`)**:
  - Aligned categories with the 55 BERT-Mini production intents.
  - Verified that all MCP intents now show full trained rows + voice counts in `nexus stats`.

---

## 2. Verification

- `python scripts/collect_nlu_samples.py --list` -> 57 intents across 7 categories.
- `python scripts/collect_nlu_samples.py --category github --intent create_pr --count 2 --text-only --yes` -> OK.
- `python scripts/collect_nlu_samples.py --intent order_food --count 2 --text-only --yes` -> OK.
- `python server/nlu/data_foundation.py validate` -> 0 errors.
- `cargo test --lib -- --test-threads=1` -> 520 / 520 tests passed.
