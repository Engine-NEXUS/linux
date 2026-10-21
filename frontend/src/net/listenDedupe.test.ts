import { describe, expect, it } from "vitest";
import { LISTEN_DEDUPE_WINDOW_MS, isDuplicateListen } from "./listenDedupe";

describe("isDuplicateListen", () => {
  it("never duplicates the first listen", () => {
    expect(isDuplicateListen(1000, 0, null, "hot_mic")).toBe(false);
  });

  it("flags a same-origin re-arm inside the window", () => {
    expect(isDuplicateListen(5000, 4500, "hot_mic", "hot_mic")).toBe(true);
    expect(isDuplicateListen(5000, 5000, "direct", "direct")).toBe(true); // same instant
  });

  it("allows a re-arm once the window has passed (normal turn spacing)", () => {
    expect(isDuplicateListen(5000, 5000 - LISTEN_DEDUPE_WINDOW_MS, "hot_mic", "hot_mic")).toBe(false);
    expect(isDuplicateListen(9000, 4000, "hot_mic", "hot_mic")).toBe(false);
  });

  it("never blocks a different origin (explicit user turn takes over a hot-mic re-arm)", () => {
    expect(isDuplicateListen(5000, 4900, "hot_mic", "direct")).toBe(false);
    expect(isDuplicateListen(5000, 4900, "direct", "hot_mic")).toBe(false);
  });

  it("tolerates a clock that moved backwards (not a duplicate)", () => {
    expect(isDuplicateListen(1000, 5000, "hot_mic", "hot_mic")).toBe(false);
  });
});
