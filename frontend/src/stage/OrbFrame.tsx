import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Avatar } from "../avatar/Avatar";
import { AuraCanvas } from "../aura/AuraCanvas";
import { useAssistant } from "../store/assistant";
import { initOrchestratorListener, hideOrbAfterSpeech } from "../net/orchestrator";
import { stepWatchdog, INITIAL_WATCHDOG_STATE, type WatchdogState } from "../net/orbIdleWatchdog";
import { initialGhostOrbState, stepGhostOrb, type GhostOrbState } from "../net/orbPolicy";
import { isGhostParked } from "../net/ghostHotMic";

function isTauri(): boolean {
  return typeof (window as any).__TAURI_INTERNALS__ !== "undefined";
}

async function tauriInvoke(cmd: string, args?: Record<string, unknown>): Promise<any> {
  if (!isTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(cmd, args);
}

interface OrbRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

function computeStretch(tau: number, mode: number, k: number): number {
  tau = Math.max(0, Math.min(1, tau));
  if (tau <= 0) return 0;
  if (tau >= 1) return 1;

  if (mode === 1) {
    // Mode 1: Elastic Thread Stretch & Snap (Direct user preference)
    const decay = Math.exp(-4.6 * tau * k);
    const osc = Math.cos(7.2 * tau * Math.sqrt(k));
    const envelope = 1.0 - Math.pow(tau, 3.0);
    return 1.0 - decay * osc * envelope;
  } else if (mode === 2) {
    // Mode 2: Bowstring Pluck & Rebound
    const plungeEnd = 0.26 / k;
    if (tau < plungeEnd) {
      return Math.pow(tau / plungeEnd, 1.7) * 1.25;
    } else {
      const tRel = (tau - plungeEnd) / (1 - plungeEnd);
      return 1.0 + 0.25 * Math.exp(-6.8 * tRel * k) * Math.cos(15.0 * tRel * Math.sqrt(k));
    }
  } else if (mode === 3) {
    // Mode 3: Liquid Meniscus Extrusion
    const alpha = 3.6 * k;
    return 1.0 - (1.0 + alpha * tau) * Math.exp(-alpha * tau);
  } else if (mode === 4) {
    // Mode 4: Horizon Split & Wing Flare
    return 1.0 - Math.exp(-5.2 * tau * k) * Math.cos(5.8 * tau * Math.sqrt(k));
  } else if (mode === 5) {
    // Mode 5: Acoustic Harmonic Ripple
    const base = 1.0 - Math.exp(-4.2 * tau * k) * Math.cos(6.2 * tau * Math.sqrt(k));
    const ripple = 0.08 * Math.exp(-5.0 * tau) * Math.sin(26.0 * tau * Math.sqrt(k));
    return base + ripple;
  }
  return 1.0;
}

/**
 * Fluid Bezel Scoop with Elastic Thread Stretch Entrance (Feature 106)
 * Continuous catenary Bézier curve dropping from the screen bezel,
 * with the Voice Orb circle nestled inside the sagging pocket.
 */
function PebbleScoopFrame({
  isShown,
  pos,
  entered,
  dispersing,
  wakeupMode = 1,
}: {
  isShown: boolean;
  pos: "top" | "bottom";
  entered: boolean;
  dispersing: boolean;
  wakeupMode?: number;
}) {
  const fillPathRef = useRef<SVGPathElement | null>(null);
  const rimPathRef = useRef<SVGPathElement | null>(null);
  const wingLeftRef = useRef<SVGPathElement | null>(null);
  const wingRightRef = useRef<SVGPathElement | null>(null);
  const cradleRef = useRef<HTMLDivElement | null>(null);
  const rafRef = useRef<number | null>(null);
  const prevShownRef = useRef<boolean>(false);
  const isInitialMount = useRef<boolean>(true);

  const W = 301;
  const H = 105;
  const wingWidth = 120;
  const k = 1.03;

  const buildPath = (stretch: number, tau: number = 1.0, mode: number = 1) => {
    const halfW = W / 2;
    const cx = halfW;
    const currentH = Math.max(1, H * stretch);

    let effW = W;
    let leftX = 0;
    let rightX = W;
    if (mode === 4) {
      const unroll = Math.min(1.0, tau * 1.85);
      effW = Math.max(60, W * unroll);
      leftX = cx - effW / 2;
      rightX = cx + effW / 2;
    }

    const wing = Math.min(wingWidth, effW * 0.25);
    let rippleDy = 0;
    if (mode === 5 && tau < 0.85) {
      rippleDy = Math.sin(tau * 30.0) * Math.exp(-4.5 * tau) * 12.0;
    }

    if (pos === "top") {
      const cp1x = leftX + wing;
      const cp1y = 0;
      const cp2x = cx - (effW / 2) * 0.52;
      const cp2y = currentH + rippleDy;
      const cp3x = cx + (effW / 2) * 0.52;
      const cp3y = currentH - rippleDy;
      const cp4x = rightX - wing;
      const cp4y = 0;

      const scoopCurve = `M ${leftX},0 C ${cp1x},${cp1y} ${cp2x},${cp2y} ${cx},${currentH} C ${cp3x},${cp3y} ${cp4x},${cp4y} ${rightX},0`;
      const closedPath = `${scoopCurve} Z`;
      const rimPath = scoopCurve;
      const wingLeft = `M ${leftX},0 C ${leftX + wing * 0.5},0 ${leftX + wing * 0.9},${currentH * 0.22} ${leftX + wing * 1.3},${currentH * 0.42}`;
      const wingRight = `M ${rightX},0 C ${rightX - wing * 0.5},0 ${rightX - wing * 0.9},${currentH * 0.22} ${rightX - wing * 1.3},${currentH * 0.42}`;

      return { closedPath, rimPath, wingLeft, wingRight, currentH };
    } else {
      const topY = H - currentH;
      const cp1x = leftX + wing;
      const cp1y = H;
      const cp2x = cx - (effW / 2) * 0.52;
      const cp2y = topY + rippleDy;
      const cp3x = cx + (effW / 2) * 0.52;
      const cp3y = topY - rippleDy;
      const cp4x = rightX - wing;
      const cp4y = H;

      const scoopCurve = `M ${leftX},${H} C ${cp1x},${cp1y} ${cp2x},${cp2y} ${cx},${topY} C ${cp3x},${cp3y} ${cp4x},${cp4y} ${rightX},${H}`;
      const closedPath = `${scoopCurve} Z`;
      const rimPath = scoopCurve;
      const wingLeft = `M ${leftX},${H} C ${leftX + wing * 0.5},${H} ${leftX + wing * 0.9},${H - currentH * 0.22} ${leftX + wing * 1.3},${H - currentH * 0.42}`;
      const wingRight = `M ${rightX},${H} C ${rightX - wing * 0.5},${H} ${rightX - wing * 0.9},${H - currentH * 0.22} ${rightX - wing * 1.3},${H - currentH * 0.42}`;

      return { closedPath, rimPath, wingLeft, wingRight, currentH };
    }
  };

  const updateGeometry = (stretch: number, tau: number) => {
    const paths = buildPath(stretch, tau, wakeupMode);
    if (fillPathRef.current) fillPathRef.current.setAttribute("d", paths.closedPath);
    if (rimPathRef.current) rimPathRef.current.setAttribute("d", paths.rimPath);
    if (wingLeftRef.current) wingLeftRef.current.setAttribute("d", paths.wingLeft);
    if (wingRightRef.current) wingRightRef.current.setAttribute("d", paths.wingRight);

    if (cradleRef.current) {
      const orbScale = Math.max(0.1, Math.min(1.0, tau * 1.3));
      const orbY = pos === "top" ? paths.currentH * 0.48 - 50 : (H - paths.currentH) + paths.currentH * 0.48 - 50;
      cradleRef.current.style.transform = `translate3d(0, ${orbY}px, 0) scale(${orbScale})`;
      cradleRef.current.style.opacity = String(Math.min(1.0, tau * 2.5));
    }
  };

  useEffect(() => {
    if (rafRef.current) cancelAnimationFrame(rafRef.current);

    if (isShown) {
      if (!isInitialMount.current && prevShownRef.current === true) {
        updateGeometry(1.0, 1.0);
        return;
      }
      prevShownRef.current = true;
      isInitialMount.current = false;

      const duration = 680;
      const start = performance.now();

      const step = (now: number) => {
        const elapsed = now - start;
        const tau = Math.min(1.0, elapsed / duration);
        const stretch = computeStretch(tau, wakeupMode, k);
        updateGeometry(stretch, tau);

        if (tau < 1.0) {
          rafRef.current = requestAnimationFrame(step);
        } else {
          rafRef.current = null;
          updateGeometry(1.0, 1.0);
        }
      };

      rafRef.current = requestAnimationFrame(step);
    } else {
      if (!isInitialMount.current && prevShownRef.current === false) {
        updateGeometry(0.01, 0.0);
        return;
      }
      prevShownRef.current = false;
      isInitialMount.current = false;

      // Retract thread back flush to screen bezel
      const duration = 300;
      const start = performance.now();

      const step = (now: number) => {
        const elapsed = now - start;
        const tau = Math.min(1.0, elapsed / duration);
        const stretch = 1.0 - tau;
        updateGeometry(Math.max(0.01, stretch), 1.0 - tau);

        if (tau < 1.0) {
          rafRef.current = requestAnimationFrame(step);
        } else {
          rafRef.current = null;
          updateGeometry(0.01, 0.0);
        }
      };

      rafRef.current = requestAnimationFrame(step);
    }

    return () => {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
    };
  }, [isShown, pos, wakeupMode]);

  return (
    <div className="pebble-scoop-wrapper">
      <svg className="pebble-scoop-svg" viewBox={`0 0 ${W} ${H}`}>
        <defs>
          <linearGradient id="pebbleRimGrad" x1="0%" y1="0%" x2="100%" y2="0%">
            <stop offset="0%" stopColor="rgba(255, 255, 255, 0.0)" />
            <stop offset="15%" stopColor="rgba(255, 255, 255, 0.25)" />
            <stop offset="50%" stopColor="rgba(255, 255, 255, 0.32)" />
            <stop offset="85%" stopColor="rgba(255, 255, 255, 0.25)" />
            <stop offset="100%" stopColor="rgba(255, 255, 255, 0.0)" />
          </linearGradient>
          <filter id="pebbleWingGlow" x="-20%" y="-20%" width="140%" height="140%">
            <feGaussianBlur in="SourceGraphic" stdDeviation="2.5" result="blur" />
            <feMerge>
              <feMergeNode in="blur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>

        {/* Pure pitch-black container pouch */}
        <path ref={fillPathRef} fill="#000000" />

        {/* Stretching Thread boundary with Specular Rim */}
        <path ref={rimPathRef} fill="none" stroke="url(#pebbleRimGrad)" strokeWidth={1.3} />

        {/* Wing Specular Contact Overlays */}
        <path ref={wingLeftRef} fill="none" stroke="#ffffff" strokeWidth={2.0} filter="url(#pebbleWingGlow)" opacity={0.0} />
        <path ref={wingRightRef} fill="none" stroke="#ffffff" strokeWidth={2.0} filter="url(#pebbleWingGlow)" opacity={0.0} />
      </svg>

      {/* Voice Orb nestled inside the stretched thread pouch */}
      <div className="orb-cradle-wrapper" ref={cradleRef}>
        <Avatar entered={entered} dispersing={dispersing} shown={isShown} />
      </div>
    </div>
  );
}

/**
 * OrbFrame — the voice orb's home inside the always-on `stage` overlay
 * (Single-Stage Shell step 2). Replaces the retired `main` OS window: the
 * orb's screen position comes from Rust's `stage:orb_rect` event (physical
 * px, same convention as `ghost:ring`/`stage:spatial_annotations` —
 * divide by devicePixelRatio for CSS px) instead of a native window move,
 * and visibility is a plain conditional render instead of `show()`/`hide()`.
 *
 * No-slide entrance (plan §1.3): there is deliberately no CSS transform
 * here for showing/hiding — the orb's own particle assemble()/disperse()
 * (inside VoiceOrb/Avatar) IS the entrance/exit now, for every wake, not
 * just ghost sessions.
 */
/**
 * Ghost sessions keep `visible` true for the whole session, so the capsule's
 * own visibility is decided here by the pure policy in net/orbPolicy.ts: shown
 * while something is happening (or the mic is parked), hidden about 4 s after
 * the last activity. Outside Ghost it returns `true` and has no effect.
 */
function useGhostOrbShown(ghostActive: boolean): { shown: boolean; parked: boolean } {
  const [out, setOut] = useState({ shown: true, parked: false });
  useEffect(() => {
    if (!ghostActive) {
      setOut({ shown: true, parked: false });
      return;
    }
    let st: GhostOrbState = initialGhostOrbState(Date.now());
    const tick = () => {
      const s = useAssistant.getState();
      const parked = isGhostParked();
      const r = stepGhostOrb(
        st,
        {
          state: s.state,
          micLevel: s.micLevel,
          captionActive: s.captionActive,
          ttsActive: s.ttsActive,
          loadingVisible: s.loadingVisible,
          awaitingInput: s.awaitingInput,
          parked,
        },
        Date.now(),
      );
      st = r.next;
      setOut((prev) => (prev.shown === r.shown && prev.parked === parked ? prev : { shown: r.shown, parked }));
    };
    tick();
    const unsub = useAssistant.subscribe(tick);
    const id = setInterval(tick, 250);
    return () => {
      unsub();
      clearInterval(id);
    };
  }, [ghostActive]);
  return out;
}

export function OrbFrame() {
  const assistantVisible = useAssistant((s) => s.visible);
  const state = useAssistant((s) => s.state);
  const loadingVisible = useAssistant((s) => s.loadingVisible);
  const ghostActive = useAssistant((s) => s.ghostActive);
  const orbPosition = useAssistant((s) => s.orbPosition || "top");
  const capsuleStyle = useAssistant((s) => s.orbCapsuleStyle || "classic");
  const wakeupMode = useAssistant((s) => s.orbWakeupMode || 1);

  const [rect, setRect] = useState<OrbRect | null>(null);
  // Calibration can force the orb hidden (Loading target active) or shown
  // (Wakeup/Waves target active) independent of the normal voice `visible`
  // flag — see calibration.rs's apply_main/calibration_set_target. `null`
  // means "no override — follow assistantVisible".
  const [stageOverride, setStageOverride] = useState<boolean | null>(null);
  const visible = stageOverride ?? assistantVisible;
  const ghostOrb = useGhostOrbShown(ghostActive);
  // Calibration can still force the orb on; otherwise Ghost follows its own
  // idle-hide policy and every other mode follows the assistant's flag.
  const isShown = stageOverride === true ? true : ghostActive ? ghostOrb.shown : Boolean(visible);

  // One-time bootstrap: voice runtime + orchestrator/tts-activity listeners
  // (previously wired in main.tsx / App.tsx's mount effect).
  useEffect(() => {
    void import("./orbRuntime").then(({ initOrbRuntime }) => initOrbRuntime());
    void initOrchestratorListener();
    void import("../audio/ttsActivity").then(({ initTtsActivityListener }) =>
      initTtsActivityListener(),
    );
  }, []);

  // Orb rect (position/size), physical px from Rust → CSS px here. Pulls
  // the pending value on mount (race-free: an emit before this effect
  // subscribes would otherwise be lost — same pattern as the codebase's
  // other dynamically-created-window pending-content commands).
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<OrbRect>("stage:orb_rect", (event) => {
        if (event.payload) setRect(event.payload);
      });
      try {
        const pending = await tauriInvoke("get_pending_orb_rect");
        if (pending) setRect(pending as OrbRect);
      } catch {
        /* non-Tauri (tests) — no-op */
      }
    })();
    return () => {
      unlisten?.();
    };
  }, []);

  // Dev-only preview harness (`stage.html?eyeball`, eyeball/EyeballPanel.tsx):
  // a plain browser has no Tauri events, so the harness hands the orb its
  // rect directly. Compiled out of production builds.
  useEffect(() => {
    if (!import.meta.env.DEV) return;
    const onRect = (e: Event) => {
      const r = (e as CustomEvent<OrbRect>).detail;
      if (r) setRect(r);
    };
    window.addEventListener("nexus:eyeball-orb-rect", onRect);
    return () => window.removeEventListener("nexus:eyeball-orb-rect", onRect);
  }, []);

  // Calibration state → visibility override + target/size mirror (moved
  // from App.tsx's "calibration:state" listener).
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
        const st = useAssistant.getState();
        if (p.active && p.target && p.target !== "loading") {
          if (st.calibrationTarget !== p.target) st.bumpCalibrationPulse();
          st.setCalibration(p.target, p.size ?? null);
          setStageOverride(true);
          st.setState("idle");
        } else if (p.active && p.target === "loading") {
          st.setCalibration(p.target, p.size ?? null);
          setStageOverride(false);
        } else if (p.active === false) {
          st.setCalibration(null, null);
          setStageOverride(null);
        }
      });
    })();
    return () => {
      unlisten?.();
    };
  }, []);

  // Explicit show/hide from Rust (wake, Tier-3 command detection,
  // calibration preview) — see window_manager.rs's show_orb_interactive/
  // wake_orb and calibration.rs's apply_main. Clears any stale calibration
  // override so a normal wake always wins.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<boolean>("stage:orb_visible", (event) => {
        if (useAssistant.getState().calibrationTarget) {
          setStageOverride(event.payload);
        } else {
          setStageOverride(null);
          if (event.payload) useAssistant.getState().setVisible(true);
        }
      });
    })();
    return () => {
      unlisten?.();
    };
  }, []);

  // Stage watchdog notices (spoken alerts outside the orchestrator
  // handshake — see stage.rs's inform_via_orb).
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | null = null;
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen<{ text?: string }>("stage:notice", (event) => {
        const text = event.payload?.text;
        if (!text) return;
        const s = useAssistant.getState();
        s.setVisible(true);
        s.setState("speaking");
        s.addAssistantMessage(text);
        import("../audio/ttsPlayer")
          .then(({ speak }) =>
            speak(text, () => {
              useAssistant.getState().setVisible(true);
              setTimeout(() => { void import("../net/ghostHotMic").then((m) => m.endGhostTurn()); }, 550);
            }),
          )
          .catch(() => {
            void import("../net/ghostHotMic")
              .then((m) => m.endGhostTurn())
              .catch(() => useAssistant.getState().reset());
          });
      }).then((fn) => { unlisten = fn; }),
    );
    return () => unlisten?.();
  }, []);

  // Ghost session event → orb ghost mode (waves). See App.tsx's original
  // comment: session-scoped, fires on enter/exit/stand-down only.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | null = null;
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen<{ active?: boolean }>("ghost:session", (event) => {
        const s = useAssistant.getState();
        const v = event.payload?.active === true;
        if (s.ghostActive !== v) {
          s.setGhostActive(v);
          s.setVisible(true);
          if (!v) s.setState("idle");
          console.log(`[ORB] ghost:session active=${v} → visible=true state=${v ? s.state : "idle"}`);
          // Stall watchdog lifecycle: re-arms capture when a ghost turn
          // never completes (hung backend, lost result event). Started
          // on enter, stopped on exit — never runs outside sessions.
          void import("../net/ghostHotMic").then((m) => {
            if (v) m.startGhostWatchdog();
            else m.stopGhostWatchdog();
          }).catch(() => {});
          if (!v) hideOrbAfterSpeech(600);
        }
      }).then((fn) => { unlisten = fn; }),
    );
    // Mount-time sync: check if a ghost session was already armed before mount
    void tauriInvoke("get_pending_ghost_session")
      .then((active) => {
        if (active === true) {
          const s = useAssistant.getState();
          s.setGhostActive(true);
          s.setVisible(true);
          console.log("[ORB] get_pending_ghost_session active=true → visible=true");
          void import("../net/ghostHotMic").then((m) => {
            m.startGhostWatchdog();
          }).catch(() => {});
        }
      })
      .catch(() => {});
    // Periodic resync, both directions. The event above fires once per
    // enter/exit; if it was lost (stage rebuilt, listener not yet attached) the
    // orb either believes in a session Rust already ended (a zombie Ghost orb)
    // or misses one that started. Rust owns the truth; reconcile within 5 s.
    const resync = window.setInterval(() => {
      void tauriInvoke("get_pending_ghost_session")
        .then((active) => {
          if (typeof active !== "boolean") return;
          const s = useAssistant.getState();
          if (s.ghostActive === active) return;
          console.warn(`[ORB] ghost state drift: store=${s.ghostActive} rust=${active} — resyncing`);
          s.setGhostActive(active);
          if (!active) s.setState("idle");
          void import("../net/ghostHotMic")
            .then((m) => (active ? m.startGhostWatchdog() : m.stopGhostWatchdog()))
            .catch(() => {});
        })
        .catch(() => {});
    }, 5000);
    return () => {
      unlisten?.();
      window.clearInterval(resync);
    };
  }, []);

  // Sentinel alerts (tracking-first, no speech/orb changes here).
  useEffect(() => {
    if (!isTauri()) return;
    void import("../store/sentinel").then(({ initSentinelAlertListener }) =>
      initSentinelAlertListener().catch(() => {}),
    );
  }, []);

  // Observability: every visible transition, one line.
  useEffect(() => {
    const s = useAssistant.getState();
    console.log(`[ORB] visible=${s.visible} state=${s.state} ghost=${s.ghostActive}`);
  }, [assistantVisible]);

  // "Idle means hidden" invariant (net/orbIdleWatchdog.ts). Backstop for every
  // turn type that never armed hideOrbAfterSpeech (boot briefing, sentinel
  // alerts, ack-less results) and for turns Rust dropped without any event.
  useEffect(() => {
    let wd: WatchdogState = INITIAL_WATCHDOG_STATE;
    const id = setInterval(() => {
      const s = useAssistant.getState();
      const r = stepWatchdog(
        wd,
        {
          visible: s.visible,
          state: s.state as "idle" | "listening" | "thinking" | "speaking",
          ghostActive: s.ghostActive,
          captionActive: s.captionActive,
          ttsActive: s.ttsActive,
          awaitingInput: s.awaitingInput,
          pendingGithubCommand: !!s.pendingGithubCommand,
          loadingVisible: s.loadingVisible,
          calibrating: !!s.calibrationTarget,
        },
        Date.now(),
      );
      wd = r.next;
      if (r.action === "none") return;
      console.warn(`[ORB] idle watchdog: ${r.action} (state=${s.state} visible=${s.visible})`);
      if (r.action === "reset_and_hide") {
        void tauriInvoke("orchestrator_cancel").catch(() => {});
        s.setLoadingVisible(false);
        // turn-end:keep-raw idle watchdog: stuck thinking/listening past its limit
        s.reset();
      }
      useAssistant.getState().setVisible(false);
    }, 500);
    return () => clearInterval(id);
  }, []);

  // No-input timeout: 8s of `listening` with no voice detected and the
  // orb hides itself (Rust owns the turn end if voice IS underway).
  useEffect(() => {
    if (!visible || state !== "listening" || ghostActive) return;
    const t = setTimeout(() => {
      void import("@tauri-apps/api/core").then(async ({ invoke }) => {
        try {
          const hadSpeech = await invoke<boolean>("stt_capture_had_speech");
          if (hadSpeech) {
            console.log("[NEXUS] no-input timeout: voice underway, Rust owns the turn end");
            return;
          }
        } catch {
          /* IPC failed — fall through to the legacy local abort below */
        }
        import("../audio/vad").then(({ stopVad }) => stopVad()).catch(() => {});
        import("../audio/recorder").then(({ abortCapture }) => {
          void abortCapture().catch(() => {});
        }).catch(() => {});
        useAssistant.getState().setVisible(false);
        // turn-end:keep-raw 8 s no-input timeout after wake
        setTimeout(() => useAssistant.getState().reset(), 300);
      }).catch(() => {});
    }, 8000);
    return () => clearTimeout(t);
  }, [visible, state, ghostActive]);

  // Speaking failsafe: force a reset if 60s pass in `speaking` with no
  // audio actually playing (genuinely long replies re-arm instead).
  useEffect(() => {
    if (state !== "speaking") return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const arm = () => {
      timer = setTimeout(() => {
        timer = null;
        if (cancelled || useAssistant.getState().state !== "speaking") return;
        import("../audio/ttsPlayer")
          .then(({ isRustTtsPlaying }) => {
            if (cancelled || useAssistant.getState().state !== "speaking") return;
            if (isRustTtsPlaying()) {
              arm();
              return;
            }
            console.warn("[NEXUS] speaking failsafe: silent 60s, forcing reset");
            const s = useAssistant.getState();
            s.setLoadingVisible(false);
            s.setVisible(true);
            setTimeout(() => { void import("../net/ghostHotMic").then((m) => m.endGhostTurn()); }, 550);
          })
          .catch(() => {});
      }, 60000);
    };
    arm();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [state]);

  // Orb interactivity: when the state is active (not idle), the orb's
  // rect becomes a stage hitbox so pointer events reach it (replaces the
  // old `set_click_through` call on the `main` window).
  useEffect(() => {
    if (state === "idle" && !useAssistant.getState().calibrationTarget) {
      tauriInvoke("set_orb_interactive", { interactive: false }).catch(() => {});
      return;
    }
    tauriInvoke("set_orb_interactive", { interactive: true }).catch(() => {});
  }, [state]);

  // Loading indicator visibility (separate stage-hosted component —
  // LoadingIndicator.tsx listens for the same events independently; this
  // just drives the Rust-side rect/visibility emit).
  useEffect(() => {
    if (loadingVisible && !ghostActive) {
      tauriInvoke("orchestrator_show_loading").catch(() => {});
    } else {
      tauriInvoke("orchestrator_hide_loading").catch(() => {});
    }
  }, [loadingVisible, ghostActive]);

  // Particle entrance/exit (no-slide, plan §1.3): assemble() on every
  // false→true, disperse() on every true→false. Ghost-mode's own exit
  // stays sphere-to-sphere (handled inside VoiceOrb/Avatar's `entered`
  // prop wiring — this just tracks the transition edge).
  const wasVisibleRef = useRef(false);
  const [entered, setEntered] = useState(false);
  const [dispersing, setDispersing] = useState(false);
  useEffect(() => {
    const was = wasVisibleRef.current;
    wasVisibleRef.current = isShown;
    if (!was && isShown) {
      setDispersing(false);
      setEntered(true);
      // Reset the one-shot trigger next tick so a later show re-fires it.
      const t = setTimeout(() => setEntered(false), 50);
      return () => clearTimeout(t);
    }
    if (was && !isShown) {
      setDispersing(true);
      const t = setTimeout(() => setDispersing(false), 950);
      return () => clearTimeout(t);
    }
  }, [isShown]);

  // Positioning is applied via DIRECT DOM mutation (ref callback + effect),
  // never React's `style` prop: the stage window's CSP injects a per-load
  // nonce into style-src for Tauri's own IPC init script, which per the
  // CSP spec makes 'unsafe-inline' inert for the WHOLE directive — silently
  // dropping every React-rendered inline style and collapsing this div to
  // zero size (live bug, 2026-10-04: orb invisible, captions still showed
  // since text has non-zero intrinsic height even unpositioned). The
  // existing ghost-ring/ghost-pointer elements elsewhere in this same file
  // already use this exact ref + `el.style.x = ...` pattern and are
  // unaffected — direct CSSOM property assignment isn't inline-style
  // text parsing, so it isn't subject to this restriction.
  const hasPositionedRef = useRef(false);
  const applyRect = (el: HTMLDivElement | null, r: OrbRect | null, shown: boolean, pos: string) => {
    if (!el || !r) return;
    const dpr = window.devicePixelRatio || 1;
    const wCss = r.w / dpr;
    const hCss = r.h / dpr;
    const xCss = r.x / dpr;
    const yCss = r.y / dpr;

    const isPebble = capsuleStyle === "pebble";
    const pebbleW = 301;
    const pebbleH = 105;

    if (isPebble) {
      el.style.position = "fixed";
      const screenW = typeof window !== "undefined" ? window.innerWidth : 1920;
      const leftX = (screenW - pebbleW) / 2;
      el.style.left = `${leftX}px`;
      el.style.width = `${pebbleW}px`;
      el.style.height = `${pebbleH}px`;
      el.style.top = pos === "top" ? "0px" : "";
      el.style.bottom = pos === "bottom" ? "0px" : "";
      el.style.transform = "none";
      el.style.opacity = shown ? "1" : "0";
      el.style.pointerEvents = shown ? "auto" : "none";
      el.style.transition = "opacity 0.3s ease";
      return;
    }

    el.style.position = "fixed";
    el.style.left = `${xCss}px`;
    el.style.top = "0";
    el.style.width = `${wCss}px`;
    el.style.height = `${hCss}px`;

    const targetY =
      pos === "top"
        ? (shown ? yCss : -hCss - 24)
        : (shown ? yCss : (typeof window !== "undefined" ? window.innerHeight : 1080) + 24);

    if (!hasPositionedRef.current) {
      // First mount: instant snap to target without flying from (0, 0)
      el.style.transition = "none";
      el.style.transform = `translate3d(0, ${targetY}px, 0)`;
      el.style.opacity = shown ? "1" : "0";
      el.style.pointerEvents = shown ? "auto" : "none";
      hasPositionedRef.current = true;
      requestAnimationFrame(() => {
        if (el) {
          el.style.transition = "transform 0.72s cubic-bezier(0.22, 1, 0.36, 1), opacity 0.5s ease";
        }
      });
      return;
    }

    el.style.transition = "transform 0.72s cubic-bezier(0.22, 1, 0.36, 1), opacity 0.5s ease";
    el.style.transform = `translate3d(0, ${targetY}px, 0)`;
    el.style.opacity = shown ? "1" : "0";
    el.style.pointerEvents = shown ? "auto" : "none";
  };
  const frameRef = useRef<HTMLDivElement | null>(null);
  // The aura rims this element: a circle the size of the visible particle
  // sphere, centred in the frame (see `.orb-aura-anchor`).
  const auraAnchorRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    console.log(`[ORB-FRAME] state: isShown=${isShown} pos=${orbPosition} (assistantVisible=${assistantVisible}, ghost=${ghostActive}, stageOverride=${stageOverride})`);
    applyRect(frameRef.current, rect, isShown, orbPosition);
  }, [rect, isShown, orbPosition, assistantVisible, ghostActive, stageOverride, capsuleStyle]);

  if (!rect) return null;

  return (
    <div
      id="orb-frame"
      data-interactive={isShown ? "true" : undefined}
      data-parked={ghostActive && ghostOrb.parked ? "true" : undefined}
      className={`orb-slider orb-slider--${orbPosition}`}
      ref={(el) => {
        frameRef.current = el;
        applyRect(el, rect, isShown, orbPosition);
      }}
    >
      {capsuleStyle === "pebble" ? (
        <PebbleScoopFrame
          isShown={isShown}
          pos={orbPosition}
          entered={entered}
          dispersing={dispersing}
          wakeupMode={wakeupMode}
        />
      ) : capsuleStyle === "aura" ? (
        // Linux redesign: no box — the bare orb, ringed by the audio-reactive
        // aura. The canvas is portalled to <body>: it is `position: fixed` in
        // viewport coordinates, and this frame slides with a transform.
        <div className="orb-aura">
          <div className="orb-aura-anchor" ref={auraAnchorRef} aria-hidden="true" />
          <div className="orb-sphere-wrapper">
            <Avatar entered={entered} dispersing={dispersing} shown={isShown} />
          </div>
          {typeof document !== "undefined" &&
            createPortal(<AuraCanvas anchorRef={auraAnchorRef} shown={isShown} />, document.body)}
        </div>
      ) : (
        <div className="orb-capsule">
          <div className="orb-sphere-wrapper">
            <Avatar entered={entered} dispersing={dispersing} shown={isShown} />
          </div>
        </div>
      )}
    </div>
  );
}
