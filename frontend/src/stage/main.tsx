import React, { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import "./ghost.css";
import "../styles.css";
import AnnotationCanvas from "./AnnotationCanvas";
import { OrbFrame } from "./OrbFrame";
import { LoadingIndicator } from "./LoadingIndicator";
import { ResponseCaption } from "./ResponseCaption";
import { LiveCaption } from "./LiveCaption";
import { TourOverlay } from "./TourOverlay";
import { HandOverlay } from "./HandOverlay";
import { initThemeSync } from "../sidebar/theme";
import { ScreenPointerLayer } from "../overlay/PointerOverlay";
import { isLinuxRuntime } from "../platform";

initThemeSync(); // Light/Dark follows the Command Hub setting (orb, callouts)

async function invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke(cmd, args);
}

// ─── Feature 86: Spatial annotation overlay ─────────────────────────

/** One pin: box + pin badge, PHYSICAL px from Rust (divide by dpr). */
interface SpatialPin {
  id: number;
  label: string;
  x: number;
  y: number;
  width: number;
  height: number;
  pin_x: number;
  pin_y: number;
}

/** Truncate a pin label for the floating badge (≤18 chars). */
function pinLabel(label: string): string {
  const t = label.trim();
  if (!t) return "Element";
  return t.length > 18 ? `${t.slice(0, 17)}…` : t;
}

/**
 * SpatialAnnotationLayer — glowing liquid-glass bounding boxes with
 * numbered pins + SVG leader lines drawn over the user's REAL screen
 * content. 100% click-through except over pin hitboxes (registered in
 * Rust via stage_set_hitboxes so the cursor exception applies there).
 */
