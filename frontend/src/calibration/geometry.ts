/**
 * Calibration geometry — pure, unit-tested mirror of the Rust placement
 * math (`window_manager::overlay_xy` is center-anchored, so the inverse
 * here is too: pct = window-center / monitor-size, physical px).
 *
 * Drag release → pct (with magnetic snap) → `calibration_report_position`
 * → Rust clamps + applies the same center-anchored formula → the element
 * stays exactly under the cursor. Wheel → ±10px → Rust clamps per target.
 */

export type CalibrationTarget = "wakeup" | "waves" | "loading";

/** Per-target size clamp rails (task §4): Orb/Waves 100–400, Loading 40–160. */
export function sizeClamp(target: CalibrationTarget): [number, number] {
  return target === "loading" ? [40, 160] : [100, 400];
}

/** Magnetic snap distance in logical px (task §3): 20px to center/bottom. */
export const SNAP_PX = 20;

/**
 * Window position (physical px, top-left) → normalized center-anchored
 * pct, with gentle magnetic snap when within `snapPx` (physical) of the
 * horizontal center (50%) or the bottom edge (100%).
 */
export function positionToPct(
  winX: number,
  winY: number,
  winPhysW: number,
  winPhysH: number,
  screenW: number,
  screenH: number,
  snapPx = 20 * 1.0
): { h: number; v: number; snappedH: boolean; snappedV: boolean } {
  const safe = (n: number, min: number) =>
    Number.isFinite(n) && n > 0 ? n : min;
  const sw = safe(screenW, 1);
  const sh = safe(screenH, 1);
  const w = Math.min(safe(winPhysW, 1), sw);
  const h = Math.min(safe(winPhysH, 1), sh);

  let hPct = (winX + w / 2) / sw;
  let vPct = (winY + h / 2) / sh;
  let snappedH = false;
  let snappedV = false;

  // Snap to horizontal center when the center is within snapPx.
  if (Math.abs(hPct * sw - sw / 2) <= snapPx) {
    hPct = 0.5;
    snappedH = true;
  }
  // Snap to the bottom edge when the window bottom is within snapPx.
  if (sh - (winY + h) <= snapPx) {
    vPct = 1.0;
    snappedV = true;
  }

  return {
    h: Math.min(1, Math.max(0, hPct)),
    v: Math.min(1, Math.max(0, vPct)),
    snappedH,
    snappedV,
  };
}

/**
 * Wheel step (task §4): deltaY < 0 grows +10px, otherwise shrinks −10px,
 * clamped to the target's rails. `step` is injectable for tests.
 */
export function wheelResize(
  target: CalibrationTarget,
  currentSize: number,
  deltaY: number,
  step = 10
): number {
  const [min, max] = sizeClamp(target);
  const next = deltaY < 0 ? currentSize + step : currentSize - step;
  return Math.min(max, Math.max(min, next));
}

/**
 * Size-slider scale (plan 04 §2): the HUD slider reads 0–100 for every
 * target; px is controlled in the background (Rust drafts, badges, save).
 * Orb/Waves map exactly (3px/step); Loading quantizes (1.2px/step —
 * callers must guard on px change so duplicate steps never invoke).
 */
export const SLIDER_MIN = 0;
export const SLIDER_MAX = 100;

export function sliderToPx(target: CalibrationTarget, s01: number): number {
  const [min, max] = sizeClamp(target);
  const s = Math.min(SLIDER_MAX, Math.max(SLIDER_MIN, s01));
  return Math.round(min + (s / 100) * (max - min));
}

export function pxToSlider(target: CalibrationTarget, px: number): number {
  const [min, max] = sizeClamp(target);
  const p = Math.min(max, Math.max(min, px));
  return Math.round(((p - min) / (max - min)) * 100);
}
