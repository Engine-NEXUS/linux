import { useAssistant, transition } from "../store/assistant";
import { speak, stopTts } from "../audio/ttsPlayer";

/**
 * Sidebar event helpers — emit events to the sidebar window.
 * The sidebar only shows for server responses (n8n/Ollama/Hermes),
 * NOT for local commands. It slides in WITH the response already rendered
 * (no "Thinking…" state) and stays until dismissed via Ctrl+Space.
 */
async function emitSidebarShow(query: string, text: string): Promise<void> {
  if (!isTauri()) return;
  try {
    // Call the Rust command that shows the sidebar window AND emits the
    // content event from Rust (more reliable than JS-to-JS event delivery).
    await tauriInvoke("show_sidebar_with_content", { query, text });
    console.log("[NEXUS] sidebar: show_sidebar_with_content IPC ok");
  } catch (e) {
    console.warn("[NEXUS] sidebar:show failed:", e);
  }
}

/** Emit sidebar:hide from the frontend. Currently the hotkey (Rust) emits
 * this directly, but this is exported for programmatic dismissal if needed. */
export async function emitSidebarHide(): Promise<void> {
  if (!isTauri()) return;
  try {
    const { emit } = await import("@tauri-apps/api/event");
    await emit("sidebar:hide", {});
  } catch (e) {
    console.warn("[NEXUS] sidebar:hide emit failed:", e);
  }
}

/**
 * Decide whether a server response warrants the sidebar.
 *
 * Three gates, all must pass:
 *   1. Response length >= 80 chars — short replies ("Done, sir") don't need a panel.
 *   2. Query is not a local-command verb (open/close/play/…) — those are handled by the orb.
 *   3. Query contains an info/research intent keyword (check/show/find/what/…).
 *
 * If the server sends an explicit `display: "sidebar"` hint in the result
 * payload, that overrides the heuristic.
 */
function shouldShowSidebar(query: string, response: string): boolean {
  // Gate 1: too short to be worth reading in a panel
  if (response.length < 80) return false;

  // Gate 2: local-command style queries never use the sidebar
  const localVerbs = /^(open|launch|start|close|quit|exit|kill|play|pause|stop|mute|volume|set|turn)\b/i;
  if (localVerbs.test(query.trim())) return false;

  // Gate 3: info/research/server intent markers
  // Note: "analyz" matches "analyze" (American), "analys" matches "analyse"/"analysis" (British)
  const infoIntent = /\b(check|show|list|find|search|look up|what|who|when|where|why|how|explain|summar|review|status|pr|pull request|issue|repo|commit|branch|deploy|log|error|analyz|analys|tell me|give me|get|fetch|read|display)\b/i;
  return infoIntent.test(query);
}

/** Track the pending query so the result handler can decide whether to show the sidebar. */
let pendingQuery = "";

/**
 * HTTP bridge facade (serverless — no WebSocket, no sidecar).
 *
 * The Rust main process makes HTTP POST requests to the Cloudflare Worker
 * and forwards results to the frontend as `assistant:server` events.
 *
 * Protocol (text-only — no audio bytes cross the network):
 *   Client → Worker: POST { request_id, requester, task }
 *   Worker → Client: { reply_text, intent }
 *   Rust emits: state(thinking), ack, result, done
 */

// Hardcoded Worker URL — the real URL comes from get_server_config at runtime.
// Single backend for all installs; change in src-tauri/src/commands.rs WORKER_URL.
const FALLBACK_URL = "https://nexus-worker.chitkullakshya.workers.dev";
const DEVICE_TOKEN = "";

/**
 * Check if we're running inside the Tauri WebView (has IPC bridge).
 */
function isTauri(): boolean {
  return typeof (window as any).__TAURI_INTERNALS__ !== "undefined";
}

/** Lazy-load Tauri APIs only when running inside the Tauri WebView. */
async function tauriInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

