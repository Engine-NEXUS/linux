import { useEffect, useState, useRef } from "react";
import lottie, { AnimationItem } from "lottie-web";
import { useAssistant, AssistantState } from "../store/assistant";

/**
 * Lottie-driven floating orb avatar.
 * Uses wakeup.json for the animation.
 *
 * Animation segments (absolute frame numbers from wakeup.json):
 *   171-260 : loading circles (3 colored circles moving)
 *   261-316 : smile arrives (face transitions back, settles by frame 289)
 *   frame 300 : stable smile hold frame (face ctrl pos=[0,0,0], scale=[100,100,100])
 *
 * Sequencing:
 *   listening (wake)  : loading (1.5x, ~1s) → smile arrives (1.5x, ~0.6s) → hold at 300
 *   thinking/speaking : loading circles loop (1.5x / 1.2x)
 *   idle (after done) : smile arrives (1.0x, ~0.9s) → hold at 300
 */

const SEG_LOADING: [number, number] = [171, 260];
const SEG_SMILE_ARRIVE: [number, number] = [261, 316];
const FRAME_HOLD_SMILE = 300;

type AnimMode = "wake-loading" | "wake-smile" | "idle-smile" | "loading-loop" | "holding";

export interface AvatarAnim {
  segment: [number, number] | null; // null = hold current frame
  loop: boolean;
  speed: number;
  mode: AnimMode;
}

/** Pure state → animation mapping (unit-tested). */
export function resolveAvatarAnim(st: AssistantState): AvatarAnim {  const speed: Record<AssistantState, number> = {
    idle: 1.0,
    listening: 1.5,
    thinking: 1.5,
    speaking: 1.2,
  };
  if (st === "listening") {
    return { segment: SEG_LOADING, loop: false, speed: speed[st], mode: "wake-loading" };
  }
  if (st === "thinking" || st === "speaking") {
    return { segment: SEG_LOADING, loop: true, speed: speed[st], mode: "loading-loop" };
  }
  return { segment: SEG_SMILE_ARRIVE, loop: false, speed: speed[st], mode: "idle-smile" };
}

/**
 * Pure speaking-animation decision (unit-tested).
 *
 * The loading loop runs ONLY while TTS audio is actually playing.
 * Silent `speaking` (meeting suppression, inter-chunk gaps, waits) holds
 * the current frame instead of looping over dead air. `listening` and
 * `thinking` are unaffected — mic/network feedback has no audio
 * counterpart and keeps its segments via resolveAvatarAnim.
 */
export function shouldHoldSpeakingFrame(
  st: AssistantState,
  ttsActive: boolean,
): boolean {
  return st === "speaking" && !ttsActive;
}

export function Avatar() {
  const state = useAssistant((s) => s.state);
  const visible = useAssistant((s) => s.visible);
  const ttsActive = useAssistant((s) => s.ttsActive);
  const awaitingInput = useAssistant((s) => s.awaitingInput);
  const [animationData, setAnimationData] = useState<object | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const animRef = useRef<AnimationItem | null>(null);
  const modeRef = useRef<AnimMode>("holding");

  // Load the Lottie JSON animation
  useEffect(() => {
    fetch(`${import.meta.env.BASE_URL}wakeup.json`)
      .then((res) => res.json())
      .then((data) => setAnimationData(data))
      .catch((err) => console.error("Failed to load lottie:", err));
  }, []);

  // Apply state-based animation to a given AnimationItem.
  // Called both on initial load and on state changes.
  // Guard: if the resolved mode is already active (e.g. thinking → speaking
  // are both "loading-loop"), only update speed — restarting the segment
  // on every tick is what made rapid sequences look possessed.
  function applyState(anim: AnimationItem, st: AssistantState) {
    const resolved = resolveAvatarAnim(st);
    if (modeRef.current === resolved.mode) {
      anim.setSpeed(resolved.speed);
      return;
    }
    modeRef.current = resolved.mode;
    anim.setSpeed(resolved.speed);
    anim.loop = resolved.loop;
    if (resolved.segment) {
      anim.playSegments(resolved.segment, true);
    }
  }

  // Initialize lottie animation when data is loaded
  useEffect(() => {
    if (!animationData || !containerRef.current) return;

    // Destroy previous animation if exists
    if (animRef.current) {
      animRef.current.destroy();
    }

    const anim = lottie.loadAnimation({
      container: containerRef.current,
      renderer: "svg",
      loop: true,
      autoplay: false,
      animationData,
    });
    animRef.current = anim;

    // onComplete handler — sequences wake/idle animation phases.
    // Fires only when loop=false (one-shot segments).
    const onComplete = () => {
      const a = animRef.current;
      if (!a) return;
      if (modeRef.current === "wake-loading") {
        // Loading done → play smile arrival
        modeRef.current = "wake-smile";
        a.loop = false;
        a.playSegments(SEG_SMILE_ARRIVE, true);
      } else if (modeRef.current === "wake-smile" || modeRef.current === "idle-smile") {
        // Smile arrival done → hold on stable frame
        modeRef.current = "holding";
        a.goToAndStop(FRAME_HOLD_SMILE, true);
      }
    };
    anim.addEventListener("complete", onComplete);

    // Apply current state/visible immediately after creation.
    // Handles the race condition where hotkey fires before animation loads.
    const { state: curState, visible: curVisible } = useAssistant.getState();
    applyState(anim, curState);
    if (!curVisible) {
      anim.pause();
    }

    return () => {
      anim.removeEventListener("complete", onComplete);
      anim.destroy();
      animRef.current = null;
    };
  }, [animationData]);

  // React to state AND audio-activity changes.
  // Speaking without audio holds the current frame (no looping over
  // silence); listening/thinking keep their segments (mic/network
  // feedback has no audio counterpart).
  useEffect(() => {
    if (!animRef.current) return;
    if (shouldHoldSpeakingFrame(state, ttsActive)) {
      modeRef.current = "holding";
      animRef.current.loop = false;
      animRef.current.pause();
      return;
    }
    applyState(animRef.current, state);
  }, [state, ttsActive]);

  // Play/pause animation based on visibility.
  // When hiding, delay the pause so the Lottie stays alive during the
  // 0.5s slide-down animation — a frozen frame sliding down looks dead.
  useEffect(() => {
    if (!animRef.current) return;
    if (visible) {
      animRef.current.play();
    } else {
      const t = setTimeout(() => {
        animRef.current?.pause();
      }, 500);
      return () => clearTimeout(t);
    }
  }, [visible]);

  const waiting = state === "speaking" && awaitingInput;
  return (
    <div
      data-interactive
      className={`avatar-wrap avatar-wrap--${state}${waiting ? " avatar-wrap--waiting" : ""}`}
      style={{
        width: 180,
        height: 180,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "transparent",
      }}
    >
      {animationData ? (
        <div ref={containerRef} style={{ width: 180, height: 180 }} />
      ) : (
        <div className={`orb orb--${state}`} />
      )}
    </div>
  );
}
