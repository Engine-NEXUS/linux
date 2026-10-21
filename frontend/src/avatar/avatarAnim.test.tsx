import { describe, expect, it } from "vitest";
import { easeScrubFrame, GHOST_LEAVE_MS, GHOST_PINCH_MS, ghostWaveBars, isWavesPreview, resolveAvatarAnim, resolveWavesShown, REST_DOT_X, REST_DOT_Y, restDotScale, scrubLevel, shouldHoldSpeakingFrame, shouldShowGhostParticles, shouldShowWaves, ttsWaveLevel, waveEnterDelayMs, waveScaleForBar, waveSource, WAVE_REST_FLOOR, zoomForFrame } from "./Avatar";

describe("resolveAvatarAnim", () => {
  it("listening resolves to the calm smile, not loading circles", () => {
    const a = resolveAvatarAnim("listening");
    expect(a.segment).toEqual([261, 316]);
    expect(a.loop).toBe(false);
    expect(a.speed).toBe(1.25);
    expect(a.mode).toBe("idle-smile");
  });

  it("speaking shows the smile and relies on frame-locked zoom, unlike thinking", () => {
    const t = resolveAvatarAnim("thinking");
    const s = resolveAvatarAnim("speaking");
    expect(t.mode).toBe("loading-loop");
    expect(s.mode).toBe("idle-smile");
  });

  it("all states play lively at 1.25x (user directive)", () => {
    for (const st of ["idle", "listening", "thinking", "speaking"] as const) {
      expect(resolveAvatarAnim(st).speed).toBe(1.25);
    }
  });

  it("idle plays the smile arrival once", () => {
    const a = resolveAvatarAnim("idle");
    expect(a.segment).toEqual([261, 316]);
    expect(a.loop).toBe(false);
    expect(a.speed).toBe(1.25);
    expect(a.mode).toBe("idle-smile");
  });

  it("thinking is exclusively loading-loop, others resolve to idle-smile", () => {
    expect(resolveAvatarAnim("thinking").mode).toBe("loading-loop");
    expect(resolveAvatarAnim("listening").mode).toBe("idle-smile");
    expect(resolveAvatarAnim("speaking").mode).toBe("idle-smile");
    expect(resolveAvatarAnim("idle").mode).toBe("idle-smile");
  });
});

