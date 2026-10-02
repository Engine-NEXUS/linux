/**
 * Adaptive endpointing — the "pre-pausal cut-off".
 *
 * NEXUS used a fixed 3000ms redemption window after speech stopped. That number
 * exists to avoid truncating mid-sentence commands, where a 300–500ms pause is
 * grammatical rather than final. The cost is that every command ends with up to
 * 3 seconds of dead air before anything is transcribed.
 *
 * The rule here decides *early* that the tail is genuinely dead. It is kept in a
 * standalone pure module, with no ONNX, AudioWorklet or DOM dependency, so it can
 * be unit tested by feeding it synthetic frame sequences. That matters: voice
 * path logic that has only been exercised by hand is voice path logic that
 * silently truncates commands in production.
 *
 * Design constraint — this must never truncate more aggressively than the old
 * behaviour:
 *
 *   1. The rule fires only when the tail is unambiguously dead: Silero's speech
 *      probability has collapsed below `probFloor` AND the tail audio is
 *      near-silent below `tailRmsMax`. A grammatical pause keeps probability
 *      elevated (breath, coarticulation, room tone), so it does not match and
 *      the caller falls through to the old redemption path — behaviour identical
 *      to today whenever this rule is unsure.
 *   2. Frames whose probability sits between the negative and positive
 *      thresholds are the *ambiguous band*. They are counted as neither silence
 *      nor speech. That is what stops the timer from accumulating across a
 *      hesitation and then firing mid-phrase.
 *   3. Worst case wait is `baseSilenceMs`, which is far below the 3000ms it
 *      replaces, so this can only reduce latency.
 *
 * CAVEAT: `probFloor` and `tailRmsMax` are set from first principles, not
 * measured — this repository ships no speech fixtures. They are deliberately
 * loose for that reason. `observe` records the statistics behind every decision
 * so a calibration pass on real NEXUS commands can tighten them with evidence.
 */

/** Frame length in ms. 512 samples @16kHz. */
export const FRAME_MS = 512 / 16;

export interface EndpointConfig {
  enabled: boolean;
  /** Tail silence that must accumulate before a decision is allowed. */
  baseSilenceMs: number;
  /** Maximum acceptable speech probability anywhere in the tail. */
  probFloor: number;
  /** Maximum acceptable mean RMS across the tail. */
  tailRmsMax: number;
  /** Minimum speech seen before we are willing to end the segment at all. */
  minSpeechMs: number;
  /** Probability at/above which a frame counts as speech (cancels tail evidence). */
  positiveThreshold: number;
  /** Probability below which a frame contributes tail evidence. */
  negativeThreshold: number;
}

export const DEFAULT_CONFIG: EndpointConfig = {
  enabled: true,
  baseSilenceMs: 400,
  probFloor: 0.15,
  tailRmsMax: 0.005,
  minSpeechMs: 500,
  positiveThreshold: 0.5,
  negativeThreshold: 0.35,
};

export interface EndpointState {
  /** Total speech accumulated so far, ms. */
  speechMs: number;
  /** Index of the frame on which speech first crossed the positive threshold. */
  onsetFrame: number;
  /** Consecutive unambiguous-silence frames, ms. */
  tailSilenceMs: number;
  /** Highest speech probability seen across the tail. */
  tailMaxProb: number;
  /** Sum of squared per-frame RMS across the tail. */
  tailSumSq: number;
  /** Tail frames observed. */
  tailFrames: number;
}

export function newState(): EndpointState {
  return {
    speechMs: 0,
    onsetFrame: -1,
    tailSilenceMs: 0,
    tailMaxProb: 0,
    tailSumSq: 0,
    tailFrames: 0,
  };
}

/**
 * Fold one frame into the endpoint state. Returns the same object mutated for
 * allocation reasons — one per 32ms frame is not worth a fresh object.
 *
 * @param frameIndex index of this frame within the rolling buffer
 * @param prob       Silero speech probability for the frame, 0..1
 * @param rms        root-mean-square amplitude of the frame, 0..1
 */
export function observe(
  s: EndpointState,
  frameIndex: number,
  prob: number,
  rms: number,
  cfg: EndpointConfig = DEFAULT_CONFIG,
): EndpointState {
  if (prob >= cfg.positiveThreshold) {
    s.speechMs += FRAME_MS;
    if (s.onsetFrame < 0) s.onsetFrame = frameIndex;
    // Speech resumed: all tail evidence is void.
    s.tailSilenceMs = 0;
    s.tailMaxProb = 0;
    s.tailSumSq = 0;
    s.tailFrames = 0;
  } else if (prob < cfg.negativeThreshold) {
    s.tailSilenceMs += FRAME_MS;
    s.tailMaxProb = Math.max(s.tailMaxProb, prob);
    s.tailSumSq += rms * rms;
    s.tailFrames += 1;
  }
  // Ambiguous band: intentionally no state change.
  return s;
}

/** Mean RMS of the observed tail. 0 when no tail frames. */
export function tailRms(s: EndpointState): number {
  return s.tailFrames > 0 ? Math.sqrt(s.tailSumSq / s.tailFrames) : 0;
}

/** True when the tail is unambiguously dead and the segment can be closed. */
export function shouldFinalize(
  s: EndpointState,
  cfg: EndpointConfig = DEFAULT_CONFIG,
): boolean {
  if (!cfg.enabled) return false;
  if (s.speechMs < cfg.minSpeechMs) return false;
  if (s.tailSilenceMs < cfg.baseSilenceMs) return false;
  if (s.tailFrames === 0) return false;
  return s.tailMaxProb <= cfg.probFloor && tailRms(s) <= cfg.tailRmsMax;
}

/**
 * Index of the first frame to keep for the final segment, walking back from the
 * detected onset by `preRollMs` so a word whose leading phonemes fall below the
 * speech threshold is not clipped. This is the "pre-pausal" half of the work:
 * we are willing to end the utterance early only because we kept the audio that
 * precedes it.
 */
export function preRollStart(
  onsetFrame: number,
  preRollMs: number,
  frameCount: number,
): number {
  const back = Math.ceil(preRollMs / FRAME_MS);
  return Math.max(0, Math.min(onsetFrame, frameCount) - back);
}