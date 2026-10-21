import { describe, expect, it } from "vitest";
import { auraLookFor, auraStateFor } from "./orbPresentation";
import type { AssistantState } from "../store/assistant";

const STATES: AssistantState[] = ["idle", "listening", "thinking", "speaking"];

describe("auraStateFor", () => {
  it("keeps a resting rim at idle rather than switching off", () => {
    expect(auraStateFor("idle")).toBe("paused");
  });

  it("makes listening and thinking differ by mechanism, not colour", () => {
    expect(auraStateFor("listening")).toBe("active");
    expect(auraStateFor("thinking")).toBe("working");
  });

  it("makes speaking audio-reactive too", () => {
    expect(auraStateFor("speaking")).toBe("active");
  });

  it("never maps a state to hidden (a remounted renderer would stay dark)", () => {
    // AuraCanvas's mount effect applies the current state to a fresh renderer,
    // which starts hidden; a state that mapped to "hidden" would leave the orb
    // with no rim after a React StrictMode remount.
    for (const state of STATES) expect(auraStateFor(state)).not.toBe("hidden");
  });
});

describe("auraLookFor", () => {
  it("uses the same band count and rings the whole orb in every state", () => {
    for (const state of STATES) {
      expect(auraLookFor(state).bands).toBe(5);
      expect(auraLookFor(state).placement).toBe("around");
    }
  });

  it("mirrors only while speaking", () => {
    expect(auraLookFor("speaking").layout).toBe("mirror");
    for (const state of ["idle", "listening", "thinking"] as const) {
      expect(auraLookFor(state).layout).not.toBe("mirror");
    }
  });

  it("slows the ripple while thinking", () => {
    expect(auraLookFor("thinking").rippleSpeed ?? 1).toBeLessThan(auraLookFor("listening").rippleSpeed ?? 0);
  });

  it("never varies colour per state", () => {
    for (const state of STATES) {
      expect((auraLookFor(state) as { palette?: string }).palette).toBeUndefined();
    }
  });

  it("blooms fully while listening and speaking, dimmer when thinking, dimmest idle", () => {
    const listening = auraLookFor("listening");
    const thinking = auraLookFor("thinking");
    const idle = auraLookFor("idle");
    for (const active of [listening, auraLookFor("speaking")]) {
      expect(active.aura ?? 0).toBeGreaterThanOrEqual(0.9);
      expect(active.brightness ?? 0).toBeGreaterThanOrEqual(0.85);
    }
    expect(thinking.aura ?? 0).toBeLessThan(listening.aura ?? 1);
    expect(idle.aura ?? 0).toBeLessThan(thinking.aura ?? 1);
    expect(idle.auraSize ?? 0).toBeLessThan(listening.auraSize ?? 1);
  });
});
