# 35 — MCP Connect System: Best-of-Combine Build + Round-2 Audit (2026-09-20)

The MCP program's missing half: when a service is disconnected, the
assistant now *requests the connection* instead of just describing the
problem — a full connect surface (state machine, sidebar card, QR
pairing, OAuth login, auto-resume) built by combining the strongest
traits of six industry implementations, then audited against live
production failure reports and hardened again. Verified end-to-end
twice per rule. Research home: `docs/research/mcp-connection/`
(9 files, deep audit in `08-gap-analysis-and-upgrades.md`).

---

## 1. The research base (what "best-of" was combined from)

| Source | What it ships | What we took |
|---|---|---|
| Claude Code `/mcp` (Anthropic) | 4-state health (`Connected / Needs auth / Pending / Failed`); inline Reauthenticate; 401→refresh→retry-once | The 4-state enum semantics + bounded single retry |
| Claude.ai/Desktop connectors | Connector list, Connect buttons; documented failures we designed around | "Fix action where the failure is seen" + never fake green |
| Composio MCP gateway | Agent hands user an OAuth link and **waits** (`COMPOSIO_WAIT_FOR_CONNECTIONS`); task resumes after connect; links regenerate on retry | Auto-resume of the *original failed call* on connect; lazy connect at moment of need |
| Zapier MCP docs | Client-stores-and-refreshes token; header-not-URL; paste-token as legit fallback | Vault design validation + the "tokens never in logs/URLs" rule (now test-pinned) |
| Cursor / VS Code | Green/red status + tool counts + click-to-expand + enable/disable + error→log jump | Foundational for the per-server dashboard shape |
| Sealjay/mcp-whatsapp v0.4.0 | `pairing_status` MCP tool returning a structured `setup_state` envelope (ready / awaiting_qr **with QR payload** / error) "so clients can poll it to drive their own pairing UI" | QR shows **first-party** in our card — no scraping, no console |
| WhatsApp official docs | QR rotates every 20-30s (replay protection); 4-device cap; ~20-day session rotation; 14-day phone-inactivity logout | Card rotation warning; idle probe so the scheduled breakage warns *before* a send fails |
| MCP Authorization spec (2025-11-25 → 2026-07-28) | 401→PRM discovery→AS metadata→registration (metadata-docs first, DCR deprecated)→PKCE S256→`resource`(RFC 8707 MUST)→refresh rotation→step-up union | Swiggy OAuth sequence + the two round-2 MUSTs we were initially missing |

The full option matrices and per-server verdicts (stdout-scrape
rejected, `pairing_status` adopted; spec-OAuth adopted; full-OAuth-
everywhere rejected) are in `07-comparison-verdict.md`.

---

## 2. Phase 1 — shared connect infrastructure (Rust + frontend)

### 2.1 State machine (`src-tauri/src/mcp_client.rs`)
`McpConnectState`: `unknown | down | auth-required | ready` — one truth
for the dashboard, the card, and the voice lines (replacing three
disconnected badges). New command `mcp_connect_state` returns a
`McpConnectCard` per server: `{server, state, note, steps[],
qr_image_uri?, qr_code_text?, pair_url?}`.

- **WhatsApp** fuses transport + `query_pairing_status()` (inner tool
  call — never trips the circuit breaker, never audits). The
  `parse_pairing_state()` parser is case-insensitive ("Status":
  "AWAITING_QR" matches), accepts key variants (`setup_state`/`state`/
  `status` + any `qr` key), and **never fakes green**: an unrecognized
  envelope is `Error`, never `Ready`.
- `normalize_qr_payload()` accepts data-URI images, raw base64
  (assumed PNG ≥100 chars of b64 alphabet), or a plain pairing code —
  exactly one renderable form per call.
- Vault services (Swiggy ×3) fuse reachability + `token_status`;
  401-during-probe = alive-but-login-required (never "down"); dormant
  Amazon is transport-only.
