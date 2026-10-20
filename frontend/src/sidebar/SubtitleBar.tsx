/**
 * "Last heard" bar — the editable copy of YOUR most recent spoken command,
 * fixable and resendable.
 *
 * Lives in the sidebar window's conversation view, not in the stage: the
 * stage is a click-through, keyboard-less overlay (it must never take
 * focus from the app underneath), so it cannot host a text field. The
 * sidebar is a normal window the user deliberately focuses to type.
 *
 * The bar only shows what NEXUS HEARD. What NEXUS says back stays in the
 * stage's ResponseCaption with real per-word karaoke (see readAlong.ts);
 * duplicating it here would put the same words on screen twice.
 *
 * Text source, in order:
 * - live `stt:transcript` events — emitted by Rust with `app.emit`, so
 *   every window receives them, including this one while it is open;
 * - a non-destructive Rust cache (`last_stt_transcript`) for when the
 *   sidebar was closed and re-opened after the words were spoken.
 *
 * Send does NOT run the turn here. The corrected line goes to Rust
 * (`stt_resubmit_transcript`), which re-broadcasts it as a normal
 * `stt:transcript` — so the stage runs it with its own turn ordering
 * (`runInTurnOrder`) and ghost state, exactly like a spoken command.
 *
 * This module deliberately does NOT import `useAssistant`: the sidebar is
 * a separate WebView/JS context with its own (empty) assistant store.
 * Everything else is imported lazily so the pure-function vitest suite can
 * import this file without touching Tauri.
 */

import { useEffect, useRef, useState } from "react";

/** Cap so a long line cannot grow the row across the card. */
export const MAX_SUBTITLE_CHARS = 220;

/**
 * Trim to `MAX_SUBTITLE_CHARS`, keeping the tail.
 *
 * The end of an utterance is the actionable part ("...send it to mom"), so
 * the head is what gets dropped when a long transcription wraps.
 */
export function clampSubtitle(text: string): string {
  if (text.length <= MAX_SUBTITLE_CHARS) return text;
  return `…${text.slice(text.length - MAX_SUBTITLE_CHARS)}`;
}

/**
 * Pure: the new draft when a NEW heard line landed, or `null` when the
 * draft must not be touched — typing must never be clobbered by a render
 * of the same line, and an empty heard line (a silent miss) must not wipe
 * a draft someone is mid-editing.
 */
export function nextDraft(synced: string, heard: string): string | null {
  if (heard.length === 0) return null;
  return heard === synced ? null : heard;
}

/**
 * Pure: extract the spoken text from an `stt:transcript` payload (or from
 * the cached string returned by `last_stt_transcript`). Returns `null` for
 * anything without usable text — notably the empty transcript emitted
 * after a silent miss, which must never erase the last thing actually
 * heard. Never returns an empty string.
 */
export function heardFromPayload(payload: unknown): string | null {
  const textOf = (value: unknown): string | null => {
    if (value === null || typeof value !== "object") return null;
    const text = (value as { text?: unknown }).text;
    return typeof text === "string" ? text : null;
  };

  let raw: string | null;
  if (typeof payload === "string") {
    // Tauri normally deserialises the payload for us. Tolerate a
    // JSON-encoded envelope too, without ever rendering `{"text":…}`.
    if (payload.trimStart().startsWith("{")) {
      try {
        raw = textOf(JSON.parse(payload));
      } catch {
        raw = payload;
      }
    } else {
      raw = payload;
    }
  } else {
    raw = textOf(payload);
  }

  const trimmed = raw?.trim() ?? "";
  return trimmed.length > 0 ? clampSubtitle(trimmed) : null;
}

/** Pure: the row exists exactly when there is a line worth correcting. */
export function shouldShowLastHeard(heard: string): boolean {
  return heard.trim().length > 0;
}

