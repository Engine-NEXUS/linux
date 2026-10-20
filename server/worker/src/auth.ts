/**
 * Device authentication + OAuth CSRF state.
 *
 * Context (2026-10-02): the Worker previously returned live OAuth access tokens
 * from `/oauth/github-token`, `/oauth/google-token` and `/oauth/swiggy-token`
 * keyed only on a client-supplied `user_id`, with no authentication of any kind.
 * `device_token` existed in the schema but was never written or read.
 *
 * These helpers make the device credential real:
 *   - the bearer token is presented per-request and only its SHA-256 is stored,
 *     so a database leak does not yield replayable credentials;
 *   - every non-public route requires a valid, unrevoked, unexpired device;
 *   - OAuth `state` is 32 random bytes, hashed at rest, single-use, and TTL'd,
 *     so it stops doubling as a `user_id` disclosure channel.
 */

const DEVICE_TOKEN_TTL_SECONDS = 60 * 60 * 24 * 365; // 1 year
export const OAUTH_STATE_TTL_SECONDS = 60 * 10; // 10 minutes, per MCP guidance

/**
 * The slice of the Worker environment these helpers need.
 *
 * Declared structurally rather than imported from `./index` so this module has
 * no dependency on the entry point (which imports us).
 */
export interface AuthEnv {
  DB: D1Database;
}