- Numbered, exact-source steps per server (mcp-whatsapp + QR language;
  Swiggy Builders-Club caveat; Amazon sign-in step). **Ready cards carry
  zero steps** — a connected server must never nag (pinned by test).

### 2.2 Failure → card (one shared rule for all three MCP error paths)
Wherever an MCP call fails — the `Subsystem::Mcp` arm, `dispatch_to_mcp`'s
ERR path and its `orchestrator_mcp_confirm` mirror — the flow is now:

1. voice speaks the existing guidance (never removed, still names the
   where and the fix);
2. `open_mcp_connect_card()` opens the Connect card in the response
   sidebar (race-free `PENDING_SIDEBAR` rails) **only on the first
   failure per server per session** (`SHOWN_CONNECT_CARDS` gate — no
   window spam; voice always speaks, the card is the fix surface);
3. the failed call is stashed in `PENDING_MCP_RETRY` (server, tool,
   params — no PII beyond what was already in flight);
4. `spawn_ready_monitor()` polls every 5s (~10min cap) and on `ready`:
   renders the truthful Connected card, retries the stashed call exactly
   once with a fresh vault token, and speaks the merged outcome —
   **without the user re-speaking the command** (the Composio
   WAIT_FOR_CONNECTIONS shape). If a newer user turn starts mid-wait,
   the retry is dropped silently (never speaks over a live turn).

Ordering conventions honored everywhere: `Result` is emitted before
window/side-effects work (TTS never waits on window creation); confirms
already gated writes so the card never bypasses a confirm gate.

### 2.3 Wire-up points
`Subsystem::Mcp` Err arm, `dispatch_to_mcp` ERR return, and
`orchestrator_mcp_confirm` Err path (with the transcript recovered from
the pending payload). Compound-task MCP failures flow through
`dispatch_to_mcp_pub` and merge the same guidance text (no separate
card spam per step).

### 2.4 Tests (all green, twice)
`test_circuit_breaker_opens_and_recovers` (3 fails → 60s cool → clear on
success · pre-existing), 5× `parse_pairing_state` variants (ready /
awaiting_qr+QR / case-insensitive / **garbage-is-Error** /
unavailable) + `normalize_qr_payload` (data-URI, raw-b64, code),
`connect_card_markdown` shape (status + steps + QR/pair link + rotation
+ burner warnings), `server_for_mcp_intent` mapping, retry stash
take-drain roundtrip, `mcp_error_guidance` (Connection-referenced fix
strings; **bridge-assist names mcp-whatsapp + QR scan step**,
breaker line says "cooling down… Recheck"), and
`test_connect_state_live_bridges` — a **live** 5-server probe pinning
shape invariants (the WhatsApp pair URL, ≤5 cards, steps-on-auth-required,
no-nag-on-ready) whatever the user's real bridges do.

---

## 3. Phase 2 — Swiggy spec-OAuth (Worker + vault + frontend)

Reuses the proven Google/GitHub rails (`setup/oauth.ts` PKCE, browser
handoff, `nexus://oauth/` deep link + status polling, Worker-side
storage) — the provider flow proves generic by construction:

- **Worker** (`server/worker/src/index.ts`): SWIGGY auth/token/scopes
  constants (+ SWIGGY secrets in `Env`); `handleAuthUrl` swiggy branch
  (PKCE S256, `access_type=offline`, `prompt=consent`);
  `handleOAuthBrowserCallback` swiggy branch (form-encoded token
  exchange, error-render parity, D1 write with Swiggy scopes);
  `handleOAuthExchange` swiggy branch (cod_verifier — the SPA/PKCE path);
  `refreshSwiggyToken` + `getValidSwiggyToken` (silent refresh, 60s
  buffer, null-on-failure exactly like Google); `GET /oauth/swiggy-token`;
  `/config/check` lists swiggy. Shared completion page + deep link —
  zero new UI surface.
