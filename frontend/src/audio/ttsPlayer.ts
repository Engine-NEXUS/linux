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
    id: "en-US-AvaNeural",
    name: "Ava (Edge TTS)",
    provider: "edge",
    accent: "American",
    description: "Warm, natural female voice. Cloud-powered, free, 0 MB RAM.",
    locale: "en-US",
    gender: "female",
    sampleText: "Hello, I am Ava. All systems are operational.",
  },
  {
    id: "en-US-GuyNeural",
    name: "Guy (Edge TTS)",
    provider: "edge",
    accent: "American",
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
    // Only fall back to Web Speech if we haven't been barged in
    if (ttsGeneration === myGen) {
      console.error("[TTS] Kokoro failed, falling back to Web Speech:", err);
      await speakWebSpeech(text, speed);
    }
  } finally {
    rustTtsPlaying = false;  // Playback complete (or barge-in stopped it)
    void emitTtsEvent("tts-ended");
    // Only fire onEnd if not barged in — prevents stale callbacks
    if (ttsGeneration === myGen) {
      onEnd?.();
    }
  }
}

/** Web Speech API fallback — uses the browser's built-in speech synthesis. */
async function speakWebSpeech(text: string, speed: number = 1.15): Promise<void> {
  return new Promise((resolve) => {
    if (!("speechSynthesis" in window)) {
      console.warn("[TTS] Web Speech API not available");
      resolve();
      return;
    }
    window.speechSynthesis.cancel();
    const utterance = new SpeechSynthesisUtterance(text);
    utterance.rate = speed;
    utterance.pitch = 1.0;
    utterance.volume = 1.0;
    // Try to use a male voice for "sir" persona
    const voices = window.speechSynthesis.getVoices();
    const preferred = voices.find(v => v.name.includes("David") || v.name.includes("Mark") || v.name.includes("George"))
      || voices.find(v => v.lang.startsWith("en"));
    if (preferred) utterance.voice = preferred;
    utterance.onend = () => resolve();
    utterance.onerror = () => resolve();
    window.speechSynthesis.speak(utterance);
  });
}

export async function previewVoice(
  voice: VoiceOption,
  _customApiKey?: string,
  onEnd?: () => void,
  speed?: number,
): Promise<void> {
  stopTts();
  // All voices now go through the Rust speak_text command which tries
  // Edge TTS (cloud) first, then Piper (local) fallback.
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
  // causing every speak() to silently fall back to Piper (local).
  const voiceId = settings?.edgeTtsVoice || "en-US-AvaNeural";
  const speed = settings?.speechRate ?? 1.15;

  // Sentence-streamed speech for long results: synthesize + play the first
  // sentence while later ones still generate (first audio in ~300ms instead
  // of after full synthesis). Short texts go direct — identical behavior.
  // Barge-in safe: playKokoro checks the generation per chunk, so a stop
  // mid-queue silences the rest. Periods only split on whitespace so
  // decimals ("3.14") and versions stay whole.
  const chunks = splitForSpeech(text);
  if (chunks.length <= 1) {
    return playKokoro(text, voiceId, speed, myGen, onEnd);
  }
  for (let i = 0; i < chunks.length; i++) {
    if (ttsGeneration !== myGen) {
      console.log("[TTS] streamed speak stopped — barge-in");
      return;
    }
    const last = i === chunks.length - 1;
    await playKokoro(chunks[i], voiceId, speed, myGen, last ? onEnd : undefined);
  }
}

/**
 * Split long text into speakable sentence chunks. Short text returns
 * a single chunk (no behavior change). Long punctuation-free stretches
 * force-flush at ~400 chars so audio never stalls.
 */
export function splitForSpeech(text: string): string[] {
  const trimmed = text.trim();
  if (trimmed.length < 150) return [trimmed];
  const parts = trimmed.match(/[^.!?…]+[.!?…]+(\s+|$)|[^.!?…]+$/g);
  const sentences = (parts ?? [trimmed]).map((s) => s.trim()).filter(Boolean);
  const chunks: string[] = [];
  let buf = "";
  const flush = () => {
    if (buf.trim()) chunks.push(buf.trim());
    buf = "";
  };
  for (const s of sentences) {
    if ((buf + " " + s).trim().length > 400) flush();
    buf = buf ? buf + " " + s : s;
    if (/[.!?…]$/.test(s.trim())) flush();
  }
  flush();
  return chunks.length ? chunks : [trimmed];
}

/**
 * Speak a pre-cached TTS phrase instantly from memory.
 * Falls back to `speak` if the phrase is not cached.
 * Emits `tts:audio-started` event before playback starts.
 */
export async function speakCached(phrase: string, onEnd?: () => void): Promise<void> {
  const meeting = await isMeetingActive();
  if (meeting) {
    console.log("[TTS] Suppressed — meeting mode active");
    onEnd?.();
    return;
  }

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
    // Fallback to regular speak if cached phrase not available
    console.warn("[TTS] speak_cached failed, falling back to speak:", e);
    rustTtsPlaying = false;
    void emitTtsEvent("tts-ended");
    return speak(phrase, onEnd);
  } finally {
    rustTtsPlaying = false;
    void emitTtsEvent("tts-ended");
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
  useAssistant.getState().setSpeakSeq(null);
}

export function ttsAvailable(): boolean {
  return true;
}
