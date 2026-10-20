# Linux port J1 — Worker deny-by-default device gate

Scope: `server/worker/src/index.ts`, `server/worker/src/auth.ts`,
`server/worker/schema.sql`, `server/worker/migrations/`,
`server/worker/src/__tests__/*`. **Code only — not deployed.**

## Why

The Worker handed out live OAuth access tokens to anybody who supplied a
string:

```
GET /oauth/google-token?user_id=X  → Gmail (read+send), Calendar, Contacts, Drive, Sheets
GET /oauth/github-token?user_id=X  → repo read:org workflow (all private repos)
GET /oauth/swiggy-token?user_id=X  → Swiggy MCP token
```

`/oauth/status`, `/oauth/disconnect` and `/apikeys/*` were keyed on the same
client-supplied `user_id`, and every response carried
`Access-Control-Allow-Origin: *`, so any web page could read them
cross-origin. The branch already shipped `server/worker/src/auth.ts` with 27
passing tests for exactly this — but **nothing imported it**: the helpers were
dead code, and `auth.ts` read a `user_devices` table that `schema.sql` drops
in favour of the Feature-88 `devices` table. It was a false assurance.

## What changed

### `server/worker/src/auth.ts` (rewritten against this branch)

* **The credential now lives in the Feature-88 `devices` table.** Lookup is
  `WHERE device_token_hash = ? AND status = 'active' AND revoked_at IS NULL`,
  so a revoked device cannot authenticate even with a valid token, and the
  plaintext token never touches the database.
* **The hash and the constant-time compare are the ones in `./identity`**
  (`sha256Hex`, `timingSafeEqualHex`) — re-used, not re-implemented, so the
  Worker has exactly one implementation of both. `safeEqualHex` remains as an
  alias.
* **The owner of a device is its `profile_id`.** That is the canonical,
  Worker-issued identity, and it is the same key `handleTranscript` already
  uses for canonical devices (`credentialKey = gate.legacy ? userId :
  gate.profileId`). The column is selected under its real name and renamed in
  TypeScript next to `DeviceRecord` — deliberately **not** through a SQL
  `AS user_id` alias: an alias is invisible to the type checker, so dropping it
  would silently produce an owner of `undefined` instead of a compile error.
  The hand-rolled D1 doubles return the table's real columns, so they need no
  alias emulation.
* **`registerDevice()` writes the Feature-88 `devices` table** (upsert on
  `device_id`, `status = 'active'`), returning the plaintext token once.
* New pure helpers, all unit-tested: `routeKey`, `isPublicRoute`,
  `resolveAuthUserId`, `parseAllowedOrigins`, `isAllowedOrigin`, `corsHeaders`.

### `server/worker/src/index.ts`

* **Deny-by-default gate.** `isPublicRoute(method, path)` decides; everything
  else must present a valid device bearer. The gate runs before the router, so
  it cannot be bypassed by adding a route later.
* **`authUserId` is the only identity any handler may trust.** The three token
  routes, `/oauth/status`, `/oauth/disconnect` and `/apikeys/*` all take the
  owner from the device.
* **OAuth `state` is 32 random bytes**, stored only as a hash, single-use,
  10-minute TTL. The provider is resolved from the state **row**, never parsed
  out of the parameter — so a legacy `provider:userId` state cannot redeem and
  the owner id no longer travels through the provider redirect, the browser
  history or a proxy log.
* **The `nexus://oauth/callback` deep link carries no user id** — provider,
  status and a short error reason only.
* **CORS reflects the origin** when it is on the allowlist (`tauri://localhost`,
  `http(s)://tauri.localhost`, loopback dev origins, plus anything in the
  optional `ALLOWED_ORIGINS` env var) and always sets `Vary: Origin`. The
  wildcard is gone from every response, including the model-download routes.

### The owner key — and why `provision_hint` is not it

`oauth_tokens.user_id` / `api_keys.user_id` now hold the canonical
`profile_id`. The alternative — resolving the device to the client's legacy
`user_xxx` through `profiles.provision_hint` — is **not** safe: the hint is
chosen by whoever calls `/v1/profiles/claim`, and the legacy id is not secret
(it rode through the OAuth redirect in the `state` and through every
`requester.id`). Anybody who learned a victim's legacy id could claim a
profile carrying that hint and inherit the victim's tokens. The canonical
`profile_id` is issued by the Worker and lives only in the laptop's keyring, so
it cannot be chosen, guessed or inherited.

The cost is stated plainly in the migration: **rows written before the gate
are keyed by the legacy id and are unreachable until they are re-keyed.** The
migration re-keys them only through `identity_migration` (an admin-controlled
legacy → profile mapping); with the usual empty table it changes nothing and
the account has to be reconnected once in the app.

### `server/worker/schema.sql` + `server/worker/migrations/`

* `schema.sql`: the `user_devices` table and its indexes are gone (the table
  was already dropped at the bottom of the file); `devices` gained
  `idx_devices_token_hash` — the index every authenticated request needs; the
  `oauth_tokens` / `api_keys` / `oauth_states` comments now say that `user_id`
  holds the canonical owner key.
* `migrations/0002_device_gate.sql` (new): creates `oauth_states` (idempotent),
  creates `idx_devices_token_hash`, drops the unused `user_devices`, and
  re-keys pre-gate rows through `identity_migration`.