/** Hex-encoded SHA-256 of `input`. */
export async function sha256Hex(input: string): Promise<string> {
  const bytes = new TextEncoder().encode(input);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

/** Cryptographically random hex string of `byteLength` bytes. */
export function randomHex(byteLength: number): string {
  const bytes = crypto.getRandomValues(new Uint8Array(byteLength));
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

/** Constant-time comparison of two equal-length hex strings. */
export function safeEqualHex(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

/**
 * Routes that must stay reachable without a device credential.
 *
 * - `/health` — liveness probe.
 * - `/api/register` — device bootstrap. This is the one route that *creates* a
 *   credential; it cannot require one. It is safe because it only ever writes a
 *   hash of a caller-chosen token for a caller-chosen device_id. An attacker can
 *   squat on a device_id, but gains nothing: they cannot read another device's
 *   row, and OAuth tokens are keyed by user_id, which they still cannot guess
 *   (and no longer need, since the token endpoints now authenticate by
 *   device, not by user_id).
 * - `/oauth/callback` — the provider redirects the *browser* here, which cannot
 *   attach an Authorization header. It is protected instead by the single-use
 *   hashed `state` issued by `/oauth/auth-url`.
 * - `/config/check` — returns only booleans and scope lists, no secrets.
 * - `/models/nlu/*` — public model distribution; the blobs are already public.
 *   (`/models/nlu/publish` keeps its separate admin-token gate.)
 */
export const PUBLIC_ROUTES = new Set([
  "GET /health",
  "POST /api/register",
  "GET /oauth/callback",
  "GET /config/check",
  "GET /models/nlu/latest",
  "GET /models/nlu/download",
]);

export interface DeviceRecord {
  user_id: string;
  device_id: string;
}

/**
 * Resolve the device credential on a request, or return an error Response.
 *
 * Accepts the bearer token from either the `Authorization: Bearer` header or an
 * `X-Nexus-Device-Token` header. The header form exists because some call sites
 * (notably the WebView) cannot set arbitrary headers on a cross-origin request
 * without triggering a preflight; it is equally hashed and equally verified.
 */
export async function authenticateDevice(
  request: Request,
  env: AuthEnv,
  json: (d: unknown, s?: number) => Response,
): Promise<{ device: DeviceRecord } | { error: Response }> {
  const authHeader = request.headers.get("Authorization") || "";
  const headerToken = request.headers.get("X-Nexus-Device-Token") || "";
  const bearer = /^Bearer\s+(.+)$/i.exec(authHeader)?.[1]?.trim();
  const token = bearer || headerToken.trim();

  if (!token) {
    return {
      error: json(
        { error: "unauthorized: missing device credential" },
        401,
      ),
    };
  }

  const tokenHash = await sha256Hex(token);

  // Looked up by hash so the plaintext token is never compared against, stored,
  // or logged. `revoked_at IS NULL` and `expires_at` are part of the predicate so
  // a revoked or expired device cannot authenticate even with a valid token.
  const row = await env.DB.prepare(
    `SELECT user_id, device_id
       FROM user_devices
      WHERE device_token_hash = ?
        AND device_revoked_at IS NULL
        AND (device_token_expires_at IS NULL OR device_token_expires_at > ?)
      LIMIT 1`,
  )
    .bind(tokenHash, Date.now() / 1000)
    .first<DeviceRecord>();

  if (!row) {
    return { error: json({ error: "unauthorized: invalid device credential" }, 401) };
  }

  return { device: row };
}

/**
 * Register (or re-key) a device. Returns the hash to persist.
 *
 * A device may re-register to rotate its token. This is also the migration path
 * for any row created before the hashing change, which would have a plaintext
 * `device_token` instead of a hash.
 */
export async function registerDevice(
  env: AuthEnv,
  userId: string,
  deviceId: string,
  deviceName: string | null,
  os: string | null,
  nowSeconds: number,
): Promise<string> {
  const token = randomHex(32);
  const tokenHash = await sha256Hex(token);
  const expiresAt = nowSeconds + DEVICE_TOKEN_TTL_SECONDS;

  await env.DB.prepare(
    `INSERT INTO user_devices
       (user_id, device_id, device_name, os, device_token_hash,
        device_token_expires_at, device_revoked_at, created_at)
     VALUES (?, ?, ?, ?, ?, ?, NULL, ?)
     ON CONFLICT(user_id, device_id) DO UPDATE SET
       device_name = excluded.device_name,
       os = excluded.os,
       device_token_hash = excluded.device_token_hash,
       device_token_expires_at = excluded.device_token_expires_at,
       device_revoked_at = NULL`,
  )
    .bind(userId, deviceId, deviceName, os, tokenHash, expiresAt, nowSeconds)
    .run();

  // Returned once, to the registering client only. Never persisted in plaintext,
  // never disclosed again.
  return token;
}

/**
 * Mint an OAuth CSRF state bound to a user/provider.
 *
 * Returns the opaque state value to hand to the provider. Only its hash is
 * stored, so the value cannot be recovered from the database — and it carries no
 * user_id, so it no longer leaks one through the redirect URL.
 */
export async function createOAuthState(
  env: AuthEnv,
  userId: string,
  provider: string,
  nowSeconds: number,
): Promise<string> {
  const state = randomHex(32);
  const stateHash = await sha256Hex(state);

  // Opportunistic cleanup keeps the table bounded without a cron trigger.
  await env.DB.prepare(
    `DELETE FROM oauth_states WHERE expires_at < ?`,
  )
    .bind(nowSeconds - 3600)
    .run()
    .catch(() => {
      /* best effort — never fail a login because cleanup failed */
    });

  await env.DB.prepare(
    `INSERT INTO oauth_states (state_hash, user_id, provider, created_at, expires_at, consumed_at)
     VALUES (?, ?, ?, ?, ?, NULL)`,
  )
    .bind(stateHash, userId, provider, nowSeconds, nowSeconds + OAUTH_STATE_TTL_SECONDS)
    .run();

  return state;
}

/**
 * Redeem an OAuth state exactly once.
 *
 * Returns the bound `{user_id, provider}`, or `null` if the state is unknown,
 * expired, or already consumed. The `consumed_at IS NULL` predicate is part of
 * the UPDATE's WHERE clause, so two concurrent callbacks carrying the same state
 * cannot both succeed.
 */
export async function consumeOAuthState(
  env: AuthEnv,
  state: string,
  nowSeconds: number,
): Promise<{ user_id: string; provider: string } | null> {
  if (!state) return null;
  const stateHash = await sha256Hex(state);

  // Mark consumed only if currently unconsumed and unexpired. D1 gives us a
  // single atomic statement, which is what makes redemption one-shot.
  const result = await env.DB.prepare(
    `UPDATE oauth_states
        SET consumed_at = ?
      WHERE state_hash = ?
        AND consumed_at IS NULL
        AND expires_at > ?`,
  )
    .bind(nowSeconds, stateHash, nowSeconds)
    .run();

  if (!result.meta?.changes) return null;

  const row = await env.DB.prepare(
    `SELECT user_id, provider FROM oauth_states WHERE state_hash = ?`,
  )
    .bind(stateHash)
    .first<{ user_id: string; provider: string }>();

  return row ?? null;
}