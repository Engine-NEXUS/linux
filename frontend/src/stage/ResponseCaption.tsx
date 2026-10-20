import { useEffect, useRef, useState } from "react";
import { initCaptionListener, onCaptionLineUpdate, CaptionLineEvent } from "../audio/captionScheduler";
import { readAlongFromEvent } from "../sidebar/readAlong";

interface OrbRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * ResponseCaption — the spoken reply's text, appearing line-by-line / clause-by-clause
 * outside the black capsule (top dock: below the capsule; bottom dock: above the capsule).
 * Synchronized with the Voice Orb: holds the orb alive until the final line clears.
 */
export function ResponseCaption() {
  const [rect, setRect] = useState<OrbRect | null>(null);
  const [lineEvent, setLineEvent] = useState<CaptionLineEvent | null>(null);

  useEffect(() => {
    initCaptionListener();
    const unsub = onCaptionLineUpdate((event, done) => {
      setLineEvent(event);
      if (done && event.phase === "cleared") {
        setLineEvent(null);
      }
    });
    return () => {
      unsub();
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<OrbRect>("stage:orb_rect", (event) => {
        if (event.payload) setRect(event.payload);
      });
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const pending = await invoke<OrbRect | null>("get_pending_orb_rect");
        if (pending) setRect(pending);
      } catch {
        /* non-Tauri (tests) — no-op */
      }
    })();
    return () => {
      unlisten?.();
    };
  }, []);

  // Position/opacity applied via direct DOM mutation, not React's `style`
  // prop — the stage window's CSP makes inline styles a silent no-op.
  // Slot is user-tuned (caption-tuner 2026-10-09: center-x 51.5%,
  // top 89%): percentages of the fullscreen stage, so they scale from
  // the tuner's 2133x1056 viewport to any screen size.
  const applyPosition = (el: HTMLDivElement | null) => {
    if (!el || !rect) return;
    const dpr = window.devicePixelRatio || 1;
    const topY = rect.y / dpr;
    const isTop = topY < (typeof window !== "undefined" ? window.innerHeight / 2 : 540);

    if (isTop) {
      const centerX = window.innerWidth * 0.515;
      const y = window.innerHeight * 0.895;
      el.style.transform = `translate(${centerX}px, ${y}px) translateX(-50%)`;
    } else {
      // Bottom-dock fallback (untuned): keep the legacy above-orb slot
      // so the caption never collides with a bottom-docked orb.
      const centerX = window.innerWidth / 2;
      const bottomY = (rect.y + rect.h) / dpr;
      el.style.transform = `translate(${centerX}px, ${bottomY + 20}px) translateX(-50%)`;
    }
    el.style.opacity = lineEvent && lineEvent.phase !== "cleared" ? "1" : "0";
  };

  const captionRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    applyPosition(captionRef.current);
  }, [rect, lineEvent]);

  if (!rect || !lineEvent || lineEvent.phase === "cleared") return null;

  const currentText = lineEvent.text || "";
  if (!currentText.trim()) return null;

  // Per-word karaoke from the scheduler's REAL word data (Edge word
  // boundaries; the scheduler's own reveal schedule for local engines).
  // `null` falls back to the plain-text render — never an estimate.
  const readAlong = lineEvent ? readAlongFromEvent(lineEvent) : null;

  const phaseClass =
    lineEvent.phase === "active"
      ? "caption-black-capsule--active"
      : lineEvent.phase === "fading"
      ? "caption-black-capsule--fading"
      : "";

  return (
    <div
      className="response-caption response-caption--black"
      ref={(el) => {
        captionRef.current = el;
        applyPosition(el);
      }}
    >
      <div className={`caption-black-capsule ${phaseClass}`}>
        <span className="caption-black-text">
          {readAlong
            ? readAlong.map((w, i) => (
                <span key={i} className={`caption-word caption-word--${w.state}`}>
                  {w.text}
                  {i < readAlong.length - 1 ? " " : ""}
                </span>
              ))
            : currentText}
        </span>
      </div>
    </div>
  );
}

