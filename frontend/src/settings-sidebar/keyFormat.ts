/** Plausible Gemini API key shape: AIza + 35 chars (39 total).
 *  Client-side hint only — real validation is the Test button
 *  (models/list call, no quota burn). Pure + unit-tested. */
export function isPlausibleGeminiKey(key: string): boolean {
  return /^AIza[0-9A-Za-z_-]{35}$/.test((key || "").trim());
}
