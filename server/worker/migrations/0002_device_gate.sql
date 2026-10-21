-- Migration: Worker deny-by-default device gate (Linux port J1)
-- Date: 2026-10-20
-- Target: D1 database `nexus-db` (id 4bca5d1b-55e1-4a30-bbf9-ca28cf96e3ea)
--
-- WHY: the Worker returned live OAuth access tokens from /oauth/github-token,
-- /oauth/google-token and /oauth/swiggy-token keyed only on a client-supplied
-- `?user_id=`, with no authentication at all, and every response carried
-- `Access-Control-Allow-Origin: *`. The Worker now authenticates every
-- non-public route against the Feature-88 `devices` table and takes the owner
-- from the device.
--
-- APPLY:
--   wrangler d1 execute nexus-db --remote --file=migrations/0002_device_gate.sql
--
-- ORDER (load-bearing):  migration  ->  Worker  ->  clients.
--   1. this migration, so the state table and the credential index exist
--   2. `wrangler deploy` — after this, every non-public route needs a device
--      bearer; a client without one gets 401
--   3. the client update, which sends the device bearer and stops sending
--      `user_id`
--
-- NOTE ON 0001: `migrations/0001_device_auth.sql` is SUPERSEDED. It ALTERs a
-- `user_devices` table that the branch dropped in favour of the Feature-88
-- `devices` table, so it fails on a database created from the current
-- schema.sql. Everything it was after is done here (idempotently), except the
-- device credential itself, which now lives in `devices.device_token_hash`.

-- ── 1. OAuth CSRF state ────────────────────────────────────────────────────
--
-- The `state` used to be the literal string "provider:userId": not random (so
-- not a CSRF token) and it published the owner in the provider redirect URL,
-- the browser history and every proxy log. It is now 32 random bytes, stored
-- only as a hash, single-use, TTL-bound. The owner is bound to the ROW, not
-- to the value, so the callback can identify the owner without the id ever
-- travelling through the browser.

CREATE TABLE IF NOT EXISTS oauth_states (
  state_hash TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  created_at REAL NOT NULL,
  expires_at REAL NOT NULL,
  consumed_at REAL
);

CREATE INDEX IF NOT EXISTS idx_oauth_states_expiry ON oauth_states(expires_at);

-- ── 2. Credential lookup index ────────────────────────────────────────────
--
-- Every authenticated request looks the device up by the SHA-256 hash of its
-- bearer token. The table had no index on that column, so the lookup was a
-- full scan of `devices`.

CREATE INDEX IF NOT EXISTS idx_devices_token_hash ON devices(device_token_hash);

-- ── 3. Retire the unused `user_devices` table ─────────────────────────────
--
-- It was defined by an older schema.sql and never read or written by any code
-- path (verified 2026-10-03). schema.sql already drops it; this makes a
-- deployed database match, and stops a stale migration from resurrecting a
-- second credential table the Worker no longer trusts.

DROP TABLE IF EXISTS user_devices;

-- ── 4. Re-key pre-gate rows (OPTIONAL, safe, usually a no-op) ─────────────
--
-- The owner key of `oauth_tokens` / `api_keys` is now the canonical
-- Feature-88 `profile_id` of the device that owns them. Rows written before
-- the gate are keyed by the client-generated legacy `user_xxx` and are
-- therefore unreachable until they are re-keyed.
--
-- ONLY rows whose legacy id has been explicitly mapped to a profile by an
-- administrator (the `identity_migration` table, written by the admin
-- tooling) are re-keyed. `profiles.provision_hint` is deliberately NOT used
-- here: it is a client-chosen string, so anybody who learned a victim's
-- legacy id could claim a profile carrying that hint and inherit the
-- victim's tokens.
--
-- If `identity_migration` is empty (the usual case) these two statements
-- change nothing, and the affected account has to be reconnected once in the
-- app — which is the honest outcome: the old key was never proof of
-- ownership.

UPDATE oauth_tokens
   SET user_id = (SELECT m.profile_id FROM identity_migration m WHERE m.legacy_user_id = oauth_tokens.user_id)
 WHERE EXISTS (SELECT 1 FROM identity_migration m WHERE m.legacy_user_id = oauth_tokens.user_id);

UPDATE api_keys
   SET user_id = (SELECT m.profile_id FROM identity_migration m WHERE m.legacy_user_id = api_keys.user_id)
 WHERE EXISTS (SELECT 1 FROM identity_migration m WHERE m.legacy_user_id = api_keys.user_id);

-- ── Verification (run after applying) ──────────────────────────────────────
--
-- Expect the state table and the credential index to exist:
--
--   wrangler d1 execute nexus-db --remote \
--     --command "SELECT name FROM sqlite_master WHERE type='table' AND name='oauth_states'"
--   wrangler d1 execute nexus-db --remote \
--     --command "SELECT name FROM sqlite_master WHERE type='index' AND name='idx_devices_token_hash'"
--
-- Expect NO `user_devices` table:
--
--   wrangler d1 execute nexus-db --remote \
--     --command "SELECT name FROM sqlite_master WHERE type='table' AND name='user_devices'"
--
-- Expect the devices to be there (this is the credential the gate reads):
--
--   wrangler d1 execute nexus-db --remote \
--     --command "SELECT device_id, profile_id, status, revoked_at FROM devices"
--
-- Rows that still need a manual re-key or a reconnect:
--
--   wrangler d1 execute nexus-db --remote \
--     --command "SELECT user_id, provider FROM oauth_tokens WHERE user_id NOT IN (SELECT profile_id FROM profiles)"
