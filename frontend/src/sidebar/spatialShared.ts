/**
 * Feature 86 — shared pure helpers for the spatial overlay/dashboard.
 * Exported + unit-tested (repo test style: pure functions, no DOM).
 */

/** Truncate a pin label for the floating stage badge (≤18 chars). */
export function pinLabel(label: string): string {
  const t = label.trim();
  if (!t) return "Element";
  return t.length > 18 ? `${t.slice(0, 17)}…` : t;
}

/**
 * Clean a raw transcript into the topic title shown above reference
 * results: strips wrapping quotes/brackets/punctuation, collapses
 * whitespace, capitalizes the first letter.
 */
export function topicTitle(query: string): string {
  const cleaned = (query || "")
    .replace(/^[\s"'“”'([\-:;{[]+/, "")
    .replace(/[\s"'“”'().,;:!?\-}\]>]+$/, "")
    .replace(/\s+/g, " ")
    .trim();
  return cleaned ? cleaned.charAt(0).toUpperCase() + cleaned.slice(1) : "";
}
