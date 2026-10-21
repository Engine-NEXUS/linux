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
 *   - every non-public route requires a valid, unrevoked device;
 *   - OAuth `state` is 32 random bytes, hashed at rest, single-use, and TTL'd,
 *     so it stops doubling as a `user_id` disclosure channel.
 *
 * ── BRANCH ADAPTATION (Linux port J1, 2026-10-20) ────────────────────────────
 *
 * This module was written against a `user_devices` table that the branch
 * dropped in favour of the Feature-88 `devices` table (`schema.sql`: one
 * canonical profile per laptop installation, Worker-issued `profile_id` +
 * `device_id` + a one-time device token whose SHA-256 is the only thing D1
 * keeps). It is re-pointed here rather than copied:
 *
 *   - the credential lives in `devices.device_token_hash` (hashed, as before);
 *   - the OWNER of a device is its `profile_id` — the canonical, Worker-issued
 *     identity. It is the only key a gated route may read or write, and it is
 *     the same key `handleTranscript` already uses for canonical devices
 *     (`credentialKey = gate.legacy ? userId : gate.profileId`);
 *   - `profiles.provision_hint` is deliberately NOT used as an owner key: it is
 *     a client-chosen hint, so anybody could claim a profile carrying somebody
 *     else's hint and inherit that person's tokens. The hint stays audit-only.
 *
 * The hash + constant-time compare are the Feature-88 primitives in
 * `./identity` — re-used rather than re-implemented, so the Worker has exactly
 * one implementation of both.
 */

import { sha256Hex, timingSafeEqualHex } from "./identity";

export { sha256Hex };

/** Constant-time comparison of two equal-length hex strings. */
export const safeEqualHex = timingSafeEqualHex;

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

