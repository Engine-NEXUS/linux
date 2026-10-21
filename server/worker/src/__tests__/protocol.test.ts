/**
 * Tests for the NEXUS wire protocol v1 (C3).
 */

import { PROTOCOL_VERSION, buildHealthBody, withProtocolVersion } from "../protocol";

describe("protocol v1", () => {
  test("version is 1", () => {
    expect(PROTOCOL_VERSION).toBe("1");
  });

  test("health body advertises the version", () => {
    const h = buildHealthBody();
    expect(h.ok).toBe(true);
    expect(h.protocol).toBe("nexus-json-v1");
    expect(h.protocol_version).toBe("1");
  });

  test("withProtocolVersion stamps replies", () => {
    const r = withProtocolVersion({ request_id: "x", reply_text: "hi" });
    expect(r.protocol_version).toBe("1");
    expect(r.reply_text).toBe("hi");
  });

  // Feature 88 (W1): additive identity/entitlement fields.
  test("health body advertises identity + entitlement gate", () => {
    const h = buildHealthBody();
    expect(h.identity).toBe("1");
    expect(h.entitlement_gate).toBe("1");
  });
});
