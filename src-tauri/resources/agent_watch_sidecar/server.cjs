// NEXUS agent-watch sidecar.
//
// Local-only Node process. Spawns external coding-agent CLIs (Claude Code,
// Gemini CLI, Codex, Aider, Hermes Agent, opencode) inside a PTY via
// `pty-manager` + `coding-agent-adapters` (both MIT, maintained upstream),
// and reports completion via the adapters' own `task_complete` detection
// (duration-summary + prompt-return regex matching on the ANSI-stripped
// output — see each adapter's source in node_modules/coding-agent-adapters).
//
// No network calls are made from this process. It only shells out to the
// agent CLI the caller named and serves plain HTTP on 127.0.0.1.
//
// Adding a new tool later = one more entry in ADAPTER_TYPES below (or a
// custom adapter built with pty-manager's `createAdapter(config)` factory
// for a CLI coding-agent-adapters doesn't ship) — this file never special-
// cases any tool by name beyond registering its adapter.
//
// Deliberately CommonJS (.cjs), not ESM: pty-manager v1.12.1's published
// ESM build (dist/index.mjs) uses a tsup-generated `__require` shim that
// assumes a global `require` exists, which is false for a real ES module —
// `manager.spawn()` fails every adapter with a misleading "node-pty is
// required but not installed" (the real, swallowed error is "Dynamic
// require of node-pty is not supported"). Confirmed live by spawning all
// six adapters against this exact node_modules tree. Its CJS build
// (dist/index.js) calls plain `require("node-pty")` and works correctly.

const http = require('node:http');
const { randomUUID } = require('node:crypto');
const { PTYManager } = require('pty-manager');
const { ADAPTER_TYPES, createAllAdapters, BaseCodingAdapter } = require('coding-agent-adapters');

class AntigravityAdapter extends BaseCodingAdapter {
  constructor() {
    super();
    this.adapterType = 'antigravity';
    this.displayName = 'Antigravity CLI';
    this.readySettleMs = 2000;
  }
  isResponseComplete(output) {
    const clean = this.stripAnsi(output);
    return clean.includes('Done') || clean.includes('Task complete') || /(?:^|\n)[>❯$]\s*$/.test(clean);
  }
}

class GenericTerminalAdapter extends BaseCodingAdapter {
  constructor() {
    super();
    this.adapterType = 'generic';
    this.displayName = 'Generic Agent Task';
    this.readySettleMs = 2000;
  }
  isResponseComplete(output) {
    const clean = this.stripAnsi(output);
    return clean.includes('Done') || clean.includes('Finished') || /(?:^|\n)[>❯$]\s*$/.test(clean);
  }
}

const ALL_ADAPTERS = {
  ...ADAPTER_TYPES,
  antigravity: AntigravityAdapter,
  agy: AntigravityAdapter,
  generic: GenericTerminalAdapter,
};

const PORT = Number(process.env.AGENT_WATCH_PORT || 39222);
const OUTPUT_RING_BYTES = 64 * 1024; // keep the last 64KB of PTY output per watch

const manager = new PTYManager();
for (const adapter of createAllAdapters()) {
  manager.registerAdapter(adapter);
}
manager.registerAdapter(new AntigravityAdapter());
manager.registerAdapter(new GenericTerminalAdapter());

/** @type {Map<string, {
 *   id: string, type: string, sessionId: string, status: string,
 *   startedAt: number, endedAt: number|null, output: string,
 *   detail: object|null, error: string|null,
 * }>} */
const watches = new Map();

function appendOutput(watch, chunk) {
  watch.output += chunk;
  if (watch.output.length > OUTPUT_RING_BYTES) {
    watch.output = watch.output.slice(watch.output.length - OUTPUT_RING_BYTES);
  }
}

function setStatus(watch, status, detail) {
  // A watch that already reached a terminal state (done/error/stopped)
  // never regresses back to "working" from a late-arriving event.
  const terminal = new Set(['done', 'error', 'stopped']);
  if (terminal.has(watch.status)) return;
  watch.status = status;
  if (detail !== undefined) watch.detail = detail;
  if (terminal.has(status)) watch.endedAt = Date.now();
}

// --- pty-manager event wiring (engine-level, not per-watch) -------------
// Each event carries the session id pty-manager assigned; map it back to
// our watch record. One listener per event type, shared across all watches
// (mirrors how pty-manager itself is a single manager for all sessions).

function findWatchBySession(sessionId) {
  for (const w of watches.values()) {
    if (w.sessionId === sessionId) return w;
  }
  return null;
}

manager.on('task_complete', (evt) => {
  const w = findWatchBySession(evt.id);
  if (w) setStatus(w, 'done', { reason: 'task_complete', ...evt });
});
manager.on('session_error', (evt) => {
  const w = findWatchBySession(evt.id);
  if (w) setStatus(w, 'error', { reason: 'session_error', ...evt });
});
manager.on('session_stopped', (evt) => {
  const w = findWatchBySession(evt.id);
  // Only a soft "stopped" if nothing better (done/error) already fired.
  if (w) setStatus(w, 'stopped', { reason: 'session_stopped', ...evt });
});
manager.on('login_required', (evt) => {
  const w = findWatchBySession(evt.id);
  if (w && w.status === 'working') {
    w.status = 'login_required';
    w.detail = { reason: 'login_required', ...evt };
  }
});
manager.on('blocking_prompt', (evt) => {
  const w = findWatchBySession(evt.id);
  if (w && w.status === 'working') {
    w.status = 'blocked';
    w.detail = { reason: 'blocking_prompt', ...evt };
  }
});
manager.on('message', (evt) => {
  const w = findWatchBySession(evt.id);
  if (w && typeof evt.data === 'string') appendOutput(w, evt.data);
});

