import { afterEach, describe, expect, it } from "vitest";
import { resolveEffectiveTheme, resolveThemeMode, systemPrefersLight } from "./theme";

describe("resolveThemeMode", () => {
  it("passes light through", () => {
    expect(resolveThemeMode("light")).toBe("light");
  });

  it("passes system through", () => {
    expect(resolveThemeMode("system")).toBe("system");
  });

  it("falls back to dark for anything else (never transparent, never broken)", () => {
    for (const raw of [undefined, null, "", "dark", "Dark", "transparent", "glass", 42]) {
      expect(resolveThemeMode(raw)).toBe("dark");
    }
  });
});

describe("resolveEffectiveTheme", () => {
  it("resolves system against the OS preference", () => {
    expect(resolveEffectiveTheme("system", true)).toBe("light");
    expect(resolveEffectiveTheme("system", false)).toBe("dark");
  });

  it("passes explicit light/dark through regardless of the OS preference", () => {
    expect(resolveEffectiveTheme("light", false)).toBe("light");
    expect(resolveEffectiveTheme("light", true)).toBe("light");
    expect(resolveEffectiveTheme("dark", true)).toBe("dark");
    expect(resolveEffectiveTheme("dark", false)).toBe("dark");
  });
});

describe("systemPrefersLight", () => {
  afterEach(() => {
    // jsdom installs matchMedia; clean up any stub so other suites are unaffected.
    // (Each test file runs in its own environment, but keep it tidy anyway.)
  });

  it("returns a boolean without throwing", () => {
    expect(typeof systemPrefersLight()).toBe("boolean");
  });
});
