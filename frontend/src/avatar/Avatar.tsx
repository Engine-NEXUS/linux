import { useEffect, useState, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { useAssistant, AssistantState } from "../store/assistant";
import { positionToPct, wheelResize, type CalibrationTarget } from "../calibration/geometry";
import { VoiceOrb } from "./VoiceOrb";
import { useThemeMode } from "../sidebar/useThemeMode";

/**
 * WebGL particle orb avatar — the sole visual in ALL modes (normal + ghost).
 *
 * The Lottie era (wakeup.json choreography, waves bars, phase machine) is
 * RETIRED: the animation JSON files stay on disk unused (kept for future
 * use), but nothing loads them anymore. Every state is now particle-driven
 * (shapes redesigned per the reference-video plan, voice-orb.js Phase 2):
 *   idle      : grey breathing sphere
 *   listening : grainy warm amber/brown sphere, calm (STT mic-reactive)
 *   thinking  : 3D rotating purple beaded starburst — 64 radial rays,
 *               dense glowing nucleus, concentric beaded steps, 3D rotation
 *   speaking  : dense irregular potato/pebble blob (real TTS-audio beat
 *               molds the silhouette, particle sparks)
 * A brief glitch/tear burst plays whenever the dominant state changes.
 * Ghost mode : the orb IS the always-on display — assemble() flight on entry.
 *
 * The exported pure functions below (resolveAvatarAnim, ghostWaveBars,
 * shouldShowWaves, zoomForFrame, …) remain for the pinned unit tests and
 * future reuse — the component body no longer calls the Lottie ones.
 */

const SEG_LOADING: [number, number] = [171, 260];
const SEG_SMILE_ARRIVE: [number, number] = [261, 316];

type AnimMode = "wake-loading" | "wake-smile" | "idle-smile" | "loading-loop" | "holding";

export interface AvatarAnim {
  segment: [number, number] | null; // null = hold current frame
  loop: boolean;
  speed: number;
  mode: AnimMode;
}

/** Pure state → animation mapping (unit-tested). */
export function resolveAvatarAnim(st: AssistantState): AvatarAnim {
  // Lively 1.25x everywhere (user directive — the original 1.0x read sleepy).
  // Frame-based zoom (zoomForFrame) tracks any speed automatically: frames
  // are frames, so the zoom stays phase-locked at 1.0x, 1.25x, or 1.5x.
  const speed: Record<AssistantState, number> = {
    idle: 1.25,
    listening: 1.25,
    thinking: 1.25,
    speaking: 1.25,
  };
  if (st === "thinking") {
    return { segment: SEG_LOADING, loop: true, speed: speed[st], mode: "loading-loop" };
  }
  return { segment: SEG_SMILE_ARRIVE, loop: false, speed: speed[st], mode: "idle-smile" };
}

/**
 * Pure speaking-animation decision (unit-tested).
 *
 * Speaking shows the smiling orb, which pulses via CSS when TTS audio is actually playing.
 * Silent `speaking` (meeting suppression, inter-chunk gaps, waits) holds
 * the current frame. `listening` and `thinking` keep their segments via resolveAvatarAnim.
 */
export function shouldHoldSpeakingFrame(
  st: AssistantState,
  ttsActive: boolean,
): boolean {
  return st === "speaking" && !ttsActive;
}

/**
 * Ghost wave bars — the exact three Lottie palette colors, tall-short-tall
 * like a live waveform. Pure spec (unit-tested); the component animates
 * heights via CSS.
 */
export interface GhostBar {
  color: string;
  /** resting height in px inside the 180px avatar box */
  height: number;
  /** css animation duration / delay for organic phase offsets */
  durationMs: number;
  delayMs: number;
}

export function ghostWaveBars(): GhostBar[] {
  return [
    { color: "#259ed6", height: 120, durationMs: 900, delayMs: 0 },
    { color: "#ef4f25", height: 70, durationMs: 700, delayMs: 150 },
    { color: "#fbdf38", height: 120, durationMs: 1100, delayMs: 300 },
  ];
}

/** Smile → pinch (220ms) → waves. Pinch first so the morph reads as one
 *  continuous motion instead of a hard swap. */
export const GHOST_PINCH_MS = 220;

/** Exit beat: waves shrink/fade before the smile returns (Esc must feel
 *  instant but not a hard cut). */
export const GHOST_LEAVE_MS = 200;

/** Staggered bar entrance step so the trio blooms left→right. Pure. */
export function waveEnterDelayMs(barIndex: number): number {
  return barIndex * 90;
}

/**
 * Waves-visibility invariant (P2 UI-director rule, doc 74). Waves may show
 * ONLY inside an active ghost session with a visible orb — never on plain
 * wake, never while hidden. This exact line broke twice (waves-on-every-wake
 * killed the wake choreography; an invisible-orb gate would show waves to
 * nobody), so it lives here as a tested pure function, not inline logic.
 */
export function shouldShowWaves(visible: boolean, ghostActive: boolean): boolean {
  return visible && ghostActive;
}

/**
 * Ghost always-on display gate (orb-as-display plan). While a ghost session
 * is live and the orb window is visible, the WebGL particle orb itself is
 * the persistent display — waves bars are NOT rendered. Same invariant
 * shape as shouldShowWaves; tested pure function.
 */
export function shouldShowGhostParticles(visible: boolean, ghostActive: boolean): boolean {
  return visible && ghostActive;
}

/**
 * Waves-container visibility (Phase 0 calibration fix). The container
 * shows during the ghost waves/leaving phases AND during a Waves
 * calibration preview — which has no ghost session, so it can never
 * reach those phases. Pure (unit-tested); every gate below delegates
 * here instead of repeating the phase list.
 */
export type WavesPhase = "smile" | "pinching" | "waves" | "leaving";

export function resolveWavesShown(
  ghostPhase: WavesPhase,
  calibrationTarget: string | null
): boolean {
  return (
    ghostPhase === "waves" ||
    ghostPhase === "leaving" ||
    calibrationTarget === "waves"
  );
}

/** Calibration Waves preview (no ghost session): lively, not frozen. */
export function isWavesPreview(calibrationTarget: string | null): boolean {
  return calibrationTarget === "waves";
}

/** Mic floor below which waves rest (still). Kills noise-floor jitter. */
export const WAVE_REST_FLOOR = 0.015;

/**
 * Target bar scale 0.15..1 from a 0..1 level, per-bar stagger so the trio
 * never pumps in lockstep. Pure (unit-tested).
 */
export function waveScaleForBar(level: number, barIndex: number): number {
  const clamped = Math.max(0, Math.min(1, level));
  if (clamped < WAVE_REST_FLOOR) return 0.15;
  const stagger = [1, 0.72, 0.88][barIndex % 3];
  return 0.15 + 0.85 * clamped * stagger;
}

/**
 * Frame-locked wrapper zoom 1.0..1.08, phase-locked to the Lottie timeline.
 * Pure (unit-tested). Thinking zooms exactly once per loading-loop revolution
 * (89 frames from 171); listening/speaking breathe on a 54-frame period once
 * the smile has settled (holding) — deeper for speaking so the states stay
 * distinguishable. Idle, waiting, and a mid-arrival smile return exactly 1:
 * no zoom fights the arrival motion, and freeze means freeze.
 */
export function zoomForFrame(
  st: AssistantState,
  frame: number,
  holding: boolean,
  waiting: boolean,
): number {
  if (st === "idle" || waiting) return 1;
  if (st === "thinking") {
    const p = (((frame - 171) % 89) + 89) % 89 / 89;
    return 1 + 0.04 * (0.5 - 0.5 * Math.cos(2 * Math.PI * p));
  }
  if (!holding) return 1;
  const amp = st === "speaking" ? 0.08 : 0.035;
  const p = ((((frame % 54) + 54) % 54) / 54);
  return 1 + amp * (0.5 - 0.5 * Math.cos(2 * Math.PI * p));
}

/**
 * Stylized TTS rhythm 0..1. There is NO true rodio playback envelope on
 * the bridge (see research doc), so speaking waves ride this deterministic
 * speech-like rhythm GATED on ttsActive (event-derived from real playback
 * start/stop) — motion exactly while NEXUS talks, still otherwise. Pure.
 */
export function ttsWaveLevel(tMs: number, barIndex: number): number {
  const t = tMs / 1000;
  const w =
    Math.sin(t * 2 * Math.PI * 2.1 + barIndex * 1.7) * 0.5 +
    Math.sin(t * 2 * Math.PI * 3.7 + barIndex * 0.9) * 0.3;
  return Math.max(0, Math.min(1, 0.45 + 0.55 * w));
}

/**
 * Rest-dot shimmer 0.85..1. Pure (unit-tested). Shown while the ghost waves
 * sit in silence: trim 50/50 renders NOTHING (zero-length subpaths are
 * culled — the "empty rest" the user reported), so procedural dots own the
 * rest visual at the exact Lottie bar geometry. Per-dot independent slow
 * frequencies + phases: organic non-order drift, never lockstep, never the
 * left→right entrance order.
 */
export function restDotScale(tMs: number, barIndex: number): number {
  const t = tMs / 1000;
  const f = [0.31, 0.47, 0.23][barIndex % 3];
  const p = [0, 2.1, 4.4][barIndex % 3];
  return 0.85 + 0.15 * (0.5 - 0.5 * Math.cos(2 * Math.PI * (f * t + p)));
}

/** Rest-dot geometry (%) matching the waves-2.json bar centers exactly.
/// X = bar x positions; Y = layer origin y (bars grow from this center).
/// A 46px vertical jump on every rest↔speech handoff taught us: eyeballing
/// "center" instead of reading the file geometry is a real defect (fixed). */
export const REST_DOT_X = [30.5, 43.5, 56.5];
export const REST_DOT_Y = 24.2;

/** Which live source drives the waves right now. Pure (unit-tested). */
export function waveSource(
  st: AssistantState,
  ttsActive: boolean,
  micLevel: number,
): "mic" | "tts" | "rest" {
  if (st === "speaking" && ttsActive) return "tts";
  if (st === "listening" && micLevel >= WAVE_REST_FLOOR) return "mic";
  return "rest";
}

/**
 * Single playhead level 0..1 for the waves-Lottie scrub. Pure (unit-tested).
 * TTS uses the MEAN across bars (not the max — max jumps between bars and
 * the lone playhead can't follow three peaks at once). Mic is clamped RMS.
 */
export function scrubLevel(src: "mic" | "tts" | "rest", nowMs: number, micLevel: number): number {
  if (src === "tts") {
    return (ttsWaveLevel(nowMs, 0) + ttsWaveLevel(nowMs, 1) + ttsWaveLevel(nowMs, 2)) / 3;
  }
  if (src === "mic") return Math.max(0, Math.min(1, micLevel));
  return 0;
}

/**
 * Eased playhead step. Pure (unit-tested). The raw level (especially the
 * 2.1/3.7Hz TTS rhythm) jumps several frames per tick — seeking it directly
 * reads as flicker. Lerping the frame (~40ms time constant at 60fps) turns
 * jumps into waves. Float frames are intentional: lottie-web renders
 * subframes, so the playhead glides instead of stepping.
 */
export function easeScrubFrame(current: number, target: number, factor = 0.3): number {
  return current + (target - current) * factor;
}

export interface AvatarProps {
  /** One-shot: fires voice-orb.js's assemble() particle-burst entrance.
   *  OrbFrame triggers this on every visible false→true transition now
   *  (not just ghost sessions — plan §1.3, no-slide entrance). */
  entered?: boolean;
  /** True while the exit disperse() tween is still playing — OrbFrame
   *  keeps rendering the orb during this window so the particles can
   *  finish flying apart instead of the host div unmounting mid-tween. */
  dispersing?: boolean;
  /** False while the capsule is hidden (Ghost idle): the WebGL loop pauses
   *  so a hidden orb costs no GPU. Defaults to true for every other caller. */
  shown?: boolean;
}

export function Avatar({ entered: enteredProp = false, dispersing = false, shown = true }: AvatarProps = {}) {
  const state = useAssistant((s) => s.state);
  const visible = useAssistant((s) => s.visible);
  const ttsActive = useAssistant((s) => s.ttsActive);
  const audioVolume = useAssistant((s) => s.audioVolume);
  const micLevel = useAssistant((s) => s.micLevel);
  const awaitingInput = useAssistant((s) => s.awaitingInput);
  const ghostActive = useAssistant((s) => s.ghostActive);
  const calibrationTarget = useAssistant((s) => s.calibrationTarget);
  const calibrationSize = useAssistant((s) => s.calibrationSize);
  const calibrationPulse = useAssistant((s) => s.calibrationPulse);
  const orbColor = useAssistant((s) => s.orbColor);
  const orbThinkMode = useAssistant((s) => s.orbThinkMode);
  const orbSpeakMode = useAssistant((s) => s.orbSpeakMode);
  const orbListenMode = useAssistant((s) => s.orbListenMode);
  const themeMode = useThemeMode();

  // Battery-aware power scaling: on battery, scale particles 5000 -> 2500
  // to halve vertex & fragment shader workloads and extend laptop runtime.
  const [onBattery, setOnBattery] = useState(false);
  useEffect(() => {
    if (typeof navigator !== "undefined" && "getBattery" in navigator) {
      (navigator as any)
        .getBattery()
        .then((b: any) => {
          setOnBattery(!b.charging);
          b.addEventListener("chargingchange", () => setOnBattery(!b.charging));
        })
        .catch(() => {});
    }
  }, []);

  // GPU-contention power scaling: Hand Mode's pipeline detects sustained contention (another app
  // competing for the same shared WebView2 GPU process — see `src-tauri/src/hand/mod.rs`) from
  // real infer-time data and broadcasts it on the same `hand:state` event the hand window itself
  // listens to. The orb reacts by giving back some of its own GPU draw cost, same mechanism as
  // the battery cap just above, combined into one ceiling below.
  const [gpuBusy, setGpuBusy] = useState(false);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<{ gpu_busy?: boolean }>("hand:state", (e) => setGpuBusy(Boolean(e.payload.gpu_busy))).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

  // Turn-gap pin (ghost always-on plan): in ghost mode an idle arriving
  // within 1.2s of the last active state (hot-mic relisten churn) holds the
  // previous active state for the orb instead — no idle↔listening double-morph.
  // Outside ghost, displayState tracks state 1:1.
  const [displayState, setDisplayState] = useState<AssistantState>(state);
  const lastActiveRef = useRef(0);

  useEffect(() => {
    if (!ghostActive) {
      setDisplayState(state);
      return;
    }
    if (state !== "idle") {
      lastActiveRef.current = performance.now();
      setDisplayState(state);
      return;
    }
    const gap = performance.now() - lastActiveRef.current;
    if (gap >= 1200) {
      setDisplayState("idle");
      return;
    }
    const t = setTimeout(() => setDisplayState("idle"), 1200 - gap);
    return () => clearTimeout(t);
  }, [state, ghostActive]);

  // ─── Animation calibration (drag + wheel) ────────────────────────────
  // Single-stage migration (plan §1.5): the orb is a div inside the
  // always-fullscreen `stage` window now, not its own OS window, so
  // `startDragging()` would drag the whole stage instead of just the
  // preview. Reimplemented as in-page pointer tracking: since `stage`
  // covers the entire primary monitor at a fixed (0,0) origin, a
  // PointerEvent's clientX/clientY IS the screen position (× dpr for
  // physical px) — no native window-move listener needed at all, the
  // math is simpler than the old cross-window version. `calibration/
  // geometry.ts`'s `positionToPct` (same pure function as before) still
  // owns the pct conversion; Rust still clamps + applies + persists.
  const handleCalibrationPointerDown = (e: React.PointerEvent) => {
    if (!calibrationTarget || calibrationTarget === "loading") return;
    e.preventDefault();
    const hostEl = (e.currentTarget as HTMLElement).closest("#orb-frame") as HTMLElement | null;
    const hostRect = (hostEl ?? (e.currentTarget as HTMLElement)).getBoundingClientRect();
    const offsetX = e.clientX - hostRect.left;
    const offsetY = e.clientY - hostRect.top;

    void (async () => {
      const [{ invoke }, { currentMonitor }] = await Promise.all([
        import("@tauri-apps/api/core"),
        import("@tauri-apps/api/window"),
      ]);
      const mon = await currentMonitor();
      if (!mon) return;
      const dpr = mon.scaleFactor;
      let pending: { h: number; v: number } | null = null;
      let reportTimer: ReturnType<typeof setTimeout> | null = null;

      const compute = (clientX: number, clientY: number) => {
        const size = useAssistant.getState().calibrationSize ?? 200;
        const physWin = Math.round(size * dpr);
        const topLeftCssX = clientX - offsetX;
        const topLeftCssY = clientY - offsetY;
        return positionToPct(
          Math.round(topLeftCssX * dpr),
          Math.round(topLeftCssY * dpr),
          physWin,
          physWin,
          mon.size.width,
          mon.size.height,
          20 * dpr
        );
      };

      const flush = () => {
        if (!pending) return;
        const { h, v } = pending;
        pending = null;
        void invoke("calibration_report_position", { hPct: h, vPct: v }).catch(() => {});
      };

      const handleMove = (ev: PointerEvent) => {
        pending = compute(ev.clientX, ev.clientY);
        // Light throttle (~60fps) — dragging reports continuously, only
        // the latest matters each frame.
        if (reportTimer) return;
        reportTimer = setTimeout(() => {
          reportTimer = null;
          flush();
        }, 16);
      };
      const handleUp = (ev: PointerEvent) => {
        window.removeEventListener("pointermove", handleMove);
        window.removeEventListener("pointerup", handleUp);
        if (reportTimer) {
          clearTimeout(reportTimer);
          reportTimer = null;
        }
        pending = compute(ev.clientX, ev.clientY);
        flush();
      };
      window.addEventListener("pointermove", handleMove);
      window.addEventListener("pointerup", handleUp);
    })();
  };

  const handleCalibrationWheel = (e: React.WheelEvent) => {
    if (!calibrationTarget || calibrationTarget === "loading") return;
    e.preventDefault();
    const current = calibrationSize ?? 200;
    const next = wheelResize(calibrationTarget as CalibrationTarget, current, e.deltaY);
    void import("@tauri-apps/api/core").then(({ invoke }) =>
      invoke("calibration_report_size", { size: next }).catch(() => {})
    );
  };

  const waiting = state === "speaking" && awaitingInput;
  const calibrating = calibrationTarget === "wakeup" || calibrationTarget === "waves";
  const wavesPreview = calibrationTarget === "waves";
  // Ghost always-on display: the WebGL orb replaces the waves bars.
  const ghostDisplay = shouldShowGhostParticles(visible, ghostActive);
  const isOrbVisible = Boolean(enteredProp || dispersing || (shown && (visible || ghostActive)));
  // Target-switch pulse (plan 04 §4): toggling suffix restarts the CSS
  // animation, so identical rects still read as switched.
  const pulseClass = calibrating ? ` calib-pulse-${calibrationPulse % 2}` : "";
  return (
    <div
      data-interactive
      className={`avatar-wrap ${ghostDisplay ? "avatar-wrap--ghost" : `avatar-wrap--${displayState}`}${waiting && !ghostDisplay ? " avatar-wrap--waiting" : ""}${calibrating ? " avatar-wrap--calibrating" : ""}${pulseClass}`}
      onPointerDown={calibrating ? handleCalibrationPointerDown : undefined}
      onWheel={calibrating ? handleCalibrationWheel : undefined}
    >
      {/* Hit-catcher: transparent overlay pixels never receive mouse events
          (OS per-pixel hit-testing), so a bare waves/loading window is
          draggable only on its painted dots. This ~1%-alpha wash makes the
          whole window hittable with zero visual change. */}
      {calibrating && <div className="calibration-hitcatcher" aria-hidden />}
      {!wavesPreview && (
        <div className="avatar-voice-orb-container">
          <VoiceOrb
            state={ghostDisplay ? displayState : state}
            color={orbColor}
            thinkMode={orbThinkMode}
            speakMode={orbSpeakMode}
            listenMode={orbListenMode}
            theme={themeMode}
            visible={isOrbVisible}
            particles={Math.min(onBattery ? 2500 : 5000, gpuBusy ? 2000 : Infinity)}
            entered={ghostActive || enteredProp}
            dispersing={dispersing}
            level={
              displayState === "speaking"
                ? (ttsActive ? Math.max(0.4, audioVolume) : 0)
                : (ghostActive ? micLevel : audioVolume)
            }
          />
        </div>
      )}
      {!ghostActive && wavesPreview && (
        <div className="ghost-waves" aria-label="listening">
          {ghostWaveBars().map((bar) => (
            <div
              key={bar.color}
              className="ghost-bar"
              ref={(el) => {
                // Per-bar dynamic styling (color/height genuinely vary per
                // bar, so this can't be a single static CSS class) applied
                // via direct DOM mutation, not React's `style` prop — see
                // the CSP note on the main wrapper div above/OrbFrame.tsx.
                if (!el) return;
                el.style.background = bar.color;
                el.style.height = `${bar.height}px`;
                el.style.transform = "scaleY(0.15)";
              }}
            />
          ))}
        </div>
      )}
      {calibrating && calibrationSize != null && (
        <div className="calibration-badge" aria-hidden>
          {calibrationSize} px
        </div>
      )}
    </div>
  );
}
