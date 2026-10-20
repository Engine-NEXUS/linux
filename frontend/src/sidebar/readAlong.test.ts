import { describe, expect, it } from "vitest";
import type { CaptionLineEvent } from "../audio/captionScheduler";
import { readAlongFromEvent, readAlongText, readAlongWords } from "./readAlong";

/**
 * The karaoke highlight must never disagree with what is actually being
 * spoken. These tests pin the state machine on the scheduler's real word
 * data — and the two ways the renderer must fall back to plain text instead
 * of guessing (no word data, or words that contradict the line's own text).
 */

describe("readAlongWords — state boundaries", () => {
  const words = ["alpha", "beta", "gamma"];

  it("marks the word at the index active, earlier words spoken, later pending", () => {
    expect(readAlongWords(words, 1)).toEqual([
      { text: "alpha", state: "spoken" },
      { text: "beta", state: "active" },
      { text: "gamma", state: "pending" },
    ]);
  });

  it("marks everything spoken once the index passes the end", () => {
    expect(readAlongWords(words, 3).every((w) => w.state === "spoken")).toBe(true);
  });

  it("treats a negative index as the whole line already spoken (fading phase)", () => {
    expect(readAlongWords(words, -1).every((w) => w.state === "spoken")).toBe(true);
  });

  it("keeps word text verbatim, including partial reveal snapshots", () => {
    // During the letter-by-letter reveal the scheduler splits the snapshot,
    // so the active word can be a prefix of the real word ("hel" of "hello").
    expect(readAlongWords(["hel"], 0)).toEqual([{ text: "hel", state: "active" }]);
  });

  it("returns empty for no words", () => {
    expect(readAlongWords([], 0)).toEqual([]);
  });
});

describe("readAlongText", () => {
  it("joins with single spaces — the scheduler's exact text contract", () => {
    expect(readAlongText(["open", "the", "pod"])).toBe("open the pod");
  });
});

describe("readAlongFromEvent", () => {
  it("builds states from a live active-phase event", () => {
    const event: CaptionLineEvent = {
      text: "open the pod",
      phase: "active",
      words: ["open", "the", "pod"],
      activeWordIndex: 2,
    };
    expect(readAlongFromEvent(event)).toEqual([
      { text: "open", state: "spoken" },
      { text: "the", state: "spoken" },
      { text: "pod", state: "active" },
    ]);
  });

  it("builds all-spoken states from a fading event (index -1)", () => {
    const event: CaptionLineEvent = {
      text: "open the pod",
      phase: "fading",
      words: ["open", "the", "pod"],
      activeWordIndex: -1,
    };
    expect(readAlongFromEvent(event)?.every((w) => w.state === "spoken")).toBe(true);
  });

  it("falls back to plain text when the event carries no words", () => {
    const cleared: CaptionLineEvent = { text: "", phase: "cleared" };
    expect(readAlongFromEvent(cleared)).toBe(null);

    const legacy: CaptionLineEvent = { text: "no word data", phase: "active" };
    expect(readAlongFromEvent(legacy)).toBe(null);
  });

  it("falls back to plain text when words disagree with the event's text", () => {
    // The scheduler guarantees text === words.join(" "); if that ever
    // drifts, rendering the words would show a caption that disagrees with
    // the audio. The plain text is the truth that survived.
    const event: CaptionLineEvent = {
      text: "open the pod bay doors",
      phase: "active",
      words: ["open", "the", "pod"],
      activeWordIndex: 2,
    };
    expect(readAlongFromEvent(event)).toBe(null);
  });

  it("treats a missing activeWordIndex as fully spoken rather than guessing", () => {
    const event: CaptionLineEvent = {
      text: "done",
      phase: "fading",
      words: ["done"],
    };
    expect(readAlongFromEvent(event)).toEqual([{ text: "done", state: "spoken" }]);
  });
});
