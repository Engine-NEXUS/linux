import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();
const emitMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  emit: (...args: unknown[]) => emitMock(...args),
}));

import { emitLogged, invokeLogged } from "./ipc";

describe("invokeLogged / emitLogged (log-completeness P2)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("passes through successful invokes silently", async () => {
    invokeMock.mockResolvedValue("ok");
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    await expect(invokeLogged("ping", { a: 1 })).resolves.toBe("ok");
    expect(invokeMock).toHaveBeenCalledWith("ping", { a: 1 });
    expect(warn).not.toHaveBeenCalled();
    warn.mockRestore();
  });

  it("warns AND rethrows on invoke failure (no silent breakage)", async () => {
    invokeMock.mockRejectedValue(new Error("no window"));
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    await expect(invokeLogged("abort_x")).rejects.toThrow("no window");
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0][0])).toContain("abort_x");
    warn.mockRestore();
  });

  it("warns AND rethrows on emit failure (lost events surface)", async () => {
    emitMock.mockRejectedValue(new Error("closed"));
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    await expect(emitLogged("stage:annotation_commit", { n: 1 })).rejects.toThrow("closed");
    expect(warn).toHaveBeenCalledTimes(1);
    warn.mockRestore();
  });
});
