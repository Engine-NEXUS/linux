import { describe, expect, it } from "vitest";
import {
  LOADING_FAILSAFE_MS,
  LOADING_HIDDEN,
  LOADING_MIN_DWELL_MS,
  loadingDue,
  loadingHide,
  loadingSettle,
  loadingShow,
} from "./loadingMachine";

describe("loadingMachine", () => {
  it("show makes visible with failsafe deadline", () => {
    const s = loadingShow(LOADING_HIDDEN, 1000);
    expect(s.visible).toBe(true);
    expect(s.shownAt).toBe(1000);
    expect(s.pendingAt).toBe(1000 + LOADING_FAILSAFE_MS);
  });

  it("show-while-showing refreshes failsafe without resetting dwell start", () => {
    const s1 = loadingShow(LOADING_HIDDEN, 1000);
    const s2 = loadingShow(s1, 2000);
    expect(s2.visible).toBe(true);
    expect(s2.shownAt).toBe(1000);
    expect(s2.pendingAt).toBe(2000 + LOADING_FAILSAFE_MS);
  });

  it("hide after dwell hides immediately", () => {
    const s1 = loadingShow(LOADING_HIDDEN, 1000);
    const s2 = loadingHide(s1, 1000 + LOADING_MIN_DWELL_MS + 1);
    expect(s2.visible).toBe(false);
    expect(s2.pendingAt).toBeNull();
  });

  it("hide before dwell delays to dwell end (no flash)", () => {
    const s1 = loadingShow(LOADING_HIDDEN, 1000);
    const s2 = loadingHide(s1, 1000 + 100);
    expect(s2.visible).toBe(true); // still shown until dwell end
    expect(s2.pendingAt).toBe(1000 + LOADING_MIN_DWELL_MS);
    expect(loadingDue(s2, 1000 + 100)).toBe(false);
    expect(loadingDue(s2, 1000 + LOADING_MIN_DWELL_MS)).toBe(true);
    const s3 = loadingSettle(s2, 1000 + LOADING_MIN_DWELL_MS);
    expect(s3.visible).toBe(false);
  });

  it("hide-when-hidden is a no-op (returns same snapshot)", () => {
    const s = loadingHide(LOADING_HIDDEN, 5000);
    expect(s).toBe(LOADING_HIDDEN);
  });

  it("failsafe settles a long-show to hidden", () => {
    const s1 = loadingShow(LOADING_HIDDEN, 1000);
    expect(loadingDue(s1, 1000 + LOADING_FAILSAFE_MS + 1)).toBe(true);
    const s2 = loadingSettle(s1, 1000 + LOADING_FAILSAFE_MS + 1);
    expect(s2.visible).toBe(false);
    expect(s2.pendingAt).toBeNull();
  });

  it("settle before deadline changes nothing", () => {
    const s1 = loadingShow(LOADING_HIDDEN, 1000);
    const s2 = loadingSettle(s1, 1000 + 100);
    expect(s2.visible).toBe(true);
    expect(s2.pendingAt).toBe(1000 + LOADING_FAILSAFE_MS);
  });
});
