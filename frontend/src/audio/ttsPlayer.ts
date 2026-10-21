import { useAssistant } from "../store/assistant";
import { invoke } from "@tauri-apps/api/core";

/**
 * Frontend TTS generation counter — mirrors the Rust TTS_GENERATION counter.
 *
 * Every `stopTts()` increments this. Every `speak()` captures the current
 * value and checks it before starting playback and before firing onEnd.
 * This prevents stale Worker responses from speaking after a barge-in.
 */
let ttsGeneration = 0;

/**
 * Tracks whether Rust/rodio TTS is currently playing audio.
 * This is set to true when speak_text is invoked and cleared when
 * the invoke resolves (playback complete) or stopTts is called.
 *
 * CRITICAL: waitForTtsIdle() checks this flag — NOT speechSynthesis.speaking —
 * because our TTS plays through Rust/rodio, not the Web Speech API.
 * The old code only checked speechSynthesis.speaking, which was always false
 * for Rust TTS, causing waitForTtsIdle() to return immediately while audio
 * was still playing. This created an echo feedback loop where TTS audio was
 * captured by the mic before playback finished.
 */
let rustTtsPlaying = false;

/**
 * @returns true if Rust/rodio TTS is currently playing audio.
 */
export function isRustTtsPlaying(): boolean {
  return rustTtsPlaying;
}

/**
 * The narrated screen tour plays audio from Rust directly (`tts::narrate`),
 * not through `speak()`, so the frontend never sees a speak_text start/end.
 * The tour handlers flag it here so the echo guards, ghost hot-mic and the
 * stuck-speaking watchdog treat narration as live playback.
 */
export function setNarrationPlaying(v: boolean): void {
  rustTtsPlaying = v;
}

export interface VoicePersona {
  key: string;
  name: string;
  persona: string;
  accentTag: string;
  avatar: string;
  cloudId: string;
  kokoroVoice: string;
  previewPhrase: string;
}

export const ICONIC_PERSONAS: VoicePersona[] = [
  {
    key: "jarvis",
    name: "JARVIS",
    persona: "Calm, intelligent, British baritone with witty refinement",
    accentTag: "UK · Male",
    avatar: "🤖",
    cloudId: "en-GB-RyanNeural",
    kokoroVoice: "bm_george",
    previewPhrase: "At your service, sir. All systems operational.",
  },
  {
    key: "friday",
    name: "FRIDAY",
    persona: "Sharp tactical companion, fast and precise",
    accentTag: "Ireland · Female",
    avatar: "🛡️",
    cloudId: "en-IE-EmilyNeural",
    kokoroVoice: "bf_emma",
    previewPhrase: "Boss, tactical links and neural feeds are live.",
  },
  {
    key: "nexus",
    name: "NEXUS",
    persona: "Expressive flagship assistant, warm and clear",
    accentTag: "US · Female",
    avatar: "🌟",
    cloudId: "en-US-AvaNeural",
    kokoroVoice: "af_heart",
    previewPhrase: "Hello, I'm NEXUS. What are we building today?",
  },
  {
    key: "siri",
    name: "SIRI",
    persona: "Modern, crisp assistant delivery",
    accentTag: "US · Female",
    avatar: "📱",
    cloudId: "en-US-JennyNeural",
    kokoroVoice: "af_bella",
    previewPhrase: "Here is what I found for you.",
  },
  {
    key: "alexa",
    name: "ALEXA",
    persona: "Confident smart-home lead, steady and direct",
    accentTag: "US · Female",
    avatar: "🔵",
    cloudId: "en-US-AriaNeural",
    kokoroVoice: "af_nicole",
    previewPhrase: "Ready. Standing by for your instructions.",
  },
  {
    key: "google",
    name: "GOOGLE",
    persona: "Analytical tech specialist, measured and exact",
    accentTag: "US · Male",
    avatar: "🎙️",
    cloudId: "en-US-BrianNeural",
    kokoroVoice: "am_michael",
    previewPhrase: "Good day. Let me know what you need analyzed.",
  },
  {
    key: "cortana",
    name: "CORTANA",
    persona: "Heroic sci-fi companion, loyal and bold",
    accentTag: "US · Female",
    avatar: "💠",
    cloudId: "en-US-MichelleNeural",
    kokoroVoice: "af_sarah",
    previewPhrase: "Chief, telemetry is locked. I'm with you.",
  },
  {
    key: "samantha",
    name: "SAMANTHA",
    persona: "Warm, intimate conversationalist",
    accentTag: "US · Female",
    avatar: "💫",
    cloudId: "en-US-SaraNeural",
    kokoroVoice: "af_sky",
    previewPhrase: "I'm here. It's really good to hear your voice.",
  },
  {
    key: "alfred",
    name: "ALFRED",
    persona: "Master butler, formal and reassuring",
    accentTag: "UK · Male",
    avatar: "🎩",
    cloudId: "en-GB-OliverNeural",
    kokoroVoice: "bm_fable",
    previewPhrase: "Very good, sir. I have prepared your workspace.",
  },
  {
    key: "offline_safe",
    name: "EMERGENCY",
    persona: "Offline-safe fallback, always available",
    accentTag: "US · Female",
    avatar: "🆘",
    cloudId: "en-US-AvaNeural",
    kokoroVoice: "af_heart",
    previewPhrase: "Local speech synthesizer operational.",
  },
];