* `migrations/0001_device_auth.sql`: marked **superseded** — it ALTERs the
  dropped `user_devices` table and fails on a database created from the
  current `schema.sql`.

## Deploy order (load-bearing)

```bash
cd server/worker

# 1. MIGRATION FIRST — creates the OAuth state table and the credential index.
wrangler d1 execute nexus-db --remote --file=migrations/0002_device_gate.sql

# 2. Verify
wrangler d1 execute nexus-db --remote \
  --command "SELECT name FROM sqlite_master WHERE type='table' AND name='oauth_states'"
wrangler d1 execute nexus-db --remote \
  --command "SELECT name FROM sqlite_master WHERE type='index' AND name='idx_devices_token_hash'"

# 3. WORKER
wrangler deploy

# 4. CLIENTS — the update that sends the device bearer and stops sending
#    `user_id` (Linux port J2 + J3).
```

**Rollback:** revert the Worker deploy. The migration is additive (a table, an
index, a dropped unused table) and does not need to be undone.

**Outage window.** Between step 3 and step 4 the app cannot reach the Worker:
the old client sends no device bearer, so every non-public route answers 401.
The client update cannot be shipped first — it stops sending `user_id`, which
the old Worker still needs. If the order has to be reversed for a release
train, deploy the client and the Worker in the same window and expect the
transcript route to be down until the user has claimed a profile.

## What breaks for old clients

| Surface | Before | After |
|---|---|---|
| `POST /` (the transcript turn) | accepted an unauthenticated caller | **401** without a device bearer |
| `GET /oauth/*-token`, `/oauth/status`, `/oauth/disconnect`, `/apikeys/*` | keyed on `?user_id=` | **401** without a device bearer |
| `GET /oauth/auth-url`, `POST /oauth/exchange` | accepted `?user_id=` | **401** without a device bearer; the `user_id` is ignored |
| `GET /v1/profiles/me`, `/v1/devices/*` | already needed the bearer | unchanged (the gate runs first; a **revoked** device now returns 401 instead of 403 `revoked`) |
| `/health`, `/v1/profiles/claim`, `/models/nlu/*` | public | still public |
| `/v1/admin/*` | admin token | still admin token (never device-gated) |
| A pre-gate Google/GitHub/Swiggy connection | reachable by legacy `user_xxx` | unreachable until reconnected (or re-keyed through `identity_migration`) |
| `nexus://oauth/callback` deep link | carried `user_id` | carries provider + status + a short error only |

A client that has not claimed a profile has no device credential at all and
cannot reach the Worker: the Setup screen's identity claim is the bootstrap,
and it stays public for exactly that reason.

## Transition rule (grace period)

A request may carry **both** a valid device bearer and a legacy `user_id`
(query parameter or body field). The `user_id` is accepted and **ignored** —
the device's owner wins (`resolveAuthUserId`). A request with only a
`user_id` and no valid bearer is **401**.

## Tests added

* `server/worker/src/__tests__/auth.test.ts` (rewritten for the Feature-88
  table): device lookup by hash, revoked/inactive devices rejected, the
  `X-Nexus-Device-Token` header form, hash-only persistence, credential
  rotation, `resolveAuthUserId` ignoring a forged id, the route
  classification, and the CORS policy.
* `server/worker/src/__tests__/device_gate.test.ts` (new): the real Worker
  router — every gated route 401 without a bearer, 401 with only a
  `user_id`, the owner's token returned with a bearer, a forged `user_id`
  ignored on read **and** write, the public routes still reachable, the random
  single-use state, a legacy `provider:userId` state refused, and the CORS
  reflection.
* `server/worker/src/__tests__/entitlement.test.ts` (updated): the transcript
  route now needs a device credential — the legacy `MIGRATION_MODE=off`
  entitlement semantics are unchanged.

Both hand-rolled D1 doubles mirror the real API surface exactly, because a
double that is "close enough" hides wiring bugs instead of catching them:
`all()`/`run()` resolve to a `D1Result` (`{ results, success, meta }`), never a
bare array, and a device SELECT returns the table's real `profile_id` column
rather than an aliased one. Both mistakes made gated routes answer `200` with
empty lists while every 401/forged-id assertion still passed.

## Known gaps / not done

* **The client half is not in this job.** `frontend/src/setup/oauth.ts` and
  `src-tauri/src/auth_vault.rs` / `github_cmd.rs` still call the Worker
  directly with `?user_id=` and no bearer — that is Linux port J2 (Rust
  `worker_proxy` allowlist) and J3 (frontend routes through it). Until those
  land, an old client gets 401 on every one of those routes.
* **`POST /` still trusts `requester.id` for the *user* dimension** once the
  device is authenticated. The gate proves the device; binding the turn to the
  device's profile is the identity-migration work, not this job.
* **The device credential does not expire.** The Feature-88 `devices` table has
  no expiry column; a credential is rotated (`POST /v1/devices/rotate-token`)
  or revoked (`DELETE /v1/devices/current`, admin revoke) instead.
* **The tokens still transit the Worker.** They are no longer readable by a
  stranger, but a Worker-side bug or a D1 leak still exposes them. The real
  fix is device-held tokens (`docs/features/research/17` §11).
* **`ALLOWED_ORIGINS` is read from the environment, not from `wrangler.toml`** —
  no deploy config was touched. The default allowlist covers the packaged app
  and the dev server; a remote web client has to be added explicitly.
