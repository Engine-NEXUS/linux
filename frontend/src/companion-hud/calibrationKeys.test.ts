import { describe, expect, it } from "vitest";

import {
  ARROW_STEP,
  ARROW_STEP_SHIFT,
  resolveCalibrationKey,
} from "./calibrationKeys";

describe("resolveCalibrationKey (HUD keyboard control)", () => {
  it("selects targets on 1/2/3 (main row + numpad + key fallback)", () => {
    expect(resolveCalibrationKey("Digit1", "1", false)).toEqual({
      kind: "select",
      target: "wakeup",
    });
    expect(resolveCalibrationKey("Numpad2", "2", false)).toEqual({
      kind: "select",
      target: "waves",
    });
    expect(resolveCalibrationKey("Digit3", "3", false)).toEqual({
      kind: "select",
      target: "loading",
    });
    expect(resolveCalibrationKey("", "1", false)).toEqual({
      kind: "select",
      target: "wakeup",
    });
  });

  it("nudges 1px per arrow, 10px with Shift", () => {
    expect(resolveCalibrationKey("", "ArrowLeft", false)).toEqual({
      kind: "nudge",
      dx: -ARROW_STEP,
      dy: 0,
    });
    expect(resolveCalibrationKey("", "ArrowRight", true)).toEqual({
      kind: "nudge",
      dx: ARROW_STEP_SHIFT,
      dy: 0,
    });
    expect(resolveCalibrationKey("", "ArrowUp", false)).toEqual({
      kind: "nudge",
      dx: 0,
      dy: -ARROW_STEP,
    });
    expect(resolveCalibrationKey("", "ArrowDown", true)).toEqual({
      kind: "nudge",
      dx: 0,
      dy: ARROW_STEP_SHIFT,
    });
  });

  it("maps Enter/Escape to save/cancel, ignores everything else", () => {
    expect(resolveCalibrationKey("", "Enter", false)).toEqual({ kind: "save" });
    expect(resolveCalibrationKey("", "Escape", false)).toEqual({
      kind: "cancel",
    });
    expect(resolveCalibrationKey("", "a", false)).toEqual({ kind: "none" });
    expect(resolveCalibrationKey("", "F5", false)).toEqual({ kind: "none" });
  });
});