- **Vault** (`auth_vault.rs`): `fetch_swiggy_token_from_worker()` +
  swiggy arm in `refresh_service_token()` — on-device expiry self-heals
  like Google. `resolve_server_token()` then feeds `call_tool` with a
  live token (dispatch pre-flight + 401-clear-and-retry already existed
  and now complete).
- **Frontend**: provider union widened (`"google" | "github" | "swiggy"`
  in `connectOAuth`/`SetupApp`); Connections tab "Login with Swiggy"
  button (`handleSwiggyLogin` → `setSidecarBaseUrl` → `connectOAuth` →
  `vault_status` refresh) and an updated hint ("One OAuth login covers
  all three… pasted token works too").
- **Setup needs (user action)**: `wrangler secret put SWIGGY_CLIENT_ID`
  / `SWIGGY_CLIENT_SECRET` + deploy; production requires Swiggy
  Builders-Club approval (card and guidance both state localhost-dev is
  free).

---

## 3. Phase 3 — long tail + proactive rotation

- **Spotify / Vercel / Render cards**: numbered 3-step instructions + a
  "Get token" button that `shell.open`s the exact provider page
  (window.open fallback) — the Telegram card's exact-source-steps style.
- **Idle rotation probe** (`auth_vault.rs::spawn_monitor`): the 90s
  vault monitor now also probes WhatsApp `pairing_status` (tracked in
 -state transition logs only; still inner-call so no breaker trip, no
  audit) so the ~20-day WhatsApp-side rotation surfaces in Connections
  *before* the user's next send fails.

---

## 4. Round-2 deep audit — live sources broke five things; all fixed

Round-2 sources (all live, September 2026): Anthropic issue trackers
(claude-ai-mcp #24/#35/#430/#744; claude-code #60572/#54649), **Novu PR
#11567** (a shipping connect card), **AutoGPT commit 97a7bc1** (five
production MCP UX bugs), **Qwen OAuth UX PR / QwenPaw OAuth PR**, and
the **2026-07-28 OAuth security-considerations** spec. Full analysis in
`08-gap-analysis-and-upgrades.md`. The five gaps and their fixes:

### 4.1 Card self-update (fresh QR + truthful completion)
WhatsApp's QR rotates every 20-30s — a card rendered once was
unscannable after half a minute (the top real-world "QR will not scan"
support case). **Fix:** `spawn_ready_monitor` now, while the card is
open, re-renders the sidebar card **only when the QR payload actually
changed** (no spam), and on `Ready` renders the Connected card (no stale
"needs login" content — the "truthfulness failure" class in claude-code
#54649). Transcript is passed into the monitor so the request context
line stays accurate across re-renders.

### 4.2 Refresh-token rotation persistence (Worker)
OAuth 2.1 §4.3.1 (public clients MUST rotate): `getValidSwiggyToken` now
persists a rotated `refresh_token` when the server issues one; the
Google path is untouched (Google returns the same token). Without this,
the first server-side rotation would permanently kill the stored
credential.

### 4.3 RFC 8707 `resource` parameter
`SWIGGY_RESOURCE = "https://mcp.swiggy.com"` is now sent on the authorize
URL, both token exchanges, and the refresh — a client MUST in the
2026-07-28 spec (tokens bound to the resource they're for; covers all
three Swiggy endpoints at origin granularity).

### 4.4 Audit-log hygiene, pinned by test
`audit_call`'s line construction extracted to `audit_line()` — the
**single choke point** with a fixed 5-field set
(`ts/server/tool/ok/latency_ms`). New test
`test_audit_line_never_carries_credentials` asserts exactly those keys
and rejects any token/bearer-shaped value in the serialized line — the
Zapier "tokens never in logs" rule is now structurally enforced, not
just promised.

### 4.5 Parallel probes
`mcp_connect_state` previously awaited 5 probes serially (5s timeout
each → 25s worst case for the dashboard). Now `tokio::join!` across all
five: worst case ≈ one probe. (Directly the claude-code#54649 "dialog
hangs while auth is stale" failure class — keep the dashboard from
stalling off one slow server.)

### 4.6 Where our design already beat the live sources
- Novu's two shipped bugs (resume failure overwriting a successful
  Connected with Error; parked sessions with no timeout) — our retry
  failure never downgrades the connect state, and the monitor has a cap.
- AutoGPT's stale-cred shadow bug (sign-in card never fired when a dead
  credential row existed): our pre-flight + 401-clear-retry + card-on-error
  covers the same shape.
