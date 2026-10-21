# Feature 62: Full-Codebase Audit & Top-5 Critical Fixes

## Method

Five parallel subagents (Rust backend / frontend / Worker+Python server / build-scripts-config / Rust↔frontend IPC contract), read-only, every "unused" claim grep-verified against call-sites. The orchestrator then re-grepped every CRITICAL and HIGH claim independently. Only findings surviving two independent checks are reported as fact.

Confidence legend: **[3]** = orchestrator + 2 agents confirmed · **[2]** = orchestrator + 1 agent.

## Critical findings (broken in production today)

### C1. Orchestrator event wire-tag mismatch — conflict UI + PR refresh unreachable [3]

- **Cause:** `OrchestratorEvent` (`src-tauri/src/orchestrator.rs:87-88`) derives `#[serde(tag = "type")]` with enum-wide `#[serde(rename_all = "lowercase")]`. Lowercase flattens multi-word variants: `ConflictReport` → `"conflictreport"`, `GitHubResult` → `"githubresult"`.
- The frontend union (`frontend/src/net/orchestrator.ts:31-41`) expects `"conflict_report"` / `"github_result"`; both `case` labels (`:339`, `:376`) and the `PrListApp.tsx:74` filter never match.
- **Effect:** merge-conflict panel never renders; live in-window PR-list refresh never matches (masked only by mount-time `get_pending_pr_list`).
- **Fix:** per-variant `#[serde(rename = "conflict_report")]` / `#[serde(rename = "github_result")]`. NOT enum-wide `snake_case` — serde's case algorithm splits every camel hump and would emit `"git_hub_result"`. Single-word tags (`state/loading/ack/result/done/error/confirm`) are byte-identical under both schemes, so the rest of the contract is untouched.
- **Regression pin:** `test_event_wire_tags_match_frontend_union` serializes all four shapes and asserts exact tag strings.

### C2. NLU OTA pipeline dead end-to-end — stale bundled servers [3]

Three independent breaks, one root cause:
- **(a)** `src-tauri/resources/server/nlu_server.py` was a stale 283-line copy (41 intents vs the 55-intent `labels.json`), missing `NEXUS_NLU_MODEL_DIR` resolution → production could `KeyError` on class IDs 41–54 and never read downloaded OTA models.
- **(b)** `nlu_update.rs:214` POSTed `/reload_model`; both servers only expose `/reload` → hot-swap always 404.
- **(c)** R2 `MODELS` binding commented out in `wrangler.toml` → `/models/nlu/*` returns 503 (client treats as "no update", silent).
- **Root cause:** `nexus.mjs syncNluModel()` copied model files but never the `.py` entrypoints.
- **Fix:** `/reload_model` → `/reload`; synced both entrypoints into `resources/` (verified: 55 intents = 55 labels, `/reload` route present); added `syncServerScripts()` to `nexus.mjs`, called on every build next to `syncNluModel()`. R2 binding left for the deploy step (requires `wrangler secret put NEXUS_ADMIN_TOKEN` + redeploy).

### C3. Production STT still faster-whisper — Moonshine never shipped [3]

`resources/server/stt_server.py` still imported `faster_whisper`; dev copy is Moonshine; installer + `nexus setup` pip-installed `faster-whisper`.
- **Fix:** synced dev script into `resources/` (verified: moonshine present, faster-whisper absent); `checkFasterWhisper()` → `checkMoonshine()` in `nexus.mjs`; pip lines swapped to `moonshine-voice` in `nexus.mjs` + `nexus-installer.nsi` (including user-facing MessageBox strings); help text + `lazy_stt.rs` header corrected (edge-tts+Piper, not Fish Audio).

### C4. Frontend VAD capture chain + command queue dead-on-arrival [2]

