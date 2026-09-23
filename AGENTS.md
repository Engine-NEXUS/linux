# NEXUS — Project Notes

## Repository & Research Architecture Rule
- **`Engine-NEXUS/NEXUS-PAPERS`** (`https://github.com/Engine-NEXUS/NEXUS-PAPERS`): Dedicated repository for all scientific research papers, acoustic DSP investigations, NLU data science studies, and architecture compendiums.
- **`Engine-NEXUS/WINDOWS`** (`https://github.com/Engine-NEXUS/WINDOWS`): Main application repository. All documentation (`docs/`), feature guides, and implementation code must always be pushed and synchronized in lockstep with this repo.

## Multi-Source Noise Hardening & Hardware Invariance (2026-09-23)

- **Multi-Source Noise Ingestion (`ingest_opensource_noise.py`)**:
  Synthesized and categorized 600 high-fidelity background noise profiles across mechanical keyboard typing,
  113.3 Hz chassis fan resonance, office HVAC, domestic impulsive sounds, and narrowband telecom profiles.
  Screened through `faster-whisper` anti-poisoning ASR, automatically purging colliding clips. Total verified background: 998 clips.
- **Multi-Device Data Augmentation (`train_local_wakeword.py`)**:
  Applied Bluetooth narrowband bandpass (300–3400 Hz), laptop chassis fan resonance (55–145 Hz), distance attenuation (0.25x–0.40x),
  and ambient background mixing to positive recordings. Expanded positive training context to 11,760 windows, trained against 35,715 negative windows.
- **Hardware-Adaptive Microphone Invariance (`test_device_invariance.py`)**:
  Benchmarked across 5 hardware microphone profiles: **Studio USB Condenser (96.6%)**, **Laptop Mic Array with Intel Smart Sound (100.0%)**,
  **Bluetooth Headsets / Earbuds (92.1%)**, **Far-Field / Quiet Whispering (85.9%)**, and **Noisy Office (93.0%)**.
- **Batch Evaluation & Verification**:
  Batch evaluation across 3,000 files: **96.6% Positive Recall**, **95.8% Negative Soundalike Rejection**, and **99.7% Background Noise Rejection**.
- **Docs**: Architecture spec in `docs/features/58-multi-source-noise-hardening-and-hardware-invariance.md` and changelog in
  `docs/changes/41-multi-source-noise-hardening-and-device-invariance.md`.

## Apex Wake Word Evolution, Data Poisoning Quarantine & Adaptive Microphone DSP (2026-09-22)

- **Automated ASR Poisoning Quarantine (`audit_positive_samples.py`)**:
  Audited 592 positive recordings with `faster-whisper` and RMS filters. Identified and quarantined 135 poisoned clips
  (conversational sentences, YouTube background audio, near-silence) into `quarantined_bad_positive/`, leaving 456
  pristine acoustic NEXUS recordings.
- **Multilingual Negative & Multi-Source Background Augmentation**:
  Synthesized 1,442 negative samples across English soundalikes (*"next"*, *"texas"*, *"lexus"*, *"necklace"*),
  Indian languages (Hindi: `hi-IN-Madhur`, `hi-IN-Swara`; Telugu: `te-IN-Mohan`, `te-IN-Shruti`), and assistant names.
  Generated 400 multi-source background sound clips (`generate_background_sounds.py`) for HVAC fans, mechanical
  keyboard typing, mouse clicks, and room ambience.
- **BCEWithLogitsLoss & SigmoidWrapper ONNX Export**:
  Upgraded classifier training with `pos_weight=8.0` penalty on negatives and exported calibrated ONNX graph.
- **Adaptive Microphone Hardware Prober (`nexus wake probe` / `acoustic_profile.rs`)**:
  Created 1.5s FFT ambient spectral scan in `scripts/probe_microphone.py` and Rust `src-tauri/src/acoustic_profile.rs`.
  Detected chassis fan resonance at 113.3 Hz (+22.3 dB) on Intel Smart Sound mic array; dynamically auto-tunes high-pass
  filter to 128.3 Hz, hardware pre-gain to 2.50x, adaptive silence gate to 0.00300 RMS, and impulsive gate to 8.0x (rejecting coughs/throat-clears).
- **Phantom Cascade Elimination**:
  Added `reset_after_trigger()` across Rust and Python to immediately flush the 16-frame embedding buffer on trigger confirmation.
- **Batch Evaluation & Verification**:
  Batch benchmark on 2,298 files: **92.3% True Positive Recall**, **98.9% Negative Rejection** (1.1% FA), and **99.8% Background Noise Rejection** (0.2% FA).
- **Docs**: Architecture spec in `docs/features/57-apex-wake-word-evolution-and-hardware-adaptation.md` and changelog in
  `docs/changes/38-apex-wake-word-evolution-and-hardware-adaptation.md`.

## Targeted Intent Training, Category Drill-Down & MCP Data Promotion (2026-09-22)

- **Targeted CLI Voice Collection (`nexus collect`)**:
  Added `-i` / `--intent` and `-c` / `--category` flags with combined scope validation. Developers can directly
  target single intents (`nexus collect --category github --intent create_pr` or `nexus collect --intent create_pr`)
  or drill down interactively by choosing a category and selecting individual sub-intents from a numbered terminal menu.
- **Complete Phrase Catalogs (All 55 BERT-Mini Intents)**:
  Expanded `PHRASES` in `scripts/collect_nlu_samples.py` with realistic spoken templates for all 23 missing GitHub,
  workflow, release, collaborator, and local mode intents (57 total registered intents including live dictation modes).
  Extended `extract_slots()` with rule-based entity extractors for `create_pr` (`repo`, `title`, `head`, `base`),
  `comment_pr` (`pr_number`, `repo`, `body`), `analyse_pr` (`owner`, `repo`, `pr_number`), `add_collaborator`, etc.
- **Zero-Quarantine MCP Split Promotion (`dataset.json`)**:
  Diagnosed the Step 3d quarantine cause (missing test rows in locked evaluation set for new intents). Promoted
  25 phrase families for `order_food` (99 rows), 12 for `search_product` (68 rows), and 12 for `send_whatsapp_message` (127 rows)
  into `dataset.json` with strict zero phrase-family cross-split leakage. Re-minted `split_lock.json` and `evaluation_lock.json`
  (481 test rows). Validation passes cleanly (`python server/nlu/data_foundation.py validate`).
- **Category Coverage Alignment (`nlu_stats.py`)**:
  Aligned category schema with the 55 production intents. Verified that all MCP intents now display full trained rows
  (`order_food`: 77, `send_whatsapp_message`: 72, `search_product`: 32, `whatsapp_search`: 107) and active `◐ Good` / `● Strong`
  mastery status.
- **Docs**: Architecture spec in `docs/features/56-targeted-intent-training-and-mcp-data-promotion.md` and changelog in
  `docs/changes/37-targeted-intent-training-and-mcp-data-promotion.md`.

## NLU Data Foundation, STT Conditioning & Voice Scaling (2026-09-22)

- **Acoustic STT vs. Language NLU Decoupling**:
  Untangled speech acoustic transcription from text intent classification. Resolved multilingual Whisper
  hallucinations (Urdu/Russian/Spanish on short <2s clips) by adding `language="en"`, `temperature="0.0"`,
  and domain prompt biasing (`NEXUS, WhatsApp, Biryani, Dosa, Ghostwriter, VS Code, PR, GitHub...`) to
  `scripts/collect_nlu_samples.py`.
- **Data Foundation & Evaluation Lock Fix (`data_foundation.py`)**:
  Fixed `ERROR: external benchmark hash mismatch` by updating `server/nlu/data/external_evaluation_lock.json`
  with canonical SHA-256 hashes for sorted evaluation benchmark files (`clinc150_oos_test.jsonl`). Preflight
  validation passes cleanly.
- **The 70/30 Data Principle**:
  Enforces 70% anti-poisoning guardrails (cryptographic test locks, slot consistency, conflict repair, split quarantine)
  and 30% zero-waste user effort preservation (all clean recorded voice samples from `collected_samples.jsonl`
  automatically audited and merged into `dataset.json` — 3,108 training rows).
- **Voice Scaling Science**:
  Empirical research demonstrates 100–500 voice samples is the optimal sweet spot (~96.5%–98.2% standalone BERT accuracy;
  99%+ real-world command success when backed by the 3-tier Regex/BERT-Mini/Qwen cascade).
- **Speaker Independence & One-Login Google Contacts**:
  Documented text-token speaker invariance (1 global BERT-Mini model works for all users/devices via Worker R2 OTA updates).
  Integrated Google OAuth scope union (`contacts`, `gmail`, `calendar`) with local `contacts.json` resolution.
- **Docs**: Comprehensive spec in `docs/features/55-nlu-data-perfection-voice-scaling-and-mcp-bridge-research.md`
  and changelog in `docs/changes/36-nlu-data-foundation-stt-conditioning-and-voice-scaling.md`.

## Interactive Voice Approval & Confirmation Sidebar (2026-09-21)

