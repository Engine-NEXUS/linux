/**
 * Logged IPC boundaries (log-completeness P2).
 *
 * The codebase historically silenced every invoke/emit failure with
 * `.catch(() => {})` — fine for idempotent no-ops (hide-if-hidden) but
 * fatal for diagnosis when a CRITICAL call fails (abortCapture paths,
 * hitbox registration, calibration save, loading show/hide): the feature
 * breaks with zero log evidence.
 *
 * Rule: critical IPC uses these wrappers (failure → console.warn, still
 * rethrows so callers keep their control flow); benign no-ops keep the
 * silent catch. Dynamic imports preserve the non-Tauri (tests) path.
 */
export async function invokeLogged<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    console.warn(`[IPC] invoke ${cmd} failed:`, e);
    throw e;
  }
}

export async function emitLogged(event: string, payload?: unknown): Promise<void> {
  const { emit } = await import("@tauri-apps/api/event");
  try {
    await emit(event, payload);
  } catch (e) {
    console.warn(`[IPC] emit ${event} failed:`, e);
    throw e;
  }
}
