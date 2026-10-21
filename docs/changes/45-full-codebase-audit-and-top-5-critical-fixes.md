# Change 45: Full-Codebase Audit & Top-5 Critical Fixes

## Overview
Five-agent read-only audit of the entire stack (Rust / frontend / Worker+Python / build-config / IPC contract), every claim cross-checked by the orchestrator. 6 critical, 5 high, ~20 medium findings. The top 5 were fixed in this change; the rest are documented in `docs/features/62-full-codebase-audit-and-top-5-critical-fixes.md` and queued for cleanup.

## Changes Made

### 1. Orchestrator event wire tags (`src-tauri/src/orchestrator.rs`)
- Added per-variant `#[serde(rename = "conflict_report")]` / `#[serde(rename = "github_result")]`. Enum-wide `lowercase` emitted `"conflictreport"` / `"githubresult"` (frontend cases unreachable); enum-wide `snake_case` would have emitted `"git_hub_result"` — variant renames were the only correct fix.
- Added `test_event_wire_tags_match_frontend_union` pinning all four tag strings.

### 2. NLU OTA pipeline (`src-tauri/src/nlu_update.rs`, `src-tauri/resources/server/`, `nexus.mjs`)
- `POST /reload_model` → `/reload` (was a guaranteed 404, killing hot-swap).
- Synced `server/nlu_server.py` → `resources/server/nlu_server.py` (55 intents, `NEXUS_NLU_MODEL_DIR`, `/reload`) and `server/stt_server.py` → `resources/server/stt_server.py` (Moonshine).
- Added `syncServerScripts()` to `nexus.mjs`, called on every build next to `syncNluModel()` so bundled servers can never drift again.

### 3. Production STT → Moonshine (`nexus.mjs`, `nexus-installer.nsi`, `lazy_stt.rs`)
- `checkFasterWhisper()` → `checkMoonshine()`; pip lines to `moonshine-voice` in `nexus.mjs` and installer NSI (including user-facing MessageBox strings); help text and `lazy_stt.rs` header corrected.

### 4. Root `package.json`
- New root shim delegating `dev`/`build` to `frontend/`, unbreaking `tauri.conf.json` hooks for any `npx tauri` path.

### 5. Ghostwriter abort (`src-tauri/src/ghostwriter.rs`)
- `expect("session checked active")` → `let...else` graceful return under the lock (a concurrent exit could panic → `panic="abort"` killed the app).

### 6. Worker search quota (`server/worker/src/index.ts`)
- Removed both inner `incrementUsage` calls in `handleSearch` (+ dead `userId` local); quota charged once by `handleTranscript`. One mid-flight mistake (`req.task.request` → `req.text`) was caught by `tsc` and restored.

## Verification Results
- **Rust:** `cargo test --lib` — 523 passed, 0 failed (incl. new wire-tag pin test).
- **Worker:** `npm test` — 49/49; `npx tsc --noEmit` — clean.
- **Python sync:** both resources servers `py_compile` clean; 55 bundled INTENTS = 55 `labels.json` entries; `/reload` route present; zero `faster_whisper` in bundled STT.
- **Build scripts:** `node --check nexus.mjs` clean.

## Round 2: all-fixes (same day)

Implemented everything the audit marked queued (see feature doc §"Round 2" for the full list):
Rust — M1/M2/M5/M6 hardening, 21 dead IPC deregistrations (+`stt_status`), 20+ dead fns/fields/variants deleted, 6 dead event families removed, `query_impact` + cascade deleted, `pipeline_bench` test-gated, 2 pre-existing test breaks fixed (integration `transcribe_with_groq` arity, stale whatsapp expectation).
Worker — `models.ts` deleted, dead exports/routes/branch deleted, encryption honesty, `scipy` added / `onnx` dropped, quota 8000 → 1200/user.
Frontend — Rust-TTS gate, dead events/globals/files/exports removed, 4 deps pruned, ~40 dead CSS rules pruned, stale comments fixed.

## Verification Results (final, all targets)
- **Rust `cargo test`:** lib 522 + offline_commands 10 + 7, 0 failed, **0 warnings**.
- **Worker:** `tsc` clean, vitest 49/49.
- **Frontend:** `tsc` clean, vitest 20/20, `vite build` succeeds.

## Round 3: settings window + queue wire (same day)

- **Settings kept:** `main-cap` covers `"settings"`; conf capability list completed; tray "Settings (Full Window)" → `open_settings_window`.
- **Queue wired:** `ensureSessionOpen()` + startup pre-open + drain retry; queued long-running commands now send.

## Ship checklist (not yet done)
- `nexus deploy` (worker quota fix), `nexus build` + `nexus start` (bundled servers + Rust fixes), installer rebuild (NSI changes), `wrangler` R2 `MODELS` binding + `NEXUS_ADMIN_TOKEN` (OTA distribution).
