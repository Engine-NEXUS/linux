import { useEffect, useState, useCallback } from "react";
import { useSidebar, type SpatialData, type SpatialItemCard } from "./sidebarStore";

function isTauri(): boolean {
  return typeof (window as any).__TAURI_INTERNALS__ !== "undefined";
}

/**
 * Feature 86 — SpatialDashboard: card-by-card deep dive for the spatial
 * screen analysis. Pins [1]..[n] here match the glowing boxes floating
 * over the desktop stage. Bi-directional sync:
 *   card hover      → stage:highlight_item   (desktop box pulses cyan)
 *   stage pin click → sidebar:focus_spatial_card → this card activates
 *                     + scrolls into view.
 */
export function SpatialView({ onDock = () => {} }: { onDock?: (d: string) => void }) {
  const spatialData = useSidebar((s) => s.spatialData);

  // Fresh-window race path: fetch the payload the orchestrator stored
  // before this window existed. Event listener is the fast path.
  useEffect(() => {
    if (!isTauri()) return;
    void (async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      const pending = await invoke<SpatialData | null>("get_pending_spatial").catch(() => null);
      if (pending && pending.items?.length) {
        useSidebar.getState().showSpatial(pending);
      }
    })();
  }, []);

  // Fast path: Rust emits the full payload on analysis completion.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | null = null;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<SpatialData>("sidebar:show_spatial", (ev) => {
        if (ev.payload?.items?.length) {
          useSidebar.getState().showSpatial(ev.payload);
        }
      });
    })().catch(() => {});
    return () => { unlisten?.(); };
  }, []);

if (!spatialData) {
    return (
      <div className="spatial-dashboard spatial-dashboard--empty">
        {/* Header row: drag region + dock controls (Left/Right) */}
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
        No spatial analysis yet, sir. Say "Analyse the screen".
      </div>
    );
  }

  return (
    <>
      {/* Header row: drag region + dock controls (Left/Right) */}
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
      <SpatialDashboard data={spatialData} />
    </>
  );
}

export function SpatialDashboard({ data }: { data: SpatialData }) {
  const [activeCardId, setActiveCardId] = useState<number | null>(null);

  // Stage pin click → activate + reveal the matching card.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | null = null;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<{ id?: number | null }>("sidebar:focus_spatial_card", (ev) => {
        const id = ev.payload?.id;
        if (typeof id !== "number") return;
        setActiveCardId(id);
        document
          .getElementById(`spatial-card-${id}`)
          ?.scrollIntoView({ behavior: "smooth", block: "nearest" });
      });
    })().catch(() => {});
    return () => { unlisten?.(); };
  }, []);

  const hoverCard = useCallback((id: number | null) => {
    // A lost highlight = dead hover-sync with no other signal.
    void import("../ipc").then(({ emitLogged }) =>
      emitLogged("stage:highlight_item", { id }).catch(() => {}),
    );
  }, []);

  return (
    <div className="spatial-dashboard">
      <div className="spatial-topic">{data.title}</div>
      {data.overview && <div className="spatial-overview">{data.overview}</div>}
      <div className="spatial-cards">
        {data.items.map((item) => (
          <SpatialCard
            key={item.id}
            item={item}
            active={activeCardId === item.id}
            onHover={hoverCard}
          />
        ))}
      </div>
      <div className="spatial-provider">via {data.provider_used}</div>
    </div>
  );
}

function SpatialCard({ item, active, onHover }: {
  item: SpatialItemCard;
  active: boolean;
  onHover: (id: number | null) => void;
}) {
  return (
    <div
      id={`spatial-card-${item.id}`}
      className={`spatial-card ${active ? "spatial-card--active" : ""}`}
      onMouseEnter={() => onHover(item.id)}
      onMouseLeave={() => onHover(null)}
    >
      <div className="spatial-card-header">
        <span className="spatial-badge">[{item.id}]</span>
        <span className="spatial-label">{item.label}</span>
        <span className="spatial-category">{item.category}</span>
      </div>
      {item.summary && <div className="spatial-summary">{item.summary}</div>}
      {item.details.length > 0 && (
        <div className="spatial-detail-grid">
          {item.details.map((row, i) => (
            <div className="spatial-detail-row" key={i}>
              <span className="spatial-detail-title">{row.title}</span>
              <span className="spatial-detail-value">{row.value}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
