/**
 * Feature 88 — canonical laptop identity + admin-gated entitlement.
 *
 * Design (docs/research/worker-identity/01-...architecture-2026-10-03.md):
 * - One canonical profile per laptop installation. The Worker issues
 *   `profile_id` / `device_id` / device token. Client-generated UUIDs are
 *   provisional hints only (stored as `provision_hint`, audit only).
 * - Default-deny: unknown / pending / suspended / revoked / expired /
 *   not-entitled profiles never reach Workers AI or external LLM relays.
 * - D1 stores identity + entitlement + operational counters ONLY.
 *   No chat history, no transcripts, no provider API keys.
 * - Device token: plaintext shown exactly once at claim time; D1 keeps
 *   only its SHA-256 hash. Raw token lives in the laptop's OS keyring.
 * - All lookups fail closed.
 *
 * Business logic (claimProfile / resolveProfile / rotateDeviceToken /
 * selfRevokeDevice) is written against the `IdentityStore` interface so
 * the whole decision table is unit-testable with an in-memory store;
 * `d1Store(env)` is the production D1 implementation.
 */

// ---- Types ----

export interface ProfileRow {
  profile_id: string;
  human_label: string | null;
  status: "pending" | "approved" | "suspended" | "revoked";
  quota_tier: string | null;
  created_at: number;
  approved_at: number | null;
  approved_by: string | null;
  expires_at: number | null;
  last_seen_at: number | null;
  provision_hint: string | null;
}

export interface DeviceRow {
  device_id: string;
  profile_id: string;
  device_token_hash: string;
  device_name: string | null;
  os: string | null;
  app_version: string | null;
  status: "active" | "revoked";
  created_at: number;
  last_seen_at: number | null;
  revoked_at: number | null;
}

export interface EntitlementRow {
  profile_id: string;
  scope: string;
  allowed: number;
  quota_tier: string | null;
  granted_by: string;
  granted_at: number;
  expires_at: number | null;
}

/** Scopes a transcript turn consumes. A profile must be entitled to ALL. */
export const TRANSCRIPT_SCOPES = ["worker_ai", "llm_relay"] as const;

export type MigrationMode = "off" | "strict" | "revoke-legacy";

export type EntitlementCode =
  | "unknown_profile"
  | "pending"
  | "suspended"
  | "revoked"
  | "expired"
  | "bad_token"
  | "not_entitled"
  | "lookup_failed";

export type EntitlementResult =
  | { ok: true; profileId: string; deviceId: string; quotaTier: string; legacy?: boolean }
  | { ok: false; code: EntitlementCode; status?: string };

export type ClaimResult =
  | { ok: true; profile_id: string; device_id: string; status: string; device_token?: string }
  | { ok: false; code: "rate_limited" | "pending_cap" | "bad_credentials" | "not_found" };

export interface ClaimBody {
  profile_id?: string;          // re-claim path (must pair with device_token)
  device_id?: string;
  device_token?: string;
  rotate?: boolean;
  reinstall?: boolean;          // explicit: create a brand-new profile
  provisional_user_id?: string; // hint only
  device_name?: string;
  os?: string;
  app_version?: string;
}

// ---- Store interface ----

export interface IdentityStore {
  getProfile(profileId: string): Promise<ProfileRow | null>;
  getLatestProfileByHint(hint: string): Promise<ProfileRow | null>;
  insertProfile(row: ProfileRow): Promise<void>;
  updateProfile(profileId: string, patch: Partial<ProfileRow>): Promise<void>;
  countPending(): Promise<number>;
  touchProfile(profileId: string, at: number): Promise<void>;
  getDevice(deviceId: string): Promise<DeviceRow | null>;
  getDeviceForProfile(deviceId: string, profileId: string): Promise<DeviceRow | null>;
  listDevicesForProfile(profileId: string): Promise<DeviceRow[]>;
  insertDevice(row: DeviceRow): Promise<void>;
  updateDeviceTokenHash(deviceId: string, hash: string, at: number): Promise<void>;
  revokeDevice(deviceId: string, at: number): Promise<void>;
  insertEvent(profileId: string, deviceId: string | null, event: string, detail: string | null, at: number): Promise<void>;
  countRecentRegistrations(ip: string, since: number): Promise<number>;
  getEntitlement(profileId: string, scope: string): Promise<EntitlementRow | null>;
  listEntitlements(profileId: string): Promise<EntitlementRow[]>;
  upsertEntitlement(row: EntitlementRow): Promise<void>;
  getMigration(legacyUserId: string): Promise<{ profile_id: string } | null>;
  insertMigration(legacyUserId: string, profileId: string, at: number, autoApproved: number): Promise<void>;
  /** Admin-only scan used by admin.ts; optional because in-memory test stores don't implement it. */
  __listProfiles?(status?: string): Promise<ProfileRow[]>;
}

