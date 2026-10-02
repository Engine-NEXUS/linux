-- NEXUS D1 Database Schema
-- Cloudflare D1 (free: 5GB storage, 5M reads/day, 100K writes/day)
--
-- Stores OAuth tokens and API keys for all NEXUS users.
-- The Worker reads/writes this database. No server needed.

-- OAuth tokens (Google, GitHub)
CREATE TABLE IF NOT EXISTS oauth_tokens (
  user_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  access_token TEXT NOT NULL,
  refresh_token TEXT,
  expires_at REAL,          -- unix timestamp, 0 = no expiry
  scopes TEXT,
  account_id TEXT,          -- GitHub login or Google email
  created_at REAL NOT NULL,
  PRIMARY KEY (user_id, provider)
);

-- API keys (Claude, Devin, etc.) — encrypted at rest
CREATE TABLE IF NOT EXISTS api_keys (
  user_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  key_encrypted TEXT NOT NULL,
  created_at REAL NOT NULL,
  PRIMARY KEY (user_id, provider)
);

-- Device registration.
--
-- SECURITY (2026-10-02): the plaintext `device_token` column was never written
-- by any client and never read by any code path — a control that did nothing.
-- It is replaced by a SHA-256 hash plus expiry/revocation so that a leaked
-- database row cannot be replayed as a credential, and so a device can be
-- rotated without touching OAuth tokens.
--
-- The bearer credential is presented as `Authorization: Bearer <token>`.
-- Only the hash is stored. Lookup is by hash, so the plaintext token never
-- touches disk or the database.
CREATE TABLE IF NOT EXISTS user_devices (
  user_id TEXT NOT NULL,
  device_id TEXT NOT NULL,
  device_name TEXT,
  os TEXT,
  device_token_hash TEXT,          -- hex SHA-256 of the bearer token (never the token)
  device_token_expires_at REAL,    -- unix ts; NULL = no expiry
  device_revoked_at REAL,          -- unix ts; NULL = active
  created_at REAL NOT NULL,
  PRIMARY KEY (user_id, device_id)
);

-- Index for fast lookups
CREATE INDEX IF NOT EXISTS idx_oauth_user ON oauth_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_apikeys_user ON api_keys(user_id);
CREATE INDEX IF NOT EXISTS idx_devices_user ON user_devices(user_id);
CREATE INDEX IF NOT EXISTS idx_devices_token ON user_devices(device_token_hash);

-- OAuth CSRF state (2026-10-02).
--
-- SECURITY: `state` used to be the literal string "provider:userId". That was
-- (a) not random, so it was not a CSRF token, and (b) published the user_id in
-- the redirect URL, browser history, and any proxy logs — and user_id was the
-- only thing protecting the OAuth token endpoints.
--
-- It is now 32 random bytes, stored here as a hash, single-use, with a TTL.
CREATE TABLE IF NOT EXISTS oauth_states (
  state_hash TEXT PRIMARY KEY,      -- hex SHA-256 of the state value
  user_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  created_at REAL NOT NULL,
  expires_at REAL NOT NULL,        -- created_at + 10 minutes
  consumed_at REAL                 -- NULL until redeemed; redemption is one-shot
);

CREATE INDEX IF NOT EXISTS idx_oauth_states_expiry ON oauth_states(expires_at);

-- Per-user daily usage tracking (quota enforcement + cost control)
CREATE TABLE IF NOT EXISTS usage_log (
  user_id TEXT NOT NULL,
  day_utc TEXT NOT NULL,          -- YYYY-MM-DD (UTC)
  requests INTEGER NOT NULL DEFAULT 0,
  ai_neurons INTEGER NOT NULL DEFAULT 0,
  d1_reads INTEGER NOT NULL DEFAULT 0,
  d1_writes INTEGER NOT NULL DEFAULT 0,
  search_calls INTEGER NOT NULL DEFAULT 0,
  deep_calls INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (user_id, day_utc)
);

-- Cache entries (supplements KV; used when KV is not configured)
CREATE TABLE IF NOT EXISTS cache_entries (
  cache_key TEXT NOT NULL,
  cache_value TEXT NOT NULL,
  expires_at REAL NOT NULL,
  created_at REAL NOT NULL,
  PRIMARY KEY (cache_key)
);
CREATE INDEX IF NOT EXISTS idx_cache_expires ON cache_entries(expires_at);
