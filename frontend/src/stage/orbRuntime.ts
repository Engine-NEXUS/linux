// ─── Orb voice runtime (relocated from the retired `main` window) ─────────
//
// Single-Stage Shell step 2: the voice orb no longer has its own OS window
// ("main") — it's a positioned div inside the always-on `stage` fullscreen
// overlay (see OrbFrame.tsx). This module is the entire voice bootstrap
// that used to live in frontend/src/main.tsx: hot-mic warmup, the
// `window.__NEXUS_*` globals Rust used to invoke via `win.eval(...)` on the
// `main` window (Rust now emits real Tauri events instead — "orb:wake" /
// "orb:first_run_greeting" — since there's no window-by-label to eval
// against anymore), and every `stt:*`/`ghost:*`/`audio:*` listener.
//
// Call `initOrbRuntime()` once from stage/main.tsx on mount.

import { preloadSileroVad, preloadMicVad, stopVad } from "../audio/vad";
import { abortCapture, processTranscript } from "../audio/recorder";
import { stopTts, isRustTtsPlaying } from "../audio/ttsPlayer";
import { transcribeAudio } from "../audio/stt";
import { resolveCapsuleStyle, useAssistant } from "../store/assistant";
import { isLinuxRuntime } from "../platform";
import { setBargedIn, clearBargedIn, clearDialogContext } from "../net/wsBridge";
import { isDuplicateListen } from "../net/listenDedupe";

/** Set to true when the wake is triggered automatically by a follow-up
 * question (not by the user pressing Ctrl+Space). When true, startListening
 * does NOT set the bargedIn flag — the follow-up response should be heard. */
let autoReopenFromFollowup = false;

/** Last accepted follow-up/hot-mic re-arm (see isDuplicateListen). */
let lastFollowupListenAt = 0;
let lastFollowupListenOrigin: string | null = null;

/** Called by wsBridge when a follow-up question finishes speaking and
 *  the mic should auto-reopen without requiring the user to press Ctrl+Space.
 *  Also the ghost hot-mic loop's listen entry. Showing the orb is NOT
 *  enough: the Rust cpal capture must be started explicitly, or the orb
 *  shows "listening" while Rust captures nothing (live bug, doc 44 —
 *  the confirm window already did this; the shared path didn't). */
export function triggerFollowupListen(hotMic = false): void {
  // Duplicate-listen guard (R3): independent re-arm paths can land together;
  // the second would restart a live capture and run the wake handler twice.
  const origin = hotMic ? "hot_mic" : "direct";
  const nowMs = Date.now();
  if (isDuplicateListen(nowMs, lastFollowupListenAt, lastFollowupListenOrigin, origin)) {
    console.log(
      `[NEXUS] followup listen skipped: duplicate ${origin} re-arm (+${nowMs - lastFollowupListenAt}ms)`,
    );
    return;
  }
  lastFollowupListenAt = nowMs;
  lastFollowupListenOrigin = origin;
  autoReopenFromFollowup = true;
  const w = window as any;
  if (w.__NEXUS_WAKE__) {
    // P2 double-wake audit: followup relistens also funnel through the
    // wake handler — tag the source so doubles are attributable.
    (w.__NEXUS_WAKE_SOURCE as string | undefined) = hotMic ? "followup-hotmic" : "followup";
    w.__NEXUS_WAKE__();
  }
  import("@tauri-apps/api/core")
    // origin: "hot_mic" = the ghost hot-mic loop re-opened the mic by itself, so the Phase 8
    // directed-speech gate applies to what it hears; every other caller is an explicit turn.
    .then(({ invoke }) => invoke("start_stt_capture", { origin: hotMic ? "hot_mic" : "direct" }))
    .catch((e) => {
      console.warn("[NEXUS] followup listen: Rust capture not started:", e);
    });
}

let micStream: MediaStream | null = null;

/**
 * Acquire the mic stream at startup and keep it warm.
 * This eliminates the 50-200ms getUserMedia() latency on every wake.
 *
 * If this fails (e.g. mic permission not yet granted), we fall back to
 * acquiring the mic on the first wake — the old behavior.
 */
