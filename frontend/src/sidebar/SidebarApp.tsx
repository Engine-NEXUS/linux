import { useEffect, useRef, useState, useMemo, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { open as openExternal } from "@tauri-apps/plugin-shell";
import { useSidebar, resolveEscDismiss } from "./sidebarStore";
import { renderMarkdownToHtml } from "./markdownRenderer";
import { stopTts } from "../audio/ttsPlayer";
import { AnalysisDashboard } from "./AnalysisDashboard";
import { GitHubConflictPanel } from "./GitHubConflictPanel";
import { ConfirmationPanel } from "./ConfirmationPanel";
import { SubtitleBar } from "./SubtitleBar";
import { topicTitle } from "./spatialShared";

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
interface SidebarAppProps {
  onDock: (d: string) => void;
}

export function SidebarApp({ onDock }: SidebarAppProps) {
  const visible = useSidebar((s) => s.visible);
  const response = useSidebar((s) => s.response);
  const query = useSidebar((s) => s.query);
  const fontSize = useSidebar((s) => s.fontSize);
  const activeImage = useSidebar((s) => s.activeImage);
  const analysisData = useSidebar((s) => s.analysisData);
  const conflictData = useSidebar((s) => s.conflictData);
  const confirmationData = useSidebar((s) => s.confirmationData);

  const show = useSidebar((s) => s.show);
  const hide = useSidebar((s) => s.hide);
  const setActiveImage = useSidebar((s) => s.setActiveImage);

  const responseScrollRef = useRef<HTMLDivElement>(null);
  const [showScrollTop, setShowScrollTop] = useState(false);
  // Track whether the sidebar was previously visible — prevents the
  // initial-mount useEffect (visible starts as false) from calling
  // stopTts() and killing "Here is the analysis, sir" before it plays.
  const wasVisibleRef = useRef(false);

  // Reference responses (PR/analyse/explain/find/research/search) show the
  // request topic above the result. Copy affordances are gated to exactly
  // this response class and hidden everywhere else.
  const showCopy = useMemo(
    () =>
      /\b(pr|pull\s*request|analy[sz]e|analysis|explain|find|research|google|search)\b/i.test(
        query || ""
      ),
    [query]
  );
  const topicTitleText = useMemo(() => topicTitle(query || ""), [query]);

  useEffect(() => {
    document.documentElement.dataset.copyEnabled = showCopy ? "true" : "false";
    return () => {
      delete document.documentElement.dataset.copyEnabled;
    };
  }, [showCopy]);

  // Render markdown to sanitized HTML with custom enhancements
  const renderedHtml = useMemo(() => {
    return renderMarkdownToHtml(response);
  }, [response]);

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

    // (View routing lives in the UnifiedSidebar shell — it listens for
    // sidebar:set_view and switches the mounted view. No listener here.)

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

    // Sample background luminance under sidebar to automatically adapt light/dark mode
    // (reads the window's ACTUAL rect — layout-agnostic across views/docks)
    const syncLuminanceAndHitbox = () => {
      const dpr = window.devicePixelRatio || 1;
      const w = Math.round(window.innerWidth * dpr);
      const h = Math.round(window.innerHeight * dpr);
      const x = Math.round((window.screenX || 0) * dpr);
      const y = Math.max(0, Math.round((window.screenY || 0) * dpr));

      invoke("get_screen_luminance", { x, y, w, h })
        .then((res: any) => {
          if (res?.mode) {
            document.documentElement.setAttribute("data-glass-luminance", res.mode);
          }
        })
        .catch(() => {});
    };

    syncLuminanceAndHitbox();
    const probeInterval = setInterval(syncLuminanceAndHitbox, 1500);

    return () => {
      unlisteners.forEach((u) => u());
      clearInterval(probeInterval);
    };
  }, [show, hide]);

  // Keyboard: Escape closes the image lightbox first, then the sidebar
  // itself via the store's hide() — so the standard dismiss path runs
  // (visible→false stops TTS and destroys the window after 400ms).
  // Window-level only: no global hotkey is registered for this.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        // Hierarchy locked by resolveEscDismiss (unit-tested in sidebarStore).
        if (resolveEscDismiss(activeImage !== null) === "close-overlay") {
          setActiveImage(null); // Lightbox closes first
          return;
        }
        hide(); // Sidebar dismiss (TTS-stop + hide_sidebar handled downstream)
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [activeImage, setActiveImage, hide]);

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


  // Scroll to top
  const scrollToTop = () => {
    responseScrollRef.current?.scrollTo({ top: 0, behavior: "smooth" });
  };

  return (
    <div id="sidebar-app" className={visible ? "sidebar--visible" : "sidebar--hidden"}>
        <div className={`sidebar-card ${confirmationData ? "mode--confirmation" : ""} font-size--${fontSize}`}>
          {/* ── Header row: drag region + dock controls (Left/Right) ────── */}
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

          {/* ── Last heard: editable copy of your most recent command ───── */}
          {/* Lives here (not in the stage) because the stage is a
              keyboard-less click-through overlay; this window is focused
              deliberately by the user to type a correction. */}
          <SubtitleBar />

          {/* ── Topic title — ONLY for research / analyse responses ────── */}
          {/* The search/research topic heads the information block here;
              this is also the only response class that shows Copy Code. */}
          {showCopy && response && topicTitleText ? (
            <div className="sidebar-topic-title">{topicTitleText}</div>
          ) : null}

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
                <kbd className="sidebar-kbd">Esc</kbd> to close
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
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
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
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
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

