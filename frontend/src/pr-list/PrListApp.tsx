import { useEffect } from "react";
import { usePrList, PrSummary } from "./prListStore";

function isTauri(): boolean {
  return typeof (window as any).__TAURI_INTERNALS__ !== "undefined";
}

async function tauriInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error("Not in Tauri");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

/** Format ISO timestamp to a relative time string. */
function timeAgo(iso: string): string {
  if (!iso) return "";
  const then = new Date(iso).getTime();
  if (isNaN(then)) return "";
  const secs = Math.floor((Date.now() - then) / 1000);
  if (secs < 60) return "just now";
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.floor(days / 30);
  return `${months}mo ago`;
}

/** A single PR card with Merge and Analyse buttons. */
function PrCard({ pr, disabled, onMerge, onAnalyse }: {
  pr: PrSummary;
  disabled: boolean;
  onMerge: () => void;
  onAnalyse: () => void;
}) {
  return (
    <div className="pr-card">
      <div className="pr-card-header">
        <span className="pr-number">#{pr.number}</span>
        <span className="pr-repo">{pr.repo}</span>
        <span className="pr-time">{timeAgo(pr.created_at)}</span>
      </div>
      <div className="pr-title">{pr.title}</div>
      <div className="pr-author">by {pr.author}</div>
      <div className="pr-actions">
        <button className="pr-btn pr-btn-merge" disabled={disabled} onClick={onMerge}>
          Merge PR
        </button>
        <button className="pr-btn pr-btn-analyse" disabled={disabled} onClick={onAnalyse}>
          Analyse
        </button>
      </div>
    </div>
  );
}

export function PrListApp() {
  const { visible, repo, state, prs, loading, actionInProgress, hide, setActionInProgress, removePr } = usePrList();

  // Listen for orchestrator github_result events with PrList data.
  // This is the FAST PATH for when the sidebar window is already open
  // (e.g. user says "show open PRs" while the sidebar is visible).
  useEffect(() => {
    if (!isTauri()) return;

    let unlisten: (() => void) | null = null;

    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<any>("orchestrator:event", (ev) => {
        const payload = ev.payload;
        if (!payload || payload.type !== "github_result") return;

        const result = payload.result;
        if (!result || typeof result !== "object") return;

        // GitHubResult is internally tagged: { type: "pr_list", repo, state, prs }
        // (Rust uses #[serde(tag = "type")] + #[serde(rename_all = "snake_case")])
        if (result.type !== "pr_list") return;

        usePrList.getState().showPrList(result.repo, result.state, result.prs);
      });
    })();

    return () => { unlisten?.(); };
  }, []);

  // Fetch pending PR list data on mount — RACE-FREE initialization.
  // The orchestrator stores the PR list in a Rust static BEFORE creating
  // this window. We fetch it here so the data is available immediately
  // after the WebView loads, regardless of how long that takes.
  // This fixes the bug where the `github_result` event was emitted before
  // this window existed, causing the event to be lost.
  useEffect(() => {
    if (!isTauri()) return;

    (async () => {
      try {
        const pending = await tauriInvoke<any>("get_pending_pr_list");
        if (pending && pending.type === "pr_list") {
          console.log("[PR List] fetched pending PR list from Rust");
          usePrList.getState().showPrList(pending.repo, pending.state, pending.prs);
        }
      } catch (e) {
        console.warn("[PR List] failed to fetch pending data:", e);
      }
    })();
  }, []);

  // Listen for sidebar:backdrop events — the Rust side captures the desktop
  // behind the window, blurs it, and sends it as a data URI. We set it as a
  // CSS variable on <html> so the ::after layer in pr-list.css can render it.
  // This is the same mechanism used by the response sidebar and architect sidebar.
  useEffect(() => {
    if (!isTauri()) return;

    let unlisten: (() => void) | null = null;

    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<string>("sidebar:backdrop", (ev) => {
        const dataUri = ev.payload;
        if (typeof dataUri === "string" && dataUri.startsWith("data:image/")) {
          document.documentElement.style.setProperty("--sidebar-backdrop-image", `url("${dataUri}")`);
        }
      });
    })();

    return () => { unlisten?.(); };
  }, []);

  // Listen for Ctrl+Space to close
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.code === "Space" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        hide();
        tauriInvoke("hide_pr_list_sidebar").catch(() => {});
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [hide]);

  if (!visible) return null;

  const handleMerge = async (pr: PrSummary) => {
    setActionInProgress({ prNumber: pr.number, action: "merge" });
    try {
      // Emit a voice-like command through the orchestrator
      await tauriInvoke("orchestrator_process", {
        transcript: `merge pr ${pr.number} in ${pr.repo}`,
        dialogContext: null,
      });
      // Remove the PR from the list on success
      removePr(pr.number, pr.repo);
    } catch (e) {
      console.error("[PR List] merge failed:", e);
    } finally {
      setActionInProgress(null);
    }
  };

  const handleAnalyse = async (pr: PrSummary) => {
    setActionInProgress({ prNumber: pr.number, action: "analyse" });
    try {
      // Trigger the same analysis flow as "analyse pr N in repo"
      await tauriInvoke("orchestrator_process", {
        transcript: `analyse pr ${pr.number} in ${pr.repo}`,
        dialogContext: null,
      });
    } catch (e) {
      console.error("[PR List] analyse failed:", e);
    } finally {
      setActionInProgress(null);
    }
  };

  return (
    <div className="pr-list-container">
      <div className="pr-list-header">
        <div className="pr-list-title">
          {prs.length} {state} PR{prs.length === 1 ? "" : "s"} in {repo}
        </div>
        <button
          className="pr-list-close"
          onClick={() => tauriInvoke("show_settings_sidebar").catch(() => {})}
          title="Settings"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
          </svg>
        </button>
      </div>
      {loading && <div className="pr-list-loading">Loading PRs...</div>}
      {!loading && prs.length === 0 && (
        <div className="pr-list-empty">No pull requests found.</div>
      )}
      <div className="pr-list-scroll">
        {prs.map((pr) => (
          <PrCard
            key={`${pr.repo}#${pr.number}`}
            pr={pr}
            disabled={actionInProgress !== null}
            onMerge={() => handleMerge(pr)}
            onAnalyse={() => handleAnalyse(pr)}
          />
        ))}
      </div>
      <div className="pr-list-footer">
        Ctrl+Space to close
      </div>
    </div>
  );
}
