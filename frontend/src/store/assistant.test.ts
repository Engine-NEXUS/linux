import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { useAssistant } from "./assistant";

describe("setGhostActive (P-C ghost exits, feature 75)", () => {
  it("enter shows the orb — visibility is the listener's job", () => {
    useAssistant.setState({ ghostActive: false, visible: false });
    useAssistant.getState().setGhostActive(true);
    expect(useAssistant.getState().ghostActive).toBe(true);
    // Enter ghost mode: orb should become visible
    expect(useAssistant.getState().visible).toBe(true);
  });

  it("exit lands on the visible normal orb — no longer invisible", () => {
    useAssistant.setState({ ghostActive: true, visible: true });
    useAssistant.getState().setGhostActive(false);
    expect(useAssistant.getState().ghostActive).toBe(false);
    // Exit should land on visible normal-mode orb (idle smile)
    expect(useAssistant.getState().visible).toBe(true);
  });
});
