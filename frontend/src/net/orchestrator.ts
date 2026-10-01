/**
 * NEXUS Central Orchestrator — frontend event listener.
 *
 * This module listens to the "orchestrator:event" channel from Rust and
 * translates events into frontend state changes (Zustand store updates,
 * TTS playback, sidebar display, etc).
 *
 * This REPLACES the scattered "assistant:server" event handling in
 * wsBridge.ts and recorder.ts. The central orchestrator in Rust now owns:
 *   - When to show/hide the loading indicator
 *   - When to speak the ack
 *   - When to speak the result
 *   - When to show the sidebar with the response
 *   - Request lifecycle (cancel, done, error)
 *
 * The frontend just reacts to orchestrator events — it no longer makes
 * independent decisions about loading state or ack timing.
 */

import { useAssistant } from "../store/assistant";
import { speak, stopTts } from "../audio/ttsPlayer";
import { useSidebar } from "../sidebar/sidebarStore";
import { clearLongRunningInFlight, isLocalAckGiven } from "./wsBridge";
import { invoke } from "@tauri-apps/api/core";

function isTauri(): boolean {
  return typeof (window as any).__TAURI_INTERNALS__ !== "undefined";
}

/** Orchestrator event shape (mirrors Rust OrchestratorEvent enum). */
interface OrchestratorEvent {
  type:
    | "state"
    | "loading"
    | "ack"
    | "result"
    | "done"
    | "error"
    | "confirm"
    | "conflict_report"
    | "github_result";
  request_id: string;
  // state
  state?: "idle" | "listening" | "thinking" | "speaking";
  // loading
  visible?: boolean;
  // ack
  text?: string;
  // result
  analysis?: unknown;
  dialog_state?: unknown;
  // error
  message?: string;
  // confirm (GitHub destructive operation)
  prompt?: string;
  command?: unknown; // Serialized GitHubCommand
  // conflict_report (GitHub merge conflict)
  pr_number?: number;
  repo?: string;
  conflict_files?: ConflictFile[];
  // github_result
  result?: GitHubResultPayload;
}

/** A file with merge conflicts (mirrors Rust ConflictFile). */
interface ConflictFile {
  filename: string;
  conflict_count: number;
  blocks: ConflictBlock[];
}

/** A single conflict block (mirrors Rust ConflictBlock). */
interface ConflictBlock {
  start_line: number;
  head_content: string;
  branch_content: string;
}

/** GitHub result payload (mirrors Rust GitHubResult enum). */
interface GitHubResultPayload {
  type: "text" | "needs_confirmation" | "merge_conflict" | "error";
  text?: string;
  prompt?: string;
  command?: unknown;
  pr_number?: number;
  repo?: string;
  conflict_files?: ConflictFile[];
  message?: string;
  status?: number;
  is_auth_error?: boolean;
}

let initialized = false;
let currentRequestId: string | null = null;
let confirmListeningTimer: ReturnType<typeof setTimeout> | null = null;

function clearConfirmListeningTimer(): void {
  if (confirmListeningTimer) {
    clearTimeout(confirmListeningTimer);
    confirmListeningTimer = null;
  }
}

/** Open the 5-second voice approval listening window for pending confirmations. */
export function openConfirmVoiceWindow(): void {
  const curStore = useAssistant.getState();
  if (curStore.pendingGithubCommand) {
    console.log("[NEXUS] orchestrator: prompt spoken — opening 5s voice approval window");
    curStore.setState("listening");
    import("@tauri-apps/api/core")
      .then(({ invoke }) => invoke("start_stt_capture"))
      .catch(() => {});

    clearConfirmListeningTimer();
    confirmListeningTimer = setTimeout(() => {
      confirmListeningTimer = null;
      const latest = useAssistant.getState();
      if (latest.state === "listening" && latest.pendingGithubCommand) {
        console.log("[NEXUS] orchestrator: 5s voice window timed out — mic back to idle, sidebar remains interactive");
        latest.setState("idle");
      }
    }, 5000);
  }
}

/** Current request ID (for debugging / diagnostics). */
export function getCurrentRequestId(): string | null {
  return currentRequestId;
}

/** Test hook: set the in-flight request id (vitest only). */
export function __testSetCurrentRequestId(id: string | null): void {
  currentRequestId = id;
}

/**
 * Complete a spoken result turn. Called when result TTS finishes (or fails).
 * Returns true when this turn was still current and the orb was reset;
 * false when a barge-in already moved on (stale onEnd must never touch the
 * new turn's state — the cancel flow owns that path).
 */
