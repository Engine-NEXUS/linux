/**
 * Pure placement math for the narrated screen tour (all CSS px).
 *
 * One callout at a time: given the thing being pointed at (`target`), the
 * screen, the measured callout box and rects that must stay clear (the orb),
 * pick where the callout goes and where the leader line runs. No DOM here —
 * unit-tested in tourGeometry.test.ts.
 */

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Size {
  w: number;
  h: number;
}

export interface Point {
  x: number;
  y: number;
}

export type Side = "right" | "left" | "below" | "above" | "inside" | "default";

export interface Placement {
  box: Rect;
  side: Side;
  /** Leader from the target edge to the callout edge; null with no target. */
  leader: { from: Point; to: Point } | null;
}

export const GAP = 28;
export const MARGIN = 16;
export const TARGET_PAD = 8;
const LARGE_TARGET_FRACTION = 0.6;

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

function overlaps(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
}

function pad(r: Rect, p: number): Rect {
  return { x: r.x - p, y: r.y - p, w: r.w + 2 * p, h: r.h + 2 * p };
}

function center(r: Rect): Point {
  return { x: r.x + r.w / 2, y: r.y + r.h / 2 };
}

function fits(r: Rect, screen: Size): boolean {
  return (
    r.x >= MARGIN &&
    r.y >= MARGIN &&
    r.x + r.w <= screen.w - MARGIN &&
    r.y + r.h <= screen.h - MARGIN
  );
}

/** Nearest point of `r` to `p` (p itself when inside). */
function nearestOn(r: Rect, p: Point): Point {
  return { x: clamp(p.x, r.x, r.x + r.w), y: clamp(p.y, r.y, r.y + r.h) };
}

function leaderFor(target: Rect, box: Rect): { from: Point; to: Point } {
  const onTarget = nearestOn(target, center(box));
  const onBox = nearestOn(box, onTarget);
  return { from: onTarget, to: onBox };
}

/**
 * Place a callout for `target` (null = no pointer, e.g. whole-screen item).
 * Preference: right, left, below, above (first that fits and stays clear of
 * the target and `reserved`); for a target covering most of the screen:
 * inside corners. Always returns a placement —
 * the last resort is clamped on-screen even if it overlaps something.
 */
export function placeCallout(
  target: Rect | null,
  screen: Size,
  callout: Size,
  reserved: Rect[] = [],
): Placement {
  const cw = Math.min(callout.w, Math.max(0, screen.w - 2 * MARGIN));
  const ch = Math.min(callout.h, Math.max(0, screen.h - 2 * MARGIN));
  const mk = (x: number, y: number): Rect => ({
    x: clamp(x, MARGIN, Math.max(MARGIN, screen.w - MARGIN - cw)),
    y: clamp(y, MARGIN, Math.max(MARGIN, screen.h - MARGIN - ch)),
    w: cw,
    h: ch,
  });
  const clear = (r: Rect, t: Rect | null): boolean =>
    (!t || !overlaps(r, pad(t, TARGET_PAD))) && !reserved.some((o) => overlaps(r, pad(o, TARGET_PAD)));

  if (!target) {
    const cx = (screen.w - cw) / 2;
    const candidates = [mk(cx, 64), mk(cx, screen.h - ch - 64), mk(cx, (screen.h - ch) / 2)];
    const box = candidates.find((c) => clear(c, null)) ?? candidates[0];
    return { box, side: "default", leader: null };
  }

  const t = target;
  const cands: { side: Side; box: Rect }[] = [];
  const largeTarget = t.w * t.h >= LARGE_TARGET_FRACTION * screen.w * screen.h;
  if (largeTarget) {
    const inset = 24;
    cands.push(
      { side: "inside", box: mk(t.x + inset, t.y + inset) },
      { side: "inside", box: mk(t.x + t.w - cw - inset, t.y + inset) },
      { side: "inside", box: mk(t.x + inset, t.y + t.h - ch - inset) },
      { side: "inside", box: mk(t.x + t.w - cw - inset, t.y + t.h - ch - inset) },
    );
  }
  cands.push(
    { side: "right", box: mk(t.x + t.w + GAP, t.y + t.h / 2 - ch / 2) },
    { side: "left", box: mk(t.x - GAP - cw, t.y + t.h / 2 - ch / 2) },
    { side: "below", box: mk(t.x + t.w / 2 - cw / 2, t.y + t.h + GAP) },
    { side: "above", box: mk(t.x + t.w / 2 - cw / 2, t.y - GAP - ch) },
  );

  // Inside placements deliberately overlap the (huge) target, so only
  // `reserved` applies to them.
  const valid = cands.filter((c) =>
    c.side === "inside"
      ? fits(c.box, screen) && !reserved.some((o) => overlaps(c.box, pad(o, TARGET_PAD)))
      : fits(c.box, screen) && clear(c.box, t),
  );
  // Candidates are already in preference order (inside corners for huge
  // targets, then right, left, below, above): first valid wins, so the
  // callout position is predictable instead of jumping between sides.
  let chosen: { side: Side; box: Rect } | undefined = valid[0];
  if (!chosen) {
    // Last resort: the roomiest side, clamped on-screen.
    const room: [Side, number][] = [
      ["right", screen.w - (t.x + t.w)],
      ["left", t.x],
      ["below", screen.h - (t.y + t.h)],
      ["above", t.y],
    ];
    room.sort((a, b) => b[1] - a[1]);
    const side = room[0][0];
    chosen = cands.find((c) => c.side === side) ?? cands[0];
  }
  return { box: chosen.box, side: chosen.side, leader: leaderFor(t, chosen.box) };
}

/**
 * Arrowhead polygon points ("x,y x,y x,y") with its tip at `tip`, pointing
 * away from `tail` (i.e. along tail → tip). Pure.
 */
export function arrowHead(tail: Point, tip: Point, len = 16, half = 7): string {
  const dx = tip.x - tail.x;
  const dy = tip.y - tail.y;
  const d = Math.hypot(dx, dy) || 1;
  const ux = dx / d;
  const uy = dy / d;
  const bx = tip.x - ux * len;
  const by = tip.y - uy * len;
  const nx = -uy;
  const ny = ux;
  const f = (n: number) => Math.round(n * 10) / 10;
  return `${f(tip.x)},${f(tip.y)} ${f(bx + nx * half)},${f(by + ny * half)} ${f(bx - nx * half)},${f(by - ny * half)}`;
}

/**
 * Physical screen px (Rust) → CSS px, using the ratio of the real viewport
 * to the monitor size Rust reported. Equals px / devicePixelRatio when the
 * stage window exactly covers the monitor, and degrades proportionally when
 * it does not (an assumption the live check must confirm).
 */
export function physToCss(
  phys: Rect,
  monitor: Size,
  viewport: Size,
): Rect {
  const sx = monitor.w > 0 ? viewport.w / monitor.w : 1;
  const sy = monitor.h > 0 ? viewport.h / monitor.h : 1;
  return { x: phys.x * sx, y: phys.y * sy, w: phys.w * sx, h: phys.h * sy };
}
