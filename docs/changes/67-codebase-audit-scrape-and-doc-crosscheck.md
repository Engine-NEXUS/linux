# Codebase Audit, Scrape & Doc Cross-Check (2026-10-02)

## 0. Purpose & Scope

Full-repo audit for dead weight (unused files, imports, dead code, duplicated models) with a doc cross-check gate before every deletion, so no cleanup breaks documented architecture. All work is **staged but uncommitted** — review with `git status` / `git diff --cached`, then commit.

Prior art this record merges with: `docs/cleanup-plan.md` (the earlier audit — several of its claims were re-verified and two were found stale; see §6).

---

## 1. Disk Picture (measured, not estimated)

Tracked in git: **~160 MB** (`git ls-files`, 1012 files). Everything large is gitignored scratch/build output:

| Location | Size | Status |
|---|---|---|
| `src-tauri/target/` | ~90 GB | Ignored. Intentionally **not** cleaned (would wipe the working release binary). Manual step: `cargo clean` when disk is needed. |
| `server/admin/` (Qwen GGUF) | ~469 MB | Ignored. Keep iff admin-brain work continues. |
| `wake_word_data/` 267 MB, `wake_camp/` 248 MB, `nexus_model/` 173 MB, `kaggle_dataset/` 88 MB, `reference_startmenu_themes/` 69 MB, `audioset_16k/`, `mit_rirs/`, `nexus_real_samples/` | ~1.1 GB total | Ignored. **Archive-first, never bare-delete** (§5). |
| `scripts/` (incl `en_US-libritts_r-medium.pt` 195 MB + nested `.git` packfiles under `scripts/kaggle_output/`) | ~364 MB | Ignored. |
| `frontend/node_modules/` 272 MB, `server/worker/node_modules/` 204 MB | ~476 MB | Ignored (worker deps = wrangler+vitest devDeps, normal). Reinstall on demand. |
| `frontend/dist/` | ~4.7 MB | Ignored build output. |
| `docs/` | ~3.9 MB | Tracked, fine. |

Largest tracked files (`git ls-files` by size): `resources/piper/en_US-amy-medium.onnx` 63 MB (offline TTS fallback — **keep**), NLU `.onnx` 17.6 MB ×2 + `.onnx.data` 17.6 MB ×2 (**de-duplicated this session**, §3), `public/silero_vad_v5.onnx` 2.3 MB (live — fetched by `frontend/src/audio/vad.ts:227`), `server/nlu/dataset.json` 0.9 MB (keep), eval locks ~0.6 MB (keep), `Cargo.lock` 0.2 MB (keep).

---

## 2. Verification Evidence (per scrape candidate)

| Candidate | Evidence of deadness | Method |
|---|---|---|
| `frontend/architect.html`, `pr-list.html`, `settings-sidebar.html` | Not in `vite.config.ts:42-51` inputs; zero Rust refs (only `dyn_windows.rs` URLs: index/loading/setup/settings/sidebar/stage/companion-hud) | vite input list + `Select-String` over `src-tauri/src` |
| `src/{architect,pr-list,settings-sidebar}/main.tsx` | No refs in `vite.config.ts`, `nexus.mjs`, `package.json`; components ship via `sidebar/main.tsx` → unified bundle | `Select-String` over config files |
| `src-tauri/src/tts_kokoro_backup.rs` (17.5 KB) | No `mod` declaration in `lib.rs` (mods: `tts`, `tts_edge`, `tts_piper`, `tts_network`, `tts_bench` only) — compiles to nothing | `lib.rs` mod inventory |
| `src-tauri/src/test_wda.rs` | Gitignored (`.gitignore:162`), no `mod` declaration, no refs | gitignore + grep |
| `frontend/src/components/LiquidGlassButton.tsx` | Exported, zero render sites (single match = its own definition) | grep `<LiquidGlassButton` |
| `walkdir = "2.5"` (Cargo) | Zero `walkdir::` hits; file walking uses `ignore::WalkBuilder` (`architect.rs:1572`) | grep |
| `live_glass` Acrylic fns | Zero callers since `dyn_windows.rs` removal; frontend never invokes `apply_live_glass` (grep: no hits) | grep + AGENTS history |
| `youtube_center::fetch_transcript` | No callers (orchestrator uses `search_videos` + `summarize_to_diary`); compiler warning | grep + build warnings |
| `sidebar_backdrop::capture_and_blur` (PNG) | Sole ref is its own doc comment; all callers use `_jpeg` | `Select-String` |
| `update.js…4`, `commands_diff.patch` | Zero refs in `*.{mjs,json,ps1,md}` | grep |
| `slot_str` (youtube) | **KEPT** — has callers in `validate()` (`youtube_center.rs:51,62,73`) | mid-flight correction, §6 |

False alarms cleared (live, do NOT delete): `@ricky0123/vad-web` (dynamic imports `vad.ts:217,264,418`), `clsx` (`Slider.tsx:3`), `tokens.css`/`liquid-glass.css` (5 CSS importers), `VoiceEnrollment.tsx` (`SetupApp.tsx:597`), all public ONNX/JSON, every other Cargo dep (spot-verified `use` lines).

---

## 3. Phase 1 — Disk-Only Reclaim (~25 MB, no repo effect)

Deleted 22 gitignored files: 15× `kaggle_logs*.txt`, `wake_word_data.zip` (10.8 MB), `run.log`, `train_phase11*.log`, `test_mic_dump.wav`, `test_slice.wav` (both zero refs in `scripts/`).

