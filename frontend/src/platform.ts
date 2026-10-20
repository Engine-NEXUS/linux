/**
 * Platform helpers for the renderer.
 *
 * On Linux the app's web layer is WebKitGTK (not WebView2/Chromium) and the
 * Linux build has no in-app global hotkey: `src-tauri/src/hotkey.rs` is
 * compiled out there (`#![cfg(not(target_os = "linux"))]`), so the wake chord
 * is bound by the desktop environment itself — `scripts/register-hotkey.sh`
 * (GNOME) or a COSMIC custom shortcut — to Ctrl+Super+Space, which launches
 * `nexus --wake`.
 *
 * Windows/macOS keep the built-in Ctrl+Space chord, so every label that names
 * the chord has to be platform-aware. `markPlatform()` also tags <body> so CSS
 * can drop the glass treatment where there is no native blur to back it
 * (see theme/tokens.css and sidebar/sidebar.css).
 */

/** The chord the Windows/macOS build registers itself. */
export const DEFAULT_HOTKEY = "Ctrl+Space";

/** The chord the Linux desktop environment binds to `nexus --wake`. */
export const LINUX_HOTKEY = "Ctrl+Super+Space";

/**
 * Pure platform check over a user-agent / platform string.
 *
 * WebKitGTK on Linux reports "Linux" in both the user agent and
 * `navigator.platform`; WebView2/Chromium on Windows and Safari/WebKit on
 * macOS never do.
 */
export function detectLinux(userAgent?: string, platform?: string): boolean {
  return /linux/i.test(userAgent ?? "") || /linux/i.test(platform ?? "");
}

/** True when the renderer runs on Linux (WebKitGTK). */
export function isLinuxRuntime(): boolean {
  if (typeof navigator === "undefined") return false;
  return detectLinux(navigator.userAgent, navigator.platform);
}

/**
 * Tag <body> with the platform so CSS can adapt: the Linux build has no
 * native window blur and no backdrop capture, so the translucent "liquid
 * glass" surfaces have to become opaque panels.
 */
export function markPlatform(): void {
  if (!isLinuxRuntime()) return;
  if (typeof document === "undefined" || !document.body) return;
  document.body.dataset.platform = "linux";
}

/** The wake chord that is actually bound on this platform. */
export function globalHotkeyLabel(isLinux: boolean = isLinuxRuntime()): string {
  return isLinux ? LINUX_HOTKEY : DEFAULT_HOTKEY;
}

/**
 * One-line explanation shown under the hotkey field.
 *
 * `windowsHint` is the wording the Windows/macOS build already used — it is
 * returned unchanged there so no Windows string shifts. On Linux the chord
 * is bound by the desktop environment, not by the app, so the hint has to
 * say so.
 */
export function globalHotkeyHint(
  windowsHint: string,
  isLinux: boolean = isLinuxRuntime(),
): string {
  return isLinux
    ? `${LINUX_HOTKEY} — bind it in your desktop settings to \`nexus --wake\``
    : windowsHint;
}
