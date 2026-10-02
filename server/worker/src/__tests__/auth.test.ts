/**
 * Tests for device authentication and OAuth CSRF state.
 *
 * Context (2026-10-02): these guard the fix for a live vulnerability — the
 * Worker returned live OAuth access tokens keyed only on a client-supplied
 * `?user_id=`, with no authentication.
 *
 * The tests use a hand-rolled in-memory D1 double rather than a real D1
 * binding. It implements only the four query shapes these helpers issue, which
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
  PUBLIC_ROUTES,
} from "../auth";

type Row = Record<string, unknown>;

/** Minimal D1 double covering exactly the statements auth.ts issues. */
function makeD1(seed: Row[] = []) {
  const tables = {
    user_devices: [...seed],
    oauth_states: [] as Row[],
  };

  const stmt = (sql: string, binds: unknown[]) => {
    const s = sql.replace(/\s+/g, " ").trim();

    if (s.includes("FROM user_devices") && s.includes("device_token_hash = ?")) {
      const [hash, now] = binds as [string, number];
      return Promise.resolve({
        results: tables.user_devices.filter(
          (r) =>
            r.device_token_hash === hash &&
            r.device_revoked_at == null &&
            (r.device_token_expires_at == null ||
              (r.device_token_expires_at as number) > now),
        ),
        meta: {},
      });
    }

    if (s.includes("INSERT INTO user_devices")) {
      const [userId, deviceId, name, os, hash, expiresAt, createdAt] = binds as (
        string | number | null
      )[];
      const existing = tables.user_devices.find(
        (r) => r.user_id === userId && r.device_id === deviceId,
      );
      const record: Row = {
        user_id: userId,
        device_id: deviceId,
        device_name: name,
        os,
        device_token_hash: hash,
        device_token_expires_at: expiresAt,
        device_revoked_at: null,
        created_at: createdAt,
      };
      if (existing) Object.assign(existing, record);
      else tables.user_devices.push(record);
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
  // this shape is what the real binding provides.
  const handle = (sql: string, binds: unknown[]) => ({
    run: async () => stmt(sql, binds),
    first: async () => (await stmt(sql, binds)).results[0] ?? null,
    all: async () => (await stmt(sql, binds)).results,
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

  it("accepts a registered token and resolves its user", async () => {
    const token = "s3cret-device-token";
    // Timestamps must be wall-clock, not epoch-relative: authenticateDevice
    // compares expiry against Date.now()/1000, so a `now` of 1000 would make a
    // freshly registered device look expired.
    const now = Math.floor(Date.now() / 1000);
    await registerDevice(envFor(d1), "user_abc", "dev_1", "NEXUS", "linux", now);

    // registerDevice mints its own token internally; pin the row's hash to the
    // token we are about to present.
    d1.__tables.user_devices[0].device_token_hash = await sha256Hex(token);

    const res = await authenticateDevice(
      req({ Authorization: `Bearer ${token}` }),
      envFor(d1),
      json,
    );
    expect("error" in res).toBe(false);
    if (!("error" in res)) {
      expect(res.device.user_id).toBe("user_abc");
      expect(res.device.device_id).toBe("dev_1");
    }
  });

  it("accepts the X-Nexus-Device-Token header form", async () => {
    const token = "header-form-token";
    d1.__tables.user_devices.push({
      user_id: "user_h",
      device_id: "dev_h",
      device_name: "NEXUS",
      os: "linux",
      device_token_hash: await sha256Hex(token),
      device_token_expires_at: null,
      device_revoked_at: null,
      created_at: 1000,
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
    d1.__tables.user_devices.push({
      user_id: "user_r",
      device_id: "dev_r",
      device_name: "NEXUS",
      os: "linux",
      device_token_hash: await sha256Hex(token),
      device_token_expires_at: null,
      device_revoked_at: 2000, // revoked
      created_at: 1000,
    });
    const res = await authenticateDevice(
      req({ Authorization: `Bearer ${token}` }),
      envFor(d1),
      json,
    );
    expect("error" in res).toBe(true);
  });

  it("rejects an expired device even with a valid token", async () => {
    const token = "expired-token";
    d1.__tables.user_devices.push({
      user_id: "user_e",
      device_id: "dev_e",
      device_name: "NEXUS",
      os: "linux",
      device_token_hash: await sha256Hex(token),
      device_token_expires_at: 1500, // already past `now`
      device_revoked_at: null,
      created_at: 1000,
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
      "user_1",
      "dev_1",
      "NEXUS",
      "linux",
      1000,
    );
    expect(token).toHaveLength(64); // 32 bytes hex
    const row = d1.__tables.user_devices[0];
    expect(row.device_token_hash).toBe(await sha256Hex(token));
    expect(JSON.stringify(row)).not.toContain(token);
    expect(row).not.toHaveProperty("device_token");
  });

  it("rotates the credential on re-registration of the same device", async () => {
    const d1 = makeD1();
    const first = await registerDevice(envFor(d1), "u", "d", "NEXUS", "linux", 1000);
    const second = await registerDevice(envFor(d1), "u", "d", "NEXUS", "linux", 2000);
    expect(second).not.toBe(first);
    expect(d1.__tables.user_devices).toHaveLength(1);
    expect(d1.__tables.user_devices[0].device_token_hash).toBe(
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
      "GET /config/check",
      "GET /health",
      "GET /models/nlu/download",
      "GET /models/nlu/latest",
      "GET /oauth/callback",
      "POST /api/register",
    ]);
  });
});