export interface VoiceOption {
  id: string;
  name: string;
  provider: "edge" | "kokoro" | "system";
  accent: string;
  description: string;
  locale: string;
  gender: "male" | "female";
  sampleText: string;
}

export const CURATED_VOICES: VoiceOption[] = [
  {
    id: "en-GB-RyanNeural",
    name: "JARVIS",
    provider: "edge",
    accent: "British Male",
    description: "Calm, intelligent British baritone with witty refinement.",
    locale: "en-GB",
    gender: "male",
    sampleText: "At your service, sir. All systems operational.",
  },
  {
    id: "en-IE-EmilyNeural",
    name: "FRIDAY",
    provider: "edge",
    accent: "Irish Female",
    description: "Sharp tactical companion, fast and precise.",
    locale: "en-IE",
    gender: "female",
    sampleText: "Boss, tactical links and neural feeds are live.",
  },
  {
    id: "en-US-AvaNeural",
    name: "NEXUS",
    provider: "edge",
    accent: "American Female",
    description: "Expressive flagship assistant, warm and clear.",
    locale: "en-US",
    gender: "female",
    sampleText: "Hello, I'm NEXUS. What are we building today?",
  },
  {
    id: "en-US-JennyNeural",
    name: "SIRI",
    provider: "edge",
    accent: "American Female",
    description: "Modern, crisp assistant delivery.",
    locale: "en-US",
    gender: "female",
    sampleText: "Here is what I found for you.",
  },
  {
    id: "en-US-AriaNeural",
    name: "ALEXA",
    provider: "edge",
    accent: "American Female",
    description: "Confident smart-home lead, steady and direct.",
    locale: "en-US",
    gender: "female",
    sampleText: "Ready. Standing by for your instructions.",
  },
  {
    id: "en-US-BrianNeural",
    name: "GOOGLE",
    provider: "edge",
    accent: "American Male",
    description: "Analytical tech specialist, measured and exact.",
    locale: "en-US",
    gender: "male",
    sampleText: "Good day. Let me know what you need analyzed.",
  },
  {
    id: "en-US-MichelleNeural",
    name: "CORTANA",
    provider: "edge",
    accent: "American Female",
    description: "Heroic sci-fi companion, loyal and bold.",
    locale: "en-US",
    gender: "female",
    sampleText: "Chief, telemetry is locked. I'm with you.",
  },
  {
    id: "en-US-SaraNeural",
    name: "SAMANTHA",
    provider: "edge",
    accent: "American Female",
    description: "Warm, intimate conversationalist.",
    locale: "en-US",
    gender: "female",
    sampleText: "I'm here. It's really good to hear your voice.",
  },
  {
    id: "en-GB-OliverNeural",
    name: "ALFRED",
    provider: "edge",
    accent: "British Male",
    description: "Master butler, formal and reassuring.",
    locale: "en-GB",
    gender: "male",
    sampleText: "Very good, sir. I have prepared your workspace.",
  },
  {
    id: "en-US-GuyNeural",
    name: "GUY",
    provider: "edge",
    accent: "American Male",
    description: "Deep, clear male voice. Cloud-powered, free, 0 MB RAM.",
    locale: "en-US",
    gender: "male",
    sampleText: "Hello, I am Guy. All systems are operational.",
  },
];