export function SubtitleBar() {
  const [heard, setHeard] = useState("");
  const [draft, setDraft] = useState("");
  const syncedRef = useRef("");
  const [sending, setSending] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const rearmTimerRef = useRef<number | null>(null);

  // Live events + one-time catch-up from Rust. The listener is registered
  // BEFORE the cache read so a turn landing mid-mount wins over the older
  // cached line instead of being overwritten by it.
  useEffect(() => {
    let alive = true;
    let sawLive = false;
    let unlisten: (() => void) | null = null;

    const apply = (payload: unknown, live: boolean) => {
      const line = heardFromPayload(payload);
      if (line === null) return;
      if (live) sawLive = true;
      setHeard(line);
    };

    void (async () => {
      try {
        const [{ invoke }, { listen }] = await Promise.all([
          import("@tauri-apps/api/core"),
          import("@tauri-apps/api/event"),
        ]);
        if (!alive) return;

        const un = await listen<unknown>("stt:transcript", (event) => {
          if (alive) apply(event.payload, true);
        });
        if (!alive) {
          un();
          return;
        }
        unlisten = un;

        try {
          const cached = await invoke<string | null>("last_stt_transcript");
          if (alive && !sawLive) apply(cached, false);
        } catch {
          /* command unavailable: the row simply starts empty */
        }
      } catch {
        /* outside Tauri (dev preview / pure tests) */
      }
    })();

    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  // Draft tracks the last heard line, but never clobbers typing: only
  // sync when a NEW line lands (nextDraft pins that rule as a pure
  // function so the regression stays testable).
  useEffect(() => {
    const next = nextDraft(syncedRef.current, heard);
    if (next !== null) {
      syncedRef.current = heard;
      setDraft(next);
    }
  }, [heard]);

  useEffect(() => {
    return () => {
      if (rearmTimerRef.current !== null) window.clearTimeout(rearmTimerRef.current);
    };
  }, []);

  const show = shouldShowLastHeard(heard);
  const canSend = show && !sending && draft.trim().length > 0;

  const send = () => {
    const text = draft.trim();
    if (!show || sending || text.length === 0) return;
    setSending(true);
    // Debounce the button, not the turn: the stage runs the turn to
    // completion (TTS + done handshake) while the button re-arms quickly.
    // Rust re-broadcasts the line to the stage; running `processTranscript`
    // in this WebView would use the sidebar's empty store and skip the
    // stage's turn ordering and ghost state.
    if (rearmTimerRef.current !== null) window.clearTimeout(rearmTimerRef.current);
    rearmTimerRef.current = window.setTimeout(() => {
      rearmTimerRef.current = null;
      setSending(false);
    }, 800);
    void import("@tauri-apps/api/core")
      .then(({ invoke }) => invoke("stt_resubmit_transcript", { text }))
      .catch((err) => {
        console.warn("[NEXUS] last-heard resubmit failed:", err);
      });
  };

  if (!show) return null;

  return (
    <div className="sidebar-last-heard" role="group" aria-label="What NEXUS heard">
      <div className="sidebar-last-heard__meta">
        <span className="sidebar-last-heard__label">Last heard</span>
        <span className="sidebar-last-heard__hint">Fix a word, then Send to retry</span>
      </div>
      <div className="sidebar-last-heard__row">
        <input
          ref={inputRef}
          className="sidebar-last-heard__input"
          value={draft}
          placeholder="Nothing heard yet"
          aria-label="Your last spoken command — edit and send to retry"
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            // Kept off the sidebar's window-level Escape handler (which
            // dismisses the whole panel): here Escape leaves the field.
            e.stopPropagation();
            if (e.key === "Enter") {
              e.preventDefault();
              send();
            } else if (e.key === "Escape") {
              e.preventDefault();
              inputRef.current?.blur();
            }
          }}
        />
        <button
          type="button"
          className="sidebar-last-heard__send"
          aria-label="Send corrected command"
          title="Send corrected command (Enter)"
          disabled={!canSend}
          onClick={(e) => {
            e.stopPropagation();
            send();
          }}
        >
          Send
        </button>
      </div>
    </div>
  );
}
