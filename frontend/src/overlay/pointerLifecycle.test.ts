import { describe, expect, it } from "vitest";
import { createPointerLifecycle, dwellFor, FADE_MS, MIN_DWELL_MS, type PointerView, type Timers } from "./pointerLifecycle";

/** Manual clock: timers fire only when `advance` passes their due time. */
function manualTimers() {
  let now = 0;
  let seq = 0;
  const pending = new Map<number, { due: number; fn: () => void }>();
  const timers: Timers = {
    set: (fn, ms) => {
      const id = ++seq;
      pending.set(id, { due: now + ms, fn });
      return id;
    },
    clear: (h) => {
      pending.delete(h as number);
    },
  };
  const advance = (ms: number) => {
    const until = now + ms;
    for (;;) {
      const next = [...pending.entries()]
        .filter(([, t]) => t.due <= until)
        .sort((a, b) => a[1].due - b[1].due)[0];
      if (!next) break;
      pending.delete(next[0]);
      now = next[1].due;
      next[1].fn();
    }
    now = until;
  };
  return { timers, advance, pendingCount: () => pending.size };
}

const target = { x: 100, y: 200, label: "Save", dwell_ms: 3000 };

describe("dwellFor", () => {
  it("uses the dwell Rust sent", () => {
    expect(dwellFor({ dwell_ms: 3000 })).toBe(3000);
  });
  it("never goes below the minimum", () => {
    expect(dwellFor({ dwell_ms: 10 })).toBe(MIN_DWELL_MS);
  });
  it("falls back to the default for a missing or bad dwell", () => {
    expect(dwellFor({ dwell_ms: 0 })).toBe(6000);
    expect(dwellFor({ dwell_ms: Number.NaN })).toBe(6000);
  });
});

describe("pointer lifecycle", () => {
  it("shows, fades after the dwell, then clears", () => {
    const views: PointerView[] = [];
    const clock = manualTimers();
    const life = createPointerLifecycle((v) => views.push(v), clock.timers);

    life.show(target);
    expect(views.at(-1)).toEqual({ target, fading: false });

    clock.advance(2999);
    expect(views.at(-1)?.fading).toBe(false);

    clock.advance(1);
    expect(views.at(-1)).toEqual({ target, fading: true });

    clock.advance(FADE_MS);
    expect(views.at(-1)).toEqual({ target: null, fading: false });
    expect(clock.pendingCount()).toBe(0);
  });

  it("hide clears at once and cancels the pending fade", () => {
    const views: PointerView[] = [];
    const clock = manualTimers();
    const life = createPointerLifecycle((v) => views.push(v), clock.timers);

    life.show(target);
    life.hide();
    expect(views.at(-1)).toEqual({ target: null, fading: false });
    expect(clock.pendingCount()).toBe(0);

    const seen = views.length;
    clock.advance(10_000);
    expect(views.length).toBe(seen);
  });

  it("a new show restarts the dwell instead of fading the new marker early", () => {
    const views: PointerView[] = [];
    const clock = manualTimers();
    const life = createPointerLifecycle((v) => views.push(v), clock.timers);

    life.show(target);
    clock.advance(2500);
    const second = { ...target, label: "Open", dwell_ms: 3000 };
    life.show(second);
    clock.advance(2500);
    expect(views.at(-1)).toEqual({ target: second, fading: false });
    clock.advance(500);
    expect(views.at(-1)).toEqual({ target: second, fading: true });
  });

  it("dispose leaves no timer behind", () => {
    const clock = manualTimers();
    const life = createPointerLifecycle(() => {}, clock.timers);
    life.show(target);
    life.dispose();
    expect(clock.pendingCount()).toBe(0);
  });
});
