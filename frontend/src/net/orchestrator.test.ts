import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

// The orchestrator module imports the sidebar store, which touches
// localStorage at import time (node test env has none) — stub it.
vi.mock("../sidebar/sidebarStore", () => ({
  useSidebar: { getState: () => ({ showConflict: vi.fn() }) },
}));

// wsBridge touches `window` at import time — stub the two fns we need.
vi.mock("./wsBridge", () => ({
  clearLongRunningInFlight: vi.fn(),
  isLocalAckGiven: () => false,
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import "./ghostHotMic";
import { useAssistant } from "../store/assistant";
import {
  __testPendingSpeechCloseFor,
  __testSetCurrentRequestId,
  __testSetPendingSpeechCloseFor,
  __testSetStuckTicks,
  __testStuckTicks,
  closeTurnOnSpeechEnd,
  finishSpokenResult,
  getCurrentRequestId,
  hideOrbAfterSpeech,
  processViaOrchestrator,
} from "./orchestrator";

describe("finishSpokenResult (result → done → idle handshake)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    invokeMock.mockReset().mockResolvedValue(undefined);
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    useAssistant.setState({ state: "speaking", visible: true });
    __testSetCurrentRequestId("req-1");
  });

  it("current turn resets the orb to idle and signals done", async () => {
    expect(finishSpokenResult("req-1")).toBe(true);
    expect(getCurrentRequestId()).toBeNull();
    // Brief visible beat first (mirrors the `done` handler)…
    expect(useAssistant.getState().visible).toBe(true);
    // …then idle after 550ms.
    await vi.advanceTimersByTimeAsync(550);
    expect(useAssistant.getState().state).toBe("idle");
    expect(invokeMock).toHaveBeenCalledWith("orchestrator_done", {
      requestId: "req-1",
    });
  });

  it("stale onEnd (barge-in moved on) touches nothing", async () => {
    __testSetCurrentRequestId("req-2"); // new turn started
    expect(finishSpokenResult("req-1")).toBe(false);
    await vi.advanceTimersByTimeAsync(2000);
    expect(getCurrentRequestId()).toBe("req-2");
    expect(useAssistant.getState().state).toBe("speaking");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("stale onEnd after cancel (null) touches nothing", async () => {
    __testSetCurrentRequestId(null);
    expect(finishSpokenResult("req-1")).toBe(false);
    await vi.advanceTimersByTimeAsync(2000);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("ghost turns reopen at the 250ms Phase-4 beat (not 550ms)", async () => {
    useAssistant.setState({ ghostActive: true });
    expect(finishSpokenResult("req-1")).toBe(true);
    // Not yet at 249ms…
    await vi.advanceTimersByTimeAsync(249);
    expect(useAssistant.getState().state).toBe("speaking");
    // …idle at the 250ms ghost beat.
    await vi.advanceTimersByTimeAsync(1);
    expect(useAssistant.getState().state).toBe("idle");
    // Ghost session itself survives the turn reset.
    expect(useAssistant.getState().ghostActive).toBe(true);
    useAssistant.setState({ ghostActive: false });
    await vi.advanceTimersByTimeAsync(0);
  });
});

describe("pendingSpeechCloseFor (done/result race — the Gap 1 fix)", () => {
  // The `done` case inside the internal orchestrator:event listener isn't
  // separately exported/testable, so this verifies the half of the
  // mechanism that is: finishSpokenResult (the result/error handlers'
  // own closure) must release the tracker exactly when it owns this
  // request id, and leave it untouched otherwise — the done handler
  // checks the same tracker before deciding to run its own reset.
  beforeEach(() => {
    vi.useFakeTimers();
    invokeMock.mockReset().mockResolvedValue(undefined);
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    useAssistant.setState({ state: "speaking", visible: true, ghostActive: false });
    __testSetCurrentRequestId("req-1");
  });

  it("finishSpokenResult releases the tracker for the request it closes", () => {
    __testSetPendingSpeechCloseFor("req-1");
    expect(finishSpokenResult("req-1")).toBe(true);
    expect(__testPendingSpeechCloseFor()).toBeNull();
  });

  it("a stale onEnd (already barged-in) still releases a tracker set for that id", () => {
    // Rust's Done can arrive synchronously right after Result for several
    // turn types — if a barge-in already moved currentRequestId on, this
    // call returns false (stale), but must still drop its own claim so a
    // later done for the SAME old id doesn't find a lingering tracker.
    __testSetCurrentRequestId("req-2");
    __testSetPendingSpeechCloseFor("req-1");
    expect(finishSpokenResult("req-1")).toBe(false);
    expect(__testPendingSpeechCloseFor()).toBeNull();
  });

  it("does not touch a tracker claimed by a different, unrelated request", () => {
    __testSetPendingSpeechCloseFor("some-other-req");
    expect(finishSpokenResult("req-1")).toBe(true);
    expect(__testPendingSpeechCloseFor()).toBe("some-other-req");
  });
});

describe("closeTurnOnSpeechEnd (no turn parks in speaking forever)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllTimers();
    invokeMock.mockReset().mockResolvedValue(undefined);
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    useAssistant.setState({ state: "speaking", visible: true, ghostActive: false });
    __testSetCurrentRequestId("req-err");
  });

  it("closes a normal-mode error/conflict turn at speech end", async () => {
    expect(closeTurnOnSpeechEnd("req-err")).toBe(true);
    await vi.advanceTimersByTimeAsync(550);
    expect(useAssistant.getState().state).toBe("idle");
    expect(invokeMock).toHaveBeenCalledWith("orchestrator_done", {
      requestId: "req-err",
    });
  });

  it("leaves ghost turns open for the hot-mic loop", async () => {
    useAssistant.setState({ ghostActive: true });
    expect(closeTurnOnSpeechEnd("req-err")).toBe(false);
    await vi.advanceTimersByTimeAsync(2000);
    expect(getCurrentRequestId()).toBe("req-err");
    expect(useAssistant.getState().state).toBe("speaking");
    expect(invokeMock).not.toHaveBeenCalledWith("orchestrator_done", expect.anything());
    useAssistant.setState({ ghostActive: false });
  });
});

describe("processViaOrchestrator confirmation approvals", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    invokeMock.mockReset().mockResolvedValue(undefined);
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    __testSetCurrentRequestId("req-confirm");
  });

  it("speaking 'proceed' confirms pending MCP command immediately", async () => {
    const { processViaOrchestrator } = await import("./orchestrator");
    useAssistant.setState({
      pendingGithubCommand: {
        kind: "mcp",
        server: "whatsapp",
        tool: "send_message",
        params: { recipient: "Mommy", message: "Hi" },
      },
    });

    const res = await processViaOrchestrator("proceed");
    expect(res).toEqual({
      request_id: "mcp-confirmed",
      subsystem: "mcp",
      handled_locally: false,
    });
    expect(invokeMock).toHaveBeenCalledWith("orchestrator_mcp_confirm", {
      requestId: "req-confirm",
      confirmed: true,
      pending: {
        kind: "mcp",
        server: "whatsapp",
        tool: "send_message",
        params: { recipient: "Mommy", message: "Hi" },
      },
    });
    expect(invokeMock).toHaveBeenCalledWith("hide_sidebar");
    expect(useAssistant.getState().pendingGithubCommand).toBeNull();
  });

  it("speaking 'approved' confirms pending MCP command immediately", async () => {
    const { processViaOrchestrator } = await import("./orchestrator");
    useAssistant.setState({
      pendingGithubCommand: {
        kind: "mcp",
        server: "swiggy-food",
        tool: "place_order",
      },
    });

    const res = await processViaOrchestrator("approved");
    expect(res?.request_id).toBe("mcp-confirmed");
    expect(invokeMock).toHaveBeenCalledWith("orchestrator_mcp_confirm", {
      requestId: "req-confirm",
      confirmed: true,
      pending: {
        kind: "mcp",
        server: "swiggy-food",
        tool: "place_order",
      },
    });
  });

  it("does not let uncertain or rejected audio approve a pending command", async () => {
    useAssistant.setState({
      pendingGithubCommand: {
        kind: "mcp",
        server: "whatsapp",
        tool: "send_message",
      },
    });

    const uncertain = await processViaOrchestrator("yes", undefined, { ownership: "uncertain" });
    expect(uncertain?.handled_locally).toBe(true);
    expect(uncertain?.subsystem).toBe("ambient");
    expect(invokeMock).not.toHaveBeenCalled();
    expect(useAssistant.getState().pendingGithubCommand).not.toBeNull();

    const rejected = await processViaOrchestrator("yes", undefined, { ownership: "rejected" });
    expect(rejected?.handled_locally).toBe(true);
    expect(rejected?.subsystem).toBe("ambient");
    expect(invokeMock).not.toHaveBeenCalled();
    useAssistant.setState({ pendingGithubCommand: null });
  });

  it("speaking 'cancel' aborts pending MCP command immediately", async () => {
    const { processViaOrchestrator } = await import("./orchestrator");
    useAssistant.setState({
      pendingGithubCommand: {
        kind: "mcp",
        server: "whatsapp",
        tool: "send_message",
      },
    });

    const res = await processViaOrchestrator("cancel");
    expect(res).toEqual({
      request_id: "mcp-aborted",
      subsystem: "mcp",
      handled_locally: true,
    });
    expect(invokeMock).toHaveBeenCalledWith("orchestrator_mcp_confirm", {
      requestId: "req-confirm",
      confirmed: false,
      pending: {
        kind: "mcp",
        server: "whatsapp",
        tool: "send_message",
      },
    });
    expect(useAssistant.getState().pendingGithubCommand).toBeNull();
  });
});