`startVad`/`resumeVad`/`setSpeechStartCallback` have zero importers; `main.tsx` imports only the two preloads + `stopVad`. So `preloadMicVad` acquires and holds the mic at startup for a VAD that never processes audio (~1,500 unreachable lines). The long-running-command queue can never succeed: `openSession()` is called only from `captureUntilSilence()`, which has zero callers → `sessionOpen` stays false → every queued `sendTranscript` throws `"no backend session"` (caught, warn-logged).
- **Status:** documented, not yet removed (cleanup phase). Live path is Rust `stt:transcript` → `processTranscript`.

### C5. Root `package.json` missing — Tauri hooks hard-fail [3]

`tauri.conf.json` `beforeDevCommand`/`beforeBuildCommand` run `npm run dev/build` at repo root; no root `package.json` existed. Masked because `nexus.mjs`/`build.ps1` bypass the hooks.
- **Fix:** root `package.json` delegating both scripts to `frontend/`.

### C6. `settings` window unreachable + capability-orphaned [3]

`open_settings_window` has zero callers; live UIs use `settings-sidebar`. No capability file lists `"settings"` → per Tauri schema that window would have zero IPC access even if opened. `hotkey.rs` branches on it (always false). Ships dead in every bundle.
- **Status:** documented, not yet removed (cleanup phase).

## High findings

### H1. Worker search quota double-count [2]

`handleSearch` incremented `{requests, ai_neurons, search_calls}` at both inner sites (`index.ts:1728, 1790`) AND `handleTranscript` incremented again after it returns (`:2757`, its only caller). `search_calls_per_day: 50` died after ~25 real searches.
- **Fix:** deleted both inner increments + now-unused `userId` local; the outer single charge (`ai_neurons: 100, search_calls: 1` for searches) remains. Cache-hit path (early return, no inner charge) behaves identically.

### H2. Ghostwriter check-then-act race → process abort [2]

`handle_turn` locked `SESSION`, checked active, dropped the lock, then `expect("session checked active")` after re-locking. A concurrent `exit()` (or `match_command` mutating the session) in the window panics → `panic = "abort"` kills NEXUS.
- **Fix:** `let Some(s) = guard.as_mut() else { return "Ghostwriter isn't open, sir." }` — re-check under the lock, graceful return, same user-facing message as the inactive path.

### H3. Live Mode (~2,500 LOC, 14 commands) compiled but unreachable [3]

Registered at `lib.rs:949-962`; zero frontend invokes; orchestrator never routes to `crate::live`. AGENTS.md documents it as shipped.
- **Status:** wire the routing hook or delete (cleanup phase).

### H4. Tier-3 parameter capture ignores Rust TTS state [2]

`main.tsx:284-297` gates on `speechSynthesis.speaking`, always false under primary Rust/Kokoro TTS → 3s capture starts while the prompt still plays. `recorder.ts` already does this correctly via `isRustTtsPlaying()`.
- **Status:** one-line fix queued for cleanup phase.

### H5. `NEXUS_ENCRYPTION_KEY` declared, never read [2]

API keys stored as reversible `btoa()` (`index.ts:2569`); schema comment falsely claims encryption.
- **Status:** remove the secret from Env/comments or implement AES-GCM (cleanup phase).

## Medium findings (verified, queued)

