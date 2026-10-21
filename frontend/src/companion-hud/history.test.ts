import { describe, expect, it } from "vitest";

import {
  cloneSnapshot,
  commitChange,
  commitCoalesced,
  popUndo,
  type CalibrationSnapshot,
} from "./history";

const snap = (h: number, size: number): CalibrationSnapshot => ({
  wakeup: { h, v: 1, size },
  waves: { h: 0.5, v: 1, size: 200 },
  loading: { h: 0.95, v: 0.05, size: 80 },
});

describe("calibration history (HUD ↶ Undo)", () => {
  it("commitChange pushes the pre-change snapshot and clones it", () => {
    const initial = snap(0.5, 200);
    const next = snap(0.8, 260);
    const st = commitChange({ past: [], present: initial }, initial, next);
    expect(st.past).toHaveLength(1);
    expect(st.present.wakeup.h).toBe(0.8);
    // pre-change snapshot is detached from both present and caller
    st.past[0].wakeup.size = 999;
    expect(initial.wakeup.size).toBe(200);
    expect(st.present.wakeup.size).toBe(260);
  });

  it("popUndo restores the last committed pre-change state", () => {
    const s0 = snap(0.5, 200);
    let st = commitChange({ past: [], present: s0 }, s0, snap(0.8, 260));
    st = commitChange(st, st.present, snap(0.2, 140));
    const r = popUndo(st);
    expect(r.restored?.wakeup.h).toBe(0.8);
    expect(r.next.past).toHaveLength(1);
    expect(r.next.present.wakeup.h).toBe(0.8);
  });

  it("popUndo on empty history is a no-op", () => {
    const st: { past: CalibrationSnapshot[]; present: CalibrationSnapshot } = {
      past: [],
      present: snap(0.5, 200),
    };
    const r = popUndo(st);
    expect(r.restored).toBeNull();
    expect(r.next.past).toHaveLength(0);
  });

  it("history is capped (runaway mics can't grow it unbounded)", () => {
    let st: { past: CalibrationSnapshot[]; present: CalibrationSnapshot } = {
      past: [],
      present: snap(0.5, 200),
    };
    for (let i = 0; i < 60; i++) {
      const pre = st.present;
      st = commitChange(st, pre, snap((i % 10) / 10, 200 + i));
    }
    expect(st.past.length).toBeLessThanOrEqual(50);
  });

  it("cloneSnapshot breaks references", () => {
    const s = snap(0.5, 200);
    const c = cloneSnapshot(s);
    c.wakeup.h = 0.9;
    expect(s.wakeup.h).toBe(0.5);
  });

  it("commitCoalesced updates present without growing past (nudge bursts)", () => {
    const s0 = snap(0.5, 200);
    let st = commitChange({ past: [], present: s0 }, s0, snap(0.51, 200));
    expect(st.past).toHaveLength(1);
    // 30-event burst → still one undo entry, present tracks the tip.
    for (let i = 0; i < 30; i++) {
      st = commitCoalesced(st, snap(0.51 + i * 0.001, 200));
    }
    expect(st.past).toHaveLength(1);
    expect(st.present.wakeup.h).toBeCloseTo(0.51 + 29 * 0.001);
    // One Undo returns to the pre-burst snapshot.
    const r = popUndo(st);
    expect(r.restored?.wakeup.h).toBe(0.5);
  });
});
