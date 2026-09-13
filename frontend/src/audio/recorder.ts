import { useAssistant } from "../store/assistant";
import {
  openSession,
  sendTranscript,
  setLongRunningInFlight,
  setLocalAckGiven,
  isLongRunningInFlight,
  isDuplicateLongRunning,
} from "../net/wsBridge";
import { transcribeAudio } from "./stt";
import { speak, speakCached, isRustTtsPlaying } from "./ttsPlayer";
import { parseIntent, type Intent } from "../intent/parser";
import { processViaOrchestrator, cancelOrchestrator } from "../net/orchestrator";

/**
 * Parse a transcript using the Rust-side enhanced intent parser.
 *
 * The Rust parser (intent_parser.rs) has:
 *   - Full app registry access (hundreds of installed apps, not a fixed list)
 *   - Phonetic + Levenshtein matching against real installed app names
 *   - "analyse PR 23 servx" / "analyse servx repo" / "analyse owner/repo" support
 *   - NLU server fallback (BERT-Mini, lazy-started)
 *
 * Falls back to the TypeScript parseIntent() if:
 *   - Running outside Tauri (e.g. in a browser dev environment)
 *   - The Rust parse_transcript command fails
 *
 * Returns the parsed intent plus metadata about the parse source.
 */
async function parseTranscriptEnhanced(
  transcript: string,
): Promise<{ intent: Intent; confidence: number; source: string }> {
  // Try Rust parser first
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const result = await invoke<{
      intent: Intent;
      confidence: number;
      source: string;
    }>("parse_transcript", { transcript });
    console.log(
      `[NEXUS] rust parse: action=${result.intent.action}, confidence=${result.confidence}, source=${result.source}`,
    );
    return result;
  } catch (err) {
    // Rust parser unavailable — fall back to TypeScript parser
    console.warn("[NEXUS] rust parse_transcript unavailable, using TS fallback:", err);
    const intent = parseIntent(transcript);
    return { intent, confidence: 1.0, source: "ts-fallback" };
  }
}

/**
 * Check if an intent is an analyse-type command that should go to the
 * remote backend (not be executed locally).
 *
 * The Rust parser can identify "analyse repo" and "analyse PR" commands
 * with structured data. These are still sent to the remote backend for
 * processing, but the structured data helps the backend and the sidebar
 * display the correct heading.
 */
function isAnalyseIntent(intent: Intent): boolean {
  return (
    intent.action === "analyse_repo" ||
    intent.action === "analyse_pr" ||
    intent.action === "analyse_latest_pr" ||
    intent.action === "check_branch"
  );
}

/**
 * Long-running query queue.
 *
 * If the user says a DIFFERENT long-running command while one is in flight,
 * it's queued here. When the current result arrives (wsBridge clears the
 * in-flight flag and fires the callback), the next queued command is sent.
 */
const pendingLongRunningQueue: string[] = [];

/** Process the next queued long-running command (if any). */
function processNextQueuedCommand(): void {
  if (pendingLongRunningQueue.length === 0) return;
  const next = pendingLongRunningQueue.shift()!;
  console.log(`[NEXUS] queue: processing next queued command: "${next}"`);
  // Send it — the session should still be open from the previous command.
  setLongRunningInFlight(next, processNextQueuedCommand);
  // The orb is already hidden from the previous command's "On it sir".
  // Re-show the loading indicator for this queued command.
  void sendTranscript(next).then(() => {
    console.log(`[NEXUS] queue: sent "${next}" to worker`);
  }).catch((e) => {
    console.warn(`[NEXUS] queue: failed to send "${next}":`, e);
  });
}

/**
 * Handle a long-running transcript when one is already in flight.
 *
 * - SAME command → say "on it sir", do NOT send again (dedup)
 * - DIFFERENT command → say "on it sir", add to queue
 *
 * The orb stays visible with the thinking animation in both cases.
 */
async function handleDuplicateOrQueuedLongRunning(transcript: string): Promise<void> {
  if (isDuplicateLongRunning(transcript)) {
    // Same command — don't send again
    console.log(`[NEXUS] dedup: same command already in flight, not sending again`);
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage("On it sir.");
    setLocalAckGiven(); // prevent server ack from double-speaking
    void speak("On it sir");
    useAssistant.getState().setState("thinking");
  } else {
    // Different long-running command — queue it
    console.log(`[NEXUS] queue: different command while in flight, queuing: "${transcript}"`);
    pendingLongRunningQueue.push(transcript);
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage("On it sir.");
    setLocalAckGiven(); // prevent server ack from double-speaking
    void speak("On it sir");
    useAssistant.getState().setState("thinking");
  }
}

/**
 * Detect if a query is a long-running analysis (PR review, repo analysis, etc).
 * These queries take 10-20 seconds on the Worker (GLM model inference).
 * For these, we give an immediate "On it sir" ack and hide the orb —
 * the result arrives later and triggers the sidebar + "Here is the analysis".
 */
function isLongRunningQuery(transcript: string): boolean {
  const t = transcript.toLowerCase();
  // PR analysis: "analyse PR 5 in servx", "review PR 3", "analyse the pull request"
  // Also matches "analysis" (noun form) — e.g. "deep analysis for the PR 24"
  // Also matches "check"/"show" for branch commands — e.g. "check the latest branch of servx"
  const hasAnalyse = /\b(analy[sz]e|analy[sz]ing|analy[sz]is|review|deep\s*dive|critique|evaluate|assess|inspect|examine|map|understand|explore|create|build|show|generate|check|what\s+is)\b/.test(t);
  const hasPR = /\b(pr|pull\s*request)\b/.test(t);
  const hasRepo = /\b(repo|repository|codebase|project|architecture|code)\b/.test(t);
  // Also catch "PR <number>" patterns even without "analyse" (STT may mishear)
  const hasPRNumber = /\bpr\s*#?\s*\d+\b/.test(t);
  // Branch analysis: "analyse branch X", "check the latest branch", "show branch"
  const hasBranch = /\bbranch(es)?\b/.test(t);
  // Architecture mapper: "analyze this repo", "map the codebase", "create architecture in servx"
  // Also catch "open architecture mapper" (no analyse word, but clearly architect intent)
  const isArchitectQuery = (hasAnalyse && hasRepo)
    || (/\barchitecture\b/.test(t) && /\b(in|of|for|from)\b/.test(t))
    || /\bopen\s+architecture\b/.test(t)
    || /\barchitecture\s+mapper\b/.test(t);
  // "check the latest branch of servx by eesha" → hasAnalyse (check) + hasBranch
  // "analyse the pr in zync" → hasAnalyse + hasPR
  // "analyse the pr by prem in servx" → hasAnalyse + hasPR
  return (hasAnalyse && (hasPR || hasRepo || hasBranch)) || hasPRNumber || isArchitectQuery;
}

