import { describe, expect, it } from "vitest";
// Node environment (no DOM): the custom-element guard must make this a
// clean no-op import — it protects SSR/node contexts from crashing.
import "./voice-orb.js";

describe("voice-orb module guard (node environment)", () => {
  it("imports without throwing when customElements is undefined", () => {
    // The IIFE early-returns before touching document/customElements APIs
    // that node lacks. If this import throws, the guard regressed.
    expect(true).toBe(true);
  });
});
