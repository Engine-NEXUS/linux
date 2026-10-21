import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const triggerFollowupListenMock = vi.fn();
vi.mock("../stage/orbRuntime", () => ({
  triggerFollowupListen: (...args: unknown[]) => triggerFollowupListenMock(...args),
}));

import { useAssistant } from "../store/assistant";
import {
  GHOST_INVOKE_BUDGET_MS,
  GHOST_SILENT_CAP,
  GHOST_STALL_AFTER_MS,
  GHOST_IGNORED_CAP,
  GHOST_IGNORED_WINDOW_MS,
  __testSetGhostTurnTimes,
  clearGhostParked,
  closeHandledLocallyTurn,
  isGhostParked,
  markGhostParked,
  recordIgnoredTurn,
  resetIgnoredTurns,
  isGhostTurnOpen,
  __testSilentMissCount,
  __testStopGhostWatchdog,
  endGhostTurn,
  maybeGhostRelisten,
  noteGhostTurnClosed,
  noteGhostTurnOpened,
  recordSilentMiss,
  resetSilentMisses,
  shouldGhostRelisten,
  shouldGhostRoute,
  startGhostWatchdog,
  stopGhostWatchdog,
} from "./ghostHotMic";

describe("ghostHotMic pure helpers", () => {
  beforeEach(() => {
    resetSilentMisses();
    useAssistant.setState({ ghostActive: false });
  });

  it("only relistens inside ghost mode", () => {
    expect(shouldGhostRelisten(true)).toBe(true);
    expect(shouldGhostRelisten(false)).toBe(false);
  });

  it("silence streak counts up and resets on speech", () => {
    expect(GHOST_SILENT_CAP).toBe(3);
    expect(recordSilentMiss()).toBe(1);
    expect(recordSilentMiss()).toBe(2);
    expect(__testSilentMissCount()).toBe(2);
    resetSilentMisses();
    expect(__testSilentMissCount()).toBe(0);
  });
});

describe("shouldGhostRoute", () => {
  it("routes open_app and whatsapp_chat to ghost runners in-session", () => {
    expect(shouldGhostRoute("open_app", true)).toBe(true);
    expect(shouldGhostRoute("whatsapp_chat", true)).toBe(true);
  });

  it("stays local outside ghost mode and for all other actions", () => {
    expect(shouldGhostRoute("open_app", false)).toBe(false);
    expect(shouldGhostRoute("whatsapp_chat", false)).toBe(false);
    expect(shouldGhostRoute("greeting", true)).toBe(false);
    expect(shouldGhostRoute("close_app", true)).toBe(false);
    expect(shouldGhostRoute("media_stop", true)).toBe(false);
    expect(shouldGhostRoute("search", true)).toBe(false);
  });
});

describe("maybeGhostRelisten", () => {
  beforeEach(() => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    invokeMock.mockReset().mockResolvedValue(false);
    useAssistant.setState({ ghostActive: false });
  });

  it("no-ops outside ghost mode without touching IPC", async () => {
    expect(await maybeGhostRelisten()).toBe(false);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("skips the loop in meeting mode", async () => {
    useAssistant.setState({ ghostActive: true });
    invokeMock.mockResolvedValue(true); // meeting_active
    expect(await maybeGhostRelisten()).toBe(false);
  });
});

describe("endGhostTurn", () => {
  beforeEach(() => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    invokeMock.mockReset().mockResolvedValue(false);
    triggerFollowupListenMock.mockReset();
    useAssistant.setState({ ghostActive: false, state: "speaking", visible: true });
  });

  it("outside ghost mode it is exactly reset() with no IPC", async () => {
    await endGhostTurn();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(triggerFollowupListenMock).not.toHaveBeenCalled();
    expect(useAssistant.getState().state).toBe("idle");
  });

  it("inside ghost mode it relistens before resetting (loop stays alive)", async () => {
    useAssistant.setState({ ghostActive: true, state: "speaking", visible: true });
    await endGhostTurn();
    expect(invokeMock).toHaveBeenCalled(); // meeting_active gate consulted
    expect(triggerFollowupListenMock).toHaveBeenCalledTimes(1);
    expect(useAssistant.getState().state).toBe("idle");
    // Session survives the turn — reset() must never clear ghostActive.
    expect(useAssistant.getState().ghostActive).toBe(true);
  });
});

describe("ignored-turn counter (TV / side talk must not silently kill the hot mic)", () => {
  beforeEach(() => {
    resetIgnoredTurns();
    clearGhostParked();
  });

  it("counts consecutive ignored turns inside the window", () => {
    expect(recordIgnoredTurn(1_000)).toBe(1);
    expect(recordIgnoredTurn(5_000)).toBe(2);
    expect(recordIgnoredTurn(9_000)).toBe(3);
    expect(GHOST_IGNORED_CAP).toBeGreaterThanOrEqual(3);
  });

  it("forgets ignored turns older than the window (a TV an hour ago is not a streak)", () => {
    recordIgnoredTurn(0);
    recordIgnoredTurn(1_000);
    expect(recordIgnoredTurn(1_000 + GHOST_IGNORED_WINDOW_MS + 1)).toBe(1);
  });

  it("a heard command resets the streak", () => {
    recordIgnoredTurn(0);
    recordIgnoredTurn(1_000);
    resetIgnoredTurns();
    expect(recordIgnoredTurn(2_000)).toBe(1);
  });

  it("ignored turns never touch the silent-capture counter (separate causes)", () => {
    resetSilentMisses();
    recordIgnoredTurn(0);
    recordIgnoredTurn(1);
    expect(__testSilentMissCount()).toBe(0);
  });

  it("tracks the parked state for the orb and for re-arming", () => {
    expect(isGhostParked()).toBe(false);
    markGhostParked();
    expect(isGhostParked()).toBe(true);
    clearGhostParked();
    expect(isGhostParked()).toBe(false);
  });
});

