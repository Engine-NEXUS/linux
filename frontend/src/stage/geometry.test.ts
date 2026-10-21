import { describe, expect, it } from "vitest";
import { dockRect, orbRect, loadingRect } from "./geometry";

describe("stage geometry mirrors legacy window placement", () => {
  it("docks a 400x1000 sidebar bottom-right with gap + taskbar reserve", () => {
    const r = dockRect(400, 1000, 1920, 1080);
    expect(r).toEqual({ x: 1920 - 400 - 12, y: 1080 - 1000 - 48 - 12, w: 400, h: 1000 });
  });

  it("docks wider panels with the same margins", () => {
    const pr = dockRect(500, 1000, 1920, 1080);
    expect(pr.x).toBe(1920 - 500 - 12);
    expect(pr.y).toBe(1080 - 1000 - 48 - 12);
    const arch = dockRect(900, 1000, 2560, 1440);
    expect(arch).toEqual({ x: 2560 - 900 - 12, y: 1440 - 1000 - 48 - 12, w: 900, h: 1000 });
  });

  it("clamps panels larger than the screen", () => {
    const r = dockRect(900, 1000, 800, 600);
    expect(r).toEqual({ x: 0, y: 0, w: 800, h: 600 });
  });

  it("places the orb center-bottom by default like position_orb", () => {
    const r = orbRect(0.5, 1.0, 200, 1920, 1080);
    expect(r).toEqual({ x: 960 - 100, y: 1080 - 200, w: 200, h: 200 });
  });

  it("clamps orb pct/size to the Rust ranges and keeps it on-screen", () => {
    const r = orbRect(9, -2, 9999, 1920, 1080);
    expect(r.w).toBe(400);
    expect(r.x).toBeGreaterThanOrEqual(0);
    expect(r.y).toBeGreaterThanOrEqual(0);
    expect(r.x + r.w).toBeLessThanOrEqual(1920);
    expect(r.y + r.h).toBeLessThanOrEqual(1080);
  });

  it("loading indicator is a fixed 80px square", () => {
    const r = loadingRect();
    expect(r.w).toBe(80);
    expect(r.h).toBe(80);
  });
});
