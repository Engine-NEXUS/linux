import { describe, it, expect } from "vitest";
import {
  stepWatchdog,
  INITIAL_WATCHDOG_STATE,
  IDLE_HIDE_MS,
  THINKING_STUCK_MS,
  LISTENING_STUCK_MS,
  type OrbSnapshot,
  type WatchdogState,
} from "./orbIdleWatchdog";

const base: OrbSnapshot = {
  visible: true,
  state: "idle",
  ghostActive: false,
  captionActive: false,
  ttsActive: false,
  awaitingInput: false,
  pendingGithubCommand: false,
  loadingVisible: false,
  calibrating: false,
};

/** Run ticks every 500 ms until `untilMs`, returning the first non-"none" action. */
function run(snap: OrbSnapshot, untilMs: number, start: WatchdogState = INITIAL_WATCHDOG_STATE) {
  let st = start;
  for (let t = 0; t <= untilMs; t += 500) {
    const r = stepWatchdog(st, snap, t);
    st = r.next;
    if (r.action !== "none") return { action: r.action, at: t };
  }
  return { action: "none" as const, at: untilMs };
}

describe("orb idle watchdog", () => {
  it("hides an orb that sits idle and visible (boot briefing / ack-less result)", () => {
    const r = run(base, 10_000);
    expect(r.action).toBe("hide");
    expect(r.at).toBeGreaterThanOrEqual(IDLE_HIDE_MS);
    expect(r.at).toBeLessThanOrEqual(IDLE_HIDE_MS + 500);
  });

  it("does nothing when the orb is already hidden", () => {
    expect(run({ ...base, visible: false }, 20_000).action).toBe("none");
  });

  it.each([
    ["ghost session", { ghostActive: true }],
    ["caption on screen", { captionActive: true }],
    ["tts playing", { ttsActive: true }],
    ["awaiting input", { awaitingInput: true }],
    ["pending github confirm", { pendingGithubCommand: true }],
    ["loading indicator", { loadingVisible: true }],
    ["calibration", { calibrating: true }],
  ])("never hides while %s", (_name, patch) => {
    expect(run({ ...base, ...(patch as Partial<OrbSnapshot>) }, 30_000).action).toBe("none");
  });

  it("restarts the idle clock when something pending clears", () => {
    let st = INITIAL_WATCHDOG_STATE;
    // 2 s of caption (not eligible), then idle: the 2.5 s must count from the moment the caption ends.
    for (let t = 0; t < 2000; t += 500) st = stepWatchdog(st, { ...base, captionActive: true }, t).next;
    const r = stepWatchdog(st, base, 2000);
    expect(r.action).toBe("none");
    const later = stepWatchdog(r.next, base, 2000 + IDLE_HIDE_MS);
    expect(later.action).toBe("hide");
  });

  it("recovers a turn stuck in thinking (lost transcript / silent drop)", () => {
    const r = run({ ...base, state: "thinking" }, THINKING_STUCK_MS + 2000);
    expect(r.action).toBe("reset_and_hide");
    expect(r.at).toBeGreaterThanOrEqual(THINKING_STUCK_MS);
  });

  it("recovers a turn stuck in listening outside ghost mode", () => {
    const r = run({ ...base, state: "listening" }, LISTENING_STUCK_MS + 2000);
    expect(r.action).toBe("reset_and_hide");
  });

  it("leaves a long-running turn alone while the loading indicator is up", () => {
    expect(run({ ...base, state: "thinking", loadingVisible: true }, 300_000).action).toBe("none");
  });

  it("does not time out hot-mic listening in a ghost session", () => {
    expect(run({ ...base, state: "listening", ghostActive: true }, 300_000).action).toBe("none");
  });

  it("restarts the busy clock when the state changes", () => {
    let st = INITIAL_WATCHDOG_STATE;
    for (let t = 0; t < 25_000; t += 500) st = stepWatchdog(st, { ...base, state: "listening" }, t).next;
    // Switching to thinking restarts the clock, so 25 s in listening must not count toward the 60 s.
    const r = stepWatchdog(st, { ...base, state: "thinking" }, 25_000);
    expect(r.action).toBe("none");
    expect(r.next.busySince).toBe(25_000);
  });

  it("never touches a speaking orb (the speaking failsafe owns that)", () => {
    expect(run({ ...base, state: "speaking" }, 300_000).action).toBe("none");
  });
});
