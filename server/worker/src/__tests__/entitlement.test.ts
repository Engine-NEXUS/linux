/**
 * Feature 88 (W2) — end-to-end entitlement gate + admin plane tests.
 *
 * Drives the REAL worker fetch handler (default export) against a compact
 * in-memory D1 that emulates exactly the SQL shapes the identity/admin/
 * quota modules issue. Proves the core guarantees:
 *   - deny ⇒ zero env.AI.run calls, zero usage increments
 *   - mode=off keeps legacy behavior untouched
 *   - admin approve unlocks; revoke kills tokens + oauth rows
 *   - two-laptop isolation
 */

import worker from "../index";
import { memoryIdentityStore } from "./identity_helpers";
import { sha256Hex } from "../auth";
const BASE = 1_800_000_000_000;

// ---- Mini D1 ----

function miniD1() {
  const t = {
    profiles: new Map<string, any>(),
    devices: new Map<string, any>(),
    entitlements: new Map<string, any>(),
    profile_events: [] as any[],
    identity_migration: new Map<string, any>(),
    oauth_tokens: new Map<string, any>(),
    usage_log: new Map<string, any>(),
  };

  const exec = (sql: string, args: any[]) => {
    const s = sql.replace(/\s+/g, " ").trim();

    // SELECT ... FROM X WHERE ... (first/all)
    if (s.startsWith("SELECT")) {
      const table = (s.match(/FROM (\w+)/) || [])[1];
      const isCount = /COUNT\(\*\) AS n/.test(s);
      const isCoalesce = /COALESCE\(SUM\(ai_neurons\), 0\)/.test(s);
      const whereProfile = /WHERE profile_id = \?/.test(s);
      const whereHint = /WHERE provision_hint = \?/.test(s);
      const whereDeviceAndProfile = /WHERE device_id = \? AND profile_id = \?/.test(s);
      const whereDevice = /WHERE device_id = \?/.test(s);
      const whereScope = /WHERE profile_id = \? AND scope = \?/.test(s);
      const whereUserDay = /WHERE user_id = \? AND day_utc = \?/.test(s);
      const whereDay = /WHERE day_utc = \?/.test(s);
      const whereUser = /WHERE user_id = \?/.test(s);
      const whereLegacy = /WHERE legacy_user_id = \?/.test(s);
      const rowsFor = (tbl: string, pred: (r: any) => boolean) =>
        [...(t as any)[tbl].values()].filter(pred);

      const count = (rows: any[]) => ({ n: rows.length });

      if (table === "profiles") {
        if (isCount) return { first: async () => count(rowsFor("profiles", r => r.status === "pending")) };
        if (whereHint) return { first: async () => rowsFor("profiles", r => r.provision_hint === args[0])
          .sort((a, b) => b.created_at - a.created_at)[0] || null };
        if (whereProfile) return { first: async () => t.profiles.get(args[0]) || null };
      }
      if (table === "devices") {
        // The device gate (Linux port J1) looks the credential up by the
        // SHA-256 hash of the bearer token and takes the owner from the
        // device's profile.
        if (/device_token_hash = \?/.test(s)) {
          return { first: async () => {
            for (const d of t.devices.values()) {
              if (d.device_token_hash === args[0]
                && d.status === "active"
                && d.revoked_at == null) {
                return d;
              }
            }
            return null;
          } };
        }
        if (whereDeviceAndProfile) return { first: async () => {
          const d = t.devices.get(args[0]);
          return d && d.profile_id === args[1] ? d : null;
        } };
        if (whereDevice) return { first: async () => t.devices.get(args[0]) || null };
        if (whereProfile) return { all: async () => ({ results: rowsFor("devices", r => r.profile_id === args[0])
          .sort((a, b) => a.created_at - b.created_at) }) };
      }
      if (table === "entitlements") {
        if (whereScope) return { first: async () => t.entitlements.get(`${args[0]}:${args[1]}`) || null };
        if (whereProfile) return { all: async () => ({ results: rowsFor("entitlements", r => r.profile_id === args[0]) }) };
      }
      if (table === "identity_migration") {
        if (whereLegacy) return { first: async () => t.identity_migration.get(args[0]) || null };
      }
      if (table === "oauth_tokens") {
        if (isCount && whereUser) return { first: async () => count(rowsFor("oauth_tokens", r => r.user_id === args[0])) };
        if (whereUserDay) return { first: async () => t.usage_log.get(`${args[0]}:${args[1]}`) || null };
      }
      if (table === "usage_log") {
        if (isCoalesce && whereDay) return { first: async () => {
          let total = 0;
          for (const r of t.usage_log.values()) if (r.day_utc === args[0]) total += r.ai_neurons || 0;
          return { total };
        } };
        if (whereUserDay) return { first: async () => t.usage_log.get(`${args[0]}:${args[1]}`) || null };
      }
      if (table === "profile_events") {
        if (isCount) {
          return { first: async () => {
            const [pattern, since] = args;
            const m = pattern.match(/"ip":"(.+?)"/);
            const ip = m ? m[1] : null;
            let n = 0;
            for (const e of t.profile_events) {
              if (e.event !== "registered" || e.at <= since) continue;
              try { if (JSON.parse(e.detail || "{}").ip === ip) n++; } catch { /* skip */ }
            }
            return { n };
          } };
        }
      }
      return { first: async () => null, all: async () => ({ results: [] }) };
    }

    if (s.startsWith("INSERT")) {
      const table = (s.match(/INTO (\w+)/) || [])[1];
      if (table === "profiles") {
        t.profiles.set(args[0], {
          profile_id: args[0], human_label: args[1], status: args[2], quota_tier: args[3],
          created_at: args[4], approved_at: args[5], approved_by: args[6], expires_at: args[7],
          last_seen_at: args[8], provision_hint: args[9],
        });
      } else if (table === "devices") {
        t.devices.set(args[0], {
          device_id: args[0], profile_id: args[1], device_token_hash: args[2],
          device_name: args[3], os: args[4], app_version: args[5], status: args[6],
          created_at: args[7], last_seen_at: args[8], revoked_at: args[9],
        });
      } else if (table === "profile_events") {
        t.profile_events.push({ profile_id: args[0], device_id: args[1], event: args[2], detail: args[3], at: args[4] });
      } else if (table === "entitlements") {
        t.entitlements.set(`${args[0]}:${args[1]}`, {
          profile_id: args[0], scope: args[1], allowed: args[2], quota_tier: args[3],
          granted_by: args[4], granted_at: args[5], expires_at: args[6],
        });
      } else if (table === "identity_migration") {
        t.identity_migration.set(args[0], { legacy_user_id: args[0], profile_id: args[1] });
      } else if (table === "usage_log") {
        t.usage_log.set(`${args[0]}:${args[1]}`, {
          user_id: args[0], day_utc: args[1], requests: args[2], ai_neurons: args[3],
          d1_reads: args[4], d1_writes: args[5], search_calls: args[6], deep_calls: args[7],
        });
      }
      return { run: async () => {} };
    }

    if (s.startsWith("UPDATE")) {
      const table = (s.match(/UPDATE (\w+)/) || [])[1];
      if (table === "profiles") {
        const p = t.profiles.get(args[args.length - 1]);
        if (p) {
          if (/SET last_seen_at = \?/.test(s) && !/status/.test(s)) p.last_seen_at = args[0];
          else {
            // dynamic patch: parse "SET col = ?, col = ?"
            const setPart = s.slice(s.indexOf("SET") + 3, s.indexOf("WHERE"));
            const cols = (setPart.match(/(\w+) = \?/g) || []).map(x => x.replace(/ = \?/, ""));
            cols.forEach((c, i) => { p[c] = args[i]; });
          }
        }
      } else if (table === "devices") {
        const d = t.devices.get(args[args.length - 1]);
        if (d) {
          if (/device_token_hash/.test(s)) { d.device_token_hash = args[0]; d.last_seen_at = args[1]; }
          else if (/status = 'revoked'/.test(s)) { d.status = "revoked"; d.revoked_at = args[0]; d.last_seen_at = args[1]; }
        }
      }
      return { run: async () => {} };
    }

    if (s.startsWith("DELETE")) {
      const table = (s.match(/FROM (\w+)/) || [])[1];
      if (table === "oauth_tokens") {
        for (const [k, r] of [...t.oauth_tokens]) if (r.user_id === args[0]) t.oauth_tokens.delete(k);
      }
      return { run: async () => {} };
    }

    throw new Error(`miniD1: unsupported SQL: ${s}`);
  };

  return {
    prepare: (sql: string) => {
      const runWith = (args: any[]) => exec(sql, args);
      return {
        // Zero-bind queries (e.g. countPending uses prepare(...).first()).
        first: async () => {
          const out = runWith([]);
          return ("first" in out) ? out.first() : null;
        },
        all: async () => {
          const out = runWith([]);
          return ("all" in out) ? out.all() : { results: [] };
        },
        run: async () => {
          const out = runWith([]);
          if ("run" in out) await out.run();
        },
        bind: (...args: any[]) => {
          const out = runWith(args);
          return {
            first: async () => ("first" in out) ? out.first() : null,
            all: async () => ("all" in out) ? out.all() : { results: [] },
            run: async () => { if ("run" in out) await out.run(); },
          };
        },
      };
    },
    _t: t,
  };
}

