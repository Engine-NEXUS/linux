/**
 * Feature 88 (W1) — canonical laptop identity tests.
 *
 * Covers the full claim idempotency matrix and the resolveProfile
 * decision table (architecture doc §7) using an in-memory IdentityStore.
 */

import {
  claimProfile, resolveProfile, rotateDeviceToken, selfRevokeDevice,
  timingSafeEqualHex, sha256Hex,
  CLAIMS_PER_HOUR, PENDING_CAP,
  type IdentityStore, type ProfileRow, type DeviceRow, type EntitlementRow, type ClaimBody,
} from "../identity";

const BASE = 1_800_000_000_000;

// ---- In-memory IdentityStore ----

function memoryStore(): IdentityStore & {
  events: Array<{ profile_id: string; device_id: string | null; event: string; detail: string | null; at: number }>;
  profiles: Map<string, ProfileRow>;
  devices: Map<string, DeviceRow>;
} {
  const profiles = new Map<string, ProfileRow>();
  const devices = new Map<string, DeviceRow>();
  const events: Array<{ profile_id: string; device_id: string | null; event: string; detail: string | null; at: number }> = [];
  const entitlements = new Map<string, EntitlementRow>();
  const migrations = new Map<string, { profile_id: string }>();

  const store: IdentityStore = {
    async getProfile(id) { return profiles.get(id) || null; },
    async getLatestProfileByHint(hint) {
      let best: ProfileRow | null = null;
      for (const p of profiles.values()) {
        if (p.provision_hint === hint && (!best || p.created_at > best.created_at)) best = p;
      }
      return best;
    },
    async insertProfile(row) { profiles.set(row.profile_id, { ...row }); },
    async updateProfile(profileId, patch) {
      const cur = profiles.get(profileId);
      if (cur) profiles.set(profileId, { ...cur, ...patch });
    },
    async countPending() {
      let n = 0;
      for (const p of profiles.values()) if (p.status === "pending") n++;
      return n;
    },
    async touchProfile(id, at) {
      const p = profiles.get(id);
      if (p) profiles.set(id, { ...p, last_seen_at: at });
    },
    async getDevice(id) { return devices.get(id) || null; },
    async getDeviceForProfile(deviceId, profileId) {
      const d = devices.get(deviceId);
      return d && d.profile_id === profileId ? d : null;
    },
    async listDevicesForProfile(profileId) {
      return [...devices.values()].filter(d => d.profile_id === profileId)
        .sort((a, b) => a.created_at - b.created_at);
    },
    async insertDevice(row) { devices.set(row.device_id, { ...row }); },
    async updateDeviceTokenHash(deviceId, hash, at) {
      const d = devices.get(deviceId);
      if (d) devices.set(deviceId, { ...d, device_token_hash: hash, last_seen_at: at });
    },
    async revokeDevice(deviceId, at) {
      const d = devices.get(deviceId);
      if (d) devices.set(deviceId, { ...d, status: "revoked", revoked_at: at, last_seen_at: at });
    },
    async insertEvent(profileId, deviceId, event, detail, at) {
      events.push({ profile_id: profileId, device_id: deviceId, event, detail, at });
    },
    async countRecentRegistrations(ip, since) {
      let n = 0;
      for (const e of events) {
        if (e.event !== "registered" || e.at <= since) continue;
        try {
          const parsed = JSON.parse(e.detail || "{}");
          if (parsed.ip === ip) n++;
        } catch { /* ignore */ }
      }
      return n;
    },
    async getEntitlement(profileId, scope) { return entitlements.get(`${profileId}:${scope}`) || null; },
    async listEntitlements(profileId) {
      return [...entitlements.values()].filter(e => e.profile_id === profileId);
    },
    async upsertEntitlement(row) { entitlements.set(`${row.profile_id}:${row.scope}`, { ...row }); },
    async getMigration(legacyUserId) { return migrations.get(legacyUserId) || null; },
    async insertMigration(legacyUserId, profileId, at, autoApproved) {
      migrations.set(legacyUserId, { profile_id: profileId });
      void at; void autoApproved;
    },
  };
  return Object.assign(store, { events, profiles, devices });
}