export function finishSpokenResult(spokenFor: string): boolean {
  if (currentRequestId !== spokenFor) return false;
  currentRequestId = null;
  clearLongRunningInFlight();
  clearConfirmListeningTimer();
  const store = useAssistant.getState();
  store.setLoadingVisible(false);
  store.setVisible(true); // brief beat, mirrors the `done` handler
  setTimeout(() => {
    if (currentRequestId === null) useAssistant.getState().reset();
  }, 550);
  void signalOrchestratorDone(spokenFor);
  return true;
}

/**
 * Initialize the orchestrator event listener.
 * Call this once at app startup (from App.tsx or main.tsx).
 *
 * This listens to the "orchestrator:event" channel and dispatches to:
 *   - useAssistant store (state, visible, loadingVisible, transcript)
 *   - TTS player (speak ack, speak result, stop on cancel)
 *   - Sidebar display (show result)
 */
export async function initOrchestratorListener(): Promise<void> {
  if (initialized || !isTauri()) return;
  initialized = true;

  const { listen } = await import("@tauri-apps/api/event");

  console.log("[NEXUS] orchestrator: initializing event listener");

  await listen<OrchestratorEvent>("orchestrator:event", async (event) => {
    const ev = event.payload;
    const store = useAssistant.getState();

    console.log(`[NEXUS] orchestrator: ${ev.type} (req=${ev.request_id})`, ev);

    switch (ev.type) {
      case "state": {
        if (ev.state) {
          store.setState(ev.state as any);
        }
        break;
      }

      case "loading": {
        // The Rust side already shows/hides the loading window directly.
        // We just update the store for UI consistency (e.g. if the frontend
        // needs to know the loading state for rendering decisions).
        if (ev.visible !== undefined) {
          store.setLoadingVisible(ev.visible);
          if (ev.visible) {
            // Hide the orb shortly after the loading indicator appears.
            // This keeps the orb visible while "On it sir" is playing,
            // then transitions to the loading indicator once it's ready.
            setTimeout(() => useAssistant.getState().setVisible(false), 600);
          }
        }
        break;
      }

      case "ack": {
        // Speak the acknowledgement ("On it sir")
        // Skip if we've already given a local ack (ackLongRunningQuery in
        // recorder.ts) to avoid double-speak ("On it sir" said twice).
        if (isLocalAckGiven()) {
          console.log("[NEXUS] orchestrator ack suppressed — local ack already given");
          // Still hide the orb after TTS finishes — the loading indicator
          // will take over. This is a fallback in case the loading event
          // hasn't arrived yet.
          setTimeout(() => useAssistant.getState().setVisible(false), 1500);
          break;
        }
        if (ev.text) {
          store.setState("speaking");
          store.addAssistantMessage(ev.text);
          void speak(ev.text);
          // Hide the orb after a short delay (TTS is playing the ack).
          // The loading indicator is already shown by Rust.
          setTimeout(() => {
            useAssistant.getState().setVisible(false);
          }, 1500);
        }
        break;
      }

      case "result": {
        // Final result from the subsystem
        currentRequestId = ev.request_id;

        // Clear the long-running in-flight flag so subsequent voice
        // commands aren't incorrectly deduped/queued.
        clearLongRunningInFlight();

        // Hide loading (Rust already does this, but update store too)
        store.setLoadingVisible(false);

        // Show the orb again for speaking the result
        store.setVisible(true);
        store.setState("speaking");
        store.setAwaitingInput(false);

        // Add to transcript
        if (ev.text) {
          store.addAssistantMessage(ev.text);
        }

        // Speak the result, then close the handshake: the backend
        // withholds `done` on success (emitting it would cancel TTS), so
        // the frontend must signal completion itself. Without this the orb
        // parks in `speaking` forever after long replies.
        // Guarded by request id: a barged-in turn must never reset the new
        // turn's state (barge-in abort skips onEnd; the cancel flow owns it).
        if (ev.text) {
          const spokenFor = ev.request_id;
          speak(ev.text, () => {
            finishSpokenResult(spokenFor);
          }).catch((err) => {
            console.warn("[NEXUS] orchestrator: result TTS failed:", err);
            finishSpokenResult(spokenFor);
          });
        }

        // If there's analysis data, we could show it in the sidebar
        // (the existing sidebar logic handles this via the old channel)
        if (ev.analysis) {
          console.log("[NEXUS] orchestrator: result has analysis data", ev.analysis);
        }
        if (ev.dialog_state) {
          console.log("[NEXUS] orchestrator: result has dialog state", ev.dialog_state);
        }
        break;
      }

      case "done": {
        // Request is fully complete (TTS finished speaking)
        currentRequestId = null;
        clearLongRunningInFlight();
        store.setLoadingVisible(false);
        store.setAwaitingInput(false);
        const curStore = useAssistant.getState();
        if (curStore.pendingGithubCommand) {
          console.log("[NEXUS] orchestrator: done event with pending confirmation — opening voice approval window");
          openConfirmVoiceWindow();
        } else {
          store.setVisible(true); // Show orb briefly before reset
          setTimeout(() => store.reset(), 550);
        }
        break;
      }

      case "error": {
        console.error("[NEXUS] orchestrator: error:", ev.message);
        clearLongRunningInFlight();
        store.setLoadingVisible(false);
        store.setVisible(true);
        store.setState("speaking");
        store.setAwaitingInput(false);
        const errMsg = ev.message || "Something went wrong sir.";
        store.addAssistantMessage(`Error: ${errMsg}`);
        void speak(errMsg);
        // After speaking the error, reset
        setTimeout(() => {
          currentRequestId = null;
          setTimeout(() => store.reset(), 550);
        }, 3000);
        break;
      }

      case "confirm": {
        // Operation needs confirmation.
        // Store the pending command so when the user says "yes" / "approved" / "proceed",
        // processViaOrchestrator can re-invoke with confirmed=true.
        clearLongRunningInFlight();
        clearConfirmListeningTimer();
        store.setLoadingVisible(false);
        store.setVisible(true);
        store.setState("speaking");
        // The prompt speech ends but the turn stays open waiting for
        // "yes" — mark it so the orb holds + glows instead of looping
        // over silence (or going dead).
        store.setAwaitingInput(true);
        store.setPendingGithubCommand(ev.command ?? null);
        console.log("[NEXUS] orchestrator: confirm needed for command", ev.command);

        if (ev.prompt) {
          store.addAssistantMessage(ev.prompt);
          void speak(ev.prompt, () => {
            openConfirmVoiceWindow();
          }).catch(() => {
            openConfirmVoiceWindow();
          });
        }
        break;
      }

      case "conflict_report": {
        // GitHub merge conflict detected.
        // Speak the conflict summary and display the conflict panel
        // in the sidebar with copy-paste options.
        clearLongRunningInFlight();
        store.setLoadingVisible(false);
        store.setVisible(true);
        store.setState("speaking");

        const prNum = ev.pr_number ?? 0;
        const repo = ev.repo || "";
        const files = ev.conflict_files || [];
        const fileCount = files.length;

        const summary = ev.message || `PR #${prNum} in ${repo} has merge conflicts.`;
        const spoken = `${summary} ${fileCount} file${fileCount !== 1 ? "s" : ""} have conflicts. Please fix the conflicts and push, then try merging again.`;

        store.addAssistantMessage(spoken);
        void speak(spoken);

        console.log("[NEXUS] orchestrator: merge conflict", {
          pr_number: prNum,
          repo,
          files,
        });

        // Show the conflict panel in the sidebar with copy-paste options
        useSidebar.getState().showConflict({
          prNumber: prNum,
          repo,
          conflictFiles: files,
          message: summary,
        });

        break;
      }

      case "github_result": {
        // Raw GitHub result — used for structured UI display.
        // The text/conflict/error cases are already handled by the
        // result/conflict_report/error events above. This event provides
        // the raw structured data for advanced UI rendering.
        console.log("[NEXUS] orchestrator: github_result", ev.result);
        break;
      }
    }
  });

  console.log("[NEXUS] orchestrator: event listener ready");
}

