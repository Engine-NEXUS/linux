// Shared vitest setup (node environment): provide minimal browser globals
// so modules that probe `window` at import time (e.g. wsBridge `isTauri()`)
// load without throwing. `__TAURI_INTERNALS__` stays undefined, so
// `isTauri()` still returns false — same as a bare node env.
(globalThis as any).window = (globalThis as any).window ?? {};

// In-memory localStorage stub (some modules touch it at import time).
if ((globalThis as any).localStorage === undefined) {
  const store = new Map<string, string>();
  (globalThis as any).localStorage = {
    getItem: (k: string) => (store.has(k) ? store.get(k) : null),
    setItem: (k: string, v: string) => void store.set(k, String(v)),
    removeItem: (k: string) => void store.delete(k),
    clear: () => store.clear(),
  };
}
