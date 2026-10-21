/**
 * Pure state → aura table for the orb's audio-reactive rim.
 *
 * Ported from the Linux redesign (`docs/features/63-…`), trimmed to the aura:
 * this branch keeps its WebGL particle orb (`avatar/voice-orb.js`), so the
 * redesign's CSS-orb sizes and bottom-dock maths are not used here.
 *
 * Two rules from the design research hold:
 * - **Geometry and mechanism carry state, energy carries audio.** Listening
 *   and thinking differ by mechanism (reactive vs orbiting comet), not hue.
 * - **Hue never means state.** No state sets a palette; colour is ambience.
 */

import type { AuraState } from "@saystack/core";
import type { AssistantState } from "../store/assistant";

const AURA_BY_STATE: Readonly<Record<AssistantState, AuraState>> = {
  // `paused` still draws a dim, audio-inert rim; `hidden` would park the
  // render loop and an idle orb would look broken.
  idle: "paused",
  listening: "active",
  thinking: "working",
  speaking: "active",
};

export interface AuraLook {
  bands: number;
  placement: "top" | "around" | "bottom";
  layout?: "mirror" | "linear" | "flow";
  rippleSpeed?: number;
  aura?: number;
  auraSize?: number;
  brightness?: number;
  lift?: number;
}

/**
 * Sizes are in CSS px around the orb. The redesign tuned them for a 44–72 px
 * mark; this orb's particle sphere is about twice that, so `lift` and
 * `auraSize` are scaled up to keep the same proportion.
 */
const AURA_LOOK_BY_STATE: Readonly<Record<AssistantState, AuraLook>> = {
  idle: { bands: 5, placement: "around", layout: "flow", rippleSpeed: 0.2, aura: 0.35, auraSize: 8, brightness: 0.35, lift: 10 },
  listening: { bands: 5, placement: "around", layout: "flow", rippleSpeed: 0.55, aura: 1.4, auraSize: 28, brightness: 0.9, lift: 16 },
  thinking: { bands: 5, placement: "around", layout: "flow", rippleSpeed: 0.15, aura: 0.8, auraSize: 16, brightness: 0.75, lift: 12 },
  speaking: { bands: 5, placement: "around", layout: "mirror", rippleSpeed: 0.5, aura: 1.4, auraSize: 28, brightness: 0.9, lift: 16 },
};

export function auraStateFor(state: AssistantState): AuraState {
  return AURA_BY_STATE[state];
}

export function auraLookFor(state: AssistantState): AuraLook {
  return AURA_LOOK_BY_STATE[state];
}