async function warmMic(): Promise<void> {
  try {
    micStream = await navigator.mediaDevices.getUserMedia({
      audio: {
        channelCount: 1,
        echoCancellation: true,
        noiseSuppression: true,
      },
    });
    console.log("[NEXUS] hot mic: stream acquired and warming");
    await preloadMicVad(micStream);
    console.log("[NEXUS] hot mic: MicVAD pre-initialized and ready");
  } catch (err) {
    console.warn("[NEXUS] hot mic: startup mic acquisition failed, will acquire on wake:", err);
    micStream = null;
  }
}

// warmMic() is DISABLED at startup — it opens getUserMedia() via WebView2,
// which conflicts with the Rust cpal wake-word stream on some audio
// drivers (Intel Smart Sound Technology). The frontend acquires the mic on
// first wake via startListening() instead. Kept for future use.
void warmMic;

/**
 * Wake-word / hotkey → mic capture → VAD → STT → intent → execute / backend.
 */
async function startListening() {
  const s = useAssistant.getState();
  console.log("[NEXUS] wake →", s.state);

  // The mic is opening again (wake word, hotkey or a relisten): it is no longer
  // parked and the ignored-speech streak starts over.
  void import("../net/ghostHotMic").then((m) => {
    m.clearGhostParked();
    m.resetIgnoredTurns();
  }).catch(() => {});

  // Second Ctrl+Space press while listening = cancel the call.
  if (s.state === "listening") {
    console.log("[NEXUS] second press while listening → aborting capture");
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const res = await invoke<{ had_speech: boolean; elapsed_ms: number }>(
        "stop_stt_capture"
      );
      if (!res.had_speech) {
        await abortCapture().catch((e) => console.warn("[NEXUS] abortCapture failed:", e));
        // turn-end:keep-raw explicit user cancel (second press while listening)
        useAssistant.getState().reset();
        useAssistant.getState().setVisible(false);
      }
    } catch (err) {
      console.warn("[NEXUS] stop_stt_capture failed:", err);
    }
    return;
  }

  // Alexa-style barge-in: ALWAYS cut audio on wake, not only when the
  // frontend believes it is speaking.
  stopTts();
  stopVad();

  const wasSpeaking = s.state === "speaking";
  const isAutoReopen = autoReopenFromFollowup;
  autoReopenFromFollowup = false; // consume the flag
  const { isLongRunningInFlight } = await import("../net/wsBridge");
  const longRunningActive = isLongRunningInFlight();
  if (wasSpeaking || s.state === "thinking") {
    if (!isAutoReopen) {
      setBargedIn();
      clearDialogContext();
    } else {
      console.log("[NEXUS] auto-reopen from follow-up — NOT setting bargedIn");
      clearBargedIn();
    }
    if (longRunningActive) {
      console.log("[NEXUS] barge-in: long-running query in flight — NOT closing session (dedup/queue will handle)");
      stopTts();
      stopVad();
    } else {
      console.log("[NEXUS] barge-in/turn-transition: cancelling current turn");
      stopTts();
      stopVad();
      await abortCapture().catch((e) => console.warn("[NEXUS] barge-in abortCapture failed:", e));
    }
    if (wasSpeaking && !isAutoReopen) {
      await new Promise((r) => setTimeout(r, 300));
    }
  } else {
    clearBargedIn();
  }

  useAssistant.getState().setLoadingVisible(false);
  useAssistant.getState().setAwaitingInput(false);
  s.setVisible(true);
  s.setState("listening");
  clearBargedIn();

  console.log("[NEXUS] Rust-side STT capture active — waiting for stt:transcript event");
}

/** Called from Rust on wake (hotkey, spoken "NEXUS", or tray click) — kept
 *  as a global for any other direct caller, and invoked by the "orb:wake"
 *  Tauri event listener below (Rust no longer has a window to `eval()`
 *  against, so it emits a real event instead). */
