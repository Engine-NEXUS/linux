import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

// ─── HOT MIC + PRE-INIT VAD (Approach A+B) ─────────────────────────────────
// At startup, we pre-acquire the mic stream and pre-initialize the Silero
// MicVAD instance. This eliminates the two biggest sources of wake-to-listen
// delay:
//   1. getUserMedia() — 50-200ms per wake → eliminated (mic stays warm)
//   2. MicVAD.new()   — 60-250ms per wake → eliminated (VAD stays paused, ready)
//
// On wake, we just resume the VAD + start recording in parallel (Approach C).
// Wake-to-listen drops from ~200-500ms to ~10-50ms.
//
// Privacy: Audio is ALWAYS processed locally. The mic stream stays open but
// audio is only captured when recording is active. VAD runs only during
// listening state. No audio leaves the device.

import { preloadSileroVad, preloadMicVad, stopVad } from "./audio/vad";
import { abortCapture, processTranscript } from "./audio/recorder";
import { stopTts } from "./audio/ttsPlayer";
import { useAssistant } from "./store/assistant";
import { setBargedIn, clearBargedIn, clearDialogContext } from "./net/wsBridge";

/** Set to true when the wake is triggered automatically by a follow-up
 * question (not by the user pressing Ctrl+Space). When true, startListening
 * does NOT set the bargedIn flag — the follow-up response should be heard. */
let autoReopenFromFollowup = false;

/** Called by wsBridge when a follow-up question finishes speaking and
 * the mic should auto-reopen without requiring the user to press Ctrl+Space. */
export function triggerFollowupListen(): void {
  autoReopenFromFollowup = true;
  const w = window as any;
  if (w.__NEXUS_WAKE__) {
    w.__NEXUS_WAKE__();
  }
}

let micStream: MediaStream | null = null;

// ─── No-speech watchdog ───────────────────────────────────────────────────
// NOTE: The no-speech watchdog is now handled on the Rust side.
// The Rust STT capture has a built-in timeout (STT_NO_SPEECH_CHUNK_LIMIT = 100
// chunks = ~8s). If no speech is detected, it stops capturing and emits an
// empty transcript, which triggers the "didn't catch that" retry logic.

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

    // Pre-initialize MicVAD with the warm stream.
    // This creates the AudioWorklet + loads the Silero model.
    // The VAD starts in paused state — it won't process audio until startVad().
    await preloadMicVad(micStream);
    console.log("[NEXUS] hot mic: MicVAD pre-initialized and ready");
  } catch (err) {
    console.warn("[NEXUS] hot mic: startup mic acquisition failed, will acquire on wake:", err);
    micStream = null;
  }
}

// Start the hot mic + VAD preload at app startup (non-blocking)
// NOTE: warmMic() is DISABLED at startup because it opens getUserMedia() via
// WebView2, which conflicts with the Rust cpal wake-word stream on some audio
// drivers (Intel Smart Sound Technology). The cpal stream gets silence when
// WebView2 is also capturing from the same mic.
// The frontend will acquire the mic on first wake via startListening().
preloadSileroVad()
  .then(() => {
    console.log("[NEXUS] Silero VAD model pre-loaded (mic NOT acquired — cpal has exclusive access)");
  })
  .catch(() => {
    console.warn("[NEXUS] Silero VAD pre-load failed at startup — will use RMS fallback");
  });

// warmMic is kept for future use but not called at startup to avoid mic conflict.
void warmMic;

/**
 * Wake-word / hotkey → mic capture → VAD → STT → intent → execute / backend.
 *
 * With hot mic + pre-init VAD, the wake-to-listen path is:
 *   1. Wake fires → show overlay, set state to "listening"
 *   2. Mic stream already warm → skip getUserMedia (saves 50-200ms)
 *   3. Start recording + start VAD in PARALLEL (saves 60-250ms)
 *   4. VAD detects silence → finishCapture() → STT → intent → execute
 *
 * Total wake-to-listen: ~10-50ms (down from ~200-500ms)
 */

