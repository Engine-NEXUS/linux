import { finishCapture, finishCaptureFromVad, getRecordingContext } from "./recorder";
import { transcribeAudio } from "./stt";
import { useAssistant } from "../store/assistant";

/**
 * Voice Activity Detection using Silero ONNX VAD via @ricky0123/vad-web.
 *
 * Silero VAD is a neural network that detects speech patterns — not just
 * volume. This is the same approach used by Alexa, Siri, and Google
 * Assistant. It can distinguish speech from background noise even when
 * the speech volume is very low (RMS 0.003-0.007), which RMS energy
 * detection cannot do.
 *
 * Architecture:
 *   - MicVAD manages the audio stream and AudioWorklet
 *   - Silero ONNX model runs on each audio frame (~16ms)
 *   - onSpeechStart → UI state to "listening"
 *   - onSpeechEnd(audio) → audio is Float32Array at 16kHz → finishCaptureFromVad()
 *   - Fallback to RMS VAD if Silero fails to load
 *
 * ONNX WASM loading strategy:
 *   onnxruntime-web internally does a dynamic `import('./ort-wasm-simd-threaded.mjs')`
 *   which is incompatible with Vite's dev server pre-bundling (the .mjs file
 *   resolves to .vite/deps/ where it doesn't exist → 404).
 *   This is a known issue: microsoft/onnxruntime#20978, #22615.
 *
 *   Solution: Load ONNX WASM binaries from CDN via onnxWASMBasePath.
 *   The CDN import bypasses Vite entirely — the browser fetches .mjs and
 *   .wasm files directly from jsdelivr with proper CORS headers.
 *   Model + worklet files are served locally via viteStaticCopy.
 *
 * Audio stays local — Silero runs in the browser via ONNX Runtime Web (WASM).
 * No audio leaves the device.
 */

// Pin to the exact installed version to avoid CDN/local version mismatch.
const ORT_VERSION = "1.27.0";
const ORT_CDN_BASE = `https://cdn.jsdelivr.net/npm/onnxruntime-web@${ORT_VERSION}/dist/`;

// ---- Silero VAD configuration ----
// These thresholds are tuned for voice commands (short utterances).
// Silero returns a speech probability 0.0-1.0 per frame.
const POSITIVE_SPEECH_THRESHOLD = 0.5;  // Above this = speech detected
const NEGATIVE_SPEECH_THRESHOLD = 0.35; // Below this = silence detected
// Grace period before declaring speech end. This is pure end-to-end latency:
// nothing happens until it expires. 3000ms absorbs natural pauses between
// words in longer commands (e.g. "deep analysis for the PR 24 in nexus-agent")
// without cutting off mid-sentence. Also helps with the Intel SST mic driver
// which can drop audio briefly mid-utterance — the extra 1s gives the stream
// time to recover before we declare speech end. The previous 500ms was too
// aggressive; 2000ms was better but still truncated "open architecture mapper"
// to "open-" on Intel SST.
const REDEMPTION_MS = 3000;
const PRE_SPEECH_PAD_MS = 500;          // Audio to prepend before speech start
const MIN_SPEECH_MS = 500;              // Discard segments shorter than this

// ---- Speculative transcription ----
// Whisper costs ~400-600ms and that cost is fixed (audio length is almost
// irrelevant: 1.5s and 12s both measure ~550-600ms, because Whisper pads to a
// 30s window). Waiting for redemption to expire and THEN transcribing makes
// those two costs add up.
//
// Instead we fire a transcription as soon as speech first drops to silence,
// so the STT call runs *during* the redemption window. By the time
// onSpeechEnd fires the transcript is usually already resolved.
//
// If the user resumes speaking during redemption the speculation is
// invalidated and we fall back to transcribing the final segment normally —
// so this can never produce a worse transcript, only a faster one.
const SPEC_FIRE_SILENCE_MS = 120;   // silence observed before firing
const SPEC_MIN_SPEECH_MS = 300;     // don't speculate on short blips
const SPEC_MAX_BUFFER_MS = 15000;   // cap the rolling frame buffer

// ---- State ----
let micVad: any = null;           // MicVAD instance (Silero) — kept alive between commands
let micVadStream: MediaStream | null = null;  // Stream associated with the pre-init VAD
let active = false;

// ---- No-speech watchdog ----
// Callback fired when VAD detects real speech start.
// Used by startListening() to cancel the no-speech timeout.
let onSpeechStartedCb: (() => void) | null = null;

