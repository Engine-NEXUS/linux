-- NEXUS D1 Database Schema
-- Cloudflare D1 (free: 5GB storage, 5M reads/day, 100K writes/day)
--
-- Stores OAuth tokens for connected integrations, plus canonical laptop
-- identity (Feature 88: profiles/devices/entitlements) and operational
-- usage counters.
--
-- NEVER stored in D1 (Feature 88 directive): chat history, conversation
-- transcripts, user provider API keys (BYOK stays in each laptop's OS
-- credential store), memory, diary.
--
-- The Worker reads/writes this database. No server needed.

-- OAuth tokens (Google, GitHub)
--
-- `user_id` holds the OWNER key of the connected account. Since the Linux port
-- J1 device gate it is the canonical Feature-88 `profile_id` of the device
-- that performed the connection — never a client-supplied id. The column name
-- is historical (it predates the canonical identity); renaming it would break
-- deployed databases.
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

-- API keys (Claude, Devin, etc.) — base64-obfuscated in the column
-- (NOT encrypted; protection is D1 encryption at rest). The key_encrypted
-- column name is historical; renaming it would break deployed databases.
-- `user_id` holds the canonical owner key (see oauth_tokens above).
CREATE TABLE IF NOT EXISTS api_keys (
  user_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  key_encrypted TEXT NOT NULL,
  created_at REAL NOT NULL,
  PRIMARY KEY (user_id, provider)
);

-- Index for fast lookups
CREATE INDEX IF NOT EXISTS idx_oauth_user ON oauth_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_apikeys_user ON api_keys(user_id);

-- OAuth CSRF state (2026-10-02).
--
-- SECURITY: `state` used to be the literal string "provider:userId". That was
-- (a) not random, so it was not a CSRF token, and (b) published the user_id in
-- the redirect URL, browser history, and any proxy logs — and user_id was the
-- only thing protecting the OAuth token endpoints.
--
-- It is now 32 random bytes, stored here as a hash, single-use, with a TTL.
-- `user_id` here holds the canonical owner key (the Feature-88 `profile_id`),
-- so the callback can bind the provider's tokens to the right owner without
-- the id ever travelling through the browser.
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

-- ─── Feature 88: canonical laptop profiles, admin-gated Workers AI ──────
--
-- One canonical profile per laptop installation. Worker-issued IDs.
-- Lifecycle: pending → (admin approves) approved → suspended|revoked.
-- The client's locally-generated UUIDs are provisional hints only.

CREATE TABLE IF NOT EXISTS profiles (
  profile_id       TEXT PRIMARY KEY,        -- "prof_" + 128-bit base64url
  human_label      TEXT,                    -- optional admin-set name
  status           TEXT NOT NULL,           -- pending|approved|suspended|revoked
  quota_tier       TEXT,                    -- "default" | "premium" | "restricted"
  created_at       REAL NOT NULL,
  approved_at      REAL,
  approved_by      TEXT,                    -- admin token id (never the token)
  expires_at       REAL,                    -- null = no expiry
  last_seen_at     REAL,
  provision_hint   TEXT                     -- client's provisional user_xxx (audit only)
);

-- Devices bound to a profile. Device token stored ONLY as SHA-256 hash;
-- the plaintext token lives in each laptop's OS credential store.
--
-- This is the credential the Worker authenticates against (Linux port J1):
-- `Authorization: Bearer <token>` (or `X-Nexus-Device-Token`) is hashed and
-- looked up here, so a leaked database row cannot be replayed as a
-- credential. The device's `profile_id` is the canonical owner key of every
-- OAuth token, API key and OAuth state the device creates.
CREATE TABLE IF NOT EXISTS devices (
  device_id         TEXT PRIMARY KEY,       -- "dev_" + 128-bit base64url
  profile_id        TEXT NOT NULL REFERENCES profiles(profile_id),
  device_token_hash TEXT NOT NULL,
  device_name       TEXT,
  os                TEXT,
  app_version       TEXT,
  status            TEXT NOT NULL,          -- active|revoked
  created_at        REAL NOT NULL,
  last_seen_at      REAL,
  revoked_at        REAL
);

-- Admin grants. Separable so entitlement operations are auditable
-- independently of profile status. scope: "all" | "worker_ai" | "llm_relay".
CREATE TABLE IF NOT EXISTS entitlements (
  profile_id       TEXT NOT NULL REFERENCES profiles(profile_id),
  scope            TEXT NOT NULL,
  allowed          INTEGER NOT NULL,        -- 1/0
  quota_tier       TEXT,
  granted_by       TEXT NOT NULL,
  granted_at       REAL NOT NULL,
  expires_at       REAL,
  PRIMARY KEY (profile_id, scope)
);

-- Append-only audit trail. detail holds admin notes / reason codes /
-- abuse-control metadata (registration IP) — never user content.
CREATE TABLE IF NOT EXISTS profile_events (
  id               INTEGER PRIMARY KEY AUTOINCREMENT,
  profile_id       TEXT NOT NULL,
  device_id        TEXT,
  event            TEXT NOT NULL,           -- registered|approved|suspended|revoked|expired|token_rotated|denied|recovery
  detail           TEXT,
  at               REAL NOT NULL
);

-- Legacy (client-generated) user_id → canonical profile mapping used by
-- the staged migration modes (off / strict / revoke-legacy).
CREATE TABLE IF NOT EXISTS identity_migration (
  legacy_user_id   TEXT PRIMARY KEY,
  profile_id       TEXT NOT NULL,
  migrated_at      REAL NOT NULL,
  auto_approved    INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_devices_profile ON devices(profile_id);
-- Hot path of the device gate: every authenticated request looks the
-- credential up by its SHA-256 hash (Linux port J1).
CREATE INDEX IF NOT EXISTS idx_devices_token_hash ON devices(device_token_hash);
CREATE INDEX IF NOT EXISTS idx_events_profile  ON profile_events(profile_id);
CREATE INDEX IF NOT EXISTS idx_events_at       ON profile_events(at);

-- `user_devices` was defined but never read or written by any code path
-- (verified 2026-10-03) — superseded by the Feature 88 `devices` table, which
-- is now the credential the Worker authenticates against. The table is
-- dropped so it can never be resurrected by a stale migration.
DROP TABLE IF EXISTS user_devices;
