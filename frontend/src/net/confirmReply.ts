/**
 * Is a spoken sentence an ANSWER to a pending confirmation ("yes" / "no"), or something else?
 *
 * The old test was `^(yes|ok|okay|sure|...)\b`: no end anchor, so "okay so what is the weather",
 * "ok open notepad" or "sure what time is it" all CONFIRMED a pending message send / PR merge / order.
 * This version is strict: after removing the wake word, EVERY word must belong to the small answer
 * vocabulary below. One unknown content word ("weather", "notepad", "chrome") makes the sentence a
 * normal command instead, and the pending question is left alone / dropped by the caller.
 *
 * Pure and unit-tested (see confirmReply.test.ts). Plan: docs/features/124 (Q1).
 */

export type ConfirmReply = "yes" | "no" | "other";

/** A pending voice confirmation expires after this long (high-risk actions: send, merge, order). */
export const PENDING_CONFIRM_TTL_MS = 2 * 60_000;

const YES_WORDS = new Set([
  "yes", "yeah", "yep", "yup", "sure", "ok", "okay", "confirm", "confirmed", "proceed", "approve", "approved",
  "agree", "agreed", "affirmative", "absolutely", "definitely", "certainly", "correct", "right", "alright",
]);

const YES_PHRASES = ["go ahead", "do it", "go for it", "that's right", "thats right", "sounds good", "of course"];

const NO_WORDS = new Set([
  "no", "nope", "nah", "cancel", "abort", "stop", "don't", "dont", "never", "disapprove", "disapproved",
  "negative", "skip", "not",
]);

/** Words that may accompany an answer without changing its meaning. */
const FILLER = new Set([
  "please", "now", "sir", "thanks", "thank", "you", "it", "that", "this", "go", "ahead", "for", "do", "the",
  "then", "with", "to", "of", "course", "fine", "good", "great", "kindly", "nexus", "mind", "thats", "that's",
  "sounds", "its", "it's",
  // verbs of the pending commands: "yes send it", "ok merge it"
  "send", "merge", "close", "order", "place", "delete", "book", "publish", "post", "submit", "execute", "run",
  "approve", "confirm",
]);

const MAX_WORDS = 6;

function words(text: string): string[] {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9'\s]/g, " ")
    .split(/\s+/)
    .filter(Boolean);
}

export function classifyConfirmReply(text: string): ConfirmReply {
  const w = words(text);
  if (w[0] === "nexus") w.shift();
  if (w.length === 0 || w.length > MAX_WORDS) return "other";
  const joined = w.join(" ");
  const known = (x: string) => YES_WORDS.has(x) || NO_WORDS.has(x) || FILLER.has(x);
  if (!w.every(known)) return "other";
  const hasNo = w.some((x) => NO_WORDS.has(x)) || joined.includes("never mind");
  const hasYes = w.some((x) => YES_WORDS.has(x)) || YES_PHRASES.some((p) => joined.includes(p));
  // "no" wins over "yes": "no don't do it" must never confirm, and "not now" is a refusal.
  if (hasNo) return "no";
  if (hasYes) return "yes";
  return "other";
}

/** True when a pending confirmation set at `at` (ms epoch) is too old to be answered by voice. */
export function isConfirmExpired(at: number | null, now: number, ttlMs: number = PENDING_CONFIRM_TTL_MS): boolean {
  return at !== null && now - at > ttlMs;
}
