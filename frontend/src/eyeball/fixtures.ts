/**
 * Fixtures for the dev-only stage preview (`stage.html?eyeball`).
 *
 * Pure: the URL → fixture parse and the orb rect maths, so the harness can be
 * tested without a DOM. The rect mirrors Rust's `window_manager::rect_for`
 * (centred, `overlay_xy` clamp, a safe-area top inset for a desktop panel).
 */

import type { AssistantState, OrbCapsuleStyle } from "../store/assistant";

export const EYEBALL_STATES: readonly AssistantState[] = ["idle", "listening", "thinking", "speaking"];
export const EYEBALL_STYLES: readonly OrbCapsuleStyle[] = ["aura", "classic", "pebble"];

export interface EyeballFixture {
  state: AssistantState;
  style: OrbCapsuleStyle;
  /** Fake energy 0..1: the mic while listening, the TTS envelope while speaking. */
  level: number;
  position: "top" | "bottom";
  /** Show the "point at X" marker. */
  pointer: boolean;
  /** Top inset (CSS px) standing in for a desktop panel. */
  panel: number;
}

export const DEFAULT_FIXTURE: EyeballFixture = {
  state: "listening",
  style: "aura",
  level: 0.5,
  position: "top",
  pointer: false,
  panel: 32,
};

const clamp01 = (v: number) => (Number.isFinite(v) ? Math.max(0, Math.min(1, v)) : 0);

/** Pure: read a fixture from a query string; anything unknown keeps the default. */
export function fixtureFromQuery(search: string): EyeballFixture {
  const q = new URLSearchParams(search);
  const f: EyeballFixture = { ...DEFAULT_FIXTURE };
  const state = q.get("state");
  if (state && (EYEBALL_STATES as readonly string[]).includes(state)) f.state = state as AssistantState;
  const style = q.get("style");
  if (style && (EYEBALL_STYLES as readonly string[]).includes(style)) f.style = style as OrbCapsuleStyle;
  const level = q.get("level");
  if (level !== null && level.trim() !== "" && Number.isFinite(Number(level))) f.level = clamp01(Number(level));
  const pos = q.get("pos");
  if (pos === "top" || pos === "bottom") f.position = pos;
  const pointer = q.get("pointer");
  if (pointer !== null) f.pointer = pointer === "1" || pointer === "true";
  const panel = q.get("panel");
  if (panel !== null && panel.trim() !== "" && Number.isFinite(Number(panel))) {
    f.panel = Math.max(0, Math.round(Number(panel)));
  }
  return f;
}

/** Pure: the query string for a fixture (inverse of `fixtureFromQuery`). */
export function fixtureToQuery(f: EyeballFixture): string {
  const q = new URLSearchParams({
    eyeball: "",
    state: f.state,
    style: f.style,
    level: String(f.level),
    pos: f.position,
    pointer: f.pointer ? "1" : "0",
    panel: String(f.panel),
  });
  return `?${q.toString()}`;
}

/**
 * Pure: the orb rect in PHYSICAL px, as Rust would emit it on a viewport of
 * `w`×`h` CSS px at `dpr`, for an orb `size` CSS px wide, centred and placed
 * inside the area below the panel (top) or at the bottom edge.
 */
export function orbRectFor(
  w: number,
  h: number,
  dpr: number,
  position: "top" | "bottom",
  panel: number,
  size = 200,
): { x: number; y: number; w: number; h: number } {
  const scale = Number.isFinite(dpr) && dpr > 0 ? dpr : 1;
  const phys = Math.round(size * scale);
  const areaW = Math.max(0, Math.round(w * scale));
  const top = Math.round(Math.max(0, panel) * scale);
  const areaH = Math.max(0, Math.round(h * scale) - top);
  const x = Math.max(0, Math.min(Math.round(areaW / 2 - phys / 2), areaW - phys));
  const yInArea = position === "top" ? 0 : Math.max(0, areaH - phys);
  return { x, y: yInArea + top, w: phys, h: phys };
}
