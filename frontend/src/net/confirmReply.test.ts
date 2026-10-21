import { describe, expect, it } from "vitest";
import { classifyConfirmReply, isConfirmExpired, PENDING_CONFIRM_TTL_MS } from "./confirmReply";

describe("classifyConfirmReply", () => {
  it("accepts real yes answers, including polite and verb-bearing ones", () => {
    for (const t of [
      "yes", "Yes.", "yeah sure", "okay", "ok do it", "go ahead", "Yes, proceed.", "approved", "sure, go ahead please",
      "nexus yes", "do it", "confirm", "yes send it", "yes merge it", "proceed with the merge", "yes please", "absolutely",
    ]) {
      expect(classifyConfirmReply(t), t).toBe("yes");
    }
  });

  it("accepts real no answers and lets no beat yes", () => {
    for (const t of ["no", "No.", "nope", "cancel", "cancel that", "don't do it", "no don't", "not now", "never mind", "stop", "abort", "no thanks"]) {
      expect(classifyConfirmReply(t), t).toBe("no");
    }
    expect(classifyConfirmReply("yes no")).toBe("no");
  });

  it("does NOT treat a sentence that merely starts with an answer word as an answer (the old bug)", () => {
    for (const t of [
      "okay so what is the weather",
      "ok open notepad",
      "sure what time is it",
      "yes and also open chrome",
      "yeah I think the report is ready",
      "ok close notepad",
      "proceed to open youtube",
    ]) {
      expect(classifyConfirmReply(t), t).toBe("other");
    }
  });

  it("is not fooled by empty, long or unrelated speech", () => {
    for (const t of ["", "   ", "what is the weather", "open brave", "hello nexus", "yes yes yes yes yes yes yes yes"]) {
      expect(classifyConfirmReply(t), t).toBe("other");
    }
  });
});

describe("isConfirmExpired", () => {
  it("never expires without a timestamp and expires after the TTL", () => {
    expect(isConfirmExpired(null, 1_000_000)).toBe(false);
    expect(isConfirmExpired(1_000, 1_000 + PENDING_CONFIRM_TTL_MS)).toBe(false);
    expect(isConfirmExpired(1_000, 1_000 + PENDING_CONFIRM_TTL_MS + 1)).toBe(true);
  });
});