let __wakeInvokeCount = 0;
let __lastWakeAt = 0;
(window as any).__NEXUS_WAKE__ = () => {
  // P2 double-wake audit: counter + gap + source on every invocation so
  // back-to-back wakes in the log are attributable to their trigger.
  __wakeInvokeCount += 1;
  const now = Date.now();
  const gap = __lastWakeAt ? now - __lastWakeAt : -1;
  __lastWakeAt = now;
  const w = window as any;
  const source = w.__NEXUS_WAKE_SOURCE ?? "unknown";
  w.__NEXUS_WAKE_SOURCE = undefined;
  console.log(`[NEXUS] __NEXUS_WAKE__ invoked #${__wakeInvokeCount} (source=${source}, +${gap}ms)`);
  void wakeWithGreeting();
};

/**
 * Called from Rust when the first-run setup wizard completes (via the
 * "orb:first_run_greeting" event now, not eval). Speaks the first-run
 * greeting, shows the orb briefly, then hides it.
 */
(window as any).__NEXUS_FIRST_RUN_GREETING__ = async () => {
  console.log("[NEXUS] __NEXUS_FIRST_RUN_GREETING__ invoked");
  const { speak } = await import("../audio/ttsPlayer");

  const s = useAssistant.getState();
  s.setVisible(true);
  s.setState("speaking");
  s.addAssistantMessage("NEXUS online, sir. Ready when you are.");

  void speak("NEXUS online, sir. Ready when you are.").then(() => {
    console.log("[NEXUS] first-run greeting done — hiding orb");
    s.setState("idle");
    s.setVisible(false);
    // turn-end:keep-raw first-run greeting finished
    setTimeout(() => s.reset(), 550);
  });

  setTimeout(() => {
    warmMic().catch((e) => console.warn("[NEXUS] post-setup warmMic failed:", e));
  }, 2000);
};

async function wakeWithGreeting() {
  void startListening();
}

/** Rust emits "orb:wake" (replaces the old win.eval("__NEXUS_WAKE__()")
 *  call against the now-retired `main` window). */
async function setupOrbWakeListener() {
  try {
    const { listen } = await import("@tauri-apps/api/event");
    await listen<{ source?: string }>("orb:wake", (event) => {
      (window as any).__NEXUS_WAKE_SOURCE = event.payload?.source ?? "orb:wake";
      (window as any).__NEXUS_WAKE__?.();
    });
    await listen("orb:first_run_greeting", () => {
      (window as any).__NEXUS_FIRST_RUN_GREETING__?.();
    });
    await listen("orb:cancel", async () => {
      console.log("[NEXUS] orb:cancel received — canceling active agent & resetting orb");
      stopTts();
      stopVad();
      await abortCapture().catch((e) => console.warn("[NEXUS] cancel abortCapture failed:", e));
      const s = useAssistant.getState();
      s.setState("idle");
      s.setVisible(false);
      s.setLoadingVisible(false);
      s.setAwaitingInput(false);
      // turn-end:keep-raw explicit user cancel (Ctrl+Space): the hot mic must not relisten
      s.reset();
    });
    console.log("[NEXUS] orb:wake / orb:first_run_greeting / orb:cancel listeners registered");
  } catch (err) {
    console.warn("[NEXUS] Failed to register orb wake listeners:", err);
  }
}

/**
 * Tier 3: Direct command detection listener. Rust shows the orb itself
 * now (window_manager::show_orb_interactive) before emitting this — the
 * handler below only needs to react to the intent payload.
 */
