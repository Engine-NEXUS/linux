import { describe, expect, it } from "vitest";
import { HAND_CONNECTIONS, cameraHint, chooseDelegate, isLikelyCovered, median, needsCpuTrial, percentile, shouldProcessFrame, statusLine, toPayload, type HudState } from "./payload";

const pts = (n: number) => Array.from({ length: n }, (_, i) => ({ x: i / 100, y: 0.5, z: -0.01 }));

describe("toPayload", () => {
  it("passes exactly 21 finite landmarks as numbers only", () => {
    const p = toPayload(pts(21), 123, 4.5);
    expect(p.lm).toHaveLength(21);
    expect(p.lm![3]).toEqual([0.03, 0.5, -0.01]);
    expect(p.t_epoch_ms).toBe(123);
  });
  it("sends null for no hand, a short list, or NaN coordinates", () => {
    expect(toPayload(undefined, 1, 1).lm).toBeNull();
    expect(toPayload(pts(20), 1, 1).lm).toBeNull();
    const bad = pts(21);
    bad[7].x = NaN;
    expect(toPayload(bad, 1, 1).lm).toBeNull();
  });
});

describe("shouldProcessFrame", () => {
  it("runs every frame while a hand was seen in the last 2 s", () => {
    for (let i = 0; i < 6; i++) expect(shouldProcessFrame(i, 10_000, 9_000)).toBe(true);
  });
  it("drops to every third frame once the hand has been gone for 2 s", () => {
    const ran = [0, 1, 2, 3, 4, 5].filter((i) => shouldProcessFrame(i, 10_000, 5_000));
    expect(ran).toEqual([0, 3]);
  });
  it("drops to half rate under sustained GPU contention even while a hand is in view", () => {
    const ran = [0, 1, 2, 3, 4, 5].filter((i) => shouldProcessFrame(i, 10_000, 9_000, true));
    expect(ran).toEqual([0, 2, 4]);
  });
  it("contention does not change the already-throttled no-hand case", () => {
    const ran = [0, 1, 2, 3, 4, 5].filter((i) => shouldProcessFrame(i, 10_000, 5_000, true));
    expect(ran).toEqual([0, 3]);
  });
});

describe("isLikelyCovered", () => {
  it("flags dark and flat (textureless) frames — a genuinely covered lens", () => {
    expect(isLikelyCovered(3, 1)).toBe(true);
  });
  it("does not flag a dim room (dark but still has some variance)", () => {
    expect(isLikelyCovered(8, 20)).toBe(false);
  });
  it("does not flag a normally lit, textured frame", () => {
    expect(isLikelyCovered(120, 800)).toBe(false);
  });
});

describe("delegate calibration", () => {
  it("median handles odd, even-ish and empty lists", () => {
    expect(median([5, 1, 3])).toBe(3);
    expect(median([])).toBe(Infinity);
  });
  it("keeps the GPU when it is fast and only times the CPU when the GPU is slow", () => {
    expect(needsCpuTrial([12, 15, 14, 13])).toBe(false);
    expect(needsCpuTrial([55, 60, 48, 52])).toBe(true);
  });
  it("switches to the CPU only if it is clearly faster", () => {
    expect(chooseDelegate([55, 60, 50], [20, 22, 25])).toBe("CPU");
    expect(chooseDelegate([55, 60, 50], [50, 52, 49])).toBe("GPU"); // not 15 % better
    expect(chooseDelegate([55, 60, 50], null)).toBe("GPU");
  });
});

describe("capture time and camera hint", () => {
  it("carries the capture time, defaulting to the processing time", () => {
    expect(toPayload(pts(21), 200, 4, 150).t_capture_epoch_ms).toBe(150);
    expect(toPayload(pts(21), 200, 4).t_capture_epoch_ms).toBe(200);
  });
  it("percentile uses nearest rank", () => {
    expect(percentile([10, 20, 30, 40, 50], 50)).toBe(30);
    expect(percentile([10, 20, 30, 40, 50], 95)).toBe(50);
    expect(percentile([], 50)).toBe(0);
  });
  it("warns only when the camera itself is slow", () => {
    expect(cameraHint(6)).toBe("camera 6 fps · add light");
    expect(cameraHint(24)).toBeNull();
    expect(cameraHint(0)).toBeNull();
  });
});

describe("skeleton", () => {
  it("only connects valid landmark indices", () => {
    for (const [a, b] of HAND_CONNECTIONS) {
      expect(a).toBeGreaterThanOrEqual(0);
      expect(b).toBeLessThanOrEqual(20);
    }
  });
});

const base: HudState = {
  tracking: true, armed: false, pose: "palm", pinch: 1, pressed: false, dragging: false, scrolling: false,
  cursor: [10, 10], arm_stage: 2, live: false, screen: [1920, 1080], primary: true, fps: 29.6, last_action: null, action_seq: 0, note: null, cam_fps: 30,
  gpu_busy: false, covered: false,
};

describe("statusLine", () => {
  it("guides the arming gesture and shows progress", () => {
    expect(statusLine(base)).toContain("2/3");
  });
  it("says PREVIEW when armed but read-only, LIVE when live", () => {
    expect(statusLine({ ...base, armed: true })).toContain("PREVIEW");
    expect(statusLine({ ...base, armed: true, live: true })).toContain("LIVE");
  });
  it("reports a lost hand and a mouse takeover", () => {
    expect(statusLine({ ...base, tracking: false })).toContain("no hand");
    expect(statusLine({ ...base, note: "disarmed (mouse)" })).toContain("disarmed (mouse)");
  });
});
