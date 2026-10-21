/**
 * Mic-holder audit (approach C): every frontend getUserMedia opener
 * records itself here, so an empty turn can answer "who was holding the
 * mic?" instead of guessing. Intel SST starves cpal while a WebView2
 * stream is live — a stale open stream is the prime suspect that leaves
 * no other trace.
 *
 * Pure registry (unit-tested); the openers call micAcquire/micRelease.
 * Exclusive-open guard lives in the shared getter: an already-live
 * stream is reused with a warning, never double-opened.
 */

export interface MicHolder {
  reason: string;
  since: number;
  releasedAt: number | null;
}

const holders = new Map<string, MicHolder>();
let seq = 0;

/** Record a mic acquisition. Returns a holder id for micRelease(). */
export function micAcquire(reason: string): string {
  seq += 1;
  const id = `${reason}#${seq}`;
  holders.set(id, { reason, since: Date.now(), releasedAt: null });
  return id;
}

/** Record a release. Unknown ids are ignored (defensive). */
export function micRelease(id: string): void {
  const h = holders.get(id);
  if (h && h.releasedAt === null) {
    h.releasedAt = Date.now();
  }
}

/** Currently unreleased holders (the mic-hog suspects). */
export function micLiveHolders(): MicHolder[] {
  return [...holders.values()].filter((h) => h.releasedAt === null);
}

/** All holders ever recorded (bounded: keeps the last 20). */
export function micAllHolders(): MicHolder[] {
  const all = [...holders.values()];
  return all.slice(Math.max(0, all.length - 20));
}

/** True when nobody holds the mic — cpal owns it alone. */
export function micFree(): boolean {
  return micLiveHolders().length === 0;
}

/** Compact one-line summary for debug_trace on empty turns. */
export function micHoldersSummary(): string {
  const live = micLiveHolders();
  if (live.length === 0) return "holders=none(free)";
  return (
    "holders=" +
    live.map((h) => `${h.reason}@${Math.round((Date.now() - h.since) / 1000)}s`).join(",")
  );
}

/** Test hook: clear all state. */
export function __testResetMicHolders(): void {
  holders.clear();
  seq = 0;
}