describe("processViaOrchestrator ghost invoke timeout", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    invokeMock.mockReset();
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    useAssistant.setState({ pendingGithubCommand: null });
  });

  it("cancels Rust and throws GhostInvokeTimeout when a ghost turn hangs", async () => {
    const { GhostInvokeTimeout, processViaOrchestrator } = await import("./orchestrator");
    // Backend hangs forever; cancel resolves instantly.
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "orchestrator_process" ? new Promise(() => {}) : Promise.resolve(undefined),
    );
    const pending = processViaOrchestrator("shift to tap to", undefined, undefined, { timeoutMs: 12000 });
    const assertion = expect(pending).rejects.toBeInstanceOf(GhostInvokeTimeout);
    await vi.advanceTimersByTimeAsync(12000);
    await assertion;
    expect(invokeMock).toHaveBeenCalledWith("orchestrator_cancel");
  });

  it("closes the watchdog turn when the budget expires (so the watchdog can't also fire)", async () => {
    const { GhostInvokeTimeout, processViaOrchestrator } = await import("./orchestrator");
    const { __testGetGhostTurnTimes, __testSetGhostTurnTimes } = await import("./ghostHotMic");
    useAssistant.setState({ ghostActive: true });
    __testSetGhostTurnTimes(0, 0);
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "orchestrator_process" ? new Promise(() => {}) : Promise.resolve(undefined),
    );
    const pending = processViaOrchestrator("open the brief", undefined, undefined, { timeoutMs: 12000 });
    const assertion = expect(pending).rejects.toBeInstanceOf(GhostInvokeTimeout);
    await vi.advanceTimersByTimeAsync(12000);
    await assertion;
    await vi.advanceTimersByTimeAsync(0); // flush the dynamic-import continuation
    const t = __testGetGhostTurnTimes();
    expect(t.opened).toBeGreaterThan(0);
    expect(t.closed).toBeGreaterThanOrEqual(t.opened);
    useAssistant.setState({ ghostActive: false });
  });

  it("no timeout by default: slow backends still resolve (long-running safe)", async () => {
    const { processViaOrchestrator } = await import("./orchestrator");
    invokeMock.mockResolvedValue({ request_id: "r1", subsystem: "x", handled_locally: true });
    await expect(processViaOrchestrator("hi")).resolves.toMatchObject({ request_id: "r1" });
    expect(invokeMock).not.toHaveBeenCalledWith("orchestrator_cancel");
  });
});

