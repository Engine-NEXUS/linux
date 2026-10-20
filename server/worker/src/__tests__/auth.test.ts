/**
 * Tests for device authentication and OAuth CSRF state.
 *
 * Context (2026-10-02): these guard the fix for a live vulnerability — the
 * Worker returned live OAuth access tokens keyed only on a client-supplied
 * `?user_id=`, with no authentication.
 *
 * BRANCH ADAPTATION (Linux port J1, 2026-10-20): the credential now lives in
 * the Feature-88 `devices` table (one canonical profile per laptop), not in
 * the dropped `user_devices` table, and the owner of a device is its
 * `profile_id`. The hash + constant-time compare are the ones in
 * `./identity` — the Worker has exactly one implementation of both.
 *
 * The tests use a hand-rolled in-memory D1 double rather than a real D1
 * binding. It implements only the query shapes these helpers issue, which
 * is deliberate: if the SQL changes, the double fails loudly rather than
 * silently diverging.
 */

import { describe, it, expect, beforeEach } from "vitest";
import {
  sha256Hex,
  randomHex,
  safeEqualHex,
  authenticateDevice,
  registerDevice,
  createOAuthState,
  consumeOAuthState,
  resolveAuthUserId,
  isPublicRoute,
  routeKey,
  corsHeaders,
  isAllowedOrigin,
  parseAllowedOrigins,
  DEFAULT_ALLOWED_ORIGINS,
  PUBLIC_ROUTES,
  SELF_AUTH_ROUTE_PREFIXES,
} from "../auth";

type Row = Record<string, unknown>;

/** Minimal D1 double covering exactly the statements auth.ts issues. */
function makeD1(seed: Row[] = []) {
  const tables = {
    devices: [...seed],
    oauth_states: [] as Row[],
  };

  const stmt = (sql: string, binds: unknown[]) => {
    const s = sql.replace(/\s+/g, " ").trim();

    if (s.includes("FROM devices") && s.includes("device_token_hash = ?")) {
      const [hash] = binds as [string];
      return Promise.resolve({
        results: tables.devices.filter(
          (r) =>
            r.device_token_hash === hash &&
            r.status === "active" &&
            r.revoked_at == null,
        ),
        meta: {},
      });
    }

    if (s.includes("INSERT INTO devices")) {
      const [deviceId, profileId, hash, name, os, createdAt] = binds as (
        string | number | null
      )[];
      const existing = tables.devices.find((r) => r.device_id === deviceId);
      const record: Row = {
        device_id: deviceId,
        profile_id: profileId,
        device_token_hash: hash,
        device_name: name,
        os,
        app_version: null,
        status: "active",
        created_at: createdAt,
        last_seen_at: createdAt,
        revoked_at: null,
      };
      if (existing) Object.assign(existing, record);
      else tables.devices.push(record);
      return Promise.resolve({ results: [], meta: { changes: 1 } });
    }

    if (s.includes("INSERT INTO oauth_states")) {
      const [hash, userId, provider, createdAt, expiresAt] = binds as (
        string | number
      )[];
      tables.oauth_states.push({
        state_hash: hash,
        user_id: userId,
        provider,
        created_at: createdAt,
        expires_at: expiresAt,
        consumed_at: null,
      });
      return Promise.resolve({ results: [], meta: { changes: 1 } });
    }

    if (s.includes("UPDATE oauth_states") && s.includes("consumed_at = ?")) {
      const [now, hash, now2] = binds as [number, string, number];
      const row = tables.oauth_states.find(
        (r) =>
          r.state_hash === hash &&
          r.consumed_at == null &&
          (r.expires_at as number) > now2,
      );
      if (row) {
        row.consumed_at = now;
        return Promise.resolve({ results: [], meta: { changes: 1 } });
      }
      return Promise.resolve({ results: [], meta: { changes: 0 } });
    }

    if (s.includes("SELECT user_id, provider FROM oauth_states")) {
      const [hash] = binds as [string];
      const row = tables.oauth_states.find((r) => r.state_hash === hash);
      return Promise.resolve({ results: row ? [row] : [], meta: {} });
    }

    if (s.includes("DELETE FROM oauth_states")) {
      const [cutoff] = binds as [number];
      tables.oauth_states = tables.oauth_states.filter(
        (r) => (r.expires_at as number) >= cutoff,
      );
      return Promise.resolve({ results: [], meta: { changes: 1 } });
    }

    throw new Error(`unstubbed SQL in test double: ${s}`);
  };

  // D1's API is `prepare(sql).bind(...args)` -> object with
  // `first()/all()/run()`. Each of those executes the statement. Returning a
  // promise from `bind()` (rather than a statement object) is the easy mistake;
  // this shape is what the real binding provides. `all()` and `run()` resolve
  // to a `D1Result` (`{ results, success, meta }`), never a bare array — a
  // double that diverges from D1 is a trap for the next test that reads
  // `result.results`.
  const handle = (sql: string, binds: unknown[]) => ({
    run: async () => {
      const res = await stmt(sql, binds);
      return { results: res.results, success: true, meta: res.meta };
    },
    first: async () => (await stmt(sql, binds)).results[0] ?? null,
    all: async () => {
      const res = await stmt(sql, binds);
      return { results: res.results, success: true, meta: res.meta };
    },
  });

  return {
    prepare: (sql: string) => ({
      bind: (...binds: unknown[]) => handle(sql, binds),
      run: () => handle(sql, []).run(),
      first: () => handle(sql, []).first(),
      all: () => handle(sql, []).all(),
    }),
    __tables: tables,
  } as unknown as D1Database & { __tables: typeof tables };
}

