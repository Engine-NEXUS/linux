import { describe, expect, it } from "vitest";
import { resolveAvatarAnim, shouldHoldSpeakingFrame } from "./Avatar";

describe("resolveAvatarAnim", () => {
  it("listening plays the wake-loading segment once", () => {
    const a = resolveAvatarAnim("listening");
    expect(a.segment).toEqual([171, 260]);
    expect(a.loop).toBe(false);
    expect(a.speed).toBe(1.5);
    expect(a.mode).toBe("wake-loading");
  });

  it("thinking and speaking share the loading loop (guard skips restart)", () => {
    const t = resolveAvatarAnim("thinking");
    const s = resolveAvatarAnim("speaking");
    expect(t.mode).toBe("loading-loop");
    expect(s.mode).toBe("loading-loop");
    expect(t.segment).toEqual(s.segment);
    // Only speed differs — the guard updates speed without replaying.
    expect(t.speed).toBe(1.5);
    expect(s.speed).toBe(1.2);
  });

  it("idle plays the smile arrival once", () => {
    const a = resolveAvatarAnim("idle");
    expect(a.segment).toEqual([261, 316]);
    expect(a.loop).toBe(false);
    expect(a.speed).toBe(1.0);
    expect(a.mode).toBe("idle-smile");
  });

  it("every state maps to a distinct mode except thinking/speaking", () => {
    const modes = ["listening", "thinking", "speaking", "idle"].map(
      (st) => resolveAvatarAnim(st as "idle").mode
    );
    expect(new Set(modes).size).toBe(3);
  });
});

describe("shouldHoldSpeakingFrame", () => {
  it("loops while TTS audio is playing", () => {
    expect(shouldHoldSpeakingFrame("speaking", true)).toBe(false);
  });

  it("holds the frame on silent speaking (meeting suppression, gaps, waits)", () => {
    expect(shouldHoldSpeakingFrame("speaking", false)).toBe(true);
  });

  it("never gates listening/thinking on audio (mic/network feedback)", () => {
    expect(shouldHoldSpeakingFrame("listening", false)).toBe(false);
    expect(shouldHoldSpeakingFrame("listening", true)).toBe(false);
    expect(shouldHoldSpeakingFrame("thinking", false)).toBe(false);
    expect(shouldHoldSpeakingFrame("thinking", true)).toBe(false);
    expect(shouldHoldSpeakingFrame("idle", false)).toBe(false);
  });
});