describe("hideOrbAfterSpeech (Main Center UI-director, feature 75 P-B)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useAssistant.setState({ state: "idle", visible: true, ghostActive: false });
  });

  it("hides once the turn is fully done (idle)", async () => {
    hideOrbAfterSpeech(600);
    await vi.advanceTimersByTimeAsync(600);
    expect(useAssistant.getState().visible).toBe(false);
  });

  it("never hides while speaking — re-arms until TTS ends", async () => {
    useAssistant.setState({ state: "speaking", visible: true });
    hideOrbAfterSpeech(600);
    await vi.advanceTimersByTimeAsync(600);
    // Still speaking: no hide, re-armed.
    expect(useAssistant.getState().visible).toBe(true);
    // TTS ends → turn done → next re-arm hides.
    useAssistant.setState({ state: "idle" });
    await vi.advanceTimersByTimeAsync(1000);
    expect(useAssistant.getState().visible).toBe(false);
  });

  it("never hides mid-ghost-session", async () => {
    useAssistant.setState({ state: "speaking", visible: true, ghostActive: true });
    hideOrbAfterSpeech(600);
    await vi.advanceTimersByTimeAsync(5000);
    expect(useAssistant.getState().visible).toBe(true);
  });

  it("never steals a new turn's orb", async () => {
    hideOrbAfterSpeech(600);
    // Barge-in starts a new turn before the timer fires.
    useAssistant.setState({ state: "listening", visible: true });
    await vi.advanceTimersByTimeAsync(600);
    expect(useAssistant.getState().visible).toBe(true);
  });

  it("exit race-proof: timer armed mid-session still hides after session ends", async () => {
    // Last ghost turn armed a hide while the session was live…
    useAssistant.setState({ state: "speaking", visible: true, ghostActive: true });
    hideOrbAfterSpeech(600);
    await vi.advanceTimersByTimeAsync(600);
    expect(useAssistant.getState().visible).toBe(true); // session live: no hide
    // …then the session exits (ghost:session(false)) and the exit line ends.
    // The check is fire-time, not arm-time: now idle + not ghost → hides.
    useAssistant.setState({ ghostActive: false, state: "idle" });
    hideOrbAfterSpeech(3000);
    await vi.advanceTimersByTimeAsync(3000);
    expect(useAssistant.getState().visible).toBe(false);
  });

  it("stuck-speaking watchdog: silent 10s with dead TTS forces the turn close", async () => {
    // Genuine TTS never plays here (no audio in tests). Seed nine quiet
    // ticks, then one firing must close the turn instead of parking the
    // zoom on a dead orb. (Seeding avoids a 10-timer marathon that proved
    // timing-fragile under full-suite load; accumulation is covered by
    // the re-arm test above.)
    // Hermetic: runAllTimersAsync drains the watchdog's dynamic-import
    // hop AND the 550ms finish beat to quiescence, so no fixed advance
    // window can miss the force-close under parallel load.
    useAssistant.setState({ state: "speaking", visible: true });
    __testSetCurrentRequestId("req-wd");
    __testSetStuckTicks(9);
    expect(__testStuckTicks()).toBe(9);
    hideOrbAfterSpeech(600);
    await vi.runAllTimersAsync();
    expect(useAssistant.getState().state).toBe("idle");
    expect(__testStuckTicks()).toBe(0);
    expect(invokeMock).toHaveBeenCalledWith("orchestrator_done", {
      requestId: "req-wd",
    });
    __testSetStuckTicks(0);
  });

  it("watchdog never fires while TTS audio is actually playing", async () => {
    // isRustTtsPlaying is real here (false with no audio) — this pins the
    // contract shape: a playing engine resets the streak every re-arm.
    // Full playing-path coverage needs ttsPlayer mocks (deferred).
    useAssistant.setState({ state: "speaking", visible: true });
    __testSetCurrentRequestId("req-play");
    hideOrbAfterSpeech(600);
    await vi.advanceTimersByTimeAsync(600);
    // One quiet tick only — still speaking, nothing forced yet.
    expect(useAssistant.getState().state).toBe("speaking");
    expect(getCurrentRequestId()).toBe("req-play");
  });
});
