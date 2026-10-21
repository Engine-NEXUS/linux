/**
 * Calibration undo history — plain past-stack (task §2: `↶ Undo` only;
 * no redo in the spec). The HUD pushes the pre-change snapshot before
 * every user-driven mutation (drag release, wheel step, target default)
 * and pops it back on Undo via `calibration_apply_drafts`.
 */

export interface TargetRect {
  h: number;
  v: number;
  size: number;
}

export interface CalibrationSnapshot {
  wakeup: TargetRect;
  waves: TargetRect;
  loading: TargetRect;
}

export interface HistoryState {
  past: CalibrationSnapshot[];
  present: CalibrationSnapshot;
}

export const HISTORY_CAP = 50;

/** Clone a snapshot (break all references before pushing to history). */
export function cloneSnapshot(s: CalibrationSnapshot): CalibrationSnapshot {
  return {
    wakeup: { ...s.wakeup },
    waves: { ...s.waves },
    loading: { ...s.loading },
  };
}

/** Commit a user-driven change: push the pre-change present to `past`. */
export function commitChange(
  state: HistoryState,
  preChange: CalibrationSnapshot,
  next: CalibrationSnapshot
): HistoryState {
  const past = [...state.past, cloneSnapshot(preChange)];
  if (past.length > HISTORY_CAP) past.shift();
  return { past, present: cloneSnapshot(next) };
}

/** Undo: pop the most recent snapshot; null when nothing to undo. */
export function popUndo(state: HistoryState): {
  next: HistoryState;
  restored: CalibrationSnapshot | null;
} {
  if (state.past.length === 0) return { next: state, restored: null };
  const past = [...state.past];
  const restored = past.pop()!;
  return { next: { past, present: cloneSnapshot(restored) }, restored: restored };
}

/**
 * Coalesced commit for high-frequency nudges (key held down ≈30 ev/s).
 * The live motion stays 1:1 (every event still reaches Rust) but the undo
 * record keeps a single entry per burst: present updates, `past` is
 * untouched — Undo returns to the pre-burst snapshot in one step.
 */
export function commitCoalesced(
  state: HistoryState,
  next: CalibrationSnapshot
): HistoryState {
  return { past: state.past, present: cloneSnapshot(next) };
}

/** Nudge bursts from the same target coalesce within this window. */
export const NUDGE_COALESCE_MS = 800;