// ---- Helpers ----

function jsonReq(url: string, method: string, body?: unknown, headers: Record<string, string> = {}): Request {
  return new Request(url, {
    method,
    headers: { "Content-Type": "application/json", ...headers },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

function makeEnv() {
  const db = miniD1();
  const aiRun = vi.fn(async () => ({ response: "ok" }));
  const env: any = {
    DB: db as any,
    AI: { run: aiRun },
    MIGRATION_MODE: "off",
    NEXUS_ADMIN_IDENTITY_TOKEN: "admin-secret-token",
  };
  return { env, db, aiRun };
}

// ---- Tests ----

describe("Feature 88 e2e — entitlement gate", () => {
  test("mode=off: legacy transcript flows exactly as before (zero AI for static intent)", async () => {
    const { env, db, aiRun } = makeEnv();
    // The device gate (Linux port J1) runs before the entitlement gate: the
    // transcript route now needs a device credential like every other
    // non-public route. The LEGACY entitlement semantics (mode=off) are
    // unchanged.
    db._t.devices.set("device_legacy1", {
      device_id: "device_legacy1", profile_id: "user_legacy1",
      device_token_hash: await sha256Hex("legacy-device-token"),
      device_name: "Laptop", os: "windows", app_version: null,
      status: "active", created_at: BASE, last_seen_at: BASE, revoked_at: null,
    });
    const res = await worker.fetch(jsonReq("https://w.test/", "POST", {
      request_id: "r1",
      requester: { id: "user_legacy1", device_id: "device_legacy1" },
      task: { type: "general", intent: "deep_analyse", request: "deep analyse owner/repo" },
    }, { Authorization: "Bearer legacy-device-token" }), env);
    expect(res.status).toBe(200);
    const body = await res.json() as any;
    expect(body.reply_text).toContain("architecture mapper");
    expect(body.protocol_version).toBe("1");
    // explicit intent → classify never ran; deep_analyse branch is static
    expect(aiRun).not.toHaveBeenCalled();
  });

  test("a transcript turn without a device credential is refused before any AI runs", async () => {
    const { env, db, aiRun } = makeEnv();
    const res = await worker.fetch(jsonReq("https://w.test/", "POST", {
      request_id: "r0",
      requester: { id: "user_legacy1", device_id: "device_legacy1" },
      task: { type: "general", request: "what is the capital of France" },
    }), env);
    expect(res.status).toBe(401);
    expect(aiRun).not.toHaveBeenCalled();
    expect(db._t.usage_log.size).toBe(0);
  });

  test("pending canonical profile is denied with zero AI calls and zero usage", async () => {
    const { env, db, aiRun } = makeEnv();
    // 1. claim
    const claimRes = await worker.fetch(jsonReq("https://w.test/v1/profiles/claim", "POST", {
      provisional_user_id: "user_new", device_name: "Laptop-B", os: "windows",
    }), env);
    expect(claimRes.status).toBe(200);
    const claim = await claimRes.json() as any;
    expect(claim.status).toBe("pending");
    expect(claim.device_token).toBeTruthy();

    // 2. transcript with canonical identity → 403
    const turnRes = await worker.fetch(jsonReq("https://w.test/", "POST", {
      request_id: "r2",
      requester: { id: claim.profile_id, profile_id: claim.profile_id, device_id: claim.device_id },
      task: { type: "general", request: "what is the capital of France" },
    }, { Authorization: `Bearer ${claim.device_token}` }), env);
    expect(turnRes.status).toBe(403);
    const denied = await turnRes.json() as any;
    expect(denied.error).toBe("worker_ai_not_enabled");
    expect(denied.code).toBe("pending");

    // 3. no AI consumed, no quota burned
    expect(aiRun).not.toHaveBeenCalled();
    expect(db._t.usage_log.size).toBe(0);
  });

  test("admin approve unlocks the pending profile; denial audit logged", async () => {
    const { env, db } = makeEnv();
    const claim = await (await worker.fetch(jsonReq("https://w.test/v1/profiles/claim", "POST", {
      provisional_user_id: "user_a2",
    }), env)).json() as any;

    // deny audit event was written
    expect(db._t.profile_events.some(e => e.event === "registered")).toBe(true);

    // approve
    const ap = await worker.fetch(jsonReq("https://w.test/v1/admin/profiles/approve", "POST", {
      profile_id: claim.profile_id, quota_tier: "default",
    }, { Authorization: "Bearer admin-secret-token" }), env);
    expect(ap.status).toBe(200);

    // /me reflects approval
    const me = await worker.fetch(new Request(
      `https://w.test/v1/profiles/me?profile_id=${claim.profile_id}&device_id=${claim.device_id}`,
      { headers: { Authorization: `Bearer ${claim.device_token}` } }), env);
    expect(me.status).toBe(200);
    const meBody = await me.json() as any;
    expect(meBody.status).toBe("approved");
    expect(meBody.quota_tier).toBe("default");
  });

  test("admin routes reject bad/missing credentials", async () => {
    const { env } = makeEnv();
    const r1 = await worker.fetch(jsonReq("https://w.test/v1/admin/profiles/pending", "GET"), env);
    expect(r1.status).toBe(401);
    const r2 = await worker.fetch(jsonReq("https://w.test/v1/admin/profiles/pending", "GET"), {
      ...makeEnv().env, NEXUS_ADMIN_IDENTITY_TOKEN: "admin-secret-token",
    });
    void r2;
    const r3 = await worker.fetch(jsonReq("https://w.test/v1/admin/profiles/pending", "GET"), {
      ...makeEnv().env,
    });
    void r3;
    const bad = await worker.fetch(jsonReq("https://w.test/v1/admin/profiles/pending", "GET", undefined, {
      Authorization: "Bearer wrong",
    }), env);
    expect(bad.status).toBe(401);
  });

  test("revoke cascade kills device tokens and deletes oauth rows", async () => {
    const { env, db } = makeEnv();
    const claim = await (await worker.fetch(jsonReq("https://w.test/v1/profiles/claim", "POST", {
      provisional_user_id: "user_r2",
    }), env)).json() as any;
    await worker.fetch(jsonReq("https://w.test/v1/admin/profiles/approve", "POST", {
      profile_id: claim.profile_id,
    }, { Authorization: "Bearer admin-secret-token" }), env);

    // seed an oauth row for the profile
    db._t.oauth_tokens.set("k1", { user_id: claim.profile_id, provider: "google", access_token: "at" });

    const rev = await worker.fetch(jsonReq("https://w.test/v1/admin/profiles/revoke", "POST", {
      profile_id: claim.profile_id, note: "lost laptop",
    }, { Authorization: "Bearer admin-secret-token" }), env);
    expect(rev.status).toBe(200);
    const revBody = await rev.json() as any;
    expect(revBody.status).toBe("revoked");
    expect(revBody.oauth_deleted).toBe(1);
    expect(db._t.oauth_tokens.size).toBe(0);

    // device token is dead — the device gate (Linux port J1) now catches the
    // revoked device before the entitlement gate, so the refusal is a 401
    // (invalid credential) rather than a 403 (revoked profile).
    const me = await worker.fetch(new Request(
      `https://w.test/v1/profiles/me?profile_id=${claim.profile_id}&device_id=${claim.device_id}`,
      { headers: { Authorization: `Bearer ${claim.device_token}` } }), env);
    expect(me.status).toBe(401);
    const meBody = await me.json() as any;
    expect(meBody.error).toContain("unauthorized");
  });

  test("two laptops are fully isolated", async () => {
    const { env } = makeEnv();
    const a = await (await worker.fetch(jsonReq("https://w.test/v1/profiles/claim", "POST", {
      provisional_user_id: "user_A2",
    }), env)).json() as any;
    const b = await (await worker.fetch(jsonReq("https://w.test/v1/profiles/claim", "POST", {
      provisional_user_id: "user_B2",
    }), env)).json() as any;
    await worker.fetch(jsonReq("https://w.test/v1/admin/profiles/approve", "POST", {
      profile_id: a.profile_id,
    }, { Authorization: "Bearer admin-secret-token" }), env);
    await worker.fetch(jsonReq("https://w.test/v1/admin/profiles/approve", "POST", {
      profile_id: b.profile_id,
    }, { Authorization: "Bearer admin-secret-token" }), env);

    // A's token cannot ride B's profile
    const r = await worker.fetch(new Request(
      `https://w.test/v1/profiles/me?profile_id=${b.profile_id}&device_id=${a.device_id}`,
      { headers: { Authorization: `Bearer ${a.device_token}` } }), env);
    expect(r.status).toBe(403);
    const body = await r.json() as any;
    expect(body.code).toBe("unknown_profile");
  });

  test("health advertises identity + entitlement additively", async () => {
    const { env } = makeEnv();
    const res = await worker.fetch(jsonReq("https://w.test/health", "GET"), env);
    const body = await res.json() as any;
    expect(body.identity).toBe("1");
    expect(body.entitlement_gate).toBe("1");
    expect(body.protocol_version).toBe("1");
  });
});
