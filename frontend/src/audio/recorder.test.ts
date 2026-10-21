import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn().mockResolvedValue(undefined);

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { useAssistant } from "../store/assistant";
import {
  __testMissStreak,
  __testSetMissStreak,
  hideOrbIfIdle,
  nextMissAction,
  processTranscript,
} from "./recorder";

describe("hideOrbIfIdle (Main Center UI-director rule, doc 74 P2)", () => {
  beforeEach(() => {
    useAssistant.setState({ ghostActive: false, visible: true });
  });

  it("hides the orb on turn end outside ghost mode", () => {
    useAssistant.setState({ visible: true, ghostActive: false });
    hideOrbIfIdle();
    expect(useAssistant.getState().visible).toBe(false);
  });

  it("keeps the orb visible for the WHOLE ghost session", () => {
    useAssistant.setState({ visible: true, ghostActive: true });
    hideOrbIfIdle();
    expect(useAssistant.getState().visible).toBe(true);
    // Repeated turn ends must not hide it either.
    hideOrbIfIdle();
    hideOrbIfIdle();
    expect(useAssistant.getState().visible).toBe(true);
  });
});

describe("processTranscript provenance gating", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useAssistant.setState({
      ghostActive: false,
      visible: true,
      state: "listening",
      transcript: [],
    });
    __testSetMissStreak(0);
  });

  it("drops a rejected turn without relistening, nagging, learning, or remembering", async () => {
    await processTranscript("", {
      ownership: "rejected",
      ownerScore: 0.12,
      decoderBias: "neutral",
      language: "en",
      initiation: "ghost",
    });

    expect(useAssistant.getState().visible).toBe(false);
    expect(useAssistant.getState().transcript).toEqual([]);
    expect(__testMissStreak()).toBe(0);
    await vi.advanceTimersByTimeAsync(550);
    expect(useAssistant.getState().state).toBe("idle");
  });
});

describe("nextMissAction (approach E retry-with-escalation)", () => {
  beforeEach(() => {
    __testSetMissStreak(0);
  });

  it("1st consecutive miss relistens silently", () => {
    expect(nextMissAction(1)).toBe("relisten");
    expect(nextMissAction(0)).toBe("relisten");
  });

  it("2nd consecutive miss nags once", () => {
    expect(nextMissAction(2)).toBe("nag");
    expect(nextMissAction(5)).toBe("nag");
  });

  it("streak helpers round-trip", () => {
    expect(__testMissStreak()).toBe(0);
    __testSetMissStreak(1);
    expect(__testMissStreak()).toBe(1);
    expect(nextMissAction(__testMissStreak())).toBe("relisten");
    __testSetMissStreak(0);
  });
});
