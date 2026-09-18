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

import { useAssistant } from "../store/assistant";
import {
  __testSetCurrentRequestId,
  finishSpokenResult,
  getCurrentRequestId,
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
