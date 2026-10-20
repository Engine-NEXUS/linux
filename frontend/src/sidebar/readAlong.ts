/**
 * Per-word karaoke (read-along) states for the spoken reply — on REAL
 * timings, never estimates.
 *
 * Rust's `tts:caption` event already carries per-word data for every line
 * (`CaptionLineEvent.words` + `activeWordIndex`, driven by Edge's real word
 * boundaries — or the scheduler's own reveal schedule for local engines).
 * This module turns that into word states for the renderer; there is no
 * char-proportional estimator here and there must never be one: the caption
 * scheduler is the single source of "which word is being spoken right now".
 *
 * Scheduler semantics this relies on (see captionScheduler.ts):
 * - `active` phase → `words` is the revealed prefix and `activeWordIndex`
 *   points at the word currently being revealed (the last one);
 * - `fading` phase → `words` is the full line and `activeWordIndex` is -1
 *   (everything has been said);
 * - `cleared` phase → no words at all;
 * - `text` is always `words` joined by single spaces.
 */

import type { CaptionLineEvent } from "../audio/captionScheduler";

export type ReadAlongState = "spoken" | "active" | "pending";

export interface ReadAlongWord {
  text: string;
  state: ReadAlongState;
}

/**
 * Pure: karaoke state per word. `activeWordIndex < 0` means the whole line
 * has been spoken (fading phase); an index at or past the end also reads as
 * "everything so far is spoken" rather than inventing a state.
 */
export function readAlongWords(words: string[], activeWordIndex: number): ReadAlongWord[] {
  return words.map((text, i) => ({
    text,
    state:
      activeWordIndex < 0 || i < activeWordIndex
        ? "spoken"
        : i === activeWordIndex
          ? "active"
          : "pending",
  }));
}

/** Pure: the caption's text as the renderer will lay it out (single spaces). */
export function readAlongText(words: string[]): string {
  return words.join(" ");
}

/**
 * Pure: word states for a line event, or `null` when the event carries no
 * usable word data (plain-text fallback) or its words disagree with its own
 * `text` — the scheduler guarantees they match, so a mismatch means the
 * defensive render must win over a caption that could disagree with the audio.
 */
export function readAlongFromEvent(event: CaptionLineEvent): ReadAlongWord[] | null {
  const words = event.words;
  if (!words || words.length === 0) return null;
  if (readAlongText(words) !== event.text) return null;
  return readAlongWords(words, event.activeWordIndex ?? -1);
}
