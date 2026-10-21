/**
 * Ghost orb policy: when the orb is on screen during a Ghost session.
 *
 * Ghost keeps the hot mic open for the whole session, so the orb's own state
 * sits in `listening` almost permanently; "listening" alone must therefore not
 * keep it visible. The orb shows while there is something to show (NEXUS is
 * thinking, speaking, a caption/loader is up, a question is waiting, or the
 * mic hears a voice), lingers briefly afterwards, then hides. The session, the
 * hot mic and Esc are untouched: this only decides visibility.
 *
 * Pure and clock-injected: no store, no DOM, no timers.
 */

import type { OrbState } from "./orbIdleWatchdog";

/** How long the orb stays after the last activity. */
export const GHOST_ORB_LINGER_MS = 4000;
/** `audio:level` (0..1, rms 0.01 -> ~0, 0.10 -> 1) at or above this counts as a voice, not room noise. */
export const GHOST_VOICE_LEVEL = 0.35;

export interface GhostOrbInputs {
  state: OrbState;
  micLevel: number;
  captionActive: boolean;
  ttsActive: boolean;
  loadingVisible: boolean;
  awaitingInput: boolean;
  /** The hot mic gave up after repeated ambient turns and is waiting for a re-wake. */
  parked: boolean;
}

export interface GhostOrbState {
  /** Epoch ms until which the orb stays shown without fresh activity. */
  shownUntil: number;
}

/** Something worth looking at is happening right now. */
export function ghostOrbBusy(i: GhostOrbInputs): boolean {
  return (
    i.state === "thinking" ||
    i.state === "speaking" ||
    i.captionActive ||
    i.ttsActive ||
    i.loadingVisible ||
    i.awaitingInput ||
    (i.state === "listening" && i.micLevel >= GHOST_VOICE_LEVEL)
  );
}

/**
 * One policy step. A parked mic keeps the orb up (it is the only sign that
 * NEXUS stopped listening); the caller renders it in its paused look.
 */
export function stepGhostOrb(
  prev: GhostOrbState,
  i: GhostOrbInputs,
  now: number,
): { shown: boolean; next: GhostOrbState } {
  if (i.parked || ghostOrbBusy(i)) {
    return { shown: true, next: { shownUntil: now + GHOST_ORB_LINGER_MS } };
  }
  return { shown: now < prev.shownUntil, next: prev };
}

/** A session that just started shows the orb for one linger so entry is visible. */
export function initialGhostOrbState(now: number): GhostOrbState {
  return { shownUntil: now + GHOST_ORB_LINGER_MS };
}