export function setSpeechStartCallback(cb: (() => void) | null): void {
  onSpeechStartedCb = cb;
}

// ---- Speculative transcription state ----
let specFrames: Float32Array[] = [];
let specSamples = 0;
let specSpeechMs = 0;
let specSilenceMs = 0;
let specPending: Promise<string> | null = null;
let specInvalid = false;

function resetSpeculation(): void {
  specFrames = [];
  specSamples = 0;
  specSpeechMs = 0;
  specSilenceMs = 0;
  specPending = null;
  specInvalid = false;
}

/**
 * Called for every VAD frame (512 samples @ 16kHz ≈ 32ms).
 * Buffers audio and decides when to fire the speculative transcription.
 */
function onVadFrame(probs: { isSpeech: number }, frame: Float32Array): void {
  if (!active || !frame || frame.length === 0) return;

  // Compute RMS volume and update store for avatar reactivity (AK port).
  let sum = 0;
  for (let i = 0; i < frame.length; i++) {
    sum += frame[i] * frame[i];
  }
  const rms = Math.sqrt(sum / frame.length);
  useAssistant.getState().setAudioVolume(rms);

  const frameMs = (frame.length / 16000) * 1000;

  // The library may reuse the frame buffer — copy before retaining it.
  specFrames.push(frame.slice());
  specSamples += frame.length;

  const maxSamples = (SPEC_MAX_BUFFER_MS / 1000) * 16000;
  while (specSamples > maxSamples && specFrames.length > 1) {
    specSamples -= specFrames[0].length;
    specFrames.shift();
  }

  if (probs.isSpeech >= POSITIVE_SPEECH_THRESHOLD) {
    specSpeechMs += frameMs;
    specSilenceMs = 0;
    // Speech resumed after we already fired — that transcript is incomplete.
    if (specPending) specInvalid = true;
  } else if (probs.isSpeech < NEGATIVE_SPEECH_THRESHOLD) {
    specSilenceMs += frameMs;
  }

  if (
    !specPending &&
    !specInvalid &&
    specSpeechMs >= SPEC_MIN_SPEECH_MS &&
    specSilenceMs >= SPEC_FIRE_SILENCE_MS
  ) {
    fireSpeculation();
  }
}

