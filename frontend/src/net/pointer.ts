/**
 * Screen pointer event bridge — listens to Rust's `pointer:show` /
 * `pointer:hide` channel (emitted by pointer.rs, NOT the orchestrator
 * channel, so the marker lifecycle stays independent of turn state).
 */

export interface PointerShowPayload {
  /** Physical screen pixels on the captured monitor. */
  x: number;
  y: number;
  label: string;
  /** Dwell time in ms before the frontend starts fading. */
  dwell_ms: number;
}

function isTauri(): boolean {
  return typeof (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== "undefined";
}

/** Subscribe to pointer events. Resolves to an unlisten function. */
export async function watchPointer(
  onShow: (p: PointerShowPayload) => void,
  onHide: () => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  const unshow = await listen<PointerShowPayload>("pointer:show", (event) => {
    onShow(event.payload);
  });
  const unhide = await listen("pointer:hide", () => {
    onHide();
  });
  return () => {
    unshow();
    unhide();
  };
}