/**
 * Process a transcript through the central orchestrator.
 *
 * This is the frontend entry point — call this after STT produces a transcript.
 * It invokes the Rust `orchestrator_process` command which:
 *   1. Parses intent (deterministic, <1ms)
 *   2. Routes to the correct subsystem
 *   3. Emits ack + loading events
 *   4. Dispatches to the subsystem
 *   5. Emits result + done
 *
 * The caller does NOT need to manage loading state, ack timing, or TTS —
 * the orchestrator handles all of that.
 */
export async function processViaOrchestrator(
  transcript: string,
  dialogContext?: unknown,
): Promise<{ request_id: string; subsystem: string; handled_locally: boolean } | null> {
  if (!isTauri()) return null;

  // ─── Confirmation flow ───
  // If there's a pending command awaiting confirmation, check if
  // the user said "yes"/"approved"/"proceed" (confirm) or "no"/"cancel" (abort).
  const store = useAssistant.getState();
  const pendingCmd = store.pendingGithubCommand;
  if (pendingCmd) {
    clearConfirmListeningTimer();
    const lower = transcript.trim().toLowerCase();
    const isYes = /^(yes|yeah|yep|yup|confirm|ok|okay|sure|go ahead|do it|proceed|approved?|agreed?|approve)\b/i.test(lower);
    const isNo = /^(no|nope|cancel|abort|stop|don't|dont|never|disapproved?|disapprove)\b/i.test(lower);

    if (isYes) {
      // Clear the pending command first, then re-execute with confirmed=true
      store.setPendingGithubCommand(null);
      useAssistant.getState().addUserMessage(transcript);

      // MCP confirmations carry kind:"mcp" + {server, tool, params} —
      // route them to orchestrator_mcp_confirm instead of github_execute.
      const isMcp = (pendingCmd as any)?.kind === "mcp";
      if (isMcp) {
        console.log("[NEXUS] orchestrator: confirming pending MCP call", pendingCmd);
        try {
          const result = await invoke<unknown>("orchestrator_mcp_confirm", {
            requestId: currentRequestId ?? "mcp-confirm",
            confirmed: true,
            pending: pendingCmd,
          });
          console.log("[NEXUS] orchestrator: mcp_confirm result", result);
          await invoke("hide_sidebar").catch(() => {});
          return {
            request_id: "mcp-confirmed",
            subsystem: "mcp",
            handled_locally: false,
          };
        } catch (err) {
          console.error("[NEXUS] orchestrator: mcp_confirm failed:", err);
          return null;
        }
      }

      console.log("[NEXUS] orchestrator: confirming pending GitHub command", pendingCmd);
      try {
        const result = await invoke<unknown>("orchestrator_github_execute", {
          command: pendingCmd,
          confirmed: true,
        });
        console.log("[NEXUS] orchestrator: github_execute confirmed result", result);
        await invoke("hide_sidebar").catch(() => {});
        // The result events are emitted by Rust on the orchestrator:event channel
        // and handled by the listener above.
        return {
          request_id: String((result as any)?.request_id ?? "github-confirmed"),
          subsystem: "github",
          handled_locally: false,
        };
      } catch (err) {
        console.error("[NEXUS] orchestrator: github_execute confirmed failed:", err);
        return null;
      }
    } else if (isNo) {
      // User declined — clear the pending command
      const wasMcp = (pendingCmd as any)?.kind === "mcp";
      store.setPendingGithubCommand(null);
      useAssistant.getState().addUserMessage(transcript);
      if (wasMcp) {
        try {
          await invoke<unknown>("orchestrator_mcp_confirm", {
            requestId: currentRequestId ?? "mcp-cancel",
            confirmed: false,
            pending: pendingCmd,
          });
        } catch (err) {
          console.warn("[NEXUS] orchestrator: mcp cancel failed:", err);
        }
      }
      await invoke("hide_sidebar").catch(() => {});
      const abortMsg = "Okay, I've cancelled that operation, sir.";
      useAssistant.getState().addAssistantMessage(abortMsg);
      void speak(abortMsg);
      setTimeout(() => useAssistant.getState().reset(), 2000);
      return {
        request_id: wasMcp ? "mcp-aborted" : "github-aborted",
        subsystem: wasMcp ? "mcp" : "github",
        handled_locally: true,
      };
    }
    // If it's neither yes nor no, fall through to normal processing
    // (the user may have said a completely different command)
    store.setPendingGithubCommand(null);
    void invoke("hide_sidebar").catch(() => {});
  }

  try {
    const result = await invoke<{
      request_id: string;
      subsystem: string;
      handled_locally: boolean;
    }>("orchestrator_process", {
      transcript,
      dialogContext: dialogContext ?? null,
    });

    currentRequestId = result.request_id;
    console.log("[NEXUS] orchestrator: process result", result);
    return result;
  } catch (err) {
    console.error("[NEXUS] orchestrator: process failed:", err);
    return null;
  }
}

/** Cancel the active orchestrator request (barge-in / new wake). */
export async function cancelOrchestrator(): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke("orchestrator_cancel");
    stopTts();
    currentRequestId = null;
  } catch (err) {
    console.warn("[NEXUS] orchestrator: cancel failed:", err);
  }
}

/** Signal that a request is done (called after TTS finishes). */
export async function signalOrchestratorDone(requestId: string): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke("orchestrator_done", { requestId });
  } catch (err) {
    console.warn("[NEXUS] orchestrator: done signal failed:", err);
  }
}
