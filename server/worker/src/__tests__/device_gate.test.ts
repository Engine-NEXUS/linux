/**
 * Route-level tests for the deny-by-default device gate (Linux port J1).
 *
 * These exercise the Worker entry point itself — not just the helpers — so
 * the claim "every non-public route needs an authenticated device" is checked
 * against the real router.
 *
 * The vulnerability being closed: `GET /oauth/google-token?user_id=X` returned
 * a live Gmail+Calendar+Drive+Contacts token to anybody who supplied a string.
 * The device bearer is now the only credential, and the owner of a request is
 * always the device's canonical profile — never a client-supplied id.
 *
 * The D1 double implements only the statements these routes issue. Anything
 * else throws, so a query that changes shape fails loudly instead of silently
 * diverging from the real database.
 */

import { describe, it, expect, beforeEach } from "vitest";
import worker from "../index";
import { sha256Hex } from "../auth";

type Row = Record<string, unknown>;

const OWNER = "prof_owner";
const VICTIM = "user_victim";
const DEVICE_TOKEN = "device-bearer-token-abc";
const DEVICE_ID = "dev_owner";

interface Tables {
  devices: Row[];
  oauth_tokens: Row[];
  api_keys: Row[];
  oauth_states: Row[];
}

function makeD1(seed: Partial<Tables> = {}) {
  const tables: Tables = {
    devices: seed.devices ?? [],
    oauth_tokens: seed.oauth_tokens ?? [],
    api_keys: seed.api_keys ?? [],
    oauth_states: seed.oauth_states ?? [],
  };

  const stmt = (sql: string, binds: unknown[]) => {
    const s = sql.replace(/\s+/g, " ").trim();

    // ── device gate ──
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

    // ── oauth token reads ──
    if (s.startsWith("SELECT access_token, refresh_token, expires_at FROM oauth_tokens")) {
      const [userId] = binds as [string];
      // The provider is a literal in the SQL, not a bind.
      const provider = (s.match(/provider = '([^']+)'/) || [])[1] || null;
      return Promise.resolve({
        results: tables.oauth_tokens.filter(
          (r) => r.user_id === userId && (!provider || r.provider === provider),
        ),
        meta: {},
      });
    }

    if (s.startsWith("SELECT provider, expires_at, scopes, refresh_token FROM oauth_tokens")) {
      const [userId] = binds as [string];
      return Promise.resolve({
        results: tables.oauth_tokens.filter((r) => r.user_id === userId),
        meta: {},
      });
    }

    if (s.startsWith("DELETE FROM oauth_tokens")) {
      const [userId, provider] = binds as [string, string];
      tables.oauth_tokens = tables.oauth_tokens.filter(
        (r) => !(r.user_id === userId && r.provider === provider),
      );
      return Promise.resolve({ results: [], meta: { changes: 1 } });
    }

    if (s.startsWith("INSERT OR REPLACE INTO oauth_tokens")) {
      const [userId, provider, accessToken, refreshToken, expiresAt, scopes, accountId, createdAt] =
        binds as (string | number | null)[];
      tables.oauth_tokens = tables.oauth_tokens.filter(
        (r) => !(r.user_id === userId && r.provider === provider),
      );
      tables.oauth_tokens.push({
        user_id: userId,
        provider,
        access_token: accessToken,
        refresh_token: refreshToken,
        expires_at: expiresAt,
        scopes,
        account_id: accountId,
        created_at: createdAt,
      });
      return Promise.resolve({ results: [], meta: { changes: 1 } });
    }

    // ── api keys ──
    if (s.startsWith("SELECT provider FROM api_keys")) {
      const [userId] = binds as [string];
      return Promise.resolve({
        results: tables.api_keys.filter((r) => r.user_id === userId),
        meta: {},
      });
    }

    if (s.startsWith("INSERT OR REPLACE INTO api_keys")) {
      const [userId, provider, key, createdAt] = binds as (string | number)[];
      tables.api_keys = tables.api_keys.filter(
        (r) => !(r.user_id === userId && r.provider === provider),
      );
      tables.api_keys.push({
        user_id: userId,
        provider,
        key_encrypted: key,
        created_at: createdAt,
      });
      return Promise.resolve({ results: [], meta: { changes: 1 } });
    }

    if (s.startsWith("DELETE FROM api_keys")) {
      const [userId, provider] = binds as [string, string];
      tables.api_keys = tables.api_keys.filter(
        (r) => !(r.user_id === userId && r.provider === provider),
      );
      return Promise.resolve({ results: [], meta: { changes: 1 } });
    }

    // ── oauth state ──
    if (s.startsWith("DELETE FROM oauth_states")) {
      const [cutoff] = binds as [number];
      tables.oauth_states = tables.oauth_states.filter(
        (r) => (r.expires_at as number) >= cutoff,
      );
      return Promise.resolve({ results: [], meta: { changes: 1 } });
    }

    if (s.startsWith("INSERT INTO oauth_states")) {
      const [hash, userId, provider, createdAt, expiresAt] = binds as (string | number)[];
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

    if (s.startsWith("UPDATE oauth_states")) {
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

    if (s.startsWith("SELECT user_id, provider FROM oauth_states")) {
      const [hash] = binds as [string];
      const row = tables.oauth_states.find((r) => r.state_hash === hash);
      return Promise.resolve({ results: row ? [row] : [], meta: {} });
    }

    throw new Error(`unstubbed SQL in test double: ${s}`);
  };

  // Mirrors D1's prepared-statement surface: `all()` and `run()` resolve to a
  // `D1Result` (`{ results, success, meta }`) — NOT a bare array. The handlers
  // read `result.results` (exactly as they do against real D1), so an `all()`
  // that returned the array itself made every list route look empty while the
  // status code was still 200.
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
  } as unknown as D1Database & { __tables: Tables };
}

function makeEnv(d1: D1Database, extra: Record<string, unknown> = {}) {
  return {
    AI: {} as never,
    DB: d1,
    GOOGLE_CLIENT_ID: "google-client",
    GOOGLE_CLIENT_SECRET: "google-secret",
    GITHUB_CLIENT_ID: "github-client",
    GITHUB_CLIENT_SECRET: "github-secret",
    SWIGGY_CLIENT_ID: "swiggy-client",
    SWIGGY_CLIENT_SECRET: "swiggy-secret",
    ...extra,
  } as never;
}

function call(
  env: unknown,
  method: string,
  path: string,
  opts: { token?: string | null; body?: unknown; raw?: string; headers?: Record<string, string> } = {},
): Promise<Response> {
  const headers: Record<string, string> = { ...(opts.headers ?? {}) };
  if (opts.token) headers["Authorization"] = `Bearer ${opts.token}`;
  const init: RequestInit = { method, headers };
  if (opts.raw !== undefined) {
    headers["Content-Type"] = "application/json";
    init.body = opts.raw;
  } else if (opts.body !== undefined) {
    headers["Content-Type"] = "application/json";
    init.body = JSON.stringify(opts.body);
  }
  return worker.fetch(new Request(`https://worker.example${path}`, init), env as never);
}

async function bodyJson(res: Response): Promise<any> {
  return await res.json();
}

describe("device gate — token routes", () => {
  let d1: ReturnType<typeof makeD1>;
  let env: unknown;

  beforeEach(async () => {
    d1 = makeD1({
      devices: [
        {
          device_id: DEVICE_ID,
          profile_id: OWNER,
          device_token_hash: await sha256Hex(DEVICE_TOKEN),
          device_name: "NEXUS",
          os: "linux",
          app_version: null,
          status: "active",
          created_at: 1000,
          last_seen_at: 1000,
          revoked_at: null,
        },
      ],
      oauth_tokens: [
        { user_id: OWNER, provider: "google", access_token: "owner-google", refresh_token: null, expires_at: 0, scopes: "s", account_id: "owner@example.com", created_at: 1 },
        { user_id: OWNER, provider: "github", access_token: "owner-github", refresh_token: null, expires_at: 0, scopes: "s", account_id: "owner", created_at: 1 },
        { user_id: OWNER, provider: "swiggy", access_token: "owner-swiggy", refresh_token: null, expires_at: 0, scopes: "s", account_id: "", created_at: 1 },
        { user_id: VICTIM, provider: "google", access_token: "victim-google", refresh_token: null, expires_at: 0, scopes: "s", account_id: "victim@example.com", created_at: 1 },
        { user_id: VICTIM, provider: "github", access_token: "victim-github", refresh_token: null, expires_at: 0, scopes: "s", account_id: "victim", created_at: 1 },
        { user_id: VICTIM, provider: "swiggy", access_token: "victim-swiggy", refresh_token: null, expires_at: 0, scopes: "s", account_id: "", created_at: 1 },
      ],
    });
    env = makeEnv(d1);
  });

  for (const [path, ownerToken, victimToken] of [
    ["/oauth/google-token", "owner-google", "victim-google"],
    ["/oauth/github-token", "owner-github", "victim-github"],
    ["/oauth/swiggy-token", "owner-swiggy", "victim-swiggy"],
  ] as const) {
    it(`${path} is 401 without a device bearer`, async () => {
      const res = await call(env, "GET", path);
      expect(res.status).toBe(401);
      expect((await bodyJson(res)).error).toContain("missing device credential");
    });

    it(`${path} is 401 with only a user_id and no bearer`, async () => {
      const res = await call(env, "GET", `${path}?user_id=${VICTIM}`);
      expect(res.status).toBe(401);
      const body = await res.text();
      expect(body).not.toContain(victimToken);
    });

    it(`${path} returns the device owner's token`, async () => {
      const res = await call(env, "GET", path, { token: DEVICE_TOKEN });
      expect(res.status).toBe(200);
      expect((await bodyJson(res)).token).toBe(ownerToken);
    });

    it(`${path} ignores a forged user_id`, async () => {
      const res = await call(env, "GET", `${path}?user_id=${VICTIM}`, {
        token: DEVICE_TOKEN,
      });
      expect(res.status).toBe(200);
      const body = await bodyJson(res);
      expect(body.token).toBe(ownerToken);
      expect(body.token).not.toBe(victimToken);
    });

    it(`${path} is 401 for an unknown bearer`, async () => {
      const res = await call(env, "GET", path, { token: "not-a-device" });
      expect(res.status).toBe(401);
    });
  }
});

describe("device gate — status / disconnect / apikeys", () => {
  let d1: ReturnType<typeof makeD1>;
  let env: unknown;

  beforeEach(async () => {
    d1 = makeD1({
      devices: [
        {
          device_id: DEVICE_ID,
          profile_id: OWNER,
          device_token_hash: await sha256Hex(DEVICE_TOKEN),
          device_name: "NEXUS",
          os: "linux",
          app_version: null,
          status: "active",
          created_at: 1000,
          last_seen_at: 1000,
          revoked_at: null,
        },
      ],
      oauth_tokens: [
        { user_id: OWNER, provider: "google", access_token: "owner-google", refresh_token: null, expires_at: 0, scopes: "s", account_id: "", created_at: 1 },
        { user_id: VICTIM, provider: "google", access_token: "victim-google", refresh_token: null, expires_at: 0, scopes: "s", account_id: "", created_at: 1 },
      ],
      api_keys: [
        { user_id: OWNER, provider: "claude", key_encrypted: "eA==", created_at: 1 },
        { user_id: VICTIM, provider: "claude", key_encrypted: "eQ==", created_at: 1 },
      ],
    });
    env = makeEnv(d1);
  });

  it("/oauth/status is 401 without a bearer", async () => {
    const res = await call(env, "GET", `/oauth/status?user_id=${VICTIM}`);
    expect(res.status).toBe(401);
  });

  it("/oauth/status lists only the device owner's providers", async () => {
    const res = await call(env, "GET", "/oauth/status", { token: DEVICE_TOKEN });
    expect(res.status).toBe(200);
    const body = await bodyJson(res);
    expect(Object.keys(body.providers)).toEqual(["google"]);
    expect(body.user_id).toBe(OWNER);
  });

  it("/oauth/disconnect is 401 without a bearer", async () => {
    const res = await call(env, "DELETE", "/oauth/disconnect", {
      body: { user_id: VICTIM, provider: "google" },
    });
    expect(res.status).toBe(401);
  });

  it("/oauth/disconnect ignores a forged user_id and drops the owner's row", async () => {
    const res = await call(env, "DELETE", "/oauth/disconnect", {
      token: DEVICE_TOKEN,
      body: { user_id: VICTIM, provider: "google" },
    });
    expect(res.status).toBe(200);
    expect(d1.__tables.oauth_tokens.some((r) => r.user_id === OWNER)).toBe(false);
    expect(d1.__tables.oauth_tokens.some((r) => r.user_id === VICTIM)).toBe(true);
  });

  it("/apikeys/list is 401 without a bearer", async () => {
    const res = await call(env, "GET", `/apikeys/list?user_id=${VICTIM}`);
    expect(res.status).toBe(401);
  });

  it("/apikeys/list returns only the device owner's providers", async () => {
    const res = await call(env, "GET", "/apikeys/list", { token: DEVICE_TOKEN });
    expect(res.status).toBe(200);
    const body = await bodyJson(res);
    expect(body.providers).toEqual(["claude"]);
    expect(body.user_id).toBe(OWNER);
  });

  it("/apikeys/add is 401 without a bearer", async () => {
    const res = await call(env, "POST", "/apikeys/add", {
      body: { user_id: VICTIM, provider: "devin", api_key: "k" },
    });
    expect(res.status).toBe(401);
  });

  it("/apikeys/add stores under the device owner, not the supplied id", async () => {
    const res = await call(env, "POST", "/apikeys/add", {
      token: DEVICE_TOKEN,
      body: { user_id: VICTIM, provider: "devin", api_key: "k" },
    });
    expect(res.status).toBe(200);
    expect(d1.__tables.api_keys.some((r) => r.user_id === OWNER && r.provider === "devin")).toBe(true);
    expect(d1.__tables.api_keys.some((r) => r.user_id === VICTIM && r.provider === "devin")).toBe(false);
  });

  it("/apikeys/remove is 401 without a bearer", async () => {
    const res = await call(env, "DELETE", "/apikeys/remove", {
      body: { user_id: VICTIM, provider: "claude" },
    });
    expect(res.status).toBe(401);
  });

  it("/apikeys/remove ignores a forged user_id", async () => {
    const res = await call(env, "DELETE", "/apikeys/remove", {
      token: DEVICE_TOKEN,
      body: { user_id: VICTIM, provider: "claude" },
    });
    expect(res.status).toBe(200);
    expect(d1.__tables.api_keys.some((r) => r.user_id === OWNER)).toBe(false);
    expect(d1.__tables.api_keys.some((r) => r.user_id === VICTIM)).toBe(true);
  });
});

describe("device gate — public routes stay reachable", () => {
  let d1: ReturnType<typeof makeD1>;
  let env: unknown;

  beforeEach(() => {
    d1 = makeD1();
    env = makeEnv(d1);
  });

  it("GET /health needs no credential", async () => {
    const res = await call(env, "GET", "/health");
    expect(res.status).toBe(200);
  });

  it("POST /v1/profiles/claim needs no credential", async () => {
    // A malformed body is rejected with 400 — the device gate did not
    // intercept it, which is the point: claiming is the bootstrap.
    const res = await call(env, "POST", "/v1/profiles/claim", { raw: "{not json" });
    expect(res.status).toBe(400);
  });

  it("POST /models/nlu/publish needs no device credential", async () => {
    // It carries its own admin-token gate; the device gate must not shadow it.
    const res = await call(env, "POST", "/models/nlu/publish", { body: {} });
    expect(res.status).not.toBe(401);
  });

  it("POST / is 401 without a bearer", async () => {
    const res = await call(env, "POST", "/", {
      body: { request_id: "r", requester: { id: OWNER, device_id: DEVICE_ID }, task: { type: "general", request: "hi" } },
    });
    expect(res.status).toBe(401);
  });
});

describe("OAuth state through the routes", () => {
  let d1: ReturnType<typeof makeD1>;
  let env: unknown;

  beforeEach(async () => {
    d1 = makeD1({
      devices: [
        {
          device_id: DEVICE_ID,
          profile_id: OWNER,
          device_token_hash: await sha256Hex(DEVICE_TOKEN),
          device_name: "NEXUS",
          os: "linux",
          app_version: null,
          status: "active",
          created_at: 1000,
          last_seen_at: 1000,
          revoked_at: null,
        },
      ],
    });
    env = makeEnv(d1);
  });

  it("/oauth/auth-url is 401 without a bearer", async () => {
    const res = await call(env, "GET", "/oauth/auth-url?provider=github");
    expect(res.status).toBe(401);
  });

  it("/oauth/auth-url mints a random state that carries no owner id", async () => {
    const res = await call(env, "GET", "/oauth/auth-url?provider=github", {
      token: DEVICE_TOKEN,
    });
    expect(res.status).toBe(200);
    const { url } = await bodyJson(res);
    const state = new URL(url).searchParams.get("state") || "";
    expect(state).toMatch(/^[0-9a-f]{64}$/);
    expect(state).not.toContain(OWNER);
    expect(state).not.toContain("github");
    // The redirect carries the state, never the owner.
    expect(url).not.toContain(OWNER);
  });

  it("/oauth/auth-url ignores a user_id in the query", async () => {
    const res = await call(env, "GET", `/oauth/auth-url?provider=github&user_id=${VICTIM}`, {
      token: DEVICE_TOKEN,
    });
    expect(res.status).toBe(200);
    const { url } = await bodyJson(res);
    expect(url).not.toContain(VICTIM);
    // The state row is bound to the DEVICE's owner.
    const row = d1.__tables.oauth_states[0];
    expect(row.user_id).toBe(OWNER);
    expect(row.provider).toBe("github");
  });

  it("/oauth/callback refuses a legacy 'provider:userId' state", async () => {
    const res = await call(env, "GET", `/oauth/callback?code=abc&state=github:${VICTIM}`);
    expect(res.status).toBe(200);
    const html = await res.text();
    expect(html).toContain("invalid or has expired");
  });

  it("/oauth/callback redeems the state exactly once", async () => {
    const authRes = await call(env, "GET", "/oauth/auth-url?provider=github", {
      token: DEVICE_TOKEN,
    });
    const { url } = await bodyJson(authRes);
    const state = new URL(url).searchParams.get("state") || "";

    // No code → the state is still redeemed (the flow is over either way).
    const first = await call(env, "GET", `/oauth/callback?state=${state}`);
    const firstHtml = await first.text();
    expect(firstHtml).toContain("Missing authorization code");

    // A second redemption of the same state must fail.
    const second = await call(env, "GET", `/oauth/callback?state=${state}`);
    const secondHtml = await second.text();
    expect(secondHtml).toContain("invalid or has expired");
  });

  it("/oauth/callback refuses an expired state", async () => {
    const state = "expired-state-value";
    d1.__tables.oauth_states.push({
      state_hash: await sha256Hex(state),
      user_id: OWNER,
      provider: "github",
      created_at: 1000,
      expires_at: 1001, // already past
      consumed_at: null,
    });
    const res = await call(env, "GET", `/oauth/callback?state=${state}`);
    const html = await res.text();
    expect(html).toContain("invalid or has expired");
  });
});

describe("CORS at the route level", () => {
  let d1: ReturnType<typeof makeD1>;
  let env: unknown;

  beforeEach(() => {
    d1 = makeD1();
    env = makeEnv(d1);
  });

  it("reflects an allowed origin and varies on Origin", async () => {
    const res = await call(env, "GET", "/health", {
      headers: { Origin: "tauri://localhost" },
    });
    expect(res.headers.get("Access-Control-Allow-Origin")).toBe("tauri://localhost");
    expect(res.headers.get("Vary")).toBe("Origin");
  });

  it("does not reflect an unknown origin and never sends a wildcard", async () => {
    const res = await call(env, "GET", "/health", {
      headers: { Origin: "https://evil.example" },
    });
    expect(res.headers.get("Access-Control-Allow-Origin")).toBeNull();
    expect(res.headers.get("Vary")).toBe("Origin");
  });

  it("answers the preflight without a wildcard", async () => {
    const res = await call(env, "OPTIONS", "/oauth/google-token", {
      headers: { Origin: "tauri://localhost" },
    });
    expect(res.status).toBe(200);
    expect(res.headers.get("Access-Control-Allow-Origin")).toBe("tauri://localhost");
    expect(res.headers.get("Access-Control-Allow-Headers")).toContain("X-Nexus-Device-Token");
  });
});
