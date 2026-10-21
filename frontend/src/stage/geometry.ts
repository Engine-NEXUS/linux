/**
 * Stage geometry — pixel-identical mapping of the legacy windows into the
 * single fullscreen stage. Same formulas the Rust side uses today:
 * - Orb: user pct/size from settings (window_manager.rs: defaults
 *   h=0.5 center, v=1.0 bottom, 200px; clamped on-screen).
 * - Sidebars: bottom-right docked, 12px gap, 48px taskbar reserve.
 * - Loading: 80x80 top-right (7px x / 9px y physical insets).
 * All rects are CSS px here; the hitbox reporter multiplies by
 * devicePixelRatio before sending physical px to Rust.
 */

export interface StageRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export const STAGE_GAP_PX = 12;
export const STAGE_TASKBAR_PX = 48;
export const LOADING_SIZE_PX = 80;
export const LOADING_INSET_X_PX = 7;
export const LOADING_INSET_Y_PX = 9;

export const PANEL_SIZES: Record<string, { w: number; h: number }> = {
  sidebar: { w: 400, h: 1000 },
  "pr-list": { w: 500, h: 1000 },
  architect: { w: 900, h: 1000 },
};

/** Bottom-right docked panel rect (all sidebars share this math). */
export function dockRect(
  panelW: number,
  panelH: number,
  screenW: number,
  screenH: number,
): StageRect {
  const w = Math.min(panelW, screenW);
  const h = Math.min(panelH, screenH);
  return {
    x: Math.max(0, screenW - w - STAGE_GAP_PX),
    y: Math.max(0, screenH - h - STAGE_TASKBAR_PX - STAGE_GAP_PX),
    w,
    h,
  };
}

/** Loading indicator rect (top-right corner). */
export function loadingRect(): StageRect {
  return {
    x: -1, // anchored right/top via CSS; hitbox computed by caller if needed
    y: -1,
    w: LOADING_SIZE_PX,
    h: LOADING_SIZE_PX,
  };
}

/** Orb rect from user settings pct/size (mirrors position_orb clamping: 100–400). */
export function orbRect(
  hPct: number,
  vPct: number,
  sizePx: number,
  screenW: number,
  screenH: number,
): StageRect {
  const h = Math.max(0, Math.min(1, hPct));
  const v = Math.max(0, Math.min(1, vPct));
  const size = Math.max(100, Math.min(400, Math.round(sizePx)));
  const rawX = screenW * h - size / 2;
  const rawY = screenH * v - size / 2;
  return {
    x: Math.max(0, Math.min(screenW - size, Math.round(rawX))),
    y: Math.max(0, Math.min(screenH - size, Math.round(rawY))),
    w: size,
    h: size,
  };
}
