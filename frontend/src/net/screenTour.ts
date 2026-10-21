import { useAssistant } from "../store/assistant";
import { isRustTtsPlaying, setNarrationPlaying } from "../audio/ttsPlayer";
import { setTourActive } from "../stage/tourState";

/**
 * Orb/TTS side of the narrated screen tour (Rust events → store).
 *
 * Rust drives the whole choreography (`orchestrator::run_screen_tour`):
 *   screen:tour_phase{fetching}  — ack was emitted, capture + vision running
 *   screen:tour_phase{fallback}  — tour unavailable, legacy chain continues
 *   screen:tour_start            — narration begins (orb returns)
 *   screen:tour_end{reason}      — overlay cleared; `done` follows from Rust
 * The callout draw/clear events are consumed by stage/TourOverlay.tsx.
 *
 * This module fixes the one gap in the shared Ack handler for this flow: it
 * parks the orb in `speaking` and `hideOrbAfterSpeech` only hides on `idle`,
 * so without help the orb would stay up through the whole fetch.
 */

interface TourPhase {
  phase: string;
  request_id: string;
}
interface TourStart {
  request_id: string;
}
interface TourEnd {
  request_id: string;
  reason: string;
}

/** How long we wait for the ack to finish before hiding the orb anyway. */
export const FETCH_HIDE_CAP_MS = 4000;
const FETCH_HIDE_POLL_MS = 150;
const FETCH_HIDE_FIRST_MS = 300;
const CLOSE_AFTER_NON_DONE_MS = 1200;

let fetchId: string | null = null;
let tourId: string | null = null;

/** Test hook. */
export function __testResetTourFlow(): void {
  fetchId = null;
  tourId = null;
}

export function handleTourPhase(p: TourPhase): void {
  if (p.phase === "fallback") {
    // Legacy chain owns the orb now — never hide it from under that speech.
    if (fetchId === p.request_id) fetchId = null;
    return;
  }
  if (p.phase !== "fetching") return;
  fetchId = p.request_id;
  const started = Date.now();
  const tick = () => {
    if (fetchId !== p.request_id) return; // superseded: tour started / fell back / new turn
    if (isRustTtsPlaying() && Date.now() - started < FETCH_HIDE_CAP_MS) {
      setTimeout(tick, FETCH_HIDE_POLL_MS);
      return;
    }
    const s = useAssistant.getState();
    if (s.ghostActive) return; // waves stay up for the whole ghost session
    s.setState("idle");
    s.setVisible(false);
  };
  setTimeout(tick, FETCH_HIDE_FIRST_MS);
}

export function handleTourStart(p: TourStart): void {
  fetchId = null;
  tourId = p.request_id;
  setTourActive(true);
  const s = useAssistant.getState();
  s.setLoadingVisible(false);
  s.setVisible(true);
  s.setState("speaking");
  s.setTtsActive(true);
  setNarrationPlaying(true);
}

export function handleTourEnd(p: TourEnd): void {
  if (tourId !== p.request_id) return; // stale tour
  tourId = null;
  setTourActive(false);
  setNarrationPlaying(false);
  useAssistant.getState().setTtsActive(false);
  if (p.reason === "done") return; // Rust emits `done`; the shared handler resets the orb
  // Cancelled / failed: no `done` follows. Close the orb unless a newer turn
  // already took it over (state moved on) or audio is playing again.
  setTimeout(() => {
    if (tourId !== null) return;
    const s = useAssistant.getState();
    if (s.state === "speaking" && !isRustTtsPlaying()) {
      // turn-end:keep-raw tour cancelled or failed: no `done` event follows
      s.reset();
      s.setVisible(false);
    }
  }, CLOSE_AFTER_NON_DONE_MS);
}

let initialized = false;

export async function initScreenTourListener(): Promise<void> {
  if (initialized) return;
  initialized = true;
  try {
    const { listen } = await import("@tauri-apps/api/event");
    await listen<TourPhase>("screen:tour_phase", (e) => e.payload && handleTourPhase(e.payload));
    await listen<TourStart>("screen:tour_start", (e) => e.payload && handleTourStart(e.payload));
    await listen<TourEnd>("screen:tour_end", (e) => e.payload && handleTourEnd(e.payload));
  } catch {
    initialized = false; // outside Tauri (tests)
  }
}
