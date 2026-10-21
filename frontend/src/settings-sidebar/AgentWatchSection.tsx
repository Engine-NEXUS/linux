import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface WatchInfo {
  id: string;
  type: string;
  status: string;
  startedAt: number;
  endedAt: number | null;
}

const TOOL_NAMES: Record<string, string> = {
  claude: "Claude Code",
  gemini: "Gemini CLI",
  codex: "Codex",
  aider: "Aider",
  hermes: "Hermes Agent",
  opencode: "opencode",
  antigravity: "Antigravity",
  agy: "Antigravity CLI",
  cursor: "Cursor",
  trae: "Trae",
  windsurf: "Windsurf",
  vscode: "VS Code",
  generic: "Generic Agent Task",
};

const ACTIVE_STATUSES = new Set(["starting", "working", "blocked", "login_required"]);

/**
 * Command Hub card for the universal agent watcher.
 * Say "Nexus, watch over this", "watch over Antigravity", or "update me when
 * Claude has stopped running" — NEXUS holds system keep-awake and alerts you
 * the moment it finishes.
 */
export function AgentWatchSection() {
  const [reason, setReason] = useState<string | null>(null);
  const [adapters, setAdapters] = useState<string[]>([]);
  const [watches, setWatches] = useState<WatchInfo[]>([]);

  useEffect(() => {
    invoke<string | null>("agent_watch_unavailable_reason").then(setReason).catch(() => {});
    invoke<string[]>("agent_watch_list_adapters").then(setAdapters).catch(() => {});
  }, []);

  useEffect(() => {
    let alive = true;
    const poll = () => {
      invoke<{ watches: WatchInfo[] }>("agent_watch_list_watches")
        .then((r) => {
          if (alive) setWatches(r.watches ?? []);
        })
        .catch(() => {});
    };
    poll();
    const id = setInterval(poll, 4000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  const stop = async (id: string) => {
    try {
      await invoke("agent_watch_stop", { id });
    } catch (e) {
      console.error("agent_watch_stop failed:", e);
    }
  };

  return (
    <div className="settings-section">
      <div className="settings-section-title">Universal Agent Watcher</div>
      <div className="setting-row">
        <div>
          <div className="setting-label">{reason ? "Unavailable" : "Ready"}</div>
          <div className="setting-desc">
            {reason ??
              "Say “Nexus, watch over this”, “watch over Antigravity”, “watch over Claude”, or “update me when the response is received and Claude has stopped running” — NEXUS keeps your machine awake and speaks the moment the task is finished."}
          </div>
        </div>
      </div>
      {!reason && adapters.length > 0 && (
        <div className="setting-row">
          <div>
            <div className="setting-label">Covered tools</div>
            <div className="setting-desc">{adapters.map((a) => TOOL_NAMES[a] ?? a).join(", ")}</div>
          </div>
        </div>
      )}
      {watches.length > 0 && (
        <div className="setting-row" style={{ flexDirection: "column", alignItems: "stretch", gap: 8 }}>
          <div className="setting-label">Active & recent</div>
          {watches.map((w) => (
            <div
              key={w.id}
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: 8,
                padding: "6px 10px",
                borderRadius: 6,
                background: "rgba(255,255,255,0.04)",
                border: "1px solid rgba(255,255,255,0.08)",
              }}
            >
              <span style={{ fontSize: 13 }}>
                {TOOL_NAMES[w.type] ?? w.type} — <strong>{w.status}</strong>
              </span>
              {ACTIVE_STATUSES.has(w.status) && (
                <button className="settings-btn" onClick={() => stop(w.id)}>
                  Stop
                </button>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
