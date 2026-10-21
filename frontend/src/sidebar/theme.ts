import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";

export type ThemeMode = "light" | "dark" | "system";

export const THEME_STORAGE_KEY = "nexus-theme-mode";
export const THEME_EVENT = "nexus:theme-change";

function isTauri(): boolean {
  return typeof window !== "undefined" && typeof (window as any).__TAURI_INTERNALS__ !== "undefined";
}

/** Normalize any stored/legacy value — unknown always falls back to dark. */
export function resolveThemeMode(raw: unknown): ThemeMode {
  return raw === "light" || raw === "system" ? raw : "dark";
}

/** Does the OS currently prefer light? False whenever the API is missing. */
export function systemPrefersLight(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-color-scheme: light)").matches
  );
}

/**
 * "system" resolves against the OS preference at call time; explicit
 * light/dark pass through untouched. The STORED mode is never rewritten to
 * the resolved value — following the system must keep following it when the
 * OS flips later, not freeze at whatever the OS was at apply time.
 */
export function resolveEffectiveTheme(mode: ThemeMode, prefersLight: boolean): "light" | "dark" {
  if (mode === "system") return prefersLight ? "light" : "dark";
  return mode;
}

/** Read the stored theme choice (explicit value, else dark default). */
export function getThemeMode(): ThemeMode {
  try {
    return resolveThemeMode(localStorage.getItem(THEME_STORAGE_KEY));
  } catch {
    return "dark";
  }
}

/** Apply theme to the document + persist + notify other mounted views and windows. */
export function applyThemeMode(mode: ThemeMode, broadcast = true): void {
  const next = resolveThemeMode(mode);
  try {
    localStorage.setItem(THEME_STORAGE_KEY, next);
  } catch {
    /* private mode — attribute still applies for this session */
  }
  // `data-theme` always gets a CONCRETE value (the CSS keys on light/dark);
  // "system" is resolved here and re-resolved on every OS flip (see
  // initThemeSync's matchMedia listener).
  const effective = resolveEffectiveTheme(next, systemPrefersLight());
  if (typeof document !== "undefined") {
    document.documentElement.setAttribute("data-theme", effective);
  }
  if (typeof window !== "undefined") {
    // Broadcast the RAW choice: every window re-resolves "system" against
    // its own matchMedia, so the follow-system semantics stay per-window.
    window.dispatchEvent(new CustomEvent(THEME_EVENT, { detail: next }));
  }
  if (broadcast && isTauri()) {
    emit(THEME_EVENT, next).catch(() => {});
  }
}

/**
 * Keep THIS window's theme in step with the others.
 * Syncs with settings.json, localStorage storage events, and cross-window Tauri emits.
 */
let themeSyncStarted = false;
export function initThemeSync(): void {
  if (typeof window === "undefined" || themeSyncStarted) return;
  themeSyncStarted = true;

  // Apply immediately from local cache
  applyThemeMode(getThemeMode(), false);

  // Sync with settings.json on disk if in Tauri
  if (isTauri()) {
    invoke<{ themeMode?: string }>("get_settings")
      .then((s) => {
        if (s?.themeMode) {
          applyThemeMode(resolveThemeMode(s.themeMode), false);
        }
      })
      .catch(() => {});

    // Listen for live settings updates from Rust
    listen<{ themeMode?: string }>("settings:updated", (event) => {
      if (event.payload?.themeMode) {
        applyThemeMode(resolveThemeMode(event.payload.themeMode), false);
      }
    }).catch(() => {});

    // Listen for cross-window theme broadcast
    listen<string>(THEME_EVENT, (event) => {
      if (event.payload) {
        applyThemeMode(resolveThemeMode(event.payload), false);
      }
    }).catch(() => {});
  }

  // Cross-view storage listener within same origin
  window.addEventListener("storage", (e) => {
    if (e.key === THEME_STORAGE_KEY) {
      applyThemeMode(resolveThemeMode(e.newValue), false);
    }
  });

  // Follow the OS while the stored choice is "system". The stored value is
  // never rewritten to light/dark here — that would freeze follow-system at
  // whatever the OS happened to be at apply time. applyThemeMode dispatches
  // the local THEME_EVENT even with broadcast=false, so every in-window
  // consumer (e.g. useThemeMode) re-resolves on each OS flip.
  if (typeof window.matchMedia === "function") {
    const mql = window.matchMedia("(prefers-color-scheme: light)");
    const onSystemChange = () => {
      if (getThemeMode() === "system") applyThemeMode("system", false);
    };
    if (typeof mql.addEventListener === "function") {
      mql.addEventListener("change", onSystemChange);
    }
  }
}

