import { useEffect, useRef, useState } from "react";
import { positionToPct } from "../calibration/geometry";

function isTauri(): boolean {
  return typeof (window as any).__TAURI_INTERNALS__ !== "undefined";
}

async function tauriInvoke(cmd: string, args?: Record<string, unknown>): Promise<any> {
  if (!isTauri()) return;
  // Logged boundary (log-completeness P2): calibration report failures
  // must surface instead of failing silently.
  const { invokeLogged } = await import("../ipc");
  return invokeLogged(cmd, args);
}

interface LoadingRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * LoadingIndicator — the top-right Lottie spinner, retired from its own
 * `loading-indicator` OS window into a positioned div inside `stage`
 * (plan §1.6). Same placement math as before (`window_manager::
 * read_loading_settings`/`rect_for`), just consumed as a `stage:
 * loading_rect` event instead of a native window move/resize. Calibration
 * drag/wheel reimplemented as in-page pointer tracking (no `startDragging`
 * on a stage-hosted div — see Avatar.tsx's §1.5 for the same pattern).
 */
export function LoadingIndicator() {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const animRef = useRef<any>(null);

  const [rect, setRect] = useState<LoadingRect | null>(null);
  const [visible, setVisible] = useState(false);
  const [calibrating, setCalibrating] = useState(false);
  const [calibSize, setCalibSize] = useState(80);
  const [pulseKey, setPulseKey] = useState(0);

  // Lottie mount (once).
  useEffect(() => {
    let destroyed = false;
    void (async () => {
      const [lottieMod, data] = await Promise.all([
        import("lottie-web"),
        fetch("./loading.json").then((r) => r.json()),
      ]);
      if (destroyed || !containerRef.current) return;
      const lottie = lottieMod.default ?? lottieMod;
      animRef.current = lottie.loadAnimation({
        container: containerRef.current,
        renderer: "svg",
        loop: true,
        autoplay: true,
        animationData: data,
      });
    })().catch((err) => console.error("LoadingIndicator: lottie load failed", err));
    return () => {
      destroyed = true;
      animRef.current?.destroy?.();
      animRef.current = null;
    };
  }, []);

  // Rect (position/size), physical px from Rust → CSS px here. Pull the
  // pending value on mount (race-free, same pattern as OrbFrame).
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<LoadingRect>("stage:loading_rect", (event) => {
        if (event.payload) setRect(event.payload);
      });
      try {
        const pending = await tauriInvoke("get_pending_loading_rect");
        if (pending) setRect(pending as LoadingRect);
      } catch {
        /* non-Tauri (tests) — no-op */
      }
    })();
    return () => {
      unlisten?.();
    };
  }, []);

  // Visibility.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<boolean>("stage:loading_visible", (event) => {
        setVisible(event.payload === true);
      });
    })();
    return () => {
      unlisten?.();
    };
  }, []);

  // Calibration mirror: only react to the "loading" target (Wakeup/Waves
  // calibration owns the orb's preview instead — see OrbFrame).
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<{
        active?: boolean;
        target?: "wakeup" | "waves" | "loading";
        size?: number;
      }>("calibration:state", (event) => {
        const p = event.payload ?? {};
        if (p.active && p.target === "loading") {
          const entering = !calibrating;
          setCalibrating(true);
          setCalibSize(p.size ?? 80);
          if (entering) setPulseKey((k) => k + 1);
        } else if (p.active === false || (p.active && p.target !== "loading")) {
          setCalibrating(false);
        }
      });
    })();
    return () => {
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [calibrating]);

  const handlePointerDown = (e: React.PointerEvent) => {
    if (!calibrating) return;
    e.preventDefault();
    const hostRect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const offsetX = e.clientX - hostRect.left;
    const offsetY = e.clientY - hostRect.top;

    void (async () => {
      const [{ invoke }, { currentMonitor }] = await Promise.all([
        import("@tauri-apps/api/core"),
        import("@tauri-apps/api/window"),
      ]);
      const mon = await currentMonitor();
      if (!mon) return;
      const dpr = mon.scaleFactor;
      let pending: { h: number; v: number } | null = null;
      let reportTimer: ReturnType<typeof setTimeout> | null = null;

      const compute = (clientX: number, clientY: number) => {
        const physWin = Math.round(calibSize * dpr);
        const topLeftCssX = clientX - offsetX;
        const topLeftCssY = clientY - offsetY;
        return positionToPct(
          Math.round(topLeftCssX * dpr),
          Math.round(topLeftCssY * dpr),
          physWin,
          physWin,
          mon.size.width,
          mon.size.height,
          20 * dpr
        );
      };

      const flush = () => {
        if (!pending) return;
        const { h, v } = pending;
        pending = null;
        void invoke("calibration_report_position", { hPct: h, vPct: v }).catch(() => {});
      };

      const handleMove = (ev: PointerEvent) => {
        pending = compute(ev.clientX, ev.clientY);
        if (reportTimer) return;
        reportTimer = setTimeout(() => {
          reportTimer = null;
          flush();
        }, 16);
      };
      const handleUp = (ev: PointerEvent) => {
        window.removeEventListener("pointermove", handleMove);
        window.removeEventListener("pointerup", handleUp);
        if (reportTimer) {
          clearTimeout(reportTimer);
          reportTimer = null;
        }
        pending = compute(ev.clientX, ev.clientY);
        flush();
      };
      window.addEventListener("pointermove", handleMove);
      window.addEventListener("pointerup", handleUp);
    })();
  };

  const handleWheel = (e: React.WheelEvent) => {
    if (!calibrating) return;
    e.preventDefault();
    const [min, max] = [40, 160];
    const next = e.deltaY < 0 ? Math.min(max, calibSize + 10) : Math.max(min, calibSize - 10);
    setCalibSize(next);
    void tauriInvoke("calibration_report_size", { size: next }).catch(() => {});
  };

  // Position/size applied via direct DOM mutation, not React's `style`
  // prop — the stage window's CSP makes inline styles a silent no-op (see
  // OrbFrame.tsx's note). Static layout lives in the `.loading-indicator`
  // CSS class; only the per-render rect-derived values are genuinely
  // dynamic. `cursor` during calibration is handled by the
  // `.loading-indicator--calibrating` modifier class instead of JS.
  const applyRect = (el: HTMLDivElement | null) => {
    if (!el || !rect) return;
    const dpr = window.devicePixelRatio || 1;
    el.style.width = `${rect.w / dpr}px`;
    el.style.height = `${rect.h / dpr}px`;
    el.style.transform = `translate(${rect.x / dpr}px, ${rect.y / dpr}px)`;
  };
  const frameRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    applyRect(frameRef.current);
  }, [rect]);

  if (!rect || !(visible || calibrating)) return null;

  return (
    <div
      data-interactive
      className={`loading-indicator${calibrating ? " loading-indicator--calibrating" : ""}`}
      onPointerDown={handlePointerDown}
      onWheel={handleWheel}
      ref={(el) => {
        frameRef.current = el;
        applyRect(el);
      }}
    >
      {calibrating && <div className="calibration-hitcatcher" aria-hidden />}
      <div
        key={pulseKey}
        ref={containerRef}
        className={`loading-indicator-lottie${calibrating ? " loading-indicator-pulse" : ""}`}
      />
      {calibrating && (
        <div className="loading-indicator-badge">{calibSize} px</div>
      )}
    </div>
  );
}
