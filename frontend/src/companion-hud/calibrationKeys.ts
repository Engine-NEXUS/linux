/**
 * Calibration HUD keyboard map — pure, unit-tested.
 * Window-scoped keydown only (the HUD window is focused during
 * calibration): 1/2/3 select the target, arrows nudge it pixel-perfect,
 * Shift multiplies the step ×10, Enter saves, Escape cancels.
 */

export type CalibrationKeyTarget = "wakeup" | "waves" | "loading";

export type CalibrationKeyAction =
  | { kind: "select"; target: CalibrationKeyTarget }
  | { kind: "nudge"; dx: number; dy: number }
  | { kind: "save" }
  | { kind: "cancel" }
  | { kind: "none" };

/** Arrow step in logical px (Shift multiplies ×10, matching the wheel). */
export const ARROW_STEP = 1;
export const ARROW_STEP_SHIFT = 10;

export function resolveCalibrationKey(
  code: string,
  key: string,
  shiftKey: boolean
): CalibrationKeyAction {
  // Target selection (main row + numpad).
  if (code === "Digit1" || code === "Numpad1" || (!code && key === "1")) {
    return { kind: "select", target: "wakeup" };
  }
  if (code === "Digit2" || code === "Numpad2" || (!code && key === "2")) {
    return { kind: "select", target: "waves" };
  }
  if (code === "Digit3" || code === "Numpad3" || (!code && key === "3")) {
    return { kind: "select", target: "loading" };
  }
  // Pixel nudging (preventDefault at the call site — arrows scroll).
  const step = shiftKey ? ARROW_STEP_SHIFT : ARROW_STEP;
  switch (key) {
    case "ArrowLeft":
      return { kind: "nudge", dx: -step, dy: 0 };
    case "ArrowRight":
      return { kind: "nudge", dx: step, dy: 0 };
    case "ArrowUp":
      return { kind: "nudge", dx: 0, dy: -step };
    case "ArrowDown":
      return { kind: "nudge", dx: 0, dy: step };
    case "Enter":
      return { kind: "save" };
    case "Escape":
      return { kind: "cancel" };
    default:
      return { kind: "none" };
  }
}