/** Mimic the W2 admin approve endpoint at the store level. */
async function adminApprove(
  store: IdentityStore,
  profileId: string,
  opts: { tier?: string; expiresAt?: number | null; allowed?: number } = {},
) {
  await store.updateProfile(profileId, {
    status: "approved",
    approved_at: BASE + 1000,
    approved_by: "admin_test",
    quota_tier: opts.tier ?? "default",
    // Profile-level expiry mirrors the grant expiry (approve endpoint sets both).
    expires_at: opts.expiresAt ?? null,
  });
  await store.upsertEntitlement({
    profile_id: profileId,
    scope: "all",
    allowed: opts.allowed === 0 ? 0 : 1,
    quota_tier: opts.tier ?? "default",
    granted_by: "admin_test",
    granted_at: BASE + 1000,
    expires_at: opts.expiresAt ?? null,
  });
}

function claim(store: IdentityStore, body: ClaimBody, ip = "1.2.3.4", now = BASE) {
  return claimProfile(store, body, ip, now);
}

// ---- Claim ----

describe("claimProfile", () => {
  test("fresh claim creates a pending profile with a one-time token", async () => {
    const store = memoryStore();
    const r = await claim(store, { provisional_user_id: "user_a", device_name: "XPS", os: "windows" });
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect(r.status).toBe("pending");
    expect(r.profile_id.startsWith("prof_")).toBe(true);
    expect(r.device_id.startsWith("dev_")).toBe(true);
    expect(typeof r.device_token).toBe("string");
    expect((r.device_token as string).length).toBeGreaterThanOrEqual(40);

    // D1 keeps the HASH, never the plaintext token.
    const device = store.devices.get(r.device_id)!;
    expect(device.device_token_hash).not.toBe(r.device_token);
    expect(device.device_token_hash).toHaveLength(64);
    const hash = await sha256Hex(r.device_token as string);
    expect(device.device_token_hash).toBe(hash);
  });

  test("retry with the same hint returns the same profile WITHOUT a token", async () => {
    const store = memoryStore();
    const first = await claim(store, { provisional_user_id: "user_a" });
    expect(first.ok).toBe(true);
    const retry = await claim(store, { provisional_user_id: "user_a" });
    expect(retry.ok).toBe(true);
    if (first.ok && retry.ok) {
      expect(retry.profile_id).toBe(first.profile_id);
      expect(retry.device_id).toBe(first.device_id);
      expect(retry.device_token).toBeUndefined();
    }
  });

  test("re-claim with valid credentials is idempotent", async () => {
    const store = memoryStore();
    const first = await claim(store, { provisional_user_id: "user_a" });
    if (!first.ok || !first.device_token) throw new Error("claim failed");
    const again = await claim(store, {
      profile_id: first.profile_id, device_id: first.device_id, device_token: first.device_token,
    });
    expect(again.ok).toBe(true);
    if (again.ok) {
      expect(again.profile_id).toBe(first.profile_id);
      expect(again.device_token).toBeUndefined();
    }
  });

  test("re-claim with rotate issues a new token and kills the old one", async () => {
    const store = memoryStore();
    const first = await claim(store, { provisional_user_id: "user_a" });
    if (!first.ok || !first.device_token) throw new Error("claim failed");
    const rotated = await claim(store, {
      profile_id: first.profile_id, device_id: first.device_id,
      device_token: first.device_token, rotate: true,
    });
    expect(rotated.ok).toBe(true);
    const newToken = rotated.ok ? rotated.device_token : undefined;
    expect(newToken).toBeTruthy();
    expect(newToken).not.toBe(first.device_token);

    const resolvedOld = await resolveProfile(store, {
      profileId: first.profile_id, deviceId: first.device_id,
      authHeader: `Bearer ${first.device_token}`, mode: "off",
    }, BASE);
    expect(resolvedOld.ok).toBe(false);

    const resolvedNew = await resolveProfile(store, {
      profileId: first.profile_id, deviceId: first.device_id,
      authHeader: `Bearer ${newToken}`, mode: "off",
    }, BASE);
    expect(resolvedNew.ok).toBe(false); // still pending — pending status, not bad token
    if (!resolvedNew.ok) expect(resolvedNew.code).toBe("pending");
  });

  test("re-claim with wrong credentials is refused", async () => {
    const store = memoryStore();
    const first = await claim(store, { provisional_user_id: "user_a" });
    if (!first.ok) throw new Error("claim failed");
    const bad = await claim(store, {
      profile_id: first.profile_id, device_id: first.device_id, device_token: "wrong-token",
    });
    expect(bad.ok).toBe(false);
    if (!bad.ok) expect(bad.code).toBe("bad_credentials");
  });

  test("reinstall creates a brand-new profile", async () => {
    const store = memoryStore();
    const first = await claim(store, { provisional_user_id: "user_a" });
    const second = await claim(store, { provisional_user_id: "user_a", reinstall: true });
    expect(first.ok).toBe(true);
    expect(second.ok).toBe(true);
    if (first.ok && second.ok) {
      expect(second.profile_id).not.toBe(first.profile_id);
      expect(second.profile_id.startsWith("prof_")).toBe(true);
    }
  });

  test("more than CLAIMS_PER_HOUR new claims from one IP are rate-limited", async () => {
    const store = memoryStore();
    for (let i = 0; i < CLAIMS_PER_HOUR; i++) {
      const r = await claim(store, { provisional_user_id: `user_${i}` });
      expect(r.ok).toBe(true);
    }
    const blocked = await claim(store, { provisional_user_id: "user_overflow" });
    expect(blocked.ok).toBe(false);
    if (!blocked.ok) expect(blocked.code).toBe("rate_limited");
  });

  test("a different IP is not affected by another IP's rate window", async () => {
    const store = memoryStore();
    for (let i = 0; i < CLAIMS_PER_HOUR; i++) {
      await claim(store, { provisional_user_id: `user_${i}` }, "1.1.1.1");
    }
    const other = await claim(store, { provisional_user_id: "user_other" }, "2.2.2.2");
    expect(other.ok).toBe(true);
  });

  test("pending cap refuses new profiles when PENDING_CAP pending exist", async () => {
    const store = memoryStore();
    for (let i = 0; i < PENDING_CAP; i++) {
      await claim(store, { provisional_user_id: `hint_${i}` }, `10.0.0.${i % 250 + 1}`);
    }
    const blocked = await claim(store, { provisional_user_id: "user_over" }, "99.9.9.9");
    expect(blocked.ok).toBe(false);
    if (!blocked.ok) expect(blocked.code).toBe("pending_cap");
  });
});

