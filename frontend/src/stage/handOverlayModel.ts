// Pure model for the hand-mode overlay on the stage: where the virtual cursor ring goes (preview only)
// and what the one status chip says. Keep the text short: the chip sits next to the orb.
import type { HudState } from "../hand/payload";

export interface OverlayView {
  /** Ring position in CSS px, or null when no ring should show. */
  ring: { x: number; y: number } | null;
  ringClass: string;
  chip: string;
  chipClass: string;
  /** Ripple to play at the ring/cursor position for this action, if any. */
  ripple: boolean;
}

const NO_RIPPLE = new Set(["armed", "disarmed"]);

const GESTURE_LABELS: Record<string, string> = {
  next_tab: "next tab",
  prev_tab: "previous tab",
  window_switcher: "windows",
  window_next: "next window",
  window_prev: "previous window",
  window_commit: "window picked",
  window_cancel: "cancelled",
  desktop_prev: "previous desktop",
  desktop_next: "next desktop",
  task_view: "all windows",
  show_desktop: "show desktop",
};

/** Short text for a gesture command ("next tab"), or null when the action is not a gesture. */
export function gestureLabel(action: string | null): string | null {
  return action ? (GESTURE_LABELS[action] ?? null) : null;
}

/** Webcams cut their frame rate in dim light; say so when it is well under what pointing needs. */
const LOW_FPS = 15;

export function chipText(s: HudState): string {
  const dim = s.cam_fps > 0 && s.cam_fps < LOW_FPS ? " · dim room, add light" : "";
  // Covered is the most actionable (nothing will ever be tracked until it's removed); heavy
  // graphics load is next (tracking still works, just less smoothly); dim room last. They rarely
  // combine, but concatenating is harmless if they ever do.
  const covered = s.covered ? " · can't see, camera covered?" : "";
  const busy = !s.covered && s.gpu_busy ? " · heavy graphics load" : "";
  const dim2 = !s.covered ? dim : "";
  const suffix = covered + busy + dim2;
  if (!s.tracking && !s.armed && s.note && s.note.startsWith("starting")) return "HAND · starting…";
  if (s.note && s.note.startsWith("disarmed (fist-hold")) return `HAND · off · keep the fist to exit${suffix}`;
  if (s.note && s.note.startsWith("disarmed")) return `HAND · off · palm, fist, palm, fist to start${suffix}`;
  if (s.armed && s.gesture === "tabs") return `HAND · 3 fingers: flick for tabs${suffix}`;
  if (s.armed && s.gesture === "windows") return `HAND · windows: flick, drop the hand to pick${suffix}`;
  if (s.armed && s.gesture === "desktops") return `HAND · 4 fingers: flick for desktops${suffix}`;
  if (s.armed) {
    if (!s.live) return `HAND · PREVIEW${suffix}`;
    return s.tracking ? `HAND · ON${suffix}` : `HAND · ON · show your hand${suffix}`;
  }
  if (!s.tracking) return `HAND · show your hand${suffix}`;
  const n = Math.min(s.arm_stage, 3);
  return `HAND · palm, fist, palm, fist${n > 0 ? `  ${n}/3` : ""}${suffix}`;
}

export function overlayModel(
  s: HudState,
  viewport: { w: number; h: number },
  newAction: boolean,
): OverlayView {
  const sx = s.screen[0] > 0 ? viewport.w / s.screen[0] : 1;
  const sy = s.screen[1] > 0 ? viewport.h / s.screen[1] : 1;
  const preview = s.armed && !s.live;

  // The overlay covers the primary monitor only: a cursor on another monitor has no ring here.
  const ring = preview && s.tracking && s.primary ? { x: s.cursor[0] * sx, y: s.cursor[1] * sy } : null;
  const ringClass =
    "hand-ring on hand-ring--preview" + (s.dragging ? " hand-ring--drag" : s.pressed ? " hand-ring--down" : "");

  const chipClass = !s.armed
    ? "hand-chip hand-chip--idle"
    : s.live
      ? "hand-chip hand-chip--live"
      : "hand-chip hand-chip--preview";

  return {
    ring,
    ringClass,
    chip: chipText(s),
    chipClass,
    ripple: newAction && !!s.last_action && !NO_RIPPLE.has(s.last_action) && gestureLabel(s.last_action) === null,
  };
}

export type OrbDock = "top" | "bottom";

/** Gap between the chip and the screen edge (or the taskbar) in CSS px. */
const EDGE_GAP = 12;
/** Used when the work area cannot be read (taskbar hidden or on another edge). */
const TASKBAR_FALLBACK = 0;

/**
 * Where the chip goes: horizontally centred, on the edge OPPOSITE the orb so the two never crowd each
 * other. Orb at the top (the default) -> chip at the bottom, just above the taskbar; orb at the bottom ->
 * chip at the top. `taskbarBottom` is the CSS px the taskbar covers at the bottom of the stage window
 * (`screen.height - screen.availTop - screen.availHeight`), 0 when unknown.
 */
export function chipPlacement(
  dock: OrbDock,
  viewport: { w: number; h: number },
  chipW: number,
  chipH: number,
  taskbarBottom: number = TASKBAR_FALLBACK,
): { left: number; top: number } {
  const left = Math.max(0, (viewport.w - chipW) / 2);
  if (dock === "bottom") return { left, top: EDGE_GAP };
  const inset = Math.max(0, taskbarBottom) + EDGE_GAP;
  return { left, top: Math.max(0, viewport.h - chipH - inset) };
}

/** CSS px the taskbar covers at the bottom edge of the screen (0 if it is elsewhere or auto-hidden). */
export function taskbarBottomPx(scr: { height: number; availHeight: number; availTop?: number }): number {
  const covered = scr.height - (scr.availTop ?? 0) - scr.availHeight;
  return covered > 0 && covered < scr.height / 3 ? covered : 0;
}
