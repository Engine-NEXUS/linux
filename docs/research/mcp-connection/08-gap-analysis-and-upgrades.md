# Gap Analysis — Deep Audit vs Live Industry Sources (2026-09-20)

Round-2 research: production failure reports (Anthropic issue trackers),
shipping implementations (Novu connect-card PR, AutoGPT MCP fix commit,
QwenPaw OAuth PR, qwen-code OAuth UX PR), and the 2026-07-28 OAuth 2.1
security spec. Each source was compared line-by-line against our
implementation; every gap found is either fixed below or listed as
known-and-accepted.

**All five fixes were implemented and verified twice the same day**
(Rust 500/500 serial, Worker 49/49 ×2 + tsc, frontend 14/14 ×2 + tsc).

---

## 1. What the live sources taught (and what they broke)

### 1.1 Anthropic issue trackers — production failure catalog

| Issue | Failure mode | Our status |
|---|---|---|
| claude-ai-mcp#744 | Revoked token (401 + `invalid_grant`) surfaced as "server isn't responding" — sent users debugging the wrong side | **Compliant**: `mcp_error_guidance` splits auth vs unreachable; auth failures always name the reconnect path, never blamed on the server |
| claude-code#54649 | "Truthfulness failure": dialog showed `Connected` while auth was stale + hung on lazy refresh | **Compliant**: 4-state machine separates reachable/authenticated/ready; auth-required never renders as ready; probes capped at 5s (no hang) |
| claude-code#60572 | Refresh token never used proactively → mid-session random disconnects | **Partially**: refresh happens on use (Worker-side, 60s buffer) + idle monitor probes status; a true proactive idle refresh is listed as accepted follow-up (§5) |
| claude-ai-mcp#430/#35/#24 | OAuth completes, client never re-attaches token → "Disconnected" forever | **Compliant by design**: our resume monitor retries the original call with the fresh token exactly once on Ready — the failure these issues describe is the failure our `PENDING_MCP_RETRY` closes |

### 1.2 Novu connect-card PR #11567 — the closest shipping analog

| Their design | Our status |
|---|---|
| Platform-native Connect card with Connect / Connect & auto-approve | Card exists (sidebar markdown); auto-approve variant not applicable (we already gate writes via confirm) |
| **Deletes card after OAuth; suppresses intermediate reply; resumes original request via follow-up** | **Was the gap — fixed**: monitor now re-renders the card (fresh QR + Connected state) and the original call auto-retries without re-speaking |
| Card metadata persisted so callback can locate/delete it | We hold `PENDING_MCP_RETRY` + `SHOWN_CONNECT_CARDS` in-process; single-session product makes this sufficient |
| **Their bug**: resume failure overwrote a successful `Connected` with `Error` → unrecoverable | **Avoided**: our retry failure renders "reachable now, but retry failed — card still open" and never downgrades the connect state itself |
| **Their bug**: no timeout on parked session → conversation lost forever | **Compliant**: ~10min monitor cap, then card stays for manual Recheck |

### 1.3 AutoGPT commit 97a7bc1 — five shipped-bug lessons

| Their bug | Our status |
|---|---|
| Stale-cred row shadowed the sign-in card (card only fired when `not creds`) | **Compliant**: pre-flight catches missing; 401-clear-and-retry + card-on-error covers dead-but-present — the exact case they fixed |
| OAuth popup close race → success reported as "window closed" | **Not exposed**: we use deep-link + status polling, not popup-closed detection |
| **No "already connected" UX** | **Gap noted, accepted**: our Ready card shows Connected/no-nag; an explicit "connect to X" voice intent that renders a Ready card with a Reconnect option is a small follow-up |
| Connected/Reconnect render branch | Matches our Ready-state card shape |

### 1.4 OAuth 2.1 security spec (2026-07-28) — normative MUSTs

