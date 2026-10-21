import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => {}),
}));

vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(),
}));

import { useAssistant } from "../store/assistant";
import { isRustTtsPlaying, speak, speakCached, stopTts } from "./ttsPlayer";

describe("speak()/speakCached() force the orb visible before any sound plays", () => {
  // Gap 2 of the caption/orb/sound sync audit: these are the only two
  // functions that ever invoke a Rust TTS command, so this is the single
  // enforcement point for every one of the 30+ call sites across the
  // frontend — none of them need to remember this themselves any more.
  beforeEach(() => {
    vi.clearAllMocks();
    useAssistant.setState({ visible: false, state: "idle" });
  });

  it("speak() shows the orb and marks it speaking before invoking", async () => {
    await speak("hello");
    expect(useAssistant.getState().visible).toBe(true);
    expect(useAssistant.getState().state).toBe("speaking");
  });

  it("speakCached() shows the orb and marks it speaking before invoking", async () => {
    await speakCached("hello");
    expect(useAssistant.getState().visible).toBe(true);
    expect(useAssistant.getState().state).toBe("speaking");
  });
});

describe("stopTts idempotency (Alexa-style barge-in)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("stops cleanly when nothing is playing (unconditional wake stop)", () => {
    expect(isRustTtsPlaying()).toBe(false);
    expect(() => {
      stopTts();
      stopTts();
    }).not.toThrow();
    expect(isRustTtsPlaying()).toBe(false);
  });

  it("stays stopped after repeated calls (no stuck-playing flag)", () => {
    stopTts();
    stopTts();
    stopTts();
    expect(isRustTtsPlaying()).toBe(false);
  });
});

describe("playing flag belongs to the newest utterance (stuck-speaking watchdog regression)", () => {
  // Live log 2026-10-17 02:20: the one-moment ack was superseded by the 18 s answer; the ack's invoke
  // resolved a beat later and its `finally` cleared the flag, so the watchdog saw "no audio" and closed
  // the turn 10 s into the answer while the voice kept talking.
  beforeEach(() => {
    vi.clearAllMocks();
    useAssistant.setState({ visible: false, state: "idle" });
  });

  it("a superseded speakCached() finishing late does not clear the flag of the newer reply", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    let releaseAck: () => void = () => {};
    vi.mocked(invoke).mockImplementation((async (cmd: string) => {
      if (cmd === "speak_cached") return new Promise<void>((r) => { releaseAck = r; });
      if (cmd === "speak_text") return new Promise<void>(() => {}); // long reply: still playing
      return undefined;
    }) as never);

    void speakCached("One moment sir.");
    await vi.waitFor(() => expect(isRustTtsPlaying()).toBe(true));
    void speak("A long answer that keeps playing for many seconds.");
    await vi.waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("speak_text", expect.anything()));
    expect(isRustTtsPlaying()).toBe(true);

    releaseAck(); // the superseded ack resolves late
    await new Promise((r) => setTimeout(r, 20));
    expect(isRustTtsPlaying()).toBe(true);
  });
});
