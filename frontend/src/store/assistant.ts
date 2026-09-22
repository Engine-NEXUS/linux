import { create } from "zustand";
import {
  LOADING_HIDDEN,
  LoadingSnapshot,
  loadingHide,
  loadingSettle,
  loadingShow,
} from "./loadingMachine";

export type AssistantState = "idle" | "listening" | "thinking" | "speaking";

interface TranscriptEntry {
  role: "user" | "assistant";
  text: string;
  timestamp: number;
}

interface AssistantStore {
  state: AssistantState;
  visible: boolean;
  /** Whether the loading animation overlay (top-right corner) is showing.
   *  Set to true when "On it sir" is spoken (command validated as long-running).
   *  Set to false when the result arrives or the orb re-shows. */
  loadingVisible: boolean;
  /** Conversation transcript for display in the sidebar. */
  transcript: TranscriptEntry[];
  /** True while TTS audio is actually playing (event-derived, see
   *  audio/ttsActivity.ts). The orb animation gates on this — not on
   *  `state` — so the loading loop never runs over silence. */
  ttsActive: boolean;
  setTtsActive: (v: boolean) => void;
  /** True while the orb is waiting on the user with no audio (e.g. a
   *  confirm prompt after its speech ends). Renders the hold-frame +
   *  steady "waiting" glow instead of a frozen loop or a dead orb. */
  awaitingInput: boolean;
  setAwaitingInput: (v: boolean) => void;
  /** Index of the TTS chunk currently playing (for avatar mouth animation). */
  speakSeq: number | null;
  /** Current microphone audio volume (RMS, 0.0 - ~1.0) for avatar reactivity. */
  audioVolume: number;
  setState: (s: AssistantState) => void;
  setVisible: (v: boolean) => void;
  setLoadingVisible: (v: boolean) => void;
  setAudioVolume: (v: number) => void;
  addUserMessage: (text: string) => void;
  addAssistantMessage: (text: string) => void;
  setSpeakSeq: (n: number | null) => void;
  /** Reset to idle and hide after a short delay (driven by an effect in App). */
  reset: () => void;
  /** Clear the transcript. */
  clearTranscript: () => void;
  /** Pending GitHub command awaiting user confirmation (destructive ops). */
  pendingGithubCommand: unknown | null;
  setPendingGithubCommand: (cmd: unknown | null) => void;
}

/** Single owner for the loading indicator (see loadingMachine.ts).
 * All ~20 writers funnel through here: minimum 800ms dwell (no flashes),
 * 120s failsafe (a lost hide can never wedge the spinner), transition log. */
let loadingSnap: LoadingSnapshot = LOADING_HIDDEN;
let loadingTimer: ReturnType<typeof setTimeout> | null = null;

export const useAssistant = create<AssistantStore>((set) => ({
  state: "idle",
  visible: false,
  loadingVisible: false,
  transcript: [],
  speakSeq: null,
  audioVolume: 0,
  ttsActive: false,
  awaitingInput: false,
  setState: (s) => set({ state: s }),
  setVisible: (v) => set({ visible: v }),
  setLoadingVisible: (v) => {
    const now = Date.now();
    const prev = loadingSnap;
    const next = v ? loadingShow(prev, now) : loadingHide(prev, now);
    if (next === prev) {
      return; // no-op (hide-when-hidden)
    }
    loadingSnap = next;
    if (loadingTimer) {
      clearTimeout(loadingTimer);
      loadingTimer = null;
    }
    if (next.pendingAt !== null) {
      const delay = Math.max(0, next.pendingAt - Date.now());
      loadingTimer = setTimeout(() => {
        loadingTimer = null;
        loadingSnap = loadingSettle(loadingSnap, Date.now());
        set({ loadingVisible: loadingSnap.visible });
        console.debug(
          `[NEXUS] loading: settled → ${loadingSnap.visible ? "shown" : "hidden"}`
        );
      }, delay);
    }
    set({ loadingVisible: next.visible });
    console.debug(
      `[NEXUS] loading: ${prev.visible} → ${next.visible} (req=${v})`
    );
  },
  setAudioVolume: (v) => set({ audioVolume: v }),
  addUserMessage: (text) =>
    set((st) => ({
      transcript: [...st.transcript, { role: "user", text, timestamp: Date.now() }],
    })),
  addAssistantMessage: (text) =>
    set((st) => ({
      transcript: [...st.transcript, { role: "assistant", text, timestamp: Date.now() }],
    })),
  setSpeakSeq: (n) => set({ speakSeq: n }),
  setTtsActive: (v) => set({ ttsActive: v }),
  setAwaitingInput: (v) => set({ awaitingInput: v }),
  reset: () => set({ state: "idle", speakSeq: null, audioVolume: 0, ttsActive: false, awaitingInput: false }),
  clearTranscript: () => set({ transcript: [] }),
  pendingGithubCommand: null,
  setPendingGithubCommand: (cmd) => set({ pendingGithubCommand: cmd }),
}));

/**
 * Canonical state-machine transitions. Enforced everywhere we call setState.
 *   idle  -> listening   (wake / hotkey)
 *   listening -> thinking (VAD silence + local STT + transcript sent)
 *   thinking -> speaking (ack or result event — local TTS speaks)
 *   speaking -> idle      (done event)
 */
export function transition(from: AssistantState, to: AssistantState): boolean {
  const allowed: Record<AssistantState, AssistantState[]> = {
    idle: ["listening"],
    listening: ["thinking", "idle"],
    thinking: ["speaking", "idle"],
    speaking: ["idle"],
  };
  return allowed[from]?.includes(to) ?? false;
}