async function setupCommandDetectionListener() {
  try {
    const { listen } = await import("@tauri-apps/api/event");
    await listen<{ action: string; target: string; needs_param?: boolean }>("command-detected", async (event) => {
      const intent = event.payload;
      console.log(`[NEXUS] Tier 3 command detected: ${intent.action} (needs_param=${intent.needs_param ?? false})`);

      const { speak } = await import("../audio/ttsPlayer");

      const s = useAssistant.getState();
      s.setVisible(true);
      s.setState("speaking");

      if (intent.needs_param) {
        s.addUserMessage(`${intent.action.replace(/_/g, " ")}...`);
        s.addAssistantMessage("On it sir");
        void speak("On it sir");

        await new Promise<void>((resolve) => {
          const idle = () =>
            !isRustTtsPlaying() &&
            (typeof speechSynthesis === "undefined" || !speechSynthesis.speaking);
          if (idle()) {
            resolve();
            return;
          }
          const check = () => {
            if (idle()) {
              resolve();
              return;
            }
            setTimeout(check, 100);
          };
          setTimeout(check, 100);
        });

        s.setState("listening");
        try {
          const { captureParameter } = await import("../audio/paramCapture");
          const pcm = await captureParameter(3000);
          if (pcm && pcm.length > 0) {
            s.setState("thinking");
            const param = await transcribeAudio(pcm);
            if (param && param.trim().length > 0) {
              console.log(`[NEXUS] Tier 3 parameter: "${param}"`);
              s.addUserMessage(param);
              const { invoke } = await import("@tauri-apps/api/core");
              const result = await invoke<{ success: boolean; message: string }>(
                "execute_command",
                { intent: { action: intent.action, query: param } }
              );
              console.log(`[NEXUS] Tier 3 execute result:`, result);
              if (result.message) {
                s.addAssistantMessage(result.message);
                void speak(result.message.replace(/,/g, ""));
              }
            } else {
              console.warn("[NEXUS] Tier 3 parameter STT returned empty");
              s.addAssistantMessage("Didn't catch that sir");
              void speak("Didn't catch that sir");
            }
          }
        } catch (err) {
          console.error("[NEXUS] Tier 3 parameter capture failed:", err);
          s.addAssistantMessage("Didn't catch that sir");
          void speak("Didn't catch that sir");
        }

        setTimeout(() => {
          if (!useAssistant.getState().ghostActive) {
            useAssistant.getState().setVisible(false);
          }
          setTimeout(() => { void import("../net/ghostHotMic").then((m) => m.endGhostTurn()); }, 550);
        }, 800);
        return;
      }

      s.addUserMessage(`${intent.action.replace(/_/g, " ")} ${intent.target}`);
      s.addAssistantMessage("Ok sir.");
      void speak("Ok sir.");

      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const result = await invoke<{ success: boolean; message: string }>(
          "execute_command",
          { intent }
        );
        console.log(`[NEXUS] Tier 3 execute result:`, result);
      } catch (err) {
        console.error("[NEXUS] Tier 3 command execution failed:", err);
      }

      setTimeout(() => {
        if (!useAssistant.getState().ghostActive) {
          useAssistant.getState().setVisible(false);
        }
        setTimeout(() => { void import("../net/ghostHotMic").then((m) => m.endGhostTurn()); }, 550);
      }, 800);
    });
    console.log("[NEXUS] Tier 3 command detection listener registered");
  } catch (err) {
    console.warn("[NEXUS] Failed to register Tier 3 command listener:", err);
  }
}

export type SttTurnOwnership = "verified" | "uncertain" | "rejected" | "unenrolled";

export interface SttTranscriptTurn {
  session?: number;
  text?: string;
  ownership?: SttTurnOwnership;
  owner_score?: number;
  decoder_bias?: string;
  language?: string;
  intent_label?: string;
  pre_parsed?: {
    intent: any;
    confidence: number;
    source: string;
  };
}

export type SttTranscriptPayload = string | SttTranscriptTurn;

async function setupSttTranscriptListener() {
  try {
    const { listen } = await import("@tauri-apps/api/event");
    const handleTranscript = async (payload: SttTranscriptPayload) => {
      const transcript = typeof payload === "string" ? payload : payload.text ?? "";
      const turn =
        typeof payload === "string"
          ? undefined
          : {
              ownership: payload.ownership ?? "unenrolled",
              ownerScore: payload.owner_score ?? 1,
              decoderBias: payload.decoder_bias ?? "owner",
              language: payload.language ?? "en",
              intentLabel: payload.intent_label,
              preParsed: payload.pre_parsed,
              initiation: useAssistant.getState().ghostActive ? ("ghost" as const) : ("explicit" as const),
            };
      console.log(`[NEXUS] stt:transcript event received: "${transcript}" intent=${typeof payload === "object" ? payload.intent_label : "none"}`);
      // Ghost turns run in the order they were heard (a fast-path turn must not overtake an older
      // one that is still being parsed); everything else keeps the direct path.
      if (turn?.initiation === "ghost") {
        const { runInTurnOrder } = await import("../net/turnChain");
        await runInTurnOrder(() => processTranscript(transcript, turn));
      } else {
        await processTranscript(transcript, turn);
      }
    };
    await listen<SttTranscriptPayload>("stt:transcript", async (event) => {
      await handleTranscript(event.payload);
    });
    console.log("[NEXUS] stt:transcript listener registered");
    // A turn that finished while this stage was down was stashed by Rust (events are
    // fire-and-forget). Replay it once on mount so the orb never parks on a lost turn.
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const pending = await invoke<SttTranscriptPayload | null>("stage_take_pending_transcript");
      if (pending) {
        console.warn("[ORB] replaying a transcript that arrived while the stage was down");
        await handleTranscript(pending);
      }
    } catch {
      /* outside Tauri / command unavailable */
    }
  } catch (err) {
    console.warn("[NEXUS] Failed to register stt:transcript listener:", err);
  }
}

