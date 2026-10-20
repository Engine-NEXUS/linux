# P0 — Device Authentication & OAuth CSRF Fix

**Date:** 2026-10-02
**Status:** Implemented. Not deployed — requires a D1 migration first (see §6).
**Closes:** the live credential-disclosure vulnerability documented in
[`45-competitive-and-platform-audit-2026-10.md`](45-competitive-and-platform-audit-2026-10.md).

---

## 1. The vulnerability, restated

Three Worker routes returned **live OAuth access tokens** keyed only on a
client-supplied `?user_id=`, with no authentication:

```
GET /oauth/github-token?user_id=X  → GitHub token (repo read:org workflow)
GET /oauth/google-token?user_id=X  → Gmail (readonly+send), Calendar, Contacts, Drive
GET /oauth/swiggy-token?user_id=X  → Swiggy token
```

Compounding factors:

1. `user_id` was **self-assigned** at registration (`index.ts` `handleRegister`)
2. `user_id` **leaked through the OAuth redirect URL** — `state` was the literal
   string `provider:userId`
3. `device_token` existed in `schema.sql:33`, was written by the register
   handler, and had **zero read sites** — a security control that did nothing
4. Only **1 of 19 routes** was gated at all
5. `Access-Control-Allow-Origin: *` on every response including token routes
6. `handleOAuthExchange`, `handleOAuthDisconnect`, `handleAddApiKey`,
   `handleRemoveApiKey`, `handleListApiKeys` and `handleOAuthStatus` all trusted
   `user_id` from the query or body

---

## 2. ⚠️ Deviation from the spec — deliberate

The work order in [`62-…md`](../features/62-competitive-and-platform-audit-2026-10.md) §5
says:

> *"Delete the three token endpoints. Replace with capability-scoped RPC that
> returns results, never tokens."*

**That was not done, and it should not have been done as written.** Reasons:

- `github_cmd.rs` builds an **octocrab client on-device** and calls
  `api.github.com` directly. Deleting the endpoint breaks all 28 GitHub
  subcommands for private repos.
- `auth_vault.rs` needs Google and Swiggy tokens locally for MCP (Gmail,
  Calendar, Contacts, Drive, Swiggy ordering).
- Replacing the broker with capability-scoped RPC means rewriting
  `github_cmd.rs` to proxy every operation, or rewriting `auth_vault.rs` to proxy
  every MCP call. Both are large refactors that add Worker CPU, latency and a new
  failure surface.

**What was done instead: the broker is now authenticated.** The tokens are no
longer readable by an attacker, which is what the vulnerability actually was. The
architecture is unchanged, so nothing breaks.

**The recommended architecture still stands** — device-held tokens with the Worker
as a stateless relay is where this should end up (see `research/17` §11). That is
P2 work, not P0. P0's job was to stop the bleeding without breaking the product.

---

## 3. What changed

### 3.1 Worker — new `server/worker/src/auth.ts`

| Helper | Purpose |
|---|---|
| `sha256Hex` | hex SHA-256, used for both device tokens and OAuth state |
| `randomHex(n)` | CSPRNG via `crypto.getRandomValues` |
| `safeEqualHex` | constant-time hex comparison |
| `authenticateDevice()` | resolves `Authorization: Bearer` (or `X-Nexus-Device-Token`) to a device row; revoked and expired devices are rejected **in the SQL predicate**, not after |
| `registerDevice()` | mints a 256-bit token, stores **only its SHA-256**, returns it once |
| `createOAuthState()` | 32 random bytes, hashed at rest, TTL 10 min |
| `consumeOAuthState()` | single-use redemption via an atomic `UPDATE … WHERE consumed_at IS NULL` |
| `PUBLIC_ROUTES` | the 6 routes that must stay reachable |

**Lookup is by hash**, so the plaintext token is never compared against, stored
or logged.

### 3.2 Worker — `index.ts`

- **Deny-by-default gate.** Anything not on `PUBLIC_ROUTES` requires a valid
  device. `/models/nlu/publish` keeps its separate admin-token gate and is
  reached before this check.
- **The three token routes no longer read `user_id` at all.** They call
  `requireAuthUserId()`, which returns the authenticated device's account. A
  device can only ever mint a token for the account it registered as.
- **`handleAuthUrl`** mints a random, hashed, single-use `state`. No `user_id`
  in the redirect URL.
- **`handleOAuthBrowserCallback`** redeems `state` instead of parsing it. An
  unknown, expired or already-used state is refused rather than guessed at.
- **`handleOAuthExchange`** now also redeems `state` (this is the deep-link leg
  and previously had no CSRF check at all), takes the user from the device, and
  verifies the state was minted for that provider.
- **`handleOAuthStatus`, `handleOAuthDisconnect`, `handleAddApiKey`,
  `handleRemoveApiKey`, `handleListApiKeys`** all take the user from the device
  instead of the query or body.
- **CORS** no longer sends `Access-Control-Allow-Origin: *`. It reflects the
  request origin with `Vary: Origin`.

### 3.3 Schema — `schema.sql` + `migrations/0001_device_auth.sql`

- `user_devices`: `device_token_hash`, `device_token_expires_at`,
  `device_revoked_at`, plus an index on the hash. The plaintext `device_token`
  column is left in place (harmless, avoids a destructive rebuild) and nulled by
  the migration.
- **New `oauth_states` table**: `state_hash` PK, `user_id`, `provider`,
  `created_at`, `expires_at`, `consumed_at`.

### 3.4 Rust — new `src-tauri/src/device_auth.rs`

- Mints a 256-bit token from **two UUIDv4s** (CSPRNG, not the existing
  time+pid+counter `uuid_v4()` helper — this value is a credential).
