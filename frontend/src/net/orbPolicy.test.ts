import { describe, expect, it } from "vitest";
import {
  GHOST_ORB_LINGER_MS,
  GHOST_VOICE_LEVEL,
  initialGhostOrbState,
  stepGhostOrb,
  type GhostOrbInputs,
} from "./orbPolicy";

const quiet: GhostOrbInputs = {
  state: "listening",
  micLevel: 0,
  captionActive: false,
  ttsActive: false,
  loadingVisible: false,
  awaitingInput: false,
  parked: false,
};

describe("ghost orb policy", () => {
  it("hides after the linger when the hot mic only listens to a quiet room", () => {
    const t0 = 1_000;
    let st = initialGhostOrbState(t0);
    expect(stepGhostOrb(st, quiet, t0 + 1).shown).toBe(true);
    const r = stepGhostOrb(st, quiet, t0 + GHOST_ORB_LINGER_MS + 1);
    expect(r.shown).toBe(false);
    st = r.next;
    // still hidden a long time later
    expect(stepGhostOrb(st, quiet, t0 + 60_000).shown).toBe(false);
  });

  it("room noise below the voice level does not keep the orb up", () => {
    const st = initialGhostOrbState(0);
    const noisy = { ...quiet, micLevel: GHOST_VOICE_LEVEL - 0.05 };
    expect(stepGhostOrb(st, noisy, GHOST_ORB_LINGER_MS + 10).shown).toBe(false);
  });

  it("a voice, thinking, speaking, caption, loader or question show it and restart the linger", () => {
    const st = { shownUntil: 0 };
    const cases: Partial<GhostOrbInputs>[] = [
      { micLevel: 1 },
      { state: "thinking" },
      { state: "speaking" },
      { captionActive: true },
      { ttsActive: true },
      { loadingVisible: true },
      { awaitingInput: true },
    ];
    for (const c of cases) {
      const r = stepGhostOrb(st, { ...quiet, ...c }, 10_000);
      expect(r.shown, JSON.stringify(c)).toBe(true);
      expect(r.next.shownUntil).toBe(10_000 + GHOST_ORB_LINGER_MS);
    }
  });

  it("lingers after speaking ends, then hides", () => {
    let st = { shownUntil: 0 };
    st = stepGhostOrb(st, { ...quiet, state: "speaking" }, 5_000).next;
    expect(stepGhostOrb(st, quiet, 5_000 + GHOST_ORB_LINGER_MS - 1).shown).toBe(true);
    expect(stepGhostOrb(st, quiet, 5_000 + GHOST_ORB_LINGER_MS + 1).shown).toBe(false);
  });

  it("a parked mic keeps the orb visible so the user can see it stopped listening", () => {
    const st = { shownUntil: 0 };
    const r = stepGhostOrb(st, { ...quiet, parked: true }, 99_000);
    expect(r.shown).toBe(true);
  });
});
