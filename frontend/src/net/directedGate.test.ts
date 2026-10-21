import { describe, expect, it, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { askDirectedGate, gateAction } from "./directedGate";

describe("gateAction", () => {
  it("proceeds when the speech is directed", () => {
    expect(gateAction({ accept: true, reason: "command" }, 0, 3)).toBe("proceed");
    expect(gateAction({ accept: true, reason: "vocative" }, 99, 3)).toBe("proceed");
  });

  it("re-listens quietly below the cap and parks at the cap", () => {
    const ignored = { accept: false, reason: "no_cues" };
    expect(gateAction(ignored, 1, 3)).toBe("relisten");
    expect(gateAction(ignored, 2, 3)).toBe("relisten");
    expect(gateAction(ignored, 3, 3)).toBe("park");
    expect(gateAction(ignored, 4, 3)).toBe("park");
  });
});

describe("askDirectedGate", () => {
  beforeEach(() => {
    // braces matter: vitest treats a RETURNED function as a teardown hook (mockReset() returns the mock)
    invokeMock.mockReset();
  });

  it("returns the Rust verdict", async () => {
    invokeMock.mockResolvedValue({ accept: false, reason: "echo" });
    const v = await askDirectedGate("stopped typing sir");
    expect(v).toEqual({ accept: false, reason: "echo" });
    expect(invokeMock).toHaveBeenCalledWith("directed_gate", { transcript: "stopped typing sir" });
  });

  it("fails OPEN when the command throws", async () => {
    invokeMock.mockImplementation(async () => {
      throw new Error("no such command");
    });
    const v = await askDirectedGate("anything");
    expect(v.accept).toBe(true);
  });

  it("fails OPEN on a malformed or missing payload", async () => {
    invokeMock.mockResolvedValue(undefined);
    expect((await askDirectedGate("x")).accept).toBe(true);
    invokeMock.mockResolvedValue({ reason: "no accept field" });
    expect((await askDirectedGate("x")).accept).toBe(true);
  });
});
