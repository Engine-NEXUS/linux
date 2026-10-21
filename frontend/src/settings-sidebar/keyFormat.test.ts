import { describe, expect, it } from "vitest";
import { isPlausibleGeminiKey } from "./keyFormat";

describe("isPlausibleGeminiKey", () => {
  it("accepts a well-formed key", () => {
    expect(isPlausibleGeminiKey("AIza" + "a".repeat(35))).toBe(true);
  });

  it("trims surrounding whitespace", () => {
    expect(isPlausibleGeminiKey("  AIza" + "b".repeat(35) + "\n")).toBe(true);
  });

  it("rejects empty, short, and wrong-prefix keys", () => {
    expect(isPlausibleGeminiKey("")).toBe(false);
    expect(isPlausibleGeminiKey("AIza short")).toBe(false);
    expect(isPlausibleGeminiKey("gsk_" + "c".repeat(35))).toBe(false);
    expect(isPlausibleGeminiKey("AIza" + "d".repeat(34))).toBe(false);
    expect(isPlausibleGeminiKey("AIza" + "e".repeat(36))).toBe(false);
  });
});
