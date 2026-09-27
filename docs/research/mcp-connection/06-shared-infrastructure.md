# Shared Infrastructure — Tech-Stack Build Plan (2026-09-20)

One system serves all five servers. Three stacks: Rust backend, frontend,
Cloudflare Worker.

---

## 1. Rust (`src-tauri/src/`)

- **Connect-state machine** (`mcp_client.rs`, beside `mcp_status`
  `:574-649`): `unknown | down | auth-required | ready` per server, from
  probe + `token_status` (+ `pairing_status` for WhatsApp). One command
  (`mcp_connect_state`). Per-server Recheck replaces the shared button.
- **First-failure gate:** in-memory set; MCP `Err` opens the Connect card
  only on first failure per server per session (no window spam). Voice
  line always speaks; card is the fix surface.
- **Resume-on-complete:** completion (QR `ready` / OAuth callback)
  auto-retries the original failed call once, then speaks the result —
  no re-speaking by the user. Shape matches `PENDING_COMPOUND` resume.
- **Refresh + probing:** extend `refresh_service_token` to Swiggy;
  idle monitor probes WhatsApp `pairing_status` (≤1/min; 5s only while
  its card is open — `/pair/*` rate limits) for rotation early-warning.
- **Single-writer refresh** (Claude-issue lesson): vault locks already
  serialize; keep exactly one refresher (monitor), never per-call.
- **Audit hygiene:** `mcp_audit.jsonl` fields stay
  server/tool/ok/latency (`mcp_client.rs:261-265`) — add a test
  asserting no token/secret-shaped value is ever written.

## 2. Frontend (`frontend/src/`)

- **Connect card** in the response sidebar: `{server, state, steps[],
  qrImage?, actionButton?, logView?}` on the race-free
  `PENDING_SIDEBAR` path (`commands.rs:764-814`) — the same rails as the
  PR-analysis dashboard.
- **Connections tab** (`settings-sidebar/SettingsSidebarApp.tsx:641-848`)
  upgraded to Cursor standard: per-server toggle, tool counts from
  `tools/list`, one-click log view, per-server login/connect button.
  Existing badges + hints stay.
- QR render: standard QR-image component from the `pairing_status`
  payload (no scraping, no console windows).

## 3. Worker (`server/worker/src/`)

- Mirror Google's OAuth endpoints for Swiggy: auth-URL generation
  (with metadata discovery + PKCE), token exchange, refresh endpoint.
- Host the Client ID Metadata Document (preferred registration path).
- Quota noting: OAuth token ops are cheap; no quota change needed, but
  log them like other auth ops.

## 4. Tests (each phase, twice)

- State transitions per server × failure class (down / expired /
  never-configured).
- Guidance strings keep naming where + action (extend
  `orchestrator.rs:2742-2769`).
- OAuth: mock-Worker URL → callback → vault live → silent refresh.
- WhatsApp: `awaiting_qr` renders; `ready` → confirm + single retry;
  rotation probe flips state with no command in flight.
- Audit log never contains secrets.
- Live: one run per server per failure class before the next phase.
