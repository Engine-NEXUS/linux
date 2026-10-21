import { describe, expect, it } from "vitest";
import {
  arrowHead,
  GAP,
  MARGIN,
  physToCss,
  placeCallout,
  Rect,
  Size,
} from "./tourGeometry";

const SCREEN: Size = { w: 1920, h: 1080 };
const CALLOUT: Size = { w: 320, h: 110 };

const inside = (r: Rect, s: Size) =>
  r.x >= MARGIN - 0.001 &&
  r.y >= MARGIN - 0.001 &&
  r.x + r.w <= s.w - MARGIN + 0.001 &&
  r.y + r.h <= s.h - MARGIN + 0.001;

const overlap = (a: Rect, b: Rect) =>
  a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;

describe("placeCallout", () => {
  it("prefers the right side with a GAP when there is room", () => {
    const target: Rect = { x: 200, y: 300, w: 300, h: 200 };
    const p = placeCallout(target, SCREEN, CALLOUT);
    expect(p.side).toBe("right");
    expect(p.box.x).toBe(target.x + target.w + GAP);
    expect(p.box.y).toBeCloseTo(target.y + target.h / 2 - CALLOUT.h / 2);
    expect(p.leader).not.toBeNull();
  });

  it("flips to the left near the right screen edge", () => {
    const target: Rect = { x: 1500, y: 300, w: 300, h: 200 };
    const p = placeCallout(target, SCREEN, CALLOUT);
    expect(p.side).toBe("left");
    expect(p.box.x + p.box.w).toBeLessThanOrEqual(target.x - GAP + 0.001);
    expect(inside(p.box, SCREEN)).toBe(true);
  });

  it("goes below/above when both sides are blocked (wide target)", () => {
    const target: Rect = { x: 60, y: 200, w: 1800, h: 200 };
    const p = placeCallout(target, SCREEN, CALLOUT);
    expect(["below", "above"]).toContain(p.side);
    expect(overlap(p.box, target)).toBe(false);
    expect(inside(p.box, SCREEN)).toBe(true);
  });

  it("clamps the cross axis to the margin (target at the top edge)", () => {
    const target: Rect = { x: 200, y: 0, w: 200, h: 40 };
    const p = placeCallout(target, SCREEN, CALLOUT);
    expect(inside(p.box, SCREEN)).toBe(true);
  });

  it("keeps clear of reserved rects (the orb)", () => {
    const target: Rect = { x: 200, y: 300, w: 300, h: 200 };
    // Orb parked exactly where the right-side callout would land.
    const orb: Rect = { x: 520, y: 280, w: 340, h: 240 };
    const p = placeCallout(target, SCREEN, CALLOUT, [orb]);
    expect(overlap(p.box, orb)).toBe(false);
    expect(p.side).not.toBe("right");
  });

  it("never overlaps the target for ordinary targets", () => {
    for (const target of [
      { x: 100, y: 100, w: 200, h: 120 },
      { x: 800, y: 500, w: 400, h: 300 },
      { x: 1600, y: 900, w: 250, h: 150 },
      { x: 20, y: 20, w: 120, h: 60 },
    ] as Rect[]) {
      const p = placeCallout(target, SCREEN, CALLOUT);
      expect(overlap(p.box, target)).toBe(false);
      expect(inside(p.box, SCREEN)).toBe(true);
    }
  });

  it("puts a callout inside a screen-sized target", () => {
    const target: Rect = { x: 0, y: 0, w: 1920, h: 1080 };
    const p = placeCallout(target, SCREEN, CALLOUT);
    expect(p.side).toBe("inside");
    expect(inside(p.box, SCREEN)).toBe(true);
  });

  it("uses a default zone and no leader without a target", () => {
    const p = placeCallout(null, SCREEN, CALLOUT);
    expect(p.side).toBe("default");
    expect(p.leader).toBeNull();
    expect(inside(p.box, SCREEN)).toBe(true);
    expect(p.box.x + p.box.w / 2).toBeCloseTo(SCREEN.w / 2);
  });

  it("still returns an on-screen box on a tiny screen", () => {
    const tiny: Size = { w: 300, h: 200 };
    const p = placeCallout({ x: 100, y: 60, w: 100, h: 60 }, tiny, CALLOUT);
    expect(p.box.x).toBeGreaterThanOrEqual(MARGIN - 0.001);
    expect(p.box.y).toBeGreaterThanOrEqual(MARGIN - 0.001);
    expect(p.box.x + p.box.w).toBeLessThanOrEqual(tiny.w - MARGIN + 0.001);
    expect(p.box.y + p.box.h).toBeLessThanOrEqual(tiny.h - MARGIN + 0.001);
  });

  it("leader endpoints touch the target and the callout edges", () => {
    const target: Rect = { x: 200, y: 300, w: 300, h: 200 };
    const p = placeCallout(target, SCREEN, CALLOUT);
    const { from, to } = p.leader!;
    const on = (pt: { x: number; y: number }, r: Rect) =>
      pt.x >= r.x - 0.001 && pt.x <= r.x + r.w + 0.001 &&
      pt.y >= r.y - 0.001 && pt.y <= r.y + r.h + 0.001;
    expect(on(from, target)).toBe(true);
    expect(on(to, p.box)).toBe(true);
    // Right-side placement: leader leaves the target's right edge.
    expect(from.x).toBeCloseTo(target.x + target.w);
    expect(to.x).toBeCloseTo(p.box.x);
  });
});

describe("arrowHead", () => {
  it("has its tip at the target end and a base behind it", () => {
    const pts = arrowHead({ x: 0, y: 0 }, { x: 100, y: 0 }, 16, 7).split(" ");
    expect(pts[0]).toBe("100,0");
    const [bx1] = pts[1].split(",").map(Number);
    const [bx2] = pts[2].split(",").map(Number);
    expect(bx1).toBeCloseTo(84);
    expect(bx2).toBeCloseTo(84);
  });

  it("does not blow up on a zero-length leader", () => {
    expect(() => arrowHead({ x: 5, y: 5 }, { x: 5, y: 5 })).not.toThrow();
  });
});

describe("physToCss", () => {
  it("equals px/dpr when the viewport covers the monitor", () => {
    // 1920x1080 logical viewport on a 2400x1350 physical monitor (dpr 1.25).
    const css = physToCss({ x: 250, y: 125, w: 500, h: 250 }, { w: 2400, h: 1350 }, { w: 1920, h: 1080 });
    expect(css).toEqual({ x: 200, y: 100, w: 400, h: 200 });
  });

  it("degrades proportionally on a viewport/monitor mismatch", () => {
    const css = physToCss({ x: 100, y: 100, w: 100, h: 100 }, { w: 2000, h: 1000 }, { w: 1000, h: 1000 });
    expect(css.x).toBe(50);
    expect(css.y).toBe(100);
  });

  it("survives a zero monitor size", () => {
    const css = physToCss({ x: 1, y: 2, w: 3, h: 4 }, { w: 0, h: 0 }, { w: 100, h: 100 });
    expect(css).toEqual({ x: 1, y: 2, w: 3, h: 4 });
  });
});
