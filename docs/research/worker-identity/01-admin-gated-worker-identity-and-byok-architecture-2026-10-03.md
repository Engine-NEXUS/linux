# Admin-Gated Worker Identity & BYOK Architecture

**Date:** 2026-10-03
**Status:** RESEARCH + APPROVED PLAN (implementation pending user go-ahead)
**Scope:** Cloudflare Worker + D1 identity model, admin-gated Workers AI access, per-laptop profile isolation, BYOK (Bring-Your-Own-Keys) enforcement, client identity lifecycle migration
**Binding user directives (2026-10-03):**

1. "cloudflare worker ai will create a profile whenever user authenticates" — profile creation is Worker-owned, not client-generated.
2. "only i have the access to give the worker ai access to users as the admin" — default-deny; admin approves every profile/device.
3. "all user worker database only stores the user id device id" — D1 stores identity/entitlement only. No chat history, no transcripts, no user provider keys.
4. "for all extra they add their own gemini groq api keys" — BYOK stays local to each laptop.
5. "cloudflare worker ai access only for the people i choose" — entitlement list controlled by admin.

---

## Table of Contents

1. [Terminology & Concepts](#1-terminology--concepts)
2. [Current State Audit (verified 2x)](#2-current-state-audit-verified-2x)
3. [Gap Analysis](#3-gap-analysis)
4. [Target Architecture](#4-target-architecture)
5. [D1 Data Model](#5-d1-data-model)
6. [Worker API Surface (new + changed)](#6-worker-api-surface-new--changed)
7. [The AI Entitlement Gate](#7-the-ai-entitlement-gate)
8. [Admin Control Plane](#8-admin-control-plane)
9. [Client Identity Lifecycle](#9-client-identity-lifecycle)
10. [BYOK Enforcement](#10-byok-enforcement)
11. [OAuth Decision (Option A adopted)](#11-oauth-decision-option-a-adopted)
12. [Migration Plan](#12-migration-plan)
13. [Security Analysis](#13-security-analysis)
14. [Failure Modes](#14-failure-modes)
15. [Testing Matrix](#15-testing-matrix)
16. [Rollout Plan](#16-rollout-plan)
17. [Open Questions (resolved)](#17-open-questions-resolved)

---

## 1. Terminology & Concepts

| Term | Definition |
|------|-----------|
| **Workers AI** | Cloudflare's serverless model inference (`env.AI.run(...)`). Stateless — stores nothing. |
| **Worker (backend)** | `server/worker/src/index.ts` — the deployed Cloudflare Worker that routes requests, holds D1 bindings, and calls Workers AI. |
| **D1** | Cloudflare SQLite (`nexus-db`). The only durable cloud storage in the system. |
| **Profile** | One canonical cloud identity per laptop installation. Worker-issued `profile_id`. Status: `pending → approved → (suspended|revoked|expired)`. |
| **Device** | One laptop bound to a profile. Worker-issued `device_id`. |
| **BYOK** | Bring-Your-Own-Keys. User's Groq/Gemini/Cerebras/etc. API keys stored ONLY in the laptop's OS credential store. Never uploaded. |
| **Entitlement** | Admin-granted permission for a profile/device to consume Workers AI (+ quota tier + expiry). |
| **Device token** | Secret issued at claim time; sent by client as bearer credential. Stored in D1 only as SHA-256 hash. |
| **Provisional install ID** | Client-generated UUID before first successful claim. Never authoritative. |
| **Claim** | The handshake where a new install registers with the Worker and receives canonical identity. |

### What Workers AI is NOT

Workers AI is model inference only. It has no database, no profile storage, no session state. The user's phrase "profile saved in workerai" is implemented as: **the Worker backend + D1 stores the profile; the Worker calls Workers AI for inference only when entitlement passes.** This distinction is critical to the design below.

---

## 2. Current State Audit (verified 2x)

Every fact below was verified twice via direct code reads on 2026-10-03.

### 2.1 Client identity generation (exists, client-owned today)

`src-tauri/src/lib.rs:795-826` — on first launch (no `nexus-config.json`):

```rust
let user_id = format!("user_{}", network::uuid_v4());
let device_id = format!("device_{}", network::uuid_v4());
let server_url = option_env!("NEXUS_SERVER_URL")
    .unwrap_or("https://nexus-worker.chitkullakshya.workers.dev");
// writes nexus-config.json { serverUrl, userId, deviceId }
```

- Client generates its own identity. Worker never sees it until first transcript.
- `uuid_v4()` at `network.rs:83-109` — hand-rolled RFC 4122 v4 from nanos+pid+counter.
- `open_session_from_config` (`network.rs:142-157`) auto-opens the session at startup.
- `save_server_config` (`commands.rs:107-124`) preserves existing identity (never overwrite with empty).

### 2.2 Session + transcript flow

`network.rs:72-167` — `Session { worker_url, user_id, device_id, cancelled }` in static `Arc<Mutex<Option<Session>>>`.

`network.rs:169-296` — `send_transcript` POSTs:

```json
{
  "request_id": "<uuid>",
  "requester": { "id": "user_xxx", "device_id": "device_xxx" },
  "task": { "type": "general", "request": "<text>" }
}
```

### 2.3 Worker route surface (15 routes, all verified)

`server/worker/src/index.ts` fetch handler (lines ~1833-1971):

| Route | Method | Uses Workers AI? | Uses requester.id? |
|-------|--------|------------------|--------------------|
| `/health` | GET | No | No |
| `/oauth/auth-url` | GET | No | Yes (state param) |
| `/oauth/callback` | GET | No | Yes |
| `/oauth/exchange` | POST | No | Yes |
| `/oauth/status` | GET | No | Yes |
| `/oauth/github-token` | GET | No | Yes |
| `/oauth/google-token` | GET | No | Yes |
| `/oauth/swiggy-token` | GET | No | Yes |
| `/oauth/disconnect` | DELETE | No | Yes |
| `/apikeys/add` | POST | No | Yes — **to deprecate** |
| `/apikeys/remove` | DELETE | No | Yes — **to deprecate** |
| `/apikeys/list` | GET | No | Yes — **to deprecate** |
| `/models/nlu/latest` | GET | No | No (admin-gated publish) |
| `/models/nlu/download` | GET | No | No |
| `/models/nlu/publish` | POST | No | Admin token gated |
| `/` (transcript) | POST | **Yes** | Yes |

### 2.4 Workers AI call sites (7, all verified)

| # | Location | Purpose | Model |
|---|----------|---------|-------|
| 1 | `index.ts:153` | `classifyIntent` | `@cf/meta/llama-3.2-1b-instruct` |
| 2 | `index.ts:259` | `summarize` | `@cf/mistral/mistral-small-3.1-24b-instruct` (fallback `llama-3.2-3b`) |
| 3 | `index.ts:1403` | (analysis path) | summary model |
| 4 | `index.ts:1581` | (analysis path) | summary model |
| 5 | `index.ts:3081` | fast-analyse summary | `SUMMARY_MODEL` |
| 6 | `index.ts:3233` | repo analysis spoken summary | `SUMMARY_MODEL` |
| 7 | `external_llm.ts:134` | `callExternalAI` Workers AI arm | configurable |

Plus external free providers (`callGroq`, `callGemini` in `external_llm.ts`) — these consume the *user's own keys*, not Workers AI neurons, but still route through the Worker, so the entitlement gate must cover them too (per directive: Worker access itself is admin-gated).

### 2.5 D1 schema (current)

`server/worker/schema.sql`:

```sql
oauth_tokens (user_id, provider, access_token, refresh_token, expires_at, scopes, account_id, created_at)
api_keys     (user_id, provider, key_encrypted, created_at)      -- BYOK → deprecate
user_devices (user_id, device_id, device_name, os, device_token, created_at)  -- UNUSED (no writer)
usage_log    (user_id, day_utc, requests, ai_neurons, d1_reads, d1_writes, search_calls, deep_calls)
cache_entries(cache_key, cache_value, expires_at, created_at)
```

- `user_devices` exists in schema but **zero code paths read or write it** (verified by grep).
- `api_keys` stores base64-obfuscated (not encrypted) user provider keys — **violates BYOK directive**.

### 2.6 Quota system

`server/worker/src/quota.ts` — per-user daily limits keyed by `user_id` in `usage_log`:

```
requests_per_day: 150, ai_neurons_per_day: 1200, deep_calls_per_day: 15, search_calls_per_day: 50
GLOBAL_NEURON_WARN = 8000, GLOBAL_NEURON_HARD = 9500
```

### 2.7 Admin precedent

`Env.NEXUS_ADMIN_TOKEN` already exists and gates `POST /models/nlu/publish` (index.ts:1943). The admin plane below reuses this credential pattern with a distinct token.

### 2.8 Client local secret storage

`src-tauri/src/auth_vault.rs` — `keyring` crate, service `com.nexus.assistant`, keys `nexus:<service>`. Tokens live in Windows Credential Manager. Payload format `"<token>|<expires_at_unix>"`. This is where the device token and all BYOK keys belong.

### 2.9 Setup wizard flow

`frontend/src/setup/SetupApp.tsx` — 4 steps: Persona & Voice → Permissions → Preferences → Accounts. Step 3 (Accounts) does OAuth connects via `oauth.ts::connectOAuth`. `get_server_config` returns `{serverUrl, userId, deviceId}` — currently the client-generated identity. **This is the natural hook point for the claim handshake.**

---

## 3. Gap Analysis

| # | Requirement | Current | Gap |
|---|------------|---------|-----|
| G1 | Worker creates profile on auth | Client generates UUIDs locally | No claim endpoint; Worker never confirms identity |
| G2 | Admin gates Workers AI | Any caller with network access can consume neurons via POST / | No entitlement check anywhere |
| G3 | D1 stores IDs only | `api_keys` stores user provider keys | BYOK violation |
| G4 | Per-laptop isolation | All cloud state keyed by client-chosen `user_id`; a cloned config = shared identity | Canonical Worker-issued IDs + device tokens |
| G5 | Chat history local-only | Worker doesn't store history (good) — keep it that way | Enforce with tests; never add history tables |
| G6 | Users BYOK | Keys stored locally AND centrally via /apikeys | Remove central path, keep local vault only |
| G7 | Revocation | Nothing to revoke — no registry | profiles/devices/entitlements tables + admin ops |
| G8 | Duplicate-profile safety | Retries/reinstalls create fresh UUIDs silently | Idempotent claim with provisional-ID mapping |

---

## 4. Target Architecture

```text
                    ┌────────────────────────────────────────────┐
                    │              ADMIN (you)                    │
                    │  · reviews pending profiles                 │
                    │  · approves / suspends / revokes            │
                    │  · sets quota tier + expiry                 │
                    └──────────────┬─────────────────────────────┘
                                   │ Bearer NEXUS_ADMIN_TOKEN
                                   ▼
┌──────────────┐  claim   ┌─────────────────────┐   inference   ┌────────────┐
│  LAPTOP (A)  │─────────▶│  Cloudflare Worker   │──────────────▶│ Workers AI │
│  profile_a   │  device  │  (entitlement gate)  │  only if ok   │ (stateless)│
│  device_a    │  token   │                      │               └────────────┘
│  BYOK local  │◀─────────│        D1            │
└──────────────┘  reply   │  profiles            │        ┌────────────┐
                          │  devices             │        │ External   │
┌──────────────┐          │  entitlements        │───────▶│ LLM (user's│
│  LAPTOP (B)  │ (same)   │  profile_events      │  BYOK  │ own keys)  │
│  profile_b   │          │  usage_log           │        └────────────┘
└──────────────┘          └─────────────────────┘
                          NO: chat, transcripts, user keys
```

### Core rules

| Rule | Statement |
|------|-----------|
| R1 | **Default-deny.** Unknown / pending / suspended / revoked / expired ⇒ no Workers AI, no external-LLM relay, no quota consumption. |
| R2 | **Worker issues identity.** `profile_id`, `device_id`, device token all Worker-generated. Client UUIDs are provisional hints only. |
| R3 | **D1 stores identity + entitlement + operational counters only.** Never: chat content, transcripts, provider API keys, TTS text, memory. |
| R4 | **BYOK stays local.** Groq/Gemini/Cerebras keys live in the laptop OS credential store. Client sends them only as ephemeral request headers/body to the Worker at request time if a given feature requires server-side relay (or keeps them entirely client-side where the flow allows). |
| R5 | **Fail closed.** Any entitlement lookup error denies AI. |
| R6 | **History is local.** No cloud chat-history tables, now or ever, without a new signed directive. |
| R7 | **Admin is sole approver.** Nobody self-serves into Workers AI access. |

---

## 5. D1 Data Model

New tables (additive; existing tables untouched except `api_keys` deprecation):

```sql
-- Canonical laptop profile (one per installation)
CREATE TABLE IF NOT EXISTS profiles (
  profile_id       TEXT PRIMARY KEY,        -- "prof_" + 22-char base64url (128-bit)
  human_label      TEXT,                    -- optional admin-set name ("Lakshya-laptop")
  status           TEXT NOT NULL,           -- pending|approved|suspended|revoked
  quota_tier       TEXT,                    -- "default" | "premium" | "restricted"
  created_at       REAL NOT NULL,
  approved_at      REAL,
  approved_by      TEXT,                    -- admin token id (never the token)
  expires_at       REAL,                    -- null = no expiry
  last_seen_at     REAL,
  provision_hint   TEXT                     -- client's provisional user_xxx (audit only)
);

-- Devices bound to a profile
CREATE TABLE IF NOT EXISTS devices (
  device_id        TEXT PRIMARY KEY,        -- "dev_" + 22-char base64url
  profile_id       TEXT NOT NULL REFERENCES profiles(profile_id),
  device_token_hash TEXT NOT NULL,          -- SHA-256 hex of device token
  device_name      TEXT,
  os               TEXT,
  app_version      TEXT,
  status           TEXT NOT NULL,           -- active|revoked
  created_at       REAL NOT NULL,
  last_seen_at     REAL,
  revoked_at       REAL
);

-- Admin grant (separable so entitlement ops are auditable independently)
CREATE TABLE IF NOT EXISTS entitlements (
  profile_id       TEXT NOT NULL REFERENCES profiles(profile_id),
  scope            TEXT NOT NULL,           -- "worker_ai" | "llm_relay" | "all"
  allowed          INTEGER NOT NULL,        -- 1/0
  quota_tier       TEXT,
  granted_by       TEXT NOT NULL,
  granted_at       REAL NOT NULL,
  expires_at       REAL,
  PRIMARY KEY (profile_id, scope)
);

-- Append-only audit trail
CREATE TABLE IF NOT EXISTS profile_events (
  id               INTEGER PRIMARY KEY AUTOINCREMENT,
  profile_id       TEXT NOT NULL,
  device_id        TEXT,
  event            TEXT NOT NULL,           -- registered|approved|suspended|revoked|expired|token_rotated|denied|recovery
  detail           TEXT,                    -- metadata only (admin notes, reason codes)
  at               REAL NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_devices_profile ON devices(profile_id);
CREATE INDEX IF NOT EXISTS idx_events_profile  ON profile_events(profile_id);
CREATE INDEX IF NOT EXISTS idx_events_at       ON profile_events(at);
```

### Storage ledger (what D1 holds vs. not)

| Data | In D1? | Where instead |
|------|--------|---------------|
| profile_id / device_id | ✅ | — |
| status / entitlement / quota tier | ✅ | — |
| device token | hash only | raw token on laptop (keyring) |
| usage counters (counts only) | ✅ | — |
| OAuth access/refresh tokens | ✅ (Option A, §11) | — |
| **Chat / transcripts** | ❌ never | laptop conversation ledger (`conversation.rs`) |
| **Gemini/Groq/Cerebras keys** | ❌ never | laptop keyring (`auth_vault.rs`) |
| Memory / remembers | ❌ never | laptop `memory.rs` |
| Diary | ❌ never | laptop `diary.rs` |

---

## 6. Worker API Surface (new + changed)

### 6.1 New client endpoints (v1)

#### `POST /v1/profiles/claim`

Idempotent registration handshake.

Request:

```json
{
  "provisional_user_id": "user_abc123",     // optional client hint
  "provisional_device_id": "device_xyz",    // optional client hint
  "device_name": "Lakshya-XPS",
  "os": "windows",
  "app_version": "0.1.0"
}
```

Response (new install):

```json
{
  "profile_id": "prof_Kx7...bA",
  "device_id": "dev_Qm3...zF",
  "device_token": "<one-time plaintext, only here>",
  "status": "pending",
  "protocol_version": "1"
}
```

Idempotency: client may re-send with the SAME `(profile_id, device_id)` + valid device token → returns same profile (rotated token if `rotate:true`). A retry WITHOUT credentials creates a *new* profile only if the client explicitly passes `reinstall:true`; otherwise returns the existing pending/approved profile matched by provisional hints.

Rate limit: per-IP token bucket (e.g. 5 claims/hour/IP) via `cache_entries`-based counter or KV.

#### `GET /v1/profiles/me`

Bearer: device token. Returns `{ profile_id, device_id, status, entitlements, quota_tier, expires_at }`.

#### `POST /v1/devices/rotate-token`

Bearer: current device token. Issues new token (hash swapped), logs `token_rotated`.

#### `DELETE /v1/devices/current`

Bearer: device token. Self-revoke (reinstall path). Logs `revoked`.

#### `POST /v1/profiles/recover` (Phase 2 — optional)

Recovery/relink flow for "I reinstalled and lost my profile": requires admin approval as a second factor. Out of Phase 1 scope; reserved in the API design.

### 6.2 Changed existing endpoint

#### `POST /` (transcript) — entitlement gate inserted

Resolution order (before ANY AI/provider call):

1. Parse `requester.id` + `device_id`.
2. If `requester.profile_id` present (new clients) → canonical lookup.
3. Else legacy `user_id` → migration lookup table (§12) with `MIGRATION_MODE` behavior.
4. Look up `devices` row + hash-verify bearer token (new clients).
5. Evaluate entitlement (§7).
6. Denied → `403 { "error": "worker_ai_not_enabled", "status": "<profile status>" }` — **zero AI calls, zero usage increments**.
7. Approved → quota check → proceed as today.

### 6.3 Unchanged

- `/oauth/*` — unchanged (still keyed by profile after migration).
- `/models/nlu/*` — unchanged (family OTA model distribution; not AI inference).
- `/health` — extended with `{ "identity": "v1", "entitlement": true }` additive fields.

---

## 7. The AI Entitlement Gate

Single choke function (new module `server/worker/src/entitlement.ts`):

```ts
export type EntitlementResult =
  | { ok: true;  profileId: string; deviceId: string; quotaTier: string }
  | { ok: false; code: "unknown_profile" | "pending" | "suspended" | "revoked"
              | "expired" | "bad_token" | "lookup_failed"; status?: string };

export async function gateAI(req, env): Promise<EntitlementResult>
```

### Call-site coverage (all 7 AI sites + relays)

The gate runs ONCE at the top of `handleTranscript` — but defense-in-depth requires `classifyIntent`, `summarize`, and `external_llm` to accept an already-validated `EntitlementContext` parameter, making it a type error to call them without one (compile-time enforcement).CI grep-gate (`check-ai-gate.mjs`, following the `check-turn-ends.mjs` precedent) fails the build if any `env.AI.run` appears outside the guarded modules.

### Decision table

| Profile state | Device token | Entitlement | Result |
|---------------|-------------|-------------|--------|
| missing | — | — | `unknown_profile` (403) |
| pending | valid | — | `pending` (403, client shows "awaiting approval") |
| approved | valid | allowed=1, not expired | ✅ proceed |
| approved | valid | allowed=0 | deny |
| approved | invalid/missing | — | `bad_token` (401) |
| suspended | valid | — | `suspended` |
| revoked | valid | — | `revoked` |
| approved | valid | allowed=1 but `expires_at < now` | `expired` |
| any | D1 error | — | `lookup_failed` (fail closed) |

### Denial contract

Denials MUST NOT: call `env.AI.run`, call Groq/Gemini, increment `usage_log`, or write transcript text anywhere. Denials MAY: write a `profile_events` row (`event:"denied"`, no content) and rate-limit repeat offenders.

---

## 8. Admin Control Plane

### 8.1 Auth

- New secret `NEXUS_ADMIN_IDENTITY_TOKEN` (separate from `NEXUS_ADMIN_TOKEN` used by model publishing — rotation isolation).
- Sent as `Authorization: Bearer <token>` on all `/v1/admin/*` routes.
- Compared via timing-safe compare; never logged; never echoed.
- Rotation: set new secret in wrangler; old token invalid immediately.

### 8.2 Admin endpoints (Phase 1 = CLI/curl only; no UI)

| Route | Method | Body/Query | Effect |
|-------|--------|-----------|--------|
| `/v1/admin/profiles/pending` | GET | — | List pending profiles+devices (IDs, device names, requested_at) |
| `/v1/admin/profiles/list` | GET | `?status=` | All profiles |
| `/v1/admin/profiles/approve` | POST | `{profile_id, quota_tier?, expires_at?, note?}` | status→approved + entitlement row |
| `/v1/admin/profiles/suspend` | POST | `{profile_id, note?}` | status→suspended (reversible by approve) |
| `/v1/admin/profiles/revoke` | POST | `{profile_id, note?}` | status→revoked (terminal; devices invalidated) |
| `/v1/admin/devices/revoke` | POST | `{device_id, note?}` | Single device revoke |
| `/v1/admin/entitlements/set` | POST | `{profile_id, scope, allowed, expires_at?}` | Granular scope toggle |
| `/v1/admin/profiles/:id/events` | GET | — | Audit trail read |

All admin writes append `profile_events`.

### 8.3 Admin operational runbook (summary)

```bash
# see who's waiting
curl -s -H "Authorization: Bearer $ADMIN_TOKEN" \
  "$WORKER/v1/admin/profiles/pending" | jq

# approve a friend's laptop, 90-day grant
curl -s -X POST -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"profile_id":"prof_Kx7...","quota_tier":"default","expires_at":1790000000}' \
  "$WORKER/v1/admin/profiles/approve"

# kick a device instantly
curl -s -X POST ... "$WORKER/v1/admin/profiles/revoke"
```

---

## 9. Client Identity Lifecycle

### 9.1 State machine

```text
FRESH_INSTALL
   │ generate provisional user/device UUID (local hint only)
   ▼
SETUP (steps 0-3 as today)
   │ at Accounts step (or final "Finish"):
   ▼
CLAIM ── offline? ──▶ LOCAL_ONLY (queue claim; retry every 10 min; never re-UUID)
   │ 200 claim response
   ▼
PENDING ── stored: profile_id+device_id in nexus-config.json
   │           device_token → keyring("nexus:device_token")
   │ user sees: "Waiting for admin approval" (orb speaks + setup banner)
   ▼ (admin approves; client polls /v1/profiles/me every 10 min, or on next wake)
APPROVED ── full cloud features
   │
   ├─ suspended/revoked/expired (any later /me or 403)
   ▼
DENIED_STATE ── cloud off; local-only features (wake, STT local, offline commands,
                 local conversation memory) keep working; clear spoken/visual reason
```

### 9.2 Storage changes

| Item | File | New rule |
|------|------|----------|
| `profile_id`, `device_id` | `nexus-config.json` | Worker-issued canonical values replace provisional after claim |
| device token | keyring `nexus:device_token` | NEVER in plaintext config |
| legacy `user_id` | kept as `legacy_user_id` for migration window | read-only |

### 9.3 Code touch-points (implementation targets)

| File | Change |
|------|--------|
| `src-tauri/src/lib.rs:795-826` | First-launch still creates provisional IDs (kept), but flags config `identity: "provisional"` |
| `src-tauri/src/network.rs` | `open_session*` gains claim-awareness; `send_transcript` payload adds `requester.profile_id/device_token_header` |
| `src-tauri/src/commands.rs` | New `claim_profile` command wrapping the POST; `get_identity_status` IPC for frontend |
| `src-tauri/src/auth_vault.rs` | Reuse for `device_token` storage |
| `frontend/src/setup/SetupApp.tsx` | Accounts step: call claim; render pending/approved/denied banner |
| `frontend/src/settings-sidebar/SettingsSidebarApp.tsx` | Identity card (profile state, device name, revoke button) |
| `frontend/src/net/orchestrator.ts` | Surface `worker_ai_not_enabled` as distinct spoken line ("Cloud access isn't enabled for this device yet, sir.") |

### 9.4 Offline behavior

- Claim failure (no network) → stay provisional, keep `pending_claim` flag, retry with backoff (10 min).
- Approved-then-offline → everything works that doesn't need the Worker (local STT, offline commands, local memory).
- NEVER generate a second provisional UUID on retry (G8 fix: provisional IDs persist until claim succeeds).

### 9.5 Reinstall

Wiped laptop = new provisional install → new claim → new `pending` profile. Recovery of the OLD profile only via `POST /v1/profiles/recover` + admin approval (Phase 2). Default: fresh profile, fresh BYOK entry, fresh local history. Nothing about the old laptop leaks into the new one.

---

## 10. BYOK Enforcement

### 10.1 Rule

Provider keys (Groq, Gemini, Cerebras, any future) live ONLY in the laptop keyring + local settings fallback. The Worker D1 `api_keys` table is deprecated.

### 10.2 Where keys are used today (verified)

- `commands.rs:1893-1906` `read_groq_api_key` — local settings/keyring read (client-side STT/LLM). ✅ stays.
- `commands.rs:1926-1955` `read_api_key` — keychain-first reads for ghost vision etc. ✅ stays.
- Worker `/apikeys/*` + `api_keys` table + `GET /config/check` listing — ❌ deprecate.
- `external_llm.ts` `callGroq`/`callGemini` — currently receives keys from the client request payload per-call (verify at implementation) or Worker env — keep per-call ephemeral pass-through; NEVER persist what arrives.

### 10.3 Enforcement steps

1. Worker: `POST /apikeys/add|remove` return `410 Gone` with explanatory message; `GET /apikeys/list` returns `{ "deprecated": true }`.
2. Migration job: read existing `api_keys` rows, **delete them** (they must never persist server-side), log count to `profile_events`.
3. Frontend: Connections/Accounts tab copy updated ("keys stay on this laptop"); remove any add-key-to-cloud affordances.
4. Client: add a local startup self-check — assert no provider key strings are included in transcript payloads (unit test with known key patterns).

### 10.4 Threat note

Central key storage (even D1-encrypted) gives the admin access to every user's billable keys. BYOK removes that trust requirement entirely — consistent with directive 4.

---

## 11. OAuth Decision (Option A adopted)

**Adopted: Option A — narrow OAuth exception.**

Server-mediated Gmail/GitHub/Swiggy flows are impossible without stored refresh tokens. The adopted rule:

- OAuth tokens remain in D1 (`oauth_tokens`) **only** for approved profiles.
- Scope stays exactly what the user connected (readonly gmail, repo, etc.).
- Tokens are deleted on revoke (`DELETE /v1/admin/profiles/revoke` cascades).
- No new secret classes are ever added to D1; provider API keys stay excluded (§10).
- If the admin later wants literal zero-secrets, Option B (kill server OAuth, client-local OAuth like the existing `google/oauth.rs` loopback flow which already exists) is a self-contained follow-up — no schema blockers.

---

## 12. Migration Plan

### 12.1 Legacy identity mapping

Existing installs already talk to the Worker with client-generated `user_id`. Migration table:

```sql
CREATE TABLE IF NOT EXISTS identity_migration (
  legacy_user_id TEXT PRIMARY KEY,
  profile_id     TEXT NOT NULL,
  migrated_at    REAL NOT NULL,
  auto_approved  INTEGER NOT NULL DEFAULT 0
);
```

### 12.2 `MIGRATION_MODE` (worker env var / secret, three states)

| Mode | Behavior |
|------|----------|
| `off` (Phase 1 default) | Legacy path fully works; new claim endpoint live; existing users unaffected |
| `strict` (Phase 2, after admin reviews) | Legacy `user_id` auto-mapped to pending profiles (auto_approved=0); transcript 403s for unmapped legacy IDs |
| `revoke-legacy` (final) | Unmapped legacy IDs hard-denied |

### 12.3 Sequencing

1. Deploy schema + claim + admin endpoints (mode=off). Zero behavior change.
2. Admin runs migration backfill: map each known legacy `user_id` → new profile (pending).
3. Admin approves the real users' profiles.
4. Flip `strict`, then `revoke-legacy` after an announcement window.
5. Drop dead `api_keys` data.

### 12.4 Existing data disposition

| Table | Action |
|-------|--------|
| `oauth_tokens` | KEEP, re-key to profile_id via migration join (Option A) |
| `api_keys` | DELETE rows; endpoints 410 |
| `user_devices` | DROP (replaced by `devices`) |
| `usage_log` | KEEP; optionally backfill profile mapping for counters |

---

## 13. Security Analysis

| Threat | Mitigation |
|--------|-----------|
| Random stranger consumes Workers AI | R1 default-deny + no registration without claim + admin approval |
| Cloned `nexus-config.json` (copy laptop identity) | Device token in keyring is machine-bound-ish; clone lacks token → 401; rotation kills old token |
| Stolen device token | Admin/device revoke + hash-only storage + rotation endpoint |
| D1 dump reveals user keys | No keys ever stored (§10); tokens are Google/GitHub-scoped OAuth only (§11) |
| Registration spam (fake pending flood) | IP rate-limit on claim + pending cap (e.g. 50 pending max) + cleanup job |
| Admin token leak in logs | Timing-safe compare, never log auth headers (audit: existing `audit_line` choke point pattern) |
| Denial bypass via external_llm | Gate covers relay paths; type-level EntitlementContext + grep-gate |
| Silent fallback masking denial | Frontend MUST speak the distinct denial line — test pins it |
| Man-in-the-middle claim | HTTPS (Workers enforce); device token delivered once over TLS; no downgrade |
| Profile enumeration | profile_ids are 128-bit random; pending list only visible to admin |

---

## 14. Failure Modes

| Failure | Client experience | Recovery |
|---------|-------------------|----------|
| Worker down during claim | LOCAL_ONLY; queued retry | Auto when Worker returns |
| Admin never approves | "Awaiting approval" state; local features work | Approve, or client ignores cloud |
| Token expired mid-session | Next request 403 `expired` → spoken notice + identity card turns amber | Admin re-approves |
| D1 hiccup during gate | Fail closed (403 `lookup_failed`) | Retry; no partial AI spend |
| User reinstalls | Fresh pending profile | Optional recover flow (admin-assisted) |
| Legacy client (old binary) hits strict mode | 403 with legacy-mapping notice | Update app / admin maps+approves |

---

## 15. Testing Matrix

### Worker (`server/worker` vitest)

| Group | Tests |
|-------|-------|
| Claim | new install; idempotent re-claim; reinstall flag; rate-limit 429; pending-cap |
| Gate | each row of §7 decision table; no-AI-call-on-deny (spy on env.AI.run); no-usage-increment-on-deny; fail-closed on D1 error |
| Admin | bad token 401; timing-safe path; approve→entitlement row + event; suspend/revoke cascade deletes oauth tokens; expire boundary |
| BYOK | /apikeys/* return 410; migration delete; transcript payload never persists keys |
| Migration | legacy mapping in off/strict modes; unmapped legacy in strict = 403 |
| Isolation | two profiles: separate quota counters; token of A cannot read B's `/me`; B's entitlement change does not affect A |

### Client (Rust + vitest)

| Group | Tests |
|-------|-------|
| Claim command | success; network fail → retry-queued, IDs unchanged; 403 denied-state mapping |
| Storage | token in keyring not config; config identity flag transitions |
| Frontend | pending banner; denied spoken line pinned; identity card states |
| Regression | legacy-config startup still boots (migration off) |

### Gates (per repo convention)

```
cargo test --lib -- --test-threads=1   (serial)
npx vitest run (frontend)              x2
cd server/worker && npm test           x2
npx tsc --noEmit; npm run build
cargo build --release --features custom-protocol,admin-brain
node check-ai-gate.mjs (new grep gate)
```

---

## 16. Rollout Plan

| Step | Gate to pass | Risk |
|------|--------------|------|
| 1. Schema + claim + admin endpoints (mode=off) | Worker tests green; /health additive | Zero client impact |
| 2. Entitlement gate in transcript path (still mode=off ⇒ gate passes everyone mapped or un mapped per off-mode rule) | Bypass-grep gate + tests | Guarded by mode=off |
| 3. Client claim integration (new binary) | Rust+FE gates; legacy config boot | Old binaries unaffected |
| 4. Admin backfill + approvals | Live check of 2 known devices | None |
| 5. `strict` mode | Two-laptop isolation drill live | Legacy unknowns get 403 (announced) |
| 6. `revoke-legacy` + api_keys deletion | Final audit | Terminal |

Rollback: flip `MIGRATION_MODE=off` at any point (gate becomes pass-through for mapped, deny only for revoked — documented per-mode semantics), redeploy. No data destruction until step 6.

---

## 17. Open Questions (resolved)

| Question | Resolution |
|----------|-----------|
| Where are profiles "stored in Worker AI"? | Nowhere — Worker backend + D1 (§1) |
| Do multiple laptops of one human share anything? | No (default). Optional future account-link does not merge history/keys |
| What about Swiggy MCP tokens? | Same class as OAuth (Option A) |
| Can a user approve themselves? | No — admin endpoints only; claim always lands `pending` |
| Does NLU OTA distribution need entitlement? | No — it's model download (client-side inference), not Workers AI; remains open with whitelist guard |
| Telegram path? | Uses same Worker transcript contract → same gate applies |

---

## Implementation readiness

Phased execution checklist lives in
[`02-implementation-checklist-admin-gated-identity.md`](./02-implementation-checklist-admin-gated-identity.md).
Feature-facing spec (user-visible behavior) lives in
[`../../features/88-admin-gated-worker-identity-and-byok.md`](../../features/88-admin-gated-worker-identity-and-byok.md).
