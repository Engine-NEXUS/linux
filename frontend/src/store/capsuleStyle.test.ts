import { describe, expect, it } from "vitest";
import { resolveCapsuleStyle } from "./assistant";

describe("resolveCapsuleStyle", () => {
  it("keeps every known style, on every platform", () => {
    for (const style of ["classic", "pebble", "aura"] as const) {
      expect(resolveCapsuleStyle(style, true)).toBe(style);
      expect(resolveCapsuleStyle(style, false)).toBe(style);
    }
  });

  it("defaults to the aura on Linux and the pebble elsewhere", () => {
    expect(resolveCapsuleStyle(null, true)).toBe("aura");
    expect(resolveCapsuleStyle(undefined, false)).toBe("pebble");
  });

  it("treats an unknown or corrupted value as unset", () => {
    expect(resolveCapsuleStyle("square", true)).toBe("aura");
    expect(resolveCapsuleStyle(42, false)).toBe("pebble");
  });
});