- **38 registered-but-never-invoked Tauri commands** (incl. all 14 `live_*`; `pause_wakeword` doc falsely claims "Called by the frontend"). Zero orphan invokes the other way — all 60 frontend calls resolve, all arg keys match.
- **6 Rust events nobody hears:** `architect:phase1-ready`, `architect:loading`, `wake-engine-status`, `meeting:paused/resumed`, `tts:audio-started` (+ `tts:audio-started` vs `tts-started` naming split; `sidebar:hide` dead both ends).
- **Entire `models.ts` dead** — `index.ts` imports it then shadows every symbol with local consts; edits to `models.ts` silently do nothing.
- **Dead Worker surface:** `impact_narration` branch, `/api/register`, `/config/check`, `/api/transcribe`, `clean.ts` ×4 exports (`stripInjection` imported but never called — injection-guard comment is false), `cacheDelete`, `retrieve`, `getCredentialsFromD1`.
- **`speak_text` `speed` accepted then discarded** (`tts.rs:157`) — speech-rate setting is a no-op on primary TTS.
- **`admin-brain` on 1 of 6 build paths** (only `nexus.mjs`) — installer/CI binaries silently lack the brain.
- **Feature orphans:** `wakeword-sherpa` cannot compile (`sherpa_onnx` missing from deps); CI `mock-wake` job omits `--no-default-features`, testing nothing; `--no-default-features` build breaks on ungated `wakeword_oww` consumers.
- **Quota/doc contradictions:** AGENTS.md limits (500/3000/10/100) vs actual (150/8000/15/50); false `NLU_PORT` override; one-sided `NEXUS_STT_PORT` (setting it breaks transcription).
- **Broken OTA pieces fixed above** (C2); R2 binding + missing `scipy` (breaks `calibrate_temperature.py`) remain.
- **Untracked-but-referenced files** (docs 44/61, 5 wake scripts incl. one invoked by `nexus.mjs:900`, gitignored `cdp_monitor.js` silently disabling `run.ps1 -Debug`); tracked-but-ignored `.onnx.data` files required by the bundle.
- **Dead code volume:** ~20 dead Rust fns behind 44 `#[allow(dead_code)]` sites, dead files (`tts_kokoro_backup.rs`, `LoadingAnimation.tsx`, `clickThrough.ts`, `pcm-worklet.js`), 4 unused npm Tauri plugins, 44 frontend `as any` (worst: `orchestrator.ts:183`), ~15 manual-only NLU scripts, legacy sidecar/n8n stack still CI-validated.

## Low / verified clean

~60 dead CSS classes, 21 unused CSS vars, stale faster-whisper comments, duplicates (`isTauri()` ×5, 3 `tauriInvoke` wrappers, triplicated recorder flow), 7 dead scripts, zero `TODO/FIXME` in `src-tauri/src` and `server/**`.
**Clean, do not touch:** no orphan invokes, `tsc --noEmit` strict-clean (frontend), 8/8 vite inputs ↔ HTML, no committed secrets, ports 39217/39218/39219/5173/8765/8766 consistent, `custom-protocol` on every release path, no lock-across-await, all untrusted-text slicing guarded, NLU model paths sound.

---

## Round 2: all-fixes implementation (2026-09-24, same day)

Everything above marked "queued" or "cleanup phase" was implemented, except items explicitly deferred at the end. Full `cargo test` (ALL targets, not just `--lib`), worker `tsc`+vitest, frontend `tsc`+vitest+`vite build` all green, zero warnings.

### Rust fixes