async function startListening() {
  const s = useAssistant.getState();
  console.log("[NEXUS] wake →", s.state);

  // Don't re-start if already listening.
  if (s.state === "listening") {
    console.log("[NEXUS] already listening, ignoring wake");
    return;
  }

  // If NEXUS is speaking or thinking, cancel the current turn before
  // starting a new one. This prevents the TTS 'interrupted' error and
  // ensures clean state transitions.
  //
  // EXCEPTION: If a long-running query is in flight (PR analysis etc),
  // do NOT close the session — the HTTP request to the Worker is already
  // in progress and can't be cancelled. The dedup/queue logic in
  // recorder.ts handles the new command instead.
  const wasSpeaking = s.state === "speaking";
  const isAutoReopen = autoReopenFromFollowup;
  autoReopenFromFollowup = false; // consume the flag
  const { isLongRunningInFlight } = await import("./net/wsBridge");
  const longRunningActive = isLongRunningInFlight();
  if (wasSpeaking || s.state === "thinking") {
    // If this is an auto-reopen from a follow-up question, do NOT set bargedIn —
    // the follow-up response should be heard, not discarded.
    if (!isAutoReopen) {
      setBargedIn();
      // User barge-in: clear any pending dialog context — they're starting fresh
      clearDialogContext();
    } else {
      console.log("[NEXUS] auto-reopen from follow-up — NOT setting bargedIn");
      clearBargedIn();
    }
    if (longRunningActive) {
      console.log("[NEXUS] barge-in: long-running query in flight — NOT closing session (dedup/queue will handle)");
      stopTts();
      stopVad();
      // Don't call abortCapture — it would closeSession and break the in-flight request
    } else {
      console.log("[NEXUS] barge-in/turn-transition: cancelling current turn");
      stopTts();
      stopVad();
      await abortCapture().catch(() => {});
    }
    if (wasSpeaking && !isAutoReopen) {
      // Dual-Phase Post-TTS Mute Gate: 300ms delay to allow DAC audio buffers and room acoustics to clear
      await new Promise((r) => setTimeout(r, 300));
    }
  } else {
    // Fresh wake (no barge-in) — clear the flag for the new turn
    clearBargedIn();
  }

  // Hide the loading indicator — the orb is showing again, so the
  // loading animation at the top-right corner is no longer needed.
  // This covers the barge-in case where the user presses Ctrl+Space
  // while a long-running query is in flight and the loading indicator
  // is still visible.
  useAssistant.getState().setLoadingVisible(false);
  // New turn: never inherit the previous turn's "waiting" glow.
  useAssistant.getState().setAwaitingInput(false);
  s.setVisible(true);
  s.setState("listening");
  // Clear barge-in flag now that we're starting a fresh listening session
  clearBargedIn();

  // ─── Rust-side STT capture ────────────────────────────────────────
  // The Rust cpal stream (which detected the wake word) captures audio
  // directly — no getUserMedia, no baton pass, no frontend audio processing.
  // This fixes the Intel SST driver issue where getUserMedia returns silence
  // but the cpal stream is still working.
  //
  // The Rust side:
  //   1. Buffers 16kHz audio from the cpal callback
  //   2. RMS-based VAD detects speech + silence
  //   3. On silence, transcribes (Groq or local faster-whisper)
  //   4. Emits "stt:transcript" event with the transcript text
  //
  // The frontend just shows the orb and waits for the event.
  // No getUserMedia, no VAD, no ScriptProcessorNode, no baton pass.
  console.log("[NEXUS] Rust-side STT capture active — waiting for stt:transcript event");
}

/** Called from Rust on wake (hotkey, spoken "NEXUS", or tray click). */
(window as any).__NEXUS_WAKE__ = () => {
  console.log("[NEXUS] __NEXUS_WAKE__ invoked");
  void wakeWithGreeting();
};

/**
 * Called from Rust when the first-run setup wizard completes.
 * Speaks the first-run greeting: "NEXUS online, sir. Ready when you are."
 * Shows the orb briefly, then hides it. Does NOT transition to listening.
 *
 * After the greeting, warms up the mic stream so the first wake is fast.
 * This is safe because the setup wizard already verified mic permission.
 */
