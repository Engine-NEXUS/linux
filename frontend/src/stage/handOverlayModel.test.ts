import { describe, expect, it } from "vitest";
import type { HudState } from "../hand/payload";
import { chipPlacement, chipText, gestureLabel, overlayModel, taskbarBottomPx } from "./handOverlayModel";

const base: HudState = {
  tracking: true, armed: false, pose: "palm", pinch: 1, pressed: false, dragging: false, scrolling: false,
  cursor: [960, 540], arm_stage: 1, live: true, screen: [1920, 1080], primary: true, fps: 20, last_action: null, action_seq: 0, note: null, cam_fps: 30,
  gpu_busy: false, covered: false,
};
const vp = { w: 1280, h: 720 };

describe("chipText (short, one line)", () => {
  it("guides the arming gesture with progress", () => {
    expect(chipText(base)).toBe("HAND · palm, fist, palm, fist  1/3");
    expect(chipText({ ...base, arm_stage: 0 })).toBe("HAND · palm, fist, palm, fist");
  });
  it("asks for a hand when none is seen", () => {
    expect(chipText({ ...base, tracking: false })).toBe("HAND · show your hand");
  });
  it("says ON when armed and live, PREVIEW when read-only", () => {
    expect(chipText({ ...base, armed: true })).toBe("HAND · ON");
    expect(chipText({ ...base, armed: true, live: false })).toBe("HAND · PREVIEW");
    expect(chipText({ ...base, armed: true, tracking: false })).toBe("HAND · ON · show your hand");
  });
  it("shows the chip at once while the camera starts", () => {
    expect(chipText({ ...base, tracking: false, arm_stage: 0, note: "starting camera…" })).toBe("HAND · starting…");
  });
  it("explains a disarm and how to start again", () => {
    expect(chipText({ ...base, note: "disarmed (fist-hold)" })).toContain("off");
  });
  it("adds a dim-room note only when the camera is slow", () => {
    expect(chipText({ ...base, armed: true, cam_fps: 6 })).toContain("dim room");
    expect(chipText({ ...base, armed: true, cam_fps: 15 })).not.toContain("dim room");
    expect(chipText({ ...base, armed: true, cam_fps: 0 })).not.toContain("dim room");
  });
});

describe("overlayModel", () => {
  it("shows no ring while disarmed or live (the real cursor is the pointer)", () => {
    expect(overlayModel(base, vp, false).ring).toBeNull();
    expect(overlayModel({ ...base, armed: true }, vp, false).ring).toBeNull();
  });
  it("in preview draws the predicted cursor scaled from physical px to the viewport", () => {
    const v = overlayModel({ ...base, armed: true, live: false }, vp, false);
    expect(v.ring).toEqual({ x: 640, y: 360 });
    expect(v.chipClass).toContain("preview");
  });
  it("draws no ring when the cursor is on another monitor than the overlay's", () => {
    expect(overlayModel({ ...base, armed: true, live: false, primary: false }, vp, false).ring).toBeNull();
  });
  it("marks the ring pressed or dragging", () => {
    expect(overlayModel({ ...base, armed: true, live: false, pressed: true }, vp, false).ringClass).toContain("down");
    expect(overlayModel({ ...base, armed: true, live: false, pressed: true, dragging: true }, vp, false).ringClass).toContain("drag");
  });
  it("ripples on real actions only, not on arm/disarm", () => {
    expect(overlayModel({ ...base, armed: true, last_action: "click" }, vp, true).ripple).toBe(true);
    expect(overlayModel({ ...base, armed: true, last_action: "armed" }, vp, true).ripple).toBe(false);
    expect(overlayModel({ ...base, armed: true, last_action: "click" }, vp, false).ripple).toBe(false);
  });
});

describe("gestures on the chip", () => {
  const armed = { ...base, armed: true };
  it("says what the held pose does", () => {
    expect(chipText({ ...armed, gesture: "tabs" })).toBe("HAND · 3 fingers: flick for tabs");
    expect(chipText({ ...armed, gesture: "windows" })).toContain("windows");
    expect(chipText({ ...armed, gesture: "desktops" })).toContain("desktops");
    expect(chipText({ ...armed, gesture: "" })).toBe("HAND · ON");
  });
  it("labels gesture commands and gives them no click ripple", () => {
    expect(gestureLabel("next_tab")).toBe("next tab");
    expect(gestureLabel("click")).toBeNull();
    expect(overlayModel({ ...armed, last_action: "next_tab" }, vp, true).ripple).toBe(false);
    expect(overlayModel({ ...armed, last_action: "click" }, vp, true).ripple).toBe(true);
  });
  it("tells how to leave after a fist-hold disarm", () => {
    expect(chipText({ ...base, note: "disarmed (fist-hold)" })).toContain("exit");
  });
});

describe("chipPlacement (opposite the orb, centred)", () => {
  const vp = { w: 1536, h: 864 };
  it("orb at the top: chip is horizontally centred at the bottom, above the taskbar", () => {
    const p = chipPlacement("top", vp, 200, 28, 48);
    expect(p.left).toBe((1536 - 200) / 2);
    expect(p.top).toBe(864 - 28 - 48 - 12);
  });
  it("orb at the bottom: chip is centred at the top", () => {
    expect(chipPlacement("bottom", vp, 200, 28, 48)).toEqual({ left: 668, top: 12 });
  });
  it("without a taskbar reading it still keeps a gap from the bottom edge", () => {
    expect(chipPlacement("top", vp, 200, 28).top).toBe(864 - 28 - 12);
  });
  it("never leaves the screen", () => {
    const p = chipPlacement("top", { w: 100, h: 20 }, 300, 28, 48);
    expect(p.left).toBe(0);
    expect(p.top).toBe(0);
  });
});

describe("taskbarBottomPx", () => {
  it("reads the bottom taskbar from the work area", () => {
    expect(taskbarBottomPx({ height: 864, availHeight: 816, availTop: 0 })).toBe(48);
  });
  it("is 0 when the work area is the whole screen or the taskbar is on top/side", () => {
    expect(taskbarBottomPx({ height: 864, availHeight: 864 })).toBe(0);
    expect(taskbarBottomPx({ height: 864, availHeight: 816, availTop: 48 })).toBe(0);
  });
});