/**
 * Post-process STT transcript to fix common mishearings.
 * tiny.en (39M params) struggles with brand names and technical terms.
 * This corrects known mishearings for NEXUS commands.
 *
 * Examples:
 *   "unless pf5 in cervix" → "analyse PR 5 in servx"
 *   "analyze PR 5 in service" → "analyse PR 5 in servx"
 *   "unless pr5 in cervix" → "analyse PR 5 in servx"
 */
function correctSttTranscript(transcript: string): string {
  let t = transcript;
  const logFixes: string[] = [];

  // Strip leading "and " prefix that tiny.en often inserts
  if (/^and\s+/i.test(t)) {
    t = t.replace(/^and\s+/i, "");
    logFixes.push("and→(stripped)");
  }

  // Fix "analyse" mishearings: "unless", "analyze", "and let's", "anlsys",
  // "anlyss", "anlys", "anlss", "analis", "analys" (without trailing e),
  // "analysis" (noun form → verb form)
  // tiny.en often drops or garbles the "analyse" word
  if (/^unless\b/i.test(t)) {
    t = t.replace(/^unless\b/i, "analyse");
    logFixes.push("unless→analyse");
  }
  if (/^analyze\b/i.test(t)) {
    t = t.replace(/^analyze\b/i, "analyse");
    logFixes.push("analyze→analyse");
  }
  if (/^and let's\b/i.test(t)) {
    t = t.replace(/^and let's\b/i, "analyse");
    logFixes.push("and let's→analyse");
  }
  // "analysis" → "analyse" (noun form misheard for verb)
  if (/^analysis\b/i.test(t)) {
    t = t.replace(/^analysis\b/i, "analyse");
    logFixes.push("analysis→analyse");
  }
  // "anlsys", "anlyss", "anlys", "anlss", "analis" → "analyse"
  if (/^an(?:l|n)?s[yi]?s\b/i.test(t)) {
    t = t.replace(/^an(?:l|n)?s[yi]?s\b/i, "analyse");
    logFixes.push("anlsys→analyse");
  }
  // "analys" without trailing "e" → "analyse"
  if (/^analys\b/i.test(t) && !/^analyse\b/i.test(t)) {
    t = t.replace(/^analys\b/i, "analyse");
    logFixes.push("analys→analyse");
  }
  // "check the PR" / "check PR" → "analyse PR" (user says "check" meaning "analyse")
  if (/^check\s+(?:the\s+)?pr\b/i.test(t)) {
    t = t.replace(/^check\s+(?:the\s+)?pr\b/i, "analyse PR");
    logFixes.push("check→analyse");
  }
  // "review the PR" / "review PR" → "analyse PR"
  if (/^review\s+(?:the\s+)?pr\b/i.test(t)) {
    t = t.replace(/^review\s+(?:the\s+)?pr\b/i, "analyse PR");
    logFixes.push("review→analyse");
  }

  // Fix "PR" mishearings: "pf", "p r", "pe" when followed by a number
  // "pf5" → "PR 5", "p r 5" → "PR 5", "pe5" → "PR 5"
  t = t.replace(/\bpf\s*(\d+)\b/gi, "PR $1");
  t = t.replace(/\bp\s*r\s*(\d+)\b/gi, "PR $1");
  t = t.replace(/\bpe\s*(\d+)\b/gi, "PR $1");
  // "pr5" → "PR 5" (no space)
  t = t.replace(/\bpr(\d+)\b/gi, "PR $1");

  // Fix "PR list" mishearings — tiny.en/base.en struggles with "give me the PR list"
  // "Google me the PR list" → "give me the PR list"
  if (/^google\s+me\s+(?:the\s+)?pr\s*list/i.test(t)) {
    t = t.replace(/^google\s+me\s+/i, "give me ");
    logFixes.push("google me→give me");
  }
  // "So, you have to list" → "show me the PR list"
  if (/^so,?\s+you\s+have\s+to\s+list/i.test(t)) {
    t = "show me the PR list";
    logFixes.push("so you have to list→show me the PR list");
  }
  // "So, we are list" → "show me the PR list"
  if (/^so,?\s+we\s+are\s+list/i.test(t)) {
    t = "show me the PR list";
    logFixes.push("so we are list→show me the PR list");
  }
  // "So, meet a PR list" → "show me the PR list"
  if (/^so,?\s+meet\s+a\s+pr\s*list/i.test(t)) {
    t = "show me the PR list";
    logFixes.push("so meet a PR list→show me the PR list");
  }
  // "give me the PR this" → "give me the PR list"
  t = t.replace(/\bpr\s+this\b/gi, "PR list");
  // Strip leading "So, " filler that STT often inserts
  if (/^so,?\s+/i.test(t) && !/^so,?\s+(show|give|list|open|close|analyse|merge|approve)/i.test(t)) {
    t = t.replace(/^so,?\s+/i, "");
    logFixes.push("so→(stripped)");
  }

  // Fix known repo name mishearings.
  // tiny.en (39M params) struggles with multi-word and hyphenated repo names.
  // This map covers common phonetic mishearings for the user's repos.
  // The Worker also does fuzzy matching, but fixing client-side means the
  // user sees the corrected name in the orb/sidebar instead of the misheard one.
  const repoCorrections: Array<[RegExp, string]> = [
    // "ledger ai" mishearings: "lageria", "ledger a", "ledger i", "ledgeria", "leg daria"
    [/\b(?:in|of|from|for)\s+(lageria|ledgeria|ledger\s*a|ledger\s*i|leg\s*daria|lager\s*ai|ledger\s*are)\b/gi, " in ledger-ai"],
    // "servx" mishearings (existing)
    [/\b(?:in|of|from|for)\s+(?:cervix|service|weeks|serve\s*x|ser\s*fixes|surf\s*x|ser\s*vicks)\b/gi, " in servx"],
    // "zync" mishearings: "zinc", "sink", "sync", "zinck", "zin", "unzinc", "on zinc"
    [/\b(?:in|of|from|for)\s+(zinc|sink|sync|zinck|zin|unzinc|on\s*zinc)\b/gi, " in zync"],
    // "nexus" mishearings: "nexus", "nexa", "nexis", "nexus agent"
    [/\b(?:in|of|from|for)\s+(nexa|nexis|nexus\s*agent)\b/gi, " in nexus"],
  ];
  for (const [pattern, replacement] of repoCorrections) {
    const before = t;
    t = t.replace(pattern, replacement);
    if (t !== before) {
      // Extract the repo name from the replacement for logging
      const repoName = replacement.trim().replace(/^(?:in|of|from)\s+/, "");
      logFixes.push(`repo→${repoName}`);
    }
  }

  // Fix truncated "open-" / "open " from Intel SST mic silence.
  // When the mic driver cuts out mid-utterance, STT only captures the
  // first word. "open architecture mapper" becomes "open-" or "open".
  // Since "open" is almost always followed by "architecture" in this
  // app's context, expand the truncation to the full command.
  if (/^open-?$/i.test(t.trim())) {
    t = "open architecture mapper";
    logFixes.push("open-→open architecture mapper (mic truncation recovery)");
  }

  if (logFixes.length > 0 || t !== transcript) {
    console.log(`[NEXUS] STT correction: "${transcript}" → "${t}"`);
  }
  return t;
}

// ─── Self-Learning STT Corrections ───────────────────────────────────
// Learned corrections are loaded from the Rust side at startup and applied
// after the hardcoded corrections above. See stt_learning.rs.

let learnedCorrections: Array<{ from: string; to: string }> = [];

async function loadLearnedCorrections(): Promise<void> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const result = await invoke<[string, string][]>("get_learned_corrections");
    learnedCorrections = result.map(([from, to]) => ({ from, to }));
    if (learnedCorrections.length > 0) {
      console.log(`[NEXUS] Loaded ${learnedCorrections.length} learned STT corrections`);
    }
  } catch {
    // Outside Tauri or command not available — silently skip
  }
}

