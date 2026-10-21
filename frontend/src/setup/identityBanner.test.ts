/**
 * Feature 88 (C2) — identity banner derivation tests.
 */

import { describe, expect, test } from "vitest";

import { identityBanner } from "./identityBanner";

describe("identityBanner", () => {
  test("null → checking state", () => {
    const b = identityBanner(null);
    expect(b.tone).toBe("dim");
    expect(b.title).toContain("Checking");
  });

  test("approved → green", () => {
    const b = identityBanner({ state: "approved" });
    expect(b.tone).toBe("green");
    expect(b.title).toBe("Cloud connected");
  });

  test("pending → amber with admin hint", () => {
    const b = identityBanner({ state: "pending" });
    expect(b.tone).toBe("amber");
    expect(b.title).toBe("Awaiting admin approval");
    expect(b.subtitle).toContain("Local features work now");
  });

  test("provisional → dim retry note; rate_limited variant names the cause", () => {
    const b = identityBanner({ state: "provisional" });
    expect(b.tone).toBe("dim");
    expect(b.subtitle).toContain("retry connecting automatically");
    const rl = identityBanner({ state: "provisional", reason: "rate_limited" });
    expect(rl.subtitle).toContain("Too many registration attempts");
  });

  test("denied states → red with reason", () => {
    for (const state of ["suspended", "revoked", "expired"]) {
      const b = identityBanner({ state });
      expect(b.tone).toBe("red");
      expect(b.title.length).toBeGreaterThan(0);
    }
  });

  test("unknown state → red fallback with the distinct spoken line", () => {
    const b = identityBanner({ state: "unknown_profile" });
    expect(b.tone).toBe("red");
    expect(b.subtitle).toBe("Cloud access isn't enabled for this device yet, sir.");
  });
});
