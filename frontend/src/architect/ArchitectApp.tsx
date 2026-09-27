import { useEffect, useState, useCallback, Component, type ReactNode } from "react";
import { invoke, Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useArchitect, type Phase1Data, type Phase2Data, type ArchitectProgress } from "./architectStore";
import { ArchitectureMap } from "./ArchitectureMap";

/** Error boundary so a layout/dagre crash doesn't take down the whole app. */
class MapErrorBoundary extends Component<{ children: ReactNode }, { hasError: boolean }> {
  state = { hasError: false };
  static getDerivedStateFromError() { return { hasError: true }; }
  componentDidCatch(err: unknown) { console.error("[architect] ArchitectureMap crashed:", err); }
  render() {
    if (this.state.hasError) {
      return (
        <div className="architect-hero-empty">
          <div className="architect-hero-card">
            <p className="architect-hero-subtitle">The architecture map hit an error. Try re-analyzing.</p>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}

export function ArchitectApp() {
  const loading = useArchitect((s) => s.loading);
  const deepScanning = useArchitect((s) => s.deepScanning);
  const progressMessage = useArchitect((s) => s.progressMessage);
  const phase1Data = useArchitect((s) => s.phase1Data);
  const phase2Data = useArchitect((s) => s.phase2Data);
  const viewMode = useArchitect((s) => s.viewMode);

  const setRepo = useArchitect((s) => s.setRepo);
  const setLoading = useArchitect((s) => s.setLoading);
  const setDeepScanning = useArchitect((s) => s.setDeepScanning);
  const setProgress = useArchitect((s) => s.setProgress);
  const setPhase1Data = useArchitect((s) => s.setPhase1Data);
  const enrichPhase1 = useArchitect((s) => s.enrichPhase1);
  const setPhase2Data = useArchitect((s) => s.setPhase2Data);
  const setSelectedNodeId = useArchitect((s) => s.setSelectedNodeId);
  const setViewMode = useArchitect((s) => s.setViewMode);

  const [inputRepo, setInputRepo] = useState("");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  // Fetch GitHub OAuth token from the Worker (for private repo access)
  const fetchGithubToken = useCallback(async (): Promise<string | null> => {
    try {
      const { getServerConfig } = await import("../net/wsBridge");
      const config = await getServerConfig();
      const resp = await fetch(`${config.url}/oauth/github-token?user_id=${encodeURIComponent(config.userId)}`);
      if (resp.ok) {
        const data = await resp.json();
        return data.token || null;
      }
    } catch (err) {
      console.warn("Failed to fetch GitHub token:", err);
    }
    return null;
  }, []);

  // Trigger Phase 2 deep dependency scan in background
  const triggerDeepScan = useCallback(
    async (o: string, r: string, token: string | null) => {
      console.log("[architect] triggerDeepScan called:", o, "/", r);
      setDeepScanning(true);

      const onProgress = new Channel<ArchitectProgress>();
      onProgress.onmessage = (msg) => {
        console.log("[architect] Phase 2 progress:", msg.type, ("message" in msg ? msg.message : ""));
        if (msg.type === "Detecting" || msg.type === "Indexing") {
          setProgress(msg.type.toLowerCase(), msg.message);
        } else if (msg.type === "GraphReady") {
          setProgress("graph", `${msg.node_count} nodes, ${msg.edge_count} edges`);
        } else if (msg.type === "HotspotsReady") {
          setProgress("hotspots", `${msg.hotspots.length} hotspots found`);
        } else if (msg.type === "CyclesReady") {
          setProgress("cycles", `${msg.circular_deps.length} circular deps found`);
        } else if (msg.type === "Failed") {
          console.warn(`Phase 2 failed at ${msg.stage}: ${msg.error}`);
          setDeepScanning(false);
        } else if (msg.type === "Complete") {
          setDeepScanning(false);
        }
      };

      try {
        console.log("[architect] invoking analyze_repo_deep...");
        const result = await invoke<Phase2Data>("analyze_repo_deep", {
          owner: o,
          repo: r,
          githubToken: token,
          onProgress,
        });
        console.log("[architect] Phase 2 complete:", result.hotspots?.length, "hotspots,", result.circular_deps?.length, "cycles");
        setPhase2Data(result);
      } catch (err: any) {
        console.error("[architect] Phase 2 deep scan FAILED:", err);
        setDeepScanning(false);
      }
    },
    [setDeepScanning, setProgress, setPhase2Data]
  );

  // Trigger Phase 1 fast analysis
  const triggerAnalysis = useCallback(
    async (o: string, r: string) => {
      if (!o || !r) return;
      setLoading(true);
      setErrorMsg(null);
      setProgress("init", `Starting Phase 1 analysis for ${o}/${r}...`);

      // Fetch GitHub token for private repo access
      const token = await fetchGithubToken();

      const onProgress = new Channel<ArchitectProgress>();
      onProgress.onmessage = (msg) => {
        if (msg.type === "Detecting" || msg.type === "Indexing") {
          setProgress(msg.type.toLowerCase(), msg.message);
        } else if (msg.type === "Failed") {
          setErrorMsg(`${msg.stage}: ${msg.error}`);
          setLoading(false);
        } else if (msg.type === "Complete") {
          setLoading(false);
        }
      };

      try {
        const result = await invoke<Phase1Data>("analyze_repo_phase1", {
          owner: o,
          repo: r,
          githubToken: token,
          onProgress,
        });
        setPhase1Data(result);

        // NOTE: AI enrichment is now done INLINE in Rust before the window opens.
        // The result we receive here is already enriched with repo-specific labels.
        // No fire-and-forget enrich_phase1 call needed.

        // Auto start background Phase 2 deep scan
        void triggerDeepScan(o, r, token);
      } catch (err: any) {
        console.error("Phase 1 analysis failed:", err);
        setErrorMsg(typeof err === "string" ? err : err.message || "Failed to analyze repo");
        setLoading(false);
      }
    },
    [setLoading, setProgress, setPhase1Data, triggerDeepScan, fetchGithubToken]
  );

  // Submit manual input
  const handleManualSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const raw = inputRepo.trim();
    if (!raw) return;

    let o = "";
    let r = "";

    if (raw.includes("github.com/")) {
      const parts = raw.split("github.com/")[1].split("/");
      o = parts[0];
      r = parts[1]?.replace(".git", "");
    } else if (raw.includes("/")) {
      const parts = raw.split("/");
      o = parts[0];
      r = parts[1];
    }

    if (o && r) {
      setRepo(o, r);
      void triggerAnalysis(o, r);
    } else {
      setErrorMsg("Please enter in format: owner/repo or full GitHub URL");
    }
  };

  // Listen to Tauri events + fetch pending repo on mount.
  //
  // When the architect window is created on-demand, the React app needs time
  // to load before it can receive Tauri events. So Rust stores the repo in a
  // pending static, and we fetch it here on mount via `get_pending_architect_repo`.
  // This is race-free. We also keep the event listener as a fast path for when
  // the window already exists (React already loaded).
  useEffect(() => {
    const unlisteners: (() => void)[] = [];
    let mounted = true;
    console.log("[architect] ArchitectApp mounted, fetching pending repo...");

    // Fetch pending repo on mount — handles the fresh-window case.
    // Also fetches the screenshot backdrop for the liquid-glass effect.
    // NOTE: `pending` is now ALWAYS returned (non-null) once the window has
    // been opened via open_architect_window, even when no active GitHub
    // repo was auto-detected — this ensures the backdrop always reaches the
    // frontend. `owner`/`repo` may independently be null in that case, so
    // they're checked separately from the backdrop.
    invoke<{ owner: string | null; repo: string | null; backdrop?: string | null; phase1_data?: Phase1Data | null } | null>("get_pending_architect_repo")
      .then((pending) => {
        console.log("[architect] get_pending_architect_repo returned:", pending ? `owner=${pending.owner}, repo=${pending.repo}, has_phase1=${!!pending.phase1_data}` : "null");
        if (!pending) return;
        // Set the backdrop image for the liquid-glass card
        if (pending.backdrop) {
          document.documentElement.style.setProperty(
            "--sidebar-backdrop-image",
            `url(${pending.backdrop})`
          );
        }
        // If Phase 1 data was pre-computed by open_architect_with_auto_detect,
        // render it immediately — no need to call analyze_repo_phase1 again.
        if (pending.phase1_data) {
          console.log("[architect] received pre-computed Phase 1 data (already AI-enriched)");
          if (pending.owner && pending.repo) {
            setRepo(pending.owner, pending.repo);
            setInputRepo(`${pending.owner}/${pending.repo}`);
          }
          setPhase1Data(pending.phase1_data);
          setLoading(false);
          // AI enrichment is already done in Rust before the window opens.
          // Only start Phase 2 deep scan in the background.
          if (pending.owner && pending.repo) {
            console.log("[architect] starting Phase 2 deep scan for", pending.owner, "/", pending.repo);
            void triggerDeepScan(pending.owner, pending.repo, null);
          }
        } else if (pending.owner && pending.repo) {
          // No pre-computed data — start analysis normally
          setRepo(pending.owner, pending.repo);
          setInputRepo(`${pending.owner}/${pending.repo}`);
          void triggerAnalysis(pending.owner, pending.repo);
        }
      })
      .catch((e) => console.error("[architect] get_pending_architect_repo FAILED:", e));

    // Listen for backdrop updates (live blur loop + window reuse)
    listen<string>("sidebar:backdrop", (event) => {
      document.documentElement.style.setProperty(
        "--sidebar-backdrop-image",
        `url(${event.payload})`
      );
    }).then((u) => { if (mounted) unlisteners.push(u); else u(); });

    listen<{ owner: string; repo: string }>("architect:set-repo", (event) => {
      setRepo(event.payload.owner, event.payload.repo);
      setInputRepo(`${event.payload.owner}/${event.payload.repo}`);
      void triggerAnalysis(event.payload.owner, event.payload.repo);
    }).then((u) => { if (mounted) unlisteners.push(u); else u(); });

    // LLM enrichment streams in ~2-3s after first paint — merges
    // repo-specific layer labels + summary into the existing diagram.
    listen<{ summary: string; layers: { id: string; label: string; tech_stack: string }[] }>(
      "architect:phase1-enriched",
      (event) => {
        enrichPhase1(event.payload);
      }
    ).then((u) => { if (mounted) unlisteners.push(u); else u(); });

    return () => {
      mounted = false;
      unlisteners.forEach((u) => u());
    };
  }, [setRepo, setPhase1Data, enrichPhase1, setPhase2Data, triggerAnalysis, triggerDeepScan]);

  return (
    <div className="architect-app">
      {/* ── Top Navigation Bar ─────────────────────────────────────── */}
      <header className="architect-header">
        {/* Repo search / input bar */}
        <form className="architect-repo-form" onSubmit={handleManualSubmit}>
          <div className="architect-input-wrapper">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="11" cy="11" r="8" />
              <line x1="21" y1="21" x2="16.65" y2="16.65" />
            </svg>
            <input
              type="text"
              className="architect-repo-input"
              placeholder="owner/repo or https://github.com/..."
              value={inputRepo}
              onChange={(e) => setInputRepo(e.target.value)}
            />
          </div>
          <button type="submit" className="architect-analyze-btn" disabled={loading || deepScanning}>
            {loading ? "Analyzing..." : deepScanning ? "Deep Scanning..." : "Analyze Repo"}
          </button>
        </form>

        {/* View buttons + stats */}
        <div className="architect-header-actions">
          {phase1Data && (
            <div className="architect-view-tabs">
              <button
                type="button"
                className={`architect-view-tab ${viewMode === "layers" || viewMode === "files" ? "active" : ""}`}
                onClick={() => setViewMode("files")}
              >
                Files
              </button>
              <button
                type="button"
                className={`architect-view-tab ${viewMode === "hotspots" ? "active" : ""}`}
                onClick={() => setViewMode("hotspots")}
                disabled={!phase2Data}
              >
                Hotspots {phase2Data ? `(${phase2Data.hotspots.length})` : ""}
              </button>
              <button
                type="button"
                className={`architect-view-tab ${viewMode === "cycles" ? "active" : ""}`}
                onClick={() => setViewMode("cycles")}
                disabled={!phase2Data}
              >
                Cycles {phase2Data ? `(${phase2Data.circular_deps.length})` : ""}
              </button>
            </div>
          )}
          {phase1Data && !phase2Data && !deepScanning && (
            <button
              type="button"
              className="architect-analyze-btn"
              onClick={() => triggerDeepScan(phase1Data.owner, phase1Data.repo, null)}
            >
              Run Deep Scan
            </button>
          )}
        </div>
      </header>

      {/* ── Progress & Status Banner ───────────────────────────────── */}
      {(loading || deepScanning) && (
        <div className="architect-progress-bar-container">
          <div className="architect-progress-indicator" />
          <span className="architect-progress-text">
            {progressMessage || (deepScanning ? "Running deep AST import scan in background..." : "Processing...")}
          </span>
        </div>
      )}

      {errorMsg && (
        <div className="architect-error-banner">
          ⚠️ {errorMsg}
          <button type="button" className="architect-error-close" onClick={() => setErrorMsg(null)}>
            ✕
          </button>
        </div>
      )}

      {/* ── Main Layout: Full-width Architecture Map (no sidebar overlay) ── */}
      <main className="architect-main">
        {phase1Data ? (
          <MapErrorBoundary>
            <ArchitectureMap />
          </MapErrorBoundary>
        ) : (
          <div className="architect-hero-empty">
            <div className="architect-hero-card">
              <div className="architect-hero-loading-spinner" />
              <p className="architect-hero-subtitle">
                {loading ? "Analyzing repository..." : "Waiting for repository..."}
              </p>
            </div>
          </div>
        )}
      </main>

      {/* ── Bottom Section: 2-Column Analytics ─────────────────────── */}
      {phase1Data && (
        <section className="architect-analytics-section">
          {/* Column 1: Top Coupling Hubs */}
          <div className="architect-analytics-col">
            <div className="architect-analytics-header">
              <span className="architect-analytics-title">Top Coupling Hubs</span>
              <span className="architect-analytics-count">
                {phase2Data ? phase2Data.hotspots.length : "—"}
              </span>
            </div>
            <div className="architect-analytics-body">
              {phase2Data && phase2Data.hotspots.length > 0 ? (
                phase2Data.hotspots.slice(0, 8).map((h, i) => (
                  <div
                    key={i}
                    className="architect-analytics-item"
                    onClick={() => setSelectedNodeId(h.file)}
                  >
                    <span className="architect-analytics-rank">#{i + 1}</span>
                    <code className="architect-analytics-file">{h.file}</code>
                    <span className={`architect-analytics-badge architect-analytics-badge--${h.risk}`}>
                      {h.in_degree} deps
                    </span>
                  </div>
                ))
              ) : (
                <div className="architect-analytics-empty">
                  {deepScanning ? "Scanning dependencies..." : "Run deep scan to find hotspots"}
                </div>
              )}
            </div>
          </div>

          {/* Column 2: Circular Dependencies */}
          <div className="architect-analytics-col">
            <div className="architect-analytics-header">
              <span className="architect-analytics-title">Circular Dependencies</span>
              <span className="architect-analytics-count">
                {phase2Data ? phase2Data.circular_deps.length : "—"}
              </span>
            </div>
            <div className="architect-analytics-body">
              {phase2Data && phase2Data.circular_deps.length > 0 ? (
                phase2Data.circular_deps.slice(0, 6).map((c, i) => (
                  <div key={i} className="architect-analytics-cycle">
                    <span className="architect-analytics-rank">#{i + 1}</span>
                    <div className="architect-analytics-chain">
                      {c.chain.map((f, j) => (
                        <span
                          key={j}
                          className="architect-analytics-chain-node architect-analytics-chain-clickable"
                          onClick={() => setSelectedNodeId(f)}
                          title={f}
                        >
                          {f.split("/").pop()}
                          {j < c.chain.length - 1 && <span className="architect-analytics-arrow">→</span>}
                        </span>
                      ))}
                    </div>
                    <span className={`architect-analytics-badge architect-analytics-badge--${c.risk}`}>
                      {c.risk}
                    </span>
                  </div>
                ))
              ) : (
                <div className="architect-analytics-empty">
                  {deepScanning ? "Scanning for cycles..." : phase2Data ? "No circular dependencies detected" : "Run deep scan to check for cycles"}
                </div>
              )}
            </div>
          </div>
        </section>
      )}
    </div>
  );
}