(window as any).__NEXUS_FIRST_RUN_GREETING__ = async () => {
  console.log("[NEXUS] __NEXUS_FIRST_RUN_GREETING__ invoked");
  const { useAssistant } = await import("./store/assistant");
  const { speak } = await import("./audio/ttsPlayer");

  const s = useAssistant.getState();
  s.setVisible(true);
  s.setState("speaking");
  s.addAssistantMessage("NEXUS online, sir. Ready when you are.");

  void speak("NEXUS online, sir. Ready when you are.").then(() => {
    console.log("[NEXUS] first-run greeting done — hiding orb");
    s.setState("idle");
    s.setVisible(false);
    setTimeout(() => s.reset(), 550);
  });

  // Warm up the mic now that permission has been granted during setup.
  // This is safe because the setup wizard's Permissions step verified
  // getUserMedia works. The mic stream will be reused on first wake.
  // NOTE: We wait 2s after the greeting starts so the TTS audio doesn't
  // interfere with the mic warm-up on Intel SST drivers.
  setTimeout(() => {
    warmMic().catch((e) => console.warn("[NEXUS] post-setup warmMic failed:", e));
  }, 2000);
};

// Tauri IPC wake events are NOT listened to here anymore.
// The Rust side calls window.__NEXUS_WAKE__() directly via eval(),
// which is more reliable than the event system for repeated rapid events.
// Listening to both caused wakeWithGreeting() to fire 2-3x, resulting in
// "on it sir" being spoken twice.

/**
 * Wake handler — goes straight to listening.
 *
 * The daily greeting has been removed. NEXUS simply shows the orb and
 * starts listening immediately when the hotkey is pressed.
 */
async function wakeWithGreeting() {
  void startListening();
}

/**
 * Tier 3: Direct command detection listener.
 *
 * When a command classifier fires in the OWW pipeline (e.g. "open youtube"),
 * Rust emits a `command-detected` Tauri event with the structured intent.
 * The frontend skips STT entirely and executes the intent directly —
 * no Whisper, no transcript, no 27-second delay.
 *
 * This is the fast path: ~200ms from speech to action.
 * The STT path remains as fallback for commands not covered by classifiers.
 */
