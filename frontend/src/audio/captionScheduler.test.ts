import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  partitionIntoLines,
  CaptionWord,
  CaptionTrack,
  clearCaptionSchedule,
  suppressNextCaption,
  __testScheduleChunk,
} from "./captionScheduler";
import { useAssistant } from "../store/assistant";

function track(words: CaptionWord[], estimated = false): CaptionTrack {
  const last = words[words.length - 1];
  return { words, total_ms: last.start_ms + last.duration_ms, estimated, envelope: [], frame_ms: 0, envelope_start_ms: 0 };
}

describe("captionScheduler line partitioning and lifecycle", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    clearCaptionSchedule();
  });

  afterEach(() => {
    clearCaptionSchedule();
    vi.useRealTimers();
  });

  it("partitions words into clean 5-word clauses and sentence boundaries", () => {
    // "hi lakshya i am nexus. how can i help u today? this is the analyssi result"
    const words: CaptionWord[] = [
      { text: "hi", start_ms: 0, duration_ms: 200 },
      { text: "lakshya", start_ms: 220, duration_ms: 300 },
      { text: "i", start_ms: 540, duration_ms: 150 },
      { text: "am", start_ms: 700, duration_ms: 150 },
      { text: "nexus.", start_ms: 860, duration_ms: 300 },
      { text: "how", start_ms: 1200, duration_ms: 200 },
      { text: "can", start_ms: 1420, duration_ms: 180 },
      { text: "i", start_ms: 1620, duration_ms: 150 },
      { text: "help", start_ms: 1780, duration_ms: 200 },
      { text: "u", start_ms: 2000, duration_ms: 150 },
      { text: "today?", start_ms: 2160, duration_ms: 300 },
      { text: "this", start_ms: 2500, duration_ms: 200 },
      { text: "is", start_ms: 2710, duration_ms: 180 },
      { text: "the", start_ms: 2900, duration_ms: 150 },
      { text: "analyssi", start_ms: 3060, duration_ms: 350 },
      { text: "result", start_ms: 3420, duration_ms: 300 },
    ];

    const lines = partitionIntoLines(words);
    expect(lines.length).toBe(3);
    expect(lines[0].text).toBe("hi lakshya i am nexus.");
    expect(lines[1].text).toBe("how can i help u today?");
    expect(lines[2].text).toBe("this is the analyssi result");
  });

  it("partitions by 10-word limit even without punctuation", () => {
    const words: CaptionWord[] = [
      { text: "one", start_ms: 0, duration_ms: 100 },
      { text: "two", start_ms: 120, duration_ms: 100 },
      { text: "three", start_ms: 240, duration_ms: 100 },
      { text: "four", start_ms: 360, duration_ms: 100 },
      { text: "five", start_ms: 480, duration_ms: 100 },
      { text: "six", start_ms: 600, duration_ms: 100 },
      { text: "seven", start_ms: 720, duration_ms: 100 },
      { text: "eight", start_ms: 840, duration_ms: 100 },
      { text: "nine", start_ms: 960, duration_ms: 100 },
      { text: "ten", start_ms: 1080, duration_ms: 100 },
      { text: "eleven", start_ms: 1200, duration_ms: 100 },
    ];

    const lines = partitionIntoLines(words);
    expect(lines.length).toBe(2);
    expect(lines[0].text).toBe("one two three four five six seven eight nine ten");
    expect(lines[1].text).toBe("eleven");
  });

  it("suppressNextCaption cancels the next scheduleChunk and resets", () => {
    suppressNextCaption();
    // clearCaptionSchedule resets suppress flag
    clearCaptionSchedule();
    expect(useAssistant.getState().captionActive).toBe(false);
  });

  it("unescapes SSML/XML entities like didn&apos;t during partition as a complete sentence", () => {
    const words: CaptionWord[] = [
      { text: "i", start_ms: 0, duration_ms: 100 },
      { text: "didn&apos;t", start_ms: 110, duration_ms: 150 },
      { text: "hear", start_ms: 270, duration_ms: 120 },
      { text: "you,", start_ms: 400, duration_ms: 130 },
      { text: "sir.", start_ms: 540, duration_ms: 200 },
    ];
    const lines = partitionIntoLines(words);
    expect(lines.length).toBe(1);
    expect(lines[0].text).toBe("i didn't hear you, sir.");
    expect(lines[0].words.length).toBe(5);
    expect(lines[0].words[1].text).toBe("didn&apos;t");
  });

  it("stores words array per partitioned line for incremental spoken reveal", () => {
    const words: CaptionWord[] = [
      { text: "Good", start_ms: 0, duration_ms: 200 },
      { text: "morning,", start_ms: 220, duration_ms: 250 },
      { text: "sir.", start_ms: 480, duration_ms: 300 },
    ];
    const lines = partitionIntoLines(words);
    expect(lines.length).toBe(1);
    expect(lines[0].words.map((w) => w.text)).toEqual(["Good", "morning,", "sir."]);
  });
});

