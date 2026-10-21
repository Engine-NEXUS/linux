import { useAssistant } from "../store/assistant";

/**
 * Ghost hot-mic loop — keeps the command mic open across turns while a
 * ghost session is live, so no wake word is needed between commands.
 *
 * Design constraints (each load-bearing, each verified in code):
 * - Reuses the follow-up auto-reopen path (`triggerFollowupListen` →
 *   `__NEXUS_WAKE__` → `startListening`), which bypasses wake-word
 *   suppression entirely — the suppressor only gates *detection*, and
 *   direct listen calls never consult it.
 * - Echo guard: polls `isRustTtsPlaying` (up to 3s) before reopening,
 *   so the mic never captures our own tail audio.
 * - Meeting mode suppresses the loop (checked live via `meeting_active`,
 *   not cached — meetings start mid-session).
 * - Pure decision + counter helpers below are unit-tested; the async
 *   orchestrator (`maybeGhostRelisten`) is wired but thin.
 */

/** Consecutive silent hot-mic turns tolerated before parking. */
export const GHOST_SILENT_CAP = 3;

let silentMisses = 0;

/** Should this turn-end reopen the mic? Pure (unit-tested). */
export function shouldGhostRelisten(ghostActive: boolean): boolean {
  return ghostActive;
}

/** Record one silent hot-mic turn; returns the running count. */
export function recordSilentMiss(): number {
  silentMisses += 1;
  return silentMisses;
}

/** Any heard speech resets the silence streak. */
export function resetSilentMisses(): void {
  silentMisses = 0;
}

/** Test hook: read the streak without touching it. */
export function __testSilentMissCount(): number {
  return silentMisses;
}

// ─── Ignored (non-directed) turns ─────────────────────────────────────
// TV, side talk and echo that the directed gate rejects are NOT silence: they
// used to share the silent-capture counter, so three sentences from the TV
// parked the mic silently and the user's next command went nowhere. They now
// have their own counter that only counts a streak INSIDE a time window, and
// parking is spoken once and tracked (isGhostParked) so the orb can show it.

/** Consecutive ignored turns inside the window before the mic parks. */
export const GHOST_IGNORED_CAP = 4;
/** An ignored turn older than this no longer counts toward a streak. */
export const GHOST_IGNORED_WINDOW_MS = 60_000;

let ignoredTurnTimes: number[] = [];
let parked = false;

/** Record one ignored turn at `now`; returns the streak length inside the window. */
export function recordIgnoredTurn(now: number = Date.now()): number {
  ignoredTurnTimes = ignoredTurnTimes.filter((t) => now - t <= GHOST_IGNORED_WINDOW_MS);
  ignoredTurnTimes.push(now);
  return ignoredTurnTimes.length;
}

/** A directed turn (or a wake) ends the streak. */
export function resetIgnoredTurns(): void {
  ignoredTurnTimes = [];
}

/** The hot mic stopped re-arming itself; only a wake or hotkey brings it back. */
export function markGhostParked(): void {
  parked = true;
}

/** The mic is open again (wake, hotkey or a relisten). */
export function clearGhostParked(): void {
  parked = false;
}

export function isGhostParked(): boolean {
  return parked;
}

function isTauri(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof (window as any).__TAURI_INTERNALS__ !== "undefined"
  );
}

/**
 * Echo guard: wait until Rust TTS actually goes quiet before reopening
 * the mic, so playback can't leak into the next capture. Capped at 1000ms
 * (Phase 4 cadence): the 300ms post-TTS DAC-drain mute in startListening
 * already absorbs the hardware tail — a 3s poll here only stacked dead
 * air onto every ghost turn. Skipped entirely by the caller for silent
 * steps (no TTS → nothing to wait for).
 */
async function waitForAudioIdle(timeoutMs = 1000): Promise<void> {
  const { isRustTtsPlaying } = await import("../audio/ttsPlayer");
  const start = Date.now();
  while (isRustTtsPlaying() && Date.now() - start < timeoutMs) {
    await new Promise((r) => setTimeout(r, 100));
  }
}

/**
 * Reopen the mic if — and only if — a ghost session is still live, no
 * meeting is active, and TTS has actually gone quiet. Returns true when
 * a relisten was triggered. Safe to call after every turn end; no-ops
 * everywhere outside ghost mode.
 */
export async function maybeGhostRelisten(): Promise<boolean> {
  if (!shouldGhostRelisten(useAssistant.getState().ghostActive)) return false;
  if (!isTauri()) return false;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const meeting = await invoke<boolean>("meeting_active").catch(() => false);
    if (meeting) return false;
  } catch {
    return false;
  }
  try {
    await waitForAudioIdle();
  } catch {
    // Echo guard is best-effort; a failed poll must not wedge the loop.
  }
  if (!useAssistant.getState().ghostActive) return false;
  try {
    const { triggerFollowupListen } = await import("../stage/orbRuntime");
    triggerFollowupListen(true); // hot-mic origin: Phase 8 gate applies to the next turn
    return true;
  } catch {
    return false;
  }
}

/**
 * End-of-turn reset that keeps the ghost hot-mic loop alive.
 *
 * Relisten BEFORE reset (the gate reads ghostActive — today reset()
 * preserves it, but this order must not depend on that). Outside ghost
 * mode this is exactly reset(): maybeGhostRelisten no-ops without
 * touching IPC. NEVER call from abort / barge-in / idle-timeout paths —
 * an explicit user cancel must stay cancelled.
 */
export async function endGhostTurn(): Promise<void> {
  noteGhostTurnClosed();
  await maybeGhostRelisten();
  useAssistant.getState().reset();
}

