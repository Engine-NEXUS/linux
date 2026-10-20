-- Migration: device authentication + OAuth CSRF state
-- Date: 2026-10-02
-- Target: D1 database `nexus-db` (id 4bca5d1b-55e1-4a30-bbf9-ca28cf96e3ea)
--
-- WHY: the Worker returned live OAuth access tokens from /oauth/github-token,
-- /oauth/google-token and /oauth/swiggy-token keyed only on a client-supplied
-- `?user_id=`, with no authentication. `device_token` existed but was never
-- written or read. These statements make the credential real.
--
-- APPLY (this REPLACES user_devices rows that carry a plaintext token, of which
-- there are none today because no client ever registered):
--   wrangler d1 execute nexus-db --remote --file=migrations/0001_device_auth.sql
--
-- NOTE ON ORDER: the device now registers and *receives* a token from the
-- Worker. Because `device_token_hash` did not exist before, no existing device
-- row can authenticate. Every device re-registers on next launch, which is the
-- intended rotation path anyway.

-- ── 1. Add the credential columns to user_devices ──────────────────────────
--
-- `device_token` (plaintext, if it were ever written) is superseded. SQLite
-- cannot DROP COLUMN in older builds and the column is harmless once unused,
-- so it is left in place for compatibility with the old schema and to avoid a
-- destructive rebuild. New code never reads or writes it.

ALTER TABLE user_devices ADD COLUMN device_token_hash TEXT;
ALTER TABLE user_devices ADD COLUMN device_token_expires_at REAL;
ALTER TABLE user_devices ADD COLUMN device_revoked_at REAL;

-- Lookup is by hash, so this index is on the hot path of every authenticated
-- request.
CREATE INDEX IF NOT EXISTS idx_devices_token ON user_devices(device_token_hash);

-- ── 2. OAuth CSRF state ────────────────────────────────────────────────────
--
-- `state` was the literal string "provider:userId": not random (so not a CSRF
-- token) and it published the user_id in the provider redirect URL, browser
-- history, and proxy logs. Now 32 random bytes, hashed at rest, single-use,
-- TTL-bound.

CREATE TABLE IF NOT EXISTS oauth_states (
  state_hash TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  created_at REAL NOT NULL,
  expires_at REAL NOT NULL,
  consumed_at REAL
);

CREATE INDEX IF NOT EXISTS idx_oauth_states_expiry ON oauth_states(expires_at);

-- ── 3. Clear any plaintext tokens that predate the change ──────────────────
--
-- Safety net. Expected to affect zero rows today, but if any environment did
-- have a plaintext token stored, it is nulled here so it can never be
-- resurrected as a credential.

UPDATE user_devices SET device_token = NULL WHERE device_token IS NOT NULL;

-- ── Verification (run after applying) ──────────────────────────────────────
--
-- Expect `device_token_hash` present and the oauth_states table to exist:
--
--   wrangler d1 execute nexus-db --remote \
--     --command "SELECT COUNT(*) AS devices FROM user_devices"
--   wrangler d1 execute nexus-db --remote \
--     --command "SELECT name FROM sqlite_master WHERE type='table' AND name='oauth_states'"
--
-- Then deploy the Worker and confirm a device registers:
--
--   wrangler d1 execute nexus-db --remote \
--     --command "SELECT user_id, device_id, device_revoked_at FROM user_devices"