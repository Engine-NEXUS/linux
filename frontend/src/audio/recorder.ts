import { useAssistant } from "../store/assistant";
import {
  openSession,
  ensureSessionOpen,
  sendTranscript,
  setLongRunningInFlight,
  setLocalAckGiven,
  isLongRunningInFlight,
  isDuplicateLongRunning,
} from "../net/wsBridge";
import { transcribeAudio } from "./stt";
import { speak, speakCached, isRustTtsPlaying } from "./ttsPlayer";
import { parseIntent, type Intent } from "../intent/parser";
import {
  processViaOrchestrator,
  cancelOrchestrator,
  clearClarificationRequest,
  type TurnProvenance,
} from "../net/orchestrator";
import { GHOST_INVOKE_BUDGET_MS, shouldGhostRoute } from "../net/ghostHotMic";

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
  preParsed?: { intent: Intent; confidence: number; source: string },
): Promise<{ intent: Intent; confidence: number; source: string }> {
  // If Rust already pre-parsed a deterministic intent during STT capture,
  // consume it directly to eliminate the redundant IPC invoke round-trip.
  if (preParsed?.intent) {
    console.log(
      `[NEXUS] pre-parsed fast-path: action=${preParsed.intent.action}, confidence=${preParsed.confidence}, source=${preParsed.source}`,
    );
    return preParsed;
  }
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
 * Check if an intent is a local command that should be executed by
 * command_executor in Rust (e.g. open/close app, media controls, search).
 */
function isLocalExecutableIntent(intent: Intent): boolean {
  switch (intent.action) {
    case "open_app":
    case "open_url":
    case "close_app":
    case "whatsapp_chat":
    case "search":
    case "media_play_pause":
    case "media_next":
    case "media_previous":
    case "media_stop":
      return true;
    default:
      return false;
  }
}

/**
 * Check if an intent belongs to a subsystem that performs network/MCP I/O
 * and may be long-running (requiring ack / loading indicator).
 */
function isLongRunningSubsystemIntent(intent: Intent): boolean {
  return (
    isAnalyseIntent(intent) ||
    intent.action === "github_command" ||
    intent.action === "order_food" ||
    intent.action === "search_product" ||
    intent.action === "send_whatsapp_message"
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
  }).catch(async () => {
    // Session may have lapsed (barge-in closes it) — reopen once and retry.
    // If the Worker is unreachable, the command is dropped with a warning
    // (same as before; local-only mode can't run Worker analysis).
    console.warn(`[NEXUS] queue: send failed, reopening session for "${next}"`);
    if (await ensureSessionOpen()) {
      try {
        await sendTranscript(next);
        console.log(`[NEXUS] queue: sent "${next}" to worker (retry)`);
        return;
      } catch (e) {
        console.warn(`[NEXUS] queue: retry failed for "${next}":`, e);
      }
    } else {
      console.warn(`[NEXUS] queue: dropping "${next}" (backend unreachable)`);
    }
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
  // Strictly require analysis verbs (no generic "create", "show", "check", "what is")
  const hasAnalyse = /\b(analy[sz]e|analy[sz]ing|analy[sz]is|review|deep\s*dive|critique|evaluate|assess|inspect|examine|blast\s+radius)\b/.test(t);
  const hasPR = /\b(pr|pull\s*request)\b/.test(t);
  // Code repo context: strictly repo/repository/codebase (not generic "code" or "project")
  const hasRepo = /\b(repo|repository|codebase)\b/.test(t);
  // Also catch "PR <number>" patterns even without "analyse" (STT may mishear)
  const hasPRNumber = /\bpr\s*#?\s*\d+\b/.test(t);
  // Branch analysis: "analyse branch X", "review branch"
  const hasBranch = /\bbranch(es)?\b/.test(t);
  // Architecture mapper: strictly require explicit architecture mapper phrases
  const isArchitectQuery =
    /\b(open\s+architecture\s+mapper|architecture\s+mapper|codebase\s+diagram|open\s+codebase\s+mapper)\b/.test(t);
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

  // Fix "ghost mode" mishearings (Groq/Whisper acoustic soundalikes)
  if (/^(?:the\s+)?(?:goes to mode|goes to mold|go to mode|post mode|post modern|postmodern|coast mode|gold mode|toast mode|host mode|dose mode|close mode|ghost mood|ghost node|ghost mod)[.?!]?$/i.test(t.trim())) {
    t = "ghost mode";
    logFixes.push("soundalike→ghost mode");
  } else if (/^(?:activate|start|open|enable|turn on)\s+(?:goes to mode|goes to mold|ghost)\b/i.test(t)) {
    t = t.replace(/^(?:activate|start|open|enable|turn on)\s+(?:goes to mode|goes to mold|ghost)\b/i, "start ghost mode");
    logFixes.push("start goes to mode→start ghost mode");
  }

  // Fix app name phonetic mishearings in commands
  t = t.replace(/\b(open|launch|start|close|exit|quit|on|in|to|via)\s+(?:what's up|whats up|what sap|what app|watch app|watts app)\b/gi, "$1 whatsapp");
  t = t.replace(/\b(open|launch|start)\s+(?:vs coat|vs chord|vs cord|visual studio coat)\b/gi, "$1 vs code");
  t = t.replace(/\b(open|launch|start)\s+(?:spot if i|spot a file|spotty fy|spot ify)\b/gi, "$1 spotify");
  t = t.replace(/\b(open|launch|start)\s+(?:this cord|dis cord)\b/gi, "$1 discord");
  t = t.replace(/\b(open|launch|start)\s+(?:u tube|you tube)\b/gi, "$1 youtube");
  t = t.replace(/\b(open|launch|start)\s+(?:note pad|not pad)\b/gi, "$1 notepad");
  t = t.replace(/^(?:stand down|stop it now)[.?!]?$/i, "stop");
  t = t.replace(/^(?:cancel action|cancel task|cancel that)[.?!]?$/i, "cancel");

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

  // Live caption (plan Phase 4): open the live-partial-transcript stream
  // for this turn, bracketing the same window as the batch capture below.
  // Fire-and-forget — a connection failure just leaves the live caption
  // silent for this turn; the batch path never waits on this.
  void import("@tauri-apps/api/core")
    .then(({ invoke }) => invoke("stt_stream_start"))
    .catch(() => {});

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
    // Live caption (plan Phase 4) — a SECOND, non-blocking push of this
    // same chunk to the live-partial-transcript stream. Purely additive:
    // never touches floatBuffer (the batch path above is untouched), and
    // any failure here (stream not started, server unreachable) is
    // swallowed — the live caption just silently stays empty.
    const livePcm = downsampleAndConvert(new Float32Array(input), nativeSampleRate, 16000);
    void import("@tauri-apps/api/core")
      .then(({ invoke }) => invoke("stt_stream_push_chunk", { samples: Array.from(livePcm) }))
      .catch(() => {});
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
  // Live caption (plan Phase 4): close the live-partial-transcript stream.
  // Fire-and-forget, same tolerance as the start() call above.
  void import("@tauri-apps/api/core")
    .then(({ invoke }) => invoke("stt_stream_stop"))
    .catch(() => {});
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
 * which was always false for Rust TTS (Edge TTS / Kokoro via rodio). This caused
 * waitForTtsIdle() to return immediately while audio was still playing, creating
 * an echo feedback loop where TTS audio was captured by the mic.
 *
 * The rustTtsPlaying flag is set by speak()/speakCached() when invoke("speak_text")
 * starts and cleared when the invoke resolves (playback complete) or stopTts() is called.
 *
 * Polls every 100ms. Times out after 10s as a safety net (local Kokoro cold-load + first sentence can take several seconds).
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
/**
 * Main Center UI-director rule (doc 74 P2): the orb stays visible for the
 * WHOLE ghost session. Every turn-end hide in this file goes through here —
 * 27 sites. Outside ghost mode this is exactly setVisible(false).
 * (Ghost enter already forces visible=true via setGhostActive.)
 */
export function hideOrbIfIdle(): void {
  if (useAssistant.getState().ghostActive) return;
  useAssistant.getState().setVisible(false);
}

/**
 * Consecutive empty-turn streak (non-ghost only; approach E).
 * 1st miss → silent auto-relisten · 2nd consecutive miss → one nag.
 * Any heard speech resets to 0 (wired next to the ghost-silence reset).
 * Pure decision fn so the escalation policy is unit-tested, not re-read.
 */
let missStreak = 0;

export function nextMissAction(misses: number): "relisten" | "nag" {
  return misses <= 1 ? "relisten" : "nag";
}

/** Test hook: read/reset the streak without touching turn flow. */
export function __testMissStreak(): number {
  return missStreak;
}
export function __testSetMissStreak(n: number): void {
  missStreak = n;
}

/**
 * End-of-turn reset that keeps the ghost hot-mic loop alive.
 * Outside ghost mode this is exactly reset() (relisten no-ops);
 * inside a live ghost session it reopens the mic BEFORE resetting
 * so follow-up commands need no wake word. NEVER call from abort /
 * barge-in / silent-park paths — an explicit cancel (or the park
 * after GHOST_SILENT_CAP misses) must stay silent.
 */
async function endTurn(): Promise<void> {
  const { endGhostTurn } = await import("../net/ghostHotMic");
  await endGhostTurn();
}

export interface SttTurnMetadata {
  ownership?: "verified" | "uncertain" | "rejected" | "unenrolled";
  ownerScore?: number;
  decoderBias?: string;
  language?: string;
  initiation?: "explicit" | "ghost";
  intentLabel?: string;
  preParsed?: {
    intent: Intent;
    confidence: number;
    source: string;
  };
}

export async function processTranscript(
  transcript: string,
  turn: SttTurnMetadata = { ownership: "unenrolled" },
): Promise<void> {
  // A new transcript always retires a pending clarification window: the
  // backend installs a new request and cancels the held turn.
  clearClarificationRequest();
  // TEMPORARY tracer (see debug_trace): mark receipt so a silent death
  // downstream is provable from `nexus start` alone. Approach C appends
  // the mic-holder audit: who (if anyone) held a WebView2 mic stream
  // during this turn — the Intel SST hog suspect with no other trace.
  const { invoke: traceInvoke } = await import("@tauri-apps/api/core");
  const { micHoldersSummary } = await import("./micHolders");
  void traceInvoke("debug_trace", { msg: `p0 receipt len=${transcript?.length ?? 0} ghost=${useAssistant.getState().ghostActive} owner=${turn?.ownership ?? "unenrolled"} ${micHoldersSummary()}` }).catch(() => {});
  if (turn?.ownership === "rejected") {
    // Provenance-gated ambient audio: another speaker, television, or media.
    // It must not relisten automatically, nag, action, learn, or remember.
    console.log("[NEXUS] non-owner turn rejected — ambient drop");
    void traceInvoke("debug_trace", { msg: "p0 ambient-drop owner=rejected" }).catch(() => {});
    if (!useAssistant.getState().ghostActive) {
      missStreak = 0;
      useAssistant.getState().setVisible(false);
      // turn-end:keep-raw non-ghost ambient drop; ghost sessions return above and keep the hot mic
      setTimeout(() => useAssistant.getState().reset(), 550);
    }
    return;
  }
  if (!transcript) {
    // Ghost hot-mic: silence during an open ghost session must NOT nag.
    // Ghost turns re-listen quietly, up to GHOST_SILENT_CAP consecutive
    // empties, then park with ONE nag.
    const ghostOn = useAssistant.getState().ghostActive;
    if (ghostOn) {
      const { recordSilentMiss, resetSilentMisses, GHOST_SILENT_CAP } =
        await import("../net/ghostHotMic");
      const misses = recordSilentMiss();
      if (misses < GHOST_SILENT_CAP) {
        console.log(`[NEXUS] ghost hot-mic: silent (${misses}/${GHOST_SILENT_CAP}) — re-listening quietly`);
        // A phantom capture ends without the orb ever reaching `thinking`, so the state is
        // still `listening` and the relisten below would read as a second-press CANCEL
        // (live log: "second press while listening → aborting capture", mic dead).
        // reset() keeps ghostActive and visibility; it only returns the state to idle.
        // turn-end:keep-raw reset FIRST so the relisten is not read as a second-press cancel
        useAssistant.getState().reset();
        const { triggerFollowupListen } = await import("../stage/orbRuntime");
        triggerFollowupListen(true);
        return;
      }
      resetSilentMisses();
      // Fall through to the single nag + park below.
    }
    // Empty transcript (non-ghost): retry-with-escalation (approach E).
    // 1st consecutive miss → silent auto-relisten (no nag, no hide — the
    // turn restarts as if re-woken). 2nd consecutive miss → ONE spoken
    // line, then the legacy hide+reset. Streak resets on any heard speech.
    missStreak += 1;
    if (nextMissAction(missStreak) === "relisten") {
      console.log("[NEXUS] empty turn: silent auto-relisten (miss 1/2)");
      // reset() FIRST (state idle unlocks startListening's second-press
      // guard — without this the relisten below reads as a cancel), then
      // re-wake. Orb stays visible: continuous listening, no flicker.
      // turn-end:keep-raw reset FIRST so the relisten below is not read as a second-press cancel
      useAssistant.getState().reset();
      const { triggerFollowupListen } = await import("../stage/orbRuntime");
      triggerFollowupListen();
      return;
    }
    missStreak = 0;
    console.log("[NEXUS] empty turn: nagging once (miss 2/2)");
    // turn-end:keep-raw (nag turn — closed by the speak promise below)
    useAssistant.getState().setVisible(true);
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage("I didn't hear you, sir.");
    {
      const { speak } = await import("./ttsPlayer");
      void speak("I didn't hear you, sir.")
        .then(() => {
          hideOrbIfIdle();
          // turn-end:keep-raw nag turn ends here, then orb closes (non-ghost path)
          useAssistant.getState().reset();
        })
        .catch(() => {
          hideOrbIfIdle();
          // turn-end:keep-raw nag turn ends here, then orb closes (non-ghost path)
          useAssistant.getState().reset();
        });
    }
    return;
  }

  // Phase 8 — directed-speech gate. Only in a live ghost session (open mic): speech that is not
  // addressed to NEXUS (TV, side talk, our own TTS echo, STT hallucination loops) must not reach the
  // command pipeline or the cloud LLM. Explicit turns (wake / hotkey / confirm window) are always
  // accepted by the Rust side; the gate fails open if it is unavailable.
  if (useAssistant.getState().ghostActive) {
    const { askDirectedGate, gateAction } = await import("../net/directedGate");
    const { recordIgnoredTurn, resetIgnoredTurns, markGhostParked, GHOST_IGNORED_CAP } =
      await import("../net/ghostHotMic");
    const verdict = await askDirectedGate(applyLearnedCorrections(correctSttTranscript(transcript)));
    if (!verdict.accept) {
      // Ignored speech (TV, side talk, echo) has its OWN streak, windowed in time:
      // it used to share the silent-capture counter, so three sentences from the TV
      // parked the mic with no word and the next real command went nowhere.
      const action = gateAction(verdict, recordIgnoredTurn(), GHOST_IGNORED_CAP);
      void traceInvoke("debug_trace", { msg: `p0 directed-gate ignored reason=${verdict.reason} action=${action}` }).catch(() => {});
      console.log(`[NEXUS] directed gate: ignored (${verdict.reason}) — ${action}`);
      if (action === "relisten") {
        // Same reason as the silent-miss path: leave `thinking`/`listening` behind first.
        // turn-end:keep-raw reset FIRST so the relisten is not read as a second-press cancel
        useAssistant.getState().reset();
        const { triggerFollowupListen } = await import("../stage/orbRuntime");
        triggerFollowupListen(true);
      } else {
        // Parked: the hot mic stops re-arming until the user wakes NEXUS (the ghost session
        // itself stays live). Say so once instead of going deaf with no explanation.
        resetIgnoredTurns();
        markGhostParked();
        const parkLine = "Pausing the mic, sir. Say Nexus when you need me.";
        const st = useAssistant.getState();
        st.setVisible(true);
        st.setState("speaking");
        st.addAssistantMessage(parkLine);
        const { speak } = await import("./ttsPlayer");
        void speak(parkLine)
          .catch(() => {})
          .finally(() => {
            // turn-end:keep-raw directed-gate park: no relisten until a wake or hotkey; announced once
            useAssistant.getState().reset();
          });
      }
      return;
    }
  }

  // Successful transcript — any heard speech resets the ghost silence and ignored-turn streaks.
  const { resetSilentMisses: resetGhostSilence, resetIgnoredTurns: resetGhostIgnored, clearGhostParked: clearParked } =
    await import("../net/ghostHotMic");
  resetGhostSilence();
  resetGhostIgnored();
  clearParked();
  missStreak = 0;
  const provenance: TurnProvenance = {
    ownership: turn.ownership ?? "unenrolled",
    ownerScore: turn.ownerScore ?? 1,
    decoderBias: turn.decoderBias ?? "owner",
    language: turn.language ?? "en",
    initiation: useAssistant.getState().ghostActive ? "ghost" : "explicit",
  };

  // 1b. Post-process the transcript to fix common STT mishearings.
  let corrected = correctSttTranscript(transcript);
  corrected = applyLearnedCorrections(corrected);

  // Log successful transcript for self-learning
  void logSuccessfulTranscript(corrected);

  // TEMPORARY tracer: corrected text (catches correction-chain crashes).
  void traceInvoke("debug_trace", { msg: `p1 corrected="${corrected.slice(0, 60)}"` }).catch(() => {});

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
  } else {
    useAssistant.getState().setState("thinking");
  }

  // 3. LOCAL-FIRST: Parse the intent locally (consumes Rust pre-parsed intent if available).
  const { intent } = await parseTranscriptEnhanced(corrected, turn.preParsed);
  // TEMPORARY tracer: the parsed action (split Rust-parse vs TS-fallback).
  void traceInvoke("debug_trace", { msg: `p2 action=${intent.action}` }).catch(() => {});

  // Special case: open architecture mapper window directly
  if (intent.action === "open_architect") {
    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 1200));
    if (!useAssistant.getState().ghostActive) {
      hideOrbIfIdle();
      useAssistant.getState().setLoadingVisible(true);
    }
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
        if (!useAssistant.getState().ghostActive) {
          hideOrbIfIdle();
        }
        setTimeout(() => { void endTurn(); }, 550);
      } else {
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          await invoke("open_architect_window");
        } catch {}
      }
    }
    useAssistant.getState().setLoadingVisible(false);
    setTimeout(() => { void endTurn(); }, 550);
    return;
  }

  // Special case: open settings sidebar (command center)
  if (intent.action === "open_settings") {
    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 800));
    hideOrbIfIdle();
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("show_settings_sidebar");
    } catch (err) {
      console.error("[NEXUS] failed to open settings sidebar:", err);
    }
    setTimeout(() => { void endTurn(); }, 550);
    return;
  }

  if (intent.action === "need_more_info") {
    useAssistant.getState().setLoadingVisible(false);
    useAssistant.getState().setVisible(true);
    const prompt = (intent as { prompt: string }).prompt;
    console.log("[NEXUS] local need_more_info prompt:", prompt);
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage(prompt);
    void speak(prompt.replace(/,/g, ""));
    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 800));
    hideOrbIfIdle();
    setTimeout(() => { void endTurn(); }, 550);
    return;
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
    hideOrbIfIdle();
    setTimeout(() => { void endTurn(); }, 550);
    return;
  } else if (shouldGhostRoute(intent.action, useAssistant.getState().ghostActive)) {
    // Ghost session owns open_app / whatsapp_chat: route to the
    // orchestrator's ghost runners (narration + focus verify + session
    // stays open) instead of silent local execute. Fallback: plain
    // local execute + endTurn (never silent).
    void traceInvoke("debug_trace", { msg: "p3 branch=ghost-route" }).catch(() => {});
    useAssistant.getState().setLoadingVisible(false);
    try {
      const result = await processViaOrchestrator(corrected, undefined, provenance);
      console.log("[NEXUS] ghost-routed intent result:", result);
    } catch (err) {
      console.warn("[NEXUS] ghost route failed, falling back local:", err);
      useAssistant.getState().setLoadingVisible(false);
      useAssistant.getState().setVisible(true);
      useAssistant.getState().setState("speaking");
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const result = await invoke<{ success: boolean; message: string }>("execute_command", { intent });
        if (result.message) {
          useAssistant.getState().addAssistantMessage(result.message);
          void speak(result.message.replace(/,/g, ""));
        }
      } catch {
        useAssistant.getState().addAssistantMessage("Couldn't do that, sir.");
        void speak("Couldn't do that sir.");
      }
      await waitForTtsIdle();
      await new Promise((resolve) => setTimeout(resolve, 800));
      hideOrbIfIdle();
      setTimeout(() => { void endTurn(); }, 550);
    }
    return;
  } else if (isLocalExecutableIntent(intent)) {
    // Known local command — execute it directly.
    void traceInvoke("debug_trace", { msg: "p3 branch=local-execute" }).catch(() => {});
    useAssistant.getState().setLoadingVisible(false);
    useAssistant.getState().setVisible(true);
    useAssistant.getState().setState("speaking");
    // No pre-spoken "Ok sir": success is announced only after execution
    // returns (its message IS "Ok sir." on success). Announcing before
    // the result lied on every failure — e.g. opening the wrong chat.
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const result = await invoke<{ success: boolean; message: string }>("execute_command", { intent });
      console.log("[NEXUS] local command result:", result);
      if (result.message) {
        useAssistant.getState().addAssistantMessage(result.message);
        void speak(result.message.replace(/,/g, ""));
      }
    } catch (err) {
      console.error("[NEXUS] command execution failed:", err);
      useAssistant.getState().addAssistantMessage("Couldn't do that, sir.");
      void speak("Couldn't do that sir.");
    }
    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 800));
    hideOrbIfIdle();
    setTimeout(() => { void endTurn(); }, 550);
    return;
  }

  // 4. Orchestrator-routed intent (MCP, GitHub, Analyse, Ghostwriter, Screen, or general query).
  try {
    const isLongFinal = isLong || isLongRunningSubsystemIntent(intent);
    console.log("[NEXUS] processTranscript: intent=", intent.action, "isLongRunning=", isLongFinal, "transcript=", corrected);
    // TEMPORARY tracer: branch taken + orchestrator outcome.
    void traceInvoke("debug_trace", { msg: "p3 branch=orchestrator" }).catch(() => {});

    if (isLongFinal && !isLongRunningInFlight()) {
      setLongRunningInFlight(corrected, processNextQueuedCommand);
      // Don't hide the orb here — the orchestrator's loading event
      // will hide it when the loading indicator appears. This avoids
      // a gap where neither the orb nor the loading animation is visible.
    }

    // Ghost turns (never long-running here — isLongFinal is false) get a
    // 12s invoke budget: a hung backend previously wedged the hot-mic
    // loop with zero feedback. On expiry the Rust side is cancelled and
    // the throw below falls through to the "Didn't catch that" + relisten
    // path instead of dying silently.
    const ghostTurn = useAssistant.getState().ghostActive && !isLongFinal;
    const result = await processViaOrchestrator(corrected, undefined, provenance, {
      timeoutMs: ghostTurn ? GHOST_INVOKE_BUDGET_MS : undefined,
    });
    // Serialized: CDP forwards console args without remote-object
    // resolution, so a raw object arrives as useless "Object".
    console.log(
      "[NEXUS] orchestrator process result:",
      JSON.stringify(result)?.slice(0, 160) ?? String(result),
    );
    void traceInvoke("debug_trace", { msg: `p4 orch result=${result ? result.subsystem : "null"}` }).catch(() => {});

    if (result?.handled_locally) {
      // Rust returns handled_locally WITHOUT any event for rejected turns,
      // ambient drops, unrecognised phrases and queued commands. Ghost turns
      // are owned by the hot-mic loop: close one that no event closed, right
      // now, and re-arm the mic (it used to wait ~28 s for the stall watchdog).
      const { closeHandledLocallyTurn } = await import("../net/ghostHotMic");
      if (await closeHandledLocallyTurn()) return;
      // Non-ghost: if nothing moved the orb within 8 s it is parked in
      // `thinking` forever; close the turn (long-running work shows the
      // loading indicator, which also exempts it).
      setTimeout(() => {
        const s = useAssistant.getState();
        if (s.state === "thinking" && !s.ghostActive && !s.loadingVisible && !s.awaitingInput) {
          console.warn("[ORB] handled_locally turn left the orb in thinking; closing it");
          // turn-end:keep-raw non-ghost stuck-in-thinking guard; ghost turns are owned by the hot-mic loop
          s.reset();
        }
      }, 8000);
      return;
    }
    return;
  } catch (err) {
    console.warn("[NEXUS] orchestrator unavailable for query:", err);
    void traceInvoke("debug_trace", { msg: `p4 orch threw=${String(err).slice(0, 80)}` }).catch(() => {});
  }

  // 5. Neither local intent nor backend available.
  useAssistant.getState().setLoadingVisible(false);
  useAssistant.getState().setVisible(true);
  useAssistant.getState().setState("speaking");
  useAssistant.getState().addAssistantMessage("Didn't catch that, sir.");
  await speak("Didn't catch that sir");
  void logFailedTranscript(corrected);
  hideOrbIfIdle();
  setTimeout(() => { void endTurn(); }, 550);
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
    hideOrbIfIdle();
    setTimeout(() => { void endTurn(); }, 550);
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
    hideOrbIfIdle();
    setTimeout(() => { void endTurn(); }, 550);
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

    if (!useAssistant.getState().ghostActive) {
      hideOrbIfIdle();
      useAssistant.getState().setLoadingVisible(true);
    }

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
        if (!useAssistant.getState().ghostActive) {
          hideOrbIfIdle();
        }
        setTimeout(() => { void endTurn(); }, 550);
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
    setTimeout(() => { void endTurn(); }, 550);
    captureInProgress = false;
    return;
  }

  // Special case: open settings sidebar (command center)
  if (intent.action === "open_settings") {
    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 800));
    hideOrbIfIdle();
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("show_settings_sidebar");
    } catch (err) {
      console.error("[NEXUS] failed to open settings sidebar:", err);
    }
    setTimeout(() => { void endTurn(); }, 550);
    captureInProgress = false;
    return;
  }

  if (intent.action === "need_more_info") {
    useAssistant.getState().setLoadingVisible(false);
    useAssistant.getState().setVisible(true);
    const prompt = (intent as { prompt: string }).prompt;
    console.log("[NEXUS] local need_more_info prompt (finishCapture):", prompt);
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage(prompt);
    void speak(prompt.replace(/,/g, ""));
    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 800));
    hideOrbIfIdle();
    setTimeout(() => { void endTurn(); }, 550);
    captureInProgress = false;
    return;
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
    hideOrbIfIdle();
    setTimeout(() => { void endTurn(); }, 550);
    captureInProgress = false;
    return;
  } else if (isLocalExecutableIntent(intent)) {
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
    hideOrbIfIdle();
    setTimeout(() => { void endTurn(); }, 550);
    captureInProgress = false;
    return;
  }

  // 4. Orchestrator-routed intent (MCP, GitHub, Analyse, Ghostwriter, Screen, or general query).
  //    The orchestrator (Rust) owns the full lifecycle:
  //      - Parses intent (deterministic, <1ms)
  //      - Routes to the correct subsystem (LocalCommand, WorkerBackend, Architect, GitHub, Mcp)
  //      - Emits ack + shows loading indicator (for long-running)
  //      - Dispatches to the Worker, GitHub API, or MCP bridges
  //      - Emits result + hides loading
  //    The frontend just calls processViaOrchestrator() and listens for events.
  try {
    const isLongFinal = isLong || isLongRunningSubsystemIntent(intent);
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
    console.log(
      "[NEXUS] orchestrator process result:",
      JSON.stringify(result)?.slice(0, 160) ?? String(result),
    );

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
  hideOrbIfIdle();
  setTimeout(() => { void endTurn(); }, 550);
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
  // turn-end:keep-raw (explicit user cancel/barge-in must stay cancelled)
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
      hideOrbIfIdle();
      setTimeout(() => { void endTurn(); }, 550);
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
    hideOrbIfIdle();
    setTimeout(() => { void endTurn(); }, 550);
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
    hideOrbIfIdle();
    setTimeout(() => { void endTurn(); }, 550);
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

    if (!useAssistant.getState().ghostActive) {
      hideOrbIfIdle();
      useAssistant.getState().setLoadingVisible(true);
    }

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
        if (!useAssistant.getState().ghostActive) {
          hideOrbIfIdle();
        }
        setTimeout(() => { void endTurn(); }, 550);
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
    setTimeout(() => { void endTurn(); }, 550);
    captureInProgress = false;
    return;
  }

  if (intent.action === "need_more_info") {
    useAssistant.getState().setLoadingVisible(false);
    useAssistant.getState().setVisible(true);
    const prompt = (intent as { prompt: string }).prompt;
    console.log("[NEXUS] local need_more_info prompt (vad):", prompt);
    useAssistant.getState().setState("speaking");
    useAssistant.getState().addAssistantMessage(prompt);
    void speak(prompt.replace(/,/g, ""));
    await waitForTtsIdle();
    await new Promise((resolve) => setTimeout(resolve, 800));
    hideOrbIfIdle();
    setTimeout(() => { void endTurn(); }, 550);
    captureInProgress = false;
    return;
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
    hideOrbIfIdle();
    setTimeout(() => { void endTurn(); }, 550);
    captureInProgress = false;
    return;
  } else if (isLocalExecutableIntent(intent)) {
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
    hideOrbIfIdle();
    setTimeout(() => { void endTurn(); }, 550);
    captureInProgress = false;
    return;
  }

  // 4. Orchestrator-routed intent (MCP, GitHub, Analyse, Ghostwriter, Screen, or general query).
  try {
    // isLong was already determined above (before intent parsing).
    // If it's long-running, we already gave the instant ack and handled
    // dedup/queue. Here we just need to set the in-flight flag and send.
    const isLongFinal = isLong || isLongRunningSubsystemIntent(intent);
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
    console.log(
      "[NEXUS] orchestrator process result (vad):",
      JSON.stringify(result)?.slice(0, 160) ?? String(result),
    );
    return;
  } catch (err) {
    console.warn("[NEXUS] orchestrator unavailable for query (vad):", err);
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
  hideOrbIfIdle();
  setTimeout(() => { void endTurn(); }, 550);
  captureInProgress = false;
}