interface TurnStats {
  session: number;
  rms_max: number;
  rms_mean: number;
  voiced_chunks: number;
  total_chunks: number;
  endpoint: string;
  stt_path: string;
  filter: string;
  owner?: string;
  owner_score?: number;
  decoder_bias?: string;
  stt_language?: string;
}

async function setupTurnStatsListener() {
  try {
    const { listen } = await import("@tauri-apps/api/event");
    await listen<TurnStats>("stt:turn_stats", async (event) => {
      const p = event.payload;
      console.info(
        `[NEXUS] stt:turn_stats session=${p.session} ` +
          `rms_max=${Number(p.rms_max).toFixed(4)} rms_mean=${Number(p.rms_mean).toFixed(5)} ` +
          `voiced=${p.voiced_chunks}/${p.total_chunks} endpoint=${p.endpoint} ` +
          `path=${p.stt_path} filter=${p.filter} owner=${p.owner ?? "unknown"}`
      );
      const { invoke } = await import("@tauri-apps/api/core");
      void invoke("debug_trace", {
        msg:
          `turn session=${p.session} rms_max=${Number(p.rms_max).toFixed(4)} ` +
          `voiced=${p.voiced_chunks}/${p.total_chunks} endpoint=${p.endpoint} ` +
          `path=${p.stt_path} filter=${p.filter} owner=${p.owner ?? "unknown"}`,
      }).catch(() => {});
    });
    console.log("[NEXUS] stt:turn_stats listener registered");
  } catch (err) {
    console.warn("[NEXUS] Failed to register stt:turn_stats listener:", err);
  }
}

async function setupAudioLevelListener() {
  try {
    const { listen } = await import("@tauri-apps/api/event");
    await listen<{ level?: unknown }>("audio:level", (event) => {
      const raw = event.payload?.level;
      const level = typeof raw === "number" && Number.isFinite(raw) ? raw : 0;
      useAssistant.getState().setMicLevel(level);
    });
    console.log("[NEXUS] audio:level listener registered");
  } catch (err) {
    console.warn("[NEXUS] Failed to register audio:level listener:", err);
  }
}

let sharedMicHolder: string | null = null;
(window as any).__NEXUS_RELEASE_MIC__ = () => {
  if (micStream) {
    micStream.getTracks().forEach((t) => (t.enabled = false));
  }
  if (sharedMicHolder !== null) {
    void import("../audio/micHolders").then(({ micRelease }) => {
      if (sharedMicHolder !== null) micRelease(sharedMicHolder);
      sharedMicHolder = null;
    }).catch(() => {});
  }
  import("@tauri-apps/api/core").then(({ invoke }) => {
    invoke("resume_wakeword").catch((e: unknown) => console.warn("resume_wakeword failed:", e));
    console.log("[NEXUS] baton pass: Rust wakeword resumed");
  }).catch(() => {});
};

(window as any).__NEXUS_GET_MIC_STREAM__ = async (): Promise<MediaStream> => {
  if (micStream) return micStream;
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: {
      channelCount: 1,
      echoCancellation: true,
      noiseSuppression: true,
    },
  });
  micStream = stream;
  if (sharedMicHolder === null) {
    const { micAcquire } = await import("../audio/micHolders");
    sharedMicHolder = micAcquire("frontend-stream");
  } else {
    console.warn("[NEXUS] mic: second opener reused live stream (no double-open)");
  }
  return stream;
};

