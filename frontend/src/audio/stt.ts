/**
 * Local Speech-to-Text interface.
 * Uses the Moonshine Python sidecar (port 39217) via Rust proxy,
 * with Groq Whisper cloud as primary (see stt_groq.rs).
 */

function isTauri(): boolean {
  return typeof (window as any).__TAURI_INTERNALS__ !== "undefined";
}

const STT_TIMEOUT_MS = 30000;

/**
 * Transcribe raw 16-bit mono PCM audio to text with the local Moonshine sidecar.
 *
 * @param samples - Raw 16-bit LE mono PCM at 16 kHz
 * @returns Transcribed text, or empty string on failure
 */
export async function transcribeAudio(samples: Int16Array): Promise<string> {
  if (!isTauri()) return "";

  const payload = Array.from(samples);
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const text = await Promise.race([
      invoke<string>("transcribe_audio", { samples: payload }),
      new Promise<string>((_, reject) =>
        setTimeout(() => reject(new Error("STT timeout")), STT_TIMEOUT_MS),
      ),
    ]);
    
    if (text && text.trim()) {
      return text.trim();
    }
  } catch (err) {
    console.error("[NEXUS] Local faster-whisper STT failed:", err);
  }

  return "";
}