describe("ghostWaveBars", () => {
  it("uses the exact three Lottie palette colors, tall-short-tall", () => {
    const bars = ghostWaveBars();
    expect(bars.map((b) => b.color)).toEqual(["#259ed6", "#ef4f25", "#fbdf38"]);
    expect(bars[0].height).toBeGreaterThan(bars[1].height);
    expect(bars[2].height).toBeGreaterThan(bars[1].height);
  });

  it("staggers animation phases so bars never move in lockstep", () => {
    const bars = ghostWaveBars();
    const keys = new Set(bars.map((b) => `${b.durationMs}/${b.delayMs}`));
    expect(keys.size).toBe(3);
  });

  it("pinch is a fast sub-beat of the transition, not the show", () => {
    expect(GHOST_PINCH_MS).toBeLessThanOrEqual(250);
  });

  it("exit beat is instant-feeling but not a hard cut", () => {
    expect(GHOST_LEAVE_MS).toBeLessThanOrEqual(250);
    expect(GHOST_LEAVE_MS).toBeGreaterThan(0);
  });

  it("entrance blooms left-to-right inside the stagger window", () => {
    expect(waveEnterDelayMs(0)).toBe(0);
    expect(waveEnterDelayMs(1)).toBe(90);
    expect(waveEnterDelayMs(2)).toBe(180);
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

describe("waveSource", () => {
  it("drives from TTS while NEXUS talks, mic while the user talks", () => {
    expect(waveSource("speaking", true, 0.9)).toBe("tts");
    expect(waveSource("listening", false, 0.5)).toBe("mic");
  });

  it("rests on silence, idle, thinking, and muted speaking", () => {
    expect(waveSource("listening", false, 0)).toBe("rest");
    expect(waveSource("listening", false, WAVE_REST_FLOOR / 2)).toBe("rest");
    expect(waveSource("speaking", false, 0.9)).toBe("rest");
    expect(waveSource("thinking", false, 0.9)).toBe("rest");
    expect(waveSource("idle", false, 0)).toBe("rest");
  });
});

describe("waveScaleForBar", () => {
  it("rests flat below the floor, scales monotonically above it", () => {
    expect(waveScaleForBar(0, 0)).toBe(0.15);
    expect(waveScaleForBar(0.01, 1)).toBe(0.15);
    const q = waveScaleForBar(0.2, 0);
    const m = waveScaleForBar(0.6, 0);
    expect(q).toBeGreaterThan(0.15);
    expect(m).toBeGreaterThan(q);
    expect(m).toBeLessThanOrEqual(1);
    expect(waveScaleForBar(2, 0)).toBeLessThanOrEqual(1);
  });

  it("staggers bars so the trio never pumps in lockstep", () => {
    const scales = [0, 1, 2].map((i) => waveScaleForBar(0.8, i));
    expect(new Set(scales).size).toBe(3);
  });
});

describe("ttsWaveLevel", () => {
  it("stays in 0..1 and varies over time per bar", () => {
    const a = ttsWaveLevel(0, 0);
    const b = ttsWaveLevel(500, 0);
    const c = ttsWaveLevel(500, 1);
    for (const v of [a, b, c]) {
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThanOrEqual(1);
    }
    expect(b).not.toBe(a);
    expect(c).not.toBe(b);
  });
});

describe("shouldShowWaves", () => {
  it("shows waves only in a visible ghost session — never on plain wake, never hidden", () => {
    expect(shouldShowWaves(true, true)).toBe(true);
    expect(shouldShowWaves(false, true)).toBe(false);
    expect(shouldShowWaves(true, false)).toBe(false);
    expect(shouldShowWaves(false, false)).toBe(false);
  });
});

describe("shouldShowGhostParticles (orb-as-display plan)", () => {
  it("the WebGL orb is the ghost display only in a visible session — same invariant as waves", () => {
    expect(shouldShowGhostParticles(true, true)).toBe(true);
    expect(shouldShowGhostParticles(false, true)).toBe(false);
    expect(shouldShowGhostParticles(true, false)).toBe(false);
    expect(shouldShowGhostParticles(false, false)).toBe(false);
  });
});

describe("resolveWavesShown (Phase 0 calibration fix)", () => {
  it("shows the container in ghost waves/leaving phases regardless of target", () => {
    expect(resolveWavesShown("waves", null)).toBe(true);
    expect(resolveWavesShown("waves", "wakeup")).toBe(true);
    expect(resolveWavesShown("leaving", null)).toBe(true);
  });

  it("shows the container for the Waves calibration target without a ghost session", () => {
    expect(resolveWavesShown("smile", "waves")).toBe(true);
    expect(resolveWavesShown("pinching", "waves")).toBe(true);
  });

  it("hides the container for non-waves targets outside ghost phases", () => {
    expect(resolveWavesShown("smile", null)).toBe(false);
    expect(resolveWavesShown("smile", "wakeup")).toBe(false);
    expect(resolveWavesShown("smile", "loading")).toBe(false);
    expect(resolveWavesShown("pinching", null)).toBe(false);
  });

  it("isWavesPreview is true only for the waves target", () => {
    expect(isWavesPreview("waves")).toBe(true);
    expect(isWavesPreview("wakeup")).toBe(false);
    expect(isWavesPreview("loading")).toBe(false);
    expect(isWavesPreview(null)).toBe(false);
  });
});

describe("scrubLevel", () => {
  it("parks at 0 on rest, clamps mic, averages TTS bars", () => {
    expect(scrubLevel("rest", 0, 0.9)).toBe(0);
    expect(scrubLevel("mic", 0, 0)).toBe(0);
    expect(scrubLevel("mic", 0, 0.6)).toBeCloseTo(0.6, 5);
    expect(scrubLevel("mic", 0, 2)).toBe(1);
    const t = scrubLevel("tts", 500, 0);
    expect(t).toBeGreaterThanOrEqual(0);
    expect(t).toBeLessThanOrEqual(1);
    expect(t).toBeCloseTo(
      (ttsWaveLevel(500, 0) + ttsWaveLevel(500, 1) + ttsWaveLevel(500, 2)) / 3,
      10,
    );
  });
});

describe("easeScrubFrame", () => {
  it("steps toward the target without overshoot and settles", () => {
    let f = 0;
    for (let i = 0; i < 60; i++) f = easeScrubFrame(f, 54);
    expect(f).toBeCloseTo(54, 3);
    let g = 54;
    for (let i = 0; i < 60; i++) g = easeScrubFrame(g, 0);
    expect(g).toBeCloseTo(0, 3);
    expect(easeScrubFrame(10, 20)).toBeCloseTo(13, 5);
    expect(easeScrubFrame(20, 10)).toBeCloseTo(17, 5);
  });
});

describe("restDotScale", () => {
  it("dots sit at the file's bar centers (no rest↔speech jump)", () => {
    expect(REST_DOT_X).toEqual([30.5, 43.5, 56.5]);
    // Layer origin y from waves-2.json — NOT the visual center. Eyeballing
    // 50% caused a 46px jump on every handoff.
    expect(REST_DOT_Y).toBe(24.2);
  });

  it("shimmers gently in range, never in lockstep, never ordered", () => {
    for (let t = 0; t < 5000; t += 137) {
      const s = [0, 1, 2].map((i) => restDotScale(t, i));
      for (const v of s) {
        expect(v).toBeGreaterThanOrEqual(0.85);
        expect(v).toBeLessThanOrEqual(1);
      }
      expect(new Set(s.map((v) => v.toFixed(4))).size).toBe(3);
    }
    expect(restDotScale(0, 0)).not.toBe(restDotScale(2000, 0));
  });
});

describe("zoomForFrame", () => {
  it("freezes idle, waiting, and mid-arrival smiles at exactly 1", () => {
    expect(zoomForFrame("idle", 300, true, false)).toBe(1);
    expect(zoomForFrame("idle", 171, false, false)).toBe(1);
    expect(zoomForFrame("speaking", 200, true, true)).toBe(1);
    expect(zoomForFrame("listening", 200, false, false)).toBe(1);
    expect(zoomForFrame("speaking", 200, false, false)).toBe(1);
  });

  it("zooms thinking exactly once per 89-frame revolution", () => {
    expect(zoomForFrame("thinking", 171, false, false)).toBe(1);
    expect(zoomForFrame("thinking", 171 + 89, false, false)).toBe(
      zoomForFrame("thinking", 171, false, false),
    );
    expect(zoomForFrame("thinking", 171 + 44.5, false, false)).toBeCloseTo(1.04, 5);
  });

  it("breathes listening/speaking on a 54-frame period, speaking deeper", () => {
    expect(zoomForFrame("listening", 0, true, false)).toBe(
      zoomForFrame("listening", 54, true, false),
    );
    const lPeak = zoomForFrame("listening", 27, true, false);
    const sPeak = zoomForFrame("speaking", 27, true, false);
    expect(lPeak).toBeCloseTo(1.035, 5);
    expect(sPeak).toBeCloseTo(1.08, 5);
    expect(sPeak).toBeGreaterThan(lPeak);
  });

  it("never inverts or exceeds the zoom envelope", () => {
    for (const st of ["listening", "thinking", "speaking"] as const) {
      for (let f = 0; f < 400; f += 7) {
        const z = zoomForFrame(st, f, true, false);
        expect(z).toBeGreaterThanOrEqual(1);
        expect(z).toBeLessThanOrEqual(1.08);
      }
    }
  });
});
