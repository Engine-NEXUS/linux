import { useCallback, useEffect, useRef, useState } from "react";
// Logged boundary (log-completeness P2): every invoke here is a
// calibration command — silent failure strands the calibrator, so
// failures warn (callers keep their `.catch` control flow).
import { invokeLogged as invoke } from "../ipc";
import { listen } from "@tauri-apps/api/event";
import { resolveCalibrationKey } from "./calibrationKeys";
import { pxToSlider, sliderToPx } from "../calibration/geometry";
import {
  commitChange,
  commitCoalesced,
  NUDGE_COALESCE_MS,
  popUndo,
  type CalibrationSnapshot,
  type TargetRect,
} from "./history";
import { GlassFilter } from "./GlassFilter";
import { Slider } from "./Slider";
import "./companion-hud.css";

type Target = "wakeup" | "waves" | "loading";

interface StatePayload {
  active: boolean;
  target: Target;
  h: number;
  v: number;
  size: number;
  dirty?: { wakeup: boolean; waves: boolean; loading: boolean };
}

interface Drafts {
  wakeup: TargetRect;
  waves: TargetRect;
  loading: TargetRect;
}

const TARGETS: { id: Target; label: string }[] = [
  { id: "wakeup", label: "Wakeup" },
  { id: "waves", label: "Waves" },
  { id: "loading", label: "Loading" },
];

