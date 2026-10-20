/**
 * Tests for the Tauri `IAuraHost` rect and viewport maths.
 *
 * The renderer is saystack's and is well covered upstream (451 tests in that
 * repo). What can break *here* is the host contract: a wrong box size, an
 * off-by-one in the margin, or a degenerate rect during the roam transition
 * when the orb is momentarily unmeasurable. Those are the bugs that make an
 * aura blink off or smear across the screen, so they are pinned here.
 *
 * The viewport is injected via `options.viewport` rather than stubbed on a
 * global `document` — vitest runs in the `node` environment (see
 * vitest.config.ts), where there is no DOM to stub. The host reads the
 * viewport once per frame, which is worth not needing a browser for.
 */

import { describe, expect, it } from "vitest";
import { createStageAuraHost } from "./stageAuraHost";

const VIEWPORT_W = 1920;
const VIEWPORT_H = 1080;
const VIEWPORT = { width: VIEWPORT_W, height: VIEWPORT_H };

/** Minimal stand-in for the canvas the host writes sizing onto. */
function fakeCanvas(): HTMLCanvasElement {
  return {
    width: 0,
    height: 0,
    style: { display: "", width: "", height: "", transform: "", filter: "" },
  } as unknown as HTMLCanvasElement;
}

/** Anchor stub reporting a fixed box. */
function anchorAt(
  left: number,
  top: number,
  width: number,
  height: number,
  connected = true,
): HTMLElement {
  return {
    isConnected: connected,
    getBoundingClientRect: () => ({
      left,
      top,
      width,
      height,
      right: left + width,
      bottom: top + height,
    }),
  } as unknown as HTMLElement;
}

const FIT = { reach: 12, resolution: 1, look: {} } as never;

describe("createStageAuraHost — target", () => {
  it("measures the anchor with symmetric padding", () => {
    const canvas = fakeCanvas();
    const host = createStageAuraHost({
      canvas,
      getAnchor: () => anchorAt(100, 200, 180, 180),
      padding: 6, viewport: VIEWPORT,
    });

    const rect = host.target();
    expect(rect.x).toBe(94);
    expect(rect.y).toBe(194);
    expect(rect.w).toBe(192);
    expect(rect.h).toBe(192);
  });

  it("marks the rect as an outer perimeter, not an inner hole", () => {
    // isInner:false is what makes the renderer draw the rim outside the orb
    // instead of clipping the fill inside it. Flipping this is the difference
    // between a halo and an inverted mask.
    const host = createStageAuraHost({
      canvas: fakeCanvas(),
      getAnchor: () => anchorAt(0, 0, 180, 180),
      padding: 6, viewport: VIEWPORT,
    });
    expect(host.target().isInner).toBe(false);
    expect(host.target().isPage).toBe(false);
  });

  it("defaults the corner radius to a circle", () => {
    // The orb is round; a square-cornered rim would visibly not hug it.
    const host = createStageAuraHost({
      canvas: fakeCanvas(),
      getAnchor: () => anchorAt(0, 0, 180, 180),
      padding: 6, viewport: VIEWPORT,
    });
    const rect = host.target();
    expect(rect.r).toBeCloseTo(rect.w / 2, 5);
  });

  it("never exceeds half the shorter side", () => {
    // A radius larger than half the box makes the SDF degenerate.
    const host = createStageAuraHost({
      canvas: fakeCanvas(),
      getAnchor: () => anchorAt(0, 0, 200, 40),
      padding: 6,
      radius: 9999, viewport: VIEWPORT,
    });
    const rect = host.target();
    expect(rect.r).toBeLessThanOrEqual(Math.min(rect.w, rect.h) / 2 + 1e-6);
  });

  it("returns a degenerate rect for a detached anchor", () => {
    // Happens during unmount / route changes. A 0×0 rect makes the renderer
    // hide rather than draw, which is the behaviour we want.
    const host = createStageAuraHost({
      canvas: fakeCanvas(),
      getAnchor: () => anchorAt(0, 0, 180, 180, false), viewport: VIEWPORT,
    });
    const rect = host.target();
    expect(rect.w).toBe(0);
    expect(rect.h).toBe(0);
  });

  it("returns a degenerate rect for a null anchor", () => {
    const host = createStageAuraHost({ canvas: fakeCanvas(), getAnchor: () => null, viewport: VIEWPORT,
    });
    expect(host.target().w).toBe(0);
  });

  it("returns a degenerate rect for an unlaid-out anchor", () => {
    // The roam transition can measure 0×0 for a frame or two.
    const host = createStageAuraHost({
      canvas: fakeCanvas(),
      getAnchor: () => anchorAt(50, 50, 0, 0), viewport: VIEWPORT,
    });
    expect(host.target().w).toBe(0);
  });
});

