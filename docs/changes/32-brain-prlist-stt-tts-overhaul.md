# 32 — Admin Brain, PR-List Sidebar, STT Capture, TTS Fallback, NLU Expansion

> **Date**: 2026-09-05 → 2026-09-06
> **Type**: Multi-subsystem overhaul
> **Impact**: Critical — adds admin-only Qwen brain, voice-driven PR list, in-process STT capture, Edge TTS primary path, NLU expansion 7→46 intents, parser performance 177×, negative training loop
> **Span**: Commits `9b3f983` through `d780ff4` (15 commits) plus uncommitted working-tree changes
> **Test gates**: 314 Rust tests passing, frontend Vite build clean, NLU model 97.9% accuracy, end-to-end voice PR-list verified

---

## Table of Contents

1. [Summary](#summary)
2. [Commits Covered](#commits-covered)
3. [Subsystem 1 — Admin-Only Qwen Brain](#subsystem-1--admin-only-qwen-brain)
4. [Subsystem 2 — NLU Expansion 7→46 Intents](#subsystem-2--nlu-expansion-746-intents)
5. [Subsystem 3 — Deterministic Parser Performance + Missing Patterns](#subsystem-3--deterministic-parser-performance--missing-patterns)
6. [Subsystem 4 — PR-List Voice Sidebar](#subsystem-4--pr-list-voice-sidebar)
7. [Subsystem 5 — STT In-Process Capture (cpal direct)](#subsystem-5--stt-in-process-capture-cpal-direct)
8. [Subsystem 6 — TTS Edge-Primary / Piper-Fallback](#subsystem-6--tts-edge-primary--piper-fallback)
9. [Subsystem 7 — Orchestrator Routing Fixes](#subsystem-7--orchestrator-routing-fixes)
10. [Subsystem 8 — UI Animation Fixes](#subsystem-8--ui-animation-fixes)
11. [Subsystem 9 — Reference Theme Research (no code change)](#subsystem-9--reference-theme-research-no-code-change)
12. [File Change Inventory](#file-change-inventory)
13. [Verification Results](#verification-results)
14. [Known Follow-Ups](#known-follow-ups)

---

## Summary

This document covers every change made from the evening of 2026-09-05 through the current working tree on 2026-09-06. The work spans nine subsystems, fifteen commits, and a set of uncommitted refinements. The throughline is turning NEXUS from a deterministic-parser-only system into a layered intelligence (deterministic → Qwen brain → BERT-Mini NLU) with a voice-driven PR-list sidebar, in-process STT capture that bypasses the broken `getUserMedia` path on Intel SST drivers, and a TTS stack that uses cloud Edge TTS as primary with Piper only as a network-down fallback.

---

## Commits Covered

| Hash | Date | Subject |
|---|---|---|
| `9b3f983` | 2026-09-05 | feat(nlu): expand intent labels 7→46 + seed 2185 balanced examples |
| `9454e3d` | 2026-09-05 | feat(brain): Qwen 0.5B brain server + pronunciation learning + auto-train monitor |
| `ba58d4a` | 2026-09-05 | feat(brain): wire brain monitor into transcript pipeline + auto-retrain script |
| `efc6416` | 2026-09-05 | feat(brain): admin isolation + compile-time gate + lazy_brain + negative training |
| `894f1f5` | 2026-09-05 | feat(brain): negative training + verbal 'wrong' feedback + execution failure reporting |
| `050ef82` | 2026-09-05 | fix(tts): use cloud Edge TTS as primary, Piper only when network is down |
| `f502910` | 2026-09-05 | perf(parser): cache all regexes + add 8 missing command patterns |
| `0122ee7` | 2026-09-05 | fix(brain): wire up complete training pipeline — 6 broken links fixed |
| `103faf4` | 2026-09-05 | fix(mic): silence-recovery skips restart during baton pass + STT corrections |
| `2ff4814` | 2026-09-05 | feat(pr-list): voice-driven PR list sidebar with Merge + Analyse buttons |
| `4fc8bce` | 2026-09-06 | fix(pr-list): match sidebar height to 1000px (same as other sidebars) |
| `9999856` | 2026-09-06 | feat(pr-analysis): structured 4-section output — impact, bugs, stats, merge conflicts |
| `fcea326` | 2026-09-06 | fix(stt): capture audio from cpal stream directly — bypass getUserMedia |
| `04fd346` | 2026-09-06 | fix(brain+parser): enable admin-brain in builds + fuzzy list fallback |
| `d780ff4` | 2026-09-06 | fix(brain+stt): try brain before NLU + upgrade STT to base.en + admin config path |

Plus uncommitted working-tree refinements (UI animation timing, PR-list close button removal, NLU dataset cleanup, repo sanitization, reference theme research).

---

## Subsystem 1 — Admin-Only Qwen Brain

### Goal

A ~350–400 MB local Qwen 0.5B model that runs **only on the administrator's laptop**, continuously observes speech/commands, cross-checks STT + deterministic parser + BERT-Mini, learns pronunciation mistakes and new phrasings, and retrains BERT-Mini. The brain must never ship to normal users.

### Architecture

```
Administrator speaks
    ↓
STT transcript
    ↓
Deterministic parser  →  BERT-Mini NLU
    ↓                        ↓
Admin-only Qwen brain observes + cross-checks (non-blocking)
    ↓
Detects: disagreements, low confidence, unknown commands,
         pronunciation variants, filler words, new phrasing
    ↓
approved_phrasings.jsonl  /  rejected_examples.jsonl
    ↓
merge_and_train.py  →  train.py  →  BERT-Mini ONNX  →  hot-swap
```

### Phase A — Admin folder isolation + gitignore

- Moved `server/brain_server.py` → `server/admin/brain_server.py`
- Moved `server/brain/model/` → `server/admin/model/`
- Created `server/admin/data/` for runtime data (approved, rejected, pronunciation map)
- Created `server/admin/admin_config.json` (runtime `is_admin` flag)
- Created `server/admin/requirements-brain.txt`
- `.gitignore`: entire `server/admin/` folder ignored — brain never committed
- `brain_server.py`: updated `MODEL_DIR` and `pronunciation_map` paths
- `brain_monitor.rs`: `brain_data_dir()` now uses `server/admin/data/`
- `merge_and_train.py`: reads from `server/admin/data/`

### Phase B — Compile-time `admin-brain` feature flag

- `src-tauri/Cargo.toml`: added `admin-brain = []` feature
- `src-tauri/src/lib.rs`: `brain_client`, `brain_monitor`, `lazy_brain` modules gated with `#[cfg(feature = "admin-brain")]`
- `src-tauri/src/orchestrator.rs`: brain_monitor calls gated
- `src-tauri/src/intent_parser.rs`: `brain_classify` + `brain_monitor` calls gated
- Normal builds (`cargo build`): brain code **compiled out entirely**
- Admin builds (`cargo build --features admin-brain`): brain code included
- `nexus.mjs`: now passes `--features custom-protocol,admin-brain` so the brain ships in the admin's local build

### Phase C — Runtime `admin.json` config check

**New file: `src-tauri/src/admin_config.rs`** (165 lines)
- Reads `%APPDATA%/com.nexus.assistant/admin.json` at runtime
- Falls back to `server/admin/admin_config.json` in dev
- `is_admin()` returns false if:
  1. `admin-brain` feature not enabled (compile-time), OR
  2. `admin.json` doesn't exist, OR
  3. `is_admin: false` in the config
- Caches config in `OnceLock` (loaded once per process)
- Provides `brain_port()`, `retrain_threshold()`, `min_confidence()`
- `brain_client.rs`: checks `is_admin()` before any network call
- `brain_monitor.rs`: checks `is_admin()` before spawning monitor task

### Phase D — `lazy_brain.rs` (admin-only sidecar launcher)

**New file: `src-tauri/src/lazy_brain.rs`** (241 lines)
- Mirrors `lazy_nlu.rs` pattern
- Finds `brain_server.py` in `server/admin/` (dev + production paths)
- Finds Python interpreter (PATH, registry, common installs)
- Spawns brain server with `CREATE_NO_WINDOW` on Windows
- Waits up to 60s for `/health` to respond (Qwen model load takes time)
- **No idle timeout** — always loaded per admin's requirement ("run non-stop until all commands understood")
- `kill_brain()` for clean shutdown
- `brain_client.rs`: calls `ensure_brain_running()` before classify

### Phase E — Negative training (discard wrong examples)

- `brain_monitor.rs`: `RejectedExample` struct + `rejected_examples_path()`
- `brain_monitor.rs`: `report_execution_failure()` — called by orchestrator when a command fails (GitHub API error, app not found, etc.). Writes the bad phrasing to `rejected_examples.jsonl`
- `merge_and_train.py`: reads `rejected_examples.jsonl`, **removes** matching examples from BOTH train and test sets before retraining. Bad examples can never re-enter the dataset — BERT-Mini unlearns.
- Fuzzy matching added: `merge_and_train.py` uses Levenshtein similarity ≥ 0.8 so "close chrome browser" rejected → "close the chrome" also blocked

### Phase F — Verbal 'wrong' detection + complete feedback loop

- `brain_monitor.rs`: `is_verbal_wrong()` — detects "wrong", "no", "nope", "that was wrong", "not right", "incorrect", "bad", "mistake", etc.
- `brain_monitor.rs`: `record_last_command()` — tracks the last command's transcript + intent
- `brain_monitor.rs`: `report_verbal_wrong()` — marks the LAST command as bad, adds to `rejected_examples.jsonl`
- `orchestrator.rs`: detects verbal "wrong" BEFORE routing, reports it, returns immediately (meta-command, not a real command)
- `orchestrator.rs`: records every command after parsing
- `orchestrator.rs`: reports execution failure at Worker error + GitHub error events

**Complete feedback loop:**
1. Admin speaks a command
2. Deterministic + BERT-Mini parse it
3. Brain monitors in background (non-blocking)
4. Command executes
5a. SUCCESS → brain auto-approves phrasing → adds to training data
5b. FAILURE → brain marks as bad → adds to `rejected_examples.jsonl`
6. Admin says "wrong" → brain marks LAST command as bad
7. Next retrain: bad examples removed, good examples added
8. BERT-Mini gets BETTER and never repeats mistakes

### Phase G — Brain→ParsedIntent mapping completion

- `nlu_client::nlu_to_parsed_intent` made public
- `brain_client` delegates to it for all 46 intents
- Slot normalization added (Qwen returns numbers as strings)
- GitHub commands from brain now execute instead of falling to Worker
- NLU result passed to brain monitor (was always `None`) — orchestrator spawns a background tokio task to call NLU when deterministic parser misses, passes result to `brain_monitor`. Non-blocking.
- Brain can now cross-check all 3: deterministic vs NLU vs brain.
- Execution success reporting wired (was never called) — `report_execution_success()` calls after local command execution, Worker backend success, GitHub Text/NeedsConfirmation success. Writes to `approved_phrasings.jsonl` as `execution_verified` source with confidence=1.0. Counts toward retrain threshold (50 examples).
- Low-confidence rejections written (were silently dropped) — brain outputs with confidence 0.30–0.90 written to `rejected_examples.jsonl` with `reason='low_confidence'`. Below 0.30 treated as noise (ignored).

### Phase H — Brain-before-NLU ordering + admin config path fix

The brain (Qwen) was never running because:
1. `admin.json` was at `server/admin/` but runtime check looked at `%APPDATA%`
2. NLU returned confident-but-wrong results, blocking the brain

Fixes:
1. Reorder `parse_transcript`: **brain BEFORE NLU** for admin users
   - Brain (Qwen 0.5B LLM) is much smarter than BERT-Mini
   - BERT-Mini classified "So, you have to list." as `MediaPlayPause` (0.90)
   - Now brain is tried first, NLU is fallback for non-admin users
   - Brain result accepted if confidence ≥ 0.5
2. Fix admin config path — was only checking `%APPDATA%/com.nexus.assistant/admin.json`, now also checks `server/admin/admin_config.json` (dev fallback)

### New files

| File | Lines | Purpose |
|---|---|---|
| `src-tauri/src/admin_config.rs` | 165 | Runtime admin detection + config cache |
| `src-tauri/src/brain_client.rs` | 205 | HTTP client to Qwen brain server |
| `src-tauri/src/brain_monitor.rs` | 638 | Cross-checking, approved/rejected writing, verbal wrong, execution reporting |
| `src-tauri/src/lazy_brain.rs` | 241 | Admin-only sidecar launcher |
| `server/admin/brain_server.py` | (moved) | Qwen 0.5B FastAPI server on port 8765 |
| `server/admin/admin_config.json` | small | Runtime `is_admin` flag |
| `server/admin/requirements-brain.txt` | small | Python deps for brain |

### Modified files

- `src-tauri/Cargo.toml` — `admin-brain` feature
- `src-tauri/src/lib.rs` — module registration, feature gates
- `src-tauri/src/intent_parser.rs` — `brain_classify` + `brain_monitor` calls gated
- `src-tauri/src/orchestrator.rs` — brain monitor wiring, verbal wrong detection, execution reporting
- `src-tauri/src/nlu_client.rs` — `nlu_to_parsed_intent` made public, slot normalization
- `nexus.mjs` — `--features custom-protocol,admin-brain` in build
- `.gitignore` — `server/admin/` ignored

---

## Subsystem 2 — NLU Expansion 7→46 Intents

### Goal

The original BERT-Mini NLU had only 7 intent labels. The GitHub sub-command system (commit `c22f6a3`) added 28 typed GitHub operations, but the NLU model couldn't classify them. Expanded to 46 intents covering the full command space.

### Changes

- `server/nlu/seed_dataset.py` (906 lines) — programmatic dataset generator with balanced examples per intent, STT-error variants, accent variants
- `server/nlu/dataset.json` — expanded from ~7-intent small dataset to 2185 balanced examples across 46 intents
- `server/nlu/train.py` — retraining pipeline with ONNX export, accuracy gate, hot-swap with rollback
- `server/nlu/merge_and_train.py` (375 lines) — merges approved/rejected JSONL into dataset, deduplicates, balances, retrains, hot-swaps only if accuracy not worse than old model
- `server/nlu_server.py` — `/parse`, `/health`, `/reload` endpoints; reload hot-swaps the ONNX model without restart
- `src-tauri/src/nlu_client.rs` — HTTP client to NLU server, `nlu_to_parsed_intent` mapping for all 46 intents

### Dataset cleanup (uncommitted)

- Removed 174 bad `list_prs` examples: closed/merged examples, literal `"owner/repo"`, literal `"myorg/myrepo"`
- Added 333 new open-only `list_prs` variants
- Fixed three `slots: null` records
- Final model accuracy: **97.9%**

### New file

- `server/nlu/expand_list_prs.py` — generates additional `list_prs` open-only phrasings (STT errors, accents, filler words)

### Test results

NLU HTTP tests through `/parse` correctly classified:
- `show me the pr list`
- `latest prs`
- `pee ars`
- `pool requests`
- `give me the latest pr`
- `what prs are open`
- `sho me de prs`
- `list prs in zync`
- `prs please`
- `lay test prs`

---

## Subsystem 3 — Deterministic Parser Performance + Missing Patterns

### Performance: regex caching

All 41 inline `Regex::new()` calls in `intent_parser.rs` replaced with a cached `regex_captures()` helper using `OnceLock` + `Mutex` + `HashMap`. Regexes compiled once and reused.

**Measured improvement:**
- Parser avg: 17011 µs → 95.9 µs (**177× faster**)
- Stress test: 1701 ms → 9.59 ms for 100 commands
- Throughput: 59 → 10426 commands/second
- Match rate: 94/100 → 101/100

### Missing patterns added

- `terminate <app>` → `CloseApp` (added to `CLOSE_VERBS`)
- `hey nexus` → `Greeting` (fixed `strip_leading_filler` eating "hey")
- `stop` (bare) → `MediaStop`
- `list pr files for pr N in repo` → `ListPrFiles`
- `comment on pr N in repo <body>` → `CommentPr`
- `add user as admin to repo` → `AddCollaborator`
- `remove user from repo` → `RemoveCollaborator`
- `create release vN in repo` → `CreateRelease`

### Fuzzy 'list' fallback

Catches STT mishearings like "So, you have to list." Auto-detects repo via `get_active_repo_url()`. Low confidence (0.6) so brain/NLU can override. Excludes branch/release/workflow/collaborator/file/member/run.

### Repo sanitization

After Qwen sometimes returned invalid repository values (`"owner/repo"`, garbage):
- Brain prompt no longer uses `"owner/repo"` as example value
- Brain prompt explicitly requires empty repository when no repository mentioned
- `sanitize_ml_intent()` and `is_valid_repo_name()` added in `orchestrator.rs`
- Invalid placeholders, spaces, or invalid characters rejected
- For account-wide `ListPrs`, empty repository is valid
- For commands requiring a repository, invalid values reduce confidence and allow fallback

---

## Subsystem 4 — PR-List Voice Sidebar

### Goal

A voice-driven PR list sidebar that shows open PRs across all repositories (account-wide) or for a specific repo, with Merge and Analyse buttons per PR.

### New files

| File | Purpose |
|---|---|
| `frontend/pr-list.html` | HTML entry point for PR-list window |
| `frontend/src/pr-list/main.tsx` | React mount point |
| `frontend/src/pr-list/PrListApp.tsx` | PR-list app component |
| `frontend/src/pr-list/pr-list.css` | PR-list styling (mirrors sidebar glass) |
| `frontend/src/pr-list/prListStore.ts` | Zustand store for PR list state |
| `src-tauri/capabilities/pr-list-cap.json` | Tauri capability for PR-list window |

### Backend

- `src-tauri/src/github_cmd.rs`:
  - `execute_list_prs_account_wide()` — gets authenticated user, lists up to 100 repositories (owner/collaborator/org-member affiliations), fetches up to 20 PRs per repo, 10 concurrent requests via `tokio::task::JoinSet`, aggregates and sorts newest first, caps at 100 PRs
  - Specific repo uses `client.pulls(owner, repo_name).list()`
- `src-tauri/src/commands.rs`: `show_pr_list_sidebar` — positions at right edge, vertically centered, 500×1000, captures backdrop, runs 1 FPS live blur loop
- `src-tauri/src/dyn_windows.rs`: `WindowConfig::pr_list_sidebar()` — `transparent: true`, `decorations: false`, `always_on_top: true`, `skip_taskbar: true`

### PR-list behavior

- `ListPrs { repo: "", state: "open" }` → account-wide listing
- `ListPrs { repo: "owner/repo", state: "open" }` → repo-specific listing
- `list_prs` intent is **open-only** by design (closed/merged removed from training)

### PR analysis (structured output)

PR analysis now returns exactly 4 sections:
1. **How It Helps the Existing Codebase** (LLM — references specific files/modules)
2. **Bugs Found** table (LLM — Critical/High/Medium/Low with source file)
3. **Stats** table (deterministic from GitHub API — insertions/deletions/files/commits)
4. **Merge Conflicts** (deterministic from GitHub `mergeable_state` — Yes/No/Unknown)

Stats and merge conflict status computed from GitHub REST API, NOT from LLM. `fetchPRContext` returns `{ context, stats }`. Worker compiles clean (`tsc --noEmit`). 28 vitest tests pass.

### UI fixes (uncommitted)

- PR-list sidebar height matched to 1000px (was 500×800) for visual consistency with response sidebar (600×1000) and architect sidebar (900×1000)
- PR sidebar close button removed — only Ctrl+Space closes
- Backdrop image applied via `--sidebar-backdrop-image` CSS variable, same mechanism as response sidebar

---

## Subsystem 5 — STT In-Process Capture (cpal direct)

### Root cause

Intel SST mic driver delivers audio in brief 5-15s bursts, then goes silent. The silence-recovery thread restarts the cpal stream to catch the next burst, but `getUserMedia` (WebView2) gets a separate audio path that stays silent. Result: wake word detects "nexus" (cpal is working), but STT gets empty transcripts (getUserMedia is silent).

### Fix

Capture audio from the cpal stream directly for STT. The same stream that detected the wake word also captures the command audio. No `getUserMedia`, no baton pass, no `ScriptProcessorNode`, no frontend VAD.

### Architecture

- `wakeword_oww.rs`: `STT_CAPTURE_BUFFER` + RMS-based VAD in `on_audio()`
- Wake word trigger calls `start_stt_capture()` — no baton pass
- `on_audio()` buffers 16kHz chunks, detects speech/silence via RMS
- On silence after speech: spawn transcription thread
- Transcription thread calls Groq or local STT, emits `stt:transcript` event
- Frontend listens for `stt:transcript` event, processes transcript

### VAD parameters

- Speech threshold: RMS > 0.005
- Silence after speech: 30 chunks (~2.4s)
- Max capture: 200 chunks (~16s)
- No-speech timeout: 100 chunks (~8s)

### STT model upgrade

- Upgraded from `tiny.en` (39M params) to `base.en` (74M params)
- `tiny.en` mishears "give me the PR list" as "Google me the PR list"
- `base.en` is ~1.5s on CPU but much more accurate
- Added "PR list", "PRs", "list PRs", "show PRs", "give me", "show me" to STT hotwords

### STT corrections

- "Google me the PR list" → "give me the PR list"
- "So, you have to list" → "show me the PR list"
- "So, we are list" → "show me the PR list"
- "So, meet a PR list" → "show me the PR list"
- "give me the PR this" → "give me the PR list"
- Strip leading "So, " filler

### Other mic fixes

- Silence-recovery skips restart during baton pass (frontend has the mic)
- Added "for" to preposition list in STT repo corrections (was only in/of/from — STT transcribed "for unzinc" not "in unzinc")
- Added "unzinc" and "on zinc" to zync mishearing list
- Kept fuzzy matcher threshold at 2 (frontend handles "unzinc"→"zync")

### Files changed

- `src-tauri/src/wakeword_oww.rs` — `STT_CAPTURE_BUFFER`, RMS VAD, `start_stt_capture()`, transcription thread spawn
- `src-tauri/src/stt.rs` — `transcribe_samples()` public function, `read_groq_api_key`/`read_local_stt_only` generic over `Runtime`
- `server/stt_server.py` — `base.en` model, hotword updates
- `frontend/src/audio/recorder.ts` — listens for `stt:transcript` event instead of capturing via `getUserMedia`

---

## Subsystem 6 — TTS Edge-Primary / Piper-Fallback

### Root cause

The TTS system was silently using local Piper for ALL dynamic responses instead of cloud Edge TTS. The frontend sent a Kokoro voice ID (`"af_sky"`) to Edge TTS, which Microsoft rejects, causing every `speak()` call to fail and fall back to Piper (local, 80 MB RAM).

### Fixes

**1. Frontend voice bug (`ttsPlayer.ts`):**
- Changed default voice from `"af_sky"` (Kokoro) to `"en-US-AvaNeural"` (Edge TTS)
- Updated `CURATED_VOICES` from Kokoro voices to Edge TTS voices
- Updated `previewVoice` to work with Edge TTS voice IDs
- Updated Settings UI: unified voice selector controls `edgeTtsVoice`, removed duplicate Edge TTS section, added engine status display

**2. Network state tracking (new: `src-tauri/src/tts_network.rs`, 260 lines):**
- `check_network_now()` — async check, pings Microsoft endpoint
- `is_network_up()` — cached sync check
- Throttled to 30s between checks
- Tracks network recovery time for Piper unload timer
- `set_network_down()` — called when Edge TTS fails despite network up
- `start_network_monitor()` — background thread, checks every 60s

**3. Smart fallback (`tts.rs::synthesize_with_fallback`):**
- Checks network BEFORE attempting Edge TTS
- If network down: skips Edge TTS entirely, goes straight to Piper (saves ~1-2s)
- If network up: tries Edge TTS, falls back to Piper on error
- Marks network as down if Edge TTS fails unexpectedly

**4. Piper lifecycle management:**
- Piper loads on first fallback (lazy, ~80 MB RAM)
- Piper stays loaded for 10 minutes after network recovers (hysteresis)
- After 10 minutes of stable network, Piper is **unloaded** (~80 MB freed)
- New: `tts_piper::unload_engine()` and `is_engine_loaded()`
- New: `tts::unload_piper_global()` (called by network monitor)
- Global Piper engine reference (`OnceLock`) for monitor access

**5. Diagnostics fix (`diagnostics.rs`):**
- Was hardcoded to "connected: true" — now reports actual state
- Shows: Edge TTS active + Piper standby (network up)
- Shows: Piper fallback active (network down)
- Shows: Piper loaded but will unload after 10 min (recovery)

### Runtime behavior after fix

| State | Engine | RAM | Latency |
|---|---|---|---|
| Network UP | Edge TTS (cloud) | 0 MB | ~200ms |
| Network DOWN | Piper (local) | 80 MB | ~40ms |
| Network recovers | Piper stays 10 min, then unloads | 80→0 MB | — |
| Network drops again | Piper reloads on next fallback | 0→80 MB | — |

### New files

- `src-tauri/src/tts_network.rs` (260 lines) — network state tracking + monitor
- `src-tauri/src/tts_edge.rs` (46 lines) — Edge TTS voice validation tests

### Modified files

- `src-tauri/src/tts.rs` — `synthesize_with_fallback`, Piper lifecycle, global engine reference
- `src-tauri/src/tts_piper.rs` — `unload_engine()`, `is_engine_loaded()`
- `src-tauri/src/diagnostics.rs` — real network state reporting
- `frontend/src/audio/ttsPlayer.ts` — Edge TTS voice IDs, unified selector
- `frontend/src/settings/SettingsApp.tsx` — unified voice selector UI

---

## Subsystem 7 — Orchestrator Routing Fixes

- `longRunningInFlight` flag cleared properly (was stuck, blocking subsequent commands)
- STT filler words stripped before parsing ("so", "um", "uh", "like")
- Brain tried BEFORE NLU for admin users (was after — NLU's confident-but-wrong results blocked brain)
- Verbal "wrong" detected before routing (meta-command, returns immediately)
- Execution success/failure reported to brain monitor
- Repo sanitization: `sanitize_ml_intent()` + `is_valid_repo_name()` reject invalid placeholders

---

## Subsystem 8 — UI Animation Fixes

### Wake orb / loading indicator gap

**Problem:** Visual gap between the wake orb and the loading indicator — orb hid too early (800ms) before the loading indicator appeared.

**Fix (`frontend/src/audio/recorder.ts`):**
- Removed premature 800ms orb hiding
- Orb now hides 600ms after the loading indicator appears
- Added 1500ms fallback hide after local acknowledgement
- Result: smooth orb → loading indicator transition with no gap

### PR sidebar close button

- Removed close button from PR list sidebar
- Only Ctrl+Space closes the sidebar (consistent with response sidebar)

---

## Subsystem 9 — Reference Theme Research (no code change)

### What was done

- Cloned `https://github.com/ramensoftware/windows-11-start-menu-styling-guide.git` to `C:\PROJECTS\ULTRON\reference_startmenu_themes`
- Added `reference_startmenu_themes/` to `.gitignore` (prevents accidental commit)
- Five parallel research subagents analyzed:
  1. LiquidGlass / LiquidGlass2 / Fluid themes
  2. Down Aero / TintedGlass themes
  3. TranslucentStartMenu / Borderless / Oversimplified&Accentuated / OnlySearch themes
  4. Brush type documentation (AcrylicBrush, WindhawkBlur, LinearGradientBrush, ImageBrush, RevealBorderBrush, Mica)
  5. NEXUS's existing glass/blur implementation for comparison

### Key findings

- Reference repo is a **Windhawk Start Menu Styler guide**, not Python or Tauri
- Theme effects use XAML-like style expressions (`WindhawkBlur`, `AcrylicBrush`, `LinearGradientBrush`, `RevealBorderBrush`)
- NEXUS already implements a parallel layering model: transparent window → fake-blur JPEG backdrop → CSS tint → CSS border/sheen → fallback solid
- NEXUS's `blur_sigma=32.0` is hardcoded in 4 places; reference themes use blur 5–30
- NEXUS has no noise/grain, no reveal-hover, no luminosity/saturation control, no light/dark variants
- Plan delivered (Phase 0: theme abstraction, Phase 1: named presets, Phase 2: polish) — **not implemented**, awaiting authorization

### Files touched

- `.gitignore` — added `reference_startmenu_themes/`
- No other files modified (research only)

---

## File Change Inventory

### New files (committed)

| File | Subsystem |
|---|---|
| `src-tauri/src/admin_config.rs` | Brain |
| `src-tauri/src/brain_client.rs` | Brain |
| `src-tauri/src/brain_monitor.rs` | Brain |
| `src-tauri/src/lazy_brain.rs` | Brain |
| `src-tauri/src/tts_network.rs` | TTS |
| `src-tauri/src/tts_edge.rs` | TTS |
| `src-tauri/src/pipeline_bench.rs` | Parser perf |
| `src-tauri/src/tts_bench.rs` | TTS |
| `frontend/pr-list.html` | PR-list |
| `frontend/src/pr-list/main.tsx` | PR-list |
| `frontend/src/pr-list/PrListApp.tsx` | PR-list |
| `frontend/src/pr-list/pr-list.css` | PR-list |
| `frontend/src/pr-list/prListStore.ts` | PR-list |
| `src-tauri/capabilities/pr-list-cap.json` | PR-list |
| `server/nlu/seed_dataset.py` | NLU |
| `server/nlu/merge_and_train.py` | NLU |
| `server/admin/brain_server.py` (moved) | Brain |
| `server/admin/admin_config.json` | Brain |

### New files (uncommitted)

| File | Subsystem |
|---|---|
| `server/nlu/expand_list_prs.py` | NLU dataset cleanup |
| `server/nlu/model/nexus_nlu.onnx.bak` | NLU rollback |
| `server/nlu/model/nexus_nlu.onnx.data.bak` | NLU rollback |
| `server/nlu/model/tokenizer/special_tokens_map.json` | NLU tokenizer |
| `server/nlu/model/tokenizer/vocab.txt` | NLU tokenizer |
| `src-tauri/resources/server/nlu/model/tokenizer/special_tokens_map.json` | NLU tokenizer (prod) |
| `src-tauri/resources/server/nlu/model/tokenizer/vocab.txt` | NLU tokenizer (prod) |

### Significantly modified files

| File | Lines changed | Subsystem |
|---|---|---|
| `server/nlu/dataset.json` | ~15884 | NLU expansion |
| `src-tauri/src/intent_parser.rs` | +422 | Parser perf + patterns |
| `src-tauri/src/orchestrator.rs` | +239 | Brain wiring + routing |
| `src-tauri/src/wakeword_oww.rs` | +223 | STT capture |
| `src-tauri/src/nlu_client.rs` | +341 | NLU 46-intent mapping |
| `frontend/src/audio/recorder.ts` | +213 | STT event + animation |
| `src-tauri/src/commands.rs` | +122 | PR-list sidebar |
| `src-tauri/src/github_cmd.rs` | +175 | PR-list account-wide |
| `server/stt_server.py` | +336 | base.en + hotwords |
| `src-tauri/src/dyn_windows.rs` | +29 | PR-list window config |
| `src-tauri/src/tts.rs` | +49 | Edge primary / Piper fallback |
| `frontend/src/audio/ttsPlayer.ts` | +35 | Edge TTS voices |
| `nexus.mjs` | +19 | admin-brain feature flag |
| `scripts/run.ps1` | +98 | unified console updates |
| `AGENTS.md` | +73 | documentation |

---

## Verification Results

| Check | Result |
|---|---|
| `cargo test --lib` (normal) | 314 passed, 0 failed |
| `cargo test --lib --features admin-brain` | 314 passed, 0 failed |
| `npm run build` (frontend Vite) | OK |
| `cargo build --release` | OK |
| NLU model accuracy | 97.9% |
| NLU `/parse` live tests | 10/10 correct classifications |
| Brain server end-to-end | "analyse pr 254 in zys" → zync (conf=1.0), pronunciation map persisted |
| Edge TTS live test | "en-US-AvaNeural" → 8496 bytes OK; "af_sky" → rejected (bug confirmed) |
| Parser benchmark | 177× faster (17011µs → 95.9µs) |
| Worker `tsc --noEmit` | OK |
| Worker vitest | 28 passed |

### Test count progression

| Commit | Normal tests | Admin-brain tests |
|---|---|---|
| `9b3f983` (NLU expansion) | — | — |
| `efc6416` (admin isolation) | 251 | 251 |
| `894f1f5` (negative training) | 251 | 257 (+6 brain tests) |
| `050ef82` (TTS fix) | 260 | 266 (+6 TTS network tests) |
| `f502910` (parser perf) | 286 | 292 (+15 pipeline_bench, +11 tts_bench) |
| `0122ee7` (brain wiring) | 286 | 292 |
| `04fd346` (brain+parser) | 292 | 292 |
| `d780ff4` (brain+stt) | — | — |
| **Current (uncommitted)** | **314** | **314** |

---

## Known Follow-Ups

1. **Settings sidebar** — user plans a comprehensive settings sidebar with: Google/GitHub/Gemini/Groq auth, volume control (default volume when NEXUS speaks), orb position/size sliders (left-right + top-bottom), wake animation window position. Research phase next.
2. **Theme abstraction** — `blur_sigma=32.0` hardcoded in 4 places (`commands.rs` 662-699, 900-1010; `architect.rs` 953-989, 1095-1096). Reference themes show blur 5–30 range. Plan delivered, awaiting authorization.
3. **Live-blur loop dimensions** — hardcoded `600×1000` / `900×1000` while initial capture uses real `inner_size`. Latent bug if window sizes change.
4. **macOS double-tint** — native vibrancy already tints, then CSS stacks `rgba(20,20,22,0.92)`. May be too dark on macOS. Needs visual verification.
5. **Reference repo cleanup** — `reference_startmenu_themes/` is gitignored but still on disk. Keep as reference until theme work is complete, then delete.

---

## Errors Encountered and Fixed

1. **Wrong Rust import** — initial import used `crate::intent_parser::GitHubCommand`, corrected to `crate::github_cmd::GitHubCommand`
2. **Brain repository hallucinations** — Qwen returned `"owner/repo"` or garbage. Fixed via prompt changes + Rust sanitization (`sanitize_ml_intent`, `is_valid_repo_name`)
3. **Training rollback file lock** — `nexus_nlu.onnx.data` couldn't be restored because NLU server held a mapped file (`WinError 1224`). Fixed by killing NLU server processes before retry.
4. **Windows console encoding** — ONNX export failed: `cp1252 can't encode '\u2705'`. Fixed by setting UTF-8 Python output encoding.
5. **Incorrect ONNX test shape** — test used sequence length 32, model expected 64. Retested with length 64.
6. **Incorrect NLU endpoint** — `/classify` returned 404. Actual endpoint is `POST /parse`. `/health` and `/reload` also exist.
7. **Rust release build lock** — `nexus.exe` was running, build failed: "Access is denied". Fixed by terminating process before build.
8. **Brain never running** — `admin.json` path mismatch + NLU blocking brain. Fixed by checking both paths + brain-before-NLU ordering.
9. **STT empty transcripts** — `getUserMedia` silent on Intel SST. Fixed by capturing from cpal stream directly.
10. **TTS always using Piper** — Kokoro voice ID rejected by Edge TTS. Fixed by using Edge TTS voice IDs.

---

*This document covers all work from 2026-09-05 evening through the current working tree on 2026-09-06. Generated for continuity.*