async function emitTtsEvent(event: string): Promise<void> {
  try {
    const { emit } = await import("@tauri-apps/api/event");
    await emit(event);
  } catch {
    // Ignore outside Tauri
  }
}

async function isMeetingActive(): Promise<boolean> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<boolean>("meeting_active");
  } catch {
    return false;
  }
}

async function getSavedSettings(): Promise<any> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke("get_settings");
  } catch {
    return null;
  }
}

export async function playKokoro(
  text: string,
  voiceId: string,
  speed: number,
  myGen: number,
  onEnd?: () => void,
): Promise<void> {
  // Check if barge-in happened before we even start
  if (ttsGeneration !== myGen) {
    console.log("[TTS] skipped — barge-in before playback");
    return;
  }

  void emitTtsEvent("tts-started");
  rustTtsPlaying = true;  // Track that Rust TTS is playing
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    // speak_text handles its own thread for rodio playback
    // This await resolves when rodio playback completes
    await invoke("speak_text", { text, voice: voiceId, speed });
  } catch (err) {
    // No browser-voice fallback: a Web Speech voice is a different speaker mid-conversation, and
    // Rust already falls back cloud -> local (one voice per utterance). Report and stay quiet.
    if (ttsGeneration === myGen) {
      console.error("[TTS] speak_text failed (no fallback voice by design):", err);
    }
  } finally {
    // Only the CURRENT utterance may clear the playing flag. A superseded call (the one-moment ack
    // that a newer reply stopped) resolves a beat later; before this guard its `finally` cleared the
    // flag while the new reply was still playing, so the stuck-speaking watchdog thought the audio was
    // dead and closed the turn ~10 s into an 18 s answer (live log 2026-10-17 02:20). stopTts() has
    // already cleared the flag and emitted tts-ended for a superseded call.
    if (ttsGeneration === myGen) {
      rustTtsPlaying = false;  // Playback complete
      void emitTtsEvent("tts-ended");
      onEnd?.();
    }
  }
}

export async function previewVoice(
  voice: VoiceOption,
  _customApiKey?: string,
  onEnd?: () => void,
  speed?: number,
): Promise<void> {
  stopTts();
  // All voices now go through the Rust speak_text command which tries
  // Edge TTS (cloud) first, then the local Kokoro voice (offline only).
  // The voice.id should be a valid Edge TTS voice (e.g. "en-US-AvaNeural").
  return playKokoro(voice.sampleText, voice.id, speed ?? 1.15, ttsGeneration, onEnd);
}

