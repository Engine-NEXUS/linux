import { create } from "zustand";
import {
  LOADING_HIDDEN,
  LoadingSnapshot,
  loadingHide,
  loadingSettle,
  loadingShow,
} from "./loadingMachine";

export type AssistantState = "idle" | "listening" | "thinking" | "speaking";

export interface TranscriptEntry {
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
  /** True while a ghost cursor session is live (mic hot, waves on). */
  ghostActive: boolean;
  setGhostActive: (v: boolean) => void;
  /** Animation-calibration preview target (null = not calibrating).
   *  When set, the orb window shows the selected target and accepts
   *  native drag + wheel-resize (task: drag & scroll calibration). */
  calibrationTarget: "wakeup" | "waves" | "loading" | null;
  /** Live calibration size badge value (px) — mirrored from the Rust
   *  `calibration:state` event so the pill under the element stays true. */
  calibrationSize: number | null;
  setCalibration: (
    target: "wakeup" | "waves" | "loading" | null,
    size: number | null
  ) => void;
  /** Target-switch pulse counter (calibration preview readability): bumped
   *  whenever the active target changes so the desktop preview plays a
   *  220ms highlight even when both rects are identical. Render-only. */
  calibrationPulse: number;
  bumpCalibrationPulse: () => void;
  /** Index of the TTS chunk currently playing (for avatar mouth animation). */
  speakSeq: number | null;
  /** Current microphone audio volume (RMS, 0.0 - ~1.0) for avatar reactivity. */
  audioVolume: number;
  /** Live mic level 0..1 from the Rust capture loop (`audio:level`,
   *  ~6 Hz during STT capture + final 0 on stop). Drives the
   *  speech-synced ghost waves. Unlike `audioVolume` (browser-VAD
   *  path, dormant under Rust capture) this is live in every mode
   *  that uses cpal capture. Reset to 0 by reset(). */
  micLevel: number;
  setMicLevel: (v: number) => void;
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
  /** When the pending confirmation was set (ms epoch): voice answers expire after PENDING_CONFIRM_TTL_MS. */
  pendingGithubCommandAt: number | null;
  setPendingGithubCommand: (cmd: unknown | null) => void;
  /** User-configured orb color theme (hex string, e.g. "#f2b859"). */
  orbColor: string;
  setOrbColor: (color: string) => void;
  /** Orb fixed position: "top" or "bottom". Default: "top". */
  orbPosition: "top" | "bottom";
  setOrbPosition: (pos: "top" | "bottom") => void;
  /** Orb background capsule style: "classic" or "pebble". Default: "classic". */
  orbCapsuleStyle: "classic" | "pebble";
  setOrbCapsuleStyle: (style: "classic" | "pebble") => void;
  /** Orb wakeup thread animation mode (1..5). Default: 1 (Elastic thread). */
  orbWakeupMode: number;
  setOrbWakeupMode: (m: number) => void;
  /** Orb interior genesis mode (0..3). Default: 2 (Spark burst). */
  orbInteriorGenesis: number;
  setOrbInteriorGenesis: (g: number) => void;
  /** True while speech captions are actively rendering/lingering on screen. */
  captionActive: boolean;
  setCaptionActive: (v: boolean) => void;
  /** Orb 3D morphology modes: thinking (1..12), speaking (1..6), listening (1..6). */
  orbThinkMode: number;
  setOrbThinkMode: (m: number) => void;
  orbSpeakMode: number;
  setOrbSpeakMode: (m: number) => void;
  orbListenMode: number;
  setOrbListenMode: (m: number) => void;
}

/** Single owner for the loading indicator (see loadingMachine.ts).
 * All ~20 writers funnel through here: minimum 800ms dwell (no flashes),
 * 120s failsafe (a lost hide can never wedge the spinner), transition log. */
let loadingSnap: LoadingSnapshot = LOADING_HIDDEN;
let loadingTimer: ReturnType<typeof setTimeout> | null = null;

/** Guaranteed 1.0s minimum dwell for thinking state (1000ms).
 * Ensures the 3D continuous woven violet ribbon knot is fully visible
 * and completes its opening expansion and rotation before morphing into speech. */
export const THINKING_MIN_DWELL_MS = 1000;
let thinkingEnteredAt: number | null = null;
let thinkingPendingTimer: ReturnType<typeof setTimeout> | null = null;

export function clearThinkingDwell(): void {
  if (thinkingPendingTimer) {
    clearTimeout(thinkingPendingTimer);
    thinkingPendingTimer = null;
  }
  thinkingEnteredAt = null;
}

