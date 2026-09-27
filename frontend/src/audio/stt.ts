/**
 * Local Speech-to-Text interface.
 * Uses the faster-whisper Python sidecar (port 39217) via Rust proxy.
 */

function isTauri(): boolean {
  return typeof (window as any).__TAURI_INTERNALS__ !== "undefined";
}

const STT_TIMEOUT_MS = 30000;

/**
 * Transcribe raw 16-bit mono PCM audio to text with local faster-whisper sidecar.
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

/**
 * Check if the local STT model is loaded/healthy.
 */
export async function sttStatus(): Promise<boolean> {
  if (!isTauri()) return false;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<boolean>("stt_status");
  } catch {
    return false;
  }
}