/** Kick off an STT call on everything buffered so far (fire and forget). */
function fireSpeculation(): void {
  const total = specSamples;
  if (total === 0) return;

  const merged = new Float32Array(total);
  let off = 0;
  for (const f of specFrames) {
    merged.set(f, off);
    off += f.length;
  }

  const pcm = new Int16Array(total);
  for (let i = 0; i < total; i++) {
    const s = Math.max(-1, Math.min(1, merged[i]));
    pcm[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
  }

  console.log(
    `[NEXUS] VAD: firing speculative STT (${total} samples) — overlapping ${REDEMPTION_MS}ms redemption`,
  );
  specPending = transcribeAudio(pcm).catch((err) => {
    console.warn("[NEXUS] speculative STT failed, will re-transcribe:", err);
    return "";
  });
}

/** Hand the in-flight speculative transcript to the consumer, if still valid. */
function takeSpeculation(): Promise<string> | null {
  if (specPending && specInvalid) {
    console.log("[NEXUS] VAD: speculation invalidated (speech resumed) — full re-transcribe");
    return null;
  }
  return specPending;
}

// ---- RMS fallback state (used only if Silero fails to load) ----
const VAD_FALLBACK_RMS = 0.015;
const SILENCE_MS = 2000;
const MAX_UTTERANCE_MS = 12000;
const MIN_LISTEN_MS = 2000;
let rmsCtx: AudioContext | null = null;
let capTimer: number | null = null;
let startedAt = 0;
let silenceSince = 0;

/**
 * Pre-load the Silero VAD ONNX model + WASM runtime at app startup so
 * they're warm in the browser cache when the first wake word fires.
 * This eliminates the ~1-2s cold-start on the first wake.
 */
let sileroPreloaded = false;
let sileroPreloadPromise: Promise<void> | null = null;

export async function preloadSileroVad(): Promise<void> {
  if (sileroPreloadPromise) return sileroPreloadPromise;
  if (sileroPreloaded) return;
  sileroPreloadPromise = _preloadSilero();
  return sileroPreloadPromise;
}

async function _preloadSilero(): Promise<void> {
  try {
    console.log("[NEXUS] VAD: pre-loading Silero ONNX runtime + model...");

    // Dynamically import the VAD module (triggers Vite pre-bundling of JS).
    await import("@ricky0123/vad-web");

    // Pre-fetch the ONNX WASM .mjs and .wasm from CDN to warm the browser cache.
    // These are the files that onnxruntime-web dynamically imports at runtime.
    const cdnFetches = [
      fetch(ORT_CDN_BASE + "ort-wasm-simd-threaded.mjs"),
      fetch(ORT_CDN_BASE + "ort-wasm-simd-threaded.wasm"),
    ];

    // Also fetch the Silero model from the local server.
    const modelFetch = fetch("./silero_vad_v5.onnx");

    const [mjsResp, wasmResp, modelResp] = await Promise.all([...cdnFetches, modelFetch]);

    if (!mjsResp.ok) console.warn("[NEXUS] VAD: CDN mjs pre-fetch failed:", mjsResp.status);
    if (!wasmResp.ok) console.warn("[NEXUS] VAD: CDN wasm pre-fetch failed:", wasmResp.status);
    if (!modelResp.ok) throw new Error(`model fetch failed: ${modelResp.status}`);

    // Read bodies to ensure full download into browser cache.
    await Promise.all([mjsResp.arrayBuffer(), wasmResp.arrayBuffer(), modelResp.arrayBuffer()]);

    sileroPreloaded = true;
    console.log("[NEXUS] VAD: Silero pre-loaded (module + CDN WASM + model cached)");
  } catch (err) {
    console.warn("[NEXUS] VAD: Silero pre-load failed, will use RMS fallback:", err);
    sileroPreloaded = false;
    throw err;
  }
}

/**
 * Pre-initialize the MicVAD instance at app startup with a warm mic stream.
 * This eliminates the 60-250ms MicVAD.new() latency on every wake.
 *
 * The VAD is created in paused state (startOnLoad: false) and won't process
 * audio until startVad() is called. The instance is kept alive between commands
 * — stopVad() pauses it instead of destroying it.
 *
 * Must be called AFTER preloadSileroVad() and AFTER the mic stream is acquired.
 */
export async function preloadMicVad(stream: MediaStream): Promise<void> {
  if (micVad) {
    console.log("[NEXUS] VAD: MicVAD already pre-initialized");
    return;
  }

  try {
    const { MicVAD } = await import("@ricky0123/vad-web");
    console.log("[NEXUS] VAD: pre-initializing MicVAD with warm stream...");

    micVad = await MicVAD.new({
      baseAssetPath: "./",
      onnxWASMBasePath: ORT_CDN_BASE,
      model: "v5",
      startOnLoad: false,
      positiveSpeechThreshold: POSITIVE_SPEECH_THRESHOLD,
      negativeSpeechThreshold: NEGATIVE_SPEECH_THRESHOLD,
      redemptionMs: REDEMPTION_MS,
      preSpeechPadMs: PRE_SPEECH_PAD_MS,
      minSpeechMs: MIN_SPEECH_MS,
      submitUserSpeechOnPause: true,
      ortConfig: (ort: any) => {
        ort.env.wasm.wasmPaths = ORT_CDN_BASE;
        ort.env.wasm.numThreads = 1;
      },
      getStream: async () => stream,
      pauseStream: async (s: MediaStream) => {
        s.getTracks().forEach((t) => (t.enabled = false));
      },
      resumeStream: async (s: MediaStream) => {
        s.getTracks().forEach((t) => (t.enabled = true));
        return s;
      },
      onSpeechStart: () => {
        console.log("[NEXUS] VAD: Silero speech start detected");
        if (onSpeechStartedCb) onSpeechStartedCb();
      },
      onSpeechRealStart: () => {
        console.log("[NEXUS] VAD: Silero speech real start (min frames met)");
      },
      onVADMisfire: () => {
        console.log("[NEXUS] VAD: Silero misfire (segment too short)");
      },
      onSpeechEnd: (audio: Float32Array) => {
        console.log(`[NEXUS] VAD: Silero speech end (${audio.length} samples @ 16kHz)`);
        const spec = takeSpeculation();
        active = false;
        void finishCaptureFromVad(audio, spec);
      },
      onFrameProcessed: (probs: { isSpeech: number; notSpeech: number }, frame: Float32Array) => {
        onVadFrame(probs, frame);
        if (Math.random() < 0.01) {
          console.log(`[NEXUS] VAD: Silero probs speech=${probs.isSpeech.toFixed(3)} silence=${probs.notSpeech.toFixed(3)}`);
        }
      },
    });

    micVadStream = stream;
    console.log("[NEXUS] VAD: MicVAD pre-initialized (paused, ready for instant start)");
  } catch (err) {
    console.warn("[NEXUS] VAD: MicVAD pre-init failed, will create on wake:", err);
    micVad = null;
    micVadStream = null;
  }
}

/**
 * Start VAD on an existing MediaStream.
 * Tries Silero VAD first, falls back to RMS if Silero is unavailable.
 *
 * If MicVAD was pre-initialized at startup, this just calls micVad.start()
 * (resume from pause) — near-instant (~1-10ms).
 * If not pre-initialized, creates a new MicVAD instance (~60-250ms).
 */
export async function startVad(stream: MediaStream): Promise<void> {
  if (active) return;
  active = true;
  startedAt = performance.now();
  resetSpeculation();

  // ─── Fast path: reuse pre-initialized MicVAD ──────────────────────
  if (micVad && micVadStream === stream) {
    try {
      await micVad.start();
      console.log("[NEXUS] VAD: Silero VAD resumed (pre-initialized, ~1ms)");
      return;
    } catch (err) {
      console.warn("[NEXUS] VAD: pre-init MicVAD start failed, recreating:", err);
      micVad = null;
      micVadStream = null;
    }
  }

  // ─── Slow path: create new MicVAD (first wake or pre-init failed) ──
  // If we have a stale micVad from a different stream, destroy it first
  if (micVad && micVadStream !== stream) {
    try { micVad.pause(); } catch {}
    micVad = null;
    micVadStream = null;
  }

  try {
    await startSileroVad(stream);
    return;
  } catch (err) {
    console.warn("[NEXUS] VAD: Silero VAD failed, falling back to RMS:", err);
    active = false;
  }

  // Fallback to RMS energy detection.
  active = true;
  startedAt = performance.now();
  startRmsVad(stream);
}

/**
 * Stop VAD (either Silero or RMS).
 * With hot mic + pre-init VAD, this PAUSES the MicVAD instead of destroying it.
 * The MicVAD instance is kept alive for instant resume on the next wake.
 */
export function stopVad(): void {
  active = false;
  useAssistant.getState().setAudioVolume(0);
  if (micVad) {
    try { micVad.pause(); } catch {}
    // DON'T set micVad = null — keep it alive for instant resume
  }
  teardownRms();
  // Drop any buffered frames / in-flight speculation so the next command
  // starts clean. onSpeechEnd has already captured the promise it needs.
  resetSpeculation();
}

/**
 * Resume VAD using the existing stream (for multi-turn hot-mic loop).
 * Used by the "didn't catch that" retry flow — after NEXUS says it didn't
 * catch the command, it resumes listening without requiring a new wake.
 * (AK port)
 */
export async function resumeVad(): Promise<void> {
  if (micVad && micVadStream) {
    active = true;
    await micVad.start();
    console.log("[NEXUS] VAD: Silero VAD resumed (multi-turn loop)");
  } else if (micVadStream) {
    // Fallback: re-start from scratch if micVad was lost
    await startVad(micVadStream);
  }
}

// ─── Silero VAD ────────────────────────────────────────────────────────────

async function startSileroVad(stream: MediaStream): Promise<void> {
  // If MicVAD already exists from pre-init, just start it
  if (micVad) {
    micVadStream = stream;
    await micVad.start();
    console.log("[NEXUS] VAD: Silero VAD started (reused pre-init instance)");
    return;
  }

  const { MicVAD } = await import("@ricky0123/vad-web");

  console.log("[NEXUS] VAD: creating new Silero ONNX VAD (WASM from CDN)");

  micVad = await MicVAD.new({
    // Local files for the model and worklet (served by viteStaticCopy).
    baseAssetPath: "./",
    // CDN for ONNX WASM binaries — this is the fix for the Vite dynamic
    // import incompatibility (microsoft/onnxruntime#20978).
    // The CDN import bypasses Vite's module system entirely.
    onnxWASMBasePath: ORT_CDN_BASE,
    model: "v5",
    startOnLoad: false,
    positiveSpeechThreshold: POSITIVE_SPEECH_THRESHOLD,
    negativeSpeechThreshold: NEGATIVE_SPEECH_THRESHOLD,
    redemptionMs: REDEMPTION_MS,
    preSpeechPadMs: PRE_SPEECH_PAD_MS,
    minSpeechMs: MIN_SPEECH_MS,
    submitUserSpeechOnPause: true,
    // Also explicitly configure ort to use CDN and single-threaded mode.
    // Single-threaded avoids SharedArrayBuffer requirements (which need
    // COOP/COEP headers that may break Tauri WebView IPC).
    ortConfig: (ort: any) => {
      ort.env.wasm.wasmPaths = ORT_CDN_BASE;
      ort.env.wasm.numThreads = 1;
    },
    // Use our existing stream — MicVAD will manage the AudioWorklet
    // but we provide the stream so we don't open a second mic.
    getStream: async () => stream,
    pauseStream: async (s: MediaStream) => {
      s.getTracks().forEach((t) => (t.enabled = false));
    },
    resumeStream: async (s: MediaStream) => {
      s.getTracks().forEach((t) => (t.enabled = true));
      return s;
    },
    // Callbacks
    onSpeechStart: () => {
      console.log("[NEXUS] VAD: Silero speech start detected");
      if (onSpeechStartedCb) onSpeechStartedCb();
    },
    onSpeechRealStart: () => {
      console.log("[NEXUS] VAD: Silero speech real start (min frames met)");
    },
    onVADMisfire: () => {
      console.log("[NEXUS] VAD: Silero misfire (segment too short)");
    },
    onSpeechEnd: (audio: Float32Array) => {
      // audio is Float32Array at 16kHz, samples between -1 and 1.
      // This is EXACTLY what STT needs — convert to Int16 and process.
      console.log(`[NEXUS] VAD: Silero speech end (${audio.length} samples @ 16kHz)`);
      const spec = takeSpeculation();
      active = false;
      void finishCaptureFromVad(audio, spec);
    },
    onFrameProcessed: (probs: { isSpeech: number; notSpeech: number }, frame: Float32Array) => {
      onVadFrame(probs, frame);
      // Optional: debug logging every 500ms
      if (Math.random() < 0.01) { // ~1% of frames to avoid spam
        console.log(`[NEXUS] VAD: Silero probs speech=${probs.isSpeech.toFixed(3)} silence=${probs.notSpeech.toFixed(3)}`);
      }
    },
  });

  micVadStream = stream;
  await micVad.start();
  console.log("[NEXUS] VAD: Silero VAD started successfully");
}

// ─── RMS fallback (only used if Silero fails to load) ──────────────────────

function teardownRms(): void {
  if (capTimer !== null) {
    clearTimeout(capTimer);
    capTimer = null;
  }
  if (rmsCtx) {
    rmsCtx.close().catch(() => {});
    rmsCtx = null;
  }
}

function startRmsVad(stream: MediaStream) {
  const sharedCtx = getRecordingContext();
  const ctx = sharedCtx || new AudioContext({ sampleRate: 16000 });
  if (!sharedCtx) rmsCtx = ctx;
  const src = ctx.createMediaStreamSource(stream);
  const analyser = ctx.createAnalyser();
  analyser.fftSize = 512;
  src.connect(analyser);
  const data = new Uint8Array(analyser.frequencyBinCount);

  let speechOccurred = false;
  let lastDebugLog = 0;

  capTimer = window.setTimeout(() => {
    if (!active) return;
    console.warn("VAD safety cap triggered — forcing end of utterance");
    teardownRms();
    void finishCapture();
  }, MAX_UTTERANCE_MS);

  const tick = () => {
    if (!active) { return; }
    analyser.getByteTimeDomainData(data);
    let sum = 0;
    for (let i = 0; i < data.length; i++) {
      const v = (data[i] - 128) / 128;
      sum += v * v;
    }
    const rms = Math.sqrt(sum / data.length);
    const now = performance.now();

    if (now - lastDebugLog > 500) {
      console.log(`[NEXUS] VAD rms=${rms.toFixed(4)} threshold=${VAD_FALLBACK_RMS} speech=${speechOccurred}`);
      lastDebugLog = now;
    }

    if (rms > VAD_FALLBACK_RMS) {
      silenceSince = 0;
      if (!speechOccurred) {
        console.log(`[NEXUS] VAD: speech detected (rms=${rms.toFixed(4)})`);
        if (onSpeechStartedCb) onSpeechStartedCb();
      }
      speechOccurred = true;
    } else if (silenceSince === 0) {
      silenceSince = now;
    }

    const elapsed = now - startedAt;
    if (elapsed < MIN_LISTEN_MS) {
      requestAnimationFrame(tick);
      return;
    }
    if ((speechOccurred && silenceSince && now - silenceSince > SILENCE_MS) || elapsed > MAX_UTTERANCE_MS) {
      teardownRms();
      void finishCapture();
      return;
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}

// re-export so App can react to state changes without circular import
export const state = useAssistant.getState;
