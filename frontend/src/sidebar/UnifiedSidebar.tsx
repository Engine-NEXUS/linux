import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { SidebarApp } from "./SidebarApp";
import { SettingsSidebarApp } from "../settings-sidebar/SettingsSidebarApp";
import { ArchitectApp } from "../architect/ArchitectApp";
import { PrListApp } from "../pr-list/PrListApp";
import { SpatialView } from "./SpatialDashboard";
import { AnnotateView } from "./AnnotatePalette";
import "./unified.css";

/**
 * Unified Dynamic Sidebar shell.
 *
 * ONE Tauri window ("sidebar") hosts every panel view. Switching views
 * happens instantly in React (no HWND create/destroy) and is driven by
 * RUST (voice commands emit `sidebar:set_view`) — no titlebar, no tabs.
 * Dock controls live in each view's header row.
 *
 * Views are keep-mounted after first activation (module-level zustand
 * stores survive across mounts; display:none keeps remount flicker at
 * zero and makes switches feel instant).
 */
export type SidebarView = "assistant" | "settings" | "architect" | "pr-list" | "spatial" | "annotate";

const VIEWS: SidebarView[] = ["assistant", "settings", "architect", "pr-list", "spatial", "annotate"];

function isTauri(): boolean {
  return typeof (window as any).__TAURI_INTERNALS__ !== "undefined";
}

export function UnifiedSidebar() {
  const [view, setView] = useState<SidebarView>("assistant");
  const [mounted, setMounted] = useState<Set<SidebarView>>(() => new Set<SidebarView>(["assistant"]));

  // Rust emits sidebar:set_view { view, dock } on every show / navigation.
  // FRESH-WINDOW RACE: that event can fire before this component mounts —
  // so we also fetch the persisted pending view on mount and land there.
  useEffect(() => {
    if (!isTauri()) return;
    const un = listen<{ view: string }>("sidebar:set_view", (event) => {
      const v = event.payload.view as SidebarView;
      if (VIEWS.includes(v)) {
        setView(v);
        setMounted((m) => (m.has(v) ? m : new Set([...m, v])));
      }
    });
    return () => {
      un.then((u) => u());
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    invoke<string | null>("get_pending_sidebar_view")
      .then((v) => {
        if (v) {
          const id = v as SidebarView;
          if (VIEWS.includes(id)) {
            setView(id);
            setMounted((m) => (m.has(id) ? m : new Set([...m, id])));
          }
        }
      })
      .catch(() => {});
  }, []);

  // Synchronize liquid frosted glass light/dark theme across all views
  useEffect(() => {
    void import("./theme").then(({ initThemeSync }) => {
      initThemeSync();
    }).catch(() => {});
  }, []);

  // Exported for view components to call
  const dock = (d: string) => {
    if (!isTauri()) return;
    invoke("set_sidebar_dock", { dock: d }).catch(() => {});
  };

  return (
    <div className="unified-shell">
      {/* ── View host: keep-mounted views, display toggled ──────────── */}
      <div className="unified-view-host">
        {mounted.has("assistant") && (
          <div className="unified-view" style={{ display: view === "assistant" ? "flex" : "none" }}>
            <SidebarApp onDock={dock} />
          </div>
        )}
        {mounted.has("settings") && (
          <div className="unified-view" style={{ display: view === "settings" ? "flex" : "none" }}>
            <SettingsSidebarApp onDock={dock} />
          </div>
        )}
        {mounted.has("architect") && (
          <div className="unified-view" style={{ display: view === "architect" ? "flex" : "none" }}>
            <ArchitectApp onDock={dock} />
          </div>
        )}
        {mounted.has("pr-list") && (
          <div className="unified-view" style={{ display: view === "pr-list" ? "flex" : "none" }}>
            <PrListApp onDock={dock} />
          </div>
        )}
        {mounted.has("spatial") && (
          <div className="unified-view" style={{ display: view === "spatial" ? "flex" : "none" }}>
            <SpatialView onDock={dock} />
          </div>
        )}
        {mounted.has("annotate") && (
          <div className="unified-view" style={{ display: view === "annotate" ? "flex" : "none" }}>
            <AnnotateView onDock={dock} />
          </div>
        )}
      </div>
    </div>
  );
}