function applyLearnedCorrections(transcript: string): string {
  let t = transcript;
  for (const { from, to } of learnedCorrections) {
    if (t.includes(from)) {
      t = t.replace(new RegExp(escapeRegExp(from), "gi"), to);
    }
  }
  return t;
}

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

async function logFailedTranscript(transcript: string): Promise<void> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("log_failed_transcript", { transcript });
  } catch {
    // Outside Tauri — silently skip
  }
}

async function logSuccessfulTranscript(transcript: string): Promise<void> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("log_successful_transcript", { transcript });
  } catch {
    // Outside Tauri — silently skip
  }
}

// Load learned corrections at module init
void loadLearnedCorrections();

/**
 * Audio recorder using ScriptProcessorNode (proven reliable in WebView2/Electron).
 *
 * AUDIO STAYS LOCAL: Float32 samples are buffered in memory on the device.
 * They are NOT sent to the server. When VAD detects silence, the buffered
 * audio is downsampled to 16kHz, converted to Int16 PCM, and sent to the
 * LOCAL faster-whisper STT engine (Tauri command) for transcription.
 * Only the resulting TEXT is sent to the remote NEXUS server.
 *
 * VAD (`vad.ts`) controls start/stop of the recorder.
 * The MediaStream is acquired in `main.tsx` on wake and shared between
 * the recorder and VAD to avoid opening two mic streams.
 */

let audioCtx: AudioContext | null = null;
let scriptNode: ScriptProcessorNode | null = null;
let mediaStreamSource: MediaStreamAudioSourceNode | null = null;

/** Expose the current recording AudioContext so VAD can reuse it
 *  instead of creating a second AudioContext for the same stream. */
export function getRecordingContext(): AudioContext | null {
  return audioCtx;
}

/** Buffer of Float32 samples at native sample rate (e.g. 48kHz). */
let floatBuffer: Float32Array[] = [];

/** The native sample rate of the AudioContext (e.g. 48000). */
let nativeSampleRate = 48000;

/** Guard: true while finishCapture is in progress. Prevents abortCapture
 *  from clearing floatBuffer mid-transcription (race condition fix). */
let captureInProgress = false;

// Auto-retry is disabled — the system goes to idle after a single failed
// capture and waits for explicit user input (hotkey or wake word).
// This prevents the system from continuously waking up and capturing
// room noise/hallucinations without user input.

/**
 * Start recording from an EXISTING MediaStream (acquired by the caller).
 * Uses ScriptProcessorNode — the proven approach for WebView2/Electron.
 *
 * Key design decisions (based on research of VS Code, Runanywhere SDK, Sokuji):
 *   - Native AudioContext sample rate (NOT forced to 16kHz) — avoids edge cases
 *   - Connect source → node → destination DIRECTLY (no gain node — Chrome
 *     optimizes away silent paths, which was the root cause of the AudioWorklet bug)
 *   - Accumulate Float32 samples, downsample to 16kHz after recording
 */
export async function startRecording(stream: MediaStream): Promise<void> {
  if (audioCtx) return; // already recording

  floatBuffer = []; // reset buffer for new turn

  // Use native sample rate — don't force 16kHz. This avoids resampling issues
  // in WebView2's audio pipeline. We downsample to 16kHz after recording.
  audioCtx = new AudioContext();
  nativeSampleRate = audioCtx.sampleRate;

  // Chrome/WebView2 autoplay policy: AudioContext starts "suspended".
  // Must resume() before the graph will process audio.
  if (audioCtx.state === "suspended") {
    await audioCtx.resume();
  }

  mediaStreamSource = audioCtx.createMediaStreamSource(stream);

  // ScriptProcessorNode: deprecated but proven reliable in WebView2/Electron.
  // Buffer size 4096 gives ~85ms at 48kHz — low latency, good throughput.
  scriptNode = audioCtx.createScriptProcessor(4096, 1, 1);

  let frameCount = 0;
  scriptNode.onaudioprocess = (e: AudioProcessingEvent) => {
    const input = e.inputBuffer.getChannelData(0);
    // Copy the Float32Array — the underlying buffer is reused by the browser.
    floatBuffer.push(new Float32Array(input));
    frameCount++;
    if (frameCount === 1) {
      console.log(`[NEXUS] first audio frame received (${input.length} samples @ ${nativeSampleRate}Hz)`);
    }
  };

  // CRITICAL: Connect source → node → destination DIRECTLY.
  // No gain node in between — Chrome optimizes away silent paths (gain=0),
  // which was the root cause of the AudioWorklet bug. The ScriptProcessorNode
  // doesn't write to its output buffer, so the output is silence by default.
  // But Chrome still processes the graph because the connection is direct.
  mediaStreamSource.connect(scriptNode);
  scriptNode.connect(audioCtx.destination);

  useAssistant.getState().setState("listening");
}

export async function stopRecording(): Promise<void> {
  if (scriptNode) {
    scriptNode.disconnect();
    scriptNode.onaudioprocess = null;
    scriptNode = null;
  }
  if (mediaStreamSource) {
    mediaStreamSource.disconnect();
    mediaStreamSource = null;
  }
  if (audioCtx) {
    await audioCtx.close();
    audioCtx = null;
  }
}

/**
 * Downsample Float32 audio from native rate to 16kHz using block averaging.
 * Then convert to Int16 PCM — the format the local STT server expects.
 */
