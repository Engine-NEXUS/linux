import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";

const ORB = 180;
const MARGIN = 24;
const SPEED_PX_S = 70;
const RESUME_AFTER_MS = 3000;
// Park anchor clearance above taskbar/dock when woken.
const PARK_BOTTOM_GAP = 96;

/** Park position: bottom-center, fully on-screen. */
function parkPosition(): { x: number; y: number } {
  return {
    x: Math.max((window.innerWidth - ORB) / 2, MARGIN),
    y: Math.max(window.innerHeight - ORB - PARK_BOTTOM_GAP, MARGIN),
  };
}

/** Random waypoint in the bottom band, keeping the full orb on-screen. */
function randomWaypoint(): { x: number; y: number } {
  const w = Math.max(window.innerWidth - ORB - MARGIN * 2, MARGIN);
  const top = Math.max(window.innerHeight - ORB - 220, MARGIN);
  const bottom = Math.max(window.innerHeight - ORB - MARGIN, top);
  return {
    x: MARGIN + Math.random() * w,
    y: top + Math.random() * Math.max(bottom - top, 1),
  };
}

export interface Roam {
  x: number;
  y: number;
  held: boolean;
  onPointerDown: (e: ReactPointerEvent) => void;
}

/**
 * Free-roam positioning for the orb inside the fullscreen stage.
 * The native window never moves (Wayland forbids it) — the orb div roams
 * via transform, waypoint to waypoint, like a desktop pet.
 *
 * NOTE: the stage window is click-through (ignore=true) while roaming, so
 * pointer events never reach the orb mid-flight — drag only engages when
 * the window is interactive (parked after wake, or once per-orb input
 * shaping lands). The drag path below stays so it works the moment the
 * window can receive events; it is inert, not dead, until then.
 *
 * @param roaming  false while woken (listening/thinking/speaking) — orb parks.
 */
// ponytail: manual drag needs per-orb input shaping (gdk input region
// tracking the orb rect) — window-wide ignore eats all clicks fullscreen.
// Upgrade = `set_orb_rect` IPC -> input_shape_combine_region, then drop
// this note and enable grab-while-roaming.
export function useRoam(roaming: boolean): Roam {
  const [pos, setPos] = useState(() => randomWaypoint());
  const [held, setHeld] = useState(false);
  const posRef = useRef(pos);
  const targetRef = useRef(randomWaypoint());
  const roamingRef = useRef(roaming);
  const heldRef = useRef(false);
  const resumeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const dragOffset = useRef<{ dx: number; dy: number } | null>(null);
  roamingRef.current = roaming;

  useEffect(() => {
    let raf = 0;
    let last = performance.now();
    const tick = (now: number) => {
      const dt = Math.min((now - last) / 1000, 0.1);
      last = now;
      if (roamingRef.current && !heldRef.current) {
        const p = posRef.current;
        const t = targetRef.current;
        const dx = t.x - p.x;
        const dy = t.y - p.y;
        const dist = Math.hypot(dx, dy);
        if (dist < 4) {
          targetRef.current = randomWaypoint();
        } else {
          const step = Math.min(dist, SPEED_PX_S * dt);
          const next = { x: p.x + (dx / dist) * step, y: p.y + (dy / dist) * step };
          posRef.current = next;
          setPos(next);
        }
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, []);

  // Pause-then-resume helper shared by drag end.
  const scheduleResume = useCallback(() => {
    if (resumeTimer.current) clearTimeout(resumeTimer.current);
    resumeTimer.current = setTimeout(() => setHeld(false), RESUME_AFTER_MS);
  }, []);
  useEffect(() => () => {
    if (resumeTimer.current) clearTimeout(resumeTimer.current);
  }, []);

  // Global move/up so dragging survives pointer leaving the orb.
  useEffect(() => {
    const move = (e: PointerEvent) => {
      if (!heldRef.current || !dragOffset.current) return;
      const next = {
        x: Math.min(Math.max(e.clientX - dragOffset.current.dx, MARGIN), window.innerWidth - ORB - MARGIN),
        y: Math.min(Math.max(e.clientY - dragOffset.current.dy, MARGIN), window.innerHeight - ORB - MARGIN),
      };
      posRef.current = next;
      targetRef.current = next;
      setPos(next);
    };
    const up = () => {
      if (!heldRef.current) return;
      dragOffset.current = null;
      scheduleResume();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
  }, [scheduleResume]);

  const onPointerDown = useCallback((e: ReactPointerEvent) => {
    if (resumeTimer.current) clearTimeout(resumeTimer.current);
    const p = posRef.current;
    dragOffset.current = { dx: e.clientX - p.x, dy: e.clientY - p.y };
    heldRef.current = true;
    setHeld(true);
  }, []);

  // `held` state mirrors heldRef (drives the held CSS class).
  useEffect(() => {
    heldRef.current = held;
  }, [held]);

  // When wake parks the orb, release drag hold and snap to park anchor.
  useEffect(() => {
    if (!roaming) {
      heldRef.current = false;
      setHeld(false);
      const park = parkPosition();
      posRef.current = park;
      targetRef.current = park;
      setPos(park);
    }
  }, [roaming]);

  return { x: pos.x, y: pos.y, held, onPointerDown };
}