// ---- resolveProfile: canonical decision table ----

describe("resolveProfile (canonical)", () => {
  async function setupApproved(store: IdentityStore, opts: { tier?: string; expiresAt?: number | null; allowed?: number } = {}) {
    const c = await claim(store, { provisional_user_id: "user_x" });
    if (!c.ok || !c.device_token) throw new Error("claim failed");
    await adminApprove(store, c.profile_id, opts);
    return { profileId: c.profile_id, deviceId: c.device_id, token: c.device_token as string };
  }

  test("unknown profile is denied", async () => {
    const store = memoryStore();
    const r = await resolveProfile(store, {
      profileId: "prof_missing", deviceId: "dev_missing",
      authHeader: "Bearer t", mode: "off",
    }, BASE);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.code).toBe("unknown_profile");
  });

  test("device not bound to the profile is denied", async () => {
    const store = memoryStore();
    const a = await setupApproved(store);
    const b = await claim(store, { provisional_user_id: "user_b" });
    if (!b.ok) throw new Error("claim failed");
    const r = await resolveProfile(store, {
      profileId: a.profileId, deviceId: b.ok ? (b as { device_id: string }).device_id : "",
      authHeader: `Bearer ${a.token}`, mode: "off",
    }, BASE);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.code).toBe("unknown_profile");
  });

  test("revoked device is denied", async () => {
    const store = memoryStore();
    const a = await setupApproved(store);
    await store.revokeDevice(a.deviceId, BASE + 2000);
    const r = await resolveProfile(store, {
      profileId: a.profileId, deviceId: a.deviceId,
      authHeader: `Bearer ${a.token}`, mode: "off",
    }, BASE + 3000);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.code).toBe("revoked");
  });

  test("missing or malformed bearer header is bad_token", async () => {
    const store = memoryStore();
    const a = await setupApproved(store);
    for (const header of [null, "Bearer", "bearer", "Token abc", "Bearer  "]) {
      const r = await resolveProfile(store, {
        profileId: a.profileId, deviceId: a.deviceId,
        authHeader: header, mode: "off",
      }, BASE);
      expect(r.ok, `header=${header}`).toBe(false);
      if (!r.ok) expect(r.code).toBe("bad_token");
    }
  });

  test("wrong token is bad_token", async () => {
    const store = memoryStore();
    const a = await setupApproved(store);
    const r = await resolveProfile(store, {
      profileId: a.profileId, deviceId: a.deviceId,
      authHeader: "Bearer deadbeef", mode: "off",
    }, BASE);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.code).toBe("bad_token");
  });

  test("pending profile is denied with pending", async () => {
    const store = memoryStore();
    const c = await claim(store, { provisional_user_id: "user_p" });
    if (!c.ok || !c.device_token) throw new Error("claim failed");
    const r = await resolveProfile(store, {
      profileId: c.profile_id, deviceId: c.device_id,
      authHeader: `Bearer ${c.device_token}`, mode: "off",
    }, BASE);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.code).toBe("pending");
  });

  test("suspended profile is denied", async () => {
    const store = memoryStore();
    const a = await setupApproved(store);
    await store.updateProfile(a.profileId, { status: "suspended" });
    const r = await resolveProfile(store, {
      profileId: a.profileId, deviceId: a.deviceId,
      authHeader: `Bearer ${a.token}`, mode: "off",
    }, BASE);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.code).toBe("suspended");
  });

  test("revoked profile is denied", async () => {
    const store = memoryStore();
    const a = await setupApproved(store);
    await store.updateProfile(a.profileId, { status: "revoked" });
    const r = await resolveProfile(store, {
      profileId: a.profileId, deviceId: a.deviceId,
      authHeader: `Bearer ${a.token}`, mode: "off",
    }, BASE);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.code).toBe("revoked");
  });

  test("expired grant is denied at the boundary", async () => {
    const store = memoryStore();
    const expiry = BASE + 10_000;
    const a = await setupApproved(store, { expiresAt: expiry });
    // exactly at expiry → still allowed (now <= expires_at)
    const atBoundary = await resolveProfile(store, {
      profileId: a.profileId, deviceId: a.deviceId,
      authHeader: `Bearer ${a.token}`, mode: "off",
    }, expiry);
    expect(atBoundary.ok).toBe(true);
    // one millisecond later → expired
    const after = await resolveProfile(store, {
      profileId: a.profileId, deviceId: a.deviceId,
      authHeader: `Bearer ${a.token}`, mode: "off",
    }, expiry + 1);
    expect(after.ok).toBe(false);
    if (!after.ok) expect(after.code).toBe("expired");
  });

  test("approved + entitled resolves with quota tier", async () => {
    const store = memoryStore();
    const a = await setupApproved(store, { tier: "premium" });
    const r = await resolveProfile(store, {
      profileId: a.profileId, deviceId: a.deviceId,
      authHeader: `Bearer ${a.token}`, mode: "off",
    }, BASE);
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(r.quotaTier).toBe("premium");
      expect(r.legacy).toBeUndefined();
    }
  });

  test("approved but entitlement allowed=0 is not_entitled", async () => {
    const store = memoryStore();
    const a = await setupApproved(store, { allowed: 0 });
    const r = await resolveProfile(store, {
      profileId: a.profileId, deviceId: a.deviceId,
      authHeader: `Bearer ${a.token}`, mode: "off",
    }, BASE);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.code).toBe("not_entitled");
  });

  test("per-scope grants: missing one required scope is not_entitled", async () => {
    const store = memoryStore();
    const c = await claim(store, { provisional_user_id: "user_s" });
    if (!c.ok || !c.device_token) throw new Error("claim failed");
    await store.updateProfile(c.profile_id, { status: "approved", approved_at: BASE, approved_by: "admin_test" });
    await store.upsertEntitlement({
      profile_id: c.profile_id, scope: "worker_ai", allowed: 1, quota_tier: "default",
      granted_by: "admin_test", granted_at: BASE, expires_at: null,
    });
    // llm_relay NOT granted → not entitled for transcripts.
    const r = await resolveProfile(store, {
      profileId: c.profile_id, deviceId: c.device_id,
      authHeader: `Bearer ${c.device_token}`, mode: "off",
    }, BASE);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.code).toBe("not_entitled");
    // worker_ai alone IS sufficient when only that scope is required.
    const r2 = await resolveProfile(store, {
      profileId: c.profile_id, deviceId: c.device_id,
      authHeader: `Bearer ${c.device_token}`, mode: "off", requiredScopes: ["worker_ai"],
    }, BASE);
    expect(r2.ok).toBe(true);
  });

  test("per-scope entitlement expiry is enforced", async () => {
    const store = memoryStore();
    const c = await claim(store, { provisional_user_id: "user_e" });
    if (!c.ok || !c.device_token) throw new Error("claim failed");
    await store.updateProfile(c.profile_id, { status: "approved", approved_at: BASE, approved_by: "admin_test" });
    await store.upsertEntitlement({
      profile_id: c.profile_id, scope: "all", allowed: 1, quota_tier: "default",
      granted_by: "admin_test", granted_at: BASE, expires_at: BASE + 1000,
    });
    const r = await resolveProfile(store, {
      profileId: c.profile_id, deviceId: c.device_id,
      authHeader: `Bearer ${c.device_token}`, mode: "off",
    }, BASE + 1001);
    expect(r.ok).toBe(false);
    // Grant existed but its expires_at passed → "expired", not "missing"
    // (arch §7 decision table: allowed=1 but expires_at < now → expired).
    if (!r.ok) expect(r.code).toBe("expired");
  });
});

