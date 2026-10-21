import { afterEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_HOTKEY,
  LINUX_HOTKEY,
  detectLinux,
  globalHotkeyHint,
  globalHotkeyLabel,
  isLinuxRuntime,
  markPlatform,
} from "./platform";

function stubNavigator(userAgent: string, platform = ""): void {
  vi.stubGlobal("navigator", { userAgent, platform });
}

describe("detectLinux", () => {
  it("detects Linux from the user agent", () => {
    expect(detectLinux("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15")).toBe(true);
  });

  it("detects Linux from the platform string when the agent is generic", () => {
    expect(detectLinux("Mozilla/5.0", "Linux x86_64")).toBe(true);
  });

  it("is false on Windows", () => {
    expect(detectLinux("Mozilla/5.0 (Windows NT 10.0; Win64; x64)", "Win32")).toBe(false);
  });

  it("is false on macOS", () => {
    expect(detectLinux("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", "MacIntel")).toBe(false);
  });

  it("is false when nothing is known", () => {
    expect(detectLinux()).toBe(false);
    expect(detectLinux(undefined, undefined)).toBe(false);
  });
});

describe("isLinuxRuntime", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("detects Linux from the user agent", () => {
    stubNavigator("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15");
    expect(isLinuxRuntime()).toBe(true);
  });

  it("detects Linux from navigator.platform when the agent is generic", () => {
    stubNavigator("Mozilla/5.0", "Linux x86_64");
    expect(isLinuxRuntime()).toBe(true);
  });

  it("is false on Windows", () => {
    stubNavigator("Mozilla/5.0 (Windows NT 10.0; Win64; x64)", "Win32");
    expect(isLinuxRuntime()).toBe(false);
  });

  it("is false on macOS", () => {
    stubNavigator("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", "MacIntel");
    expect(isLinuxRuntime()).toBe(false);
  });
});

describe("global hotkey label", () => {
  it("names the built-in chord off Linux", () => {
    expect(globalHotkeyLabel(false)).toBe(DEFAULT_HOTKEY);
  });

  it("names the desktop-bound chord on Linux", () => {
    expect(globalHotkeyLabel(true)).toBe(LINUX_HOTKEY);
  });
});

describe("global hotkey hint", () => {
  it("keeps the Windows wording unchanged off Linux", () => {
    expect(globalHotkeyHint("Press this key combo to wake NEXUS", false)).toBe(
      "Press this key combo to wake NEXUS",
    );
  });

  it("explains the desktop binding on Linux", () => {
    const hint = globalHotkeyHint("Press this key combo to wake NEXUS", true);
    expect(hint).toContain(LINUX_HOTKEY);
    expect(hint).toContain("nexus --wake");
  });
});

describe("markPlatform", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("tags the body on Linux", () => {
    stubNavigator("Mozilla/5.0 (X11; Linux x86_64)");
    const body = { dataset: {} as Record<string, string> };
    vi.stubGlobal("document", { body });
    markPlatform();
    expect(body.dataset.platform).toBe("linux");
  });

  it("leaves the body untagged off Linux", () => {
    stubNavigator("Mozilla/5.0 (Windows NT 10.0; Win64; x64)", "Win32");
    const body = { dataset: {} as Record<string, string> };
    vi.stubGlobal("document", { body });
    markPlatform();
    expect(body.dataset.platform).toBeUndefined();
  });
});
