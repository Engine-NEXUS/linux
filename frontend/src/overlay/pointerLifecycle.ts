/**
 * Screen pointer lifecycle: show, dwell, fade, clear.
 *
 * Rust decides WHETHER the marker may show (pointer.rs `decide`); this
 * module owns WHEN it goes away: after its dwell it fades for `FADE_MS`,
 * then clears. A new `show` restarts the cycle; `hide` (a new turn, Esc,
 * `pointer:hide`) clears at once. Timers are injectable so the timing is
 * testable without a DOM.
 */

import type { PointerShowPayload } from "../net/pointer";

export const FADE_MS = 450;
export const MIN_DWELL_MS = 1000;
export const DEFAULT_DWELL_MS = 6000;

export interface PointerView {
  target: PointerShowPayload | null;
  fading: boolean;
}

export interface Timers {
  set: (fn: () => void, ms: number) => unknown;
  clear: (handle: unknown) => void;
}

const realTimers: Timers = {
  set: (fn, ms) => setTimeout(fn, ms),
  clear: (h) => clearTimeout(h as ReturnType<typeof setTimeout>),
};

/** Pure: how long the marker stays before it starts fading. */
export function dwellFor(p: Pick<PointerShowPayload, "dwell_ms">): number {
  const ms = Number.isFinite(p.dwell_ms) && p.dwell_ms > 0 ? p.dwell_ms : DEFAULT_DWELL_MS;
  return Math.max(MIN_DWELL_MS, ms);
}

export function createPointerLifecycle(
  onChange: (view: PointerView) => void,
  timers: Timers = realTimers,
) {
  let dwell: unknown = null;
  let fade: unknown = null;

  const clearTimers = () => {
    if (dwell !== null) timers.clear(dwell);
    if (fade !== null) timers.clear(fade);
    dwell = null;
    fade = null;
  };

  return {
    show(p: PointerShowPayload) {
      clearTimers();
      onChange({ target: p, fading: false });
      dwell = timers.set(() => {
        dwell = null;
        onChange({ target: p, fading: true });
        fade = timers.set(() => {
          fade = null;
          onChange({ target: null, fading: false });
        }, FADE_MS);
      }, dwellFor(p));
    },
    hide() {
      clearTimers();
      onChange({ target: null, fading: false });
    },
    dispose() {
      clearTimers();
    },
  };
}
