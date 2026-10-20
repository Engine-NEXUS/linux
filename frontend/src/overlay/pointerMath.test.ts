import { describe, expect, it } from "vitest";
import { clampToStage, physicalToCss } from "./pointerMath";

describe("clampToStage (Table B #8 edge handling)", () => {
  it("leaves in-bounds targets untouched", () => {
    expect(clampToStage(500, 300, 1920, 1080)).toEqual({ x: 500, y: 300, clamped: false });
  });

  it("pulls off-stage targets to the edge with the clamped flag", () => {
    const r = clampToStage(2500, 300, 1920, 1080);
    expect(r.x).toBe(1920 - 28);
    expect(r.y).toBe(300);
    expect(r.clamped).toBe(true);
  });

  it("pulls negative coordinates to the margin", () => {
    const r = clampToStage(-50, -10, 1920, 1080);
    expect(r).toEqual({ x: 28, y: 28, clamped: true });
  });

  it("clamps both axes independently", () => {
    const r = clampToStage(100, 5000, 1920, 1080);
    expect(r.x).toBe(100);
    expect(r.y).toBe(1080 - 28);
    expect(r.clamped).toBe(true);
  });

  it("handles garbage input without throwing", () => {
    const r = clampToStage(NaN, Infinity, 1920, 1080);
    expect(r.clamped).toBe(true);
    expect(Number.isFinite(r.x)).toBe(true);
    expect(Number.isFinite(r.y)).toBe(true);
  });

  it("respects a custom margin", () => {
    const r = clampToStage(5, 5, 1920, 1080, 60);
    expect(r).toEqual({ x: 60, y: 60, clamped: true });
  });
});

describe("physicalToCss (HiDPI mapping)", () => {
  it("divides by the device pixel ratio", () => {
    expect(physicalToCss(1920, 2)).toBe(960);
    expect(physicalToCss(1000, 1.25)).toBe(800);
  });

  it("falls back to 1:1 on bad ratios", () => {
    expect(physicalToCss(500, 0)).toBe(500);
    expect(physicalToCss(500, -2)).toBe(500);
  });
});
