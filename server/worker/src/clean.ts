/**
 * Result cleaning, source deduplication, and structured-output validation.
 */

import type { SearchResult } from "./research";

/**
 * Normalize a URL for deduplication (strip tracking params, lowercase host).
 */
function normalizeUrl(url: string): string {
  try {
    const u = new URL(url);
    // Strip common tracking params
    const trackingParams = ["utm_source", "utm_medium", "utm_campaign", "utm_term", "utm_content", "gclid", "fbclid"];
    trackingParams.forEach(p => u.searchParams.delete(p));
    return `${u.protocol}//${u.hostname.toLowerCase()}${u.pathname}${u.search}`;
  } catch {
    return url.toLowerCase();
  }
}

/**
 * Deduplicate search results by normalized URL.
 */
export function dedupeSources(results: SearchResult[]): SearchResult[] {
  const seen = new Set<string>();
  const out: SearchResult[] = [];
  for (const r of results) {
    const key = normalizeUrl(r.url);
    if (!seen.has(key)) {
      seen.add(key);
      out.push(r);
    }
  }
  return out;
}
// NOTE: stripInjection / validateAnalysisResult / extractCaveats /
// buildResponse were deleted (audit M4) — imported but never called. The
// prompt-injection guard lives in research.ts buildSearchSynthesisPrompt.