async function setupCommandDetectionListener() {
  try {
    const { listen } = await import("@tauri-apps/api/event");
    await listen<{ action: string; target: string; needs_param?: boolean }>("command-detected", async (event) => {
      const intent = event.payload;
      console.log(`[NEXUS] Tier 3 command detected: ${intent.action} (needs_param=${intent.needs_param ?? false})`);

      const { useAssistant } = await import("./store/assistant");
      const { speak } = await import("./audio/ttsPlayer");

      // Show the overlay and set state to speaking
      const s = useAssistant.getState();
      s.setVisible(true);
      s.setState("speaking");

      // ─── Type 2: Parameterized command ─────────────────────────────
      // The acoustic classifier detected the command PATTERN (e.g. "play ... in spotify").
      // Now we need to capture the PARAMETER (e.g. song name) via STT.
      // Flow: speak "On it sir" → record 3s → STT → execute with parameter
      if (intent.needs_param) {
        s.addUserMessage(`${intent.action.replace(/_/g, " ")}...`);
        s.addAssistantMessage("On it sir");
        void speak("On it sir");

        // Wait for TTS to finish before recording (so we don't capture TTS audio)
        await new Promise<void>((resolve) => {
          if (typeof speechSynthesis === "undefined" || !speechSynthesis.speaking) {
            resolve();
            return;
          }
          const check = () => {
            if (!speechSynthesis.speaking) {
              resolve();
              return;
            }
            setTimeout(check, 100);
          };
          setTimeout(check, 100);
        });

        // Record 3 seconds of audio for the parameter
        s.setState("listening");
        try {
          const { captureParameter } = await import("./audio/paramCapture");
          const pcm = await captureParameter(3000);
          if (pcm && pcm.length > 0) {
            s.setState("thinking");
            const { transcribeAudio } = await import("./audio/stt");
            const param = await transcribeAudio(pcm);
            if (param && param.trim().length > 0) {
              console.log(`[NEXUS] Tier 3 parameter: "${param}"`);
              s.addUserMessage(param);
              // Execute with the parameter as the query
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
          useAssistant.getState().setVisible(false);
          setTimeout(() => useAssistant.getState().reset(), 550);
        }, 800);
        return;
      }

      // ─── Type 1: Fixed command (no parameter) ──────────────────────
      // Execute directly — no STT needed.
      s.addUserMessage(`${intent.action.replace(/_/g, " ")} ${intent.target}`);
      s.addAssistantMessage("Ok sir.");
      void speak("Ok sir.");

      // Execute the command directly — no STT needed
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

      // Hide after a short delay
      setTimeout(() => {
        useAssistant.getState().setVisible(false);
        setTimeout(() => useAssistant.getState().reset(), 550);
      }, 800);
    });
    console.log("[NEXUS] Tier 3 command detection listener registered");
  } catch (err) {
    // Non-fatal — STT fallback handles all commands if this listener fails
    console.warn("[NEXUS] Failed to register Tier 3 command listener:", err);
  }
}

// Register the listener at startup (non-blocking, non-fatal)
void setupCommandDetectionListener();

// ─── Rust-side STT transcript listener ─────────────────────────────────
// The Rust cpal stream captures audio and transcribes it (Groq or local
// faster-whisper). When the transcript is ready, Rust emits "stt:transcript".
// The frontend processes it using the same logic as the old finishCapture().
async function setupSttTranscriptListener() {
  try {
    const { listen } = await import("@tauri-apps/api/event");
    await listen<string>("stt:transcript", async (event) => {
      const transcript = event.payload;
      console.log(`[NEXUS] stt:transcript event received: "${transcript}"`);
      // Process the transcript using the same logic as finishCapture()
      await processTranscript(transcript);
    });
    console.log("[NEXUS] stt:transcript listener registered");
  } catch (err) {
    console.warn("[NEXUS] Failed to register stt:transcript listener:", err);
  }
}

// Register at startup
void setupSttTranscriptListener();

/** Called from Rust to cancel the current session. */
(window as any).__NEXUS_CANCEL__ = async () => {
  console.log("[NEXUS] cancel");
  // Stop TTS first so we don't hear the rest of the speech.
  stopTts();
  // Stop VAD so it doesn't trigger finishCapture during cleanup.
  stopVad();
  // Then abort recording and close the session.
  await abortCapture();
  // Hot mic: don't release the stream, just disable tracks
  if (micStream) {
    micStream.getTracks().forEach((t) => (t.enabled = false));
  }
  // Hide the loading indicator if it was showing.
  useAssistant.getState().setLoadingVisible(false);
  useAssistant.getState().reset();
  useAssistant.getState().setVisible(false);
};

/** Called by finishCapture/abortCapture cleanup to release the mic stream.
 *  With hot mic, we DON'T release the stream — we keep it warm for the next wake.
 *  The stream tracks are disabled by VAD's pauseStream callback instead. */
(window as any).__NEXUS_RELEASE_MIC__ = () => {
  // Hot mic: keep the stream alive, just disable the tracks
  if (micStream) {
    micStream.getTracks().forEach((t) => (t.enabled = false));
  }
  // THE BATON PASS: Tell Rust to resume wake-word detection now that
  // the frontend is done with the mic. Without this, the wake-word
  // engine stays deaf after the first voice command.
  import("@tauri-apps/api/core").then(({ invoke }) => {
    invoke("resume_wakeword").catch((e: unknown) => console.warn("resume_wakeword failed:", e));
    console.log("[NEXUS] baton pass: Rust wakeword resumed");
  }).catch(() => {});
};

/** Called by paramCapture to get the existing mic stream (or null if not active). */
(window as any).__NEXUS_GET_MIC_STREAM__ = async (): Promise<MediaStream> => {
  if (micStream) return micStream;
  // If no existing stream, get a new one
  return await navigator.mediaDevices.getUserMedia({
    audio: {
      channelCount: 1,
      echoCancellation: true,
      noiseSuppression: true,
    },
  });
};

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