export async function speak(text: string, onEnd?: () => void): Promise<void> {
  const meeting = await isMeetingActive();
  if (meeting) {
    console.log("[TTS] Suppressed — meeting mode active");
    onEnd?.();
    return;
  }

  // Every caller of speak()/speakCached() is expected to show the orb and
  // mark it speaking before invoking this — but that was convention across
  // 30+ call sites, not enforced anywhere. Enforce it once, here, at the
  // single choke point every sound-producing invoke actually passes
  // through, so a future (or past-unaudited) caller can never let sound
  // play with the orb hidden.
  const assistant = useAssistant.getState();
  assistant.setVisible(true);
  assistant.setState("speaking");

  // Stop any currently-playing TTS before starting new playback.
  // This prevents overlapping audio when the server ack and result
  // arrive in quick succession (especially during first-load when
  // the Kokoro engine takes ~7s to initialize).
  stopTts();

  // Capture generation after stopTts — any in-flight speak() calls
  // from a previous turn will see the mismatch and skip playback.
  const myGen = ttsGeneration;

  const settings = await getSavedSettings();
  // Check if barge-in happened during the async getSavedSettings call
  if (ttsGeneration !== myGen) {
    console.log("[TTS] skipped — barge-in during setup");
    return;
  }

  // Use Edge TTS voice (cloud) — this is the primary engine.
  // The old default "af_sky" was a Kokoro voice ID that Edge TTS rejects,
  // causing every speak() to silently fall back to the local voice.
  const voiceId = settings?.edgeTtsVoice || "en-US-AvaNeural";
  const speed = settings?.speechRate ?? 1.15;

  // ONE invoke per utterance. Rust's speak_text already streams long replies sentence by sentence
  // (first audio after one sentence) under a single volume lease and a single voice decision.
  // Splitting here made each sentence its own lease (volume dipped to the original level between
  // sentences) and its own engine decision (a failed sentence changed the voice).
  return playKokoro(text, voiceId, speed, myGen, onEnd);
}

/**
 * Speak a pre-cached TTS phrase instantly from memory.
 * Falls back to `speak` if the phrase is not cached.
 * Emits `tts-started` (heard by Rust) before playback starts.
 */
export async function speakCached(phrase: string, onEnd?: () => void): Promise<void> {
  const meeting = await isMeetingActive();
  if (meeting) {
    console.log("[TTS] Suppressed — meeting mode active");
    onEnd?.();
    return;
  }

  // Same enforcement as speak() — see its comment. speakCached() is the
  // other (and only other) function that ever invokes a Rust TTS command.
  const assistant = useAssistant.getState();
  assistant.setVisible(true);
  assistant.setState("speaking");

  // Stop any currently-playing TTS before starting new playback.
  stopTts();

  const myGen = ttsGeneration;
  rustTtsPlaying = true;
  void emitTtsEvent("tts-started");
  try {
    await invoke("speak_cached", { text: phrase });
    if (ttsGeneration !== myGen) return;
    onEnd?.();
  } catch (e) {
    // A superseded call must neither clear the newer speech's flag nor speak its stale phrase.
    if (ttsGeneration !== myGen) return;
    // Fallback to regular speak if cached phrase not available
    console.warn("[TTS] speak_cached failed, falling back to speak:", e);
    rustTtsPlaying = false;
    void emitTtsEvent("tts-ended");
    return speak(phrase, onEnd);
  } finally {
    if (ttsGeneration === myGen) {
      rustTtsPlaying = false;
      void emitTtsEvent("tts-ended");
    }
  }
}

export function stopTts(): void {
  // Increment frontend generation — any in-flight speak() calls will
  // see the mismatch and skip playback / onEnd.
  ttsGeneration++;
  rustTtsPlaying = false;  // Clear playing flag immediately on barge-in
  // Tell Rust to stop the rodio playback immediately (barge-in).
  // Uses static import for instant invocation — no dynamic import delay.
  void invoke("stop_tts").catch((e: unknown) => console.warn("[TTS] stop_tts failed:", e));
  // Also cancel Web Speech API if it's being used as fallback
  if ("speechSynthesis" in window) {
    window.speechSynthesis.cancel();
  }
  void emitTtsEvent("tts-ended");
  // Response caption (plan Phase 3): cancel every pending word-reveal so a
  // barge-in never leaves a stale caption's words trickling in afterward.
  void import("./captionScheduler").then(({ clearCaptionSchedule }) => clearCaptionSchedule());
  useAssistant.getState().setSpeakSeq(null);
}

// Global listener for Rust-initiated barge-in / stop requests
if (typeof window !== "undefined") {
  void (async () => {
    try {
      const { listen } = await import("@tauri-apps/api/event");
      await listen("tts:stop", () => {
        stopTts();
      });
    } catch {
      // Outside Tauri environment (e.g. headless unit tests)
    }
  })();
}
