/**
 * Directed-speech gate client (Phase 8).
 *
 * While a ghost session keeps the mic open, utterances that are not addressed to NEXUS (TV, a side
 * conversation, our own TTS echo, STT hallucination loops) must not reach the command pipeline or the
 * cloud LLM. The decision itself is made in Rust (`directed_gate`, deterministic, < 1 ms); this module
 * only asks, fails OPEN on any error, and maps the verdict to what the hot-mic loop should do.
 */

export interface DirectedVerdict {
  accept: boolean;
  reason: string;
}

/** Ask the Rust gate. Any failure (no Tauri, command missing, bad payload) accepts — never drop a command by accident. */
export async function askDirectedGate(transcript: string): Promise<DirectedVerdict> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const v = await invoke<DirectedVerdict | undefined>("directed_gate", { transcript });
    if (v && typeof v.accept === "boolean") return v;
  } catch {
    // fall through: fail open
  }
  return { accept: true, reason: "gate_unavailable" };
}

export type GateAction = "proceed" | "relisten" | "park";

/**
 * What the hot-mic loop does with a verdict. Ignored (non-directed) speech counts like a silent turn so
 * a television or a long side conversation cannot keep the loop (and the cloud STT bill) running forever:
 * the first `cap - 1` consecutive ignored turns re-listen quietly, the `cap`-th parks the mic until the
 * user wakes NEXUS again. Pure (unit-tested).
 */
export function gateAction(verdict: DirectedVerdict, consecutiveIgnored: number, cap: number): GateAction {
  if (verdict.accept) return "proceed";
  return consecutiveIgnored < cap ? "relisten" : "park";
}
