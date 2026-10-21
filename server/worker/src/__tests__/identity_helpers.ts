/**
 * Shared in-memory IdentityStore for Feature 88 tests.
 * (Extracted from identity.test.ts so e2e suites can reuse it.)
 */

import type {
  IdentityStore, ProfileRow, DeviceRow, EntitlementRow,
} from "../identity";

export interface MemoryStoreExtras {
  events: Array<{ profile_id: string; device_id: string | null; event: string; detail: string | null; at: number }>;
  profiles: Map<string, ProfileRow>;
  devices: Map<string, DeviceRow>;
  entitlements: Map<string, EntitlementRow>;
  migrations: Map<string, { profile_id: string }>;
}

export function memoryIdentityStore(): IdentityStore & MemoryStoreExtras {
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
  return Object.assign(store, { events, profiles, devices, entitlements, migrations });
}

/** Mimic the admin approve endpoint at the store level (tests). */
export async function adminApproveStore(
  store: IdentityStore,
  profileId: string,
  opts: { tier?: string; expiresAt?: number | null; allowed?: number } = {},
  baseTime = 1_800_000_000_000,
) {
  await store.updateProfile(profileId, {
    status: "approved",
    approved_at: baseTime + 1000,
    approved_by: "admin_test",
    quota_tier: opts.tier ?? "default",
    expires_at: opts.expiresAt ?? null,
  });
  await store.upsertEntitlement({
    profile_id: profileId,
    scope: "all",
    allowed: opts.allowed === 0 ? 0 : 1,
    quota_tier: opts.tier ?? "default",
    granted_by: "admin_test",
    granted_at: baseTime + 1000,
    expires_at: opts.expiresAt ?? null,
  });
}
