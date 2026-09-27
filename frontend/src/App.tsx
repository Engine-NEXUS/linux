import { useEffect, useRef } from "react";
import { Avatar } from "./avatar/Avatar";
import { LoadingAnimation } from "./LoadingAnimation";
import { useAssistant } from "./store/assistant";
import { useRoam } from "./avatar/useRoam";
import { initOrchestratorListener } from "./net/orchestrator";

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
  const loadingVisible = useAssistant((s) => s.loadingVisible);

  // Initialize the central orchestrator event listener (once).
  // This listens to "orchestrator:event" from Rust and handles:
  //   - state transitions (thinking → speaking)
  //   - loading indicator visibility
  //   - ack TTS ("On it sir")
  //   - result TTS + sidebar display
  //   - error handling
  useEffect(() => {
    void initOrchestratorListener();
    void import("./audio/ttsActivity").then(({ initTtsActivityListener }) =>
      initTtsActivityListener(),
    );
  }, []);

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

  // Speaking failsafe (origin): if TTS ends without onEnd the orb parks
  // in `speaking` forever. Silent 60s in speaking forces reset.
  useEffect(() => {
    if (state !== "speaking") return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const arm = () => {
      timer = setTimeout(() => {
        timer = null;
        if (cancelled || useAssistant.getState().state !== "speaking") return;
        import("./audio/ttsPlayer")
          .then(({ isRustTtsPlaying }) => {
            if (cancelled || useAssistant.getState().state !== "speaking") return;
            if (isRustTtsPlaying()) { arm(); return; }
            console.warn("[NEXUS] speaking failsafe: silent 60s, forcing reset");
            const s = useAssistant.getState();
            s.setLoadingVisible(false);
            s.setVisible(true);
            setTimeout(() => useAssistant.getState().reset(), 550);
          })
          .catch(() => {});
      }, 60000);
    };
    arm();
    return () => { cancelled = true; if (timer) clearTimeout(timer); };
  }, [state]);
  // Linux fullscreen stage: window stays shown natively (click-through when
  // idle). These IPCs only flip click-through: OFF when woken, ON when idle.
  useEffect(() => {
    tauriInvoke(visible ? "show_overlay" : "hide_overlay").catch(() => {});
  }, [visible]);

  // When state is active (not idle), ensure click-through is OFF.
  useEffect(() => {
    if (state === "idle") return;
    tauriInvoke("set_click_through", { ignore: false }).catch(() => {});
  }, [state]);

  // Loading window management — show/hide a separate Tauri window at the
  // top-right corner of the screen. This window contains the Lottie loading
  // animation and is completely independent from the orb window.
  useEffect(() => {
    if (loadingVisible) {
      console.log("[NEXUS] loading: showing loading window at top-right corner");
      tauriInvoke("show_loading_indicator").catch((e) =>
        console.warn("[NEXUS] loading: show_loading_indicator failed:", e)
      );
    } else {
      console.log("[NEXUS] loading: hiding loading window");
      tauriInvoke("hide_loading_indicator").catch((e) =>
        console.warn("[NEXUS] loading: hide_loading_indicator failed:", e)
      );
    }
  }, [loadingVisible]);

  // Cleanup: destroy the loading window when the App unmounts
  useEffect(() => {
    return () => {
      tauriInvoke("hide_loading_indicator").catch(() => {});
    };
  }, []);

  return (
    // Linux fullscreen stage: orb roams inside native window via transform.
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
