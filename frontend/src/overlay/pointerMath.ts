/**
 * Screen pointer math — pure helpers for the Phase-2 overlay marker.
 *
 * The main stage window is fullscreen on the primary monitor, so Rust
 * emits PHYSICAL screen pixels and the marker is CSS-positioned inside
 * the stage (Wayland-safe: no native window positioning involved).
 */

export interface ClampedPoint {
  x: number;
  y: number;
  /** True when the target fell outside the stage and was pulled to the edge. */
  clamped: boolean;
}

/**
 * Clamp a target into the stage rect, keeping a margin so the marker
 * ring + label stay fully visible. Returns whether clamping happened
 * (drives the "edge" badge per Table B #8).
 */
export function clampToStage(
  x: number,
  y: number,
  stageW: number,
  stageH: number,
  margin = 28,
): ClampedPoint {
  if (!Number.isFinite(x) || !Number.isFinite(y) || stageW <= 0 || stageH <= 0) {
    return { x: margin, y: margin, clamped: true };
  }
  const cx = Math.min(Math.max(x, margin), Math.max(margin, stageW - margin));
  const cy = Math.min(Math.max(y, margin), Math.max(margin, stageH - margin));
  return { x: cx, y: cy, clamped: cx !== x || cy !== y };
}

/**
 * Physical screenshot pixels → CSS pixels. Guards divide-by-zero and
 * SSR/test environments where devicePixelRatio is undefined.
 */
export function physicalToCss(physicalPx: number, devicePixelRatio?: number): number {
  const dpr =
    typeof devicePixelRatio === "number" && devicePixelRatio > 0
      ? devicePixelRatio
      : typeof window !== "undefined" && window.devicePixelRatio > 0
        ? window.devicePixelRatio
        : 1;
  return physicalPx / dpr;
}
