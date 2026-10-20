/**
 * `IAuraHost` for the Tauri stage window.
 *
 * Saystack's aura renderer is deliberately platform-agnostic: it never touches
 * the DOM and asks its host for four things (`renderer.ts` IAuraHost). The web
 * host in `@saystack/web` implements them against a `position: fixed` canvas on
 * a normal page. We cannot use it directly because the stage window is
 * transparent, click-through most of the time, and always "dark" — so the
 * luminance probing and blend-mode switching in that host would be dead code
 * guessing at an unknown desktop behind the window.
 *
 * So this is ~100 lines instead of a dependency: same renderer, same shader,
 * same band meter, same palette, but a host that knows it is always dark.
 */

import type { AuraBackground, IAuraFit, IAuraHost, IAuraRect, IAuraView } from "@saystack/core";

/**
 * Slack around the target rect for the glow to occupy, in CSS px.
 *
 * Deliberately tight. The orb is now 44–64px (it used to be 180px), so a large
 * margin would make the canvas far bigger than the thing it rims — and a
 * canvas much larger than the orb is exactly the "aesthetic way too big"
 * failure. The renderer's own `reach` is the main contributor; this is just
 * enough to avoid clipping the falloff.
 */
const PLACE_MARGIN_PX = 16;

/** Cap device pixel ratio: past 2× the aura gains nothing visible. */
const MAX_PIXEL_RATIO = 2;

/** The stage viewport, in CSS px. */
export interface StageViewport {
  width: number;
  height: number;
}

export interface StageAuraHostOptions {
  /** The canvas to draw into. Sized and positioned by `place`. */
  canvas: HTMLCanvasElement;
  /** The element whose perimeter the aura rims — the orb wrapper. */
  getAnchor: () => HTMLElement | null;
  /** Extra px of padding around the anchor rect. */
  padding?: number;
  /** Corner radius for the aura rect. Defaults to half the shorter side. */
  radius?: number | null;
  /**
   * Current viewport size, in CSS px.
   *
   * Injected rather than read from `document` so the clamping maths is testable
   * without a DOM (vitest runs in the `node` environment — see
   * vitest.config.ts). Defaults to the live document, which is what production
   * wants; on resize the host re-reads it every frame anyway.
   */
  viewport?: StageViewport;
}

export interface StageAuraHost extends IAuraHost {
  /** Tear down canvas listeners and reset sizing. */
  destroy(): void;
}

