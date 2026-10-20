import { describe, expect, it } from "vitest";
import { auraRendererState } from "./AuraCanvas";

describe("auraRendererState", () => {
  it("parks the aura whenever the orb is not shown", () => {
    for (const state of ["idle", "listening", "thinking", "speaking"] as const) {
      expect(auraRendererState(state, false)).toBe("hidden");
    }
  });

  it("follows the assistant state while the orb is shown", () => {
    expect(auraRendererState("listening", true)).toBe("active");
    expect(auraRendererState("thinking", true)).toBe("working");
    expect(auraRendererState("speaking", true)).toBe("active");
    expect(auraRendererState("idle", true)).toBe("paused");
  });
});