function downsampleAndConvert(float32: Float32Array, inRate: number, outRate: number): Int16Array {
  if (outRate >= inRate) {
    // No downsampling needed — just convert float32 → int16
    const pcm = new Int16Array(float32.length);
    for (let i = 0; i < float32.length; i++) {
      const s = Math.max(-1, Math.min(1, float32[i]));
      pcm[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
    }
    return pcm;
  }

  const ratio = inRate / outRate;
  const outLen = Math.floor(float32.length / ratio);
  const pcm = new Int16Array(outLen);

  for (let i = 0; i < outLen; i++) {
    const start = Math.floor(i * ratio);
    const end = Math.min(float32.length, Math.floor((i + 1) * ratio));
    let sum = 0;
    let n = 0;
    for (let j = start; j < end; j++) {
      sum += float32[j];
      n++;
    }
    const avg = n ? sum / n : 0;
    const s = Math.max(-1, Math.min(1, avg));
    pcm[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
  }

  return pcm;
}

/**
 * Open the backend session (non-fatal) and start recording.
 *
 * CRITICAL: Recording starts FIRST, then the backend session is opened in
 * the background. This eliminates the ~1 second delay where the orb was
 * visible but the mic wasn't recording yet (TCP connection timeout to the
 * unavailable backend was blocking startRecording).
 *
 * The user can speak the instant the orb appears — no words are lost.
 */
export async function captureUntilSilence(
  stream: MediaStream,
  serverUrl?: string,
  token?: string,
): Promise<void> {
  // Start recording IMMEDIATELY — don't wait for the backend session.
  // The mic must be capturing audio the moment the orb appears so the
  // user's first words aren't lost.
  await startRecording(stream);

  // Try to open the backend session in the background (fire and forget).
  // If the backend is unavailable, local-only mode still works.
  // This runs AFTER startRecording so it never blocks audio capture.
  openSession(serverUrl, token).catch((err) => {
    console.warn("[NEXUS] backend session unavailable (local-only mode):", err);
  });
}

/** Release the mic stream (stops all tracks, frees the hardware). */
function releaseMicStream(): void {
  const release = (window as any).__NEXUS_RELEASE_MIC__;
  if (typeof release === "function") release();
}

/**
 * Wait until TTS is no longer playing.
 *
 * CRITICAL FIX: This now checks BOTH the Rust/rodio TTS playback state
 * AND the Web Speech API. Previously it only checked speechSynthesis.speaking,
 * which was always false for Rust TTS (Edge TTS / Piper via rodio). This caused
 * waitForTtsIdle() to return immediately while audio was still playing, creating
 * an echo feedback loop where TTS audio was captured by the mic.
 *
 * The rustTtsPlaying flag is set by speak()/speakCached() when invoke("speak_text")
 * starts and cleared when the invoke resolves (playback complete) or stopTts() is called.
 *
 * Polls every 100ms. Times out after 10s as a safety net (Piper cold-load can take ~7s).
 */
function waitForTtsIdle(): Promise<void> {
  return new Promise((resolve) => {
    const checkInitial = () => {
      const webSpeechPlaying = typeof speechSynthesis !== "undefined" && speechSynthesis.speaking;
      if (!isRustTtsPlaying() && !webSpeechPlaying) {
        resolve();
        return;
      }
      const start = Date.now();
      const check = () => {
        const webSpeechStillPlaying = typeof speechSynthesis !== "undefined" && speechSynthesis.speaking;
        if ((!isRustTtsPlaying() && !webSpeechStillPlaying) || Date.now() - start > 10000) {
          resolve();
          return;
        }
        setTimeout(check, 100);
      };
      setTimeout(check, 100);
    };
    checkInitial();
  });
}

/**
 * Process a transcript from the Rust-side STT capture.
 * This is the same logic as finishCapture() but without the audio
 * capture/STT parts — the transcript is already available.
 *
 * Called when the Rust cpal stream captures audio, transcribes it,
 * and emits the "stt:transcript" event.
 */
export async function processTranscript(transcript: string): Promise<void> {
  if (!transcript) {
    // Failed capture — speak "Didn't catch that" and go back to idle.
    // Do NOT auto-retry. Wait for explicit user input (hotkey or wake word)
    // before capturing again. Auto-retry caused the system to keep waking
    // up and capturing room noise/hallucinations without user input.
    console.warn("[NEXUS] Rust STT returned empty transcript — going to idle");
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage("Didn't catch that, sir.");
    await speak("Didn't catch that sir");
    useAssistant.getState().setVisible(false);
    setTimeout(() => useAssistant.getState().reset(), 550);
    return;
  }

  // Successful transcript

  // 1b. Post-process the transcript to fix common STT mishearings.
  let corrected = correctSttTranscript(transcript);
  corrected = applyLearnedCorrections(corrected);

  // Log successful transcript for self-learning
  void logSuccessfulTranscript(corrected);

  // 2. Add the transcript to the UI.
  useAssistant.getState().addUserMessage(corrected);

  // 2b. INSTANT ACK for long-running queries — BEFORE intent parsing.
  const isLong = isLongRunningQuery(corrected);
  if (isLong) {
    if (isLongRunningInFlight()) {
      await handleDuplicateOrQueuedLongRunning(corrected);
      return;
    }
    console.log("[NEXUS] instant ack (before parsing): long-running query detected");
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage("On it sir.");
    setLocalAckGiven();
    void speakCached("On it sir");
  }

  // 3. LOCAL-FIRST: Parse the intent locally.
  const { intent } = await parseTranscriptEnhanced(corrected);

  // Special case: open architecture mapper window directly
  if (intent.action === "open_architect") {
    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 1200));
    useAssistant.getState().setVisible(false);
    useAssistant.getState().setLoadingVisible(true);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("open_architect_with_auto_detect");
    } catch (err) {
      console.error("[NEXUS] failed to open architect window:", err);
      const errMsg = String(err).toLowerCase();
      if (errMsg.includes("no github repository") || errMsg.includes("no repo")) {
        useAssistant.getState().setLoadingVisible(false);
        useAssistant.getState().setVisible(true);
        useAssistant.getState().setState("speaking");
        useAssistant.getState().addAssistantMessage("No repository found, sir. Open a repo in your browser or GitHub Desktop.");
        void speak("No repository found sir. Open a repo in your browser or GitHub Desktop.");
        await waitForTtsIdle();
        await new Promise((resolve) => setTimeout(resolve, 800));
        useAssistant.getState().setVisible(false);
        setTimeout(() => useAssistant.getState().reset(), 550);
      } else {
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          await invoke("open_architect_window");
        } catch {}
      }
    }
    useAssistant.getState().setLoadingVisible(false);
    setTimeout(() => useAssistant.getState().reset(), 550);
    return;
  }

  // Special case: open settings sidebar (command center)
  if (intent.action === "open_settings") {
    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 800));
    useAssistant.getState().setVisible(false);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("show_settings_sidebar");
    } catch (err) {
      console.error("[NEXUS] failed to open settings sidebar:", err);
    }
    setTimeout(() => useAssistant.getState().reset(), 550);
    return;
  }

  // Analyse intents and GitHub commands go to the remote backend / orchestrator
  if (isAnalyseIntent(intent) || intent.action === "github_command") {
    console.log("[NEXUS] analyse/github intent detected, sending to orchestrator:", intent);
  } else if (intent.action === "greeting") {
    useAssistant.getState().setLoadingVisible(false);
    useAssistant.getState().setVisible(true);
    const reply = (intent as { reply: string }).reply;
    console.log("[NEXUS] local greeting reply:", reply);
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage(reply);
    void speak(reply.replace(/,/g, ""));
    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 800));
    useAssistant.getState().setVisible(false);
    setTimeout(() => useAssistant.getState().reset(), 550);
    return;
  } else if (intent.action !== "unknown") {
    // Known local command — execute it directly.
    useAssistant.getState().setLoadingVisible(false);
    useAssistant.getState().setVisible(true);
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage("Ok sir.");
    void speak("Ok sir.");
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const result = await invoke<{ success: boolean; message: string }>("execute_command", { intent });
      console.log("[NEXUS] local command result:", result);
      if (result.message && result.message !== "Ok sir.") {
        useAssistant.getState().addAssistantMessage(result.message);
        void speak(result.message.replace(/,/g, ""));
      }
    } catch (err) {
      console.error("[NEXUS] command execution failed:", err);
    }
    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 800));
    useAssistant.getState().setVisible(false);
    setTimeout(() => useAssistant.getState().reset(), 550);
    return;
  }

  // 4. Unknown intent (or analyse/github intent) — route through the CENTRAL ORCHESTRATOR.
  try {
    const isLongFinal = isLong || isAnalyseIntent(intent) || intent.action === "github_command";
    console.log("[NEXUS] processTranscript: intent=", intent.action, "isLongRunning=", isLongFinal, "transcript=", corrected);

    if (isLongFinal && !isLongRunningInFlight()) {
      setLongRunningInFlight(corrected, processNextQueuedCommand);
      // Don't hide the orb here — the orchestrator's loading event
      // will hide it when the loading indicator appears. This avoids
      // a gap where neither the orb nor the loading animation is visible.
    }

    const result = await processViaOrchestrator(corrected);
    console.log("[NEXUS] orchestrator process result:", result);

    if (result?.handled_locally) {
      return;
    }
    return;
  } catch (err) {
    console.warn("[NEXUS] orchestrator unavailable for unknown query:", err);
  }

  // 5. Neither local intent nor backend available.
  useAssistant.getState().setLoadingVisible(false);
  useAssistant.getState().setVisible(true);
  useAssistant.getState().setState("speaking");
  useAssistant.getState().addAssistantMessage("Didn't catch that, sir.");
  await speak("Didn't catch that sir");
  void logFailedTranscript(corrected);
  useAssistant.getState().setVisible(false);
  setTimeout(() => useAssistant.getState().reset(), 550);
}