let initialized = false;

/** Call once from stage/main.tsx on mount. Idempotent (StrictMode-safe). */
export function initOrbRuntime(): void {
  if (initialized) return;
  initialized = true;

  preloadSileroVad()
    .then(() => {
      console.log("[NEXUS] Silero VAD model pre-loaded (mic NOT acquired — cpal has exclusive access)");
    })
    .catch(() => {
      console.warn("[NEXUS] Silero VAD pre-load failed at startup — will use RMS fallback");
    });

  void setupOrbWakeListener();
  void setupCommandDetectionListener();
  void setupSttTranscriptListener();
  void setupTurnStatsListener();
  void setupAudioLevelListener();

  // Open the backend session at startup (fire-and-forget).
  void (async () => {
    try {
      const { ensureSessionOpen } = await import("../net/wsBridge");
      void ensureSessionOpen();
    } catch (e) {
      console.warn("[NEXUS] session pre-open failed:", e);
    }
  })();

  // Sync persisted orbColor & orbPosition and listen for live updates from Command Hub
  void (async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const s = await invoke<{
        orbColor?: string;
        orbPosition?: "top" | "bottom";
        orbCapsuleStyle?: string;
        orbWakeupMode?: number;
        orbInteriorGenesis?: number;
        orbThinkMode?: number;
        orbSpeakMode?: number;
        orbListenMode?: number;
      }>("get_settings");
      if (s?.orbColor) {
        useAssistant.getState().setOrbColor(s.orbColor);
      }
      if (s?.orbPosition) {
        useAssistant.getState().setOrbPosition(s.orbPosition);
      }
      if (s?.orbCapsuleStyle) {
        useAssistant.getState().setOrbCapsuleStyle(resolveCapsuleStyle(s.orbCapsuleStyle, isLinuxRuntime()));
      }
      if (s?.orbWakeupMode) {
        useAssistant.getState().setOrbWakeupMode(s.orbWakeupMode);
      }
      if (s?.orbInteriorGenesis) {
        useAssistant.getState().setOrbInteriorGenesis(s.orbInteriorGenesis);
      }
      if (s?.orbThinkMode) {
        useAssistant.getState().setOrbThinkMode(s.orbThinkMode);
      }
      if (s?.orbSpeakMode) {
        useAssistant.getState().setOrbSpeakMode(s.orbSpeakMode);
      }
      if (s?.orbListenMode) {
        useAssistant.getState().setOrbListenMode(s.orbListenMode);
      }
    } catch (_) {}

    try {
      const { listen } = await import("@tauri-apps/api/event");
      await listen<{ color: string }>("orb:color", (e) => {
        if (e.payload?.color) {
          useAssistant.getState().setOrbColor(e.payload.color);
        }
      });
      await listen<{ style?: string; mode?: number; genesis?: number } | string>("orb:capsule_style", (e) => {
        const payload = e.payload;
        if (typeof payload === "string") {
          if (payload === "classic" || payload === "pebble" || payload === "aura") {
            useAssistant.getState().setOrbCapsuleStyle(payload);
          }
        } else if (payload?.style) {
          if (payload.style === "classic" || payload.style === "pebble" || payload.style === "aura") {
            useAssistant.getState().setOrbCapsuleStyle(payload.style);
          }
          if (payload.mode) useAssistant.getState().setOrbWakeupMode(payload.mode);
          if (payload.genesis) useAssistant.getState().setOrbInteriorGenesis(payload.genesis);
        }
      });
      await listen<string>("stage:orb_position", (e) => {
        const p = e.payload as "top" | "bottom";
        if (p === "top" || p === "bottom") {
          useAssistant.getState().setOrbPosition(p);
        }
      });
      await listen<{ think?: number; speak?: number; listen?: number }>("orb:morphology_changed", (e) => {
        if (e.payload?.think) useAssistant.getState().setOrbThinkMode(e.payload.think);
        if (e.payload?.speak) useAssistant.getState().setOrbSpeakMode(e.payload.speak);
        if (e.payload?.listen) useAssistant.getState().setOrbListenMode(e.payload.listen);
      });
    } catch (_) {}
  })();
}