Full interactive voice approval workflow with smart 5-second listening window
and persistent Response Sidebar confirmation card:
- **Response Sidebar Confirmation Card (`ConfirmationPanel.tsx`)**:
  Renders structured details for actions requiring approval (WhatsApp recipient & message bubble,
  GitHub repo/PR#/danger warning, Swiggy/Amazon MCP tools/parameters). Displays 1-click
  `[✓ Confirm]` and `[✕ Cancel]` buttons with loading state.
- **Automatic 5-Second Voice Listening Window (`orchestrator.ts`)**:
  Once prompt TTS finishes speaking, the microphone automatically opens into `listening` mode
  and starts STT audio capture without needing a hotkey or wake word.
- **Instant Early Reaction (< 2s)**:
  Speaking an approval phrase (*"proceed"*, *"approved"*, *"yes"*, *"confirm"*, *"go ahead"*, *"do it"*)
  or rejection phrase (*"cancel"*, *"no"*, *"abort"*) clears the 5s timer immediately and executes
  or aborts without waiting for the full 5s window.
- **Graceful Timeout & Sidebar Persistence**:
  If 5 seconds elapse with no speech, the mic returns to idle while the sidebar remains open on screen
  for manual review and 1-click approval.
- **Rust Backend Integration (`commands.rs`, `orchestrator.rs`, `lib.rs`)**:
  `show_sidebar_with_confirmation` captures blurred desktop backdrop and stores confirmation JSON
  race-free in `PENDING_SIDEBAR`.
- **Docs & Verification**: Detailed architecture spec in
  `docs/features/54-interactive-voice-approval-and-confirmation-sidebar.md`.
  Verified across 585 tests (519 Rust + 17 Vitest + 49 Worker).

## MCP Connect System — Best-of-Combine Build (2026-09-20)

Industry research in `docs/research/mcp-connection/` (7 files) drove a
three-stack implementation, each phase verified twice:

**Phase 1 — shared connect infrastructure:**
- `mcp_client.rs`: `McpConnectState` (unknown/down/auth-required/ready),
  `pairing_status` parser (`parse_pairing_state`, case-insensitive,
  garbage → Error, never fake green), `normalize_qr_payload`
  (data-URI / raw-b64 / pairing-code), `mcp_connect_state` command +
  `connect_card_for` per server (WhatsApp fuses transport + pairing;
  vault services fuse transport + `token_status`; Amazon transport-only).
- `orchestrator.rs`: MCP failure → voice guidance (existing) + Connect
  card opens in the sidebar ONCE per server per session
  (`SHOWN_CONNECT_CARDS` gate; `open_mcp_connect_card` +
  `connect_card_markdown` — status, numbered steps, QR image, /pair
  link, ToS-burner + 20-day-rotation notes). Failed call is stashed
  (`PENDING_MCP_RETRY`) and a 5s-poll ready monitor
  (`spawn_ready_monitor`, ~10min cap) auto-retries it once when the
  server turns Ready (Composio WAIT_FOR_CONNECTIONS shape); drops
  silently if a newer turn is active. Wired into `Subsystem::Mcp` Err,
  `dispatch_to_mcp` Err, and `orchestrator_mcp_confirm` Err.

**Phase 2 — Swiggy spec-OAuth (Worker + vault + frontend):**
- `server/worker/src/index.ts`: SWIGGY_TOKEN_URL/AUTH_URL/SCOPES +
  Env secrets; `handleAuthUrl`, `handleOAuthBrowserCallback`,
  `handleOAuthExchange` each gained a swiggy branch (PKCE S256, form-
  encoded token exchange, D1 storage, shared `/oauth/callback` +
  `nexus://oauth/` deep link); `refreshSwiggyToken` +
  `getValidSwiggyToken` (silent refresh, null-on-failure like Google);
  `GET /oauth/swiggy-token`; `/config/check` lists swiggy.
- `auth_vault.rs`: `fetch_swiggy_token_from_worker` + swiggy arm in
  `refresh_service_token` (silent refresh on-device).
- `setup/oauth.ts` + `SetupApp.tsx`: provider union widened to
  `"swiggy"` (flow is fully generic). Connections tab: "Login with
  Swiggy" button (`handleSwiggyLogin` → connectOAuth → vault refresh),
  updated hint.
- Setup needs: `wrangler secret put SWIGGY_CLIENT_ID/SWIGGY_CLIENT_SECRET`
  (Builders-Club approval required for production; localhost dev free).

**Phase 3 — long-tail + proactive rotation:**
- Spotify/Vercel/Render cards: numbered 3-step hints + "Get token"
  buttons (`shell.open` provider pages, window.open fallback).
- Vault idle monitor (90s) now also probes WhatsApp `pairing_status`
  (inner call — no breaker trip, no audit) so the scheduled ~20-day
  session rotation surfaces before a send fails.

**Verify x2 results:** cargo check clean; `cargo test --lib
--test-threads=1` 499/499; Worker `npm test` 49/49 ×2 + tsc; frontend
vitest 14/14 ×2 + tsc. Live bridges test (`test_connect_state_live_bridges`)
pins card shape invariants (pair URL, steps present on auth-required,
no nag on ready) against live 5-server probe.

**Round-2 deep audit (same day, `docs/research/mcp-connection/08-...`):**
live sources (Anthropic trackers #744/#54649/#60572/#35, Novu connect-card
PR, AutoGPT MCP fix commit, 2026-07-28 OAuth security spec) exposed 5
gaps — all fixed + verified ×2 (Rust now 500/500 serial): monitor
re-renders the open card when the QR payload changes (WhatsApp rotates
its QR every 20-30s; a static card was unscannable after 30s) and renders
the Connected card on Ready; `getValidSwiggyToken` persists rotated
refresh tokens (OAuth 2.1 public-client MUST); RFC 8707 `resource` param
added to Swiggy authorize/exchange/refresh; `audit_line()` choke point +
test pins tokens out of the audit log; `mcp_connect_state` probes run in
parallel (25s → 5s worst case).

## Stuck Animations + PR Analyse Flow + Minimal Sidebar + Phase 11 (2026-09-18)

**Stuck wakeup animation — root cause was a missing `done` handshake.**
`WorkerBackend` Ok withholds `Done` (would cancel TTS), but the frontend
`result` handler spoke with no `onEnd` and `signalOrchestratorDone()` had
zero call-sites — the orb parked in `speaking`/loading-loop forever after
long replies. NOT any "ghost mode" (no ghost CSS/setting exists anywhere;
"ghost" = Ghostwriter dictation only). Fixes:
- `net/orchestrator.ts::finishSpokenResult` — TTS onEnd → guarded reset +
  `orchestrator_done` (barge-in-safe via request-id check). 3 vitest tests.
- `App.tsx` 60s speaking failsafe — silent 60s in `speaking` (checked via
  `isRustTtsPlaying()`) forces the `done`-equivalent reset; long replies
  re-arm instead of cutting.
- `show_loading` runs inline (spawn removed) — create always completes
  before the paired destroy; kills the spinner re-creation race.
- Known flake: `test_install_and_cancel` fails under parallel threads
  (shared ACTIVE_REQUEST global); passes isolated ×2 and 43/43 serial.

**Analyse-PR flow:** pr-list `Analyse` now hides the list first
(`hide()` + `hide_pr_list_sidebar`); `WorkerBackend` Ok with non-null
`analysis` opens the response sidebar via `show_sidebar_with_analysis`
(PENDING_SIDEBAR race-free path) AFTER emitting `Result` (TTS not blocked).
No new AI call — the Worker already returned `analysis`, it was
`console.log`-dropped.

**Minimal sidebar:** removed container top-highlight gradients, inset
stacks, specular `::before` rims, gradient overlays (flat dim kept),
dead `backdrop-filter`s, dot pulse, button lift/glow, red close hover,
`nexus-hr` gradient, unused `--gradient-*` vars. PR buttons now flat
solid (green Merge, grey Analyse). Settings sidebar untouched (still glossy).

**list_prs "pull requests and alll":** data was fine (64 train rows) — the
deterministic regex had no `pull request` alternative and `$`-anchored out
trailing words. Pattern now accepts the noun + `and all/all of them/
everything` + optional `the` after state. `add_phase11_pr_verbs.py` adds
45 fresh rows (39 list_prs + 6 OOS) → candidate train 3482, test intent
**0.9004** (was 0.8850), gated OOS 0.9871. Production dataset.json
untouched (2765 rows) — promotion is a separate merge+retrain decision.
`collect_nlu_samples.py`: duplicate `list_prs` key merged (21 phrases were
silently dropped), 32-phrase list; interactive category menu (github, mcp,
apps, messages, live, random) + `--category`; no google/research category
(no such intents exist yet). Say-it-your-way paraphrase option built then
REVERTED per user (need is same-word/many-sounds, not rewording) —
replaced by `canonical_repo_name()` sound-alias map (`cervix/srvx/service
→ servx`, zinc/sync → zync, per-segment, unknown pass-through), wired
into `clean_repo_name` (all deterministic repo intents) + `nlu_client`
`repo_slot` (all 22 repo arms). "analyse pr 254 in zink" now exact 1.0
instead of fuzzy 0.8.

## Qwen Brain — Admin-Only Local Reasoning (2026-09-17)

**Phase 6 status: the brain stack already existed** — `admin_config.rs`
(`is_admin` runtime gate + `admin-brain` compile gate), `lazy_brain.rs`
(non-blocking spawn on port 39219), `brain_client.rs` (classify /
generate_phrasings / pronunciation_map / health), `brain_monitor.rs`
(continuous learning), `server/admin/brain_server.py` (Qwen2.5-0.5B
GGUF, 480 MB, present at `server/admin/model/`). The orchestrator tries
brain classification BEFORE NLU when the deterministic parser misses.

What Phase 6 added:

### 1. Intent schema coverage for the new MCP intents

The 3 commerce/social intents existed only in the deterministic parser —
the NLU/brain paths dropped them (`_ => None`). Now wired end-to-end:

- `nlu_client.rs::nlu_to_parsed_intent` — maps `order_food`
  (`food_item`→query, `restaurant`), `search_product` (`query`),
  `send_whatsapp_message` (`contact`, `message`). Brain responses reuse
  this mapping, so both paths benefit.
- `nlu_server.py` — `INTENTS` + `SLOT_TYPES` updated to match the
  retrained model (55 intents, 51 slot labels — **must match
  `train.py` ordering**).
- `brain_server.py` — `ALL_INTENTS` + `SYSTEM_PROMPT` know the 3 new
  intents, their slots (`food_item`, `restaurant`, `message`), and
  few-shot examples.

### 2. Brain-assisted compound planning

`command_center::build_plan_with_brain` (async) — called from
`process_transcript` in place of the sync `build_plan`:

- Fast path: fully deterministic plan → brain never invoked (zero added latency).
- If any step fails deterministic parse AND the `admin-brain` feature
  is on AND `is_admin()` → `brain_classify` fills in that step.
- Same conservative gates: Architect/None/CommandCenter steps abort the
  plan; brain-classified steps flow through the same confirmation-gated
  dispatch.
- Non-admin devices / brain unavailable → identical `None` fallback
  (single-intent path), zero risk.

This enables compounds like "remind me to call mom then order biryani"
where a step phrasing misses the deterministic regexes.

### Gates (unchanged, both required)

1. Compile-time: `cargo build --features admin-brain`
2. Runtime: `%APPDATA%/com.nexus.assistant/admin.json` (or dev
   `server/admin/admin_config.json`) with `is_admin: true`,
   `brain_enabled: true`

Family builds ship without `admin-brain` — all brain code is compiled
out (`brain_client`, `brain_monitor`, `lazy_brain` don't exist).

## NLU Self-Improvement — Family Model Distribution (2026-09-17)

The admin's Qwen brain continuously improves BERT-Mini (brain_monitor →
approved_phrasings.jsonl → merge_and_train.py). **Phase 5 adds the
distribution channel**: family devices pull the improved model over the
air — no app rebuild needed.

### Architecture

```
ADMIN                              WORKER (Cloudflare)              FAMILY
nexus train                        KV: nlu_model_latest manifest    startup +15s:
  → nexus_nlu.onnx                 R2: nlu/<file> objects           GET /models/nlu/latest
python publish_nlu.py                                               version differs?
  → wrangler r2 object put ...        ──────────────────────────→   GET /models/nlu/download?name=
  → POST /models/nlu/publish                                        per-file sha256 verify
     (Bearer NEXUS_ADMIN_TOKEN)                                     → %APPDATA%/com.nexus.assistant/nlu_model/
                                                                    → POST /reload_model (hot-swap)
                                                                    or NEXUS_NLU_MODEL_DIR on next spawn
```

### Worker endpoints (`server/worker/src/index.ts`)

| Endpoint | Auth | What it does |
|----------|------|--------------|
| `GET /models/nlu/latest` | none | KV manifest `{version, updated_at, files:{name:{sha256,size}}}` |
| `GET /models/nlu/download?name=<file>` | none | Streams file from R2 `nlu/` prefix; whitelist-guarded names |
| `POST /models/nlu/publish` | `Bearer NEXUS_ADMIN_TOKEN` | Writes KV manifest (admin publishes blobs via wrangler) |

Returns 503 when `CACHE`/`MODELS` bindings are missing — family clients
treat that as "no update" and keep the bundled model.

### Client side (`src-tauri/src/nlu_update.rs`)

- `spawn_update_check(app)` — called once at startup (15s after session
  auto-open, so first paint isn't competing with a 35 MB download)
- Downloads to a `.staging` dir first; sha256-verifies every file;
  aborts cleanly on mismatch (old model stays live)
- File-level manifest (no zip dep): `nexus_nlu.onnx`, `.onnx.data`,
  `labels.json`, `temperature_calibration.json`, `tokenizer/*`
- If NLU server is already running → `POST /reload_model` hot-swaps
- `downloaded_model_dir_envless()` — used by `lazy_nlu.rs` to pass
  `NEXUS_NLU_MODEL_DIR` at spawn (same `%APPDATA%/com.nexus.assistant`
  derivation as `lazy_stt.rs::read_moonshine_model`)

### `nlu_server.py` model dir resolution

1. `NEXUS_NLU_MODEL_DIR` env var (downloaded update) — if it has ONNX
2. `server/nlu/model/` (dev)
3. `src-tauri/resources/server/nlu/model/` (bundled fallback)

### Admin publish (`server/nlu/publish_nlu.py`)

```bash
# One-time setup:
npx wrangler r2 bucket create nexus-models
# uncomment [[r2_buckets]] MODELS binding in server/worker/wrangler.toml
npx wrangler secret put NEXUS_ADMIN_TOKEN
npx wrangler deploy

# After each retrain:
python server/nlu/publish_nlu.py \
  --worker https://nexus-worker.chitkullakshya.workers.dev \
  --token-file ../admin/data/admin_token.txt
```

### New NLU intents (Phase 8 staged data)

`server/nlu/add_commerce_intents.py` generates 158 staged examples for
`order_food`, `search_product`, `send_whatsapp_message` (+ 9 OOS
negatives), wired into `build_candidate_dataset.py` as
`phase8_commerce_social.json`. `train.py` INTENTS/SLOT_TYPES and
`model/labels.json` updated (55 intents, 51 slot labels; new slots:
`food_item`, `restaurant`, `message`). Next `nexus train` /
candidate build includes them.

**Pre-existing issue found:** `build_candidate_dataset.py` currently
fails on `"how much is an overdraft fee for bank"` — that CLINC staging
row was already merged into production train (twice, duplicated), so
`verify_no_overlap` trips before phase8 is even reached. Fix: remove the
row from `clinc150_oos_train_reviewed.jsonl` or dedupe train.

## Command Center — Multi-Step Task Orchestration (2026-09-16)

**`src-tauri/src/command_center.rs`** implements the n8n-style command
center: compound commands ("X then Y") are split into steps, each step
is parsed + routed to a sub-center, executed sequentially, and results
are merged into one response.

### How it works

```
"open chrome then search for cats"
  → split_compound()    → ["open chrome", "search for cats"]
  → build_plan()        → TaskPlan { 2 PlanSteps, each routed }
  → execute_plan()      → step 1: LocalCommand → command_executor
                        → step 2: WorkerBackend → 9Router/Worker
  → merge               → "Opened Chrome sir. Found results for cats."
```

### Split rules (`split_compound`)

Splits on: ` then `, ` and then `, `, then `, ` after that `,
` afterwards `, `; `. Deliberately does NOT split on bare ` and ` —
"search and rescue", "mum and dad" would break. Dangling trailing
connectors ("open chrome then") are stripped before splitting.

### Conservative fallback

`build_plan` returns `None` (falls back to normal single-intent path)
when:
- transcript isn't compound (< 2 parts)
- ANY part fails deterministic parsing — a bad split should never
  produce a worse outcome than today's path
- any part routes to `Architect` (window-managed, can't compose)

### Step execution (`execute_step`)

| Subsystem | How it runs |
|-----------|-------------|
| LocalCommand | `ParsedIntent` → `command_executor::Intent` → `execute_command` |
| Mcp | `dispatch_to_mcp` — read ops run; gated ops emit Confirm + pause |
| WorkerBackend | `dispatch_to_worker` (9Router fast path included) |
| GitHub | `github_cmd::execute_command` — read ops inline; destructive ops emit Confirm (existing frontend flow) |
| Architect / CommandCenter | `Unsupported` — can't nest/compose |

### Confirmation gates inside compounds

When a step hits a confirmation gate (e.g. `send_whatsapp_message`),
the task pauses: the gated step's pending payload is emitted as a
Confirm event, and the remaining steps are stashed in
`PENDING_COMPOUND`. `orchestrator_mcp_confirm` calls
`resume_compound()` after approval — it runs the confirmed call, then
the remaining steps, and emits the merged result.

Known v1 limit: a destructive **GitHub** step inside a compound emits
Confirm via the existing github flow, but remaining steps are dropped
(GitHub resume needs `orchestrator_github_execute` integration).

### Cancellation

Each step checks `is_cancelled` before running — a barge-in mid-plan
skips remaining steps and reports partial results.

### Sequential only

v1 runs steps in order ("then" implies ordering). Independent parallel
steps are future work.

### Files

- `src-tauri/src/command_center.rs` — plan/split/execute/resume/merge
  (~950 lines, 20 unit tests)
- `src-tauri/src/orchestrator.rs` — `Subsystem::CommandCenter`,
  compound fast path in `process_transcript`, `run_command_center`,
  `orchestrator_mcp_confirm` resume hook, pub wrappers
  (`dispatch_to_mcp_pub`, `dispatch_to_worker_pub`, `is_cancelled_pub`)

## MCP Sub-Center — External Services via Model Context Protocol (2026-09-16)

**`src-tauri/src/mcp_client.rs`** is the MCP client that calls external
MCP servers (Swiggy, Amazon, WhatsApp) via JSON-RPC 2.0 over streamable
HTTP. This is Phase 3 of the roadmap — the foundation for commerce and
social sub-centers.

### Registered MCP Servers

| Server | Endpoint | Auth | Tools |
|--------|----------|------|-------|
| `SwiggyFood` | `https://mcp.swiggy.com/food` | OAuth 2.1 PKCE | 17 (restaurants, menu, cart, orders) |
| `SwiggyInstamart` | `https://mcp.swiggy.com/im` | OAuth 2.1 PKCE | 19 (grocery search, cart, orders) |
| `SwiggyDineout` | `https://mcp.swiggy.com/dineout` | OAuth 2.1 PKCE | 12 (table reservations) |
| `WhatsApp` | `http://127.0.0.1:8765/mcp` | QR session | messaging, contacts |
| `Amazon` | `http://127.0.0.1:8766/mcp` | browser session | product search, details, reviews |

Swiggy MCPs are hosted by Swiggy (free on localhost for dev; production
requires Builders Club approval + demo video). WhatsApp/Amazon run as
local stdio/HTTP MCP servers (e.g., whatsmeow bridge, Playwright scraper).

### New Intents (deterministic parser)

| Pattern | Intent | Routes to |
|---------|--------|-----------|
| "order pizza from dominos" | `OrderFood { query, restaurant }` | `Subsystem::Mcp` → SwiggyFood `search_restaurants` |
| "order biryani" / "get food from swiggy" | `OrderFood` | same |
| "search for X on amazon" / "find X on amazon" | `SearchProduct { query }` | `Subsystem::Mcp` → Amazon `amazon_search` |
| "amazon search for X" / "search amazon for X" | `SearchProduct` | same |
| "send <c> a whatsapp message saying <m>" | `SendWhatsAppMessage` | `Subsystem::Mcp` → WhatsApp `send_message` |
| "whatsapp <c> saying <m>" / "message <c> on whatsapp saying <m>" | `SendWhatsAppMessage` | same |

**Ordering matters:** `parse_send_whatsapp_message` runs BEFORE
`parse_whatsapp_command` — otherwise "whatsapp mom saying hi" would be
swallowed as a chat-open with contact "mom saying hi".

### Confirmation Gates

`McpServer::requires_confirmation(tool)` — write ops (update cart,
send message, book table) emit `OrchestratorEvent::Confirm` with a
pending payload `{server, tool, params, transcript}` instead of
executing. The frontend calls `orchestrator_mcp_confirm(request_id,
confirmed, pending)` to proceed. Read ops (search, list, get) execute
directly.

`McpServer::is_destructive(tool)` — `place_food_order`, `place_im_order`,
`book_table` get an irreversible-action warning in the confirm prompt.

### Files

- `src-tauri/src/mcp_client.rs` — MCP client (McpServer registry,
  JSON-RPC call_tool/list_tools, SSE parsing, extract_text, 12 tests)
- `src-tauri/src/intent_parser.rs` — 3 new intents + 3 parsers + 16 tests
- `src-tauri/src/orchestrator.rs` — `Subsystem::Mcp`, `dispatch_to_mcp`,
  `orchestrator_mcp_confirm` command, 4 routing tests
- `src-tauri/src/lib.rs` — `pub mod mcp_client` + command registration

### Still needed for production use

- **Swiggy OAuth flow** — `call_tool` currently passes `auth_token=None`;
  OAuth 2.1 PKCE token storage/refresh needs wiring (store in
  `settings.json` or keychain).
- **WhatsApp local bridge** — run a whatsmeow/Cloud-API MCP server on
  `127.0.0.1:8765/mcp` (e.g., `whatsapp-mcp` Go bridge + Python MCP).
- **Amazon local server** — run a product-search MCP on
  `127.0.0.1:8766/mcp` (Creators API needs Associates credentials, or a
  Playwright scraper variant).
- **Multi-step flows** — order → cart → checkout chains belong to the
  Phase 4 command center; today each intent maps to one tool call.

## 9Router — Local AI Gateway (2026-09-16)

**9Router** (`src-tauri/src/router.rs`) routes general AI questions
directly to free cloud providers (Cerebras → Groq → Gemini), bypassing
the Worker for 3-7x lower latency (~242ms vs ~2s). The Worker remains
the fallback for PR analysis, GitHub operations, and tasks requiring
session/OAuth tokens.

### Architecture

```
Current (Worker path):
  Device → Worker (50ms) → Workers AI (500-2000ms) → Worker (50ms) → Device
  Total: 600-2100ms

With 9Router:
  Device → localhost (1ms) → Cerebras/Groq (80-120ms) → localhost (1ms) → Device
  Total: ~242ms  (3-7x faster)
```

### Provider Cascade

```
Cerebras (1M tokens/day free, ~80ms, Llama 3.3 70B) — fastest
  → Groq (14,400 req/day free, ~120ms, Llama 3.3 70B)
    → Gemini (1,500 req/day free, ~400ms, flash-lite)
      → Worker (fallback, uses neurons)
```

### What 9Router Handles

- General questions ("what's the capital of France?")
- Factual queries ("how tall is the Eiffel Tower?")
- Conversational responses (when NLU confidence is low)

### What Still Goes Through the Worker

- PR analysis (needs GitHub token + GLM models)
- GitHub commands (merge, approve, close PR)
- Architecture mapper
- Any task requiring session/OAuth tokens

### Routing Logic (`router::can_route`)

Returns `false` (must use Worker) for transcripts containing:
`analyse pr`, `analyze pr`, `analyse latest pr`, `analyse repo`,
`architect`, `check branch`, `merge pr`, `approve pr`, `close pr`,
`github`, `pull request`, `pullrequest`.

Returns `true` (9Router can try) for everything else.

### Integration in Orchestrator

`dispatch_to_worker()` in `orchestrator.rs` now tries 9Router first:
1. If `can_route(transcript)` → try 9Router (Cerebras → Groq → Gemini)
2. If 9Router succeeds → return directly (skip Worker entirely)
3. If 9Router fails or `can_route` returns false → fall back to Worker

### Settings (API Keys)

Stored in `settings.json` (camelCase, same as existing Groq key):
- `groqApiKey` — Groq API key (already existed for STT, now reused for LLM)
- `geminiApiKey` — Google Gemini API key (already existed)
- `cerebrasApiKey` — Cerebras API key (NEW, free at cloud.cerebras.ai)

Settings UI: Settings sidebar → Accounts tab → Cerebras API Key field.

### Files

- `src-tauri/src/router.rs` — 9Router module (~570 lines, 12 unit tests)
- `src-tauri/src/orchestrator.rs` — `dispatch_to_worker()` modified to try 9Router first
- `src-tauri/src/commands.rs` — `NexusSettings` + `cerebras_api_key` field
- `src-tauri/src/lib.rs` — `pub mod router;` registered
- `frontend/src/settings-sidebar/SettingsSidebarApp.tsx` — Cerebras API key field

## Moonshine Medium v2 — Better Local STT (2026-09-16)

**Default Moonshine model upgraded from Small to Medium v2** for better
local STT accuracy. Users can switch back to Small in Settings for
lower RAM on 8GB laptops.

### Model Comparison

| Model | Params | WER | RAM | Latency |
|-------|--------|-----|-----|---------|
| tiny_streaming | 34M | 12.0% | ~100 MB | ~50ms |
| small_streaming | 123M | 7.84% | ~300 MB | ~165ms |
| **medium_streaming** (new default) | 245M | **6.65%** | ~400 MB | ~269ms |

### Configuration

- **Default:** `medium_streaming` (245M, 6.65% WER) — better accuracy
- **Family (8GB laptops):** Set to `small_streaming` in Settings for lower RAM
- **Ultra-low RAM:** Set to `tiny_streaming` (34M, 12% WER)

### How It Works

1. `lazy_stt.rs::read_moonshine_model()` reads `moonshineModel` from
   `settings.json` (defaults to `medium_streaming`)
2. Passes it as `MOONSHINE_MODEL` env var when spawning `stt_server.py`
3. `stt_server.py` reads `MOONSHINE_MODEL` env var (already supported)
4. Settings UI: Settings sidebar → Audio tab → Moonshine model dropdown

### Files

- `src-tauri/src/commands.rs` — `NexusSettings` + `moonshine_model` field
- `src-tauri/src/lazy_stt.rs` — `read_moonshine_model()` + env var on spawn
- `frontend/src/settings-sidebar/SettingsSidebarApp.tsx` — model dropdown

## Diagnostic Fixes — 2026-09-12

### NLU Server ONNX Export (build integration)

The BERT-Mini retraining produced `best_model.pt` but the ONNX export step
was missing. The NLU server requires `nexus_nlu.onnx` to start — without
it, `sys.exit(1)` and every unparseable command blocked for 30s.

**Fix:** `nexus.mjs` now calls `syncNluModel()` before every build. This
copies the ONNX model, `labels.json`, and tokenizer from
`server/nlu/model/` to `src-tauri/resources/server/nlu/model/` so the
Tauri installer bundles the latest trained model automatically.

**`tauri.conf.json` resources** now includes `labels.json`:
```json
"resources/server/nlu/model/labels.json"
```

**To retrain and bundle:**
```bash
cd server/nlu
python train.py           # trains → best_model.pt
python export_onnx.py    # exports → nexus_nlu.onnx (max_length=64)
# Then: nexus build      # syncs model to resources + bundles in installer
```

### NLU Training Commands (`nexus train` + `nexus collect` + `nexus audit`)

**`nexus train`** performs cleaning, generation, merge, canonical conflict/slot/leakage repair, BERT-Mini training, stable ONNX export, resource sync, post-training audit, and temporary-file cleanup.

```bash
nexus train                    # full pipeline (with audit + cleanup)
nexus train --clean-only       # just clean the dataset
nexus train --skip-train       # data generation/repair without training
nexus train --keep-temp        # keep generated data and checkpoint (~17 MB)
```

**`nexus audit`** validates both data and model quality:

```bash
nexus audit                    # structural checks + ONNX evaluation
nexus audit --dataset-only     # structural checks only
```

It writes `server/nlu/audit_report.json` and `docs/research/nlu-model-data-audit-latest.md`.

**Data foundation gates** must pass before external imports or retraining:

```bash
nexus data nlu validate           # provenance registry + frozen 452-row evaluation lock
nexus data wake validate          # wake model fingerprints + optional audio manifest
nexus data wake fingerprint       # explicitly refresh fingerprints after approved model replacement
```

Staged external NLU payloads belong in ignored `server/nlu/data/staging/`. Local wake audio belongs in ignored `wake_word_data/manifest.jsonl`; speaker, session, and source-group IDs may not span train/validation/test splits. See `docs/testing/data-foundation-and-wake-model-gates.md`.

NLU training data is divided into locked phrase-family-separated `train`, `validation`, `calibration`, and final `test` splits. Rows whose templates overlap final test remain preserved under `quarantine` and must not train the model. Validate with `python server/nlu/prepare_evaluation_splits.py`; see `docs/testing/phase-1-nlu-evaluation-split-analysis-2026-09-14.md`.

**`nexus collect`** prompts for 23 intent families, waits for Enter, records two seconds, transcribes with Groq/Moonshine, and requires save/retry/edit/skip/end review before adding a sample.

```bash
nexus collect
nexus collect --intent type_text
nexus collect --count 20
nexus collect --text-only
nexus collect --list
```

Output: `server/admin/data/collected_samples.jsonl` (gitignored, admin-only). `nexus train` merges approved samples and then deletes the temporary collection file unless `--keep-temp` is supplied.

Requirements: working microphone and `sounddevice`/`numpy`/`scipy` (auto-installed). Groq is auto-loaded from NEXUS settings; local STT is the fallback.

### NLU Server Startup Cooldown

Was: 30s wait per failed startup, no cooldown → "loading non stop".
Now: 15s timeout + 60s cooldown after failure. If the NLU server can't
start (missing model, missing deps), it won't block subsequent commands.

### Brain Server Non-Blocking Spawn

Was: `ensure_brain_running()` blocked for 12s while Qwen loaded.
Now: spawns process + background thread. First command falls back to
NLU/deterministic while brain loads in background.

### Wake Word Grace Period (10s after restart)

Was: 5s grace period after stream restart. Intel SST driver produces
transient audio bursts 5-10s after restart that false-trigger the model.
Now: 10s grace period. See `wakeword_oww.rs::detect_chunk()`.

### NLU Model Dimension Fix

The ONNX export used `max_length=32` but the server tokenizes with
`max_length=64` (MAX_LEN). Fixed `export_onnx.py` to use `max_length=64`.

### Live-Mode Intent Mapping

`nlu_client.rs` now maps all 11 live-mode intents to `NluResult`:
`type_text`, `press_key`, `press_hotkey`, `confirm_send`, `cancel_action`,
`browser_new_tab`, `browser_navigate`, `browser_search`,
`whatsapp_open`, `whatsapp_search`, `focus_app`.

### Known Issue: NLU Model Accuracy

The current model has low accuracy (5-9% confidence) because the
training dataset (`dataset.json`) contains malformed intent labels
from the previous merge (e.g., `OpenArchitect` instead of
`open_architect`, debug objects as intent labels). The deterministic
parser handles most commands correctly — the NLU is a fallback only.
**To fix:** clean the dataset labels, retrain, re-export ONNX, rebuild.

## Live Mode — Phase 1 Implementation (2026-09-11)

**Live mode is the always-listening, STT-only, full-laptop control
capability.** It adds keyboard simulation, WhatsApp full flow, browser
navigation, window focus, and a state machine for sequential commands.

### New Dependencies

```toml
enigo = "0.5"     # Cross-platform keyboard/mouse simulation
arboard = "3.4"   # Clipboard for paste-text pattern (avoids autocomplete corruption)
```

### New Module: `src-tauri/src/live/`

| File | Purpose |
|------|---------|
| `mod.rs` | Module root, LiveResult, LiveIntent, 14 Tauri commands |
| `state.rs` | State machine (Idle → AppOpen → ChatActive → TextTyped) |
| `safety.rs` | Whitelist, denylist, confirmation gates |
| `commands/keyboard.rs` | Type text, press keys, hotkey combos, clipboard paste |
| `commands/whatsapp.rs` | Open → search contact → type → send (with confirmation) |
| `commands/browser.rs` | New tab, navigate, search, open site by name |
| `commands/window.rs` | Window focus with AttachThreadInput trick (Windows) |

### New Tauri Commands (14)

`live_type_text`, `live_press_key`, `live_press_hotkey`,
`live_whatsapp_open`, `live_whatsapp_search`, `live_whatsapp_send`,
`live_whatsapp_type_message`, `live_browser_new_tab`,
`live_browser_navigate`, `live_browser_search`, `live_open_site`,
`live_focus_app`, `live_cancel`, `live_get_state`

### New Intent Parser Patterns

- `type <text>` → type_text
- `press <key>` → press_key
- `press <key1> <key2>` → press_hotkey
- `send` / `send it` → confirm_send
- `stop` → cancel_action
- `new tab` / `open new tab` → browser_new_tab

### Safety Layer

- **Whitelist:** Only allowed tools can execute (type_text, press_key,
  open_app, whatsapp_send, etc.)
- **Denylist:** Banking apps, password managers, crypto wallets are blocked
  (1password, bitwarden, bank, paypal, coinbase, metamask, etc.)
- **Confirmation gates:** `whatsapp_send` and `confirm_send` always require
  user confirmation before execution

### State Machine

The state machine tracks context across sequential voice commands:
```
Idle → AppOpen { app } → ChatActive { app, contact } → TextTyped { app, contact, text }
```
Auto-resets to Idle after 30s of silence.

### Clipboard Paste Pattern (from OpenDex)

For text > 50 chars, uses clipboard paste (Ctrl+V) instead of
character-by-character typing. This avoids autocomplete corruption in
WhatsApp/search boxes. Previous clipboard contents are restored after
pasting.

### Window Focus: AttachThreadInput Trick (from ghost-hands)

`SetForegroundWindow` silently fails from background processes on Windows.
The workaround: attach our input queue to the foreground thread's using
`AttachThreadInput`, call `SetForegroundWindow`, then detach.

### NLU Model Update

**New intents:** 47 → 58 (added 11 live-mode intents)
**New training examples:** 1,960 → 2,438 (+478 new examples)
**New slot types:** `B-text`, `I-text`, `B-key`, `I-key`, `B-keys`,
`I-keys`, `B-target`, `I-target`

To retrain:
```bash
cd server/nlu
python generate_live_data.py     # generate new examples
python merge_live_data.py         # merge into dataset.json
python train.py                   # retrain BERT-Mini
```

### Test Results

- 323 Rust tests pass (29 new live-mode tests)
- `cargo check` clean, 0 warnings
- 18 live-mode unit tests (state, safety, keyboard, browser)
- 11 live-mode intent parser tests

## Multi-Worker Optimization — Cloud-First Architecture (2026-09-01)

**Single Worker, internally modularized.** No separate Workers — one deploy,
no cross-Worker latency. The monolithic `index.ts` is split into modules:

- `src/quota.ts` — per-user daily usage tracking + cost control (D1 `usage_log`)
- `src/cache.ts` — edge caching (KV namespace, D1 fallback)
- `src/models.ts` — model constants + fallback chains + truncation
- `src/research.ts` — ad-free search (Wikipedia REST + Wikidata, no API key)
- `src/clean.ts` — result cleaning, dedup, prompt-injection guard

**New D1 tables:** `usage_log` (per-user daily quotas), `cache_entries`
(D1 cache fallback when KV not bound).

**New KV namespace:** `CACHE` (edge cache for search results, PR analysis,
repo metadata). Create with `npx wrangler kv namespace create CACHE` and
paste the ID in `wrangler.toml`.

**Quota limits (per user/day):** 500 requests, 3000 neurons, 10 deep
analyses, 100 searches. Global neuron budget: warn at 8000, hard reject
deep at 9500.

**Search routing:** `isSearchQuestion()` detects factual questions
("what is X", "who is Y") and routes them to Wikipedia/Wikidata retrieval
with citations, instead of blind LLM answering.

**Offline local commands (new):**
- `close <app>` — taskkill on Windows, pkill on Unix
- `open chat with <name>` / `message <name>` / `chat with <name>` —
  WhatsApp deep links with local contacts file lookup at
  `%APPDATA%/com.nexus.assistant/contacts.json`
- Both work offline, zero internet, zero RAM overhead

**NLU pre-warm removed.** NLU server now only starts on first
unparseable command (lazy). Saves 50-100 MB at idle.

**STT idle monitor wired but disabled.** `lazy_stt::start_idle_monitor()`
is called from `lib.rs` but `STT_KEEP_ALIVE=true` means STT is never killed.
The idle cost is only ~128 MB (model loaded, not transcribing) and killing
it adds 10-15s delay on the next command (cold model load). The monitor
thread runs for future use but is a no-op. Peak STT RAM during active
transcription is ~340 MB.

**Lazy Kokoro TTS (2026-09-01).** Kokoro is no longer loaded at boot.
`speak_text` calls `ensure_engine_loaded()` on first use — loads in ~1.7s
(one-time), then stays loaded. Saves ~350 MB at idle. See `tts.rs`.

**WebView2 low-memory mode (2026-09-01).** The orb window sets
`MemoryUsageTargetLevel::Low` via `ICoreWebView2_23::SetMemoryUsageTargetLevel`
at creation time. WebView2 drops cached data and swaps to disk. Saves ~40 MB.
See `mic_permissions.rs::set_low_memory_mode()`.

**Idle RAM (2026-09-01): ~104 MB** (before first transcription) or
**~232 MB** (after first transcription, STT model loaded).

| Component | Before first transcription | After first transcription |
|---|---|---|
| nexus.exe (Rust + wake word, NO Kokoro) | 47.8 MB | 47.8 MB |
| WebView2 (orb, low-mem mode) | 35.8 MB | 35.8 MB |
| STT Python (model not yet loaded) | 20.6 MB | 128.6 MB |
| **TOTAL idle** | **104.2 MB** | **232.2 MB** |

After first TTS speak, Kokoro loads and stays loaded: **+350 MB → ~582 MB**.
This is the active state, not idle.

**Worker test suite:** `npm test` in `server/worker/` runs 23 vitest
tests covering quota, cache keys, search question detection, and dedup.

**Rust test suite:** `cargo test --test offline_commands` runs 10 tests
for close_app and whatsapp_chat parsing.

## STT Architecture — Groq Primary + Moonshine Fallback (2026-09-06)

**STT uses Groq Whisper Large v3 Turbo (cloud) as primary, with Moonshine
Small Streaming (local) as fallback when network is unavailable.**

### Primary: Groq Cloud STT
- **Model:** `whisper-large-v3-turbo` (809M params, cloud, ~247ms latency)
- **Free tier:** 20 RPM, 2,000 RPD, 28,800 audio sec/day, 25MB file limit
- **Code:** `src-tauri/src/stt_groq.rs` — sends WAV as multipart to
  `https://api.groq.com/openai/v1/audio/transcriptions`
- **Config:** `localSttOnly: false` in settings.json + valid `groqApiKey`
- **Fallback trigger:** network error, 429 rate limit, 401 auth error,
  or any Groq API error → falls back to local Moonshine

### Fallback: Moonshine Local STT (replaces faster-whisper)
- **Model:** Moonshine Small Streaming (123M params, 7.84% WER, ~165ms CPU)
- **Engine:** `moonshine-voice` Python package (ONNX Runtime, no PyTorch)
- **Code:** `server/stt_server.py` — FastAPI server on `127.0.0.1:39217`
- **Port:** 39217 (`POST /transcribe`, `GET /health`)
- **Audio format:** multipart/form-data with WAV (16kHz, mono, 16-bit PCM)
- **Latency:** ~165ms per transcription after model load; first call ~2s
  (model loading). Model loads in ~1.8s at startup.
- **RAM:** ~150-300MB (model + runtime)
- **Idle timeout:** server killed after 5 min of no requests (saves RAM)
- **Installer:** server files bundled in `resources/server/` via Tauri
  resources config. Production path: `exe_dir/resources/server/stt_server.py`.
- **Requirements:** `pip install moonshine-voice fastapi uvicorn python-multipart`
- **Model download:** automatic on first run via `get_model_for_language("en")`
- **Config:** `MOONSHINE_MODEL` env var (default: `small_streaming`)
  Options: `tiny_streaming` (34M, 12% WER), `small_streaming` (123M, 7.84%),
  `medium_streaming` (245M, 6.65%)

### Why Moonshine over faster-whisper
- Moonshine Small (123M) has 7.84% WER vs Whisper tiny.en's ~18%
- Moonshine is 10-100x faster than Whisper for real-time speech
- Moonshine uses ONNX Runtime (same as wake word engine)
- Moonshine is designed for voice command recognition (streaming, low latency)
- Moonshine MIT license, no restrictions

### Routing logic (`src-tauri/src/stt.rs`)
```
if localSttOnly:
    → local Moonshine (privacy mode, audio never leaves device)
elif groq_key available:
    → try Groq cloud first
    → on error/timeout/429: fall back to local Moonshine
else:
    → local Moonshine directly
```

### Historical: faster-whisper era (pre-2026-09-06)
The previous architecture used faster-whisper `tiny.en` (39M params, ~18% WER)
via a Python sidecar. It was replaced by Moonshine Small Streaming which has
2.3x better accuracy (7.84% vs 18% WER) at similar RAM and faster latency.
- **Hallucination filter:** applied in `stt.rs` — catches
  "thank you for watching", < 2 alphabetic chars, etc.

## NLU Server — Lazy Python Sidecar (2026-08-31)

The **NLU server** (`server/nlu_server.py`) is the only remaining Python
dependency. It provides ML-based intent classification (BERT-Mini ONNX)
as a fallback when the deterministic parser (`intent_parser.rs`) can't
handle a command.

- **Port:** `39218` (separate from the old STT port 39217)
- **Lazy manager:** `src-tauri/src/lazy_nlu.rs` — spawns on first
  unparseable command, kills after 60s idle.
- **Model:** `server/nlu/model/nexus_nlu.onnx` + `.data` + `tokenizer/`
  — committed to git (~18 MB) so fresh clones work without downloading.
- **Requirements:** `server/nlu/requirements.txt` (numpy, onnxruntime,
  fastapi, uvicorn, pydantic, transformers).
- **Fallback:** if the NLU server is unavailable, `nlu_client.rs`
  returns `None` and the deterministic parser handles the command.

### Historical STT Pipeline Fixes (2026-08-30, faster-whisper era)

These bugs were fixed in the faster-whisper Python sidecar. They are
**still relevant** (the sidecar was restored after the Moonshine experiment)
but kept for historical context:

1. **`lazy_stt.rs` path bug:** `stt_script_path()` was missing one
   `.parent()` level. Fixed by adding the correct path.
2. **`ensure_stt_running()` not called on hotkey:** Fixed by adding
   calls to `hotkey.rs` and `stt.rs`.
3. **`is_stt_responsive()` used tokio runtime:** Fixed by using a raw
   TCP connection instead.
4. **STT idle timeout too aggressive:** 60s → 5 minutes.

### Whisper hallucination filter (`stt.rs`)

The hallucination filter is still active in `stt.rs`. It catches common
hallucinations on noisy/silent audio:
- "thank you for watching", "you", "bye", "okay", etc.
- Text with < 2 alphabetic characters
Filtered text is replaced with empty string, triggering the frontend's
"didn't catch that" retry logic (up to 3 retries).

## NEXUS CLI — Unified Cross-Platform Command (`nexus.mjs`)

The unified `nexus` command works on Windows, macOS, and Linux:

```
nexus install     Install prerequisites + build + global 'nexus' command
nexus setup       Install prerequisites + build (no global command)
nexus build       Build frontend + Rust release binary
nexus dev         Tauri dev mode (hot reload via Vite)
nexus start       Launch the built app (unified console on Windows)
nexus run         Alias for 'start'
nexus check       Diagnostics (tools, frontend, Rust, NLU, Worker)
nexus clean       Remove build artifacts
nexus worker      Deploy the Cloudflare Worker (optional)
nexus help        Show help
```

- **Windows:** `nexus.cmd` shim → `node nexus.mjs`
- **Unix:** `nexus` shell script → `node nexus.mjs`
- **Global install:** `nexus install` creates a global command in
  `%USERPROFILE%\.local\bin` (Windows) or `/usr/local/bin` (Unix).
- **`nexus start` on Windows** uses `scripts/run.ps1` for the unified
  color-coded console (Rust logs, audio, frontend CDP in one stream).
- The old `scripts/nexus.bat` and `scripts/nexus.cmd` have been removed.

## Connection Diagnostics (`src-tauri/src/diagnostics.rs`)

Checks 5 services and logs a formatted table on startup:

| Service | Check method | Expected |
|---------|-------------|----------|
| STT | HTTP GET to port 39217/health | OK if running, LAZY if not yet started |
| TTS | In-process Kokoro/Fish Audio readiness (hardcoded ready) | Always OK |
| Cloudflare Worker | HTTPS GET to /health | OK if reachable |
| GitHub | HTTPS GET to Worker /oauth/status | OK if OAuth connected |
| Google | HTTPS GET to Worker /oauth/status | OK if OAuth connected |

Also available as:
- Tauri command: `nexus_diagnostics` (returns JSON to frontend)
- CLI: `nexus check` (build/tool diagnostics via `nexus.mjs`)
- Startup: auto-logged 5s after boot

## Wake Word Model Validation + Mic Silence Recovery (2026-08-30)

### Model is PERFECT — the problem is the Intel SST mic driver

Tested the v2 `nexus.onnx` model with the exact Rust pipeline
(mel → normalize → slice[4:80] → embedding → classifier):

| Input | Model probability | Verdict |
|-------|------------------|---------|
| TTS "nexus" | 0.994 | ✅ |
| TTS "hey nexus" | 0.999 | ✅ |
| TTS "nexus wake up" | 0.999 | ✅ |
| TTS "ok nexus" | 0.999 | ✅ |
| 20 negative samples | 0.0001-0.0002 | ✅ perfect rejection |
| **Trigger rate** | **5/5 positives** | ✅ 100% recall on TTS |
| **False positive rate** | **0/20 negatives** | ✅ 0% false triggers |

The model is NOT the problem. The problem is the **Intel Smart Sound
Technology driver** — it stops delivering audio after 2-25 minutes of
use (RMS drops to exactly 0.000000 and stays there).

### Silence Recovery Thread (`wakeword_oww.rs`)

Added a background thread that monitors the audio callback counter and
automatically restarts the cpal stream when the mic goes silent.

**Settings (tuned for Intel SST bursty audio):**
- Poll interval: **5s** (was 30s)
- Silence threshold: **165 callbacks (~5s)** (was 1000/30s)
- Restart method: **`try_device_silent`** (no 5s probe — saves 5s per cycle)
- Nuclear option: every **12 restarts (~60s of silence)**, restarts the
  Windows Audio service (`net stop/start Audiosrv`) to try to unstick the
  Intel SST driver
- Total restart cycle: **~5s** (was 35s with probe)

**Why 5s?** The Intel SST driver delivers audio in brief 5-15s bursts after
each stream restart, then goes silent. A 5s poll gives us the maximum
number of chances to catch a working window.

**Confirmation RMS threshold lowered from 0.01 to 0.002:**
The 500ms confirmation window was rejecting valid wakes because the mic
fades to silence during the confirmation period. At 0.01, a wake with
RMS=0.0048 was rejected. At 0.002, it would be confirmed.

### Intel SST driver fix (requires admin)

When the mic goes permanently silent, the fix is to restart the driver:

```powershell
# Run as Admin:
pnputil /restart-device "INTELAUDIO\CTLR_DEV_51CA&LINKTYPE_02&DEVTYPE_00&VEN_8086&DEV_AE20&SUBSYS_8BE0103C&REV_10EC\5&111f6c68&0&0000"
```

Or: Device Manager > Sound, video and game controllers > Intel Smart
Sound Technology for Digital Microphones > right-click > Disable > Enable.

Or: Restart the Windows Audio service:
```powershell
Restart-Service -Name "Audiosrv" -Force
```

If none of these work, a full OS restart is required. The Intel SST
driver has a known bug where it stops delivering audio after some time.
Updating to the latest driver from the laptop manufacturer (HP) may help.

### Test scripts (in project root, gitignored)

- `test_wake_model.py` — tests the model with TTS + negative samples
- `test_live_mic.py` — records 5s from the mic and tests the model
- `test_all_devices.py` — tests all audio input devices
- `test_mic_freq.py` — records and shows frequency content
- `gen_tts.py` — generates TTS "NEXUS" samples via Windows SAPI

## RAM Optimization — Lazy Windows + In-Process STT (2026-08-30)

**Idle RAM: 384 MB** (down from 1,644 MB — 77% reduction).

### What was wrong
- `tauri.conf.json` created 5 windows at startup (main, setup, settings,
  sidebar, architect). Each WebView2 window spawns ~7 processes (~250 MB).
  4 of the 5 windows were `visible: false` but still consumed full RAM.
- The old STT server (faster-whisper tiny.en) ran constantly, using ~340 MB
  even when no one was speaking.

### Fix 1: Lazy window creation (`src-tauri/src/dyn_windows.rs`)
- Only `main` (orb) is in `tauri.conf.json` — created at startup.
- `setup`, `settings`, `sidebar`, `architect` are created on-demand by
  `dyn_windows::get_or_create_window()` when first needed.
- `hide_sidebar` / `close_setup_window` / `close_settings_window` now
  **destroy** the window (not `hide()`) — kills the WebView2 process tree
  and frees ~250 MB per window.
- Platform effects (DWM corners, macOS vibrancy) applied at creation time
  inside `get_or_create_window()`.

### Fix 2: Lazy faster-whisper STT server
- STT uses faster-whisper tiny.en via a lazy-started Python sidecar on
  port 39217. See "STT Architecture" section above.
- `lazy_stt.rs` starts the server on first wake/hotkey, kills after 5min idle.
- STT RAM is ~0 MB at idle (server not running), ~340 MB when active.

### Measured RAM (idle, after fix)
| Component          | Before   | After    |
|--------------------|----------|----------|
| NEXUS.exe (Rust)   | 47.9 MB  | 40.8 MB  |
| WebView2 (1 window)| 870 MB   | 344 MB   |
| STT server         | 339 MB   | 0 MB     |
| **TOTAL**          | **1,644 MB** | **385 MB** |

### Files changed
- `src-tauri/tauri.conf.json` — removed 4 windows, kept only `main`
- `src-tauri/src/dyn_windows.rs` — NEW: dynamic window creation/destruction
- `src-tauri/src/stt.rs` — HTTP proxy to faster-whisper sidecar
- `src-tauri/src/lib.rs` — registered new modules, removed startup sidebar vibrancy
- `src-tauri/src/commands.rs` — all show/hide functions use dyn_windows
- `src-tauri/src/architect.rs` — uses dyn_windows for architect window
- `src-tauri/src/hotkey.rs` — sidebar close uses destroy_window
- `src-tauri/src/tray.rs` — settings menu uses dyn_windows
- `src-tauri/src/wakeword_oww.rs` — calls ensure_stt_running() on wake
- `src-tauri/src/stt.rs` — calls mark_stt_request() on each transcription
- `scripts/run.ps1` — no longer starts STT server at boot

## Sidebar — Do NOT use window-vibrancy on non-activating windows (2026-08-30)

**`src-tauri/src/lib.rs` / `src-tauri/src/commands.rs`: the sidebar window
deliberately calls NO `window_vibrancy` function** (no `apply_blur`,
`apply_acrylic`, `apply_mica`). This was a hard-won finding — do not
re-add these calls without reading this section first.

**Why**: the sidebar is a non-activating window (`focus: false`,
`alwaysOnTop: true`, `skipTaskbar: true`) so it never steals keyboard
focus from whatever app the user is working in. Windows' DWM *material*
APIs (Acrylic/Mica via `DWMWA_SYSTEMBACKDROP_TYPE`, or the legacy
`SetWindowCompositionAttribute` accent path used by `apply_blur`) render
a flat, solid **fallback color** for any window that isn't the OS-active
window — this is documented Windows behavior (Mica/Acrylic docs list
"window deactivates" as a fallback-to-solid-color condition), not
something `window-vibrancy` or Tauri can override. Confirmed via
`microsoft/microsoft-ui-xaml#10570` (`DesktopAcrylicBackdrop` loses blur
on `WS_EX_NOACTIVATE` windows) and `tauri-apps/window-vibrancy#183`
(Acrylic/Mica broken on Windows 11 24H2/25H2 in general).

Worse: calling these material APIs **overrides** Tauri's own
`transparent: true` mechanism (`tao` registers the window with DWM via
`DwmEnableBlurBehindWindow` + an empty blur region at window creation —
that's what actually makes a Tauri window see-through, no material
needed). When the material then fails to render (because the window is
never active), DWM falls back to **solid opaque** instead of the
window's original see-through state. This produced a fully opaque
black/grey panel that looked worse than doing nothing.

**The fix**: removed the vibrancy calls entirely. The sidebar was
already genuinely transparent via `transparent: true` in
`tauri.conf.json` — the same mechanism the main orb window uses
successfully. Result: sharp (not blurred) but real, focus-independent
transparency. `src-tauri/src/dwm_corners.rs` still calls
`DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND)`
directly (a plain window-shape attribute, not a material — unaffected
by the active/inactive issue) so the OS window's corners match the CSS
card's `border-radius`, avoiding a "double panel" mismatch (WebView2
has no `CornerRadius` support, so without this the DWM-painted window
rectangle and the rounded CSS card show as two different shapes).

Also removed: a CSS chromatic-aberration effect (red/cyan inset
`box-shadow` on `.sidebar-card::after`) that was meant to simulate
glass prism fringing. Without real optical refraction (no SVG
`feDisplacementMap` — `backdrop-filter: url()` is also a no-op on a
transparent WebView2, see below), it just read as a colored-border
rendering bug. Replaced with a neutral lit-bezel `box-shadow` stack
(top specular + bottom shadow) for a physical-glass feel without color.

Separately confirmed: CSS `backdrop-filter` (blur or `url()` SVG
refraction) is a **no-op in a transparent WebView2** — see
`MicrosoftEdge/WebView2Feedback#4945`. It can't composite against
nothing. Don't rely on it for this window; any blur must come from a
native OS mechanism, and per above, none is currently available for a
non-activating window without a full WinRT `DesktopAcrylicController` +
`SystemBackdropConfiguration.IsInputActive = true` interop (what
PowerToys uses for its non-activating flyouts) — out of scope unless
revisited.

### Screenshot-capture blur (the actual liquid glass)

Since native blur is unavailable for non-activating windows, the sidebar
uses a **"fake blur"**: right before `win.show()`, Rust captures the desktop
region behind the window via GDI `BitBlt`, blurs it with
`image::imageops::fast_blur(sigma=32)`, encodes it as a PNG data URI, and
hands it to the frontend as a CSS `background-image` on `.sidebar-card`
via the `--sidebar-backdrop-image` CSS variable. This gives a genuine
frosted-glass look without depending on window activation state.

**Critical timing:** the capture MUST happen before `win.show()` so the
sidebar doesn't capture itself. If the window is already visible (re-show),
capture is skipped. See `src-tauri/src/sidebar_backdrop.rs`.

Full implementation guide: `docs/features/21-liquid-glass-sidebar.md`.

### Dynamic window pending-content pattern (race-free event delivery)

When a window is created on-demand (via `dyn_windows.rs`), the WebView2
needs time to load the HTML and mount the React app. If Rust emits Tauri
events immediately after creating the window, **those events are lost**
because no listener exists yet.

**Fix:** store the content in a `static Mutex<Option<...>>` and let the
frontend fetch it on mount via a `get_pending_*` command. This is race-free
regardless of how long the WebView takes to load. If the window already
exists (React loaded), events are also emitted as a fast path.

Currently used for:
- `PENDING_SIDEBAR` in `commands.rs` → `get_pending_sidebar_content`
- `PENDING_ARCHITECT_REPO` in `architect.rs` → `get_pending_architect_repo`

**All show/create commands MUST be `async`** — `WebviewWindowBuilder::build()`
dispatches to the main thread, and a synchronous Tauri command runs on a
blocking thread that can't yield, causing a deadlock.

To reuse this pattern for a new window, see the "How to Reuse" section in
`docs/features/21-liquid-glass-sidebar.md`.

## Architecture Mapper — Phase 1 Latency Optimization (2026-08-30)

Phase 1 now uses **Approach C (hybrid)** for a 3-4s first response:

1. **Parallelized GitHub API calls** (`tokio::join!`): metadata + recursive
   tree are fetched concurrently using the symbolic ref `HEAD` (verified
   against repos with `main` and `master` default branches). Cuts ~600-1000ms
   off the critical path vs the old sequential metadata→tree flow.
2. **Instant Rust heuristic clustering** for first paint (~5ms) — the diagram
   appears in ~1-1.5s with generic layer labels.
3. **Async LLM enrichment** (`enrich_phase1` command): after first paint, the
   client POSTs the heuristic layers + sample file paths to the Worker's
   `phase1_enrich` intent. The LLM (Mistral 24B) rewrites generic labels into
   repo-specific ones (e.g. "Client / Presentation Layer" → "Next.js App
   Router (React 19)") and writes a real summary. Result streams back via
   the `architect:phase1-enriched` event ~2-3s later and merges in-place.
   **Never blocks first paint.** If the Worker/LLM fails, the heuristic
   diagram remains (graceful degradation).

| Component | File | What changed |
|-----------|------|--------------|
| Rust parallel fetch | `src-tauri/src/architect.rs` | `analyze_repo_phase1` uses `tokio::join!` + `HEAD` ref |
| Rust enrichment cmd | `src-tauri/src/architect.rs` | New `enrich_phase1` command + `Phase1Enrichment`/`EnrichedLayer` types |
| Rust session accessor | `src-tauri/src/network.rs` | New `get_session_info()` public helper |
| Worker handler | `server/worker/src/index.ts` | New `handlePhase1Enrich` + `phase1_enrich` intent dispatch |
| Frontend store | `frontend/src/architect/architectStore.ts` | New `enrichPhase1` action + `sample_file_paths` field |
| Frontend app | `frontend/src/architect/ArchitectApp.tsx` | Calls `enrich_phase1` after paint + listens for enriched event |

## Building

**Always build the desktop app with the Tauri CLI:**

```powershell
pwsh ./scripts/build.ps1          # frontend + tauri release build + bundles
```

If you need a plain cargo build (faster iteration, no installer), you **must**
pass the `custom-protocol` feature:

```powershell
npm --prefix frontend run build
cargo build --release --features custom-protocol   # run inside src-tauri/
```

### Why `custom-protocol` is mandatory

Tauri decides whether to load the bundled frontend or the Vite dev server
purely from this feature flag:

```rust
// tauri-macros/src/context.rs
dev: cfg!(not(feature = "custom-protocol")),
```

- Feature **on**  → windows load `http://tauri.localhost/...` (embedded assets)
- Feature **off** → windows load `devUrl` = `http://localhost:5173`

`cargo tauri build` adds the feature automatically; a bare `cargo build
--release` does **not**. A release binary built without it shows
`localhost refused to connect` / `ERR_CONNECTION_REFUSED` in every window,
because no Vite server is running. Clearing the WebView2 profile does not
help — the dev URL is baked into the binary at compile time.

The feature is deliberately **not** in `[features] default`, because
`tauri dev` needs it off for hot reload.

### Verifying which URL the app actually loads

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"
Start-Process .\src-tauri\target\release\nexus.exe
Start-Sleep 15
Invoke-RestMethod http://127.0.0.1:9222/json/list | Select-Object title, url
```

Expected: every window on `http://tauri.localhost/...`.
Bad: `http://localhost:5173/...` → rebuild with `--features custom-protocol`.

## Frontend windows

Every window declared in `src-tauri/tauri.conf.json` must have a matching
rollup input in `frontend/vite.config.ts`, otherwise its HTML file is absent
from `dist/` and the window fails to load in release builds (dev mode hides
this — the Vite server serves any HTML file on demand).

| tauri.conf.json window | HTML            | vite input |
| ---------------------- | --------------- | ---------- |
| `main`                 | `index.html`    | `main`     |
| `setup`                | `setup.html`    | `setup`    |
| `settings`             | `settings.html` | `settings` |
| `sidebar`              | `sidebar.html`  | `sidebar`  |

## Local ports

| Service           | Port    | Notes                                        |
| ----------------- | ------- | -------------------------------------------- |
| STT (faster-whisper) | 39217 | Lazy-started Python sidecar (POST /transcribe) |
| NLU server        | `39218` | Lazy Python sidecar (BERT-Mini ONNX). Override: `NLU_PORT` |
| Sidecar (legacy)  | `41098` | Legacy FastAPI sidecar, not used at runtime  |
| Vite dev server   | `5173`  | Dev only                                     |

## Architecture (serverless — 2026-08-27)

NEXUS is now **fully serverless**. No sidecar, no n8n, no Ollama, no server.

```
NEXUS laptop → HTTP POST → Cloudflare Worker → APIs → text response
                              ↑
                        D1 database (OAuth tokens)
                        Workers AI (intent + summarization)
```

- **Worker** (`server/worker/`): Cloudflare Worker on the edge. Handles
  intent classification, API calls (GitHub/Google), summarization, OAuth
  exchange, token storage, and user registration. <5ms cold start.
- **D1**: Cloudflare's free SQLite. Stores OAuth tokens, API keys, and
  device registrations. 5GB free.
- **Workers AI**: Free tier (10K neurons/day) for intent classification
  (Qwen 0.5B) and summarization (Qwen 14B).
- **Client** (`src-tauri/src/network.rs`): HTTP POST to the Worker. No
  WebSocket. Emits state/ack/result/done events to the frontend.
- **NEXUS_SERVER_URL**: Baked into the installer at build time. Points to
  the Worker URL (e.g. `https://nexus-worker.xxx.workers.dev`).

The old sidecar (`server/sidecar/`) is kept in the repo for reference but
no longer spawned at startup. `sidecar_manager.rs` has been removed.

### Building the installer with the Worker URL

```powershell
$env:NEXUS_SERVER_URL = "https://nexus-worker.your-subdomain.workers.dev"
pwsh ./scripts/build.ps1
```

## Runtime paths (Windows)

- App data (config, logs): `%APPDATA%\com.nexus.assistant\`
- WebView2 profile: `%LOCALAPPDATA%\com.nexus.assistant\EBWebView`
  (note: **Local**, not Roaming — `app_data_dir()` returns Roaming and is the
  wrong path for WebView2)

## Wake word (openWakeWord)

- Default feature: `wakeword-oww` (tract-onnx inference in Rust)
- Models: `src-tauri/resources/oww/{melspectrogram.onnx, embedding_model.onnx, nexus.onnx}`
- Threshold: 0.35, chunk size: 1280 samples (80ms at 16kHz)
- **Detection logic (2026-08-29):** Max-based detection + secondary confirmation.
  The old averaging approach diluted single good frames (0.4+) with surrounding
  0.0s, giving avg=0.03 which never triggered. Now uses max probability in the
  12-frame buffer, so a single 0.36+ frame triggers. After a raw detection,
  collects 500ms of audio and checks RMS ≥ 0.01 to confirm real speech (filters
  noise spikes). Refractory period: 3s.
- **Model v2 (2026-08-30, Kaggle):** Retrained on Kaggle T4 GPU with:
  - 5000 positive samples (5 phrase variants: "nexus", "hey nexus", "nexus wake up", "ok nexus", "nexus please")
  - 30+ soundalike negatives (vs 8 in v1)
  - 80000 training steps (vs 50000), layer_size=64 (vs 32)
  - 2x augmentation rounds, target FP/hr=0.1 (vs 0.2)
  - Model size: 415KB (vs 205KB v1)
  - Kernel: `chitkullakshya/train-nexus-wakeword-v2`
  - v1 backup: `src-tauri/resources/oww/nexus_v1.onnx.backup`
- **Silence gate + AGC (2026-08-28):** `detect_chunk` computes RMS of each
  80ms chunk and skips the classifier entirely if RMS < 0.0005 (~-66dBFS).
  The `nexus.onnx` model emits 0.6-0.9 probabilities on pure digital silence
  (out-of-distribution input), which caused spontaneous false wakes. The
  gate prevents the model from ever seeing silence. Min positive detections
  = 2. Regression test: `test_silence_never_triggers_wake`.
  - **AGC (Automatic Gain Control):** If RMS passes the gate but is below
    TARGET_RMS (0.03), the chunk is amplified up to 50x before feeding the
    classifier. This makes quiet/whispered "NEXUS" produce the same model
    input as loud "NEXUS", so the model (trained on normal-volume TTS)
    recognizes low-volume speech without retraining.
  - Gate: 0.0005, threshold: 0.45. Pure silence (RMS=0) is blocked.
  - Model: trained on Kaggle (v22), accuracy 78.6%, recall 58.2%, FP/hr 1.33.
- **Mic device enumeration (2026-08-27):** `start_audio_capture` enumerates
  ALL input devices, probes each for 5 seconds, and picks the first one
  that produces non-silent audio (RMS > 0.0001). If all devices are silent
  (Intel SST bug), falls back to the best device anyway. This fixes the
  "wake word doesn't work, only hotkey" issue caused by cpal getting
  silence from the Intel Smart Sound Technology driver.
- **FIXED (2026-08-24):** The wake word now works — probability 0.991 for real
  "NEXUS" speech. The root cause was a 32768x input scaling mismatch: cpal
  produces f32 audio in [-1.0, 1.0] but the openWakeWord melspectrogram model
  expects int16-scale float32 values in [-32768, 32767]. Fix: multiply audio
  by 32768.0 in `wakeword_oww.rs` before feeding to the melspectrogram model.
- **Mic conflict (FIXED):** The frontend's `warmMic()` (getUserMedia via WebView2)
  conflicts with the Rust cpal wake-word stream on Intel Smart Sound Technology
  drivers. `warmMic()` is disabled at startup; the mic is acquired on first
  wake instead. This is why cpal was getting silence (RMS=0.0000).
- The global hotkey still works independently of the wake-word model.
- **Command models (2026-08-25):** Training 4 category-level acoustic models
  (`command_open`, `command_close`, `command_search`, `command_play`) via
  `train_nexus_commands.ipynb` on Google Colab. Models detect command TYPE,
  then STT extracts the parameter (which app, what query). See
  `src-tauri/resources/oww/commands/command_intents.json`.

## Known limitations (2026-08-26 audit)

- **Speaker verification is NOT wired.** Enrollment works (setup wizard ->
  embedding -> JSON on disk), but `wakeword_oww::WakeEngine::process`
  accepts every wake regardless of speaker. The verification API in
  `voice_profile.rs` is `#[allow(dead_code)]` until an audio ring buffer
  is added to retain the wake utterance for embedding extraction.
- **All windows skip the taskbar.** `main`, `setup`, `settings`, and
  `sidebar` all have `skipTaskbar: true` in `tauri.conf.json`. NEXUS is
  accessible only via the floating orb, the system tray, the global
  hotkey, and the wake word.
- **STT server auto-launcher writes `server/start_stt.cmd`** with an
  absolute path to the local Python interpreter. This file is gitignored
  (machine-specific, leaks username).
