/**
 * Feature 88 (W2) — the AI entitlement gate.
 *
 * Single choke point: resolveProfile (identity.ts) → explicit deny or a
 * validated EntitlementContext. handleTranscript MUST call this before
 * any intent classification / AI / external-LLM path. The deny path
 * performs NO env.AI.run calls, NO external LLM calls, NO usage
 * increments — only an audit event (no user content).
 *
 * Fail-closed: any lookup error denies.
 */

import {
  resolveProfile,
  type EntitlementCode,
  type IdentityStore,
  type MigrationMode,
} from "./identity";

export interface EntitlementContext {
  profileId: string;
  deviceId: string;
  quotaTier: string;
  legacy?: boolean;
}

export type GateDenial = { ok: false; code: EntitlementCode; status?: string };
export type GateResult = ({ ok: true } & EntitlementContext) | GateDenial;

export interface GateOpts {
  profileId?: string;
  deviceId?: string;
  authHeader?: string | null;
  legacyUserId?: string;
  mode: MigrationMode;
}

export function gateTranscript(
  store: IdentityStore,
  opts: GateOpts,
  now: number,
): Promise<GateResult> {
  return resolveProfile(store, opts, now) as Promise<GateResult>;
}

/** Audit row for a denial — metadata only, never user content. */
export async function recordDenial(
  store: IdentityStore,
  profileId: string,
  deviceId: string | null,
  code: string,
  now: number,
): Promise<void> {
  try {
    await store.insertEvent(profileId, deviceId, "denied", JSON.stringify({ code }), now);
  } catch {
    // Audit must never break the denial response.
  }
}

/** HTTP status for a denial code. */
export function denialStatus(code: EntitlementCode): number {
  switch (code) {
    case "bad_token":
      return 401;
    case "lookup_failed":
      return 503;
    default:
      return 403;
  }
}

export const DENIAL_ERROR = "worker_ai_not_enabled";
