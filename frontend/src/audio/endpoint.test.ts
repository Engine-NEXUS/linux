import { describe, expect, it } from "vitest";
import {
  DEFAULT_CONFIG,
  FRAME_MS,
  newState,
  observe,
  preRollStart,
  shouldFinalize,
  tailRms,
  type EndpointConfig,
  type EndpointState,
} from "./endpoint";

/** Feed a frame sequence and report the frame index at which we would cut off. */
function run(
  frames: Array<{ prob: number; rms: number }>,
  cfg: EndpointConfig = DEFAULT_CONFIG,
): { firedAt: number | null; state: EndpointState } {
  const s = newState();
  for (let i = 0; i < frames.length; i++) {
    observe(s, i, frames[i].prob, frames[i].rms, cfg);
    if (shouldFinalize(s, cfg)) return { firedAt: i, state: s };
  }
  return { firedAt: null, state: s };
}

/** N speech frames followed by N dead-silent frames. */
function utterance(speechFrames: number, silenceFrames: number) {
  return [
    ...Array.from({ length: speechFrames }, () => ({ prob: 0.95, rms: 0.05 })),
    ...Array.from({ length: silenceFrames }, () => ({ prob: 0.01, rms: 0.0005 })),
  ];
}

describe("adaptive endpoint — true cut-off", () => {
  it("fires once the tail is long enough and unambiguously dead", () => {
    const { firedAt } = run(utterance(20, 20));
    // 20 speech frames = 640ms (above minSpeechMs 500).
    // Fire needs 400ms of tail = 13 frames, so the earliest possible index is 20+12.
    expect(firedAt).not.toBeNull();
    expect(firedAt!).toBeGreaterThanOrEqual(20);
    expect(firedAt!).toBeLessThanOrEqual(20 + 13);
  });

  it("never waits longer than baseSilenceMs after speech stops", () => {
    const { firedAt } = run(utterance(20, 60));
    const tailFrames = firedAt! - 19; // frames of silence before the decision
    expect(tailFrames * FRAME_MS).toBeLessThanOrEqual(
      DEFAULT_CONFIG.baseSilenceMs + FRAME_MS,
    );
  });

  it("is far quicker than the 3000ms redemption it replaces", () => {
    const { firedAt } = run(utterance(20, 60));
    const waitedMs = (firedAt! - 19) * FRAME_MS;
    expect(waitedMs).toBeLessThan(3000);
    expect(DEFAULT_CONFIG.baseSilenceMs).toBeLessThan(1000);
  });

  it("does not fire on pure silence with no speech", () => {
    expect(run(Array.from({ length: 200 }, () => ({ prob: 0.01, rms: 0.0005 }))).firedAt)
      .toBeNull();
  });

  it("does not fire on a blip shorter than minSpeechMs", () => {
    // 4 frames = 128ms of speech, well under the 500ms floor.
    expect(run(utterance(4, 60)).firedAt).toBeNull();
  });
});