// --- HTTP API -------------------------------------------------------------

function sendJson(res, status, body) {
  const text = JSON.stringify(body);
  res.writeHead(status, {
    'Content-Type': 'application/json',
    'Content-Length': Buffer.byteLength(text),
  });
  res.end(text);
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    let data = '';
    req.on('data', (c) => (data += c));
    req.on('end', () => resolve(data));
    req.on('error', reject);
  });
}

const server = http.createServer(async (req, res) => {
  try {
    const url = new URL(req.url, 'http://127.0.0.1');
    const parts = url.pathname.split('/').filter(Boolean);

    // GET /health
    if (req.method === 'GET' && parts.length === 1 && parts[0] === 'health') {
      return sendJson(res, 200, { ok: true, adapters: Object.keys(ALL_ADAPTERS) });
    }

    // GET /adapters -> static list, instant, no CLI probing
    if (req.method === 'GET' && parts.length === 1 && parts[0] === 'adapters') {
      return sendJson(res, 200, { adapters: Object.keys(ALL_ADAPTERS) });
    }

    // POST /watch { type, workdir?, prompt?, adapterConfig? }
    if (req.method === 'POST' && parts.length === 1 && parts[0] === 'watch') {
      const body = JSON.parse((await readBody(req)) || '{}');
      const type = String(body.type || '');
      if (!ALL_ADAPTERS[type]) {
        return sendJson(res, 400, {
          error: `unknown adapter type "${type}"`,
          known: Object.keys(ALL_ADAPTERS),
        });
      }
      const id = randomUUID();
      const watch = {
        id,
        type,
        sessionId: '',
        status: 'starting',
        startedAt: Date.now(),
        endedAt: null,
        output: '',
        detail: null,
        error: null,
      };
      watches.set(id, watch);

      try {
        const session = await manager.spawn({
          name: `nexus-watch-${id}`,
          type,
          workdir: body.workdir || undefined,
          adapterConfig: body.adapterConfig || {},
        });
        watch.sessionId = session.id;
        watch.status = 'working';
        if (body.prompt) {
          // Non-interactive adapters read their task from the initial
          // input line; interactive ones treat it the same as a user
          // typing it in. Either way this is the one line the caller
          // asked the agent to do.
          manager.send(session.id, `${body.prompt}\n`);
        }
      } catch (e) {
        watch.status = 'error';
        watch.error = String(e && e.message ? e.message : e);
        watch.endedAt = Date.now();
      }

      return sendJson(res, 200, { id });
    }

    // GET /watch -> list all (diagnostics)
    if (req.method === 'GET' && parts.length === 1 && parts[0] === 'watch') {
      return sendJson(res, 200, {
        watches: [...watches.values()].map((w) => ({
          id: w.id,
          type: w.type,
          status: w.status,
          startedAt: w.startedAt,
          endedAt: w.endedAt,
        })),
      });
    }

    // GET /watch/:id
    if (req.method === 'GET' && parts.length === 2 && parts[0] === 'watch') {
      const w = watches.get(parts[1]);
      if (!w) return sendJson(res, 404, { error: 'not found' });
      return sendJson(res, 200, {
        id: w.id,
        type: w.type,
        status: w.status,
        startedAt: w.startedAt,
        endedAt: w.endedAt,
        detail: w.detail,
        error: w.error,
        output: w.output,
      });
    }

    // POST /watch/:id/stop
    if (req.method === 'POST' && parts.length === 3 && parts[0] === 'watch' && parts[2] === 'stop') {
      const w = watches.get(parts[1]);
      if (!w) return sendJson(res, 404, { error: 'not found' });
      if (w.sessionId) {
        try {
          await manager.stop(w.sessionId);
        } catch {
          // already gone; fall through to mark stopped anyway
        }
      }
      setStatus(w, 'stopped', w.detail || { reason: 'user_stop' });
      return sendJson(res, 200, { ok: true });
    }

    sendJson(res, 404, { error: 'no such route' });
  } catch (e) {
    sendJson(res, 500, { error: String(e && e.message ? e.message : e) });
  }
});

server.listen(PORT, '127.0.0.1', () => {
  // Single line on purpose: Rust's lazy-sidecar launcher greps stdout for
  // readiness via the HTTP /health check, not this line, but it's useful
  // in manual runs / the unified console.
  console.log(`[AGENT-WATCH] listening on 127.0.0.1:${PORT}`);
});

// Idle shutdown: if nothing has been watched (ever, or recently finished)
// for a while, exit cleanly so the parent (Rust) can lazily respawn next
// time. Mirrors this project's STT/NLU sidecars.
const IDLE_EXIT_MS = 10 * 60 * 1000;
let lastActivity = Date.now();
function touchActivity() {
  lastActivity = Date.now();
}
manager.on('session_started', touchActivity);
manager.on('task_complete', touchActivity);
manager.on('session_error', touchActivity);
manager.on('session_stopped', touchActivity);
setInterval(() => {
  const anyWorking = [...watches.values()].some((w) => w.status === 'working' || w.status === 'blocked' || w.status === 'login_required' || w.status === 'starting');
  if (anyWorking) {
    touchActivity();
    return;
  }
  if (Date.now() - lastActivity > IDLE_EXIT_MS) {
    console.log('[AGENT-WATCH] idle timeout, exiting');
    process.exit(0);
  }
}, 30_000).unref();