async function tauriListen<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<T>(event, (e) => handler(e.payload));
}

// Track the unlisten function for the assistant:server event listener.
// The listener is set up at module load time (below) to ensure events
// are received regardless of how the session was opened.
const _unlistenRef: { fn: (() => void) | null } = { fn: null };

/** Set up the assistant:server event listener at module load time.
 * This ensures events are received regardless of how the session was opened
 * (wake word, hotkey, test script, etc.).
 *
 * We try the synchronous __TAURI__ global first (available in bundled mode)
 * so the listener is registered BEFORE any events can fire. The async
 * import() path is a fallback for dev mode. */
if (isTauri()) {
  try {
    // Synchronous path — works in bundled (release) mode
    const tauriGlobal = (window as any).__TAURI__;
    if (tauriGlobal?.event?.listen) {
      const unlisten = tauriGlobal.event.listen("assistant:server", (event: any) => {
        console.log("[NEXUS] assistant:server callback fired: kind=" + event?.payload?.kind);
        void handle(event.payload as ServerEvent);
      });
      // listen() returns a Promise<UnlistenFn> — store it when resolved
      if (unlisten && typeof unlisten.then === "function") {
        unlisten.then((u: () => void) => { _unlistenRef.fn = u; }).catch(() => {});
      }
      console.log("[NEXUS] assistant:server listener registered (sync path)");
    } else {
      // Fallback: async import (dev mode)
      tauriListen<ServerEvent>("assistant:server", (payload) => {
        void handle(payload);
      }).then((u) => {
        _unlistenRef.fn = u;
        console.log("[NEXUS] assistant:server listener registered (async path)");
      }).catch((e) => {
        console.warn("[NEXUS] failed to set up assistant:server listener:", e);
      });
    }
  } catch (e) {
    console.warn("[NEXUS] failed to set up assistant:server listener:", e);
  }
}

/** Tracks whether a backend session is actually open. */
let sessionOpen = false;

/**
 * Long-running query tracking — dedup + queue.
 *
 * When a long-running query (PR analysis, repo review) is sent to the Worker,
 * we set `longRunningInFlight = true` and record `lastSentTranscript`.
 *
 * If the user says the SAME command while it's processing:
 *   → say "on it sir" + hide orb, do NOT send again (dedup)
 *
 * If the user says a DIFFERENT long-running command while it's processing:
 *   → say "on it sir" + hide orb, add to `pendingLongRunningQueue`
 *   → when the current result arrives, process the next queued command
 */
type LongRunningResultCallback = () => void;
let longRunningResultCb: LongRunningResultCallback | null = null;

/** Track whether we've already given a local "On it sir" ack for the
 *  current long-running query. If true, the server's `ack` event is
 *  suppressed to avoid double-speak. */
let localAckGiven = false;

/** Track whether we've already spoken "Here is the analysis, sir" for the
 *  current result. Prevents duplicate announcements if the result event
 *  fires more than once. Reset when a new turn starts. */
let analysisAnnounced = false;

/** Barge-in flag: set when the user presses Ctrl+Space while NEXUS is
 *  speaking or thinking. When true, any Worker response that arrives
 *  is discarded instead of being spoken. Cleared when a new turn starts. */
let bargedIn = false;

/** Called by main.tsx when the user barges in (Ctrl+Space during TTS). */
export function setBargedIn(): void {
  bargedIn = true;
}

/** Called by main.tsx when a new turn starts (recording begins). */
export function clearBargedIn(): void {
  bargedIn = false;
}

/** Called by recorder.ts when it gives a local "On it sir" ack via
 *  ackLongRunningQuery(). The wsBridge ack handler checks this flag. */
export function setLocalAckGiven(): void {
  localAckGiven = true;
}

/** Reset the local ack flag — called when a new query starts or when
 *  the result arrives. Also resets the analysis-announced guard. */
function resetLocalAck(): void {
  localAckGiven = false;
  analysisAnnounced = false;
}