export function CompanionHudApp() {
  const [target, setTarget] = useState<Target>("wakeup");
  const [dirty, setDirty] = useState<Record<Target, boolean>>({
    wakeup: false,
    waves: false,
    loading: false,
  });
  const [drafts, setDrafts] = useState<Drafts | null>(null);
  const [history, setHistory] = useState<{
    past: CalibrationSnapshot[];
    present: CalibrationSnapshot;
  }>({ past: [], present: toSnapshot(null) });

  function toSnapshot(d: Drafts | null): CalibrationSnapshot {
    return {
      wakeup: d?.wakeup ?? { h: 0.5, v: 1.0, size: 200 },
      waves: d?.waves ?? { h: 0.5, v: 1.0, size: 200 },
      loading: d?.loading ?? { h: 0.95, v: 0.05, size: 80 },
    };
  }

  // Mirror the Rust session state (target + active draft + size badge).
  useEffect(() => {
    let un: (() => void) | null = null;
    listen<StatePayload>("calibration:state", (ev) => {
      const p = ev.payload;
      if (!p || !p.active) return;
      setTarget(p.target);
      if (p.dirty) setDirty({ ...p.dirty });
      const rect: TargetRect = { h: p.h, v: p.v, size: p.size };
      setDrafts((prev) => ({ ...toSnapshot(prev), [p.target]: rect }));
    }).then((u) => {
      un = u;
    });
    return () => {
      un?.();
    };
  }, []);

  const switchTarget = useCallback((t: Target) => {
    invoke("calibration_set_target", { target: t }).catch(() => {});
  }, []);

  const undo = useCallback(() => {
    setHistory((h) => {
      const { next, restored } = popUndo(h);
      if (restored) {
        invoke("calibration_apply_drafts", { drafts: toArray(restored) }).catch(
          () => {}
        );
      }
      return next;
    });
  }, []);

  const defaultTarget = useCallback(() => {
    invoke("calibration_default_target").catch(() => {});
  }, []);

  const cancel = useCallback(() => {
    invoke("calibration_cancel").catch(() => {});
  }, []);

  const save = useCallback(() => {
    invoke("calibration_save").catch(() => {});
  }, []);

  // Nudge-burst coalescing: consecutive nudges to the same target within
  // NUDGE_COALESCE_MS share one undo entry (live motion stays 1:1).
  const nudgeMarkRef = useRef<{ target: Target; until: number } | null>(null);

  // Esc = cancel, Enter = save, 1/2/3 = select, arrows = nudge.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const action = resolveCalibrationKey(e.code, e.key, e.shiftKey);
      if (action.kind === "none") return;
      if (action.kind === "nudge") e.preventDefault();
      if (action.kind === "select") {
        void invoke("calibration_set_target", { target: action.target }).catch(
          () => {}
        );
      } else if (action.kind === "nudge") {
        nudgeMarkRef.current = {
          target,
          until: Date.now() + NUDGE_COALESCE_MS,
        };
        void invoke("calibration_nudge", {
          dxPx: action.dx,
          dyPx: action.dy,
        }).catch(() => {});
      } else if (action.kind === "save") {
        void invoke("calibration_save").catch(() => {});
      } else if (action.kind === "cancel") {
        void invoke("calibration_cancel").catch(() => {});
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [target]);

  // Track history pushes
  useEffect(() => {
    let un: (() => void) | null = null;
    let last: Drafts | null = null;
    listen<StatePayload>("calibration:state", (ev) => {
      const p = ev.payload;
      if (!p || !p.active) return;
      const rect: TargetRect = { h: p.h, v: p.v, size: p.size };
      const before = last ? toSnapshot(last) : history.present;
      const after = { ...toSnapshot(last), [p.target]: rect };
      if (JSON.stringify(before) !== JSON.stringify(after)) {
        const mark = nudgeMarkRef.current;
        const coalesce =
          mark !== null && mark.target === p.target && Date.now() < mark.until;
        setHistory((h) =>
          coalesce
            ? commitCoalesced(h, after)
            : commitChange(h, before, after)
        );
      }
      last = { ...toSnapshot(last), [p.target]: rect };
    }).then((u) => {
      un = u;
    });
    return () => {
      un?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const activeSize =
    drafts?.[target]?.size ?? (target === "loading" ? 80 : 200);

  // Plan 04 §2: the slider reads 0–100 for every target; px is controlled
  // in the background (mapped here + in Rust clamps). The thumb seats from
  // the live draft, so target switch / wheel / drag / undo re-seat it.
  // Loading quantization (1.2px/step): skip px no-ops so duplicate steps
  // never invoke. Drags coalesce into one undo entry (nudge mechanism).
  const lastSliderPxRef = useRef<number | null>(null);
  const handleSliderChange = useCallback(
    (val: number[]) => {
      if (val[0] == null) return;
      const px = sliderToPx(target, val[0]);
      if (px === lastSliderPxRef.current) return;
      lastSliderPxRef.current = px;
      nudgeMarkRef.current = {
        target,
        until: Date.now() + NUDGE_COALESCE_MS,
      };
      invoke("calibration_report_size", { size: px }).catch(() => {});
    },
    [target]
  );
  const handleSliderCommit = useCallback(() => {
    // Let the nudge mark expire naturally (800ms): the final state event
    // from this drag still lands inside the window and coalesces, so one
    // drag = one undo entry. Only reset the px guard for the next gesture.
    lastSliderPxRef.current = null;
  }, []);

  return (
    <div className="hud" data-target={target}>
      {/* Row 1: Liquid Radio (3 options) + Waves Preview + Action Buttons */}
      <div className="hud-row hud-row--main">
        {/* Liquid Glass Radio Selector */}
        <div className="liquid-radio-container" data-state={target}>
          <div className="liquid-radio-glass-layer" />
          <div className="liquid-radio-pill" />
          {TARGETS.map((t) => (
            <button
              key={t.id}
              type="button"
              className={`liquid-radio-item ${target === t.id ? "active" : ""}`}
              onClick={() => switchTarget(t.id)}
            >
              <span className="liquid-radio-label">{t.label}</span>
              {dirty[t.id] && <span className="liquid-radio-dot" title="Positioned ✓" />}
            </button>
          ))}
          <GlassFilter />
        </div>

        {/* Waves Preview — animated bars shown when Waves target is active */}
        {target === "waves" && (
          <div className="hud-waves-preview" aria-label="Waves preview">
            {[0.4, 0.7, 1.0, 0.75, 0.5, 0.85, 0.6].map((h, i) => (
              <div
                key={i}
                className="hud-waves-bar"
                style={{ "--bar-delay": `${i * 0.08}s`, "--bar-base": h } as React.CSSProperties}
              />
            ))}
          </div>
        )}

        {/* Action Controls */}
        <div className="hud-actions">
          <button className="hud-btn hud-btn--icon" onClick={undo} title="Undo last change (↶)">
            ↶
          </button>
          <button className="hud-btn hud-btn--icon" onClick={defaultTarget} title="Reset target to default (↺)">
            ↺
          </button>
          <button className="hud-btn hud-btn--cancel" onClick={cancel} title="Cancel calibration (Esc)">
            ✕ Cancel
          </button>
          <button className="hud-btn hud-btn--save" onClick={save} title="Save to settings.json (Enter)">
            ✓ Save
          </button>
        </div>
      </div>

      {/* Row 2: Size slider (0–100; px in background) with NumberFlow */}
      <div className="hud-row hud-row--slider">
        <span className="hud-slider-label">Size</span>
        <div className="hud-slider-container">
          <Slider
            value={[pxToSlider(target, activeSize)]}
            min={0}
            max={100}
            step={1}
            onValueChange={handleSliderChange}
            onValueCommit={handleSliderCommit}
          />
        </div>
      </div>
    </div>
  );
}

function toArray(s: CalibrationSnapshot) {
  return [s.wakeup, s.waves, s.loading];
}

export default CompanionHudApp;
