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

export function Avatar() {
  const state = useAssistant((s) => s.state);
  const visible = useAssistant((s) => s.visible);
  const [animationData, setAnimationData] = useState<object | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const animRef = useRef<AnimationItem | null>(null);
  const modeRef = useRef<AnimMode>("holding");

  // Load the Lottie JSON animation
  useEffect(() => {
    fetch("/wakeup.json")
      .then((res) => res.json())
      .then((data) => setAnimationData(data))
      .catch((err) => console.error("Failed to load lottie:", err));
  }, []);

  // Apply state-based animation to a given AnimationItem.
  // Called both on initial load and on state changes.
  function applyState(anim: AnimationItem, st: AssistantState) {
    const speed: Record<AssistantState, number> = {
      idle: 1.0,
      listening: 1.5,
      thinking: 1.5,
      speaking: 1.2,
    };
    anim.setSpeed(speed[st]);

    if (st === "listening") {
      // Wake sequence: loading (~1s at 1.5x) → smile arrives → hold
      modeRef.current = "wake-loading";
      anim.loop = false;
      anim.playSegments(SEG_LOADING, true);
    } else if (st === "thinking" || st === "speaking") {
      // Loading circles loop continuously
      modeRef.current = "loading-loop";
      anim.loop = true;
      anim.playSegments(SEG_LOADING, true);
    } else {
      // idle: smile arrives → hold
      modeRef.current = "idle-smile";
      anim.loop = false;
      anim.playSegments(SEG_SMILE_ARRIVE, true);
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

    // Apply current state immediately after creation.
    // Handles the race condition where hotkey fires before animation loads.
    const { state: curState } = useAssistant.getState();
    applyState(anim, curState);

    return () => {
      anim.removeEventListener("complete", onComplete);
      anim.destroy();
      animRef.current = null;
    };
  }, [animationData]);

  // React to state changes — apply correct segment/speed/mode.
  useEffect(() => {
    if (!animRef.current) return;
    applyState(animRef.current, state);
  }, [state]);

  // Fullscreen stage: the orb is always on-screen (roaming when idle),
  // so the Lottie always plays. No play/pause on visibility — visibility
  // only switches animation segments via applyState above.
  useEffect(() => {
    animRef.current?.play();
  }, [visible]);

  return (
    <div
      data-interactive
      className={`avatar-wrap avatar-wrap--${state}`}
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