// ---- resolveProfile: legacy migration modes ----

describe("resolveProfile (legacy modes)", () => {
  test("mode=off: legacy user passes through ungated", async () => {
    const store = memoryStore();
    const r = await resolveProfile(store, {
      legacyUserId: "user_legacy123", deviceId: "device_legacy", mode: "off",
    }, BASE);
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(r.legacy).toBe(true);
      expect(r.profileId).toBe("user_legacy123");
    }
  });

  test("mode=off: canonical clients are still gated properly", async () => {
    const store = memoryStore();
    const c = await claim(store, { provisional_user_id: "user_c" });
    if (!c.ok || !c.device_token) throw new Error("claim failed");
    const r = await resolveProfile(store, {
      profileId: c.profile_id, deviceId: c.device_id,
      authHeader: `Bearer ${c.device_token}`, mode: "off",
    }, BASE);
    expect(r.ok).toBe(false); // pending — canonical path gates even in off mode
    if (!r.ok) expect(r.code).toBe("pending");
  });

  test("mode=off with no identity at all is denied", async () => {
    const store = memoryStore();
    const r = await resolveProfile(store, { mode: "off" }, BASE);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.code).toBe("unknown_profile");
  });

  test("mode=strict: mapped legacy goes through the canonical gate", async () => {
    const store = memoryStore();
    const c = await claim(store, { provisional_user_id: "user_m" });
    if (!c.ok || !c.device_token) throw new Error("claim failed");
    await store.insertMigration("user_legacy_m", c.profile_id, BASE, 0);

    // Mapped + pending → pending denial.
    const r = await resolveProfile(store, {
      legacyUserId: "user_legacy_m", deviceId: c.device_id,
      authHeader: `Bearer ${c.device_token}`, mode: "strict",
    }, BASE);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.code).toBe("pending");

    // Approve → passes.
    await adminApprove(store, c.profile_id);
    const ok = await resolveProfile(store, {
      legacyUserId: "user_legacy_m", deviceId: c.device_id,
      authHeader: `Bearer ${c.device_token}`, mode: "strict",
    }, BASE);
    expect(ok.ok).toBe(true);
  });

  test("mode=strict: unmapped legacy is denied", async () => {
    const store = memoryStore();
    const r = await resolveProfile(store, {
      legacyUserId: "user_never_seen", deviceId: "device_x", mode: "strict",
    }, BASE);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.code).toBe("unknown_profile");
  });

  test("mode=revoke-legacy: unmapped legacy is denied", async () => {
    const store = memoryStore();
    const r = await resolveProfile(store, {
      legacyUserId: "user_never_seen", deviceId: "device_x", mode: "revoke-legacy",
    }, BASE);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.code).toBe("unknown_profile");
  });
});