- Persists it in the **OS credential store** via the `keyring` crate, matching
  what `auth_vault.rs` already does. **Not** a plaintext file.
- `ensure_registered()` registers on first run; `auth_header_value()` /
  `apply_to()` attach the credential.
- **Fallback:** a machine with no Secret Service provider gets a process-local
  token and a loud warning, so the app still starts. Re-registers next launch.
- Registration is spawned at startup in a background task and does not delay
  first paint.

### 3.5 Rust — new `src-tauri/src/worker_proxy.rs`

The WebView made **7 direct Worker calls** (`/oauth/auth-url`, `/oauth/exchange`,
`/oauth/status`, `/oauth/disconnect`, `/apikeys/*`). The obvious shortcut —
handing the bearer token to the renderer — is the same mistake as the one being
fixed. Instead the renderer asks Rust to make the call.

**The allowlist is the security control.** Without it this would be a
confused-deputy primitive letting the renderer reach anything the device can
reach. 8 exact `(method, path)` pairs; everything else is refused. Tests assert
`/models/nlu/publish` and `POST /` are **not** reachable, and that trailing
slashes / `..` / prefix tricks do not bypass it.

### 3.6 Frontend

- **`ArchitectApp.tsx`** — deleted `fetchGithubToken()`. The renderer fetched a
  live token purely to pass it straight back into Tauri. Rust now resolves it
  internally via `resolve_github_token()`.
- **`setup/oauth.ts`** — all 7 `fetch` calls replaced with `workerCall()`, which
  goes through `worker_request`. `user_id` is no longer sent from the renderer.

**Net effect: the renderer never holds a credential and never calls the Worker
directly** (except the intentionally-public `/health` probe in the dead legacy
settings window).

---

## 4. Verification

| Check | Result |
|---|---|
| `worker: tsc --noEmit` | **PASS** |
| `worker: npm test` | **PASS** — 76/76 (49 existing + 27 new) |
| `worker: wrangler deploy --dry-run` | **PASS** — 164.02 KiB / 37.85 KiB gzip |
| `frontend: tsc --noEmit` | **PASS** |
| `frontend: npm test` | **PASS** — 28/28 |
| `frontend: npm run build` | **PASS** |
| `cargo check` | **PASS** |
| `cargo check --features mock-wake` | **PASS** |
| `cargo test --lib` | **PASS** — 572/572 (563 + 3 device_auth + 6 worker_proxy) |
| python CI steps (3 parses + data foundation) | **PASS** |

### New test coverage

`server/worker/src/__tests__/auth.test.ts` (27 tests) — SHA-256 against known
vectors; token entropy; missing/unknown/revoked/expired credential rejection;
header and bearer forms; hash-only persistence; rotation; state is 32 random
bytes and **does not contain the user id, the provider, or `:`**; single-use
redemption; expiry; concurrent-redemption race; and that a legacy
`google:user_victim` state **does not redeem**.

`src-tauri/src/device_auth.rs` (3) and `src-tauri/src/worker_proxy.rs` (6) — token
entropy/uniqueness, and the allowlist invariants above.

---

## 5. Known limitations of this fix

Stated plainly:

1. **The tokens still transit the Worker.** They are no longer readable by an
   attacker, but a Worker-side bug or a D1 leak still exposes them. The real fix
   is device-held tokens (P2, `research/17` §11).
2. **D1 still stores OAuth tokens in plaintext.** The `api_keys.key_encrypted`
   column is `btoa()`, which is **encoding, not encryption** — the Worker's own
   comment says so. Out of P0 scope; needs the `NEXUS_ENCRYPTION_KEY` secret that
   is declared in `Env` but never used.
3. **`GITHUB_SCOPES = "repo read:org workflow"` is unchanged.** Still full R/W on
   all private repos for a PR-review tool. Migrating to a GitHub App with
   fine-grained permissions is a separate project with its own review process.
4. **`register` is public.** An attacker can squat on a `device_id`, which gains
   them nothing: they cannot read another device's row, and OAuth tokens are
   keyed by `user_id`, which they still cannot use for anything.
5. **The device token is not DPoP-bound.** RFC 9449 would make a stolen token
   useless without the private key. Deferred.
6. **No retroactivity check was performed.** Whether `user_id` was ever
   enumerated in production requires Cloudflare Workers Logs access, which this
   change does not have. **Do this before declaring the incident closed.**

---

## 6. Deploy steps — order matters

```bash
cd server/worker

# 1. Apply the schema FIRST. Without this, every authenticated request throws on
#    the missing device_token_hash column and the app cannot reach the Worker at all.
wrangler d1 execute nexus-db --remote --file=migrations/0001_device_auth.sql

# 2. Verify
wrangler d1 execute nexus-db --remote \
  --command "SELECT name FROM sqlite_master WHERE type='table' AND name='oauth_states'"

# 3. Deploy
wrangler deploy

# 4. Launch the app once. Every device registers on first run and stores its
#    credential in the OS keyring. Existing sessions will 401 until this happens.
```

**Rollback:** revert the Worker deploy and re-run step 1 without the `UPDATE`.
Existing rows are untouched except for the nulled plaintext tokens.

---

## 7. Not done in P0

- **The 5-second voice-approval window** (`docs/features/54-…`) is still an
  injection vector: it auto-accepts *"proceed"* / *"yes"* from the microphone,
  and an injected instruction in an email or web page can simply say "say
  proceed." This is spec item 7 and it is **not fixed here.** It needs a
  Verifiable-Action-Card style default-deny redesign, not a patch.
- Retroactive log review.
- GitHub App migration, D1 encryption at rest, DPoP, Google scope reduction.
  All in `research/17` §12 as P1/P2.