import { describe, expect, it } from "vitest";

import { positionToPct, pxToSlider, sizeClamp, sliderToPx, wheelResize } from "./geometry";

/** 1920×1080 physical, 200px window → matches Rust overlay_xy inverse. */
const SW = 1920;
const SH = 1080;

describe("positionToPct (center-anchored, mirrors window_manager::overlay_xy)", () => {
  it("center of screen → h=0.5", () => {
    // x = 960 - 100 = 860 (center-anchored left edge for 200px window)
    const r = positionToPct(860, 500, 200, 200, SW, SH);
    expect(r.h).toBeCloseTo(0.5);
  });

  it("bottom edge (v=1.0) round-trips: y = SH - phys", () => {
    const r = positionToPct(860, SH - 200, 200, 200, SW, SH);
    expect(r.v).toBeCloseTo(1.0);
  });

  it("snaps to h=0.5 within 20px of center", () => {
    // Center at 960+15 → pct slightly off; must snap to exactly 0.5.
    const r = positionToPct(860 + 15, 500, 200, 200, SW, SH, 20);
    expect(r.h).toBe(0.5);
    expect(r.snappedH).toBe(true);
  });

  it("does NOT snap beyond 20px of center", () => {
    const r = positionToPct(860 + 40, 500, 200, 200, SW, SH, 20);
    expect(r.snappedH).toBe(false);
    expect(r.h).toBeGreaterThan(0.5);
  });

  it("snaps to bottom when window bottom is within 20px", () => {
    const r = positionToPct(860, SH - 200 - 15, 200, 200, SW, SH, 20);
    expect(r.v).toBe(1.0);
    expect(r.snappedV).toBe(true);
  });

  it("clamps out-of-range positions into 0..1", () => {
    const r = positionToPct(-500, -500, 200, 200, SW, SH);
    expect(r.h).toBeGreaterThanOrEqual(0);
    expect(r.v).toBeGreaterThanOrEqual(0);
    const r2 = positionToPct(SW + 400, SH + 400, 200, 200, SW, SH);
    expect(r2.h).toBeLessThanOrEqual(1);
    expect(r2.v).toBeLessThanOrEqual(1);
  });

  it("degrades safely on zero/negative screen metrics", () => {
    const r = positionToPct(100, 100, 200, 200, 0, 0);
    expect(Number.isFinite(r.h)).toBe(true);
    expect(Number.isFinite(r.v)).toBe(true);
  });
});

describe("wheelResize (±10px, per-target rails)", () => {
  it("wheel up grows, wheel down shrinks", () => {
    expect(wheelResize("wakeup", 200, -100)).toBe(210);
    expect(wheelResize("wakeup", 200, 100)).toBe(190);
  });

  it("clamps Orb/Waves to 100–400", () => {
    expect(wheelResize("wakeup", 395, -100)).toBe(400);
    expect(wheelResize("waves", 105, 100)).toBe(100);
    expect(sizeClamp("waves")).toEqual([100, 400]);
  });

  it("clamps Loading to 40–160", () => {
    expect(wheelResize("loading", 155, -100)).toBe(160);
    expect(wheelResize("loading", 45, 100)).toBe(40);
    expect(sizeClamp("loading")).toEqual([40, 160]);
  });
});

describe("sliderToPx / pxToSlider (plan 04 §2: 0–100 scale, px in background)", () => {
  it("maps rails exactly (0→min, 100→max)", () => {
    expect(sliderToPx("wakeup", 0)).toBe(100);
    expect(sliderToPx("wakeup", 100)).toBe(400);
    expect(sliderToPx("loading", 0)).toBe(40);
    expect(sliderToPx("loading", 100)).toBe(160);
    expect(pxToSlider("wakeup", 100)).toBe(0);
    expect(pxToSlider("wakeup", 400)).toBe(100);
  });

  it("orb step is exactly 3px (no dead steps)", () => {
    expect(sliderToPx("wakeup", 50)).toBe(250);
    expect(sliderToPx("waves", 25)).toBe(175);
    expect(pxToSlider("wakeup", 250)).toBe(50);
  });

  it("round-trips stably for orb/waves (on-grid px only: 100+3k)", () => {
    for (const px of [100, 160, 220, 280, 340, 400]) {
      expect(sliderToPx("wakeup", pxToSlider("wakeup", px))).toBe(px);
    }
  });

  it("loading quantizes (±1px documented): adjacent steps may share px", () => {
    // 1 slider step = 1.2px → callers must guard on px change.
    const a = sliderToPx("loading", 50);
    const b = sliderToPx("loading", 51);
    expect(Math.abs(a - b)).toBeLessThanOrEqual(2);
    expect(pxToSlider("loading", 80)).toBeGreaterThanOrEqual(0);
    expect(pxToSlider("loading", 80)).toBeLessThanOrEqual(100);
  });

  it("clamps out-of-range inputs", () => {
    expect(sliderToPx("wakeup", -5)).toBe(100);
    expect(sliderToPx("wakeup", 150)).toBe(400);
    expect(pxToSlider("loading", 10)).toBe(0);
    expect(pxToSlider("loading", 999)).toBe(100);
  });

  it("current size seats the thumb (init-from-draft contract)", () => {
    // A 250px orb opens the slider at exactly 50 (100–400 rails).
    expect(pxToSlider("wakeup", 250)).toBe(50);
    // An 80px loader opens at 100*(80-40)/120 ≈ 33.
    expect(pxToSlider("loading", 80)).toBe(33);
  });
});