// ---- Device lifecycle ----

describe("rotateDeviceToken", () => {
  test("wrong token cannot rotate", async () => {
    const store = memoryStore();
    const c = await claim(store, { provisional_user_id: "user_r" });
    if (!c.ok) throw new Error("claim failed");
    const r = await rotateDeviceToken(store, c.profile_id, c.device_id, "bad", BASE);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.code).toBe("bad_credentials");
  });

  test("rotation swaps the hash and logs an event", async () => {
    const store = memoryStore();
    const c = await claim(store, { provisional_user_id: "user_r" });
    if (!c.ok || !c.device_token) throw new Error("claim failed");
    const oldHash = store.devices.get(c.device_id)!.device_token_hash;
    const r = await rotateDeviceToken(store, c.profile_id, c.device_id, c.device_token as string, BASE);
    expect(r.ok).toBe(true);
    if (!r.ok || !r.device_token) throw new Error("rotate failed");
    const newHash = store.devices.get(c.device_id)!.device_token_hash;
    expect(newHash).not.toBe(oldHash);
    expect(newHash).toBe(await sha256Hex(r.device_token));
    expect(store.events.some(e => e.event === "token_rotated")).toBe(true);
  });
});

describe("selfRevokeDevice", () => {
  test("revoked device can no longer resolve", async () => {
    const store = memoryStore();
    const c = await claim(store, { provisional_user_id: "user_v" });
    if (!c.ok || !c.device_token) throw new Error("claim failed");
    await adminApprove(store, c.profile_id);
    const r = await selfRevokeDevice(store, c.profile_id, c.device_id, c.device_token as string, BASE);
    expect(r.ok).toBe(true);
    const resolved = await resolveProfile(store, {
      profileId: c.profile_id, deviceId: c.device_id,
      authHeader: `Bearer ${c.device_token}`, mode: "off",
    }, BASE);
    expect(resolved.ok).toBe(false);
    if (!resolved.ok) expect(resolved.code).toBe("revoked");
    expect(store.events.some(e => e.event === "revoked" && e.detail === "self_revoke")).toBe(true);
  });
});

