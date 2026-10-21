import { describe, expect, it } from "vitest";

import { resolveEscDismiss } from "./sidebarStore";

/**
 * Escape dismissal hierarchy for NEXUS overlay windows (plan Phase 2, C3).
 * Locks the contract the SidebarApp/Settings keydown effects delegate to:
 * an open overlay dies first, the window itself only when nothing overlays.
 */
describe("resolveEscDismiss (Phase 2 Esc realignment)", () => {
  it("closes the overlay first when one is open (lightbox hierarchy)", () => {
    expect(resolveEscDismiss(true)).toBe("close-overlay");
  });

  it("closes the window itself when nothing overlays it", () => {
    expect(resolveEscDismiss(false)).toBe("close-window");
  });
});