function SpatialAnnotationLayer() {
  const [pins, setPins] = useState<SpatialPin[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);

  // Rust dispatches annotations (physical px) + hover highlights from
  // the sidebar cards + clearing (empty pins array).
  useEffect(() => {
    let unlisteners: (() => void)[] = [];
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");

      const u1 = await listen<{ title?: string; pins?: SpatialPin[] }>(
        "stage:spatial_annotations",
        (event) => {
          const list = event.payload?.pins ?? [];
          setPins(list);
          setActiveId(null);
          // Register pin hitboxes (physical px) so the cursor exception
          // lets the user click pins; everything else stays click-through.
          // Empty list = fully click-through again.
          const dpr = window.devicePixelRatio || 1;
          const rects = list.map((p) => ({
            x: Math.round((p.pin_x - 6) * dpr),
            y: Math.round((p.pin_y - 4) * dpr),
            w: Math.round(96 * dpr),
            h: Math.round(26 * dpr),
          }));
          invoke("stage_set_hitboxes", { source: "spatial", rects }).catch(() => {});
        },
      );
      const u2 = await listen<{ id?: number | null }>("stage:highlight_item", (event) => {
        const id = event.payload?.id;
        setActiveId(typeof id === "number" ? id : null);
      });
      unlisteners = [u1, u2];
    })().catch(() => {});
    return () => {
      unlisteners.forEach((u) => u());
      invoke("stage_set_hitboxes", { source: "spatial", rects: [] }).catch(() => {});
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setPins([]);
        setActiveId(null);
        invoke("stage_set_hitboxes", { source: "spatial", rects: [] }).catch(() => {});
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Pin click → activate the matching card in the sidebar.
  const handlePinClick = (id: number) => {
    // A lost focus event = dead pin with no other signal — must surface.
    void import("../ipc").then(({ emitLogged }) =>
      emitLogged("sidebar:focus_spatial_card", { id }).catch(() => {}),
    );
    setActiveId(id);
  };

  const dpr = window.devicePixelRatio || 1;
  const css = (n: number) => n / dpr;

  return (
    <>
      {/* SVG leader lines under the badges */}
      <svg
        style={{
          position: "fixed", inset: 0, width: "100vw", height: "100vh",
          pointerEvents: "none", zIndex: 99998,
        }}
      >
        {pins.map((p) => {
          const x1 = css(p.pin_x) + 14;
          const y1 = css(p.pin_y) + 24;
          const x2 = css(p.x) + css(p.width) - 18;
          const y2 = css(p.y) + 4;
          return (
            <path
              key={`leader-${p.id}`}
              className={`spatial-leader-line ${activeId === p.id ? "spatial-leader-line--active" : ""}`}
              d={`M ${x1} ${y1} L ${x2} ${y1} L ${x2} ${y2}`}
            />
          );
        })}
      </svg>

      {/* Glowing bounding boxes — positioned via CSSOM assignment (CSP drops React style prop) */}
      {pins.map((p) => (
        <div
          key={`box-${p.id}`}
          className={`spatial-box ${activeId === p.id ? "spatial-box--active" : ""}`}
          ref={(el) => {
            if (el) {
              el.style.position = "fixed";
              el.style.left = `${css(p.x)}px`;
              el.style.top = `${css(p.y)}px`;
              el.style.width = `${css(p.width)}px`;
              el.style.height = `${css(p.height)}px`;
            }
          }}
        />
      ))}

      {/* Numbered floating pin badges (clickable — cursor exception) */}
      {pins.map((p) => (
        <button
          key={`pin-${p.id}`}
          type="button"
          className={`spatial-pin ${activeId === p.id ? "spatial-pin--active" : ""}`}
          ref={(el) => {
            if (el) {
              el.style.position = "fixed";
              el.style.left = `${css(p.pin_x)}px`;
              el.style.top = `${css(p.pin_y)}px`;
            }
          }}
          onClick={() => handlePinClick(p.id)}
          title={p.label}
        >
          <span className="spatial-pin-num">[{p.id}]</span>
          <span className="spatial-pin-label">{pinLabel(p.label)}</span>
        </button>
      ))}
    </>
  );
}

/**
 * Stage shell (step 1: parallel-run, overlay host).
 *
 * Fullscreen transparent window hosting:
 * 1. Ghost Ring — follows user's commanded cursor during active automation.
 * 2. Clicky Guidance Pointer — energetic visual arrow pointing at detected screen elements.
 * 3. Clicky Guidance Speech Bubble — liquid-glass contextual bubble with auto-edge flip.
 */
function StageApp() {
  const ringRef = useRef<HTMLDivElement | null>(null);
  const pointerRef = useRef<HTMLDivElement | null>(null);
  const bubbleRef = useRef<HTMLDivElement | null>(null);
  const [guideLabel, setGuideLabel] = useState<string>("");
  const autoHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    console.log(
      `[ANIM] Stage overlay mounted: ${window.innerWidth}x${window.innerHeight} (DPR: ${window.devicePixelRatio || 1})`
    );
    let alive = true;
    let timer: ReturnType<typeof setInterval> | null = null;
    const start = setTimeout(() => {
      if (!alive) return;
      invoke("stage_heartbeat", { client: "stage-shell-v1" }).catch(() => {});
      timer = setInterval(() => {
        invoke("stage_heartbeat", { client: "stage-shell-v1" }).catch(() => {});
      }, 2000);
    }, 500);
    return () => {
      alive = false;
      clearTimeout(start);
      if (timer) clearInterval(timer);
    };
  }, []);

  // Ghost ring: Rust pushes physical-px cursor positions (~30Hz, only on change).
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen<{ x?: number; y?: number; visible?: boolean }>(
        "ghost:ring",
        (event) => {
          const el = ringRef.current;
          if (!el) return;
          const { x, y, visible } = event.payload ?? {};
          if (!visible || x === undefined || y === undefined) {
            el.classList.remove("on");
            return;
          }
          const scale = window.devicePixelRatio || 1;
          el.style.transform = `translate(${x / scale - 19}px, ${y / scale - 19}px)`;
          el.classList.add("on");
        },
      ).then((fn) => {
        unlisten = fn;
      }).catch(() => {}),
    );
    return () => {
      unlisten?.();
    };
  }, []);

  // Clicky Guidance Pointer & Bubble: Rust emits "ghost:point" with target coordinates & guidance text.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen<{ x?: number; y?: number; label?: string; duration_ms?: number; visible?: boolean }>(
        "ghost:point",
        (event) => {
          const pointer = pointerRef.current;
          const bubble = bubbleRef.current;
          if (!pointer || !bubble) return;

          const { x, y, label, duration_ms, visible } = event.payload ?? {};
          if (autoHideTimerRef.current) {
            clearTimeout(autoHideTimerRef.current);
            autoHideTimerRef.current = null;
          }

          if (!visible || x === undefined || y === undefined) {
            pointer.classList.remove("on");
            bubble.classList.remove("on");
            setGuideLabel("");
            return;
          }

          const scale = window.devicePixelRatio || 1;
          const targetX = x / scale;
          const targetY = y / scale;

          // Position the pointer centered right above the target
          pointer.style.transform = `translate(${targetX - 12}px, ${targetY - 24}px)`;
          pointer.classList.add("on");

          // Compute Clicky-style auto-edge-flipping bubble coordinates
          if (label && label.trim().length > 0) {
            setGuideLabel(label.trim());
            const bubbleW = 280;
            const bubbleH = 50;

            let bx = targetX + 22;
            let by = targetY + 6;

            // Flip to left if right edge overflows
            if (bx + bubbleW > window.innerWidth) {
              bx = targetX - bubbleW - 22;
            }

            // Flip to above if bottom edge overflows
            if (by + bubbleH > window.innerHeight) {
              by = targetY - bubbleH - 12;
            }

            // Boundary clamps
            bx = Math.max(12, Math.min(bx, window.innerWidth - bubbleW - 12));
            by = Math.max(12, Math.min(by, window.innerHeight - bubbleH - 12));

            bubble.style.transform = `translate(${bx}px, ${by}px)`;
            bubble.classList.add("on");
          } else {
            bubble.classList.remove("on");
            setGuideLabel("");
          }

          // Auto-hide after duration
          const ttl = duration_ms && duration_ms > 0 ? duration_ms : 4000;
          autoHideTimerRef.current = setTimeout(() => {
            pointer.classList.remove("on");
            bubble.classList.remove("on");
            setGuideLabel("");
          }, ttl);
        },
      ).then((fn) => {
        unlisten = fn;
      }).catch(() => {}),
    );
    return () => {
      unlisten?.();
      if (autoHideTimerRef.current) clearTimeout(autoHideTimerRef.current);
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (bubbleRef.current) bubbleRef.current.classList.remove("on");
        if (pointerRef.current) pointerRef.current.classList.remove("on");
        setGuideLabel("");
        invoke("orchestrator_cancel").catch(() => {});
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div id="stage-root" style={{ width: "100vw", height: "100vh", background: "transparent" }}>
      <div id="ghost-ring" ref={ringRef} />
      <div id="ghost-pointer" ref={pointerRef}>
        <div id="ghost-pointer-arrow" />
      </div>
      <div id="ghost-guide-bubble" ref={bubbleRef}>
        {guideLabel}
      </div>
      <SpatialAnnotationLayer />
      <TourOverlay />
      {/* Linux: the "point at X" marker. Windows shows its ghost pointer instead. */}
      {isLinuxRuntime() && <ScreenPointerLayer />}
      <HandOverlay />
      <AnnotationCanvas />
      <OrbFrame />
      <LoadingIndicator />
      <ResponseCaption />
      <LiveCaption />
    </div>
  );
}

// Dev-only preview harness: `stage.html?eyeball` in a normal browser (see
// eyeball/EyeballPanel.tsx). `import.meta.env.DEV` is a build-time constant,
// so production builds drop this import and the harness chunk entirely.
const EyeballPanel =
  import.meta.env.DEV && new URLSearchParams(window.location.search).has("eyeball")
    ? React.lazy(() => import("../eyeball/EyeballPanel").then((m) => ({ default: m.EyeballPanel })))
    : null;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <StageApp />
    {EyeballPanel && (
      <React.Suspense fallback={null}>
        <EyeballPanel />
      </React.Suspense>
    )}
  </React.StrictMode>,
);
