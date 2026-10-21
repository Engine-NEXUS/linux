import { useEffect, useState } from "react";
import type { SpatialData } from "./sidebarStore";
import type { AnnotElement, AnnotTool } from "../stage/annotationTypes";
import { toAnnotJson, toMermaid, toPlantUml } from "./diagramSerializer";

function isTauri(): boolean {
  return typeof (window as any).__TAURI_INTERNALS__ !== "undefined";
}

async function emit(event: string, payload?: Record<string, unknown>): Promise<void> {
  // Logged boundary (log-completeness P2): canvas control events
  // (tool_change, commit_request, begin/append) lost silently strand the
  // ink canvas — failures must surface.
  const { emitLogged } = await import("../ipc");
  await emitLogged(event, payload ?? {}).catch(() => {});
}

const TOOLS: Array<{ id: AnnotTool; label: string; hint: string }> = [
  { id: "select", label: "✥", hint: "Select / move / toggle" },
  { id: "arrow", label: "↗", hint: "Draw arrow pointer" },
  { id: "text", label: "T", hint: "Place text box" },
  { id: "freehand", label: "✎", hint: "Freehand ink" },
  { id: "checkbox", label: "☐", hint: "Place checkbox" },
];

const COLORS = ["#259ED6", "#22c55e", "#ef4444", "#ffffff", "#fbbf24"];

type DiagramTab = "mermaid" | "plantuml" | "json";

/**
 * Feature 87 — AnnotatePalette: tool palette + diagram export for the
 * stage ink canvas. Backend wires itself on mount:
 *   fresh-window → get_pending_annotation → stage:annotation_begin
 *   warm-window  → sidebar:show_annotation → stage:annotation_begin
 * Canvas ops stay frontend-local; commit serializes here.
 */
export function AnnotateView({ onDock = () => {} }: { onDock?: (d: string) => void }) {
  const [tool, setTool] = useState<AnnotTool>("select");
  const [color, setColor] = useState(COLORS[0]);
  const [count, setCount] = useState(0);
  const [lastEls, setLastEls] = useState<AnnotElement[] | null>(null);
  const [tab, setTab] = useState<DiagramTab>("mermaid");
  const diagram = lastEls === null ? null : renderTab(tab, lastEls);

  const beginOnStage = (elements: AnnotElement[]) => {
    void emit("stage:annotation_begin", { elements });
    setCount(elements.length);
  };

  useEffect(() => {
    if (!isTauri()) return;
    void (async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      const pending = await invoke<{ tool?: string; elements?: AnnotElement[] } | null>(
        "get_pending_annotation",
      ).catch(() => null);
      beginOnStage(pending?.elements ?? []);
    })();
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | null = null;
    let unlistenCommit: (() => void) | null = null;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<{ tool?: string; elements?: AnnotElement[] }>(
        "sidebar:show_annotation",
        (ev) => beginOnStage(ev.payload?.elements ?? []),
      );
      unlistenCommit = await listen<{ elements?: AnnotElement[] }>(
        "stage:annotation_commit",
        (ev) => {
          const els = ev.payload?.elements ?? [];
          setCount(els.length);
          setLastEls(els);
        },
      );
    })().catch(() => {});
    return () => {
      unlisten?.();
      unlistenCommit?.();
    };
  }, []);

  const pickTool = (t: AnnotTool) => {
    setTool(t);
    void emit("stage:tool_change", { tool: t, color });
  };

  const pickColor = (c: string) => {
    setColor(c);
    void emit("stage:tool_change", { tool, color: c });
  };

  const importSpatialPins = () => {
    void (async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      const data = await invoke<SpatialData | null>("get_pending_spatial").catch(() => null);
      const items = data?.items ?? [];
      if (items.length === 0) return;
      const els: AnnotElement[] = items.slice(0, 8).map((item, i) => ({
        id: `import-${i}`,
        kind: "text" as const,
        box: { x: 60, y: 80 + i * 56, w: 240, h: 36 },
        text: item.label,
        color,
      }));
      await emit("stage:annotation_append", { elements: els });
      setCount((c) => c + els.length);
    })();
  };

  const commit = () => {
    setLastEls(null);
    void emit("stage:annotation_commit_request", {});
  };

  return (
    <div className="annotate-palette">
      <header className="sidebar-header-row" data-tauri-drag-region>
        <div className="sidebar-header-spacer" data-tauri-drag-region />
        <div className="sidebar-dock-controls">
          <button type="button" className="sidebar-dock-btn" onClick={() => onDock("left")} title="Dock Left">
            ◧
          </button>
          <button type="button" className="sidebar-dock-btn" onClick={() => onDock("right")} title="Dock Right">
            ◨
          </button>
        </div>
      </header>

      <div className="annotate-title">Annotate Screen {count > 0 && <span className="annotate-count">{count}</span>}</div>

      <div className="annotate-tools">
        {TOOLS.map((t) => (
          <button
            key={t.id}
            type="button"
            title={t.hint}
            className={`annotate-tool ${tool === t.id ? "annotate-tool--active" : ""}`}
            onClick={() => pickTool(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>

      <div className="annotate-colors">
        {COLORS.map((c) => (
          <button
            key={c}
            type="button"
            title={c}
            className={`annotate-swatch ${color === c ? "annotate-swatch--active" : ""}`}
            style={{ background: c }}
            onClick={() => pickColor(c)}
          />
        ))}
      </div>

      <div className="annotate-actions">
        <button type="button" className="annotate-btn" onClick={() => void emit("stage:annotation_undo", {})}>
          Undo
        </button>
        <button type="button" className="annotate-btn" onClick={() => void emit("stage:annotation_redo", {})}>
          Redo
        </button>
        <button type="button" className="annotate-btn" onClick={() => void emit("stage:annotation_clear", {})}>
          Clear
        </button>
        <button type="button" className="annotate-btn" onClick={importSpatialPins} title="Import Feature-86 spatial pins as text nodes">
          Import pins
        </button>
      </div>

      <div className="annotate-actions">
        <button type="button" className="annotate-btn annotate-btn--primary" onClick={commit}>
          Commit diagram
        </button>
        <button type="button" className="annotate-btn" onClick={() => void emit("stage:annotation_done", {})} title="Exit capture (Esc). Ink stays on screen.">
          Done
        </button>
      </div>

      {diagram !== null && (
        <div className="annotate-diagram">
          <div className="annotate-tabs">
            {(["mermaid", "plantuml", "json"] as DiagramTab[]).map((t) => (
              <button
                key={t}
                type="button"
                className={`annotate-tab ${tab === t ? "annotate-tab--active" : ""}`}
                onClick={() => setTab(t)}
              >
                {t}
              </button>
            ))}
            <button
              type="button"
              className="annotate-tab"
              onClick={() => void navigator.clipboard?.writeText(diagram).catch(() => {})}
            >
              Copy
            </button>
          </div>
          <pre className="annotate-code">{diagram}</pre>
        </div>
      )}
    </div>
  );
}

function renderTab(tab: DiagramTab, els: AnnotElement[]): string {
  if (tab === "plantuml") return toPlantUml(els);
  if (tab === "json") return toAnnotJson(els);
  return toMermaid(els);
}
