/**
 * Orb idle watchdog - the "idle means hidden" invariant.
 *
 * Before this existed the orb was hidden only by one-shot timers armed by
 * SOME turn types (`hideOrbAfterSpeech`). Turns that never armed one (the
 * boot briefing, sentinel alerts, a result with no ack, a turn Rust dropped
 * silently, a lost transcript) left the orb on screen forever, because
 * `reset()` deliberately keeps `visible` unchanged.
 *
 * Pure decision function + an installer. The decision is injectable-clock
 * testable and never touches the DOM or the store itself.
 */

export type OrbState = "idle" | "listening" | "thinking" | "speaking";

export interface OrbSnapshot {
  visible: boolean;
  state: OrbState;
  ghostActive: boolean;
  captionActive: boolean;
  ttsActive: boolean;
  awaitingInput: boolean;
  pendingGithubCommand: boolean;
  loadingVisible: boolean;
  calibrating: boolean;
}

export interface WatchdogState {
  idleSince: number | null;
  busySince: number | null;
  busyState: OrbState | null;
}

export type WatchdogAction = "none" | "hide" | "reset_and_hide";

/** Idle + visible this long with nothing pending -> hide. */
export const IDLE_HIDE_MS = 2500;
/** `thinking` this long with no loading indicator and no ghost -> the turn was lost. */
export const THINKING_STUCK_MS = 60_000;
/** `listening` this long outside ghost mode -> the turn was lost (Rust ends real turns in <= 10 s). */
export const LISTENING_STUCK_MS = 30_000;

export const INITIAL_WATCHDOG_STATE: WatchdogState = {
  idleSince: null,
  busySince: null,
  busyState: null,
};

/** True when nothing on screen or in flight justifies an idle orb staying visible. */
export function isIdleEligible(s: OrbSnapshot): boolean {
  return (
    s.visible &&
    s.state === "idle" &&
    !s.ghostActive &&
    !s.captionActive &&
    !s.ttsActive &&
    !s.awaitingInput &&
    !s.pendingGithubCommand &&
    !s.loadingVisible &&
    !s.calibrating
  );
}

/** True when the orb is in a turn state that should have ended by now if nothing is pending. */
export function isBusyWatchable(s: OrbSnapshot): boolean {
  return (
    s.visible &&
    (s.state === "thinking" || s.state === "listening") &&
    !s.ghostActive &&
    !s.loadingVisible &&
    !s.awaitingInput &&
    !s.pendingGithubCommand &&
    !s.calibrating
  );
}

/** One watchdog tick. Returns the next bookkeeping state and the action to take. */
export function stepWatchdog(
  prev: WatchdogState,
  snap: OrbSnapshot,
  now: number,
): { next: WatchdogState; action: WatchdogAction } {
  let idleSince = prev.idleSince;
  let busySince = prev.busySince;
  let busyState = prev.busyState;

  if (isIdleEligible(snap)) {
    if (idleSince === null) idleSince = now;
    busySince = null;
    busyState = null;
    if (now - idleSince >= IDLE_HIDE_MS) {
      return {
        next: { idleSince: null, busySince: null, busyState: null },
        action: "hide",
      };
    }
    return { next: { idleSince, busySince, busyState }, action: "none" };
  }

  idleSince = null;

  if (isBusyWatchable(snap)) {
    if (busyState !== snap.state || busySince === null) {
      busyState = snap.state;
      busySince = now;
    }
    const limit = snap.state === "thinking" ? THINKING_STUCK_MS : LISTENING_STUCK_MS;
    if (now - busySince >= limit) {
      return {
        next: { idleSince: null, busySince: null, busyState: null },
        action: "reset_and_hide",
      };
    }
    return { next: { idleSince, busySince, busyState }, action: "none" };
  }

  return {
    next: { idleSince: null, busySince: null, busyState: null },
    action: "none",
  };
}