const json = (d: unknown, s = 200) =>
  new Response(JSON.stringify(d), { status: s });

const envFor = (d1: D1Database) => ({ DB: d1 }) as never;

function req(headers: Record<string, string> = {}): Request {
  return new Request("https://worker.example/x", { headers });
}

describe("sha256Hex", () => {
  it("matches the known SHA-256 of the empty string", async () => {
    expect(await sha256Hex("")).toBe(
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
  });

  it("matches the known SHA-256 of 'abc'", async () => {
    expect(await sha256Hex("abc")).toBe(
      "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
  });

  it("is stable and differs for different inputs", async () => {
    expect(await sha256Hex("x")).toBe(await sha256Hex("x"));
    expect(await sha256Hex("x")).not.toBe(await sha256Hex("y"));
  });
});

describe("randomHex", () => {
  it("returns 2 hex chars per byte", () => {
    expect(randomHex(32)).toHaveLength(64);
    expect(randomHex(16)).toHaveLength(32);
  });

  it("does not repeat across calls", () => {
    const seen = new Set(Array.from({ length: 50 }, () => randomHex(32)));
    expect(seen.size).toBe(50);
  });
});

describe("safeEqualHex", () => {
  it("rejects length mismatches", () => {
    expect(safeEqualHex("ab", "abc")).toBe(false);
  });
  it("accepts equal strings and rejects differing ones", () => {
    expect(safeEqualHex("deadbeef", "deadbeef")).toBe(true);
    expect(safeEqualHex("deadbeef", "deadbeee")).toBe(false);
  });
});

describe("authenticateDevice", () => {
  let d1: ReturnType<typeof makeD1>;

  beforeEach(() => {
    d1 = makeD1();
  });

  it("rejects a request with no credential at all", async () => {
    const res = await authenticateDevice(req(), envFor(d1), json);
    expect("error" in res).toBe(true);
    if ("error" in res) expect(res.error.status).toBe(401);
  });

  it("rejects an unknown token", async () => {
    const res = await authenticateDevice(
      req({ Authorization: "Bearer not-a-real-token" }),
      envFor(d1),
      json,
    );
    expect("error" in res).toBe(true);
  });

  it("accepts a registered token and resolves its owner", async () => {
    const token = "s3cret-device-token";
    const now = Math.floor(Date.now() / 1000);
    await registerDevice(envFor(d1), "prof_owner", "dev_1", "NEXUS", "linux", now);

    // registerDevice mints its own token internally; pin the row's hash to the
    // token we are about to present.
    d1.__tables.devices[0].device_token_hash = await sha256Hex(token);

    const res = await authenticateDevice(
      req({ Authorization: `Bearer ${token}` }),
      envFor(d1),
      json,
    );
    expect("error" in res).toBe(false);
    if (!("error" in res)) {
      // The owner is the canonical profile id — never a client-supplied id.
      expect(res.device.user_id).toBe("prof_owner");
      expect(res.device.device_id).toBe("dev_1");
    }
  });

  it("accepts the X-Nexus-Device-Token header form", async () => {
    const token = "header-form-token";
    d1.__tables.devices.push({
      device_id: "dev_h",
      profile_id: "prof_h",
      device_token_hash: await sha256Hex(token),
      device_name: "NEXUS",
      os: "linux",
      app_version: null,
      status: "active",
      created_at: 1000,
      last_seen_at: 1000,
      revoked_at: null,
    });
    const res = await authenticateDevice(
      req({ "X-Nexus-Device-Token": token }),
      envFor(d1),
      json,
    );
    expect("error" in res).toBe(false);
  });

  it("rejects a revoked device even with a valid token", async () => {
    const token = "revoked-token";
    d1.__tables.devices.push({
      device_id: "dev_r",
      profile_id: "prof_r",
      device_token_hash: await sha256Hex(token),
      device_name: "NEXUS",
      os: "linux",
      app_version: null,
      status: "revoked",
      created_at: 1000,
      last_seen_at: 1000,
      revoked_at: 2000, // revoked
    });
    const res = await authenticateDevice(
      req({ Authorization: `Bearer ${token}` }),
      envFor(d1),
      json,
    );
    expect("error" in res).toBe(true);
  });

  it("rejects a device whose status is not active", async () => {
    const token = "pending-token";
    d1.__tables.devices.push({
      device_id: "dev_p",
      profile_id: "prof_p",
      device_token_hash: await sha256Hex(token),
      device_name: "NEXUS",
      os: "linux",
      app_version: null,
      status: "suspended",
      created_at: 1000,
      last_seen_at: 1000,
      revoked_at: null,
    });
    const res = await authenticateDevice(
      req({ Authorization: `Bearer ${token}` }),
      envFor(d1),
      json,
    );
    expect("error" in res).toBe(true);
  });

  it("ignores a bearer value that is not a real device token", async () => {
    // The old vulnerability, restated as a test: an attacker-supplied user id
    // in a query string is NOT a credential. Auth is header-only.
    const res = await authenticateDevice(
      req({ Authorization: "Bearer user_victim" }),
      envFor(d1),
      json,
    );
    expect("error" in res).toBe(true);
  });
});

describe("registerDevice", () => {
  it("stores only a hash, never the plaintext token", async () => {
    const d1 = makeD1();
    const token = await registerDevice(
      envFor(d1),
      "prof_1",
      "dev_1",
      "NEXUS",
      "linux",
      1000,
    );
    expect(token).toHaveLength(64); // 32 bytes hex
    const row = d1.__tables.devices[0];
    expect(row.device_token_hash).toBe(await sha256Hex(token));
    expect(JSON.stringify(row)).not.toContain(token);
    expect(row).not.toHaveProperty("device_token");
    expect(row.profile_id).toBe("prof_1");
    expect(row.status).toBe("active");
  });

  it("rotates the credential on re-registration of the same device", async () => {
    const d1 = makeD1();
    const first = await registerDevice(envFor(d1), "prof", "dev", "NEXUS", "linux", 1000);
    const second = await registerDevice(envFor(d1), "prof", "dev", "NEXUS", "linux", 2000);
    expect(second).not.toBe(first);
    expect(d1.__tables.devices).toHaveLength(1);
    expect(d1.__tables.devices[0].device_token_hash).toBe(
      await sha256Hex(second),
    );
  });
});

describe("OAuth state", () => {
  it("produces 32 random bytes and does not leak the user id", async () => {
    const d1 = makeD1();
    const state = await createOAuthState(envFor(d1), "user_secret_id", "google", 1000);
    expect(state).toHaveLength(64);
    expect(state).not.toContain("user_secret_id");
    expect(state).not.toContain("google");
    expect(state).not.toContain(":");
  });

  it("stores only the hash of the state", async () => {
    const d1 = makeD1();
    const state = await createOAuthState(envFor(d1), "u", "github", 1000);
    const row = d1.__tables.oauth_states[0];
    expect(row.state_hash).toBe(await sha256Hex(state));
    expect(JSON.stringify(row)).not.toContain(state);
  });

  it("binds the provider and user for redemption", async () => {
    const d1 = makeD1();
    const state = await createOAuthState(envFor(d1), "user_x", "google", 1000);
    const redeemed = await consumeOAuthState(envFor(d1), state, 1001);
    expect(redeemed).not.toBeNull();
    expect(redeemed?.user_id).toBe("user_x");
    expect(redeemed?.provider).toBe("google");
  });

  it("is single-use — a second redemption fails", async () => {
    const d1 = makeD1();
    const state = await createOAuthState(envFor(d1), "u", "github", 1000);
    expect(await consumeOAuthState(envFor(d1), state, 1001)).not.toBeNull();
    expect(await consumeOAuthState(envFor(d1), state, 1002)).toBeNull();
  });

  it("refuses an expired state", async () => {
    const d1 = makeD1();
    const state = await createOAuthState(envFor(d1), "u", "github", 1000);
    // 10 minutes later, past the TTL.
    expect(await consumeOAuthState(envFor(d1), state, 1000 + 601)).toBeNull();
  });

  it("refuses an unknown or empty state", async () => {
    const d1 = makeD1();
    expect(await consumeOAuthState(envFor(d1), "deadbeef", 1000)).toBeNull();
    expect(await consumeOAuthState(envFor(d1), "", 1000)).toBeNull();
  });

  it("does not accept a legacy 'provider:userId' state", async () => {
    // This is the exact format the vulnerability allowed. It must not redeem.
    const d1 = makeD1();
    expect(await consumeOAuthState(envFor(d1), "google:user_victim", 1000)).toBeNull();
  });

  it("two concurrent redemptions of one state cannot both succeed", async () => {
    const d1 = makeD1();
    const state = await createOAuthState(envFor(d1), "u", "github", 1000);
    const [a, b] = await Promise.all([
      consumeOAuthState(envFor(d1), state, 1001),
      consumeOAuthState(envFor(d1), state, 1001),
    ]);
    expect([a, b].filter(Boolean)).toHaveLength(1);
  });
});

describe("resolveAuthUserId", () => {
  const device = { user_id: "prof_real", device_id: "dev_1" };

  it("returns the device's owner", () => {
    expect(resolveAuthUserId(device, null)).toBe("prof_real");
  });

  it("ignores a forged user id supplied by the caller", () => {
    expect(resolveAuthUserId(device, "user_victim")).toBe("prof_real");
    expect(resolveAuthUserId(device, "")).toBe("prof_real");
  });
});

describe("route classification", () => {
  it("builds a METHOD PATH key", () => {
    expect(routeKey("get", "/health")).toBe("GET /health");
  });

  it("keeps the bootstrap + public routes reachable", () => {
    expect(isPublicRoute("GET", "/health")).toBe(true);
    expect(isPublicRoute("POST", "/v1/profiles/claim")).toBe(true);
    expect(isPublicRoute("GET", "/oauth/callback")).toBe(true);
    expect(isPublicRoute("POST", "/models/nlu/publish")).toBe(true);
    expect(isPublicRoute("GET", "/models/nlu/latest")).toBe(true);
    expect(isPublicRoute("GET", "/models/nlu/download")).toBe(true);
  });

  it("gates every credential-bearing route", () => {
    expect(isPublicRoute("GET", "/oauth/google-token")).toBe(false);
    expect(isPublicRoute("GET", "/oauth/github-token")).toBe(false);
    expect(isPublicRoute("GET", "/oauth/swiggy-token")).toBe(false);
    expect(isPublicRoute("GET", "/oauth/status")).toBe(false);
    expect(isPublicRoute("DELETE", "/oauth/disconnect")).toBe(false);
    expect(isPublicRoute("GET", "/oauth/auth-url")).toBe(false);
    expect(isPublicRoute("POST", "/oauth/exchange")).toBe(false);
    expect(isPublicRoute("POST", "/apikeys/add")).toBe(false);
    expect(isPublicRoute("DELETE", "/apikeys/remove")).toBe(false);
    expect(isPublicRoute("GET", "/apikeys/list")).toBe(false);
    expect(isPublicRoute("POST", "/")).toBe(false);
    expect(isPublicRoute("GET", "/v1/profiles/me")).toBe(false);
  });

  it("does not treat a different method on a public path as public", () => {
    expect(isPublicRoute("POST", "/health")).toBe(false);
    expect(isPublicRoute("GET", "/v1/profiles/claim")).toBe(false);
  });

  it("leaves the self-authenticated admin plane alone", () => {
    for (const prefix of SELF_AUTH_ROUTE_PREFIXES) {
      expect(isPublicRoute("GET", `${prefix}profiles/pending`)).toBe(true);
    }
  });
});

describe("PUBLIC_ROUTES", () => {
  it("does not expose any route that returns an OAuth token", () => {
    for (const entry of PUBLIC_ROUTES) {
      expect(entry).not.toContain("token");
    }
  });

  it("leaves the token endpoints out of the allowlist", () => {
    for (const entry of PUBLIC_ROUTES) {
      expect(entry).not.toMatch(/oauth\/(github|google|swiggy)-token/);
    }
  });

  it("keeps exactly the routes that must bootstrap or are public", () => {
    expect([...PUBLIC_ROUTES].sort()).toEqual([
      "GET /health",
      "GET /models/nlu/download",
      "GET /models/nlu/latest",
      "GET /oauth/callback",
      "POST /models/nlu/publish",
      "POST /v1/profiles/claim",
    ]);
  });
});

describe("CORS", () => {
  it("parses the configured origins on top of the built-in ones", () => {
    const allowed = parseAllowedOrigins("https://app.example, https://other.example");
    expect(allowed).toContain("https://app.example");
    expect(allowed).toContain("https://other.example");
    for (const origin of DEFAULT_ALLOWED_ORIGINS) expect(allowed).toContain(origin);
  });

  it("ignores blank entries and an unset variable", () => {
    expect(parseAllowedOrigins(" , ")).toEqual(DEFAULT_ALLOWED_ORIGINS);
    expect(parseAllowedOrigins(undefined)).toEqual(DEFAULT_ALLOWED_ORIGINS);
  });

  it("allows the packaged app origins and loopback dev origins", () => {
    const allowed = parseAllowedOrigins(undefined);
    expect(isAllowedOrigin("tauri://localhost", allowed)).toBe(true);
    expect(isAllowedOrigin("http://tauri.localhost", allowed)).toBe(true);
    expect(isAllowedOrigin("http://localhost:5173", allowed)).toBe(true);
    expect(isAllowedOrigin("http://127.0.0.1:1420", allowed)).toBe(true);
  });

  it("refuses an unknown origin", () => {
    const allowed = parseAllowedOrigins(undefined);
    expect(isAllowedOrigin("https://evil.example", allowed)).toBe(false);
    expect(isAllowedOrigin("", allowed)).toBe(false);
  });

  it("reflects an allowed origin and always varies on Origin", () => {
    const allowed = parseAllowedOrigins(undefined);
    const headers = corsHeaders("tauri://localhost", allowed);
    expect(headers["Access-Control-Allow-Origin"]).toBe("tauri://localhost");
    expect(headers["Vary"]).toBe("Origin");
  });

  it("never sends a wildcard", () => {
    const allowed = parseAllowedOrigins(undefined);
    for (const origin of ["https://evil.example", null]) {
      expect(corsHeaders(origin, allowed)["Access-Control-Allow-Origin"]).toBeUndefined();
    }
  });
});
