import { useState, useRef, useEffect, useMemo, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useArchitect, type ImpactResult } from "./architectStore";
import { renderMarkdownToHtml } from "../sidebar/markdownRenderer";
import { speak } from "../audio/ttsPlayer";
import { getServerConfig } from "../net/wsBridge";

export function ArchitectSidebar() {
  const phase1Data = useArchitect((s) => s.phase1Data);
  const phase2Data = useArchitect((s) => s.phase2Data);
  const selectedNodeId = useArchitect((s) => s.selectedNodeId);
  const chatMessages = useArchitect((s) => s.chatMessages);
  const setSelectedNodeId = useArchitect((s) => s.setSelectedNodeId);
  const setImpactResult = useArchitect((s) => s.setImpactResult);
  const setHighlightedPaths = useArchitect((s) => s.setHighlightedPaths);
  const addChatMessage = useArchitect((s) => s.addChatMessage);

  const [inputVal, setInputVal] = useState("");
  const messagesEndRef = useRef<HTMLDivElement>(null);

  // Determine if selected node is a Layer or a File
  const selectedLayer = useMemo(() => {
    if (!phase1Data || !selectedNodeId) return null;
    return phase1Data.layers.find((l) => l.id === selectedNodeId) || null;
  }, [phase1Data, selectedNodeId]);

  const selectedFile = useMemo(() => {
    if (!phase2Data || !selectedNodeId) return null;
    return phase2Data.nodes[selectedNodeId] || null;
  }, [phase2Data, selectedNodeId]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [chatMessages]);

  // Run impact query via Rust reverse BFS, then get LLM narration from Worker
  const runImpactAnalysis = useCallback(
    async (filePath: string) => {
      try {
        const impact = await invoke<ImpactResult>("query_impact", {
          targetFile: filePath,
          maxDepth: 6,
        });

        setImpactResult(impact);
        setHighlightedPaths(impact.dependency_paths);

        // Add assistant message with explanation and path breakdown
        let impactMd = `💥 **Blast Radius Analysis for \`${filePath}\`:**\n\n${impact.explanation}\n\n`;
        impactMd += `* **Direct Dependents (Depth 1):** ${impact.direct_count}\n`;
        impactMd += `* **Transitive Dependents:** ${impact.transitive_count}\n`;
        impactMd += `* **Max Dependency Depth:** ${impact.max_depth}\n`;

        if (impact.test_files_affected.length > 0) {
          impactMd += `* **Test Files Affected:** ${impact.test_files_affected.map((t) => `\`${t}\``).join(", ")}\n`;
        }

        if (impact.dependency_paths.length > 0) {
          impactMd += `\n**Critical Dependency Paths:**\n`;
          impact.dependency_paths.slice(0, 4).forEach((p) => {
            impactMd += `- ${p.map((f) => `\`${f}\``).join(" → ")}\n`;
          });
        }

        addChatMessage({ role: "assistant", text: impactMd });
        void speak(
          impact.direct_count > 0
            ? `Changing this file directly affects ${impact.direct_count} files.`
            : "This is an isolated leaf node with minimal blast radius."
        );

        // Phase 3 LLM narration: ask the Worker to explain WHY the paths matter
        if (impact.affected_files.length > 0) {
          try {
            const config = await getServerConfig();
            const repoName = `${phase1Data?.owner || "unknown"}/${phase1Data?.repo || "repo"}`;
            const narrationResp = await fetch(config.url, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({
                request_id: `impact-${Date.now()}`,
                requester: { id: config.userId || "architect-local" },
                task: {
                  request: "impact narration",
                  intent: "impact_narration",
                  target_file: filePath,
                  affected_files: impact.affected_files,
                  dependency_paths: impact.dependency_paths,
                  direct_count: impact.direct_count,
                  transitive_count: impact.transitive_count,
                  test_files: impact.test_files_affected,
                  repo: repoName,
                },
              }),
            });
            if (narrationResp.ok) {
              const narrationData = await narrationResp.json();
              const narration = narrationData.reply_text;
              if (narration && narration.length > 20) {
                addChatMessage({
                  role: "assistant",
                  text: `🧠 **LLM Risk Assessment:**\n\n${narration}`,
                });
              }
            }
          } catch (narrationErr) {
            console.warn("LLM narration failed (non-fatal):", narrationErr);
          }
        }
      } catch (err: any) {
        console.warn("Impact analysis error:", err);
      }
    },
    [setImpactResult, setHighlightedPaths, addChatMessage, phase1Data]
  );

  const handleSendMessage = (textToSend?: string) => {
    const text = (textToSend || inputVal).trim();
    if (!text) return;

    addChatMessage({ role: "user", text });
    setInputVal("");

    const lower = text.toLowerCase();

    // If asking about impact on a selected file
    if ((lower.includes("break") || lower.includes("impact") || lower.includes("change")) && selectedFile) {
      void runImpactAnalysis(selectedFile.file_path);
      return;
    }

    // Architecture chat: deterministic summary from the local dependency graph.
    // No external model — ponytail: route through Worker when server-side
    // graph-aware chat lands.
    {
        let reply = "";
        if (lower.includes("circular") || lower.includes("cycle")) {
          if (phase2Data && phase2Data.circular_deps.length > 0) {
            reply = `� **Found ${phase2Data.circular_deps.length} Circular Dependency Chains:**\n\n`;
            phase2Data.circular_deps.forEach((c, idx) => {
              reply += `**Cycle #${idx + 1}:**\n${c.chain.map((f) => `\`${f}\``).join(" ➔ ")}\n*Risk: ${c.risk}*\n\n`;
            });
          } else if (phase2Data) {
            reply = `✅ **No Circular Dependencies Detected.** The import hierarchy has a clean directed acyclic structure.`;
          } else {
            reply = `� Run Phase 2 Deep Scan to trace exact circular dependency chains across the entire codebase.`;
          }
        } else if (lower.includes("hotspot") || lower.includes("hub") || lower.includes("critical")) {
          if (phase2Data && phase2Data.hotspots.length > 0) {
            reply = `� **Top Coupling Hotspots:**\n\n`;
            phase2Data.hotspots.slice(0, 5).forEach((h) => {
              reply += `- \`${h.file}\` — **${h.in_degree} files import this** (${h.risk.toUpperCase()} risk)\n`;
            });
          } else {
            reply = `� Run Phase 2 Deep Scan to identify critical hotspots.`;
          }
        } else {
          reply = `💡 **Architecture Agent:**\n\n${phase2Data?.summary || phase1Data?.summary || "Analyzing codebase..."}\n\nSelect any node on the canvas to inspect dependencies or run blast radius queries.`;
        }
        addChatMessage({ role: "assistant", text: reply });
        void speak("Analysis updated.");
    }
  };

  return (
    <aside className="architect-sidebar">
      {/* ── Top Panel: Inspector ─────────────────────────────────── */}
      <div className="architect-inspector-panel">
        <div className="architect-sidebar-header">
          <span className="architect-sidebar-title">
            {selectedFile
              ? "FILE INSPECTOR"
              : selectedLayer
              ? `LAYER: ${selectedLayer.label.toUpperCase()}`
              : "REPOSITORY OVERVIEW"}
          </span>
          {selectedFile ? (
            <span
              className="architect-layer-type-tag"
              style={{
                background: selectedFile.is_hotspot ? "rgba(255, 69, 58, 0.2)" : "rgba(250, 88, 106, 0.14)",
                color: selectedFile.is_hotspot ? "#ff453a" : "#fa586a",
              }}
            >
              {selectedFile.risk_level.toUpperCase()}
            </span>
          ) : selectedLayer ? (
            <span className="architect-layer-type-tag">{selectedLayer.layer_type.toUpperCase()}</span>
          ) : null}
        </div>

        <div className="architect-inspector-content">
          {selectedFile ? (
            <div className="architect-file-detail">
              <div className="architect-file-path-box">
                <code>{selectedFile.file_path}</code>
              </div>

              <div className="architect-detail-row">
                <span className="architect-detail-label">Dependents (Imported By)</span>
                <span className="architect-detail-val">{selectedFile.in_degree} files</span>
              </div>
              <div className="architect-detail-row">
                <span className="architect-detail-label">Dependencies (Imports)</span>
                <span className="architect-detail-val">{selectedFile.out_degree} files</span>
              </div>

              <button
                type="button"
                className="architect-impact-btn"
                onClick={() => runImpactAnalysis(selectedFile.file_path)}
              >
                💥 Run Reverse BFS Impact Analysis
              </button>

              {selectedFile.imported_by.length > 0 && (
                <div className="architect-detail-section">
                  <span className="architect-detail-label">Imported By (Direct):</span>
                  <div className="architect-detail-file-list">
                    {selectedFile.imported_by.slice(0, 5).map((f, i) => (
                      <div
                        key={i}
                        className="architect-file-link"
                        onClick={() => setSelectedNodeId(f)}
                        title="Focus file"
                      >
                        📥 {f}
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {selectedFile.imports.length > 0 && (
                <div className="architect-detail-section">
                  <span className="architect-detail-label">Imports:</span>
                  <div className="architect-detail-file-list">
                    {selectedFile.imports.slice(0, 5).map((f, i) => (
                      <div
                        key={i}
                        className="architect-file-link"
                        onClick={() => setSelectedNodeId(f)}
                        title="Focus file"
                      >
                        📦 {f}
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          ) : selectedLayer ? (
            <div className="architect-layer-detail">
              <div className="architect-detail-row">
                <span className="architect-detail-label">Tech Stack</span>
                <span className="architect-detail-val">{selectedLayer.tech_stack}</span>
              </div>
              <div className="architect-detail-row">
                <span className="architect-detail-label">File Count</span>
                <span className="architect-detail-val">{selectedLayer.file_count} files</span>
              </div>
              <div className="architect-detail-section">
                <span className="architect-detail-label">Root Directories:</span>
                <div className="architect-detail-tags">
                  {selectedLayer.dirs.map((d, i) => (
                    <span key={i} className="architect-detail-pill">
                      {d}
                    </span>
                  ))}
                </div>
              </div>
              <div className="architect-detail-section">
                <span className="architect-detail-label">Sample Files:</span>
                <div className="architect-detail-file-list">
                  {selectedLayer.sample_files.map((f, i) => (
                    <div
                      key={i}
                      className="architect-file-link"
                      onClick={() => phase2Data?.nodes[f] && setSelectedNodeId(f)}
                    >
                      📄 {f}
                    </div>
                  ))}
                </div>
              </div>
            </div>
          ) : phase2Data ? (
            <div className="architect-repo-overview">
              <p className="architect-repo-desc">{phase2Data.summary}</p>
              <div className="architect-overview-grid">
                <div className="architect-stat-card">
                  <div className="architect-stat-num">{phase2Data.total_files}</div>
                  <div className="architect-stat-lbl">Files</div>
                </div>
                <div className="architect-stat-card">
                  <div className="architect-stat-num">{phase2Data.hotspots.length}</div>
                  <div className="architect-stat-lbl">Hotspots</div>
                </div>
                <div className="architect-stat-card">
                  <div className="architect-stat-num">{phase2Data.circular_deps.length}</div>
                  <div className="architect-stat-lbl">Cycles</div>
                </div>
              </div>

              {phase2Data.hotspots.length > 0 && (
                <div className="architect-detail-section">
                  <span className="architect-detail-label">🔥 Top Coupling Hubs:</span>
                  <div className="architect-detail-file-list">
                    {phase2Data.hotspots.slice(0, 4).map((h, i) => (
                      <div
                        key={i}
                        className="architect-file-link"
                        onClick={() => setSelectedNodeId(h.file)}
                      >
                        ⚡ <strong>{h.file}</strong> ({h.in_degree} deps)
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          ) : phase1Data ? (
            <div className="architect-repo-overview">
              <p className="architect-repo-desc">{phase1Data.description}</p>
              <div className="architect-overview-grid">
                <div className="architect-stat-card">
                  <div className="architect-stat-num">{phase1Data.layers.length}</div>
                  <div className="architect-stat-lbl">Layers</div>
                </div>
                <div className="architect-stat-card">
                  <div className="architect-stat-num">{phase1Data.total_files}</div>
                  <div className="architect-stat-lbl">Files</div>
                </div>
                <div className="architect-stat-card">
                  <div className="architect-stat-num">{phase1Data.primary_language}</div>
                  <div className="architect-stat-lbl">Language</div>
                </div>
              </div>
            </div>
          ) : (
            <div className="architect-empty-state">No repository loaded. Enter a repository above.</div>
          )}
        </div>
      </div>

      {/* ── Bottom Panel: Agentic Chat ───────────────────────────── */}
      <div className="architect-chat-panel">
        <div className="architect-sidebar-header">
          <span className="architect-sidebar-title">CONSEQUENCE & IMPACT AGENT</span>
          <span className="architect-agent-dot" />
        </div>

        {/* Quick Action Suggestions */}
        <div className="architect-quick-prompts">
          <button
            type="button"
            className="architect-prompt-chip"
            onClick={() => handleSendMessage(selectedFile ? `What breaks if I change ${selectedFile.file_path}?` : "What breaks if I change the core?")}
          >
            💥 What breaks if I change this?
          </button>
          <button
            type="button"
            className="architect-prompt-chip"
            onClick={() => handleSendMessage("Show top coupling hotspots")}
          >
            🔥 Top Hotspots
          </button>
          <button
            type="button"
            className="architect-prompt-chip"
            onClick={() => handleSendMessage("Are there any circular dependencies?")}
          >
            🔄 Circular Deps
          </button>
        </div>

        {/* Chat Stream */}
        <div className="architect-chat-stream">
          {chatMessages.map((msg) => (
            <div
              key={msg.id}
              className={`architect-chat-bubble architect-chat-bubble--${msg.role}`}
            >
              <div className="architect-bubble-role">
                {msg.role === "assistant" ? "🤖 NEXUS" : "👤 YOU"}
              </div>
              <div
                className="architect-bubble-content nexus-markdown-body"
                dangerouslySetInnerHTML={{ __html: renderMarkdownToHtml(msg.text) }}
              />
            </div>
          ))}
          <div ref={messagesEndRef} />
        </div>

        {/* Chat Input */}
        <form
          className="architect-chat-form"
          onSubmit={(e) => {
            e.preventDefault();
            handleSendMessage();
          }}
        >
          <input
            type="text"
            className="architect-chat-input"
            placeholder={
              selectedFile
                ? `Ask about ${selectedFile.file_path.split("/").pop()}...`
                : selectedLayer
                ? `Ask about ${selectedLayer.label}...`
                : "Ask impact or consequence question..."
            }
            value={inputVal}
            onChange={(e) => setInputVal(e.target.value)}
          />
          <button type="submit" className="architect-chat-send" title="Send (Enter)">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="22" y1="2" x2="11" y2="13" />
              <polygon points="22 2 15 22 11 13 2 9 22 2" />
            </svg>
          </button>
        </form>
      </div>
    </aside>
  );
}
