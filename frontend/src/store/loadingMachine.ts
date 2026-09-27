/**
 * Loading-indicator state machine (pure logic, injectable clock).
 *
 * Problem it solves: ~20 scattered `setLoadingVisible` writers (recorder,
 * wsBridge, orchestrator, main) with no ownership — races leave the spinner
 * stuck on, or flashing for a single frame. All writers now funnel through
 * the store setter, which runs this machine:
 *   - minimum dwell 800ms (sub-second show→hide becomes a clean 800ms pulse)
 *   - 30s failsafe (a lost hide — e.g. network hang — can never wedge it on)
 *   - no-op transitions (hide-when-hidden, show refreshes failsafe only)
 *
 * Time is a parameter (`now` ms) so unit tests run without timers.
 * The store owns the real setTimeout scheduling around `due()`.
 */

export const LOADING_MIN_DWELL_MS = 800;
// 120s: bounds true hangs (network death) while covering long tasks
// (deep PR analysis can exceed 30s). Previously: stuck forever.
export const LOADING_FAILSAFE_MS = 120_000;

export interface LoadingSnapshot {
  visible: boolean;
  /** When the current show started (null when hidden). */
  shownAt: number | null;
  /** Next deadline: failsafe while showing, delayed-hide while hiding. Null when settled. */
  pendingAt: number | null;
}

export const LOADING_HIDDEN: LoadingSnapshot = {
  visible: false,
  shownAt: null,
  pendingAt: null,
};

/** Show request: no-op if already visible (just refresh the failsafe). */
export function loadingShow(s: LoadingSnapshot, now: number): LoadingSnapshot {
  if (s.visible) {
    return { ...s, pendingAt: now + LOADING_FAILSAFE_MS };
  }
  return { visible: true, shownAt: now, pendingAt: now + LOADING_FAILSAFE_MS };
}

/**
 * Hide request: if shown for less than the minimum dwell, schedule the hide
 * for dwell-end instead of hiding immediately (prevents flashes).
 */
export function loadingHide(s: LoadingSnapshot, now: number): LoadingSnapshot {
  if (!s.visible) {
    return s;
  }
  const shownAt = s.shownAt ?? now;
  const dwellEnd = shownAt + LOADING_MIN_DWELL_MS;
  if (now < dwellEnd) {
    return { ...s, pendingAt: dwellEnd };
  }
  return { ...LOADING_HIDDEN };
}

/** True when a pending deadline (failsafe or delayed hide) has arrived. */
export function loadingDue(s: LoadingSnapshot, now: number): boolean {
  return s.pendingAt !== null && now >= s.pendingAt;
}

/** Settle a due snapshot. Any reached deadline (failsafe or delayed hide)
 * resolves to hidden — pendingAt only ever exists while visible. */
export function loadingSettle(s: LoadingSnapshot, now: number): LoadingSnapshot {
  if (!loadingDue(s, now)) {
    return s;
  }
  return { ...LOADING_HIDDEN };
}
