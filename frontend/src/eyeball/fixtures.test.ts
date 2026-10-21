import { describe, expect, it } from "vitest";
import { DEFAULT_FIXTURE, fixtureFromQuery, fixtureToQuery, orbRectFor } from "./fixtures";

describe("fixtureFromQuery", () => {
  it("returns the default for a bare ?eyeball", () => {
    expect(fixtureFromQuery("?eyeball")).toEqual(DEFAULT_FIXTURE);
  });

  it("reads every field", () => {
    expect(fixtureFromQuery("?eyeball&state=speaking&style=pebble&level=0.8&pos=bottom&pointer=1&panel=0")).toEqual({
      state: "speaking",
      style: "pebble",
      level: 0.8,
      position: "bottom",
      pointer: true,
      panel: 0,
    });
  });

  it("ignores unknown or broken values instead of guessing", () => {
    const f = fixtureFromQuery("?eyeball&state=dancing&style=box&level=abc&pos=left&panel=-x");
    expect(f).toEqual(DEFAULT_FIXTURE);
  });

  it("clamps the level and the panel", () => {
    expect(fixtureFromQuery("?level=7").level).toBe(1);
    expect(fixtureFromQuery("?level=-2").level).toBe(0);
    expect(fixtureFromQuery("?panel=-40").panel).toBe(0);
  });

  it("round-trips through fixtureToQuery", () => {
    const f = { ...DEFAULT_FIXTURE, state: "thinking" as const, style: "classic" as const, pointer: true, level: 0.25 };
    expect(fixtureFromQuery(fixtureToQuery(f))).toEqual(f);
  });
});

describe("orbRectFor", () => {
  it("centres the orb just under the panel at the top", () => {
    expect(orbRectFor(1920, 1080, 1, "top", 32)).toEqual({ x: 860, y: 32, w: 200, h: 200 });
  });

  it("sits on the bottom edge at the bottom", () => {
    expect(orbRectFor(1920, 1080, 1, "bottom", 32)).toEqual({ x: 860, y: 880, w: 200, h: 200 });
  });

  it("works in physical pixels at 2x", () => {
    expect(orbRectFor(1280, 800, 2, "top", 32)).toEqual({ x: 1080, y: 64, w: 400, h: 400 });
  });

  it("never goes negative on a viewport smaller than the orb", () => {
    const r = orbRectFor(100, 100, 1, "bottom", 0);
    expect(r.x).toBe(0);
    expect(r.y).toBe(0);
  });
});
