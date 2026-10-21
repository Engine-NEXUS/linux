/**
 * Ghost turns run in the order they were heard.
 *
 * The hot mic opens the next capture while the previous transcript is still being parsed, so a turn
 * with a pre-parsed fast path ("nexus exit ghost mode") could overtake an older turn that needs the
 * slow parse ("next is ghost mode"), and the older one then ran AFTER it (live log 2026-10-13: the
 * cloud chat answered "switching to ghost mode now" right after Ghost mode had been turned off).
 *
 * `runInTurnOrder` chains each turn behind the previous one. A stuck predecessor can only delay the
 * next turn by `maxWaitMs`.
 */

let tail: Promise<void> = Promise.resolve();

export function runInTurnOrder<T>(fn: () => Promise<T>, maxWaitMs = 15_000): Promise<T> {
  const prev = tail;
  let release!: () => void;
  tail = new Promise<void>((resolve) => {
    release = resolve;
  });
  return (async () => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    await Promise.race([
      prev,
      new Promise<void>((resolve) => {
        timer = setTimeout(resolve, maxWaitMs);
      }),
    ]);
    if (timer) clearTimeout(timer);
    try {
      return await fn();
    } finally {
      release();
    }
  })();
}

/** Test hook: forget the chain. */
export function __testResetTurnChain(): void {
  tail = Promise.resolve();
}
