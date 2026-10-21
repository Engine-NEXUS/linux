// ─── Feature 87: screen annotation model (pure, unit-tested) ──────
// All coordinates are PHYSICAL px (same contract as ghost:ring and
// spatial pins: frontend divides by dpr to render, Rust compares raw).
// The hitbox builders take NO dpr argument by design — callers pass
// physical coords straight through (regression guard for the
// stage/main.tsx double-dpr pin bug).

export type AnnotTool = "select" | "arrow" | "text" | "freehand" | "checkbox";

export type ElKind = "stroke" | "arrow" | "text" | "checkbox";

export interface AnnotPoint {
  x: number;
  y: number;
}

export interface AnnotBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface AnnotElement {
  id: string;
  kind: ElKind;
  /** stroke + arrow polylines (arrow: exactly [start, end]). Physical px. */
  points?: AnnotPoint[];
  /** text + checkbox rects. Physical px. */
  box?: AnnotBox;
  text?: string;
  checked?: boolean;
  color: string;
}

export interface AnnotState {
  elements: AnnotElement[];
  past: AnnotElement[][];
  future: AnnotElement[][];
  tool: AnnotTool;
  color: string;
  /** Capture mode: fullscreen hitbox, all pointer events come to stage. */
  active: boolean;
  seq: number;
}

export const initialAnnotState: AnnotState = {
  elements: [],
  past: [],
  future: [],
  tool: "select",
  color: "#259ED6",
  active: false,
  seq: 0,
};

export type AnnotAction =
  | { type: "mode"; active: boolean }
  | { type: "tool"; tool: AnnotTool }
  | { type: "color"; color: string }
  | { type: "add"; el: Omit<AnnotElement, "id"> }
  | { type: "toggle"; id: string }
  | { type: "move"; id: string; dx: number; dy: number }
  | { type: "setText"; id: string; text: string }
  | { type: "remove"; id: string }
  | { type: "seed"; elements: AnnotElement[] }
  | { type: "append"; elements: AnnotElement[] }
  | { type: "undo" }
  | { type: "redo" }
  | { type: "clear" };

const MAX_HISTORY = 50;

function pushHistory(s: AnnotState): AnnotState {
  const past = [...s.past, s.elements].slice(-MAX_HISTORY);
  return { ...s, past, future: [] };
}

function shiftPoints(el: AnnotElement, dx: number, dy: number): AnnotElement {
  const out = { ...el };
  if (out.points) out.points = out.points.map((p) => ({ x: p.x + dx, y: p.y + dy }));
  if (out.box) out.box = { ...out.box, x: out.box.x + dx, y: out.box.y + dy };
  return out;
}

export function annotationReducer(s: AnnotState, a: AnnotAction): AnnotState {
  switch (a.type) {
    case "mode":
      return { ...s, active: a.active };
    case "tool":
      return { ...s, tool: a.tool };
    case "color":
      return { ...s, color: a.color };
    case "add": {
      const h = pushHistory(s);
      const el: AnnotElement = { ...a.el, id: `a${s.seq + 1}` };
      return { ...h, elements: [...h.elements, el], seq: s.seq + 1 };
    }
    case "toggle": {
      const h = pushHistory(s);
      return {
        ...h,
        elements: h.elements.map((el) =>
          el.id === a.id && el.kind === "checkbox" ? { ...el, checked: !el.checked } : el,
        ),
      };
    }
    case "move": {
      const h = pushHistory(s);
      return { ...h, elements: h.elements.map((el) => (el.id === a.id ? shiftPoints(el, a.dx, a.dy) : el)) };
    }
    case "setText": {
      const h = pushHistory(s);
      return {
        ...h,
        elements: h.elements.map((el) => (el.id === a.id ? { ...el, text: a.text } : el)),
      };
    }
    case "remove": {
      const h = pushHistory(s);
      return { ...h, elements: h.elements.filter((el) => el.id !== a.id) };
    }
    case "seed": {
      const h = pushHistory(s);
      const maxSeq = a.elements.reduce((m, el) => {
        const n = parseInt(el.id.replace(/^a/, ""), 10);
        return Number.isFinite(n) ? Math.max(m, n) : m;
      }, 0);
      return { ...h, elements: [...a.elements], seq: Math.max(s.seq, maxSeq) };
    }
    case "append": {
      const h = pushHistory(s);
      let seq = s.seq;
      // Reassign ids so imported elements can never collide.
      const fresh = a.elements.map((el) => ({ ...el, id: `a${(seq += 1)}` }));
      return { ...h, elements: [...h.elements, ...fresh], seq };
    }
    case "undo": {
      if (s.past.length === 0) return s;
      const prev = s.past[s.past.length - 1];
      return { ...s, past: s.past.slice(0, -1), future: [s.elements, ...s.future], elements: prev };
    }
    case "redo": {
      if (s.future.length === 0) return s;
      const [next, ...rest] = s.future;
      return { ...s, past: [...s.past, s.elements], future: rest, elements: next };
    }
    case "clear": {
      if (s.elements.length === 0) return s;
      return { ...pushHistory(s), elements: [] };
    }
  }
}

/** Bounding box of an element in PHYSICAL px (6px interaction pad). */
export function elementHitbox(el: AnnotElement): { x: number; y: number; w: number; h: number } | null {
  const pad = 6;
  if (el.box) {
    return {
      x: Math.round(el.box.x - pad),
      y: Math.round(el.box.y - pad),
      w: Math.round(el.box.w + pad * 2),
      h: Math.round(el.box.h + pad * 2),
    };
  }
  if (el.points && el.points.length > 0) {
    const xs = el.points.map((p) => p.x);
    const ys = el.points.map((p) => p.y);
    const x0 = Math.min(...xs);
    const y0 = Math.min(...ys);
    return {
      x: Math.round(x0 - pad),
      y: Math.round(y0 - pad),
      w: Math.round(Math.max(...xs) - x0 + pad * 2),
      h: Math.round(Math.max(...ys) - y0 + pad * 2),
    };
  }
  return null;
}

/** CSS px converter (physical → CSS). Render-only helper. */
export function cssPx(n: number): number {
  const dpr = typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1;
  return n / dpr;
}
