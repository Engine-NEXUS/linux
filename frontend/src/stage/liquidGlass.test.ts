import { describe, it, expect } from "vitest";

export function calculatePhysicalHitbox(
  cssRect: { x: number; y: number; width: number; height: number },
  dpr: number = 1
) {
  return {
    x: Math.round(cssRect.x * dpr),
    y: Math.round(cssRect.y * dpr),
    w: Math.round(cssRect.width * dpr),
    h: Math.round(cssRect.height * dpr),
  };
}

export function resolveLuminanceMode(
  y: number,
  current: "light" | "dark" = "dark",
  thresholdHigh: number = 140,
  thresholdLow: number = 115
): "light" | "dark" {
  if (current === "dark" && y > thresholdHigh) {
    return "light";
  }
  if (current === "light" && y < thresholdLow) {
    return "dark";
  }
  return current;
}

describe("Liquid Glass & Hitbox Exception Testing", () => {
  it("scales CSS bounding box to exact physical pixels for DPI hit-testing", () => {
    const cssRect = { x: 100, y: 50, width: 220, height: 48 };

    // Standard 1x scale
    expect(calculatePhysicalHitbox(cssRect, 1.0)).toEqual({
      x: 100,
      y: 50,
      w: 220,
      h: 48,
    });

    // 1.25x scaling (standard Windows display scaling)
    expect(calculatePhysicalHitbox(cssRect, 1.25)).toEqual({
      x: 125,
      y: 63,
      w: 275,
      h: 60,
    });

    // 1.5x scaling (high DPI laptop)
    expect(calculatePhysicalHitbox(cssRect, 1.5)).toEqual({
      x: 150,
      y: 75,
      w: 330,
      h: 72,
    });
  });

  it("applies 25-point hysteresis on luminance mode transitions", () => {
    // Starting in dark
    let mode: "light" | "dark" = "dark";

    // Value inside dead-band maintains dark
    mode = resolveLuminanceMode(120, mode);
    expect(mode).toBe("dark");

    // Crossing high threshold switches to light
    mode = resolveLuminanceMode(145, mode);
    expect(mode).toBe("light");

    // Value in dead-band maintains light
    mode = resolveLuminanceMode(130, mode);
    expect(mode).toBe("light");

    // Value dropping below low threshold switches back to dark
    mode = resolveLuminanceMode(105, mode);
    expect(mode).toBe("dark");
  });
});
