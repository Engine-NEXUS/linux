import React, { useEffect, useReducer, useRef, useState } from "react";
import {
  annotationReducer,
  cssPx,
  elementHitbox,
  initialAnnotState,
  type AnnotElement,
  type AnnotTool,
} from "./annotationTypes";

async function invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  // Logged boundary (log-completeness P2): hitbox registration failure
  // means dead click regions with no other signal — it must surface.
  const { invokeLogged } = await import("../ipc");
  return invokeLogged(cmd, args);
}

const phys = (client: number) => client * (window.devicePixelRatio || 1);

/**
 * AnnotationCanvas (Feature 87) — freehand ink layer over the real screen.
 *
 * Two modes:
 * - capture (active): fullscreen hitbox, every pointer event reaches the
 *   canvas (draw arrows/strokes, place text + checkboxes).
 * - view (inactive): only element bboxes registered — checkboxes stay
 *   toggleable, everything else is click-through.
 * All coords PHYSICAL px (no dpr multiplication on hitboxes).
 */
export default function AnnotationCanvas() {
  const [state, dispatch] = useReducer(annotationReducer, initialAnnotState);
  const [draft, setDraft] = useState<Array<{ x: number; y: number }> | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const editRef = useRef<HTMLInputElement | null>(null);
  const dragRef = useRef<{ id: string; sx: number; sy: number } | null>(null);
  const stateRef = useRef(state);
  stateRef.current = state;

  // ── Event wiring ──────────────────────────────────────────────
  useEffect(() => {
    let unlisteners: (() => void)[] = [];
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");

      const u1 = await listen<{ active?: boolean }>("stage:annotation_mode", (e) => {
        dispatch({ type: "mode", active: e.payload?.active !== false });
      });
      const u2 = await listen<{ tool?: AnnotTool; color?: string }>("stage:tool_change", (e) => {
        if (e.payload?.tool) dispatch({ type: "tool", tool: e.payload.tool });
        if (e.payload?.color) dispatch({ type: "color", color: e.payload.color });
        dispatch({ type: "mode", active: true });
      });
      const u3 = await listen<{ elements?: AnnotElement[] }>("stage:annotation_begin", (e) => {
        if (Array.isArray(e.payload?.elements)) {
          dispatch({ type: "seed", elements: e.payload.elements ?? [] });
        }
        dispatch({ type: "mode", active: true });
      });
      const u4 = await listen("stage:annotation_clear", () => dispatch({ type: "clear" }));
      const u5 = await listen("stage:annotation_commit_request", () => {
        // Commit loss = user's drawing vanishes silently — must surface.
        void import("../ipc").then(({ emitLogged }) =>
          emitLogged("stage:annotation_commit", { elements: stateRef.current.elements }).catch(() => {}),
        );
      });
      const u6 = await listen("stage:annotation_done", () => dispatch({ type: "mode", active: false }));
      const u7 = await listen("stage:annotation_undo", () => dispatch({ type: "undo" }));
      const u8 = await listen("stage:annotation_redo", () => dispatch({ type: "redo" }));
      const u9 = await listen<{ elements?: AnnotElement[] }>("stage:annotation_append", (e) => {
        if (Array.isArray(e.payload?.elements)) dispatch({ type: "append", elements: e.payload.elements ?? [] });
      });
      unlisteners = [u1, u2, u3, u4, u5, u6, u7, u8, u9];
    })().catch(() => {});
    return () => {
      unlisteners.forEach((u) => u());
    };
  }, []);

  // ── Hitboxes: fullscreen in capture, element bboxes in view ──
  useEffect(() => {
    const dpr = window.devicePixelRatio || 1;
    const rects = state.active
      ? [{ x: 0, y: 0, w: Math.round(window.innerWidth * dpr), h: Math.round(window.innerHeight * dpr) }]
      : state.elements.map(elementHitbox).filter((r): r is NonNullable<typeof r> => r !== null);
    invoke("stage_set_hitboxes", { rects }).catch(() => {});
  }, [state.active, state.elements]);

  // ── Esc exits capture (keeps ink visible + interactive) ──────
  useEffect(() => {
    if (!state.active) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        dispatch({ type: "mode", active: false });
        void import("../ipc").then(({ emitLogged }) =>
          emitLogged("stage:annotation_done", {}).catch(() => {}),
        );
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [state.active]);

  useEffect(() => {
    if (editingId) editRef.current?.focus();
  }, [editingId]);

  // ── Pointer drawing ───────────────────────────────────────────
  const tool = state.tool;
  const color = state.color;

  const onPointerDown = (e: React.PointerEvent<SVGSVGElement>) => {
    if (!state.active) return;
    const x = phys(e.clientX);
    const y = phys(e.clientY);
    if (tool === "freehand") setDraft([{ x, y }]);
    else if (tool === "arrow") setDraft([{ x, y }]);
    else if (tool === "checkbox")
      dispatch({ type: "add", el: { kind: "checkbox", box: { x: x - 11, y: y - 11, w: 22, h: 22 }, checked: false, color } });
    else if (tool === "text") {
      dispatch({
        type: "add",
        el: { kind: "text", box: { x, y, w: 220, h: 34 }, text: "", color },
      });
      const id = `a${stateRef.current.seq + 1}`;
      setEditingId(id);
    } else if (tool === "select") {
      const hit = [...stateRef.current.elements].reverse().find((el) => {
        const hb = elementHitbox(el);
        return hb !== null && x >= hb.x && x <= hb.x + hb.w && y >= hb.y && y <= hb.y + hb.h;
      });
      if (hit) dragRef.current = { id: hit.id, sx: x, sy: y };
    }
  };

  const onPointerMove = (e: React.PointerEvent<SVGSVGElement>) => {
    if (!state.active || !draft) return;
    if (tool === "freehand") setDraft([...draft, { x: phys(e.clientX), y: phys(e.clientY) }]);
    else if (tool === "arrow") setDraft([draft[0], { x: phys(e.clientX), y: phys(e.clientY) }]);
  };

  const onPointerUp = (e: React.PointerEvent<SVGSVGElement>) => {
    // Select-drag: click (<3px) toggles/focuses, drag moves the element.
    if (dragRef.current) {
      const { id, sx, sy } = dragRef.current;
      dragRef.current = null;
      const dx = phys(e.clientX) - sx;
      const dy = phys(e.clientY) - sy;
      const el = stateRef.current.elements.find((el) => el.id === id);
      if (!el) return;
      if (Math.hypot(dx, dy) < 3 * (window.devicePixelRatio || 1)) {
        if (el.kind === "checkbox") dispatch({ type: "toggle", id });
        else if (el.kind === "text") setEditingId(id);
      } else {
        dispatch({ type: "move", id, dx: Math.round(dx), dy: Math.round(dy) });
      }
      return;
    }
    if (!draft) return;
    if (tool === "freehand" && draft.length > 1)
      dispatch({ type: "add", el: { kind: "stroke", points: draft, color: stateRef.current.color } });
    else if (tool === "arrow" && draft.length === 2)
      dispatch({ type: "add", el: { kind: "arrow", points: draft, color: stateRef.current.color } });
    setDraft(null);
  };

  const commitEdit = (id: string, text: string) => {
    const el = stateRef.current.elements.find((el) => el.id === id);
    if (el && (el.text ?? "") === "" && text.trim() === "") dispatch({ type: "remove", id });
    else dispatch({ type: "setText", id, text });
    setEditingId(null);
  };

  if (!state.active && state.elements.length === 0) return null;

  const preview = draft && draft.length > 0 ? draft : null;

  return (
    <>
      <svg
        className="annot-layer"
        style={{ pointerEvents: state.active ? "auto" : "none" }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
      >
        <defs>
          <marker id="annot-arrowhead" markerWidth="10" markerHeight="8" refX="8" refY="4" orient="auto">
            <path d="M 0 0 L 10 4 L 0 8 z" className="annot-arrowhead" />
          </marker>
        </defs>
        {state.elements.map((el) => {
          if (el.kind === "stroke" && el.points) {
            const d = el.points.map((p, i) => `${i === 0 ? "M" : "L"} ${cssPx(p.x)} ${cssPx(p.y)}`).join(" ");
            return <path key={el.id} className="annot-stroke" d={d} style={{ stroke: el.color }} />;
          }
          if (el.kind === "arrow" && el.points && el.points.length === 2) {
            const [a, b] = el.points;
            return (
              <line
                key={el.id} className="annot-arrow"
                x1={cssPx(a.x)} y1={cssPx(a.y)} x2={cssPx(b.x)} y2={cssPx(b.y)}
                style={{ stroke: el.color }} markerEnd="url(#annot-arrowhead)"
              />
            );
          }
          if (el.kind === "checkbox" && el.box) {
            return (
              <g key={el.id} className="annot-checkbox" onClick={() => dispatch({ type: "toggle", id: el.id })}>
                <rect x={cssPx(el.box.x)} y={cssPx(el.box.y)} width={cssPx(el.box.w)} height={cssPx(el.box.h)} rx={5} />
                {el.checked && (
                  <path
                    d={`M ${cssPx(el.box.x + 5)} ${cssPx(el.box.y + 11)} L ${cssPx(el.box.x + 10)} ${cssPx(el.box.y + 16)} L ${cssPx(el.box.x + 17)} ${cssPx(el.box.y + 6)}`}
                  />
                )}
              </g>
            );
          }
          return null;
        })}
        {preview && tool === "freehand" && (
          <path
            className="annot-stroke annot-preview"
            d={preview.map((p, i) => `${i === 0 ? "M" : "L"} ${cssPx(p.x)} ${cssPx(p.y)}`).join(" ")}
          />
        )}
        {preview && tool === "arrow" && preview.length === 2 && (
          <line
            className="annot-arrow annot-preview"
            x1={cssPx(preview[0].x)} y1={cssPx(preview[0].y)}
            x2={cssPx(preview[1].x)} y2={cssPx(preview[1].y)}
            markerEnd="url(#annot-arrowhead)"
          />
        )}
      </svg>

      {/* Text boxes + inline editor live in HTML (foreignObject-free) */}
      {state.elements
        .filter((el) => el.kind === "text" && el.box)
        .map((el) => (
          <div
            key={el.id}
            className="annot-text"
            style={{
              left: cssPx(el.box!.x), top: cssPx(el.box!.y),
              width: cssPx(el.box!.w), minHeight: cssPx(el.box!.h),
              pointerEvents: state.active ? "auto" : "none",
            }}
            onDoubleClick={() => state.active && setEditingId(el.id)}
          >
            {editingId === el.id ? (
              <input
                ref={editRef}
                className="annot-text-input"
                defaultValue={el.text ?? ""}
                placeholder="Type here…"
                onKeyDown={(e) => {
                  if (e.key === "Enter") commitEdit(el.id, (e.target as HTMLInputElement).value);
                  if (e.key === "Escape") commitEdit(el.id, (e.target as HTMLInputElement).value);
                }}
                onBlur={(e) => commitEdit(el.id, e.target.value)}
              />
            ) : (
              <span style={{ color: el.color }}>{el.text || "…"}</span>
            )}
          </div>
        ))}
    </>
  );
}
