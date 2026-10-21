import { describe, expect, it } from "vitest";
import { levelFor, type LevelInputs } from "./levels";

const base: LevelInputs = { state: "idle", ttsActive: false, envelope: null, audioVolume: 0, micLevel: 0 };

describe("levelFor", () => {
  it("is silent while idle or thinking, whatever the mic hears", () => {
    expect(levelFor({ ...base, micLevel: 0.9, audioVolume: 0.9 })).toBe(0);
    expect(levelFor({ ...base, state: "thinking", micLevel: 0.9 })).toBe(0);
  });

  it("follows the louder mic source while listening", () => {
    expect(levelFor({ ...base, state: "listening", micLevel: 0.3, audioVolume: 0.1 })).toBe(0.3);
    expect(levelFor({ ...base, state: "listening", micLevel: 0.1, audioVolume: 0.6 })).toBe(0.6);
  });

  it("prefers the TTS envelope while speaking", () => {
    expect(levelFor({ ...base, state: "speaking", envelope: 0.25, ttsActive: true, audioVolume: 0.9 })).toBe(0.25);
  });

  it("falls back to the playback proxy while speaking without an envelope", () => {
    expect(levelFor({ ...base, state: "speaking", ttsActive: true, audioVolume: 0.1 })).toBe(0.4);
    expect(levelFor({ ...base, state: "speaking", ttsActive: true, audioVolume: 0.7 })).toBe(0.7);
    expect(levelFor({ ...base, state: "speaking", ttsActive: false })).toBe(0);
  });

  it("never mixes sources: the mic is ignored while speaking", () => {
    expect(levelFor({ ...base, state: "speaking", envelope: 0.2, micLevel: 1 })).toBe(0.2);
  });

  it("clamps to 0..1 and never returns NaN", () => {
    expect(levelFor({ ...base, state: "listening", micLevel: 3 })).toBe(1);
    expect(levelFor({ ...base, state: "listening", micLevel: Number.NaN })).toBe(0);
    expect(levelFor({ ...base, state: "speaking", envelope: Number.NaN, ttsActive: false })).toBe(0);
    expect(levelFor({ ...base, state: "speaking", envelope: -1 })).toBe(0);
  });
});