/** Called by recorder.ts before sending a long-running query. */
export function setLongRunningInFlight(transcript: string, onResult: LongRunningResultCallback): void {
  longRunningInFlight = true;
  lastSentTranscript = normalizeTranscript(transcript);
  longRunningResultCb = onResult;
  // NOTE: Do NOT reset localAckGiven here — it was set by ackLongRunningQuery()
  // just before this call, and the server ack needs to be suppressed.
  // The flag is reset when the result arrives (clearLongRunningInFlight).
  // Safety timeout: auto-clear after 60s in case the Worker never responds
  if (longRunningTimeout) clearTimeout(longRunningTimeout);
  longRunningTimeout = setTimeout(() => {
    console.warn("[NEXUS] long-running query timeout — auto-clearing in-flight flag");
    longRunningInFlight = false;
    lastSentTranscript = "";
    longRunningResultCb = null;
  }, 60_000);
}

/** Called by recorder.ts to check if a long-running query is in flight. */
export function isLongRunningInFlight(): boolean {
  return longRunningInFlight;
}

/** Called by recorder.ts to check if a transcript matches the in-flight one. */
export function isDuplicateLongRunning(transcript: string): boolean {
  return longRunningInFlight && lastSentTranscript === normalizeTranscript(transcript);
}

/** Normalize transcript for dedup comparison. */
function normalizeTranscript(t: string): string {
  return t.toLowerCase().trim().replace(/[.,!?;:'"]/g, "").replace(/\s+/g, " ");
}

let longRunningInFlight = false;
let lastSentTranscript = "";
let longRunningTimeout: ReturnType<typeof setTimeout> | null = null;

/** Called by wsBridge when a result arrives — clears in-flight + fires callback. */
function clearLongRunningInFlight(): void {
  if (longRunningTimeout) {
    clearTimeout(longRunningTimeout);
    longRunningTimeout = null;
  }
  const wasInFlight = longRunningInFlight;
  longRunningInFlight = false;
  lastSentTranscript = "";
  const cb = longRunningResultCb;
  longRunningResultCb = null;
  resetLocalAck();
  if (wasInFlight && cb) {
    try { cb(); } catch (e) { console.warn("[NEXUS] long-running result callback error:", e); }
  }
}

/** Cached server config — loaded once from Rust at first use, then reused. */
let cachedConfig: { url: string; token: string; userId: string; deviceId: string } | null = null;

/**
 * Load server config from Rust (reads nexus-config.json).
 * Falls back to the hardcoded Worker URL if not in Tauri or IPC fails.
 */
export async function getServerConfig(): Promise<{ url: string; token: string; userId: string; deviceId: string }> {
  if (cachedConfig) return cachedConfig;

  if (isTauri()) {
    try {
      const config = await tauriInvoke<{ serverUrl: string; userId: string; deviceId: string }>("get_server_config");
      cachedConfig = {
        url: config.serverUrl,
        token: DEVICE_TOKEN,
        userId: config.userId,
        deviceId: config.deviceId,
      };
      return cachedConfig;
    } catch (err) {
      console.warn("[NEXUS] get_server_config failed, using fallback:", err);
    }
  }

  // Fallback: hardcoded URL + local identity (browser preview, no Tauri IPC)
  cachedConfig = {
    url: FALLBACK_URL,
    token: DEVICE_TOKEN,
    userId: "local-user",
    deviceId: "local-device",
  };
  return cachedConfig;
}

/**
 * Open a backend session — loads the Worker URL + identity from config.
 * No WebSocket connection is made (serverless HTTP architecture).
 * The Worker is called on-demand when sendTranscript() is invoked.
 */
export async function openSession(
  url?: string,
  token?: string,
  userId?: string,
  deviceId?: string,
): Promise<string> {
  if (!isTauri()) return "";

  // If URL not passed, load from runtime config.
  if (!url) {
    const config = await getServerConfig();
    url = config.url;
    token = token ?? config.token;
    userId = userId ?? config.userId;
    deviceId = deviceId ?? config.deviceId;
  }

  const maxRetries = 5;
  const baseDelayMs = 1000;
  let lastErr: unknown = null;

  for (let attempt = 0; attempt < maxRetries; attempt++) {
    try {
      const sessionId = await tauriInvoke<string>("open_session", { url, token, userId, deviceId });
      sessionOpen = true;
      return sessionId;
    } catch (err) {
      lastErr = err;
      if (attempt < maxRetries - 1) {
        const delay = baseDelayMs * Math.pow(2, attempt); // 1s, 2s, 4s, 8s
        console.warn(`[NEXUS] backend session attempt ${attempt + 1}/${maxRetries} failed, retrying in ${delay}ms:`, err);
        await new Promise((r) => setTimeout(r, delay));
      }
    }
  }

  // All retries exhausted — throw so caller falls through to local-only mode.
  throw new Error(`backend session failed after ${maxRetries} retries: ${lastErr}`);
}

/** Returns true if a backend session is currently open. */
export function hasSession(): boolean {
  return sessionOpen;
}

/** Pending dialog context from a previous follow-up question.
 * When non-null, the next sendTranscript call will include this context
 * so the Worker can combine it with the new input. */
let pendingDialogContext: { pending_intent: string; original_request: string; missing: string[] } | null = null;

/** Returns the pending dialog context (if any) and clears it.
 * Called by recorder.ts before sending a follow-up transcript. */
export function consumeDialogContext(): { pending_intent: string; original_request: string; missing: string[] } | null {
  const ctx = pendingDialogContext;
  pendingDialogContext = null;
  return ctx;
}

/** Returns true if there is a pending dialog context (follow-up expected). */
export function hasDialogContext(): boolean {
  return pendingDialogContext !== null;
}

/** Clears the pending dialog context (e.g. on timeout or cancel). */
export function clearDialogContext(): void {
  pendingDialogContext = null;
}

/** Send the transcribed text to the server for processing.
 * Throws if no session is open so the caller can handle the local-only case.
 * Does NOT show the sidebar — the sidebar appears only when the response
 * arrives (in the "result" handler), and only if shouldShowSidebar() agrees. */
export async function sendTranscript(text: string): Promise<void> {
  if (!isTauri()) return;
  if (!sessionOpen) {
    throw new Error("no backend session — local-only mode");
  }
  pendingQuery = text;
  // If there's a pending dialog context, pass it to the Worker so it can
  // combine the original request with this follow-up input.
  const ctx = consumeDialogContext();
  if (ctx) {
    console.log("[NEXUS] sendTranscript: including dialog_context", ctx);
    await tauriInvoke("send_transcript", { text, dialogContext: ctx });
  } else {
    await tauriInvoke("send_transcript", { text });
  }
}

/** Cancel the current turn. */
export async function cancelSession(): Promise<void> {
  if (!isTauri()) return;
  await tauriInvoke("cancel_session", {});
}

export async function closeSession(): Promise<void> {
  if (!isTauri()) return;
  sessionOpen = false;
  await tauriInvoke("close_session", {});
}

interface ServerEvent {
  kind: string;
  state?: string | null;
  data?: string | null;
  message?: string | null;
  analysis?: any;
  dialogState?: {
    expects_followup: boolean;
    pending_intent: string;
    context: {
      pending_intent: string;
      original_request: string;
      missing: string[];
    };
  } | null;
}

async function handle(ev: ServerEvent): Promise<void> {
  const store = useAssistant.getState();
  switch (ev.kind) {
    case "state": {
      const s = (ev.state as any) as AssistantStateKind | undefined;
      if (s && transition(store.state, s)) store.setState(s);
      else if (s) store.setState(s); // allow explicit server override
      break;
    }
    case "ack":
      // Acknowledgement text (e.g. "On it, sir.") — speak it locally and add to transcript.
      // Skip if we've already given a local ack (ackLongRunningQuery in recorder.ts)
      // to avoid double-speak ("On it sir" said twice).
      if (localAckGiven) {
        console.log("[NEXUS] server ack suppressed — local ack already given");
        break;
      }
      // Skip if user barged in — they don't want to hear the ack
      if (bargedIn) {
        console.log("[NEXUS] server ack suppressed — user barged in");
        break;
      }
      if (ev.data) {
        store.addAssistantMessage(ev.data);
        store.setState("speaking");
        void speak(ev.data, () => {
          // After ack finishes, go back to thinking while server processes.
          if (useAssistant.getState().state === "speaking") {
            useAssistant.getState().setState("thinking");
          }
        });
      }
      break;
    case "result":
      // Final result text from the Worker.
      try {
        // Hide the loading indicator immediately (non-blocking). The
        // destroy is dispatched to a thread pool (async Rust command)
        // so it won't block the result handler or the main thread.
        // We add a small delay before showing the sidebar/orb to give
        // the OS compositor time to remove the loading window from the
        // screen, ensuring the loading animation is gone before the
        // response appears — no visual overlap or divergence.
        await new Promise((r) => setTimeout(r, 120));
        console.log("[NEXUS] handle result: pendingQuery=" + pendingQuery + " dataLen=" + (ev.data?.length || 0) + " hasAnalysis=" + ((ev as any).analysis !== undefined) + " hasDialogState=" + (ev.dialogState !== undefined && ev.dialogState !== null));
        // Capture whether this was a long-running query BEFORE clearing the flag.
        // Used to decide whether to say "Here is the analysis, sir" prefix.
        const wasLongRunning = longRunningInFlight;
        clearLongRunningInFlight();
        // If the user barged in (pressed Ctrl+Space) while the Worker was
        // processing, discard the result — they've moved on to a new command.
        if (bargedIn) {
          console.log("[NEXUS] result discarded — user barged in");
          sessionOpen = false;
          break;
        }
        // Store dialog state for follow-up if the Worker expects more info
        if (ev.dialogState && ev.dialogState.expects_followup) {
          console.log("[NEXUS] dialog state: follow-up expected, pending_intent=" + ev.dialogState.pending_intent);
          pendingDialogContext = ev.dialogState.context;
        }
        if (ev.data) {
          store.addAssistantMessage(ev.data);
          // If pendingQuery is empty (e.g. session was opened externally),
          // try to infer the query from the response text
          const query = pendingQuery || (
            ev.data.includes("analyse") || ev.data.includes("analyz") ||
            ev.data.includes("repository") || ev.data.includes("repo") ||
            ev.data.includes("Ok sir,") && ev.data.includes("/")
              ? "analyse" : ""
          );
          const isArchitectQuery =
            // Check the Worker's intent field directly (most reliable)
            (ev as any).intent === "analyze_repo" ||
            (ev as any).intent === "deep_analyse" ||
            // Fallback: check the query text for architect keywords
            (/\b(analy[sz]e|map|understand|explore|create|build|show|generate|architecture|what breaks|blast radius)\b/i.test(query)
              && /\b(repo|repository|codebase|project|architecture|code)\b/i.test(query));

          if (isArchitectQuery && isTauri()) {
            // Detect the active GitHub repo from the foreground window and
            // pass it to the architect window so it auto-starts analysis.
            // Rust stores the repo in a pending static; the architect frontend
            // fetches it on mount via get_pending_architect_repo (race-free).
            void (async () => {
              try {
                const active = await tauriInvoke<{ owner: string; repo: string } | null>("get_active_repo_url");
                const owner = active?.owner;
                const repo = active?.repo;
                await tauriInvoke("open_architect_window", owner && repo ? { owner, repo } : {});
              } catch {
                void tauriInvoke("open_architect_window");
              }
            })();
          }

          const showSidebar = shouldShowSidebar(query, ev.data) || ((ev as any).analysis !== undefined);
          // For long-running queries that don't trigger the sidebar heuristic,
          // still show the sidebar if the response is substantial — the user
          // said "On it sir" and expects "Here is the analysis, sir" back.
          const isLongRunningResult = wasLongRunning || (ev.data && ev.data.length > 200);
          console.log("[NEXUS] handle result: query=" + query + " showSidebar=" + showSidebar + " wasLongRunning=" + wasLongRunning + " isLongRunningResult=" + isLongRunningResult);
          if (showSidebar || isLongRunningResult) {
          // If the Worker included structured analysis data, use the
          // show_sidebar_with_analysis command which stores the analysis
          // in the pending content (race-free for fresh sidebar windows)
          // and also emits the sidebar:analysis event for existing windows.
          if ((ev as any).analysis && isTauri()) {
            try {
              await tauriInvoke("show_sidebar_with_analysis", {
                query: query || "analyse",
                text: ev.data,
                analysis: (ev as any).analysis,
              });
            } catch (e) {
              console.warn("[NEXUS] show_sidebar_with_analysis failed, falling back:", e);
              await emitSidebarShow(query || "analyse", ev.data);
            }
          } else {
            // Show the sidebar with the full response text
            void emitSidebarShow(query || "analyse", ev.data);
          }
          // The orb may have been hidden after "On it sir" — show it briefly
          // so the user sees NEXUS is speaking the confirmation.
          store.setVisible(true);
          store.setState("speaking");
          // Guard against duplicate "Here is the analysis" announcements.
          // The result handler should only fire once per turn, but this
          // prevents double-speak if the event is emitted twice.
          if (!analysisAnnounced) {
            analysisAnnounced = true;
            void speak("Here is the analysis, sir", () => {
              sessionOpen = false;
              store.reset();
              // Auto-close the orb after the short confirmation
              store.setVisible(false);
            });
          } else {
            console.log("[NEXUS] 'Here is the analysis' already announced — skipping duplicate");
            sessionOpen = false;
            store.reset();
            store.setVisible(false);
          }
        } else {
          // Short response — speak it aloud and auto-close
          // The orb may have been hidden — show it for the response.
          store.setVisible(true);
          store.setState("speaking");
          // Check if this is a follow-up question — if so, auto-reopen the mic
          const expectsFollowup = !!(ev.dialogState && ev.dialogState.expects_followup);
          void speak(ev.data, () => {
            if (expectsFollowup && !bargedIn) {
              // Follow-up expected: auto-reopen the mic for the user's response
              console.log("[NEXUS] follow-up: auto-reopening mic after TTS");
              // Import and call triggerFollowupListen from main.tsx
              import("../main").then(({ triggerFollowupListen }) => {
                triggerFollowupListen();
              }).catch((e) => {
                console.warn("[NEXUS] failed to trigger follow-up listen:", e);
                sessionOpen = false;
                store.reset();
                store.setVisible(false);
              });
            } else {
              sessionOpen = false;
              store.reset();
              store.setVisible(false);
            }
          });
        }
      }
      } catch (e) {
        console.error("[NEXUS] handle result error:", e);
      }
      break;
    case "done":
      // "done" from Rust is now only emitted on error/cancel paths.
      // Normal flow: the "result" handler above emits done after TTS.
      clearLongRunningInFlight();
      sessionOpen = false;
      store.reset();
      break;
    case "error":
      clearLongRunningInFlight();
      sessionOpen = false;
      console.error("server error:", ev.message);
      if (ev.message) store.addAssistantMessage(`Error: ${ev.message}`);
      stopTts();
      store.reset();
      // Do NOT auto-hide on error — the user may want to read the error.
      break;
  }
}

type AssistantStateKind = "idle" | "listening" | "thinking" | "speaking";