describe("closeHandledLocallyTurn (the 28 s freeze after an unrecognised sentence)", () => {
  beforeEach(() => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    invokeMock.mockReset().mockResolvedValue(false);
    triggerFollowupListenMock.mockReset();
    useAssistant.setState({ ghostActive: true, state: "thinking", visible: true });
    __testSetGhostTurnTimes(0, 0);
  });

  it("an open turn that no event closed is closed at once and the mic re-arms", async () => {
    noteGhostTurnOpened(); // transcript entered the pipeline
    expect(isGhostTurnOpen()).toBe(true);
    // Rust answered handled_locally with NO result/done/error event (ambient drop / Unknown).
    const closed = await closeHandledLocallyTurn();
    expect(closed).toBe(true);
    expect(triggerFollowupListenMock).toHaveBeenCalledTimes(1);
    expect(useAssistant.getState().state).toBe("idle");
    expect(isGhostTurnOpen()).toBe(false);
    expect(useAssistant.getState().ghostActive).toBe(true);
  });

  it("does NOT touch a turn a result/done/error event already closed (it is speaking; its own end relistens)", async () => {
    noteGhostTurnOpened();
    noteGhostTurnClosed(); // the result handler ran before the invoke returned
    const closed = await closeHandledLocallyTurn();
    expect(closed).toBe(false);
    expect(triggerFollowupListenMock).not.toHaveBeenCalled();
    expect(useAssistant.getState().state).toBe("thinking");
  });

  it("does nothing outside ghost mode (the 8 s non-ghost guard owns that case)", async () => {
    useAssistant.setState({ ghostActive: false });
    noteGhostTurnOpened();
    expect(await closeHandledLocallyTurn()).toBe(false);
    expect(triggerFollowupListenMock).not.toHaveBeenCalled();
  });
});

/** Advance fake time, then let the guarded relisten's dynamic imports finish (no leakage into the next test). */
async function settle(ms: number): Promise<void> {
  await vi.advanceTimersByTimeAsync(ms);
  await vi.dynamicImportSettled();
  await vi.advanceTimersByTimeAsync(0);
}

describe("ghost stall watchdog (hung turns re-arm the mic)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    // The watchdog re-arms through the guarded maybeGhostRelisten (meeting probe first).
    invokeMock.mockReset().mockResolvedValue(false);
    triggerFollowupListenMock.mockReset();
    __testStopGhostWatchdog();
    useAssistant.setState({ ghostActive: false });
  });

  afterEach(() => {
    __testStopGhostWatchdog();
    vi.useRealTimers();
  });

  it("re-arms once when a turn stays open past the stall budget", async () => {
    useAssistant.setState({ ghostActive: true });
    __testSetGhostTurnTimes(Date.now() - GHOST_STALL_AFTER_MS - 5000, 0);
    startGhostWatchdog();
    await settle(5000);
    expect(triggerFollowupListenMock).toHaveBeenCalledTimes(1);
    // Single re-arm per stall: further ticks stay quiet until a new turn.
    await settle(20000);
    expect(triggerFollowupListenMock).toHaveBeenCalledTimes(1);
  });

  it("stays quiet for fresh turns, closed turns, and outside ghost mode", async () => {
    startGhostWatchdog();
    // Fresh turn (just opened).
    useAssistant.setState({ ghostActive: true });
    noteGhostTurnOpened();
    await settle(GHOST_STALL_AFTER_MS - 4000 - 1000); // inside budget (poll is 4s)
    expect(triggerFollowupListenMock).not.toHaveBeenCalled();
    // Closed turn.
    noteGhostTurnClosed();
    __testSetGhostTurnTimes(Date.now() - 60000, Date.now() - 50000);
    await settle(20000);
    expect(triggerFollowupListenMock).not.toHaveBeenCalled();
    // Outside ghost mode entirely.
    useAssistant.setState({ ghostActive: false });
    __testSetGhostTurnTimes(Date.now() - 60000, 0);
    await settle(20000);
    expect(triggerFollowupListenMock).not.toHaveBeenCalled();
  });

  it("stall threshold stays well above the invoke budget (timeout owns recovery, watchdog is the backstop)", () => {
    // Regression: at 12s (== budget) the watchdog raced the timeout recovery and
    // re-armed the mic while the "Didn't catch that" fallback was still speaking.
    expect(GHOST_STALL_AFTER_MS).toBeGreaterThanOrEqual(GHOST_INVOKE_BUDGET_MS + 5000);
  });

  it("does not re-arm during a meeting (guarded path, not a raw trigger)", async () => {
    useAssistant.setState({ ghostActive: true });
    invokeMock.mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "meeting_active" ? true : false),
    );
    __testSetGhostTurnTimes(Date.now() - GHOST_STALL_AFTER_MS - 5000, 0);
    startGhostWatchdog();
    await settle(5000);
    expect(triggerFollowupListenMock).not.toHaveBeenCalled();
  });

  it("stop prevents all future firing", async () => {
    useAssistant.setState({ ghostActive: true });
    __testSetGhostTurnTimes(Date.now() - 60000, 0);
    startGhostWatchdog();
    stopGhostWatchdog();
    await settle(20000);
    expect(triggerFollowupListenMock).not.toHaveBeenCalled();
  });
});
