import { useEffect, useRef } from "react";
import "./hand-overlay.css";
import type { HudState } from "../hand/payload";
import { useAssistant } from "../store/assistant";
import { chipPlacement, gestureLabel, overlayModel, taskbarBottomPx } from "./handOverlayModel";

/**
 * Hand mode on the stage: ONE short status chip centred on the edge opposite the orb, a virtual cursor ring
 * (PREVIEW only; in live mode the real cursor moves) and a click ripple. Driven by `hand:state`
 * (Rust emits it ~20 times a second while frames arrive) and `hand:stopped`.
 *
 * CSP note (see OrbFrame.tsx): the stage drops React `style` props, so all visuals are classes and
 * ref + CSSOM writes. Everything is pointer-events: none (the stage stays click-through).
 */
export function HandOverlay() {
  const ringRef = useRef<HTMLDivElement | null>(null);
  const rippleRef = useRef<HTMLDivElement | null>(null);
  const chipRef = useRef<HTMLDivElement | null>(null);
  const lastState = useRef<HudState | null>(null);

  useEffect(() => {
    let lastSeq = 0;
    let lastBeat = 0;
    let flashText = "";
    let flashUntil = 0;
    let disposed = false;
    const unlisten: Array<() => void> = [];

    const hide = () => {
      ringRef.current?.setAttribute("class", "hand-ring");
      chipRef.current?.setAttribute("class", "hand-chip");
    };

    const placeChip = () => {
      const chip = chipRef.current;
      if (!chip) return;
      const box = chip.getBoundingClientRect();
      const p = chipPlacement(
        useAssistant.getState().orbPosition,
        { w: window.innerWidth, h: window.innerHeight },
        box.width,
        box.height || 28,
        taskbarBottomPx(window.screen as unknown as { height: number; availHeight: number; availTop?: number }),
      );
      chip.style.left = `${p.left}px`;
      chip.style.top = `${p.top}px`;
    };

    const onState = (s: HudState) => {
      lastBeat = Date.now();
      lastState.current = s;
      const newAction = s.action_seq !== lastSeq;
      lastSeq = s.action_seq;
      const v = overlayModel(s, { w: window.innerWidth, h: window.innerHeight }, newAction);
      const ring = ringRef.current;
      const chip = chipRef.current;
      const label = newAction ? gestureLabel(s.last_action) : null;
      if (label) {
        flashText = `HAND · ${label}${s.live ? "" : " (preview)"}`;
        flashUntil = Date.now() + 1200;
      }
      if (chip) {
        chip.textContent = Date.now() < flashUntil ? flashText : v.chip;
        chip.setAttribute("class", v.chipClass + " on");
        placeChip();
      }
      if (ring) {
        if (v.ring) {
          ring.style.transform = `translate3d(${v.ring.x - 22}px, ${v.ring.y - 22}px, 0)`;
          ring.setAttribute("class", v.ringClass);
        } else {
          ring.setAttribute("class", "hand-ring");
        }
      }
      const ripple = rippleRef.current;
      if (ripple && v.ripple) {
        const sx = s.screen[0] > 0 ? window.innerWidth / s.screen[0] : 1;
        const sy = s.screen[1] > 0 ? window.innerHeight / s.screen[1] : 1;
        ripple.style.transform = `translate3d(${s.cursor[0] * sx - 30}px, ${s.cursor[1] * sy - 30}px, 0)`;
        ripple.setAttribute("class", "hand-ripple");
        void ripple.getBoundingClientRect(); // restart the animation on repeats
        ripple.setAttribute("class", "hand-ripple on");
      }
    };

    void import("@tauri-apps/api/event").then(async ({ listen }) => {
      const a = await listen<HudState>("hand:state", (e) => onState(e.payload));
      const b = await listen("hand:stopped", hide);
      const c = useAssistant.subscribe((st, prev) => {
        if (st.orbPosition !== prev.orbPosition) placeChip();
      });
      if (disposed) {
        a();
        b();
        c();
      } else {
        unlisten.push(a, b, c);
      }
    });

    // The camera window can die without a goodbye: no state for 2.5 s hides everything.
    const beat = window.setInterval(() => {
      if (lastBeat && Date.now() - lastBeat > 2500) {
        lastBeat = 0;
        hide();
      }
    }, 1000);

    return () => {
      disposed = true;
      window.clearInterval(beat);
      unlisten.forEach((u) => u());
    };
  }, []);

  return (
    <>
      <div className="hand-ring" ref={ringRef} />
      <div className="hand-ripple" ref={rippleRef} />
      <div className="hand-chip" ref={chipRef} />
    </>
  );
}