- **Silent-error logging (M1/M2):** deep-link register failure now warns (was `let _`); all three `show_sidebar_with_confirmation` sites log failures with the Confirm-without-UI hazard noted (emit order unchanged — the PENDING_SIDEBAR race-free pattern depends on it).
- **Connect-card reservation (M5):** `open_mcp_connect_card` keeps the atomic insert as a reservation but RELEASES it on HTTP-client or sidebar failure, so a failed open no longer burns the session's one chance.
- **MCP retry overwrite (M6):** `stash_mcp_retry` warns with both server/tool names when the single slot drops a pending call.
- **20 dead IPC commands deregistered** (`orchestrator_status/show_loading/hide_loading/github_clear_token`, `mcp_connect_state`, `save_server_config`, `is_nexus_paused/meeting_status/set_meeting_detection`, `open/close_settings_window`, `is_autostart_enabled`, `refresh_app_registry`, `mic_self_test`, `nexus_diagnostics`, `get_active_repo_url`, `cancel_architect_analysis`, `query_impact`, `show_sidebar_with_confirmation`, `show_pr_list_sidebar`, `stt_status`). Functions stay for tests/internal callers. Kept deliberately: `pause_wakeword` (external/CDP use, doc corrected), `enrich_phase1` + `show_sidebar` (intentional), `analyze_repo_fast` (`scripts/test_fast_analysis.js` invokes it via CDP — verified), `live_*` (pending decision).
- **Dead fns deleted:** `State::done`, `spawn_prewarm` (wiring it would violate the documented lazy-NLU RAM decision), `is_nlu_available`, `ensure_window`, `refresh_overlay`, `blur_bgra_to_jpeg`, both `send_native_notification` arms, `extract_repo_from_url`, `get_valid_google_token`, `is_noise`, `is_stt_capturing`, `AiExplanation` variant, `process_name` field, `hidden_title` field + 8 constructors, `fetched_at` field + prod/test constructors, `cancel_architect_analysis` + `cancel()`, `nexus_diagnostics`, `stt_status`, `query_impact` + `ImpactResult` + its test, `handleRegister`, `getCredentialsFromD1`. Kept after verification: `AudioPreprocessor::new` (audit wrong — unit tests call it), `verify_transcript` (tests pin threshold-1.0; comment corrected), `floor` (tests), all `CachedGraphState` fields (every one read by live deep analysis), `JsonRpcResponse.id` (protocol completeness), `has_pending_compound`/`is_long_running`/`set_paused` (test-covered).
- **Dead emits deleted (6):** `architect:loading` ×5, `architect:phase1-ready`, `wake-engine-status` ×3, `meeting:paused/resumed`, `tts:audio-started` ×3. Progress now lives in `tracing` logs; each site carries a re-add note.
- **`query_impact` cascade:** deleting the command orphaned its graph-field readers; removed `graph/node_indices/index_to_file` from `CachedGraphState` + prod constructor (live phase-2 still builds/uses the local petgraph for hotspots/SCC/counts — untouched). `CACHED_GRAPH` stays live for `phase2_response` re-queries.
- **`pipeline_bench` gated `#[cfg(test)]`** (was compiled into every release); `pause_wakeword` doc corrected; `diagnostics.rs` module docs + stale faster-whisper wording fixed.
- **Pre-existing breaks fixed:** `tests/phase2_integration.rs` never updated for the 4-arg `transcribe_with_groq` (added `None`); `offline_commands.rs` whatsapp test expected pre-MCP `WhatsappChat` — updated to the intended `NeedMoreInfo` partial-send contract (handled end-to-end by orchestrator + command_executor, pinned by in-module tests).

### Worker fixes

- **Deleted `models.ts`** (dual source of truth) + its import; recovered live `FLASH_CONTEXT_LIMIT_CHARS = 520000` as a local const (the audit missed this one live symbol — `tsc` caught it).
- **Dead exports deleted:** `clean.ts` ×4, `cacheDelete`, `retrieve`, `getCredentialsFromD1`; imports trimmed (`getUsage/UsageRow`, `SearchResult`, `retrieve`).
- **Dead routes/branch deleted:** `/api/register` + `handleRegister`, `/config/check`, `/api/transcribe`, `impact_narration` branch + `handleImpactNarration` (consistent with the Rust `query_impact` deletion — both halves of the dead feature are gone).
- **Encryption honesty (H2):** `NEXUS_ENCRYPTION_KEY` removed from both `Env` interfaces + deploy header; `encrypted` var renamed `obfuscated` with an honest comment; `schema.sql` comment corrected (column name kept — renaming breaks deployed D1).
- **Deps:** dropped unused `onnx`, added missing `scipy` in `requirements-train.txt`.
- **Quota tune:** `ai_neurons_per_day` 8000 → 1200 (6 × 1200 = 7200 < 9500 global hard stop); both plan-contradiction comments fixed; AGENTS.md quota line corrected (150/1200/15/50).

### Frontend fixes