// ─── Ghost stall watchdog (P0 insurance) ──────────────────────────
// Turns that never complete (hung backend, lost result event) wedge the
// hot-mic loop with zero feedback — the relisten chain only fires on
// turn END, so a turn that never ends never relistens. The watchdog
// re-arms capture when a turn stays open too long. Quiet by design:
// capture-only, no nag speech (the silent-miss path owns nagging);
// single re-arm per stall; mid-drill speech queues via the FIFO design.

/** Budget for one ghost orchestrator round trip (recorder passes it as `timeoutMs`). */
export const GHOST_INVOKE_BUDGET_MS = 12000;
/**
 * Turn older than this with no completion counts as stalled. Must stay well
 * above GHOST_INVOKE_BUDGET_MS + the "Didn't catch that" fallback line: the
 * invoke timeout owns recovery for hung backends, and this watchdog is only
 * the backstop for a turn whose completion event was lost. At 12s (equal to
 * the budget) it raced the timeout recovery and re-armed the mic while the
 * fallback line was still speaking (double wake + the mic hearing NEXUS).
 */
export const GHOST_STALL_AFTER_MS = 20000;
/** Watchdog poll cadence (cheap timestamp checks only). */
export const GHOST_WATCHDOG_POLL_MS = 4000;

let lastTurnOpenedAt = 0;
let lastTurnClosedAt = 0;
let watchdogTimer: ReturnType<typeof setInterval> | null = null;

/** A ghost turn started (called when a ghost turn enters the pipeline). */
export function noteGhostTurnOpened(): void {
  lastTurnOpenedAt = Date.now();
}

/** A ghost turn completed (result, done, error, or turn-end reset). */
export function noteGhostTurnClosed(): void {
  lastTurnClosedAt = Date.now();
}

/** True while the current ghost turn was opened and nothing has closed it yet. */
export function isGhostTurnOpen(): boolean {
  return lastTurnOpenedAt > lastTurnClosedAt;
}

/**
 * The orchestrator invoke returned `handled_locally`. Rust answers that WITHOUT
 * any event for ambient drops, unrecognised phrases and queued commands, so
 * nothing else closes the turn: the mic stayed shut until the stall watchdog
 * fired (20 s + capture time, the 28 s freeze after "let's go."). If no
 * result/done/error event closed the turn already, close it now and re-arm the
 * mic. A turn that DID get a result is speaking; its own end relistens, so it
 * is left alone. Returns true when it closed the turn.
 */
export async function closeHandledLocallyTurn(): Promise<boolean> {
  if (!useAssistant.getState().ghostActive || !isGhostTurnOpen()) return false;
  await endGhostTurn();
  return true;
}

/** Test hooks (do not use in production code). */
export function __testSetGhostTurnTimes(opened: number, closed: number): void {
  lastTurnOpenedAt = opened;
  lastTurnClosedAt = closed;
}
export function __testGetGhostTurnTimes(): { opened: number; closed: number } {
  return { opened: lastTurnOpenedAt, closed: lastTurnClosedAt };
}
export function __testStopGhostWatchdog(): void {
  if (watchdogTimer) clearInterval(watchdogTimer);
  watchdogTimer = null;
}

/** Start the stall watchdog (call on ghost session enter; idempotent). */
export function startGhostWatchdog(): void {
  if (watchdogTimer) return;
  watchdogTimer = setInterval(() => {
    void watchdogTick().catch(() => {});
  }, GHOST_WATCHDOG_POLL_MS);
}

/** Stop the stall watchdog (call on ghost session exit). */
export function stopGhostWatchdog(): void {
  if (watchdogTimer) clearInterval(watchdogTimer);
  watchdogTimer = null;
}

async function watchdogTick(): Promise<void> {
  if (!useAssistant.getState().ghostActive) return;
  if (lastTurnOpenedAt <= lastTurnClosedAt) return; // no open turn
  if (Date.now() - lastTurnOpenedAt < GHOST_STALL_AFTER_MS) return;
  // A drill that is still carrying out the command (WhatsApp, a click chain) is progress, not a stall:
  // re-arming the mic here cancelled it mid-way (2026-10-15 log: "WhatsApp lost focus").
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    if (await invoke<boolean>("ghost_drill_running")) return;
  } catch {
    // unknown = treat as stalled (the old behaviour)
  }
  // Stall: one quiet re-arm, then mark closed so this stall never
  // refires (the next transcript re-opens tracking).
  console.warn("[NEXUS] ghost watchdog: turn open with no completion — re-arming listen");
  lastTurnClosedAt = Date.now();
  try {
    // Guarded path (ghost still live, no meeting, TTS idle) — the same one
    // the Rust-side watchdog's `ghost:relisten` uses. A direct
    // triggerFollowupListen here would open the mic over a speaking
    // fallback line.
    await maybeGhostRelisten();
  } catch {
    // A failed re-arm must not wedge the loop either.
  }
}

/**
 * Intents a live ghost session owns. With a session active these must
 * bypass the frontend local-execute path and go to the orchestrator,
 * whose ghost runners narrate, focus-verify, and keep the session open
 * for follow-ups (plain local execute opens silently and starves the
 * drill pipeline). Everything else stays local. Pure (unit-tested).
 */
const GHOST_ROUTED_ACTIONS = ["open_app", "whatsapp_chat"];

export function shouldGhostRoute(intentAction: string, ghostActive: boolean): boolean {
  return ghostActive && GHOST_ROUTED_ACTIONS.includes(intentAction);
}