describe("scheduleChunk utterance-end grace window (Part 5 of the sync audit)", () => {
  // A line's clear-timer can only tell whether it's the last line it KNOWS
  // about — not whether a later streamed chunk is simply running behind.
  // Closing captionActive immediately on that guess is the "caption
  // disappeared before speech ended" bug; the grace window gives a
  // genuinely-in-flight next chunk a chance to arrive and cancel the close.
  beforeEach(() => {
    vi.useFakeTimers();
    clearCaptionSchedule();
  });

  afterEach(() => {
    clearCaptionSchedule();
    vi.useRealTimers();
  });

  // One short, complete-sentence line: startDelay=0, displayDuration=2000ms
  // (the 2000ms minDwell floor dominates), fadeDelay=2000, clearDelay=2450.
  const chunk1 = track([{ text: "Hi.", start_ms: 0, duration_ms: 200 }]);

  it("a late-arriving next chunk cancels the premature close (never flips captionActive false)", () => {
    __testScheduleChunk(chunk1);
    vi.advanceTimersByTime(2450); // chunk1's clear-timer fires, arms the grace timer
    expect(useAssistant.getState().captionActive).toBe(true);

    vi.advanceTimersByTime(250); // mid-grace — chunk 2 arrives late, but in time
    __testScheduleChunk(track([{ text: "Still here.", start_ms: 2450, duration_ms: 300 }]));

    vi.advanceTimersByTime(1000); // well past where the cancelled grace timer would have fired
    expect(useAssistant.getState().captionActive).toBe(true);
  });

  it("with no next chunk, the caption actually closes after the grace window", () => {
    __testScheduleChunk(chunk1);
    vi.advanceTimersByTime(2450); // clear-timer fires, grace timer armed
    expect(useAssistant.getState().captionActive).toBe(true);

    vi.advanceTimersByTime(499); // just before the 500ms grace elapses
    expect(useAssistant.getState().captionActive).toBe(true);

    vi.advanceTimersByTime(2); // grace elapses
    expect(useAssistant.getState().captionActive).toBe(false);
  });

  it("estimated (local-engine) tracks get a wider safety margin before fading", async () => {
    const { onCaptionLineUpdate } = await import("./captionScheduler");
    const realTiming = track([{ text: "Hi.", start_ms: 0, duration_ms: 200 }], false);
    const estimatedTiming = track([{ text: "Hi.", start_ms: 0, duration_ms: 200 }], true);

    // Real timing (edge-tts): fadeDelay = 0 + max(2000, 300+1500) = 2000ms.
    let faded = false;
    let unsub = onCaptionLineUpdate((line) => {
      if (line.phase === "fading") faded = true;
    });
    __testScheduleChunk(realTiming);
    vi.advanceTimersByTime(1999);
    expect(faded).toBe(false);
    vi.advanceTimersByTime(1);
    expect(faded).toBe(true); // fired at exactly 2000ms
    unsub();
    clearCaptionSchedule();

    // Estimated (local Kokoro): same math plus the +300ms margin on linger
    // -> fadeDelay = 0 + max(2000, 300 + 1800) = 2100ms — 100ms later.
    faded = false;
    unsub = onCaptionLineUpdate((line) => {
      if (line.phase === "fading") faded = true;
    });
    __testScheduleChunk(estimatedTiming);
    vi.advanceTimersByTime(2099);
    expect(faded).toBe(false); // the margin held it open past the real-timing point
    vi.advanceTimersByTime(1);
    expect(faded).toBe(true); // fired at exactly 2100ms
    unsub();
  });
});