/**
 * Called by VAD on silence: stop the recorder, run local STT on the
 * buffered audio, send the transcript text to the server, and speak
 * the acknowledgement locally.
 *
 * This is the key function — audio is processed locally, only text
 * crosses the network.
 */
export async function finishCapture(): Promise<void> {
  // Guard: prevent re-entrant finishCapture (e.g. VAD safety cap + speech end).
  if (captureInProgress) return;
  captureInProgress = true;

  await stopRecording();

  // SYNCHRONOUSLY copy the buffer before any await — abortCapture might
  // clear floatBuffer while we're waiting for STT (race condition fix).
  const totalFloat = floatBuffer.reduce((sum, arr) => sum + arr.length, 0);
  const allFloat = new Float32Array(totalFloat);
  let offset = 0;
  for (const chunk of floatBuffer) {
    allFloat.set(chunk, offset);
    offset += chunk.length;
  }
  floatBuffer = []; // free the buffer

  if (totalFloat === 0) {
    console.warn("no audio captured");
    releaseMicStream();
    // Hide FIRST, then reset after slide-down completes (prevents animation glitch).
    useAssistant.getState().setVisible(false);
    setTimeout(() => useAssistant.getState().reset(), 550);
    captureInProgress = false;
    return;
  }

  // Downsample from native rate (e.g. 48kHz) to 16kHz and convert to Int16 PCM.
  console.log(`[NEXUS] captured ${totalFloat} samples @ ${nativeSampleRate}Hz, downsampling to 16kHz`);
  const allPcm = downsampleAndConvert(allFloat, nativeSampleRate, 16000);
  console.log(`[NEXUS] downsampled to ${allPcm.length} Int16 samples @ 16kHz`);

  // 1. Local STT — audio goes to faster-whisper, never to the remote server.
  useAssistant.getState().setState("thinking");
  let transcript = await transcribeAudio(allPcm);

  // Mic stream is no longer needed — release it now to free the hardware.
  releaseMicStream();

  if (!transcript) {
    // Failed capture — speak "Didn't catch that" and go back to idle.
    // Do NOT auto-retry. Wait for explicit user input (hotkey or wake word).
    console.warn("STT returned empty transcript — going to idle");
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage("Didn't catch that, sir.");
    await speak("Didn't catch that sir");
    useAssistant.getState().setVisible(false);
    setTimeout(() => useAssistant.getState().reset(), 550);
    captureInProgress = false;
    return;
  }
  // Successful transcript

  // 1b. Post-process the transcript to fix common STT mishearings.
  transcript = correctSttTranscript(transcript);
  transcript = applyLearnedCorrections(transcript);

  // Log successful transcript for self-learning
  void logSuccessfulTranscript(transcript);

  // 2. Add the transcript to the UI.
  useAssistant.getState().addUserMessage(transcript);

  // 2b. INSTANT ACK for long-running queries — BEFORE intent parsing.
  //     The NLU server can take 3-4s to cold-start on first use, and the
  //     user shouldn't wait in silence. isLongRunningQuery() is a pure
  //     regex check (<1ms) that catches "analyse PR/repo/branch" patterns.
  //     We give "On it sir" immediately, then parse + send in the background.
  const isLong = isLongRunningQuery(transcript);
  if (isLong) {
    // Check dedup/queue BEFORE acking
    if (isLongRunningInFlight()) {
      captureInProgress = false;
      await handleDuplicateOrQueuedLongRunning(transcript);
      return;
    }
    // Immediate ack — fire and forget, don't block on TTS
    console.log("[NEXUS] instant ack (before parsing): long-running query detected");
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage("On it sir.");
    setLocalAckGiven(); // prevent server ack from double-speaking
    // DON'T hide the orb or show loading yet — wait for "On it sir" TTS
    // to complete first. The user's desired flow is:
    //   "On it sir" plays → orb disappears → loading animation → response
    // If parsing later reveals this is a local command, we roll back below.
    void speakCached("On it sir");
  }

  // 3. LOCAL-FIRST: Parse the intent locally. If it's a known local command
  //    (open app, open URL, search), execute it locally — no need to send
  //    to the remote backend. Only send to the backend if the intent is
  //    "unknown" (i.e. it's a conversational query needing n8n/Ollama).
  //    Uses the Rust-side enhanced parser (app registry + analyse patterns).
  const { intent } = await parseTranscriptEnhanced(transcript);

  // Special case: open architecture mapper window directly
  if (intent.action === "open_architect") {
    // Flow: "On it sir" plays → orb disappears → loading animation
    // → Phase 1 + AI enrichment run in background (2-5s)
    // → architect window opens with map ALREADY RENDERED
    // → loading animation hides
    //
    // We use open_architect_with_auto_detect (not open_architect_window)
    // because it runs Phase 1 analysis BEFORE opening the window, so the
    // user never sees "Waiting for repository..." — the map is ready when
    // the window appears.

    // Step 1: Wait for "On it sir" TTS to finish playing.
    // The orb is still visible at this point.
    // waitForTtsIdle checks speechSynthesis.speaking (Web Speech API),
    // but our TTS is played via Rust (edge-tts → rodio), so we also add
    // a fixed delay matching the "On it sir" audio duration (~1.2s).
    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 1200));

    // Step 2: Now hide the orb and show the loading animation.
    useAssistant.getState().setVisible(false);
    useAssistant.getState().setLoadingVisible(true);

    try {
      const { invoke } = await import("@tauri-apps/api/core");
      // This runs Phase 1 + AI enrichment in the background, then opens
      // the architect window with the completed map. The loading indicator
      // stays visible until this returns.
      await invoke("open_architect_with_auto_detect");
    } catch (err) {
      console.error("[NEXUS] failed to open architect window:", err);
      const errMsg = String(err).toLowerCase();
      if (errMsg.includes("no github repository") || errMsg.includes("no repo")) {
        // No repo detected — speak error and hide loading, do NOT open window
        useAssistant.getState().setLoadingVisible(false);
        useAssistant.getState().setVisible(true);
        useAssistant.getState().setState("speaking");
        useAssistant.getState().addAssistantMessage("No repository found, sir. Open a repo in your browser or GitHub Desktop.");
        void speak("No repository found sir. Open a repo in your browser or GitHub Desktop.");
        await waitForTtsIdle();
        await new Promise((resolve) => setTimeout(resolve, 800));
        useAssistant.getState().setVisible(false);
        setTimeout(() => useAssistant.getState().reset(), 550);
      } else {
        // Other error — fallback: open without auto-detect
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          await invoke("open_architect_window");
        } catch {}
      }
    }

    // The architect window is now open with the map ready.
    // Hide the loading indicator and reset.
    useAssistant.getState().setLoadingVisible(false);
    setTimeout(() => useAssistant.getState().reset(), 550);
    captureInProgress = false;
    return;
  }

  // Special case: open settings sidebar (command center)
  if (intent.action === "open_settings") {
    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 800));
    useAssistant.getState().setVisible(false);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("show_settings_sidebar");
    } catch (err) {
      console.error("[NEXUS] failed to open settings sidebar:", err);
    }
    setTimeout(() => useAssistant.getState().reset(), 550);
    captureInProgress = false;
    return;
  }

  // Analyse intents and GitHub commands go to the remote backend (they're long-running queries)
  // but the Rust parser has already extracted the repo/PR data.
  if (isAnalyseIntent(intent) || intent.action === "github_command") {
    console.log("[NEXUS] analyse/github intent detected, sending to backend:", intent);
  } else if (intent.action === "greeting") {
    // Roll back loading state — greetings are local, not long-running.
    useAssistant.getState().setLoadingVisible(false);
    useAssistant.getState().setVisible(true);
    // Greeting/conversational reply — speak the reply directly, no "Ok sir."
    // preface and no "execute_command" round-trip (the reply is already
    // in the intent).
    const reply = (intent as { reply: string }).reply;
    console.log("[NEXUS] local greeting reply:", reply);
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage(reply);
    void speak(reply.replace(/,/g, ""));

    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 800));
    useAssistant.getState().setVisible(false);
    setTimeout(() => useAssistant.getState().reset(), 550);
    captureInProgress = false;
    return;
  } else if (intent.action !== "unknown") {
    // Known local command — execute it directly.
    // Roll back loading state — local commands are not long-running.
    useAssistant.getState().setLoadingVisible(false);
    useAssistant.getState().setVisible(true);
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage("Ok sir.");
    void speak("Ok sir.");

    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const result = await invoke<{ success: boolean; message: string }>("execute_command", { intent });
      console.log("[NEXUS] local command result:", result);
      if (result.message && result.message !== "Ok sir.") {
        useAssistant.getState().addAssistantMessage(result.message);
        void speak(result.message.replace(/,/g, ""));
      }
    } catch (err) {
      console.error("[NEXUS] command execution failed:", err);
    }

    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 800));
    useAssistant.getState().setVisible(false);
    setTimeout(() => useAssistant.getState().reset(), 550);
    captureInProgress = false;
    return;
  }

  // 4. Unknown intent (or analyse/github intent) — route through the CENTRAL ORCHESTRATOR.
  //    The orchestrator (Rust) owns the full lifecycle:
  //      - Parses intent (deterministic, <1ms)
  //      - Routes to the correct subsystem (LocalCommand, WorkerBackend, Architect, GitHub)
  //      - Emits ack + shows loading indicator (for long-running)
  //      - Dispatches to the Worker or GitHub API
  //      - Emits result + hides loading
  //    The frontend just calls processViaOrchestrator() and listens for events.
  try {
    const isLongFinal = isLong || isAnalyseIntent(intent) || intent.action === "github_command";
    console.log("[NEXUS] finishCapture: intent=", intent.action, "isLongRunning=", isLongFinal, "transcript=", transcript);

    if (isLongFinal && !isLongRunningInFlight()) {
      // Track in-flight state for dedup + queue (ack already given above)
      setLongRunningInFlight(transcript, processNextQueuedCommand);
      // Don't hide the orb here — the orchestrator's loading event
      // will hide it when the loading indicator appears.
    }
    // Release captureInProgress BEFORE dispatch so subsequent voice
    // commands can be processed while the Worker is generating the response.
    captureInProgress = false;

    // ─── CENTRAL ORCHESTRATOR PATH ───
    // The orchestrator handles: routing, ack, loading, Worker dispatch, result.
    // It emits events on the "orchestrator:event" channel which the
    // frontend listener (net/orchestrator.ts) translates to UI state.
    const result = await processViaOrchestrator(transcript);
    console.log("[NEXUS] orchestrator process result:", result);

    if (result?.handled_locally) {
      // Local command — orchestrator emitted "done", we're finished.
      return;
    }
    // Worker/Architect — orchestrator is handling it.
    // The orchestrator listener will speak ack + result + reset.
    return;
  } catch (err) {
    // Backend unavailable — can't handle this query.
    console.warn("[NEXUS] orchestrator unavailable for unknown query:", err);
  }

  // 5. Neither local intent nor backend available.
  // Roll back loading state if it was set (long-running query detected
  // but backend is unavailable — don't leave the loading animation hanging).
  useAssistant.getState().setLoadingVisible(false);
  useAssistant.getState().setVisible(true);
  useAssistant.getState().setState("speaking");
  useAssistant.getState().addAssistantMessage("Didn't catch that, sir.");
  await speak("Didn't catch that sir");
  // Log failed transcript for self-learning
  void logFailedTranscript(transcript);
  useAssistant.getState().setVisible(false);
  setTimeout(() => useAssistant.getState().reset(), 550);
  captureInProgress = false;
}