// ---- Cross-profile isolation ----

describe("two-laptop isolation", () => {
  test("laptop A's token never unlocks laptop B", async () => {
    const store = memoryStore();
    const a = await claim(store, { provisional_user_id: "user_A" });
    const b = await claim(store, { provisional_user_id: "user_B" });
    if (!a.ok || !a.device_token || !b.ok) throw new Error("claim failed");
    await adminApprove(store, a.profile_id);
    await adminApprove(store, b.profile_id);

    const r = await resolveProfile(store, {
      profileId: b.profile_id, deviceId: a.device_id,
      authHeader: `Bearer ${a.device_token}`, mode: "off",
    }, BASE);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.code).toBe("unknown_profile");
  });

  test("revoking A does not affect B", async () => {
    const store = memoryStore();
    const a = await claim(store, { provisional_user_id: "user_A" });
    const b = await claim(store, { provisional_user_id: "user_B" });
    if (!a.ok || !a.device_token || !b.ok || !b.device_token) throw new Error("claim failed");
    await adminApprove(store, a.profile_id);
    await adminApprove(store, b.profile_id);

    await store.updateProfile(a.profile_id, { status: "revoked" });

    const ra = await resolveProfile(store, {
      profileId: a.profile_id, deviceId: a.device_id,
      authHeader: `Bearer ${a.device_token}`, mode: "off",
    }, BASE);
    const rb = await resolveProfile(store, {
      profileId: b.profile_id, deviceId: b.device_id,
      authHeader: `Bearer ${b.device_token}`, mode: "off",
    }, BASE);
    expect(ra.ok).toBe(false);
    expect(rb.ok).toBe(true);
  });
});

// ---- Primitives ----

describe("timingSafeEqualHex", () => {
  test("equal strings match", () => {
    expect(timingSafeEqualHex("abcd", "abcd")).toBe(true);
  });
  test("different strings of equal length do not match", () => {
    expect(timingSafeEqualHex("abcd", "abce")).toBe(false);
  });
  test("different lengths do not match", () => {
    expect(timingSafeEqualHex("abcd", "abc")).toBe(false);
  });
});
