/**
 * Dev-only stage preview: `http://localhost:5173/stage.html?eyeball`.
 *
 * Renders the REAL stage (the WebGL orb, the aura, the pointer marker) in a
 * normal browser with fixture states, so the look of idle / listening /
 * thinking / speaking can be checked without a voice turn. It drives the real
 * store and hands the orb its rect through a DEV-only window event (there are
 * no Tauri events in a browser). Loaded by `stage/main.tsx` only when
 * `import.meta.env.DEV` is true, so production builds never contain it.
 */

import { useEffect, useState } from "react";
import "./eyeball.css";
import { useAssistant } from "../store/assistant";
import { PointerOverlay } from "../overlay/PointerOverlay";
import {
  EYEBALL_STATES,
  EYEBALL_STYLES,
  fixtureFromQuery,
  fixtureToQuery,
  orbRectFor,
  type EyeballFixture,
} from "./fixtures";

export const EYEBALL_RECT_EVENT = "nexus:eyeball-orb-rect";

function applyFixture(f: EyeballFixture) {
  const st = useAssistant.getState();
  st.setOrbCapsuleStyle(f.style);
  st.setOrbPosition(f.position);
  st.setVisible(true);
  st.setState(f.state);
  st.setTtsActive(f.state === "speaking");
  if (f.state !== "listening") st.setMicLevel(0);
  if (f.state !== "speaking") st.setAudioVolume(0);
  const dpr = window.devicePixelRatio || 1;
  window.dispatchEvent(
    new CustomEvent(EYEBALL_RECT_EVENT, {
      detail: orbRectFor(window.innerWidth, window.innerHeight, dpr, f.position, f.panel),
    }),
  );
  history.replaceState(null, "", `${location.pathname}${fixtureToQuery(f)}`);
}

export function EyeballPanel() {
  const [f, setF] = useState<EyeballFixture>(() => fixtureFromQuery(location.search));

  useEffect(() => {
    document.body.classList.add("eyeball-backdrop");
    document.documentElement.style.setProperty("--eyeball-panel", `${f.panel}px`);
    applyFixture(f);
    const onResize = () => applyFixture(f);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [f]);

  // A speech-like wobble around the fixture level, so the aura moves the way
  // it does with a real voice: the mic while listening, playback while speaking.
  useEffect(() => {
    if (f.state !== "listening" && f.state !== "speaking") return;
    const t0 = performance.now();
    const id = window.setInterval(() => {
      const t = (performance.now() - t0) / 1000;
      const v = f.level * (0.55 + 0.45 * Math.abs(Math.sin(t * 5.3) * Math.sin(t * 1.7)));
      const st = useAssistant.getState();
      if (f.state === "listening") st.setMicLevel(v);
      else st.setAudioVolume(v);
    }, 50);
    return () => window.clearInterval(id);
  }, [f.state, f.level]);

  const set = (patch: Partial<EyeballFixture>) => setF((cur) => ({ ...cur, ...patch }));
  const dpr = window.devicePixelRatio || 1;

  return (
    <>
      <div className="eyeball-panelbar" aria-hidden="true" />
      {f.pointer && (
        <PointerOverlay
          target={{ x: window.innerWidth * 0.68 * dpr, y: window.innerHeight * 0.62 * dpr, label: "Save button" }}
          fading={false}
        />
      )}
      <div className="eyeball-controls">
        <div className="eyeball-row">
          {EYEBALL_STATES.map((s) => (
            <button key={s} className={s === f.state ? "on" : ""} onClick={() => set({ state: s })}>
              {s}
            </button>
          ))}
        </div>
        <div className="eyeball-row">
          {EYEBALL_STYLES.map((s) => (
            <button key={s} className={s === f.style ? "on" : ""} onClick={() => set({ style: s })}>
              {s}
            </button>
          ))}
          <button onClick={() => set({ position: f.position === "top" ? "bottom" : "top" })}>{f.position}</button>
          <button className={f.pointer ? "on" : ""} onClick={() => set({ pointer: !f.pointer })}>
            pointer
          </button>
        </div>
        <label className="eyeball-row">
          level {f.level.toFixed(2)}
          <input
            type="range"
            min={0}
            max={1}
            step={0.05}
            value={f.level}
            onChange={(e) => set({ level: Number(e.target.value) })}
          />
        </label>
      </div>
    </>
  );
}
