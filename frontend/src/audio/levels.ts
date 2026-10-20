/**
 * The one energy level the orb and its aura both show.
 *
 * Same sources the orb already uses (`avatar/Avatar.tsx` `level` prop and
 * `avatar/VoiceOrb.tsx`'s envelope pump), so the orb and the aura can never
 * show two different sources at the same instant:
 * - speaking: NEXUS's own voice envelope (`tts:caption.envelope`), else the
 *   playback proxy while TTS is audible;
 * - listening: the microphone (`audio:level` from Rust, or the browser VAD
 *   volume, whichever is louder);
 * - idle / thinking: no audio drives the visuals.
 */

import { getEnvelopeLevel } from "./captionScheduler";
import { useAssistant, type AssistantState } from "../store/assistant";

export interface LevelInputs {
  state: AssistantState;
  ttsActive: boolean;
  /** The TTS envelope value for this instant, or null when none is scheduled. */
  envelope: number | null;
  /** Browser-VAD volume, 0..1. */
  audioVolume: number;
  /** Rust capture level (`audio:level`), 0..1. */
  micLevel: number;
}

const clamp01 = (v: number): number => (Number.isFinite(v) ? Math.max(0, Math.min(1, v)) : 0);

/** Pure: the level for these inputs, 0..1. Never NaN (NaN into a shader uniform draws NaN pixels). */
export function levelFor(i: LevelInputs): number {
  if (i.state === "speaking") {
    if (i.envelope !== null && Number.isFinite(i.envelope)) return clamp01(i.envelope);
    return i.ttsActive ? clamp01(Math.max(0.4, i.audioVolume)) : 0;
  }
  if (i.state === "listening") return clamp01(Math.max(i.micLevel, i.audioVolume));
  return 0;
}

/** The live level, read from the store (and the envelope while speaking). */
export function getLevel(): number {
  const s = useAssistant.getState();
  return levelFor({
    state: s.state,
    ttsActive: s.ttsActive,
    envelope: s.state === "speaking" ? getEnvelopeLevel() : null,
    audioVolume: s.audioVolume,
    micLevel: s.micLevel,
  });
}