// ---- D1 implementation ----

export function d1Store(env: { DB: D1Database }): IdentityStore {
  return {
    async getProfile(profileId) {
      return (await env.DB.prepare(
        "SELECT * FROM profiles WHERE profile_id = ?"
      ).bind(profileId).first()) as ProfileRow | null;
    },
    async getLatestProfileByHint(hint) {
      return (await env.DB.prepare(
        "SELECT * FROM profiles WHERE provision_hint = ? ORDER BY created_at DESC LIMIT 1"
      ).bind(hint).first()) as ProfileRow | null;
    },
    async insertProfile(row) {
      await env.DB.prepare(
        "INSERT INTO profiles (profile_id, human_label, status, quota_tier, created_at, approved_at, approved_by, expires_at, last_seen_at, provision_hint) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
      ).bind(row.profile_id, row.human_label, row.status, row.quota_tier, row.created_at,
        row.approved_at, row.approved_by, row.expires_at, row.last_seen_at, row.provision_hint).run();
    },
    async updateProfile(profileId, patch) {
      const cols: string[] = [];
      const args: unknown[] = [];
      // Only identity-administration columns are patchable.
      const allowed: Array<[keyof ProfileRow, string]> = [
        ["human_label", "human_label"], ["status", "status"], ["quota_tier", "quota_tier"],
        ["approved_at", "approved_at"], ["approved_by", "approved_by"], ["expires_at", "expires_at"],
      ];
      for (const [key, col] of allowed) {
        if (key in patch) { cols.push(`${col} = ?`); args.push(patch[key]); }
      }
      if (cols.length === 0) return;
      args.push(profileId);
      await env.DB.prepare(`UPDATE profiles SET ${cols.join(", ")} WHERE profile_id = ?`)
        .bind(...args).run();
    },
    async countPending() {
      const row = await env.DB.prepare(
        "SELECT COUNT(*) AS n FROM profiles WHERE status = 'pending'"
      ).first();
      return (row?.n as number) || 0;
    },
    async touchProfile(profileId, at) {
      await env.DB.prepare(
        "UPDATE profiles SET last_seen_at = ? WHERE profile_id = ?"
      ).bind(at, profileId).run();
    },
    async getDevice(deviceId) {
      return (await env.DB.prepare(
        "SELECT * FROM devices WHERE device_id = ?"
      ).bind(deviceId).first()) as DeviceRow | null;
    },
    async getDeviceForProfile(deviceId, profileId) {
      return (await env.DB.prepare(
        "SELECT * FROM devices WHERE device_id = ? AND profile_id = ?"
      ).bind(deviceId, profileId).first()) as DeviceRow | null;
    },
    async listDevicesForProfile(profileId) {
      const result = await env.DB.prepare(
        "SELECT * FROM devices WHERE profile_id = ? ORDER BY created_at ASC"
      ).bind(profileId).all();
      return (result.results || []) as unknown as DeviceRow[];
    },
    async insertDevice(row) {
      await env.DB.prepare(
        "INSERT INTO devices (device_id, profile_id, device_token_hash, device_name, os, app_version, status, created_at, last_seen_at, revoked_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
      ).bind(row.device_id, row.profile_id, row.device_token_hash, row.device_name,
        row.os, row.app_version, row.status, row.created_at, row.last_seen_at, row.revoked_at).run();
    },
    async updateDeviceTokenHash(deviceId, hash, at) {
      await env.DB.prepare(
        "UPDATE devices SET device_token_hash = ?, last_seen_at = ? WHERE device_id = ?"
      ).bind(hash, at, deviceId).run();
    },
    async revokeDevice(deviceId, at) {
      await env.DB.prepare(
        "UPDATE devices SET status = 'revoked', revoked_at = ?, last_seen_at = ? WHERE device_id = ?"
      ).bind(at, at, deviceId).run();
    },
    async insertEvent(profileId, deviceId, event, detail, at) {
      await env.DB.prepare(
        "INSERT INTO profile_events (profile_id, device_id, event, detail, at) VALUES (?, ?, ?, ?, ?)"
      ).bind(profileId, deviceId, event, detail, at).run();
    },
    async countRecentRegistrations(ip, since) {
      const pattern = `%\"ip\":\"${escapeLike(ip)}\"%`;
      const row = await env.DB.prepare(
        "SELECT COUNT(*) AS n FROM profile_events WHERE event = 'registered' AND detail LIKE ? AND at > ?"
      ).bind(pattern, since).first();
      return (row?.n as number) || 0;
    },
    async getEntitlement(profileId, scope) {
      return (await env.DB.prepare(
        "SELECT * FROM entitlements WHERE profile_id = ? AND scope = ?"
      ).bind(profileId, scope).first()) as EntitlementRow | null;
    },
    async listEntitlements(profileId) {
      const result = await env.DB.prepare(
        "SELECT * FROM entitlements WHERE profile_id = ?"
      ).bind(profileId).all();
      return (result.results || []) as unknown as EntitlementRow[];
    },
    async upsertEntitlement(row) {
      await env.DB.prepare(
        "INSERT OR REPLACE INTO entitlements (profile_id, scope, allowed, quota_tier, granted_by, granted_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?)"
      ).bind(row.profile_id, row.scope, row.allowed, row.quota_tier, row.granted_by,
        row.granted_at, row.expires_at).run();
    },
    async getMigration(legacyUserId) {
      return (await env.DB.prepare(
        "SELECT profile_id FROM identity_migration WHERE legacy_user_id = ?"
      ).bind(legacyUserId).first()) as { profile_id: string } | null;
    },
    async insertMigration(legacyUserId, profileId, at, autoApproved) {
      await env.DB.prepare(
        "INSERT OR REPLACE INTO identity_migration (legacy_user_id, profile_id, migrated_at, auto_approved) VALUES (?, ?, ?, ?)"
      ).bind(legacyUserId, profileId, at, autoApproved).run();
    },
    async __listProfiles(status?: string): Promise<ProfileRow[]> {
      const q = status
        ? "SELECT * FROM profiles WHERE status = ? ORDER BY created_at DESC"
        : "SELECT * FROM profiles ORDER BY created_at DESC";
      const stmt = status ? env.DB.prepare(q).bind(status) : env.DB.prepare(q);
      const res = await stmt.all();
      return (res.results || []) as unknown as ProfileRow[];
    },
  };
}