describe("adaptive endpoint — grammatical pauses must NOT cut off", () => {
  it("does not fire through a mid-sentence pause that keeps probability elevated", () => {
    // "deep analysis for the PR 24 in nexus-agent": two phrases split by a pause
    // where Silero still reports speech-like probability and real audio energy.
    const frames = [
      ...Array.from({ length: 30 }, () => ({ prob: 0.95, rms: 0.05 })),
      // 600ms "pause" — enough to blow past baseSilenceMs, but NOT dead.
      ...Array.from({ length: 19 }, () => ({ prob: 0.28, rms: 0.012 })),
      ...Array.from({ length: 30 }, () => ({ prob: 0.95, rms: 0.05 })),
      ...Array.from({ length: 20 }, () => ({ prob: 0.01, rms: 0.0005 })),
    ];
    const { firedAt } = run(frames);
    // Must not have cut during the pause (frames 30..48).
    expect(firedAt).toBeGreaterThan(48);
  });

  it("treats the ambiguous band as neither speech nor silence", () => {
    // A long ambiguous stretch must not accumulate tail silence, so it cannot
    // reach baseSilenceMs on its own.
    const s = newState();
    observe(s, 0, 0.95, 0.05); // speech
    for (let i = 1; i <= 40; i++) observe(s, i, 0.42, 0.01); // ambiguous
    expect(s.tailSilenceMs).toBe(0);
    expect(s.speechMs).toBe(FRAME_MS);
    expect(shouldFinalize(s)).toBe(false);
  });

  it("voids tail evidence when speech resumes, forcing a full re-accumulation", () => {
    const s = newState();
    for (let i = 0; i < 20; i++) observe(s, i, 0.95, 0.05);
    for (let i = 20; i < 32; i++) observe(s, i, 0.02, 0.0005); // 384ms near-silence
    // Right on the edge, then the speaker continues.
    observe(s, 32, 0.9, 0.05);
    expect(s.tailSilenceMs).toBe(0);
    expect(s.tailMaxProb).toBe(0);
    expect(shouldFinalize(s)).toBe(false);
  });

  it("does not fire when the tail is quiet but probability never collapsed", () => {
    // Digital silence is not the only thing that matters — if Silero still sees
    // something speech-like, we defer to the old path.
    const frames = [
      ...Array.from({ length: 20 }, () => ({ prob: 0.95, rms: 0.05 })),
      ...Array.from({ length: 30 }, () => ({ prob: 0.16, rms: 0.0004 })),
    ];
    expect(run(frames).firedAt).toBeNull();
  });

  it("does not fire when probability collapsed but room tone persists", () => {
    const frames = [
      ...Array.from({ length: 20 }, () => ({ prob: 0.95, rms: 0.05 })),
      ...Array.from({ length: 30 }, () => ({ prob: 0.01, rms: 0.02 })),
    ];
    expect(run(frames).firedAt).toBeNull();
  });
});

describe("adaptive endpoint — multi-clause commands are not truncated", () => {
  it("survives several pauses and cuts only after the final one", () => {
    const phrase = Array.from({ length: 25 }, () => ({ prob: 0.95, rms: 0.05 }));
    const pause = Array.from({ length: 18 }, () => ({ prob: 0.3, rms: 0.014 }));
    const frames = [
      ...phrase, ...pause,
      ...phrase, ...pause,
      ...phrase, ...pause,
      ...phrase,
      ...Array.from({ length: 20 }, () => ({ prob: 0.01, rms: 0.0005 })),
    ];
    const { firedAt } = run(frames);
    const lastSpeechIdx = 3 * (25 + 18) + 24; // index of final speech frame
    expect(firedAt).toBeGreaterThan(lastSpeechIdx);
  });
});

describe("pre-roll keeps the onset of the utterance", () => {
  it("walks back from the detected onset", () => {
    // onset at frame 10, 320ms pre-roll ≈ 10 frames.
    expect(preRollStart(10, 320, 100)).toBe(0);
    expect(preRollStart(50, 320, 100)).toBe(40);
  });

  it("clamps at the start of the buffer when onset is early", () => {
    expect(preRollStart(3, 320, 100)).toBe(0);
    expect(preRollStart(-1, 320, 100)).toBe(0);
  });

  it("never runs past the end of the buffer", () => {
    expect(preRollStart(1000, 320, 100)).toBe(90);
  });
});

describe("config plumbing", () => {
  it("respects enabled=false", () => {
    expect(run(utterance(20, 20), { ...DEFAULT_CONFIG, enabled: false }).firedAt)
      .toBeNull();
  });

  it("respects a raised baseSilenceMs", () => {
    const strict = { ...DEFAULT_CONFIG, baseSilenceMs: 1200 };
    const { firedAt } = run(utterance(20, 80), strict);
    expect(firedAt).not.toBeNull();
    expect((firedAt! - 19) * FRAME_MS).toBeGreaterThanOrEqual(1200);
  });

  it("reports zero tail rms with no tail frames", () => {
    expect(tailRms(newState())).toBe(0);
  });
});