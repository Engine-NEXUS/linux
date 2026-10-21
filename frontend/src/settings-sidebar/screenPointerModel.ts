/**
 * Pure helpers for the "Screen pointer" settings card.
 *
 * The backend clamps and normalizes these values again; this module keeps the
 * UI predictable before and after a save (and mirrors the same defaults).
 */

export interface PointerSettings {
  enabled: boolean;
  dwellSeconds: number;
  excludedApps: string[];
  suppressFullscreen: boolean;
}

const MIN_DWELL_SECONDS = 3;
const MAX_DWELL_SECONDS = 15;
const DEFAULT_DWELL_SECONDS = 6;
const MAX_EXCLUDED_APPS = 64;
const MAX_EXCLUDED_APP_CHARS = 128;

export const DEFAULT_POINTER_SETTINGS: PointerSettings = {
  enabled: true,
  dwellSeconds: DEFAULT_DWELL_SECONDS,
  excludedApps: [],
  suppressFullscreen: true,
};

/** Whole seconds in 3..15; NaN and non-numbers fall back to 6. */
export function clampDwell(n: unknown): number {
  if (typeof n !== "number" || Number.isNaN(n)) return DEFAULT_DWELL_SECONDS;
  const rounded = Math.round(n);
  return Math.min(MAX_DWELL_SECONDS, Math.max(MIN_DWELL_SECONDS, rounded));
}

function normalizeEntry(value: string): string {
  return value.trim().toLowerCase().slice(0, MAX_EXCLUDED_APP_CHARS);
}

function collectEntries(values: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const value of values) {
    const entry = normalizeEntry(value);
    if (!entry || seen.has(entry)) continue;
    seen.add(entry);
    out.push(entry);
    if (out.length >= MAX_EXCLUDED_APPS) break;
  }
  return out;
}

/** Split comma/newline separated fragments into a clean, ordered list. */
export function parseExcludedApps(text: string): string[] {
  if (typeof text !== "string") return [];
  return collectEntries(text.split(/[,\r\n]+/));
}

/** Render the list back into the comma separated text field. */
export function formatExcludedApps(list: string[]): string {
  return Array.isArray(list) ? list.join(", ") : "";
}

/** Fill missing or wrong-typed fields from the defaults. */
export function normalizePointerSettings(raw: unknown): PointerSettings {
  const r =
    typeof raw === "object" && raw !== null
      ? (raw as Record<string, unknown>)
      : {};
  const excludedApps = Array.isArray(r.excludedApps)
    ? collectEntries(
        r.excludedApps.filter((value): value is string => typeof value === "string"),
      )
    : [];

  return {
    enabled:
      typeof r.enabled === "boolean"
        ? r.enabled
        : DEFAULT_POINTER_SETTINGS.enabled,
    dwellSeconds: clampDwell(r.dwellSeconds),
    excludedApps,
    suppressFullscreen:
      typeof r.suppressFullscreen === "boolean"
        ? r.suppressFullscreen
        : DEFAULT_POINTER_SETTINGS.suppressFullscreen,
  };
}