/** Called on error / cancel: stop everything and close the session.
 *  If finishCapture is in progress, don't clear the buffer — let it finish. */
export async function abortCapture(): Promise<void> {
  // If finishCapture is mid-flight, don't interfere — it has already copied
  // the buffer synchronously and is processing it. Just stop the recording.
  if (captureInProgress) {
    await stopRecording();
    return;
  }
  await stopRecording();
  floatBuffer = [];
  // Cancel any active orchestrator request (barge-in)
  // NOTE: Do NOT close the session here — the session is just config data
  // (worker_url, user_id, device_id) stored in Rust. Closing it on every
  // barge-in causes "no session open" errors on subsequent orchestrator
  // calls. The session should persist for the app lifetime.
  void cancelOrchestrator();
  releaseMicStream();
  useAssistant.getState().reset();
}

/**
 * Called by Silero VAD's onSpeechEnd callback.
 *
 * Silero gives us the audio directly as Float32Array at 16kHz — no
 * downsampling needed. We convert to Int16 PCM and run the same
 * STT → intent → execute flow as finishCapture().
 *
 * This bypasses the ScriptProcessorNode recorder entirely since Silero
 * (via MicVAD) manages its own audio capture with an AudioWorklet.
 */
export async function finishCaptureFromVad(
  audio: Float32Array,
  speculative?: Promise<string> | null,
): Promise<void> {
  console.log("[NEXUS] finishCaptureFromVad: called, captureInProgress=", captureInProgress);
  if (captureInProgress) {
    console.log("[NEXUS] finishCaptureFromVad: SKIPPING — captureInProgress is true");
    return;
  }
  captureInProgress = true;

  // Safety net: if STT hangs for >25s, force-reset so the next command works
  const safetyTimeout = setTimeout(() => {
    if (captureInProgress) {
      console.warn("[NEXUS] captureInProgress stuck for 12s — force resetting");
      captureInProgress = false;
      useAssistant.getState().setState("idle");
      useAssistant.getState().setVisible(false);
      setTimeout(() => useAssistant.getState().reset(), 550);
    }
  }, 12000);

  try {
    await _finishCaptureFromVadInner(audio, speculative);
  } finally {
    clearTimeout(safetyTimeout);
  }
}

