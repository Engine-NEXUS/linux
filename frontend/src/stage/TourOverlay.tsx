import { useEffect, useLayoutEffect, useRef, useState } from "react";
import "./tour.css";
import { arrowHead, physToCss, placeCallout, Placement, Rect } from "./tourGeometry";

async function invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke(cmd, args);
}

/**
 * TourOverlay — the narrated screen tour's pointer + callout, ONE at a time.
 *
 * Rust sends only the CURRENT callout (`screen:callout`) while its line is
 * audible and clears it (`screen:callout_clear`) before the next one, so
 * nothing here can run ahead of the speech: this component holds a single
 * slot and has no queue.
 *
 * CSP note (see OrbFrame.tsx): the stage window drops React `style` props,
 * so everything visual is a class, an SVG attribute or a ref + CSSOM write.
 */

interface CalloutPayload {
  request_id: string;
  idx: number;
  total: number;
  title: string;
  text: string;
  /** physical px, same contract as ghost:ring */
  x: number;
  y: number;
  width: number;
  height: number;
  has_target: boolean;
}
interface TourStartPayload {
  request_id: string;
  screen_w: number;
  screen_h: number;
}
interface OrbRectPayload {
  x: number;
  y: number;
  w: number;
  h: number;
}

const LEAVE_MS = 200;
const RING_OUTSET = 4;

export function TourOverlay() {
  const [callout, setCallout] = useState<CalloutPayload | null>(null);
  const [placed, setPlaced] = useState<{ p: Placement; target: Rect | null } | null>(null);
  const [entered, setEntered] = useState(false);
  const [leaving, setLeaving] = useState(false);

  const boxRef = useRef<HTMLDivElement | null>(null);
  const monitorRef = useRef<{ w: number; h: number } | null>(null);
  const orbRef = useRef<Rect | null>(null);
  const tourIdRef = useRef<string | null>(null);
  const leaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const cancelLeave = () => {
    if (leaveTimer.current) {
      clearTimeout(leaveTimer.current);
      leaveTimer.current = null;
    }
  };
  const wipe = () => {
    cancelLeave();
    setCallout(null);
    setPlaced(null);
    setEntered(false);
    setLeaving(false);
    void invoke("stage_set_hitboxes", { source: "tour", rects: [] }).catch(() => {});
  };
  const handleDismiss = () => {
    tourIdRef.current = null;
    wipe();
    void invoke("orchestrator_cancel").catch(() => {});
  };

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        handleDismiss();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  useEffect(() => {
    let unlisteners: (() => void)[] = [];
    let alive = true;
    const stale = (id: string) => tourIdRef.current !== null && id !== tourIdRef.current;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const subs = await Promise.all([
        listen<TourStartPayload>("screen:tour_start", (e) => {
          const p = e.payload;
          if (!p) return;
          tourIdRef.current = p.request_id;
          monitorRef.current = p.screen_w > 0 && p.screen_h > 0 ? { w: p.screen_w, h: p.screen_h } : null;
          wipe();
        }),
        listen<CalloutPayload>("screen:callout", (e) => {
          const p = e.payload;
          if (!p || stale(p.request_id)) return;
          cancelLeave();
          setPlaced(null);
          setEntered(false);
          setLeaving(false);
          setCallout(p);
        }),
        listen<{ request_id: string }>("screen:callout_clear", (e) => {
          const p = e.payload;
          if (!p || stale(p.request_id)) return;
          setLeaving(true);
          cancelLeave();
          leaveTimer.current = setTimeout(() => {
            leaveTimer.current = null;
            setCallout(null);
            setPlaced(null);
            setEntered(false);
            setLeaving(false);
          }, LEAVE_MS);
        }),
        listen<{ request_id: string }>("screen:tour_end", (e) => {
          const p = e.payload;
          if (!p || stale(p.request_id)) return;
          tourIdRef.current = null;
          wipe();
        }),
        listen<OrbRectPayload>("stage:orb_rect", (e) => {
          const r = e.payload;
          if (!r) return;
          const dpr = window.devicePixelRatio || 1;
          orbRef.current = { x: r.x / dpr, y: r.y / dpr, w: r.w / dpr, h: r.h / dpr };
        }),
      ]);
      if (!alive) subs.forEach((u) => u());
      else unlisteners = subs;
    })().catch(() => {});
    return () => {
      alive = false;
      unlisteners.forEach((u) => u());
      cancelLeave();
      void invoke("stage_set_hitboxes", { source: "tour", rects: [] }).catch(() => {});
    };
  }, []);

  // Measure the callout, place it, and start the enter transition.
  useLayoutEffect(() => {
    const el = boxRef.current;
    if (!el || !callout) return;
    const viewport = { w: window.innerWidth, h: window.innerHeight };
    const dpr = window.devicePixelRatio || 1;
    const monitor = monitorRef.current ?? { w: viewport.w * dpr, h: viewport.h * dpr };
    const target = callout.has_target
      ? physToCss({ x: callout.x, y: callout.y, w: callout.width, h: callout.height }, monitor, viewport)
      : null;
    const p = placeCallout(
      target,
      viewport,
      { w: el.offsetWidth, h: el.offsetHeight },
      orbRef.current ? [orbRef.current] : [],
    );
    el.style.transform = `translate(${p.box.x}px, ${p.box.y}px)`;
    setPlaced({ p, target });

    // Register interactive hitbox for the callout box (physical px)
    const calloutRect = {
      x: Math.round(p.box.x * dpr),
      y: Math.round(p.box.y * dpr),
      w: Math.round(el.offsetWidth * dpr),
      h: Math.round(el.offsetHeight * dpr),
    };
    void invoke("stage_set_hitboxes", { source: "tour", rects: [calloutRect] }).catch(() => {});

    const raf = requestAnimationFrame(() => setEntered(true));
    return () => cancelAnimationFrame(raf);
  }, [callout]);

  if (!callout) return null;

  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const live = entered && !leaving;
  const leader = placed?.p.leader ?? null;
  const target = placed?.target ?? null;

  return (
    <>
      <svg
        className={`tour-svg${live ? " in" : ""}`}
        width={vw}
        height={vh}
        key={`svg-${callout.idx}`}
      >
        {target && (
          <rect
            className="tour-ring"
            x={target.x - RING_OUTSET}
            y={target.y - RING_OUTSET}
            width={target.w + 2 * RING_OUTSET}
            height={target.h + 2 * RING_OUTSET}
            rx={12}
          />
        )}
        {leader && (
          <>
            <path
              className="tour-leader"
              pathLength={1}
              d={`M ${leader.from.x} ${leader.from.y} L ${leader.to.x} ${leader.to.y}`}
            />
            {target && <polygon className="tour-arrow" points={arrowHead(leader.to, leader.from)} />}
            <circle className="tour-dot" cx={leader.to.x} cy={leader.to.y} r={5} />
          </>
        )}
      </svg>
      <div
        ref={boxRef}
        key={`box-${callout.idx}`}
        className={`tour-callout${live ? " in" : ""}`}
      >
        <button
          type="button"
          className="tour-callout-close"
          onClick={handleDismiss}
          title="Dismiss tour (Esc)"
          aria-label="Close tour"
        >
          ×
        </button>
        <span className="tour-callout-num">
          {callout.idx + 1}
          <span className="tour-callout-of">/{callout.total}</span>
        </span>
        <div className="tour-callout-title">{callout.title}</div>
        <div className="tour-callout-text">{callout.text}</div>
      </div>
    </>
  );
}
