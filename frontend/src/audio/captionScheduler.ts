import { useAssistant } from "../store/assistant";

// Response caption scheduler (plan Phase 3) — word-by-word reveal driven
// by Rust's `tts:caption` event, anchored to performance.now() the moment
// the first chunk of an utterance arrives. A streamed multi-chunk reply
// emits several `tts:caption` events (one per chunk); each chunk's word
// `start_ms` values already carry every prior chunk's cumulative audio
// duration baked in (see tts.rs's `play_audio_streamed`), so this module
// just keeps scheduling against the SAME anchor across chunks — it never
// needs to know chunking happened.

export interface CaptionWord {
  text: string;
  start_ms: number;
  duration_ms: number;
}

export interface CaptionTrack {
  words: CaptionWord[];
  total_ms: number;
  estimated: boolean;
  /** TTS amplitude envelope (sub-phase C2) — real voice loudness per
   *  `frame_ms`-spaced frame, 0..1 normalized to the loudest frame in the
   *  track. Empty when Rust couldn't compute one (degrade silently —
   *  the orb's speaking "beat" falls back to the mic-proxy level). */
  envelope: number[];
  frame_ms: number;
  /** Same cumulative-offset convention as each word's `start_ms` — rides
   *  the SAME absolute utterance timeline across streamed chunks. */
  envelope_start_ms: number;
}

export interface CaptionLineEvent {
  text: string;
  previousText?: string;
  phase: "active" | "fading" | "cleared";
  words?: string[];
  activeWordIndex?: number;
}

export type LineListener = (line: CaptionLineEvent, done: boolean) => void;

interface EnvelopeSegment {
  startMs: number;
  frameMs: number;
  values: number[];
}

type Listener = (revealed: string[], done: boolean) => void;

let anchorMs: number | null = null;
let timers: ReturnType<typeof setTimeout>[] = [];
let doneTimer: ReturnType<typeof setTimeout> | null = null;
let revealed: string[] = [];
let segments: EnvelopeSegment[] = [];
let suppressNext = false;
let latestScheduledEndMs = 0;
let currentLineSeq = 0;
let activeLineSeq = 0;
/**
 * A line's clear-timer can only tell whether it's the last line it KNOWS
 * about (`line.end_ms >= latestScheduledEndMs`) — not whether a later
 * streamed chunk is still mid-flight (synthesis/network delay) and about
 * to arrive with more. Committing `setCaptionActive(false)`/`done: true`
 * immediately on that guess closes the caption early whenever a later
 * chunk is simply running a little behind. This grace timer defers that
 * commit briefly; any subsequent chunk's `scheduleChunk()` call cancels it,
 * since that chunk schedules its own, later close.
 */
let pendingCloseTimer: ReturnType<typeof setTimeout> | null = null;
const UTTERANCE_END_GRACE_MS = 500;
const listeners = new Set<Listener>();
const lineListeners = new Set<LineListener>();

export function suppressNextCaption(): void {
  suppressNext = true;
}

function notify(done: boolean): void {
  const snapshot = revealed.slice();
  listeners.forEach((l) => l(snapshot, done));
}

function notifyLine(line: CaptionLineEvent, done: boolean): void {
  lineListeners.forEach((l) => l(line, done));
}

/** Subscribe to word reveal updates. Returns an unsubscribe function. */
export function onCaptionUpdate(fn: Listener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

/** Subscribe to line-by-line replacement updates. Returns an unsubscribe function. */
export function onCaptionLineUpdate(fn: LineListener): () => void {
  lineListeners.add(fn);
  return () => lineListeners.delete(fn);
}

/** Barge-in: cancel every pending reveal and clear the caption immediately. */
export function clearCaptionSchedule(): void {
  timers.forEach(clearTimeout);
  timers = [];
  if (doneTimer) {
    clearTimeout(doneTimer);
    doneTimer = null;
  }
  if (pendingCloseTimer) {
    clearTimeout(pendingCloseTimer);
    pendingCloseTimer = null;
  }
  anchorMs = null;
  revealed = [];
  segments = [];
  suppressNext = false;
  latestScheduledEndMs = 0;
  currentLineSeq = 0;
  activeLineSeq = 0;
  useAssistant.getState().setCaptionActive(false);
  notify(true);
  notifyLine({ text: "", phase: "cleared" }, true);
}

/**
 * Current real TTS voice amplitude, 0..1, interpolated between the two
 * nearest envelope frames — or `null` when no envelope has arrived yet
 * (no utterance speaking, or this chunk's engine couldn't produce one).
 * Callers (VoiceOrb.tsx) fall back to the mic-proxy level on `null`.
 */
export function getEnvelopeLevel(): number | null {
  if (anchorMs === null || !segments.length) return null;
  const elapsed = performance.now() - anchorMs;
  for (const seg of segments) {
    if (!seg.values.length) continue;
    const segEnd = seg.startMs + seg.values.length * seg.frameMs;
    if (elapsed >= seg.startMs && elapsed < segEnd) {
      const idxF = (elapsed - seg.startMs) / seg.frameMs;
      const i0 = Math.max(0, Math.floor(idxF));
      const i1 = Math.min(seg.values.length - 1, i0 + 1);
      const frac = idxF - i0;
      return seg.values[i0] + (seg.values[i1] - seg.values[i0]) * frac;
    }
  }
  // Elapsed is past every known segment (more audio still synthesizing,
  // or this was the final chunk and we're in its tail) — hold the last
  // known value instead of snapping to 0 (a flat silent gap would look
  // like the beat died rather than just outrunning known data).
  const last = segments[segments.length - 1];
  return last.values.length ? last.values[last.values.length - 1] : null;
}

function unescapeXml(str: string): string {
  return str
    .replace(/&apos;/g, "'")
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">");
}

