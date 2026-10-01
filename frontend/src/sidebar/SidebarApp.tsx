import { useEffect, useRef, useState, useMemo, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { open as openExternal } from "@tauri-apps/plugin-shell";
import { useSidebar } from "./sidebarStore";
import { renderMarkdownToHtml } from "./markdownRenderer";
import { speak, stopTts } from "../audio/ttsPlayer";
import { AnalysisDashboard } from "./AnalysisDashboard";
import { GitHubConflictPanel } from "./GitHubConflictPanel";
import { ConfirmationPanel } from "./ConfirmationPanel";

/**
 * NEXUS Response Sidebar
 *
 * Rich frosted-glass panel rendering full Markdown with:
 * - GitHub Flavored Markdown (GFM) tables & formatting
 * - Syntax-highlighted code blocks with 1-click Copy Code buttons
 * - Responsive images with Lightbox full-size zoom modal
 * - GitHub Callout / Alert cards ([!NOTE], [!TIP], [!IMPORTANT], etc.)
 * - Safe external link handling in user's default browser
 * - Read Aloud (TTS) control, Copy Full Response, Font-size adjustment
 * - Rich repository analysis dashboard with pie charts
 * - Interactive action confirmation cards (WhatsApp, Swiggy, GitHub)
 * - Smooth acrylic blur with readable high-contrast typography
 */
export function SidebarApp() {
  const visible = useSidebar((s) => s.visible);
  const response = useSidebar((s) => s.response);
  const query = useSidebar((s) => s.query);
  const fontSize = useSidebar((s) => s.fontSize);
  const speaking = useSidebar((s) => s.speaking);
  const activeImage = useSidebar((s) => s.activeImage);
  const analysisData = useSidebar((s) => s.analysisData);
  const conflictData = useSidebar((s) => s.conflictData);
  const confirmationData = useSidebar((s) => s.confirmationData);

  const show = useSidebar((s) => s.show);
  const hide = useSidebar((s) => s.hide);
  const setSpeaking = useSidebar((s) => s.setSpeaking);
  const setActiveImage = useSidebar((s) => s.setActiveImage);

  const responseScrollRef = useRef<HTMLDivElement>(null);
  const [showScrollTop, setShowScrollTop] = useState(false);
  // Track whether the sidebar was previously visible — prevents the
  // initial-mount useEffect (visible starts as false) from calling
  // stopTts() and killing "Here is the analysis, sir" before it plays.
  const wasVisibleRef = useRef(false);

  // Format the query as a heading:
  //   PR analysis  → "PR Analysis"
  //   Repo analysis → "Repository Analysis"
  const heading = useMemo(() => {
    if (!query) return "";
    const q = query.trim().toLowerCase();
    // PR analysis: "analyse PR #5 in repo", "review PR 12", "analyse pull request"
    if (/\bpr\b|\bpull\s*request\b/.test(q)) {
      return "PR Analysis";
    }
    // Repo analysis: "analyse repo", "analyse owner/repo", structured analysis data
    if (analysisData?.repo || /\banaly[sz]e\s+(?!pr\b|pull\b)/.test(q)) {
      return "Repository Analysis";
    }
    // Fallback: format the raw query
    const raw = query.trim();
    if (/analy[sz]e/i.test(raw)) {
      const match = raw.match(/analy[sz]e\s+(.+)/i);
      if (match) return `Analysis: ${match[1]}`;
    }
    return raw.charAt(0).toUpperCase() + raw.slice(1);
  }, [query, analysisData]);

  // Render markdown to sanitized HTML with custom enhancements
  const renderedHtml = useMemo(() => {
    return renderMarkdownToHtml(response);
  }, [response]);

  // Expose global hooks for direct IPC evaluation
  useEffect(() => {
    (window as any).__NEXUS_SET_SIDEBAR_CONTENT__ = (q: string, t: string) => {
      show(q, t);
    };
    (window as any).__NEXUS_HIDE_SIDEBAR__ = () => {
      stopTts();
      hide();
    };

    return () => {
      delete (window as any).__NEXUS_SET_SIDEBAR_CONTENT__;
      delete (window as any).__NEXUS_HIDE_SIDEBAR__;
    };
  }, [show, hide]);

  // Listen for Tauri events + fetch pending content on mount.
  //
  // When the sidebar window is created on-demand, the React app needs time
  // to load before it can receive Tauri events. So Rust stores the content
  // in a pending static, and we fetch it here on mount via
  // `get_pending_sidebar_content`. This is race-free.
  //
  // We also keep the event listeners as a fast path for when the window
  // already exists (React already loaded) — in that case Rust emits the
  // events directly.
  useEffect(() => {
    const unlisteners: (() => void)[] = [];

    // Fetch pending content on mount — handles the fresh-window case
    // where events were emitted before the listener was registered.
    invoke<{ query: string; text: string; backdrop: string | null; analysis?: any; confirmation?: any } | null>(
      "get_pending_sidebar_content"
    )
      .then((pending) => {
        console.log("[sidebar] get_pending_sidebar_content result:", pending ? `query=${pending.query?.length}chars text=${pending.text?.length}chars` : "null");
        if (pending) {
          if (pending.backdrop) {
            console.log("[sidebar] setting backdrop image");
            document.documentElement.style.setProperty(
              "--sidebar-backdrop-image",
              `url(${pending.backdrop})`
            );
          }
          // If confirmation data is present, render confirmation panel
          if (pending.confirmation) {
            console.log("[sidebar] showing with confirmation data");
            useSidebar.getState().showConfirmation(pending.confirmation);
          } else if (pending.analysis) {
            // If analysis data is present, use the rich dashboard view
            console.log("[sidebar] showing with analysis data");
            useSidebar.getState().showAnalysis(pending.query, pending.text, pending.analysis);
          } else {
            console.log("[sidebar] showing with plain text");
            show(pending.query, pending.text);
          }
        } else {
          console.warn("[sidebar] no pending content — sidebar will be empty");
        }
      })
      .catch((e) => console.error("[sidebar] get_pending_sidebar_content FAILED:", e));

    listen<{ query: string; text: string }>("sidebar:show", (event) => {
      show(event.payload.query, event.payload.text);
    }).then((u) => unlisteners.push(u));

    // Listen for structured analysis data from the Worker (rich dashboard)
    listen<{ query: string; text: string; analysis: any }>("sidebar:analysis", (event) => {
      const { query: q, text: t, analysis } = event.payload;
      if (analysis) {
        useSidebar.getState().showAnalysis(q, t, analysis);
      } else {
        show(q, t);
      }
    }).then((u) => unlisteners.push(u));

    // Listen for action confirmation requests (WhatsApp, Swiggy, GitHub)
    listen<{ query: string; prompt: string; confirmation: any }>("sidebar:confirmation", (event) => {
      const { confirmation } = event.payload;
      if (confirmation) {
        useSidebar.getState().showConfirmation(confirmation);
      }
    }).then((u) => unlisteners.push(u));

    listen("sidebar:hide", () => {
      stopTts();
      hide();
    }).then((u) => unlisteners.push(u));

    // "Fake blur" backdrop (Windows only — see sidebar_backdrop.rs).
    // Rust captures + blurs the screen region behind the window right
    // before it becomes visible and sends the result here as a data:
    // URI. Set directly as a CSS variable (not React state) so it
    // applies instantly without waiting on a render cycle.
    listen<string>("sidebar:backdrop", (event) => {
      document.documentElement.style.setProperty(
        "--sidebar-backdrop-image",
        `url(${event.payload})`
      );
    }).then((u) => unlisteners.push(u));

    return () => {
      unlisteners.forEach((u) => u());
    };
  }, [show, hide]);

  // Keyboard shortcut: Escape only closes the image lightbox (not the sidebar).
  // The sidebar itself is closed via Ctrl+Space (the global hotkey).
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && activeImage) {
        setActiveImage(null);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [activeImage, setActiveImage]);

  // Scroll to top when new response arrives
  useEffect(() => {
    if (responseScrollRef.current) {
      responseScrollRef.current.scrollTop = 0;
    }
  }, [response]);

  // Monitor scroll position for "Scroll to Top" button
  const handleScroll = useCallback(() => {
    if (!responseScrollRef.current) return;
    const st = responseScrollRef.current.scrollTop;
    setShowScrollTop(st > 250);
  }, []);

  // Native window visibility management
  // The sidebar window is shown by Rust (show_sidebar_with_content) before
  // this React app even loads, so we do NOT call invoke("show_sidebar") here.
  // We only need to call hide_sidebar when the user dismisses the sidebar.
  //
  // CRITICAL: only call stopTts() when visible transitions true→false
  // (user dismissed the sidebar). On initial mount, visible starts as
  // false — if we call stopTts() there, it kills "Here is the analysis,
  // sir" which wsBridge speaks right before the pending content arrives.
  useEffect(() => {
    if (!visible) {
      if (wasVisibleRef.current) {
        // User dismissed the sidebar → stop TTS and destroy the window
        stopTts();
        const t = setTimeout(() => invoke("hide_sidebar").catch(() => {}), 400);
        return () => clearTimeout(t);
      }
      // Initial mount (wasVisibleRef is false) → do nothing, don't kill TTS
    } else {
      wasVisibleRef.current = true;
    }
  }, [visible]);

  // Event delegation on the markdown container (handles Copy Code, Image Zoom, Links)
  const handleContainerClick = useCallback(
    async (e: React.MouseEvent<HTMLDivElement>) => {
      const target = e.target as HTMLElement;

      // 1. Copy Code button clicked
      const copyBtn = target.closest(".nexus-copy-code-btn") as HTMLButtonElement | null;
      if (copyBtn) {
        e.preventDefault();
        e.stopPropagation();
        const rawCode = copyBtn.dataset.code ? decodeURIComponent(copyBtn.dataset.code) : "";
        if (rawCode) {
          try {
            await navigator.clipboard.writeText(rawCode);
            const labelEl = copyBtn.querySelector(".nexus-btn-label");
            const originalText = labelEl ? labelEl.textContent : "Copy";
            if (labelEl) labelEl.textContent = "Copied!";
            copyBtn.classList.add("nexus-copied");
            setTimeout(() => {
              if (labelEl) labelEl.textContent = originalText;
              copyBtn.classList.remove("nexus-copied");
            }, 2000);
          } catch (err) {
            console.error("Failed to copy code:", err);
          }
        }
        return;
      }

      // 2. Image Zoom button or Image clicked
      const zoomBtn = target.closest(".nexus-image-zoom-btn") as HTMLButtonElement | null;
      const imgEl = target.closest(".nexus-image") as HTMLImageElement | null;
      if (zoomBtn || imgEl) {
        e.preventDefault();
        e.stopPropagation();
        const src = zoomBtn?.dataset.src || imgEl?.src || "";
        const alt = zoomBtn?.dataset.alt || imgEl?.alt || "Image preview";
        if (src) {
          setActiveImage({ src, alt });
        }
        return;
      }

      // 3. Link clicked
      const linkEl = target.closest(".nexus-link") as HTMLAnchorElement | null;
      if (linkEl) {
        e.preventDefault();
        e.stopPropagation();
        const href = linkEl.dataset.href || linkEl.href;
        if (href && href !== "#") {
          try {
            await openExternal(href);
          } catch {
            window.open(href, "_blank");
          }
        }
      }
    },
    [setActiveImage]
  );

  // Toggle Read Aloud (TTS)
  const handleToggleTts = () => {
    if (speaking) {
      stopTts();
      setSpeaking(false);
    } else {
      if (!response) return;
      setSpeaking(true);
      void speak(response, () => {
        setSpeaking(false);
      });
    }
  };

  // Scroll to top
  const scrollToTop = () => {
    responseScrollRef.current?.scrollTo({ top: 0, behavior: "smooth" });
  };

  return (
    <div id="sidebar-app" className={visible ? "sidebar--visible" : "sidebar--hidden"}>
      <div className={`sidebar-card ${confirmationData ? "mode--confirmation" : ""} font-size--${fontSize}`}>
        {/* ── Top Header Toolbar (hidden in confirmation mode for minimal UI) ── */}
        {!confirmationData && (
          <header className="sidebar-header">
            <div className="sidebar-header-actions">
              {/* Read Aloud (TTS) */}
              <button
                type="button"
                className={`sidebar-action-btn ${speaking ? "sidebar-action-btn--active" : ""}`}
                onClick={handleToggleTts}
                title={speaking ? "Stop reading aloud" : "Read aloud (TTS)"}
              >
                {speaking ? (
                  <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
                    <rect x="6" y="6" width="12" height="12" rx="2" />
                  </svg>
                ) : (
                  <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
                    <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
                    <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />
                  </svg>
                )}
              </button>
            </div>
            {/* Heading — shows the command/repo name */}
            {heading && (
              <div className="sidebar-header-heading">{heading}</div>
            )}
            <div className="sidebar-header-spacer" />
            {/* Settings (gear) button — opens the settings sidebar */}
            <button
              type="button"
              className="sidebar-action-btn"
              onClick={() => {
                import("@tauri-apps/api/core").then(({ invoke }) => {
                  invoke("show_settings_sidebar").catch(() => {});
                });
              }}
              title="Settings"
            >
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <circle cx="12" cy="12" r="3" />
                <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
              </svg>
            </button>
          </header>
        )}

        {/* ── Response Body ─────────────────────────────────────────── */}
        {/* Priority: conflict panel > confirmation panel > analysis dashboard > markdown */}
        <div className="sidebar-response" ref={responseScrollRef} onScroll={handleScroll}>
          {conflictData ? (
            <GitHubConflictPanel
              prNumber={conflictData.prNumber}
              repo={conflictData.repo}
              conflictFiles={conflictData.conflictFiles}
              message={conflictData.message}
            />
          ) : confirmationData ? (
            <ConfirmationPanel
              data={confirmationData}
              onClose={() => {
                hide();
              }}
            />
          ) : analysisData ? (
            <AnalysisDashboard data={analysisData} />
          ) : (
            <div
              className="nexus-markdown-body"
              dangerouslySetInnerHTML={{ __html: renderedHtml }}
              onClick={handleContainerClick}
            />
          )}
        </div>

        {/* ── Floating Scroll to Top button ──────────────────────────── */}
        {showScrollTop && !confirmationData && (
          <button type="button" className="sidebar-scroll-top-btn" onClick={scrollToTop} title="Scroll to top">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <polyline points="18 15 12 9 6 15" />
            </svg>
          </button>
        )}

        {/* ── Footer Status Bar (hidden in confirmation mode) ──────────────── */}
        {!confirmationData && (
          <footer className="sidebar-footer">
            <div className="sidebar-footer-hint">
              <kbd className="sidebar-kbd">Ctrl+Space</kbd> to close
            </div>
          </footer>
        )}
      </div>

      {/* ── Image Lightbox Modal ─────────────────────────────────────── */}
      {activeImage && (
        <div className="nexus-lightbox-overlay" onClick={() => setActiveImage(null)}>
          <div className="nexus-lightbox-content" onClick={(e) => e.stopPropagation()}>
            <div className="nexus-lightbox-header">
              <span className="nexus-lightbox-title">{activeImage.alt || "Image Preview"}</span>
              <div className="nexus-lightbox-actions">
                <button
                  type="button"
                  className="nexus-lightbox-btn"
                  onClick={() => openExternal(activeImage.src).catch(() => window.open(activeImage.src, "_blank"))}
                  title="Open in external browser"
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
                    <polyline points="15 3 21 3 21 9" />
                    <line x1="10" y1="14" x2="21" y2="3" />
                  </svg>
                </button>
                <button
                  type="button"
                  className="nexus-lightbox-btn nexus-lightbox-close"
                  onClick={() => setActiveImage(null)}
                  title="Close (Esc)"
                >
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <line x1="18" y1="6" x2="6" y2="18" />
                    <line x1="6" y1="6" x2="18" y2="18" />
                  </svg>
                </button>
              </div>
            </div>
            <div className="nexus-lightbox-body">
              <img src={activeImage.src} alt={activeImage.alt} className="nexus-lightbox-img" />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
