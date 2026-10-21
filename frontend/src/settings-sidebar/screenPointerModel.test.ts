import { describe, expect, it } from "vitest";
import {
  DEFAULT_POINTER_SETTINGS,
  clampDwell,
  formatExcludedApps,
  normalizePointerSettings,
  parseExcludedApps,
} from "./screenPointerModel";

describe("DEFAULT_POINTER_SETTINGS", () => {
  it("starts enabled, for six seconds, with no exclusions", () => {
    expect(DEFAULT_POINTER_SETTINGS).toEqual({
      enabled: true,
      dwellSeconds: 6,
      excludedApps: [],
      suppressFullscreen: true,
    });
  });
});

describe("clampDwell", () => {
  it("returns 6 for NaN and non-numbers", () => {
    expect(clampDwell(Number.NaN)).toBe(6);
    expect(clampDwell("8")).toBe(6);
    expect(clampDwell(undefined)).toBe(6);
    expect(clampDwell(null)).toBe(6);
    expect(clampDwell({ seconds: 8 })).toBe(6);
  });

  it("clamps below and above the allowed range", () => {
    expect(clampDwell(2)).toBe(3);
    expect(clampDwell(-40)).toBe(3);
    expect(clampDwell(99)).toBe(15);
  });

  it("rounds fractional seconds to the nearest whole number", () => {
    expect(clampDwell(7.6)).toBe(8);
    expect(clampDwell(4.4)).toBe(4);
  });

  it("keeps whole seconds already inside the range", () => {
    expect(clampDwell(3)).toBe(3);
    expect(clampDwell(9)).toBe(9);
    expect(clampDwell(15)).toBe(15);
  });
});

describe("parseExcludedApps", () => {
  it("splits on commas and newlines, trimming and lowercasing", () => {
    expect(parseExcludedApps("  Banking, Photoshop\nTerminal  ")).toEqual([
      "banking",
      "photoshop",
      "terminal",
    ]);
    expect(parseExcludedApps("Banking\r\nPhotoshop")).toEqual([
      "banking",
      "photoshop",
    ]);
  });

  it("drops empty entries and duplicates, keeping the first order", () => {
    expect(parseExcludedApps("zeta, , alpha,\n\nzeta, ALPHA,,")).toEqual([
      "zeta",
      "alpha",
    ]);
  });

  it("treats mixed-case entries as the same fragment", () => {
    expect(parseExcludedApps("BAnking, banking, BaNkInG")).toEqual(["banking"]);
  });

  it("returns an empty list for blank text or a non-string", () => {
    expect(parseExcludedApps("")).toEqual([]);
    expect(parseExcludedApps("  ,\n,  ")).toEqual([]);
    expect(parseExcludedApps(undefined as unknown as string)).toEqual([]);
  });

  it("caps each entry at 128 characters", () => {
    const long = "x".repeat(200);
    const parsed = parseExcludedApps(long);
    expect(parsed).toHaveLength(1);
    expect(parsed[0]).toHaveLength(128);
  });

  it("caps the list at 64 entries", () => {
    const many = Array.from({ length: 80 }, (_, i) => `app ${i}`);
    const parsed = parseExcludedApps(many.join(","));
    expect(parsed).toHaveLength(64);
    expect(parsed[0]).toBe("app 0");
    expect(parsed[63]).toBe("app 63");
  });
});

describe("formatExcludedApps", () => {
  it("joins fragments with a comma and a space", () => {
    expect(formatExcludedApps(["banking", "photoshop", "terminal"])).toBe(
      "banking, photoshop, terminal",
    );
  });

  it("renders an empty list as an empty string", () => {
    expect(formatExcludedApps([])).toBe("");
  });
});

describe("normalizePointerSettings", () => {
  it("returns the defaults for a non-object", () => {
    for (const raw of [null, undefined, 42, "banking", true]) {
      expect(normalizePointerSettings(raw)).toEqual(DEFAULT_POINTER_SETTINGS);
    }
  });

  it("fills missing fields from the defaults", () => {
    expect(normalizePointerSettings({})).toEqual(DEFAULT_POINTER_SETTINGS);
  });

  it("replaces wrong-typed fields with the defaults", () => {
    expect(
      normalizePointerSettings({
        enabled: "yes",
        dwellSeconds: "fast",
        excludedApps: "banking",
        suppressFullscreen: 0,
      }),
    ).toEqual(DEFAULT_POINTER_SETTINGS);
  });

  it("keeps valid values and clamps the dwell time", () => {
    expect(
      normalizePointerSettings({
        enabled: false,
        dwellSeconds: 99,
        excludedApps: ["  Banking ", "banking", "", "Photoshop"],
        suppressFullscreen: false,
      }),
    ).toEqual({
      enabled: false,
      dwellSeconds: 15,
      excludedApps: ["banking", "photoshop"],
      suppressFullscreen: false,
    });

    expect(
      normalizePointerSettings({ dwellSeconds: 2 }).dwellSeconds,
    ).toBe(3);
    expect(
      normalizePointerSettings({ dwellSeconds: 7.6 }).dwellSeconds,
    ).toBe(8);
  });

  it("ignores non-string entries inside the excluded list", () => {
    expect(
      normalizePointerSettings({
        excludedApps: ["Banking", 7, null, "photoshop"],
      }).excludedApps,
    ).toEqual(["banking", "photoshop"]);
  });

  it("does not share the default excluded list", () => {
    const normalized = normalizePointerSettings({});
    normalized.excludedApps.push("banking");
    expect(DEFAULT_POINTER_SETTINGS.excludedApps).toEqual([]);
    expect(normalizePointerSettings({}).excludedApps).toEqual([]);
  });
});