export function partitionIntoLines(words: CaptionWord[]): { text: string; start_ms: number; end_ms: number; words: CaptionWord[] }[] {
  if (!words.length) return [];
  const lines: { text: string; start_ms: number; end_ms: number; words: CaptionWord[] }[] = [];
  let current: CaptionWord[] = [];

  for (let i = 0; i < words.length; i++) {
    const w = words[i];
    current.push(w);

    const isLast = i === words.length - 1;
    const clean = unescapeXml(w.text).trim();
    const isSentenceBreak = /[.?!]["']?$/.test(clean);
    const nextGap = !isLast && (words[i + 1].start_ms - (w.start_ms + w.duration_ms) > 650);
    const isWordCap = current.length >= 10;

    if (isLast || isSentenceBreak || nextGap || isWordCap) {
      lines.push({
        text: current.map((c) => unescapeXml(c.text)).join(" "),
        start_ms: current[0].start_ms,
        end_ms: current[current.length - 1].start_ms + current[current.length - 1].duration_ms,
        words: current.slice(),
      });
      current = [];
    }
  }
  return lines;
}

function scheduleChunk(track: CaptionTrack): void {
  // A new chunk just arrived — any earlier line's "this might be the end"
  // grace timer is now known to be wrong and must not fire. This chunk's
  // own lines will schedule their own, later close.
  if (pendingCloseTimer) {
    clearTimeout(pendingCloseTimer);
    pendingCloseTimer = null;
  }
  const isFirstChunk = anchorMs === null;
  if (isFirstChunk) {
    anchorMs = performance.now();
    revealed = [];
    segments = [];
    latestScheduledEndMs = 0;
  }
  if (track.envelope?.length && track.frame_ms > 0) {
    segments.push({ startMs: track.envelope_start_ms ?? 0, frameMs: track.frame_ms, values: track.envelope });
  }
  if (isFirstChunk && track.estimated) {
    // Kokoro/local synthesis has no real per-word boundary data — every
    // word's start_ms/duration_ms is evenly estimated from total_ms, which
    // can drift from the actual decoded audio if Kokoro's real pacing
    // isn't perfectly even. Nothing previously surfaced this anywhere, so
    // a live drift report had no way to be diagnosed. Logged once per
    // utterance (not per chunk) to stay quiet in normal use.
    console.debug("[CAPTION] estimated timing (local TTS engine, no real word-boundary data)");
  }
  if (suppressNext) {
    suppressNext = false;
    return;
  }
  if (!track.words.length) return;
  const anchor = anchorMs as number;

  // Legacy word-by-word reveal (for any word listeners)
  for (const w of track.words) {
    const delay = Math.max(0, w.start_ms - (performance.now() - anchor));
    timers.push(
      setTimeout(() => {
        revealed.push(w.text);
        notify(false);
      }, delay),
    );
  }

  // Modern Line-by-line progressive stream schedule (strict spoken words only + 2s dwell)
  const lines = partitionIntoLines(track.words);
  for (const l of lines) {
    if (l.end_ms > latestScheduledEndMs) {
      latestScheduledEndMs = l.end_ms;
    }
  }

  for (let li = 0; li < lines.length; li++) {
    const line = lines[li];
    const prevLineText = li > 0 ? lines[li - 1].text : undefined;
    const isLastInChunk = li === lines.length - 1;
    const lineSeq = ++currentLineSeq;
    const now = performance.now();
    const startDelay = Math.max(0, line.start_ms - (now - anchor));

    const wordCount = line.words.length;
    const spokenDuration = Math.max(300, line.end_ms - line.start_ms);
    // User choice: 2.0s reading dwell for final sentence, 1.6s for intermediate
    const minDwell = isLastInChunk ? Math.max(2000, wordCount * 300) : Math.max(1600, wordCount * 260);
    // Estimated (local-engine) timing has no real per-word boundary data to
    // verify against — a small fixed margin gives the real audio some room
    // to run slightly longer than the naive even-distribution guess before
    // the caption fades/clears ahead of it.
    const linger = (isLastInChunk ? 1500 : 1000) + (track.estimated ? 300 : 0);
    const displayDuration = Math.max(minDwell, spokenDuration + linger);

    const fadeDelay = startDelay + displayDuration;
    const clearDelay = fadeDelay + 450;

    // Line activates
    timers.push(
      setTimeout(() => {
        activeLineSeq = lineSeq;
        useAssistant.getState().setCaptionActive(true);
      }, startDelay)
    );

    // Schedule incremental letter-by-letter reveal inside this active line
    let accumulatedText = "";
    for (let wi = 0; wi < line.words.length; wi++) {
      const cw = line.words[wi];
      const cleanWord = unescapeXml(cw.text);
      if (wi > 0) {
        accumulatedText += " ";
      }
      const wordPrefix = accumulatedText;
      const charCount = cleanWord.length;
      const wordDur = Math.max(80, cw.duration_ms);

      for (let ci = 0; ci < charCount; ci++) {
        const textSnapshot = wordPrefix + cleanWord.slice(0, ci + 1);
        const charOffset = charCount > 1 ? (ci / charCount) * wordDur : 0;
        const charDelay = Math.max(0, (cw.start_ms + charOffset) - (now - anchor));

        timers.push(
          setTimeout(() => {
            if (activeLineSeq === lineSeq) {
              notifyLine({
                text: textSnapshot,
                previousText: prevLineText,
                phase: "active",
                words: textSnapshot.trim().split(/\s+/).filter(Boolean),
                activeWordIndex: wi,
              }, false);
            }
          }, charDelay)
        );
      }
      accumulatedText += cleanWord;
    }

    timers.push(
      setTimeout(() => {
        if (activeLineSeq === lineSeq) {
          notifyLine({
            text: line.text,
            previousText: prevLineText,
            phase: "fading",
            words: line.words.map((c) => unescapeXml(c.text)),
            activeWordIndex: -1,
          }, false);
        }
      }, fadeDelay)
    );

    timers.push(
      setTimeout(() => {
        if (activeLineSeq === lineSeq) {
          const looksLikeUtteranceEnd = line.end_ms >= latestScheduledEndMs;
          notifyLine({ text: "", previousText: "", phase: "cleared", words: [] }, false);
          if (looksLikeUtteranceEnd) {
            // This is the last line we currently know about — but a later
            // streamed chunk may simply be running behind, not finished.
            // Give it UTTERANCE_END_GRACE_MS to arrive and cancel this
            // (via scheduleChunk's own pendingCloseTimer clear) before
            // actually closing the caption.
            if (pendingCloseTimer) clearTimeout(pendingCloseTimer);
            pendingCloseTimer = setTimeout(() => {
              pendingCloseTimer = null;
              useAssistant.getState().setCaptionActive(false);
              notify(true);
              notifyLine({ text: "", previousText: "", phase: "cleared", words: [] }, true);
            }, UTTERANCE_END_GRACE_MS);
          }
        }
      }, clearDelay)
    );
  }

  // Re-arm legacy doneTimer
  const last = track.words[track.words.length - 1];
  const endMs = last.start_ms + last.duration_ms;
  if (doneTimer) clearTimeout(doneTimer);
  const delay = Math.max(0, endMs - (performance.now() - anchor)) + 50;
  doneTimer = setTimeout(() => notify(true), delay);
}

let initialized = false;

/** Call once (idempotent) to start listening for Rust's `tts:caption` event. */
/** Test hook: drive scheduleChunk directly (vitest only) — it isn't
 * otherwise reachable except through the real tts:caption Tauri event. */
export function __testScheduleChunk(track: CaptionTrack): void {
  scheduleChunk(track);
}

export function initCaptionListener(): void {
  if (initialized) return;
  initialized = true;
  void import("@tauri-apps/api/event")
    .then(({ listen }) =>
      listen<CaptionTrack>("tts:caption", (event) => {
        if (event.payload) scheduleChunk(event.payload);
      }),
    )
    .catch(() => {
      // Outside Tauri (tests) — no-op.
    });
}