/** Cryptographically random hex string of `byteLength` bytes. */
export function randomHex(byteLength: number): string {
  const bytes = crypto.getRandomValues(new Uint8Array(byteLength));
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

/**
 * Routes that must stay reachable without a device credential.
 *
 * - `/health` — liveness probe.
 * - `/v1/profiles/claim` — device bootstrap. This is the one route that
 *   *creates* a credential; it cannot require one. It is safe because it only
 *   ever writes a hash of a caller-chosen token for a caller-chosen device_id.
 *   An attacker can squat on a device_id, but gains nothing: they cannot read
 *   another device's row, and the OAuth token endpoints authenticate by
 *   device, never by a client-supplied user id.
 * - `/oauth/callback` — the provider redirects the *browser* here, which cannot
 *   attach an Authorization header. It is protected instead by the single-use
 *   hashed `state` issued by `/oauth/auth-url`.
 * - `/models/nlu/latest` + `/models/nlu/download` — public model distribution;
 *   the blobs are already public and the family client polls them
 *   unauthenticated (`src-tauri/src/nlu_update.rs`).
 * - `/models/nlu/publish` — admin-token gated inside the handler, so it must
 *   stay reachable by the admin tooling that has no device credential.
 */
export const PUBLIC_ROUTES = new Set([
  "GET /health",
  "POST /v1/profiles/claim",
  "GET /oauth/callback",
  "GET /models/nlu/latest",
  "GET /models/nlu/download",
  "POST /models/nlu/publish",
]);

/**
 * Route prefixes that carry their own credential and therefore never need a
 * device bearer. The admin plane authenticates with the admin token
 * (`NEXUS_ADMIN_IDENTITY_TOKEN`), not with a device.
 */
export const SELF_AUTH_ROUTE_PREFIXES = ["/v1/admin/"];

/** `"GET /health"` — the key form used by {@link PUBLIC_ROUTES}. */
export function routeKey(method: string, path: string): string {
  return `${method.toUpperCase()} ${path}`;
}

/**
 * True when the route may be reached without a device credential.
 *
 * Everything else is deny-by-default: it needs a valid, unrevoked device.
 */
export function isPublicRoute(method: string, path: string): boolean {
  if (SELF_AUTH_ROUTE_PREFIXES.some((prefix) => path.startsWith(prefix))) return true;
  return PUBLIC_ROUTES.has(routeKey(method, path));
}

export interface DeviceRecord {
  /**
   * The canonical owner of this device — the Feature-88 `profile_id` of the
   * `devices` row, renamed.
   *
   * This is the ONLY identity a gated route may use. It is Worker-issued, so
   * it cannot be chosen, guessed or forged by the caller.
   */
  user_id: string;
  /** The device row id (`dev_…`). */
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
  // or logged. The Feature-88 `devices` row carries the credential; `status`
  // and `revoked_at` are part of the predicate so a revoked device cannot
  // authenticate even with a valid token.
  //
  // The owner is the device's `profile_id`. The column is selected under its
  // real name and renamed in TypeScript below, next to `DeviceRecord` — not
  // through a SQL alias — so a wrong column name is a compile error instead of
  // a silent `undefined` owner. (An alias here once hid exactly that: the
  // hand-rolled D1 doubles return the table's real columns, and the owner
  // came back undefined.)
  const row = await env.DB.prepare(
    `SELECT d.profile_id, d.device_id, d.device_token_hash
       FROM devices d
      WHERE d.device_token_hash = ?
        AND d.status = 'active'
        AND d.revoked_at IS NULL
      LIMIT 1`,
  )
    .bind(tokenHash)
    .first<{ profile_id: string; device_id: string; device_token_hash: string }>();

  if (!row) {
    return { error: json({ error: "unauthorized: invalid device credential" }, 401) };
  }

  // Constant-time re-check of the value the row was matched on, so the
  // comparison never short-circuits on the first differing character.
  if (!timingSafeEqualHex(row.device_token_hash, tokenHash)) {
    return { error: json({ error: "unauthorized: invalid device credential" }, 401) };
  }

  return { device: { user_id: row.profile_id, device_id: row.device_id } };
}

/**
 * The owner key a gated route may use — always the authenticated device's
 * owner, never a client-supplied id.
 *
 * Transition rule: during the grace period a client may still send a
 * `user_id` (query parameter or body field) alongside its device bearer. The
 * id is accepted but IGNORED — the device credential is the only thing that
 * decides who the caller is. A request that carries only a `user_id` never
 * reaches this function: the gate already refused it with 401.
 */
export function resolveAuthUserId(device: DeviceRecord, requestedUserId: string | null): string {
  void requestedUserId;
  return device.user_id;
}

/**
 * Register (or re-key) a device against the Feature-88 `devices` table.
 * Returns the plaintext token once, to the registering client only.
 *
 * A device may re-register to rotate its token. The credential is stored only
 * as its SHA-256 hash; the plaintext never touches the database.
 */
export async function registerDevice(
  env: AuthEnv,
  profileId: string,
  deviceId: string,
  deviceName: string | null,
  os: string | null,
  nowSeconds: number,
): Promise<string> {
  const token = randomHex(32);
  const tokenHash = await sha256Hex(token);

  await env.DB.prepare(
    `INSERT INTO devices
       (device_id, profile_id, device_token_hash, device_name, os, app_version,
        status, created_at, last_seen_at, revoked_at)
     VALUES (?, ?, ?, ?, ?, NULL, 'active', ?, ?, NULL)
     ON CONFLICT(device_id) DO UPDATE SET
       profile_id = excluded.profile_id,
       device_name = excluded.device_name,
       os = excluded.os,
       device_token_hash = excluded.device_token_hash,
       status = 'active',
       last_seen_at = excluded.last_seen_at,
       revoked_at = NULL`,
  )
    .bind(deviceId, profileId, tokenHash, deviceName, os, nowSeconds, nowSeconds)
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
 * user id, so it no longer leaks one through the redirect URL. The user is
 * bound to the state ROW, which is what the callback redeems.
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

// ---- CORS ----

/**
 * The origins the Worker's own clients use.
 *
 * - `tauri://localhost` — packaged Tauri app on Windows.
 * - `http(s)://tauri.localhost` — packaged Tauri app on Linux/macOS.
 */
export const DEFAULT_ALLOWED_ORIGINS = [
  "tauri://localhost",
  "http://tauri.localhost",
  "https://tauri.localhost",
];

/** Loopback dev origins (`vite dev`, `tauri dev`) — never a public origin. */
const LOOPBACK_ORIGIN = /^https?:\/\/(localhost|127\.0\.0\.1)(:\d+)?$/;

/**
 * The allowlist: the built-in origins plus any extra ones configured through
 * the `ALLOWED_ORIGINS` environment variable (comma-separated).
 */
export function parseAllowedOrigins(raw?: string | null): string[] {
  const extra = (raw || "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  return [...DEFAULT_ALLOWED_ORIGINS, ...extra];
}

/** True when the origin may be reflected back in an `Access-Control-Allow-Origin`. */
export function isAllowedOrigin(origin: string, allowed: string[]): boolean {
  if (!origin) return false;
  if (allowed.includes(origin)) return true;
  return LOOPBACK_ORIGIN.test(origin);
}

/**
 * CORS headers for a response.
 *
 * The origin is REFLECTED — never `*` — and only when it is on the allowlist.
 * `*` let any web page read the token routes cross-origin. `Vary: Origin` is
 * always set so a shared cache never serves one origin's response to another.
 */
export function corsHeaders(
  origin: string | null,
  allowed: string[],
): Record<string, string> {
  const headers: Record<string, string> = { Vary: "Origin" };
  if (origin && isAllowedOrigin(origin, allowed)) {
    headers["Access-Control-Allow-Origin"] = origin;
  }
  return headers;
}