- The token-drop-after-OAuth family (#35/#24/#430 — token issued, client
  never re-attaches): our resume monitor *is* the missing re-attach,
  with the fresh token pulled at retry time.

---

## 5. Full research source list (both rounds)

Round 1 (filed as `01`-`06` in `docs/research/mcp-connection/`):
Composio docs (connect + WAIT_FOR_CONNECTIONS), Zapier "How connections
work", Claude family docs + calmara 2026 survey, Cursor/VS Code MCP
management docs, Composio ChatGPT guide, MCP spec 2025-11-25 +
2026-07-28 authorization, sealjay/mcp-whatsapp repo + CLAUDE.md + v0.4.0
release notes, WhatsApp Help Center + 2026 Web/Desktop guides.

Round 2 (filed as `08-gap-analysis-and-upgrades.md`): claude-ai-mcp
#24/#35/#430/#744; claude-code #60572/#54649; novuhq/novu PR #11567
(connect-card lifecycle + resume, incl. their two shipped bugs);
Significant-Gravitas/AutoGPT 97a7bc1 (five MCP UX bugs); QwenLM/
qwen-code PR #2327; agentscope-ai/QwenPaw PR #4256; the 2026-07-28
security-considerations page (RFC 8707 + rotation + PKCE).

---

## 6. Verification record (both rounds)

| Suite | Round 1 | Round 2 |
|---|---|---|
| Rust `cargo check --lib` | clean | clean |
| Rust full lib (serial) | 499/499 | **500/500** |
| mcp_client module | 23/23 (incl. live bridges probe) | 24/24 (audit-hygiene test added) |
| orchestrator module | 46/46 | 46/46 |
| Worker `npm test` + tsc | 49/49 | 49/49 |
| Frontend vitest + tsc | 14/14 | 14/14 |

Known pre-existing (documented, unrelated): orchestrator
`test_install_and_cancel` fails under parallel threads (shared
`ACTIVE_REQUEST` global); serial-only. Known honest gaps (accepted,
listed in `08`): proactive idle refresh for vault services
(rate-limited), "already connected → Reconnect" card variant for future
explicit connect intents, PKCE capability check before proceeding,
Worker refresh single-consumer lock (multi-user future), Amazon managed
launcher (bridge as child process).

---

## 7. Files touched (high-level map)

| Stack | Files |
|---|---|
| Rust | `src-tauri/src/mcp_client.rs` (state machine, pairing parser, QR normalizer, cards, parallel command, audit choke point), `src-tauri/src/orchestrator.rs` (card-on-failure + retry stash + ready monitor), `src-tauri/src/auth_vault.rs` (swiggy refresh + WhatsApp probe), `src-tauri/src/lib.rs` (command registration) |
| Worker | `server/worker/src/index.ts` (swiggy OAuth ×3 handlers, refresh+getValid, `/oauth/swiggy-token`, `/config/check`) |
| Frontend | `frontend/src/setup/oauth.ts` + `SetupApp.tsx` (provider union), `frontend/src/settings-sidebar/SettingsSidebarApp.tsx` (Swiggy login button, per-service Get-token buttons, updated hints) |

Pending user actions: wrangler secrets + Worker deploy (Swiggy),
app rebuild, live click-through of each failure class per server
(failure-class × server matrix in `06-shared-infrastructure.md` §4).
