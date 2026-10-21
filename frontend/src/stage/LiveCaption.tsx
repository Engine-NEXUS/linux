import { useEffect, useRef, useState } from "react";

interface OrbRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * LiveCaption — the user's OWN words, growing while they're still talking
 * (plan Phase 4). Entirely best-effort: driven by Rust's `stt:partial`
 * event (itself fed by the Python STT server's optional `/stream`
 * WebSocket — see `server/stt_server.py` and `stt_stream.rs`), which may
 * simply never arrive if the stream failed to start. In that case this
 * component just never renders anything — the real transcript still
 * arrives normally via the existing batch `stt:transcript` path.
 *
 * Same family/positioning as `ResponseCaption` (the user-tuned screen
 * slot) — the two are temporally mutually exclusive (this one only has
 * text while listening; ResponseCaption only has text while speaking),
 * so sharing the same screen slot never conflicts.
 */
export function LiveCaption() {
  const [rect, setRect] = useState<OrbRect | null>(null);
  const [text, setText] = useState("");

  useEffect(() => {
    let unlistenPartial: (() => void) | null = null;
    let unlistenFinal: (() => void) | null = null;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      // Moonshine's LineTextChanged gives the full growing line each time.
      // Format as 5-word sliding FIFO window for clean display.
      unlistenPartial = await listen<{ text?: string }>("stt:partial", (event) => {
        const t = event.payload?.text;
        if (typeof t === "string") {
          const words = t.trim().split(/\s+/).filter(Boolean);
          const windowed = words.length > 5 ? words.slice(-5).join(" ") : t;
          setText(windowed);
        }
      });
      // Turn end: the real batch transcript arrived (or came back empty) —
      // clear so the NEXT turn's listening phase starts from blank instead
      // of flashing this turn's last words.
      unlistenFinal = await listen("stt:transcript", () => setText(""));
    })();
    return () => {
      unlistenPartial?.();
      unlistenFinal?.();
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

  // Position applied via direct DOM mutation, not React's `style` prop —
  // the stage window's CSP makes inline styles a silent no-op (see
  // OrbFrame.tsx's note). Static layout lives in the shared
  // `.response-caption` CSS class; only the per-render transform is
  // genuinely dynamic.
  const applyPosition = (el: HTMLDivElement | null) => {
    if (!el || !rect) return;
    const dpr = window.devicePixelRatio || 1;
    const topY = rect.y / dpr;
    const isTop = topY < (typeof window !== "undefined" ? window.innerHeight / 2 : 540);

    // Same user-tuned slot as ResponseCaption (caption-tuner 2026-10-09).
    if (isTop) {
      const centerX = window.innerWidth * 0.515;
      const y = window.innerHeight * 0.895;
      el.style.transform = `translate(${centerX - 260}px, ${y}px)`;
    } else {
      const centerX = typeof window !== "undefined" ? window.innerWidth / 2 : rect.x / dpr + rect.w / dpr / 2;
      const bottomY = (rect.y + rect.h) / dpr;
      el.style.transform = `translate(${centerX - 260}px, ${bottomY + 24}px)`;
    }
  };
  const captionRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    applyPosition(captionRef.current);
  }, [rect]);

  if (!rect || !text.trim()) return null;

  return (
    <div
      className="response-caption live-caption"
      ref={(el) => {
        captionRef.current = el;
        applyPosition(el);
      }}
    >
      {text}
    </div>
  );
}