function escapeLike(s: string): string {
  return s.replace(/[%_\\]/g, "\\$&");
}

// ---- Credential helpers ----

export function randomId(prefix: "prof" | "dev"): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return `${prefix}_${b64url(bytes)}`;
}

/** One-time device token: 32 bytes → base64url (43 chars). */
export function randomToken(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return b64url(bytes);
}

function b64url(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export async function sha256Hex(input: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(input));
  return [...new Uint8Array(digest)].map(b => b.toString(16).padStart(2, "0")).join("");
}

/** Constant-time comparison of two hex strings (equal length assumed for hex). */
export function timingSafeEqualHex(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

// ---- Abuse controls ----

export const CLAIMS_PER_HOUR = 5;
export const PENDING_CAP = 50;

// ---- Claim (registration handshake) ----

/**
 * Idempotent registration:
 * 1. Re-claim with (profile_id, device_id, device_token) → verified →
 *    returns the same profile (rotates the token when `rotate: true`).
 * 2. Retry with only `provisional_user_id` → returns the existing profile's
 *    ids + status WITHOUT a token (prevents duplicate profiles on network
 *    retry; a hijacker who guesses the hint gains nothing — no token).
 * 3. `reinstall: true` → always creates a fresh profile.
 * 4. Otherwise → new pending profile + one-time token.
 */
export async function claimProfile(
  store: IdentityStore,
  body: ClaimBody,
  ip: string,
  now: number,
): Promise<ClaimResult> {
  try {
    // Path 1 — credential-verified re-claim.
    if (body.profile_id && body.device_id && body.device_token) {
      const device = await store.getDeviceForProfile(body.device_id, body.profile_id);
      if (!device || device.status !== "active") return { ok: false, code: "bad_credentials" };
      const hash = await sha256Hex(body.device_token);
      if (!timingSafeEqualHex(hash, device.device_token_hash)) {
        return { ok: false, code: "bad_credentials" };
      }
      await store.touchProfile(body.profile_id, now);
      let token: string | undefined;
      if (body.rotate) {
        token = randomToken();
        await store.updateDeviceTokenHash(body.device_id, await sha256Hex(token), now);
        await store.insertEvent(body.profile_id, body.device_id, "token_rotated", null, now);
      }
      const profile = await store.getProfile(body.profile_id);
      return {
        ok: true,
        profile_id: body.profile_id,
        device_id: body.device_id,
        status: profile?.status || "pending",
        ...(token ? { device_token: token } : {}),
      };
    }

    // Paths 2-4 all create or read profiles — shared abuse gates apply to
    // NEW profile creation only.
    const createsNew = body.reinstall === true || !(body.provisional_user_id &&
      (await store.getLatestProfileByHint(body.provisional_user_id)));

    if (createsNew) {
      const recent = await store.countRecentRegistrations(ip, now - 3600 * 1000);
      if (recent >= CLAIMS_PER_HOUR) return { ok: false, code: "rate_limited" };
      const pending = await store.countPending();
      if (pending >= PENDING_CAP) return { ok: false, code: "pending_cap" };
    }

    // Path 2 — hint-match retry (no credentials): status only, no token.
    if (!body.reinstall && body.provisional_user_id) {
      const existing = await store.getLatestProfileByHint(body.provisional_user_id);
      if (existing) {
        await store.touchProfile(existing.profile_id, now);
        const devices = await store.listDevicesForProfile(existing.profile_id);
        const device = devices.find(d => d.status === "active") || devices[0] || null;
        if (device) {
          return {
            ok: true,
            profile_id: existing.profile_id,
            device_id: device.device_id,
            status: existing.status,
          };
        }
      }
    }

    // Paths 3-4 — fresh profile (explicit reinstall, or first claim).
    const profileId = randomId("prof");
    const deviceId = randomId("dev");
    const token = randomToken();
    const provisionHint = body.provisional_user_id || null;
    await store.insertProfile({
      profile_id: profileId,
      human_label: null,
      status: "pending",
      quota_tier: null,
      created_at: now,
      approved_at: null,
      approved_by: null,
      expires_at: null,
      last_seen_at: now,
      provision_hint: provisionHint,
    });
    await store.insertDevice({
      device_id: deviceId,
      profile_id: profileId,
      device_token_hash: await sha256Hex(token),
      device_name: body.device_name || null,
      os: body.os || null,
      app_version: body.app_version || null,
      status: "active",
      created_at: now,
      last_seen_at: now,
      revoked_at: null,
    });
    await store.insertEvent(profileId, deviceId, "registered", JSON.stringify({ ip }), now);
    return { ok: true, profile_id: profileId, device_id: deviceId, status: "pending", device_token: token };
  } catch (e) {
    console.error("claimProfile error:", e);
    return { ok: false, code: "not_found" };
  }
}

// ---- Entitlement resolution (the gate primitive) ----

export interface ResolveOpts {
  profileId?: string;      // canonical path (new clients)
  deviceId?: string;
  authHeader?: string | null;  // "Bearer <device token>"
  legacyUserId?: string;   // legacy path (pre-identity clients)
  mode: MigrationMode;
  requiredScopes?: string[];   // default TRANSCRIPT_SCOPES
}

/**
 * Resolve the caller's profile + device and evaluate entitlement.
 * This is the primitive the AI gate wraps (W2). Fail-closed throughout.
 */
export async function resolveProfile(
  store: IdentityStore,
  opts: ResolveOpts,
  now: number,
): Promise<EntitlementResult> {
  try {
    // ── Canonical path ──
    if (opts.profileId) {
      const profile = await store.getProfile(opts.profileId);
      if (!profile) return { ok: false, code: "unknown_profile" };
      if (!opts.deviceId) return { ok: false, code: "unknown_profile" };
      const device = await store.getDeviceForProfile(opts.deviceId, opts.profileId);
      if (!device) return { ok: false, code: "unknown_profile" };
      if (device.status === "revoked") return { ok: false, code: "revoked" };

      const token = extractBearer(opts.authHeader);
      if (!token) return { ok: false, code: "bad_token" };
      const hash = await sha256Hex(token);
      if (!timingSafeEqualHex(hash, device.device_token_hash)) {
        return { ok: false, code: "bad_token" };
      }

      if (profile.status === "revoked") return { ok: false, code: "revoked" };
      if (profile.status === "suspended") return { ok: false, code: "suspended" };
      if (profile.status === "pending") return { ok: false, code: "pending" };
      if (profile.expires_at !== null && profile.expires_at !== undefined && now > profile.expires_at) {
        return { ok: false, code: "expired" };
      }

      const scopes = opts.requiredScopes || TRANSCRIPT_SCOPES;
      const grant = await evaluateScopes(store, opts.profileId, scopes, now);
      if (grant === "expired") return { ok: false, code: "expired" };
      if (grant === "missing") return { ok: false, code: "not_entitled", status: "approved" };

      // Fire-and-forget liveness (awaited — store may be in-memory in tests).
      await store.touchProfile(opts.profileId, now);
      return {
        ok: true,
        profileId: opts.profileId,
        deviceId: opts.deviceId,
        quotaTier: profile.quota_tier || "default",
      };
    }

    // ── Legacy path (pre-identity clients) ──
    const legacy = opts.legacyUserId || "";
    if (opts.mode === "off") {
      // Status quo: legacy IDs keep working ungated until the strict flip.
      if (!legacy) return { ok: false, code: "unknown_profile" };
      return { ok: true, profileId: legacy, deviceId: opts.deviceId || "", quotaTier: "default", legacy: true };
    }

    // strict / revoke-legacy: map legacy → canonical, then gate canonically.
    if (!legacy) return { ok: false, code: "unknown_profile" };
    const mapped = await store.getMigration(legacy);
    if (!mapped) return { ok: false, code: "unknown_profile" };
    return resolveProfile(store, {
      ...opts,
      profileId: mapped.profile_id,
      legacyUserId: undefined,
    }, now);
  } catch {
    return { ok: false, code: "lookup_failed" };
  }
}

type ScopeVerdict = "active" | "missing" | "expired";

/**
 * Evaluate the grant chain: an "all" grant covers every scope; otherwise
 * EVERY required scope needs its own active grant. A grant whose
 * expires_at has passed reports "expired" (distinct from "missing") so
 * the resolver can surface the right denial code.
 */
async function evaluateScopes(store: IdentityStore, profileId: string, scopes: readonly string[], now: number): Promise<ScopeVerdict> {
  const all = await store.getEntitlement(profileId, "all");
  if (all && all.allowed === 1) {
    if (isExpiredGrant(all, now)) return "expired";
    return "active";
  }
  let verdict: ScopeVerdict = "active";
  for (const scope of scopes) {
    const ent = await store.getEntitlement(profileId, scope);
    if (!ent || ent.allowed !== 1) {
      // A missing/non-granted scope stays "missing" unless another
      // required grant already proved expiry (expiry is the louder signal).
      if (verdict !== "expired") verdict = "missing";
      continue;
    }
    if (isExpiredGrant(ent, now)) verdict = "expired";
  }
  return verdict;
}

function isExpiredGrant(ent: EntitlementRow, now: number): boolean {
  return ent.expires_at !== null && ent.expires_at !== undefined && now > ent.expires_at;
}

function extractBearer(header: string | null | undefined): string | null {
  if (!header) return null;
  const m = header.match(/^Bearer\s+(\S+)$/i);
  return m ? m[1] : null;
}

// ---- Device lifecycle ----

export type TokenOpResult =
  | { ok: true; device_token?: string }
  | { ok: false; code: "bad_credentials" | "not_found" };

export async function rotateDeviceToken(
  store: IdentityStore,
  profileId: string,
  deviceId: string,
  token: string,
  now: number,
): Promise<TokenOpResult> {
  try {
    const device = await store.getDeviceForProfile(deviceId, profileId);
    if (!device || device.status !== "active") return { ok: false, code: "bad_credentials" };
    const hash = await sha256Hex(token);
    if (!timingSafeEqualHex(hash, device.device_token_hash)) {
      return { ok: false, code: "bad_credentials" };
    }
    const fresh = randomToken();
    await store.updateDeviceTokenHash(deviceId, await sha256Hex(fresh), now);
    await store.insertEvent(profileId, deviceId, "token_rotated", null, now);
    return { ok: true, device_token: fresh };
  } catch {
    return { ok: false, code: "not_found" };
  }
}

export async function selfRevokeDevice(
  store: IdentityStore,
  profileId: string,
  deviceId: string,
  token: string,
  now: number,
): Promise<TokenOpResult> {
  try {
    const device = await store.getDeviceForProfile(deviceId, profileId);
    if (!device || device.status !== "active") return { ok: false, code: "bad_credentials" };
    const hash = await sha256Hex(token);
    if (!timingSafeEqualHex(hash, device.device_token_hash)) {
      return { ok: false, code: "bad_credentials" };
    }
    await store.revokeDevice(deviceId, now);
    await store.insertEvent(profileId, deviceId, "revoked", "self_revoke", now);
    return { ok: true };
  } catch {
    return { ok: false, code: "not_found" };
  }
}
