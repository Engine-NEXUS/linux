/**
 * Screen pointer marker — Phase-2 overlay (NEXUS brand styling).
 *
 * Rendered inside the fullscreen main stage; positioned with a CSS
 * transform (no native window moves — Wayland-safe). The wrapper is
 * `pointer-events: none` so the marker can NEVER eat clicks, regardless
 * of the window-level click-through state.
 */

import { clampToStage, physicalToCss } from "./pointerMath";

export interface PointerTarget {
  /** Physical screen pixels (Rust emits these). */
  x: number;
  y: number;
  label: string;
}

const RING = 26;
const CYAN = "#22d3ee";

export function PointerOverlay({
  target,
  fading,
}: {
  target: PointerTarget;
  fading: boolean;
}) {
  const stageW = typeof window !== "undefined" ? window.innerWidth : 0;
  const stageH = typeof window !== "undefined" ? window.innerHeight : 0;
  const cssX = physicalToCss(target.x);
  const cssY = physicalToCss(target.y);
  const { x, y, clamped } = clampToStage(cssX, cssY, stageW, stageH);

  return (
    <div
      data-testid="screen-pointer"
      aria-hidden="true"
      style={{
        position: "absolute",
        left: 0,
        top: 0,
        width: 0,
        height: 0,
        pointerEvents: "none",
        zIndex: 60,
        opacity: fading ? 0 : 1,
        transition: "opacity 0.4s ease",
      }}
    >
      <div
        style={{
          position: "absolute",
          transform: `translate(${x}px, ${y}px) translate(-50%, -50%)`,
        }}
      >
        {/* Pulsing ring */}
        <div
          style={{
            width: RING * 2,
            height: RING * 2,
            marginLeft: -RING,
            marginTop: -RING,
            borderRadius: "50%",
            border: `3px solid ${CYAN}`,
            boxShadow: `0 0 18px ${CYAN}, inset 0 0 8px rgba(34, 211, 238, 0.55)`,
            animation: "nexus-pointer-pulse 1.6s ease-in-out infinite",
          }}
        />
        {/* Center dot */}
        <div
          style={{
            position: "absolute",
            left: -4,
            top: -4,
            width: 8,
            height: 8,
            borderRadius: "50%",
            background: CYAN,
            boxShadow: `0 0 10px ${CYAN}`,
          }}
        />
        {/* Label chip (+ edge badge when the target was off-stage) */}
        {target.label.trim() !== "" && (
          <div
            style={{
              position: "absolute",
              left: RING + 10,
              top: -RING - 6,
              whiteSpace: "nowrap",
              fontSize: 14,
              fontWeight: 600,
              color: "#e8fbff",
              background: "rgba(8, 20, 28, 0.82)",
              border: `1px solid ${CYAN}`,
              borderRadius: 10,
              padding: "4px 12px",
              textShadow: "0 1px 2px rgba(0,0,0,0.8)",
            }}
          >
            {clamped ? `⤢ ${target.label}` : target.label}
          </div>
        )}
      </div>
      <style>{`@keyframes nexus-pointer-pulse {
        0%, 100% { transform: scale(1); opacity: 1; }
        50% { transform: scale(1.18); opacity: 0.75; }
      }`}</style>
    </div>
  );
}
