/**
 * Screen pointer marker, drawn inside the fullscreen stage.
 *
 * Positioned with a CSS transform (no native window moves, so it is
 * Wayland-safe) and `pointer-events: none`, so the marker can never eat a
 * click whatever the window's input region is.
 *
 * CSP note (see stage/OrbFrame.tsx): the stage window drops React `style`
 * props, so everything visual is a class in pointer.css and the position is
 * a ref + CSSOM write.
 */

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import "./pointer.css";
import { clampToStage, physicalToCss } from "./pointerMath";
import { createPointerLifecycle, type PointerView } from "./pointerLifecycle";
import { watchPointer } from "../net/pointer";

export interface PointerTarget {
  /** Physical screen pixels (Rust emits these). */
  x: number;
  y: number;
  label: string;
}

export function PointerOverlay({ target, fading }: { target: PointerTarget; fading: boolean }) {
  const anchorRef = useRef<HTMLDivElement | null>(null);
  const stageW = typeof window !== "undefined" ? window.innerWidth : 0;
  const stageH = typeof window !== "undefined" ? window.innerHeight : 0;
  const { x, y, clamped } = clampToStage(physicalToCss(target.x), physicalToCss(target.y), stageW, stageH);

  useLayoutEffect(() => {
    const el = anchorRef.current;
    if (el) el.style.transform = `translate(${x}px, ${y}px)`;
  }, [x, y]);

  const label = target.label.trim();
  return (
    <div
      data-testid="screen-pointer"
      aria-hidden="true"
      className={fading ? "screen-pointer screen-pointer--fading" : "screen-pointer"}
    >
      <div className="screen-pointer__anchor" ref={anchorRef}>
        <div className="screen-pointer__ring" />
        <div className="screen-pointer__dot" />
        {label !== "" && <div className="screen-pointer__label">{clamped ? `⤢ ${label}` : label}</div>}
      </div>
    </div>
  );
}

/**
 * The stage's pointer layer: Rust's `pointer:show` / `pointer:hide` drive
 * it, the marker fades after its dwell, and a new turn (`orb:wake`,
 * `stt:transcript`) or Esc hides it at once. Display-only.
 */
export function ScreenPointerLayer() {
  const [view, setView] = useState<PointerView>({ target: null, fading: false });

  useEffect(() => {
    const life = createPointerLifecycle(setView);
    const disposers: (() => void)[] = [];
    let alive = true;
    const keep = (un: () => void) => {
      if (alive) disposers.push(un);
      else un();
    };

    void watchPointer(
      (p) => life.show(p),
      () => life.hide(),
    ).then(keep);
    void import("@tauri-apps/api/event")
      .then(async ({ listen }) => {
        keep(await listen("orb:wake", () => life.hide()));
        keep(await listen("stt:transcript", () => life.hide()));
      })
      .catch(() => {
        /* outside Tauri */
      });

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") life.hide();
    };
    window.addEventListener("keydown", onKey);

    return () => {
      alive = false;
      window.removeEventListener("keydown", onKey);
      disposers.forEach((un) => un());
      life.dispose();
    };
  }, []);

  if (!view.target) return null;
  return <PointerOverlay target={view.target} fading={view.fading} />;
}
