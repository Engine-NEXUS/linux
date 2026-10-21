import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type HandKey = "handMode" | "handMousePreview" | "handWithGhost" | "handSnap" | "handShowCamera" | "handGestures" | "handGesturesDesktop";

interface Props {
  settings: Partial<Record<HandKey, boolean>>;
  update: (key: HandKey, value: boolean) => void;
}

const ROWS: { key: HandKey; label: string; desc: string; fallback: boolean }[] = [
  { key: "handMode", label: "Allow hand mode", desc: "Lets \"hand mode\" turn the camera on. The camera is only on while the mode is.", fallback: true },
  { key: "handMousePreview", label: "Preview only (never click)", desc: "Off (default): after palm, fist, palm, fist the cursor follows your hand. On: it only shows what it would click.", fallback: false },
  { key: "handSnap", label: "Snap clicks to the nearest control", desc: "A pinch just outside a button lands on the button.", fallback: true },
  { key: "handGestures", label: "Gestures: tabs and windows", desc: "Three fingers up, then flick right/left = next/previous tab; flick up = window switcher (drop the pose to pick).", fallback: true },
  { key: "handGesturesDesktop", label: "Gestures: desktops (4 fingers together)", desc: "Flick right/left = other virtual desktop, up = Task View, down = show desktop. Off until you have checked it on your hand.", fallback: false },
  { key: "handShowCamera", label: "Show camera picture (debug)", desc: "A small window with the camera and the hand skeleton. Off by default: nothing is shown.", fallback: false },
  { key: "handWithGhost", label: "Start with Ghost mode", desc: "Turns hand mode on with Ghost mode and off with it.", fallback: false },
];

/** Command Hub card for hand mode (camera pointer). Toggles save with the rest of Settings. */
export function HandModeSection({ settings, update }: Props) {
  const [active, setActive] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    invoke<{ active: boolean }>("hand_status")
      .then((s) => setActive(s.active))
      .catch(() => {});
  }, []);

  const toggle = async () => {
    setBusy(true);
    try {
      if (active) {
        await invoke("hand_stop");
        setActive(false);
      } else {
        await invoke("hand_start");
        setActive(true);
      }
    } catch (e) {
      console.error("hand mode toggle failed:", e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="settings-section">
      <div className="settings-section-title">Hand Mode (camera pointer)</div>
      <div className="setting-row" style={{ alignItems: "center" }}>
        <div>
          <div className="setting-label">{active ? "Hand mode is on" : "Hand mode is off"}</div>
          <div className="setting-desc">Say "hand mode". Show palm, fist, palm, fist to arm the pointer.</div>
        </div>
        <div className="setting-control">
          <button className="settings-btn" disabled={busy || (!active && settings.handMode === false)} onClick={toggle}>
            {active ? "Turn off" : "Turn on"}
          </button>
        </div>
      </div>
      {ROWS.map((r) => {
        const on = settings[r.key] ?? r.fallback;
        return (
          <div className="setting-row" key={r.key}>
            <div>
              <div className="setting-label">{r.label}</div>
              <div className="setting-desc">{r.desc}</div>
            </div>
            <div className="setting-control">
              <div className={`settings-toggle ${on ? "settings-toggle--on" : ""}`} onClick={() => update(r.key, !on)} />
            </div>
          </div>
        );
      })}
    </div>
  );
}
