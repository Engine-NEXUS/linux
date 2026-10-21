// Pure helpers for the hand window (no DOM, no camera): easy to test.

export interface Landmark {
  x: number;
  y: number;
  z: number;
}

/** What crosses IPC per frame: 21 numeric points (or null), never an image. */
export interface FramePayload {
  lm: [number, number, number][] | null;
  t_epoch_ms: number;
  /** When the camera captured the frame (epoch ms); Rust measures the real pipeline delay from it. */
  t_capture_epoch_ms: number;
  infer_ms: number;
}

export function toPayload(
  hand: Landmark[] | undefined,
  tEpochMs: number,
  inferMs: number,
  captureEpochMs: number = tEpochMs,
): FramePayload {
  const ok = hand && hand.length === 21 && hand.every((p) => Number.isFinite(p.x) && Number.isFinite(p.y));
  return {
    lm: ok ? hand.map((p) => [p.x, p.y, Number.isFinite(p.z) ? p.z : 0]) : null,
    t_epoch_ms: tEpochMs,
    t_capture_epoch_ms: captureEpochMs,
    infer_ms: inferMs,
  };
}

/** Nearest-rank percentile of a list (0-100). */
export function percentile(xs: number[], p: number): number {
  if (xs.length === 0) return 0;
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.max(0, Math.ceil((p / 100) * s.length) - 1))];
}

/** Shown on the camera preview when the camera itself is slow (dim rooms make webcams cut their rate). */
export function cameraHint(fps: number): string | null {
  if (!(fps > 0) || fps >= 15) return null;
  return `camera ${Math.round(fps)} fps · add light`;
}

/**
 * Full rate while a hand is around; a third of the frames once it has been gone for 2 s (saves
 * CPU/battery). Under sustained GPU contention (another app competing for the same shared
 * WebView2 GPU process — Rust detects this from real infer-time data and reports it back on
 * `hand:state`) a hand still in view is throttled to half rate too: fewer, steadier updates read
 * as smoother than many wildly-delayed ones once the pipeline is genuinely behind.
 */
export function shouldProcessFrame(frameIdx: number, nowMs: number, lastHandMs: number, gpuBusy = false): boolean {
  if (nowMs - lastHandMs < 2000) return gpuBusy ? frameIdx % 2 === 0 : true;
  return frameIdx % 3 === 0;
}

/**
 * A covered lens reads as both dark AND flat (no texture) at once — there's genuinely nothing to
 * see. A merely dim room is dark but still has some variance from whatever light exists. Checking
 * both avoids false-flagging a dark room as a covered camera. Thresholds are a first pass, not
 * measured against a real physically-covered laptop camera yet — expect one tuning pass after
 * this ships.
 */
export function isLikelyCovered(avgBrightness: number, variance: number): boolean {
  return avgBrightness < 12 && variance < 4;
}

export function median(xs: number[]): number {
  if (xs.length === 0) return Infinity;
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
}

/** GPU is the default; only if it is slow on THIS machine is the CPU path worth timing as well. */
export const GPU_GOOD_ENOUGH_MS = 30;

export function needsCpuTrial(gpuMs: number[]): boolean {
  return median(gpuMs) > GPU_GOOD_ENOUGH_MS;
}

/** Pick the faster delegate from measured per-frame inference times. The CPU must win by 15 % to switch. */
export function chooseDelegate(gpuMs: number[], cpuMs: number[] | null): "GPU" | "CPU" {
  if (!cpuMs || cpuMs.length === 0) return "GPU";
  return median(cpuMs) < median(gpuMs) * 0.85 ? "CPU" : "GPU";
}

/** MediaPipe hand skeleton (pairs of landmark indices). */
export const HAND_CONNECTIONS: [number, number][] = [
  [0, 1], [1, 2], [2, 3], [3, 4],
  [0, 5], [5, 6], [6, 7], [7, 8],
  [5, 9], [9, 10], [10, 11], [11, 12],
  [9, 13], [13, 14], [14, 15], [15, 16],
  [13, 17], [17, 18], [18, 19], [19, 20],
  [0, 17],
];

/** HUD text for a state event from Rust. */
export interface HudState {
  tracking: boolean;
  armed: boolean;
  pose: string;
  pinch: number;
  pressed: boolean;
  dragging: boolean;
  scrolling: boolean;
  cursor: [number, number];
  arm_stage: number;
  live: boolean;
  /** Size of the monitor the cursor is on; `cursor` is inside it. */
  screen: [number, number];
  /** True when that monitor is the primary one (the stage overlay is drawn there). */
  primary: boolean;
  fps: number;
  last_action: string | null;
  action_seq: number;
  note: string | null;
  /** Camera frames per second as measured by the hand window (0 = unknown). */
  cam_fps: number;
  /** Engaged gesture pose: "" | "tabs" (3 fingers) | "windows" (switcher open) | "desktops" (4 fingers). */
  gesture?: string;
  /** Sustained GPU contention suspected (infer time well above the calibrated baseline). Informational — tracking keeps running either way. */
  gpu_busy: boolean;
  /** The hand window's own brightness+variance judgement that the camera looks covered. */
  covered: boolean;
}

export function statusLine(s: HudState): string {
  if (!s.tracking) return s.armed ? "armed — show your hand" : "no hand — show your palm";
  if (!s.armed) {
    return s.note && s.note.startsWith("disarmed")
      ? s.note
      : `palm → fist → palm → fist to arm (${s.arm_stage}/3)`;
  }
  const mode = s.live ? "LIVE" : "PREVIEW (not clicking)";
  const what = s.dragging ? "dragging" : s.scrolling ? "scrolling" : s.pressed ? "pinched" : s.pose;
  return `${mode} · ${what} · ${Math.round(s.fps)} fps`;
}
