import { useEffect, useState } from "react";
import {
  getThemeMode,
  resolveEffectiveTheme,
  resolveThemeMode,
  systemPrefersLight,
  THEME_EVENT,
} from "./theme";

/**
 * Current EFFECTIVE Light/Dark mode of this window (follows live changes).
 * A stored "system" choice resolves against the OS preference at read time
 * and re-resolves on every theme event (including OS flips, which
 * initThemeSync re-broadcasts), so consumers always get a concrete
 * "light" | "dark" to style with.
 */
export function useThemeMode(): "light" | "dark" {
  const [mode, setMode] = useState<"light" | "dark">(() =>
    resolveEffectiveTheme(getThemeMode(), systemPrefersLight()),
  );
  useEffect(() => {
    const onChange = (e: Event) => {
      const m = resolveThemeMode((e as CustomEvent).detail);
      setMode(resolveEffectiveTheme(m, systemPrefersLight()));
    };
    window.addEventListener(THEME_EVENT, onChange);
    return () => window.removeEventListener(THEME_EVENT, onChange);
  }, []);
  return mode;
}
