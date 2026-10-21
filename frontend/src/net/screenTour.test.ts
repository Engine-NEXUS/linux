import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const ttsMock = vi.hoisted(() => ({
  playing: false,
  narration: [] as boolean[],
}));

vi.mock("../audio/ttsPlayer", () => ({
  isRustTtsPlaying: () => ttsMock.playing,
  setNarrationPlaying: (v: boolean) => {
    ttsMock.narration.push(v);
  },
}));

import { useAssistant } from "../store/assistant";
import { isTourActive } from "../stage/tourState";
import {
  FETCH_HIDE_CAP_MS,
  __testResetTourFlow,
  handleTourEnd,
  handleTourPhase,
  handleTourStart,
} from "./screenTour";

function orb() {
  return useAssistant.getState();
}

describe("screen tour orb flow", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    __testResetTourFlow();
    ttsMock.playing = false;
    ttsMock.narration.length = 0;
    const s = orb();
    s.reset();
    s.setGhostActive(false);
    s.setVisible(true);
    s.setState("speaking"); // the shared Ack handler leaves the orb here
  });

  afterEach(() => {
    vi.useRealTimers();
    handleTourEnd({ request_id: "x", reason: "done" });
  });

  it("hides the orb after the ack finishes while the tour fetches", () => {
    ttsMock.playing = true; // ack audio
    handleTourPhase({ phase: "fetching", request_id: "r1" });
    vi.advanceTimersByTime(600);
    expect(orb().visible).toBe(true); // still speaking the ack
    ttsMock.playing = false;
    vi.advanceTimersByTime(400);
    expect(orb().visible).toBe(false);
    expect(orb().state).toBe("idle");
  });

  it("does not wait forever for a stuck ack", () => {
    ttsMock.playing = true;
    handleTourPhase({ phase: "fetching", request_id: "r1" });
    vi.advanceTimersByTime(FETCH_HIDE_CAP_MS + 600);
    expect(orb().visible).toBe(false);
  });

  it("never hides the ghost-session orb", () => {
    orb().setGhostActive(true);
    handleTourPhase({ phase: "fetching", request_id: "r1" });
    vi.advanceTimersByTime(1000);
    expect(orb().visible).toBe(true);
  });

  it("a fallback cancels the pending hide (legacy speech owns the orb)", () => {
    handleTourPhase({ phase: "fetching", request_id: "r1" });
    handleTourPhase({ phase: "fallback", request_id: "r1" });
    vi.advanceTimersByTime(1000);
    expect(orb().visible).toBe(true);
    expect(orb().state).toBe("speaking");
  });

  it("tour start brings the orb back speaking and flags narration", () => {
    handleTourPhase({ phase: "fetching", request_id: "r1" });
    vi.advanceTimersByTime(500);
    expect(orb().visible).toBe(false);
    handleTourStart({ request_id: "r1" });
    expect(orb().visible).toBe(true);
    expect(orb().state).toBe("speaking");
    expect(orb().ttsActive).toBe(true);
    expect(isTourActive()).toBe(true);
    expect(ttsMock.narration).toEqual([true]);
    // a stale hide timer from the fetch phase must not fire anymore
    vi.advanceTimersByTime(5000);
    expect(orb().visible).toBe(true);
  });

  it("done end leaves orb reset to the shared done handler", () => {
    handleTourStart({ request_id: "r1" });
    handleTourEnd({ request_id: "r1", reason: "done" });
    expect(isTourActive()).toBe(false);
    expect(orb().ttsActive).toBe(false);
    vi.advanceTimersByTime(3000);
    // `done` (from Rust) resets; this module must not hide it on its own.
    expect(orb().visible).toBe(true);
    expect(ttsMock.narration).toEqual([true, false]);
  });

  it("a cancelled tour closes the orb unless a newer turn took over", () => {
    handleTourStart({ request_id: "r1" });
    handleTourEnd({ request_id: "r1", reason: "cancelled" });
    vi.advanceTimersByTime(1500);
    expect(orb().visible).toBe(false);
    expect(orb().state).toBe("idle");

    // newer turn already listening → untouched
    __testResetTourFlow();
    orb().setVisible(true);
    handleTourStart({ request_id: "r2" });
    handleTourEnd({ request_id: "r2", reason: "cancelled" });
    orb().setState("listening");
    vi.advanceTimersByTime(1500);
    expect(orb().visible).toBe(true);
    expect(orb().state).toBe("listening");
  });

  it("ignores an end from a stale tour", () => {
    handleTourStart({ request_id: "r2" });
    handleTourEnd({ request_id: "r1", reason: "done" });
    expect(isTourActive()).toBe(true);
  });
});
