/**
 * Audio-reactive aura around the orb (ported from the Linux redesign,
 * `docs/features/63-…`).
 *
 * One small WebGL canvas, driven by saystack's `createAuraRenderer`
 * (`@saystack/core`, MIT) through a `requestAnimationFrame` loop, rims the
 * orb's particle sphere. Its energy is `audio/levels.ts` — the same level the
 * orb itself shows.
 *
 * - Every band carries the same scalar level: NEXUS has one energy number,
 *   not a spectrum, so the rim lights uniformly and brightness/motion follow
 *   the voice; the hue spread is ambience, never a state label.
 * - `frame()` returns false once the aura has fully faded, so the loop parks
 *   itself. The aura goes `hidden` whenever the orb is not shown, so it costs
 *   no GPU while NEXUS is idle.
 * - Reduced motion keeps the aura but calms it: the renderer itself drops the
 *   glide and runs its motion at 12% (`isReducedMotion`). Hiding it instead
 *   would remove the look the user asked for on any desktop that turns
 *   animations off (Pop!_OS COSMIC here: `enable-animations false`).
 * - The canvas is `position: fixed` and positioned by the host in viewport
 *   coordinates, so it must not live inside a transformed ancestor (the orb
 *   frame slides with a transform): OrbFrame mounts it through a portal.
 * - No WebGL (blocklisted GPU, llvmpipe without GL): no rim, the orb keeps
 *   working.
 *
 * CSP note (see stage/OrbFrame.tsx): no React `style` props; the host sizes
 * and places the canvas through CSSOM writes.
 */

import { useCallback, useEffect, useRef } from "react";
import { createAuraRenderer, type AuraState, type IAuraRenderer } from "@saystack/core";
import { useAssistant, type AssistantState } from "../store/assistant";
import { getLevel } from "../audio/levels";
import { auraLookFor, auraStateFor } from "./orbPresentation";
import { createStageAuraHost } from "./stageAuraHost";

const AURA_BANDS = 5;
const levelBands = new Float32Array(AURA_BANDS);

/** Pull callback handed to the renderer: every band carries the one level. */
const pullLevels = (): Float32Array => {
  const level = getLevel();
  for (let i = 0; i < levelBands.length; i++) levelBands[i] = level;
  return levelBands;
};

/**
 * Base look; per-state geometry and energy come from `auraLookFor`.
 * `palette: "prism"` spreads the hue wheel around the rim as ambience.
 */
const BASE_STYLE = {
  bands: AURA_BANDS,
  layout: "flow",
  palette: "prism",
  lift: 10,
  lineWidth: 1.4,
  // The renderer blurs in-shader (effects: "shader"); the host must not add a
  // CSS filter on top — double blur is the classic tell.
  blur: 0,
  aura: 0.55,
  auraSize: 8,
  brightness: 0.5,
  rippleSpeed: 0.35,
  resolution: 1,
  placement: "around",
  outline: "fade",
} as const;

interface AuraHandles {
  renderer: IAuraRenderer;
  /** Restart the rAF loop if it parked itself. */
  arm: () => void;
}

/** Pure: the renderer state for the orb right now (a hidden orb gets a parked aura). */
export function auraRendererState(state: AssistantState, shown: boolean): AuraState {
  return shown ? auraStateFor(state) : "hidden";
}

const prefersReducedMotion = (): boolean =>
  typeof window !== "undefined" &&
  typeof window.matchMedia === "function" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

export function AuraCanvas({
  anchorRef,
  shown,
  padding = 0,
}: {
  /** The element whose perimeter the aura rims (sized to the visible sphere). */
  anchorRef: React.RefObject<HTMLElement | null>;
  /** Whether the orb is on screen; a hidden orb gets a hidden (parked) aura. */
  shown: boolean;
  /** Slack around the anchor rect, CSS px. */
  padding?: number;
}) {
  const state = useAssistant((s) => s.state);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const handlesRef = useRef<AuraHandles | null>(null);
  const shownRef = useRef(shown);
  shownRef.current = shown;

  const applyState = useCallback((handles: AuraHandles, next: AssistantState) => {
    handles.renderer.setState(auraRendererState(next, shownRef.current));
    handles.renderer.configure({
      levels: pullLevels,
      style: { ...BASE_STYLE, ...auraLookFor(next) },
      gap: 1.5,
      glideMs: 480,
    });
    handles.renderer.retarget(false, performance.now());
    handles.arm();
  }, []);

  // Mount once: GL context, host, render loop.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const gl = canvas.getContext("webgl", {
      alpha: true,
      premultipliedAlpha: true,
      antialias: false,
      depth: false,
      stencil: false,
      powerPreference: "low-power",
    });
    if (!gl) {
      canvas.style.display = "none";
      return;
    }

    const host = createStageAuraHost({ canvas, getAnchor: () => anchorRef.current, padding });

    let renderer: IAuraRenderer;
    try {
      renderer = createAuraRenderer(gl, {
        settings: { levels: pullLevels, style: BASE_STYLE, gap: 1.5, glideMs: 480 },
        background: "dark",
        effects: "shader",
        // MUST stay false. `true` sets u_additive = 1 and the dark-mode output
        // alpha becomes exactly 0 (saystack shaders, `maxRGB * (1 - u_additive)`);
        // composited normally, the aura would be invisible.
        isAdditive: false,
        isReducedMotion: prefersReducedMotion,
      });
    } catch {
      canvas.style.display = "none";
      return;
    }

    const handleLost = (event: Event) => {
      event.preventDefault();
      renderer.lose();
    };
    const handleRestored = () => renderer.restore();
    canvas.addEventListener("webglcontextlost", handleLost);
    canvas.addEventListener("webglcontextrestored", handleRestored);

    let raf = 0;
    const loop = (now: number) => {
      raf = 0;
      // false → fully faded out: park the loop; arm() restarts it.
      if (renderer.frame(now, host)) raf = requestAnimationFrame(loop);
    };
    const arm = () => {
      if (raf === 0) raf = requestAnimationFrame(loop);
    };

    const handles: AuraHandles = { renderer, arm };
    handlesRef.current = handles;
    raf = requestAnimationFrame(loop);

    // Apply the live state now: React StrictMode remounts effects, the fresh
    // renderer starts hidden, and the state effect below would not re-fire.
    applyState(handles, useAssistant.getState().state);

    const motion =
      typeof window.matchMedia === "function" ? window.matchMedia("(prefers-reduced-motion: reduce)") : null;
    const onMotion = () => {
      if (handlesRef.current) applyState(handlesRef.current, useAssistant.getState().state);
    };
    motion?.addEventListener?.("change", onMotion);

    return () => {
      motion?.removeEventListener?.("change", onMotion);
      if (raf) cancelAnimationFrame(raf);
      canvas.removeEventListener("webglcontextlost", handleLost);
      canvas.removeEventListener("webglcontextrestored", handleRestored);
      handlesRef.current = null;
      host.destroy();
      renderer.dispose();
    };
  }, [anchorRef, padding, applyState]);

  // State or visibility changed: new mechanism/geometry, glide the rim, re-arm.
  useEffect(() => {
    const handles = handlesRef.current;
    if (handles) applyState(handles, state);
  }, [state, shown, applyState]);

  return <canvas ref={canvasRef} className="aura-canvas" aria-hidden="true" data-interactive="false" />;
}
