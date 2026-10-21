/**
 * NEXUS wire protocol v1 (C3) — versioned Tauri<->Worker JSON contract.
 *
 * Rules:
 * - Clients send `protocol_version: "1"` on every POST.
 * - The Worker echoes `protocol_version` on transcript replies and
 *   advertises it on GET /health. Unknown versions are served
 *   best-effort (fail-open) — never hard-rejected.
 * - Additive changes only within v1 (new optional fields). Breaking
 *   changes require v2 + a major client release.
 */

export const PROTOCOL_VERSION = "1";

export interface HealthBody {
  ok: boolean;
  service: string;
  protocol: string;
  protocol_version: string;
  serverless: boolean;
  /** Feature 88: canonical laptop identity API version (additive, v1). */
  identity: string;
  /** Feature 88: entitlement gate present (additive, v1). */
  entitlement_gate: string;
}

export function buildHealthBody(): HealthBody {
  return {
    ok: true,
    service: "NEXUS Worker",
    protocol: "nexus-json-v1",
    protocol_version: PROTOCOL_VERSION,
    serverless: true,
    identity: "1",
    entitlement_gate: "1",
  };
}

/** Stamp a transcript reply with the protocol version (mutates + returns). */
export function withProtocolVersion<T extends Record<string, unknown>>(resp: T): T {
  (resp as Record<string, unknown>)["protocol_version"] = PROTOCOL_VERSION;
  return resp;
}
