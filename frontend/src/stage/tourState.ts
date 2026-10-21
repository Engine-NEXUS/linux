/**
 * Tiny shared flag: is a narrated screen tour running right now?
 * The response caption hides while it is (the callout already shows the
 * text — drawing both would duplicate it). Plain module state + subscribe,
 * no React/store dependency, so any stage component can read it.
 */
let active = false;
const subscribers = new Set<(v: boolean) => void>();

export function isTourActive(): boolean {
  return active;
}

export function setTourActive(v: boolean): void {
  if (active === v) return;
  active = v;
  subscribers.forEach((cb) => cb(v));
}

export function onTourActive(cb: (v: boolean) => void): () => void {
  subscribers.add(cb);
  return () => {
    subscribers.delete(cb);
  };
}