describe("createStageAuraHost — place", () => {
  function hostFor(anchor: HTMLElement | null) {
    const canvas = fakeCanvas();
    const host = createStageAuraHost({ canvas, getAnchor: () => anchor, padding: 6, viewport: VIEWPORT,
    });
    return { canvas, host };
  }

  it("sizes the canvas to cover the rim plus its reach", () => {
    const { canvas, host } = hostFor(anchorAt(800, 600, 180, 180));
    const rect = host.target();
    const view = host.place(rect, FIT);

    expect(view).not.toBeNull();
    // Must be strictly larger than the orb, or the glow gets clipped.
    expect(view!.width).toBeGreaterThan(rect.w);
    expect(view!.height).toBeGreaterThan(rect.h);
    expect(canvas.style.display).toBe("block");
  });

  it("positions the canvas at the box origin", () => {
    const { canvas, host } = hostFor(anchorAt(800, 600, 180, 180));
    const view = host.place(host.target(), FIT)!;
    expect(canvas.style.transform).toBe(`translate(${view.x}px, ${view.y}px)`);
  });

  it("keeps the canvas inside the viewport", () => {
    // The orb can park near the screen edge; the aura must not overflow it,
    // or the stage window's own bounds clip the glow.
    const { host } = hostFor(anchorAt(0, 0, 180, 180));
    const view = host.place(host.target(), FIT)!;
    expect(view.x).toBeGreaterThanOrEqual(0);
    expect(view.y).toBeGreaterThanOrEqual(0);
    expect(view.x + view.width).toBeLessThanOrEqual(VIEWPORT_W);
    expect(view.y + view.height).toBeLessThanOrEqual(VIEWPORT_H);
  });

  it("clamps an orb hanging off the bottom-right edge", () => {
    const { host } = hostFor(anchorAt(VIEWPORT_W - 100, VIEWPORT_H - 100, 180, 180));
    const view = host.place(host.target(), FIT)!;
    expect(view.x + view.width).toBeLessThanOrEqual(VIEWPORT_W);
    expect(view.y + view.height).toBeLessThanOrEqual(VIEWPORT_H);
  });

  it("returns null when the target is fully off-screen", () => {
    // place() returning null is the documented "hide" signal. Returning a view
    // here would draw the rim in the wrong place entirely.
    const { host } = hostFor(anchorAt(VIEWPORT_W + 500, 500, 180, 180));
    expect(host.place(host.target(), FIT)).toBeNull();
  });

  it("returns null for a degenerate rect", () => {
    const { host } = hostFor(null);
    expect(host.place(host.target(), FIT)).toBeNull();
  });

  it("never applies a CSS blur filter", () => {
    // The renderer blurs in-shader (effects:"shader"). A CSS filter here would
    // double-blur — the single most common way a WebGL glow looks like mush.
    const { canvas, host } = hostFor(anchorAt(800, 600, 180, 180));
    host.place(host.target(), FIT);
    expect(canvas.style.filter).toBe("");
  });

  it("scales the backing store by device pixel ratio", () => {
    const { canvas, host } = hostFor(anchorAt(800, 600, 180, 180));
    const view = host.place(host.target(), FIT)!;
    // devicePixelRatio is 1 under vitest's node environment; the backing store
    // must still be at least as large as the CSS box.
    expect(canvas.width).toBeGreaterThanOrEqual(view.width);
    expect(canvas.height).toBeGreaterThanOrEqual(view.height);
  });

  it("writes a backing store of at least 1px", () => {
    // A 0-sized WebGL backing store makes drawArrays a no-op with no error,
    // which fails silently as "the aura just doesn't work".
    const { canvas, host } = hostFor(anchorAt(800, 600, 1, 1));
    host.place(host.target(), FIT);
    expect(canvas.width).toBeGreaterThanOrEqual(1);
    expect(canvas.height).toBeGreaterThanOrEqual(1);
  });
});

describe("createStageAuraHost — hide and destroy", () => {
  it("hides the canvas", () => {
    const canvas = fakeCanvas();
    const host = createStageAuraHost({ canvas, getAnchor: () => anchorAt(0, 0, 180, 180), viewport: VIEWPORT,
    });
    canvas.style.display = "block";
    host.hide();
    expect(canvas.style.display).toBe("none");
  });

  it("hides on destroy", () => {
    const canvas = fakeCanvas();
    const host = createStageAuraHost({ canvas, getAnchor: () => anchorAt(0, 0, 180, 180), viewport: VIEWPORT,
    });
    canvas.style.display = "block";
    host.destroy();
    expect(canvas.style.display).toBe("none");
  });

  it("always reports a dark background", () => {
    // The stage is a transparent window over an unknown desktop — there is no
    // luminance to adapt to, and probing would only guess wrong.
    const host = createStageAuraHost({ canvas: fakeCanvas(), getAnchor: () => null, viewport: VIEWPORT,
    });
    expect(host.background(0)).toBe("dark");
    expect(host.background(10000)).toBe("dark");
  });
});