import { useAssistant } from "../store/assistant";

/**
 * TTS activity signal — the single truthful source for "is audio actually
 * playing right now", derived from the tts-started/tts-ended events that
 * playKokoro/speakCached already emit (previously observed by nobody).
 *
 * The orb animation gates on `ttsActive`, not on `state`, so the loading
 * loop never runs over silence. A 400ms grace window after tts-ended
 * covers inter-chunk gaps in sentence-streamed replies without flicker:
 * a tts-started arriving inside the window cancels the pending false.
 *
 * Note the leading edge: every speak()/speakCached() calls stopTts()
 * first, which emits a stray tts-ended before the real tts-started.
 * That's harmless here — the stray ended only arms the grace timer and
 * the started cancels it.
 */

let initialized = false;
let graceTimer: ReturnType<typeof setTimeout> | null = null;
let graceGen = 0;

function clearGrace() {
  graceGen++;
  if (graceTimer) {
    clearTimeout(graceTimer);
    graceTimer = null;
  }
}

export async function initTtsActivityListener(): Promise<void> {
  if (initialized) return;
  initialized = true;
  try {
    const { listen } = await import("@tauri-apps/api/event");
    await listen("tts-started", () => {
      clearGrace();
      useAssistant.getState().setTtsActive(true);
    });
    await listen("tts-ended", () => {
      const myGen = ++graceGen;
      if (graceTimer) clearTimeout(graceTimer);
      graceTimer = setTimeout(() => {
        graceTimer = null;
        // Only go inactive if no newer started/ended arrived meanwhile.
        if (myGen === graceGen) {
          useAssistant.getState().setTtsActive(false);
        }
      }, 400);
    });
  } catch {
    // Ignore outside Tauri (dev browser, tests).
  }
}

/** Test hook: reset module state between tests. */
export function __resetTtsActivityForTest(): void {
  initialized = false;
  clearGrace();
  try {
    useAssistant.getState().setTtsActive(false);
  } catch {
    // Store may not exist in unit tests importing this module standalone.
  }
}