export function createStageAuraHost(options: StageAuraHostOptions): StageAuraHost {
  const { canvas, getAnchor } = options;
  const padding = options.padding ?? 8;
  const explicitRadius = options.radius ?? null;
  const injectedViewport = options.viewport;

  /**
   * Read the live viewport each frame.
   *
   * Polling rather than caching is deliberate: the orb roams inside a
   * fullscreen window and the user can resize or move it between monitors, so a
   * cached size would clamp the aura against a stale viewport.
   */
  const readViewport = (): StageViewport => {
    if (injectedViewport) return injectedViewport;
    const root = document.documentElement;
    return { width: root.clientWidth, height: root.clientHeight };
  };

  let appliedWidth = 0;
  let appliedHeight = 0;
  let appliedTransform = "";

  /**
   * Measure the anchor into an aura rect.
   *
   * `isInner: false` tells the renderer this is an outer perimeter to draw
   * around rather than a hole to cut, so the glow sits *outside* the orb and
   * the Lottie face stays crisp on top of it.
   *
   * Returns a degenerate 0×0 rect when the anchor is missing or not yet laid
   * out. The renderer detects that and calls `hide()` instead of drawing —
   * which is what we want during the roam transition, when the orb's box is
   * momentarily unmeasurable.
   */
  const target = (): IAuraRect => {
    const anchor = getAnchor();
    if (anchor === null || !anchor.isConnected) return EMPTY_RECT;

    const box = anchor.getBoundingClientRect();
    if (box.width <= 0 || box.height <= 0) return EMPTY_RECT;

    const w = box.width + padding * 2;
    const h = box.height + padding * 2;
    // A circle is the orb's actual shape; half the shorter side matches it.
    const r = explicitRadius ?? Math.min(w, h) / 2;

    return {
      x: box.left - padding,
      y: box.top - padding,
      w,
      h,
      r: Math.max(0, Math.min(r, w / 2, h / 2)),
      isPage: false,
      // Outer perimeter: the renderer draws the rim here rather than clipping
      // the fill inside.
      isInner: false,
    };
  };

  /**
   * Size and position the canvas to cover only the rim and its glow.
   *
   * This is what keeps the aura cheap. The stage window is fullscreen, so
   * drawing fullscreen would alpha-blend every pixel every frame — the cost
   * profile behind the 100-150% CPU figure in `62-...md:29-70`. Here the
   * canvas is a scissored box roughly (orb + 2×reach) on a side, and the
   * fragment shader early-outs on distance from the rect, so interior pixels
   * cost one comparison.
   */
  const place = (rect: IAuraRect, fit: IAuraFit): IAuraView | null => {
    // A degenerate target means "the orb is not measurable right now" (mid
    // roam transition, or unmounting). The renderer guards this too, but
    // expanding the margin would turn a 0×0 rect at the origin into a real
    // ~28×28 box and draw a rim where there is no orb — so reject it here.
    if (rect.w < 1 || rect.h < 1) return null;

    const { width: viewportWidth, height: viewportHeight } = readViewport();
    // `globalThis` rather than `window`: this module is unit-tested in the
    // node environment, where `window` is undefined but `devicePixelRatio` is
    // absent either way — so the `|| 1` fallback is the real behaviour there.
    const dpr =
      typeof globalThis.devicePixelRatio === "number" && globalThis.devicePixelRatio > 0
        ? globalThis.devicePixelRatio
        : 1;
    const pixelRatio = Math.min(dpr, MAX_PIXEL_RATIO);
    const scale = pixelRatio * fit.resolution;

    const margin = Math.ceil(fit.reach + PLACE_MARGIN_PX);
    const x0 = Math.max(0, Math.floor(rect.x - margin));
    const y0 = Math.max(0, Math.floor(rect.y - margin));
    const x1 = Math.min(viewportWidth, Math.ceil(rect.x + rect.w + margin));
    const y1 = Math.min(viewportHeight, Math.ceil(rect.y + rect.h + margin));

    // Fully off-screen, or a degenerate box: hide rather than draw garbage.
    if (x1 <= x0 || y1 <= y0) return null;

    const width = x1 - x0;
    const height = y1 - y0;

    const pixelWidth = Math.max(1, Math.round(width * scale));
    const pixelHeight = Math.max(1, Math.round(height * scale));
    if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
      canvas.width = pixelWidth;
      canvas.height = pixelHeight;
    }

    if (width !== appliedWidth || height !== appliedHeight) {
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
      appliedWidth = width;
      appliedHeight = height;
    }

    const transform = `translate(${x0}px, ${y0}px)`;
    if (transform !== appliedTransform) {
      canvas.style.transform = transform;
      appliedTransform = transform;
    }

    canvas.style.display = "block";
    // Blur is done by the renderer here (effects: "shader"), so the host must
    // not also apply a CSS filter — double-blurring is the classic tell.
    canvas.style.filter = "";

    return { x: x0, y: y0, width, height, pixelWidth, pixelHeight, clip: null };
  };

  const hide = (): void => {
    canvas.style.display = "none";
  };

  return {
    // The stage is a transparent window over an unknown desktop, so there is
    // no luminance to probe and no correct answer to adapt to. Always dark:
    // the aura is additive light on top, which reads on any wallpaper.
    background: (_now: number): AuraBackground => "dark",
    target,
    place,
    hide,
    destroy() {
      hide();
      appliedWidth = 0;
      appliedHeight = 0;
      appliedTransform = "";
    },
  };
}

const EMPTY_RECT: IAuraRect = { x: 0, y: 0, w: 0, h: 0, r: 0, isPage: false, isInner: false };