**Kept:** `test_doc.wav` — audit fixture referenced by `scripts/verify_hardened_model.py:78` and `scripts/compare_jarvis_vs_nexus.py:107`. `wake_word_data/augmented/` absent (nothing to do).

---

## 4. Phase 2 — Git Tracking (~35 MB de-duplicated, files intact on disk)

- `git rm --cached server/nlu/model/nexus_nlu.onnx server/nlu/model/nexus_nlu.onnx.data` — MD5-verified byte-identical to the resources copy (`0AB3EA…`, `644657…`). Training (`train.py:80` `OUTPUT_DIR = server/nlu/model`) regenerates locally; `syncNluModel()` (`nexus.mjs:501-573`) + resources fallback cover fresh clones and dev (`lazy_nlu.rs`).
- `.gitignore`: appended the two binary paths with rationale comment (+ pointer to `cleanup-plan.md` Phase 4).
- `git rm` tracked scratch: `update.js`, `update2.js`, `update3.js`, `update4.js`, `commands_diff.patch`, `newStep3.txt`, `CHANGELOG_PREM22K.md` (last two doc-pre-approved in `cleanup-plan.md:505`).

**Correction vs `cleanup-plan.md`:** its "exact duplicate" claim for `server/nlu_server.py` is **stale** — hashes differ (`A698…` vs `0B1F…`). Both copies kept; dev/prod divergence is real.

---

## 5. Phase 3 — Dead Source Removal

- Staged deletions (8 tracked): 3 dead HTML + 3 orphan `main.tsx` + `tts_kokoro_backup.rs` (+ pre-existing `test_wda.rs` was untracked — disk-only delete).
- Disk-only deletions (2 untracked): `test_wda.rs`, `LiquidGlassButton.tsx` (both verified absent from disk + index).
- `Cargo.toml`: removed `walkdir` (+ comment); `Cargo.lock` auto-purged (verified: no `name = "walkdir"`).
- `live_glass.rs`: deleted `mod win32` + `apply_live_blur_hwnd` + `apply_live_glass` + `apply_live_glass_cmd` (133 lines) + dead imports (`c_void`, `raw_window_handle`, `tauri::{AppHandle, Manager, Runtime}`); module doc updated to ADR-05 pointer. Hitbox registry + `register_glass_hitboxes` + tests kept (`stage.rs:199` depends on the query path).
- `lib.rs`: handler entry replaced with ADR-05 note; `register_glass_hitboxes` stays registered.
- `youtube_center.rs`: removed `fetch_transcript`. `sidebar_backdrop.rs`: removed PNG `capture_and_blur`, promoted the JPEG doc comment.
- Doc annotations: `features/85` (SUPERSEDED banner + standalone-page removal note), `features/16` (unified-sidebar supersession note + walkdir snippet fix). `changes/32` deliberately untouched (historical record).

**Withdrawn items (docs forbid):** standalone `settings/` window + `settings.html` (designed in `features/12`, explicitly kept in `features/62` + `changes/45`); `wake_camp/manifest.jsonl` + heldout (source of truth per `docs/wake-word/21:46,171`); capabilities (identifiers all match `tauri.conf.json` — earlier "mismatch" was filename-vs-identifier confusion).

---

## 6. Doc Cross-Check Table (the gate that shaped the plan)

| Candidate | Docs verdict | Rationale |
|---|---|---|
| Untrack `server/nlu/model/*` | FOLLOW DOCS (but reversed my first proposal) | `cleanup-plan.md:120,271` + `changes/33` (`syncNluModel`): canonical copy is `resources/`; dev copy is the duplicate |
| `server/nlu_server.py` removal | BLOCKED (stale doc claim) | Hashes differ — doc's MD5s predate divergence |
| Standalone settings window | BLOCKED | `features/12` design + `features/62`/`changes/45` explicit keep |
| Dead HTML ×3 | CONDITIONAL (annotate) | `features/85`, `features/16`, `changes/32` reference filenames — annotated 85/16, left 32 as history |
| Acrylic prune | ALIGNED | ADR-05 (`architecture/06`) + `changes/62` rule; research doc 02 is superseded exploration |
| `walkdir` removal | ALIGNED (+ doc touch-up) | `features/16:491` snippet updated |
| Training corpora deletion | CONDITIONAL (archive-first) | Gates doc requires manifest "when data exists"; `cleanup-plan.md` Cat C gates on v3; `wake-word/21` protects `wake_camp/` |
| `cargo clean` | DEFERRED (manual) | `cleanup-plan.md` Cat D approved, but wipes the working binary — operator decision |

---

## 7. Gates & Known Blocker

- `npx tsc --noEmit`: **0 errors**. `vitest`: **120/120**.
- `cargo check --lib --features custom-protocol`: **1 error, pre-existing, out of lane** — `nlu_client.rs:77` E0308 from another session's uncommitted change (`nlu_to_parsed_intent` 2→3 args for Feature 86 `screen_analysis`; `ParseResult` construction not updated). Zero errors in any file touched here; 1 pre-existing warning (`ocr.rs:16` unused import). Left unfixed — flag to the owning lane.

## 8. How To Proceed

1. Review: `git status`, `git diff --cached --stat`, spot-check `live_glass.rs` (~119 lines, hitbox-only) and `sidebar_backdrop.rs` (JPEG-only).
2. Commit (message suggestion): `chore: audit scrape — untrack NLU dev duplicate, drop dead pages/modules, prune DWM acrylic path`.
3. Optional follow-ups: `cargo clean` (90 GB, manual); archive training corpora (keep `wake_camp/manifest.jsonl`); resolve `nlu_client.rs:77` with the owning lane; full `node nexus.mjs build` once the blocker clears.