- **TTS gate (H2):** `main.tsx` parameter capture now checks `isRustTtsPlaying()` first, `speechSynthesis.speaking` second.
- **Dead IPC removed (H3/H4):** `emitSidebarHide`, `sidebar:hide` listener, `__NEXUS_SET_SIDEBAR_CONTENT__` / `__NEXUS_HIDE_SIDEBAR__`, `__NEXUS_CANCEL__`.
- **Dead files deleted:** `LoadingAnimation.tsx`, `overlay/clickThrough.ts`, `audio/pcm-worklet.js` (zero importers each, re-verified).
- **Dead exports deleted:** `hasSession`, `hasDialogContext`, `consumeDialogContext` privatized (it IS used internally — the "recorder calls it" comment was false), oauth `getSidecarBaseUrl/addApiKey/removeApiKey/listApiKeys/openInBrowser`, `sttStatus`, `ttsAvailable`, `__resetTtsActivityForTest`, both panel default exports. `cancelSession/closeSession` + `send_transcript/open_session` KEPT (session subsystem tied to the pending VAD decision).
- **Deps pruned:** 4 unused Tauri plugins removed from `package.json` (+ lock sync).
- **Stale comments fixed:** `ttsPlayer` event name, `stt.ts` Moonshine wording (×2).
- **CSS prune (L1):** ~40 dead rules deleted from `sidebar.css` (brand/tag/success/close/font-size/query/h1-h6/charts/databases/features/activity blocks; combined `.sidebar-action-btn` selectors untouched) + `status-badge--error` from `settings-sidebar.css`. Verified: dead selectors absent, all live selectors (`minimal-list*`, `analysis-architecture`, `analysis-no-data`, `sidebar-response`…) intact, `vite build` succeeds.

### Deferred (documented, needs a decision or a safety net)

- **Live Mode / VAD+capture+session chain:** delete vs wire — product decision, asked separately.
- **`setup.css` / `settings.css` dead classes + 21 unused CSS vars + `as any` escapes + `isTauri`/`tauriInvoke`/snippet duplicates:** deferred — no visual/type safety net for restyling churn; zero functional impact.

### Round 3: settings window kept + queue wired (2026-09-24)

- **Settings window fixed and kept:** `"settings"` added to `main-cap` windows (its permissions cover everything `SettingsApp` invokes); `tauri.conf.json` capability list completed with the two auto-discovered-but-unlisted identifiers (`settings-sidebar-cap`, `global-shortcut-cap`); tray gained a "Settings (Full Window)" item wired to the previously caller-less `open_settings_window` (Ctrl+Space destroy path already handled it). Verified: `cargo test` 522+10+7, 0 warnings.
- **Long-running queue wired:** root cause of the dead queue was a single missing link — `sessionOpen` is set only by `openSession()`, whose only caller was the dead `captureUntilSilence`. The dedup/ack/result-callback machinery was already live in the transcript flows. Fix: new `ensureSessionOpen()` in `wsBridge.ts` (open-if-closed, warn-and-false instead of throw), fire-and-forget call at startup in `main.tsx` (session open is config-only, no network, never blocks boot), and one-shot reopen-and-retry in `processNextQueuedCommand` (barge-in closes the session mid-queue). Queued commands now send instead of dying with "no backend session".
- **Live Mode:** briefing delivered, no action taken (pending plan decision).
- **R2 `MODELS` binding, `temperature_calibration.json` manifest gap, untracked-but-referenced scripts, legacy sidecar stack, 7 dead `scripts/*.py`, root one-off helpers:** ops/cleanup items, no runtime effect.
- **One-sided `NEXUS_STT_PORT` / false `NLU_PORT` docs:** documented in the audit; not yet fixed.

### Verification (final)

- **Rust `cargo test` (all targets):** lib 522 + offline_commands 10 + 7, 0 failed, **0 warnings** (fresh `cargo check` confirm; two stale-incremental phantom warnings investigated and dismissed with evidence).
- **Worker:** `tsc` clean, vitest 49/49.
- **Frontend:** `tsc` clean, vitest 20/20, `vite build` succeeds (validates pruned CSS).
- **Audit self-corrections during implementation:** `AudioPreprocessor::new` (kept — tests call it), `FLASH_CONTEXT_LIMIT_CHARS` (recovered as local const), `analyze_repo_fast` (kept registered — CDP script depends on it), `CachedGraphState` fields (all live), one mid-flight `req.task.request` → `req.text` slip caught by `tsc` and restored.
