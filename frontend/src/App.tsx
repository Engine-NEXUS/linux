import { useEffect } from "react";
import { Avatar } from "./avatar/Avatar";
import { LoadingAnimation } from "./LoadingAnimation";
import { useAssistant } from "./store/assistant";
import { useRoam } from "./avatar/useRoam";

function isTauri(): boolean {
  return typeof (window as any).__TAURI_INTERNALS__ !== "undefined";
}

async function tauriInvoke(cmd: string, args?: Record<string, unknown>): Promise<any> {
  if (!isTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(cmd, args);
}

export default function App() {
  const state = useAssistant((s) => s.state);
  const visible = useAssistant((s) => s.visible);

  // Fullscreen stage: the orb roams when idle, parks when woken.
  const roaming = !visible;
  const { x, y, held, onPointerDown } = useRoam(roaming);

  // 8-second auto-hide: if user doesn't respond while listening, park off.
  // Also cleans up VAD + recording + mic stream to avoid orphaned AudioContexts.
  useEffect(() => {
    if (!visible || state !== "listening") return;
    const t = setTimeout(() => {
      // Stop VAD + recording + mic stream before hiding.
      import("./audio/vad").then(({ stopVad }) => stopVad()).catch(() => {});
      import("./audio/recorder").then(({ abortCapture }) => {
        void abortCapture().catch(() => {});
      }).catch(() => {});
      useAssistant.getState().setVisible(false);
      // Delay state reset until the fade finishes.
      setTimeout(() => useAssistant.getState().reset(), 350);
    }, 8000);
    return () => clearTimeout(t);
  }, [visible, state]);

  // Stage window stays shown natively (click-through when idle). These IPCs
  // only flip click-through: OFF when woken (orb interactive), ON when idle
  // (clicks pass through the invisible stage).
  useEffect(() => {
    tauriInvoke(visible ? "show_overlay" : "hide_overlay").catch(() => {});
  }, [visible]);

  // When state is active (not idle), ensure click-through is OFF.
  useEffect(() => {
    if (state === "idle") return;
    tauriInvoke("set_click_through", { ignore: false }).catch(() => {});
  }, [state]);

  return (
    <div id="app">
      <div
        className="stage-orb"
        data-interactive
        onPointerDown={onPointerDown}
        style={{ transform: `translate(${x}px, ${y}px)` }}
      >
        <div className={`avatar-section${held ? " avatar-section--held" : ""}${visible ? " avatar-section--awake" : ""}`}>
          {state === "thinking" ? <LoadingAnimation /> : <Avatar />}
        </div>
      </div>
    </div>
  );
}
