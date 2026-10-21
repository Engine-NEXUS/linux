/**
 * Duplicate-listen guard for the follow-up / hot-mic re-arm path.
 *
 * Several independent paths re-open the mic (end-of-turn relisten, the stall
 * watchdog, the Rust-side `ghost:relisten` poke, the silent-miss path). When
 * two of them land close together the second call restarts a capture that is
 * already running (Rust clears the buffer and resets VAD state) and the
 * wake handler runs twice. One guard at the shared choke point
 * (`triggerFollowupListen`) covers every caller.
 *
 * Pure so it is unit-testable without Tauri.
 */

/** Two re-arms of the same origin closer than this are one request. */
export const LISTEN_DEDUPE_WINDOW_MS = 1200;

/**
 * True when this re-arm duplicates the previous one: same origin and inside
 * the window. A different origin is never a duplicate — an explicit user
 * turn must always be able to take over from a hot-mic re-arm.
 */
export function isDuplicateListen(
  now: number,
  lastAt: number,
  lastOrigin: string | null,
  origin: string,
  windowMs: number = LISTEN_DEDUPE_WINDOW_MS,
): boolean {
  if (lastAt <= 0 || lastOrigin === null) return false;
  if (lastOrigin !== origin) return false;
  const gap = now - lastAt;
  return gap >= 0 && gap < windowMs;
}