| Requirement | Our status |
|---|---|
| PKCE S256 mandatory, verify `code_challenge_methods_supported` | **Compliant** (S256; capability check is a listed follow-up for strict-Swiggy hardening) |
| **RFC 8707 `resource` parameter is a client MUST** | **Was the gap — fixed**: `SWIGGY_RESOURCE` now sent on authorize + both token exchanges + refresh |
| **Refresh token rotation (public clients MUST)** | **Was the gap — fixed**: `getValidSwiggyToken` persists a rotated `refresh_token` when the server issues one (Google untouched — it returns the same token) |
| Tokens never in logs | **Was unpinned — fixed**: `audit_line()` is now the single choke point with a FIXED field set; `test_audit_line_never_carries_credentials` asserts no token/bearer/authorization value can appear |
| Single-consumer refresh locking (double-refresh consumes rotated tokens) | Accepted risk: single-user product, Worker refreshes serialize per request; two simultaneous `/oauth/swiggy-token` fetches can race — noted for multi-user future |

### 1.5 QwenPaw PR #4256 / qwen-code PR #2327 — supporting patterns

- 401 fast-fail (no 30s hang): our circuit breaker + `breaker_check` fast-fail matches.
- OAuth button on all HTTP clients by default: our card + per-server state matches.
- Token values never in API list responses: `vault_status` returns status strings only — compliant.
- Post-auth feedback (tool count + completion message): our Ready card + "X is connected, sir" speech matches; tool-count on the Ready card is cosmetic follow-up.
- "Clear Authentication" (delete tokens AND disconnect): our Delete button clears the vault token — matches.

### 1.6 WhatsApp official docs — pairing realities

- Up to 4 linked devices; **QR refreshes every 20-30s (replay protection)**; biometric gate on the phone before linking; phone unused >14 days logs sessions out.
- **This validated the biggest fix**: our card embedded a *static* QR — after ~30s it was expired and unscannable (exactly the "QR will not scan" top support issue). **Fixed**: the monitor re-renders the card whenever the bridge's `pairing_status` payload changes, so the QR is always live while the card is open.

---

## 2. Fixes applied this round (all verified ×2)

1. **Card self-update** (`orchestrator.rs::spawn_ready_monitor`): polls every 5s; re-renders sidebar card only when the QR payload actually changes (no sidebar spam); on Ready renders the Connected card (truthful state) before retrying.
2. **Refresh-token rotation persistence** (`server/worker/src/index.ts::getValidSwiggyToken`): rotated refresh tokens are stored; Google path untouched.
3. **RFC 8707 `resource`** (`SWIGGY_RESOURCE`): authorize URL + browser-callback exchange + direct exchange + refresh all carry it.
4. **Audit hygiene pinned** (`mcp_client.rs::audit_line` + test): fixed 5-field set, no credential-shaped value possible.
5. **Parallel probes** (`mcp_connect_state`): `tokio::join!` across all 5 servers — dashboard worst case drops from ~25s to ~5s.

## 3. Verdict after the audit

Our best-of-combine build now matches or exceeds the live industry
implementations on every axis the sources tested: failure-class mapping
(#744), truthful state reporting (#54649), token-drop recovery (#35/#24/
#430 — our resume monitor is the fix they lack), card lifecycle (Novu),
stale-cred card firing (AutoGPT), and the normative OAuth 2.1 MUSTs.

**Known-and-accepted gaps** (each small, listed for honesty):
- Proactive idle refresh (vs on-use refresh) for vault services.
- "Already connected → Reconnect" card variant for explicit connect intents.
- PKCE capability check (`code_challenge_methods_supported`) before proceeding.
- Worker refresh single-consumer locking (multi-user future).
- Amazon managed launcher (bridge as child process) still pending.

## 4. Sources (round 2)

- github.com/anthropics/claude-ai-mcp: #24, #35, #430, #744 (failure taxonomy)
- github.com/anthropics/claude-code: #60572, #54649 (refresh + truthfulness)
- github.com/novuhq/novu PR #11567 (connect card lifecycle + resume, incl. their two shipped bugs)
- github.com/Significant-Gravitas/AutoGPT commit 97a7bc1 (five MCP UX bugs, stale-cred lesson)
- github.com/agentscope-ai/QwenPaw PR #4256, QwenLM/qwen-code PR #2327 (OAuth UX patterns)
- modelcontextprotocol.io 2026-07-28 `authorization/security-considerations` (RFC 8707, rotation, PKCE)
- FAQ WhatsApp Help Center + 2026 Web/Desktop guides (4-device cap, QR 20-30s rotation, 14-day rule)