export const useAssistant = create<AssistantStore>((set) => ({
  state: "idle",
  visible: false,
  loadingVisible: false,
  transcript: [],
  speakSeq: null,
  audioVolume: 0,
  micLevel: 0,
  ttsActive: false,
  awaitingInput: false,
  ghostActive: false,
  calibrationTarget: null,
  calibrationPulse: 0,
  calibrationSize: null,
  setState: (s) => {
    return set((st) => {
      const now = performance.now();
      if (s === "thinking") {
        thinkingEnteredAt = now;
        if (thinkingPendingTimer) {
          clearTimeout(thinkingPendingTimer);
          thinkingPendingTimer = null;
        }
        console.log(`[ORB] state ${st.state} → thinking (1.0s dwell armed)`);
        return { state: "thinking" };
      }

      // If currently thinking and requesting a state change (e.g. to speaking or idle), enforce 1.0s minimum
      if (st.state === "thinking" && thinkingEnteredAt !== null) {
        const elapsed = now - thinkingEnteredAt;
        const remaining = THINKING_MIN_DWELL_MS - elapsed;
        if (remaining > 0) {
          console.log(`[ORB] state thinking holding for remaining ${Math.round(remaining)}ms (dwell floor 1000ms)`);
          if (thinkingPendingTimer) clearTimeout(thinkingPendingTimer);
          thinkingPendingTimer = setTimeout(() => {
            thinkingPendingTimer = null;
            thinkingEnteredAt = null;
            useAssistant.setState({ state: s });
          }, remaining);
          return st; // Hold current thinking state!
        }
      }

      if (thinkingPendingTimer) {
        clearTimeout(thinkingPendingTimer);
        thinkingPendingTimer = null;
      }
      thinkingEnteredAt = null;
      console.log(`[ORB] state ${st.state} → ${s}`);
      return { state: s };
    });
  },
  setVisible: (v) => set((st) => {
      return { visible: st.ghostActive ? true : v };
    }),
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
  setMicLevel: (v) => set({ micLevel: Math.max(0, Math.min(1, v)) }),
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
  setGhostActive: (v) => set((st) => {
      console.log(`[ORB] setGhostActive(${v}) from:${st.ghostActive} visible:${st.visible} state:${st.state}`);
      return { ghostActive: v, visible: v ? true : st.visible };
    }),
  setCalibration: (target, size) =>
    set({ calibrationTarget: target, calibrationSize: size }),
  bumpCalibrationPulse: () => set((st) => ({ calibrationPulse: st.calibrationPulse + 1 })),
  reset: () => {
    clearThinkingDwell();
    return set((st) => ({
      state: "idle",
      speakSeq: null,
      audioVolume: 0,
      micLevel: 0,
      ttsActive: false,
      awaitingInput: false,
      visible: st.ghostActive ? true : st.visible,
    }));
  },
  clearTranscript: () => set({ transcript: [] }),
  pendingGithubCommand: null,
  pendingGithubCommandAt: null,
  setPendingGithubCommand: (cmd) =>
    set({ pendingGithubCommand: cmd, pendingGithubCommandAt: cmd ? Date.now() : null }),
  orbColor: typeof localStorage !== "undefined" ? (localStorage.getItem("nexus:orb_color") || "#ffffff") : "#ffffff",
  setOrbColor: (c) => {
    try { localStorage.setItem("nexus:orb_color", c); } catch (_) {}
    set({ orbColor: c });
  },
  orbPosition: typeof localStorage !== "undefined" ? ((localStorage.getItem("nexus:orb_position") as "top" | "bottom") || "top") : "top",
  setOrbPosition: (p) => {
    try { localStorage.setItem("nexus:orb_position", p); } catch (_) {}
    set({ orbPosition: p });
  },
  orbCapsuleStyle: typeof localStorage !== "undefined" ? ((localStorage.getItem("nexus:orb_capsule_style") as "classic" | "pebble") || "pebble") : "pebble",
  setOrbCapsuleStyle: (s) => {
    try { localStorage.setItem("nexus:orb_capsule_style", s); } catch (_) {}
    set({ orbCapsuleStyle: s });
  },
  orbWakeupMode: typeof localStorage !== "undefined" ? Number(localStorage.getItem("nexus:orb_wakeup_mode") || "1") : 1,
  setOrbWakeupMode: (m) => {
    try { localStorage.setItem("nexus:orb_wakeup_mode", String(m)); } catch (_) {}
    set({ orbWakeupMode: m });
  },
  orbInteriorGenesis: typeof localStorage !== "undefined" ? Number(localStorage.getItem("nexus:orb_interior_genesis") || "2") : 2,
  setOrbInteriorGenesis: (g) => {
    try { localStorage.setItem("nexus:orb_interior_genesis", String(g)); } catch (_) {}
    set({ orbInteriorGenesis: g });
  },
  captionActive: false,
  setCaptionActive: (v) => set((st) => {
    console.log(`[ORB] setCaptionActive(${v}) (was:${st.captionActive})`);
    return { captionActive: v };
  }),
  orbThinkMode: 6,
  setOrbThinkMode: (m) => set({ orbThinkMode: m }),
  orbSpeakMode: 2,
  setOrbSpeakMode: (m) => set({ orbSpeakMode: m }),
  orbListenMode: 1,
  setOrbListenMode: (m) => set({ orbListenMode: m }),
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