async function _finishCaptureFromVadInner(
  audio: Float32Array,
  speculative?: Promise<string> | null,
): Promise<void> {
  // Stop the recorder if it's running (it may be if we fell back to RMS).
  await stopRecording();

  if (!audio || audio.length === 0) {
    console.warn("no audio from VAD");
    releaseMicStream();
    useAssistant.getState().setVisible(false);
    setTimeout(() => useAssistant.getState().reset(), 550);
    captureInProgress = false;
    return;
  }

  // Convert Float32 (-1 to 1) to Int16 PCM — Silero already gives us 16kHz.
  console.log(`[NEXUS] VAD audio: ${audio.length} samples @ 16kHz, converting to Int16 PCM`);
  const pcm = new Int16Array(audio.length);
  for (let i = 0; i < audio.length; i++) {
    const s = Math.max(-1, Math.min(1, audio[i]));
    pcm[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
  }
  console.log(`[NEXUS] converted to ${pcm.length} Int16 samples @ 16kHz`);

  // Release the mic stream — Silero's MicVAD has already captured the audio.
  releaseMicStream();

  // 1. Local STT — audio goes to faster-whisper, never to the remote server.
  //
  // If the VAD fired a speculative transcription when speech first dropped to
  // silence, that request has been running during the redemption window and is
  // usually already finished — so this resolves immediately instead of costing
  // another ~500ms. Any empty/failed result falls through to a normal
  // transcription of the final segment, so this can only be faster, never worse.
  useAssistant.getState().setState("thinking");
  let transcript = "";
  if (speculative) {
    const t0 = performance.now();
    try {
      transcript = await speculative;
    } catch {
      transcript = "";
    }
    if (transcript) {
      console.log(
        `[NEXUS] used speculative transcript after ${Math.round(performance.now() - t0)}ms wait`,
      );
    }
  }
  if (!transcript) {
    transcript = await transcribeAudio(pcm);
  }

  if (!transcript) {
    // Failed capture — speak "Didn't catch that" and go back to idle.
    // Do NOT auto-retry. Wait for explicit user input (hotkey or wake word).
    console.warn("STT returned empty transcript — going to idle");
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage("Didn't catch that, sir.");
    await speak("Didn't catch that sir");
    useAssistant.getState().setVisible(false);
    setTimeout(() => useAssistant.getState().reset(), 550);
    captureInProgress = false;
    return;
  }
  // Successful transcript

  // 1b. Post-process the transcript to fix common STT mishearings.
  transcript = correctSttTranscript(transcript);
  transcript = applyLearnedCorrections(transcript);

  // Log successful transcript for self-learning
  void logSuccessfulTranscript(transcript);

  // 2. Add the transcript to the UI.
  useAssistant.getState().addUserMessage(transcript);

  // 2b. INSTANT ACK for long-running queries — BEFORE intent parsing.
  //     The NLU server can take 3-4s to cold-start on first use, and the
  //     user shouldn't wait in silence. isLongRunningQuery() is a pure
  //     regex check (<1ms) that catches "analyse PR/repo/branch" patterns.
  //     We give "On it sir" immediately, then parse + send in the background.
  const isLong = isLongRunningQuery(transcript);
  if (isLong) {
    // Check dedup/queue BEFORE acking
    if (isLongRunningInFlight()) {
      captureInProgress = false;
      await handleDuplicateOrQueuedLongRunning(transcript);
      return;
    }
    // Immediate ack — fire and forget, don't block on TTS
    console.log("[NEXUS] instant ack (before parsing): long-running query detected");
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage("On it sir.");
    setLocalAckGiven(); // prevent server ack from double-speaking
    // DON'T hide the orb or show loading yet — wait for "On it sir" TTS
    // to complete first. The user's desired flow is:
    //   "On it sir" plays → orb disappears → loading animation → response
    // If parsing later reveals this is a local command, we roll back below.
    void speakCached("On it sir");
  }

  // 3. LOCAL-FIRST: Parse the intent locally. If it's a known local command
  //    (open app, open URL, search), execute it locally — no need to send
  //    to the remote backend. Only send to the backend if the intent is
  //    "unknown" (i.e. it's a conversational query needing n8n/Ollama).
  //    Uses the Rust-side enhanced parser (app registry + analyse patterns).
  const { intent } = await parseTranscriptEnhanced(transcript);

  // Special case: open architecture mapper window directly
  if (intent.action === "open_architect") {
    // Flow: "On it sir" plays → orb disappears → loading animation
    // → Phase 1 + AI enrichment run in background (2-5s)
    // → architect window opens with map ALREADY RENDERED
    // → loading animation hides
    //
    // We use open_architect_with_auto_detect (not open_architect_window)
    // because it runs Phase 1 analysis BEFORE opening the window, so the
    // user never sees "Waiting for repository..." — the map is ready when
    // the window appears.

    // Step 1: Wait for "On it sir" TTS to finish playing.
    // The orb is still visible at this point.
    // waitForTtsIdle checks speechSynthesis.speaking (Web Speech API),
    // but our TTS is played via Rust (edge-tts → rodio), so we also add
    // a fixed delay matching the "On it sir" audio duration (~1.2s).
    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 1200));

    // Step 2: Now hide the orb and show the loading animation.
    useAssistant.getState().setVisible(false);
    useAssistant.getState().setLoadingVisible(true);

    try {
      const { invoke } = await import("@tauri-apps/api/core");
      // This runs Phase 1 + AI enrichment in the background, then opens
      // the architect window with the completed map. The loading indicator
      // stays visible until this returns.
      await invoke("open_architect_with_auto_detect");
    } catch (err) {
      console.error("[NEXUS] failed to open architect window (vad):", err);
      const errMsg = String(err).toLowerCase();
      if (errMsg.includes("no github repository") || errMsg.includes("no repo")) {
        // No repo detected — speak error and hide loading, do NOT open window
        useAssistant.getState().setLoadingVisible(false);
        useAssistant.getState().setVisible(true);
        useAssistant.getState().setState("speaking");
        useAssistant.getState().addAssistantMessage("No repository found, sir. Open a repo in your browser or GitHub Desktop.");
        void speak("No repository found sir. Open a repo in your browser or GitHub Desktop.");
        await waitForTtsIdle();
        await new Promise((resolve) => setTimeout(resolve, 800));
        useAssistant.getState().setVisible(false);
        setTimeout(() => useAssistant.getState().reset(), 550);
      } else {
        // Other error — fallback: open without auto-detect
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          await invoke("open_architect_window");
        } catch {}
      }
    }

    // The architect window is now open with the map ready.
    // Hide the loading indicator and reset.
    useAssistant.getState().setLoadingVisible(false);
    setTimeout(() => useAssistant.getState().reset(), 550);
    captureInProgress = false;
    return;
  }

  // Analyse intents and GitHub commands go to the remote backend (they're long-running queries)
  // but the Rust parser has already extracted the repo/PR data.
  if (isAnalyseIntent(intent) || intent.action === "github_command") {
    console.log("[NEXUS] analyse/github intent detected (vad), sending to backend:", intent);
  } else if (intent.action === "greeting") {
    // Roll back loading state — greetings are local, not long-running.
    useAssistant.getState().setLoadingVisible(false);
    useAssistant.getState().setVisible(true);
    // Greeting/conversational reply — speak the reply directly, no "Ok sir."
    // preface and no "execute_command" round-trip (the reply is already
    // in the intent).
    const reply = (intent as { reply: string }).reply;
    console.log("[NEXUS] local greeting reply (vad):", reply);
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage(reply);
    void speak(reply.replace(/,/g, ""));

    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 800));
    useAssistant.getState().setVisible(false);
    setTimeout(() => useAssistant.getState().reset(), 550);
    captureInProgress = false;
    return;
  } else if (intent.action !== "unknown") {
    // Known local command — execute it directly.
    // Roll back loading state — local commands are not long-running.
    useAssistant.getState().setLoadingVisible(false);
    useAssistant.getState().setVisible(true);
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage("Ok sir.");
    void speak("Ok sir.");

    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const result = await invoke<{ success: boolean; message: string }>("execute_command", { intent });
      console.log("[NEXUS] local command result:", result);
      if (result.message && result.message !== "Ok sir.") {
        useAssistant.getState().addAssistantMessage(result.message);
        void speak(result.message.replace(/,/g, ""));
      }
    } catch (err) {
      console.error("[NEXUS] command execution failed:", err);
    }

    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 800));
    useAssistant.getState().setVisible(false);
    setTimeout(() => useAssistant.getState().reset(), 550);
    captureInProgress = false;
    return;
  }

  // 4. Unknown intent (or analyse/github intent) — route through the CENTRAL ORCHESTRATOR.
  try {
    // isLong was already determined above (before intent parsing).
    // If it's long-running, we already gave the instant ack and handled
    // dedup/queue. Here we just need to set the in-flight flag and send.
    const isLongFinal = isLong || isAnalyseIntent(intent) || intent.action === "github_command";
    console.log("[NEXUS] finishCaptureFromVad: intent=", intent.action, "isLongRunning=", isLongFinal, "transcript=", transcript);

    if (isLongFinal && !isLongRunningInFlight()) {
      // Track in-flight state for dedup + queue (ack already given above)
      setLongRunningInFlight(transcript, processNextQueuedCommand);
    }
    // Release captureInProgress BEFORE dispatch so subsequent voice
    // commands can be processed while the Worker is generating the response.
    // The result handler in orchestrator.ts handles the sidebar + TTS when the
    // response arrives, so we don't need to block here.
    captureInProgress = false;
    const result = await processViaOrchestrator(transcript);
    console.log("[NEXUS] orchestrator process result (vad):", result);
    return;
  } catch (err) {
    console.warn("[NEXUS] orchestrator unavailable for unknown query (vad):", err);
  }

  // 5. Neither local intent nor backend available.
  // Roll back loading state if it was set (long-running query detected
  // but backend is unavailable — don't leave the loading animation hanging).
  useAssistant.getState().setLoadingVisible(false);
  useAssistant.getState().setVisible(true);
  useAssistant.getState().setState("speaking");
  useAssistant.getState().addAssistantMessage("Didn't catch that, sir.");
  await speak("Didn't catch that sir");
  // Log failed transcript for self-learning
  void logFailedTranscript(transcript);
  useAssistant.getState().setVisible(false);
  setTimeout(() => useAssistant.getState().reset(), 550);
  captureInProgress = false;
}
