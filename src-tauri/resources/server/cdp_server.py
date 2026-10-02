"""
CDP tier for NEXUS — semantic perception and actuation for Chromium/Electron.

Runs LOCALLY (127.0.0.1:39222).

WHY THIS EXISTS
---------------
AT-SPI is the right tier for GTK/Qt, and useless for Chromium/Electron. Those
apps expose only an `application -> frame` skeleton over AT-SPI unless launched
with `--force-renderer-accessibility`. Measured on this machine:

    VS Code via AT-SPI .................. 1 node
    VS Code via CDP (this tier) ........ 817 nodes

One node versus 817. That is why the audit lists Chromium apps as unsearchable
over AT-SPI, and why this tier is the only way to see VS Code, Chrome, Slack,
Discord or any other Electron app.

The chain, verified on this machine before this file was written:

    Accessibility.getFullAXTree   -> role + accessible name + backendDOMNodeId
    DOM.resolveNode               -> a live JavaScript object handle
    Runtime.callFunctionOn        -> this.click()      (or the native value setter)

No pixels, no compositor, no portal, no consent dialog. And because the target is
addressed by a *node identity* rather than a coordinate, a layout shift cannot
invalidate it and an unrelated element cannot be substituted at the same
coordinates. See docs/features/research/20 for why that matters.

TWO TRANSPORT MODES
-------------------
`direct`  Chrome / Chromium / Brave / Edge. Connect straight to the page target's
           `webSocketDebuggerUrl` from `/json/list`. Requires the browser to have
           been launched with `--remote-debugging-port=<port>`.

`electron`  VS Code, Discord, Slack, and other Electron apps. The renderer is not
           reachable directly, so this connects to the **Node inspector** of the
           main process and drives `webContents.debugger` from in there.

           The `--remote-debugging-port` flag is NOT usable for Electron: shipped
           Electron has an authenticated-CDP gate that terminates the app when
           that flag is on argv. `--inspect` (or SIGUSR1) is not gated. Verified:
           VS Code launched with `--inspect=<port>` starts and stays up, and the
           full chain works from the main process.

           Two traps, both hit while writing this and both avoided below:
             * bare `require` is not a global in the CDP eval scope — it must be
               reached as `process.mainModule.require(...)`
             * an already-running Electron app cannot be attached after the fact;
               the flag is a launch-time decision, so the app must be restarted

LAUNCH REQUIREMENTS
-------------------
  Chromium:  brave-browser --remote-debugging-port=9222 ...
  Electron:  code --inspect=9229 ...          (or send SIGUSR1 to the main process)

This tier reports honestly when no endpoint is reachable rather than pretending.
A browser that was not launched with a debug port is simply not visible here,
and /health says so.

Grounding order (docs/features/research/21):
  1. App native API  2. AT-SPI  3. THIS  4. OCR / Set-of-Mark / vision

Sync note: production bundles a copy at src-tauri/resources/server/cdp_server.py.
Keep both identical — see server/ocr_server.py precedent.

Requirements:
  pip install websockets fastapi uvicorn      # websockets ships wheels

Run:
  uvicorn cdp_server:app --host 127.0.0.1 --port 39222

Environment:
  CDP_PORT        — comma-separated debug ports to probe (default 9222,9229,9230)
  CDP_SCAN_RANGE  — also scan 9200-9400 (default off; slow but thorough)
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import time
import urllib.error
import urllib.request
from typing import Any

from fastapi import FastAPI, Request
from fastapi.concurrency import run_in_threadpool
from fastapi.responses import JSONResponse

log = logging.getLogger("nexus.cdp")
logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s")

app = FastAPI(title="NEXUS CDP", version="0.1.0")

DEFAULT_PORTS = [9222, 9229, 9230]
AX_BUDGET = int(os.environ.get("CDP_AX_BUDGET", "6000"))
# A cold AX read can be partial. Retries stop as soon as the tree looks real.
AX_WARM_ATTEMPTS = int(os.environ.get("CDP_AX_WARM_ATTEMPTS", "4"))
AX_WARM_ENOUGH = int(os.environ.get("CDP_AX_WARM_ENOUGH", "40"))

try:
    import websockets  # type: ignore
    HAVE_WEBSOCKETS = True
except Exception:  # pragma: no cover
    websockets = None  # type: ignore
    HAVE_WEBSOCKETS = False


def probe_ports() -> list[int]:
    ports = [
        int(p)
        for p in os.environ.get("CDP_PORT", ",".join(str(x) for x in DEFAULT_PORTS)).split(",")
        if p.strip().isdigit()
    ]
    if os.environ.get("CDP_SCAN_RANGE") == "1":
        ports += [p for p in range(9200, 9401) if p not in ports]
    return ports


def _http_json(url: str, timeout: float = 3.0) -> Any:
    with urllib.request.urlopen(url, timeout=timeout) as r:
        return json.load(r)


def http_list(port: int) -> list[dict]:
    """Page/window targets on a DevTools HTTP port. Empty when nothing is there."""
    try:
        data = _http_json(f"http://127.0.0.1:{port}/json/list")
    except (urllib.error.URLError, OSError, ValueError, TimeoutError):
        return []
    return [t for t in data if isinstance(t, dict)]


def inspector_main_target(port: int) -> dict | None:
    """The Node main-process target on an Electron inspector port."""
    try:
        data = _http_json(f"http://127.0.0.1:{port}/json/list")
    except (urllib.error.URLError, OSError, ValueError, TimeoutError):
        return None
    for t in data:
        if isinstance(t, dict) and t.get("type") == "node" and t.get("webSocketDebuggerUrl"):
            return t
    return None


def reachable() -> dict:
    """Which endpoints are live right now, and what kind each one is.

    Classification is by *target shape*, not merely "something answered on this
    port". An Electron inspector port lists a `node` main-process target and no
    `page`, so treating it as a direct Chromium port makes every lookup fail with
    "no page target" — which is what the first version did. A port is direct
    only if it actually serves a `page` target.
    """
    out = {"direct_ports": [], "electron_ports": [], "any": False}
    for p in probe_ports():
        tgts = http_list(p)
        if any(t.get("type") == "page" and t.get("webSocketDebuggerUrl") for t in tgts):
            out["direct_ports"].append(p)
        if any(t.get("type") == "node" and t.get("webSocketDebuggerUrl") for t in tgts):
            out["electron_ports"].append(p)
    out["any"] = bool(out["direct_ports"] or out["electron_ports"])
    return out


# ── CDP client ───────────────────────────────────────────────────────────────

class CdpError(RuntimeError):
    pass


class Cdp:
    """Minimal CDP session over a websocket. Auto-reconnects per call."""

    def __init__(self, ws_url: str):
        self.ws_url = ws_url
        self._id = 0
        self._ws = None

    async def __aenter__(self):
        if not HAVE_WEBSOCKETS:
            raise CdpError("websockets package not installed")
        self._ws = await websockets.connect(self.ws_url, max_size=64 * 1024 * 1024)
        return self

    async def __aexit__(self, *exc):
        if self._ws is not None:
            await self._ws.close()
            self._ws = None

    async def call(self, method: str, params: dict | None = None) -> dict:
        if self._ws is None:
            raise CdpError("not connected")
        self._id += 1
        mid = self._id
        await self._ws.send(json.dumps({"id": mid, "method": method, "params": params or {}}))
        while True:
            raw = await asyncio.wait_for(self._ws.recv(), timeout=30)
            try:
                msg = json.loads(raw)
            except (TypeError, ValueError):
                continue
            if msg.get("id") == mid:
                if "error" in msg:
                    raise CdpError(f"{method}: {msg['error']}")
                return msg.get("result", {})


# ── Direct (Chromium) transport ──────────────────────────────────────────────

def _pick_page_target(tgts: list[dict], want: str | None) -> dict | None:
    pages = [t for t in tgts if t.get("type") == "page" and t.get("webSocketDebuggerUrl")]
    if not pages:
        return None
    if want:
        for t in pages:
            if want in (t.get("url") or "") or want == (t.get("title") or ""):
                return t
        return None
    # Prefer a non-blank page.
    for t in pages:
        if (t.get("url") or "") not in ("about:blank", ""):
            return t
    return pages[0]


async def direct_ax(port: int, target: str | None) -> list[dict]:
    tgts = http_list(port)
    t = _pick_page_target(tgts, target)
    if t is None:
        raise CdpError(f"no page target on port {port}")
    async with Cdp(t["webSocketDebuggerUrl"]) as c:
        try:
            await c.call("Accessibility.enable")
        except CdpError:
            pass
        r = await c.call("Accessibility.getFullAXTree")
    return r.get("nodes", []) or []


async def direct_activate(port: int, target: str | None, name: str, role: str | None) -> dict:
    tgts = http_list(port)
    t = _pick_page_target(tgts, target)
    if t is None:
        raise CdpError(f"no page target on port {port}")
    async with Cdp(t["webSocketDebuggerUrl"]) as c:
        try:
            await c.call("Accessibility.enable")
        except CdpError:
            pass
        r = await c.call("Accessibility.getFullAXTree")
        hit = _match((r.get("nodes") or []), name, role)
        if hit is None:
            return {"ok": False, "error": "not found", "name": name}
        res = await c.call("DOM.resolveNode", {"backendNodeId": hit["backendDOMNodeId"]})
        oid = (res.get("object") or {}).get("objectId")
        if not oid:
            return {"ok": False, "error": "no objectId", "name": name}
        try:
            clicked = await c.call(
                "Runtime.callFunctionOn",
                {"objectId": oid, "functionDeclaration": "function(){ this.click(); }",
                 "returnByValue": True},
            )
        finally:
            try:
                await c.call("Runtime.releaseObject", {"objectId": oid})
            except CdpError:
                pass
        if clicked.get("exceptionDetails"):
            return {"ok": False, "error": "click threw", "name": name,
                    "detail": json.dumps(clicked["exceptionDetails"])[:300]}
        return {"ok": True, "name": name, "role": hit["_role"]}


# ── Electron transport (via the Node inspector) ──────────────────────────────

_MAIN_LIST_RENDERERS = """
  const { webContents } = process.mainModule.require('electron');
  return webContents.getAllWebContents().map(w => ({
    id: w.id, type: w.getType(), url: (w.getURL()||'').slice(0,120),
    title: (w.getTitle()||'').slice(0,80)
  }));
"""

_MAIN_AX = """
  const { webContents } = process.mainModule.require('electron');
  const w = webContents.getAllWebContents().find(x => x.id === __WCID__);
  if (!w) return { error: 'webContents gone' };
  // Always detach on the way out. `webContents.debugger` is sticky: if a previous
  // client attached and then vanished, the webContents stays attached to an
  // orphaned session, and every later sendCommand on it silently returns an empty
  // result. Measured: the same query returned 1026 nodes on a fresh process and
  // 11 after an earlier caller had attached. The widely-copied
  // "attach only if !isAttached()" pattern is what makes this worse.
  if (w.debugger.isAttached()) { try { w.debugger.detach(); } catch (e) {} }
  w.debugger.attach('1.3');
  try {
  try { await w.debugger.sendCommand('Accessibility.enable'); } catch (e) {}
  const r = await w.debugger.sendCommand('Accessibility.getFullAXTree');
  return { nodes: (r.nodes || []).map(n => ({
      role: (n.role && n.role.value) || '',
      name: (n.name && n.name.value) || '',
      backendDOMNodeId: n.backendDOMNodeId || 0 })) };
  } finally { try { if (w.debugger.isAttached()) w.debugger.detach(); } catch (e) {} }
"""

_MAIN_CLICK = """
  const { webContents } = process.mainModule.require('electron');
  const w = webContents.getAllWebContents().find(x => x.id === __WCID__);
  if (!w) return { error: 'webContents gone' };
  if (w.debugger.isAttached()) { try { w.debugger.detach(); } catch (e) {} }
  w.debugger.attach('1.3');
  try {
  try { await w.debugger.sendCommand('Accessibility.enable'); } catch (e) {}
  const tree = await w.debugger.sendCommand('Accessibility.getFullAXTree');
  const want = __WANT__;
  const role = __ROLE__;
  const hit = (tree.nodes || []).find(n =>
    (n.role && n.role.value) &&
    ((n.name && n.name.value) === want.name) &&
    (role === null || n.role.value === role));
  if (!hit || !hit.backendDOMNodeId) {
    const anyBtn = (tree.nodes || []).filter(n => n.role && n.role.value === 'button');
    return { error: 'not found', roleHint: role,
             availableButtons: anyBtn.slice(0,10).map(n => (n.name && n.name.value) || '') };
  }
  const res = await w.debugger.sendCommand('DOM.resolveNode',
    { backendNodeId: hit.backendDOMNodeId });
  const oid = res && res.object && res.object.objectId;
  if (!oid) return { error: 'no objectId' };
  let out = null;
  try {
    out = await w.debugger.sendCommand('Runtime.callFunctionOn',
      { objectId: oid, functionDeclaration: 'function(){ this.click(); }', returnByValue: true });
  } finally {
    try { await w.debugger.sendCommand('Runtime.releaseObject', { objectId: oid }); } catch (e) {}
  }
  return { ok: true, name: (hit.name && hit.name.value) || '',
           role: hit.role.value,
           threw: !!(out && out.exceptionDetails),
           detail: out && out.exceptionDetails ? JSON.stringify(out.exceptionDetails).slice(0,300) : null };
  } finally { try { if (w.debugger.isAttached()) w.debugger.detach(); } catch (e) {} }
"""

_MAIN_FILL = """
  const { webContents } = process.mainModule.require('electron');
  const w = webContents.getAllWebContents().find(x => x.id === __WCID__);
  if (!w) return { error: 'webContents gone' };
  if (w.debugger.isAttached()) { try { w.debugger.detach(); } catch (e) {} }
  w.debugger.attach('1.3');
  try {
  const script =
    'function(v){ var el=this;' +
    ' var proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype' +
    '            : (el instanceof HTMLSelectElement ? HTMLSelectElement.prototype' +
    '            : HTMLInputElement.prototype);' +
    ' var d = Object.getOwnPropertyDescriptor(proto, "value");' +
    ' if (!d || !d.set) return "NO_SETTER";' +
    ' d.set.call(el, v);' +
    ' el.dispatchEvent(new Event("input", {bubbles:true}));' +
    ' el.dispatchEvent(new Event("change", {bubbles:true}));' +
    ' return el.value; }';
  let result = null;
  if (__WANT_NAME__) {
    const r = await w.debugger.sendCommand('Runtime.evaluate', { expression: __JSFIND__, returnByValue: true });
    const oid = (r.get('result') or {}).get('objectId');
    if (!oid) return { error: 'no field matched' };
    try {
      const o = await w.debugger.sendCommand('Runtime.callFunctionOn',
        { objectId: oid, functionDeclaration: script + '})', arguments: [{ value: __TEXT__ }],
          returnByValue: true });
      result = (o.get('result') or {}).get('value');
      if (o.get('exceptionDetails')) return { error: 'fill threw' };
    } finally { try { await w.debugger.sendCommand('Runtime.releaseObject', { objectId: oid }); } catch (e) {} }
  } else {
    const r = await w.debugger.sendCommand('Runtime.evaluate',
      { expression: 'document.activeElement ? document.activeElement.value : null', returnByValue: true });
    result = (r.get('result') or {}).get('value');
  }
  return { observed: result };
  } finally { try { if (w.debugger.isAttached()) w.debugger.detach(); } catch (e) {} }
"""

_JS_FIND = (
    "[...document.querySelectorAll('input,textarea,[contenteditable=\\\"true\\\"]')]"
    ".find(e => (e.getAttribute('placeholder')||e.getAttribute('aria-label')||"
    "e.name||e.id||'') === __NEEDLE__) || null"
)


class Electron:
    """A connection to an Electron main process, with renderer operations."""

    def __init__(self, port: int):
        self.port = port
        t = inspector_main_target(port)
        if t is None:
            raise CdpError(f"no Node main-process target on port {port}")
        self.ws_url = t["webSocketDebuggerUrl"]
        self._cdp = Cdp(self.ws_url)

    async def __aenter__(self):
        await self._cdp.__aenter__()
        return self

    async def __aexit__(self, *exc):
        await self._cdp.__aexit__(*exc)

    async def renderers(self) -> list[dict]:
        v = await self.main_eval(_MAIN_LIST_RENDERERS)
        return v if isinstance(v, list) else []

    async def main_eval(self, body: str) -> Any:
        """Evaluate in the main process, awaiting promises and JSON round-tripping.

        Bare `require` is not a global in the CDP eval scope, which is why the
        bodies above go through `process.mainModule.require`.
        """
        expr = (
            "globalThis.__r = (async () => { const __v = await (async () => {"
            + body
            + "})(); return JSON.stringify(__v === undefined ? null : __v); })(); globalThis.__r;"
        )
        r = await self._cdp.call(
            "Runtime.evaluate",
            {"expression": expr, "awaitPromise": True, "returnByValue": True},
        )
        if r.get("exceptionDetails"):
            raise CdpError("main eval threw: " + json.dumps(r["exceptionDetails"])[:300])
        v = (r.get("result") or {}).get("value")
        return json.loads(v) if isinstance(v, str) else v

    async def pick_renderer(self, target: str | None) -> int | None:
        rs = await self.renderers()
        if not rs:
            return None
        if target:
            for r in rs:
                if target in (r.get("url") or "") or target == (r.get("title") or ""):
                    return r["id"]
            return None
        for r in rs:
            if r.get("type") == "window":
                return r["id"]
        return rs[0]["id"]

    async def ax(self, wcid: int) -> list[dict]:
        """Fetch the accessibility tree, warming it if the first read is partial.

        Measured on this machine: the FIRST `Accessibility.getFullAXTree` after
        attaching the debugger returns 11 nodes for VS Code; the second, moments
        later, returns 1026. Chromium builds the tree lazily, so a cold read sees
        only what is realised.

        Left uncorrected this is the worst kind of bug for this tier: it is not an
        error, it is a *small answer*. Every locate would miss most of the app
        while reporting success. So a suspiciously small first read is retried and
        the largest tree wins. A genuinely small app converges immediately because
        the retry budget is small and bounded.
        """
        body = _MAIN_AX.replace("__WCID__", str(wcid))
        best: list[dict] = []
        for _ in range(AX_WARM_ATTEMPTS):
            v = await self.main_eval(body)
            if not isinstance(v, dict) or v.get("error"):
                raise CdpError((v or {}).get("error", "ax failed"))
            nodes = v["nodes"]
            if len(nodes) > len(best):
                best = nodes
            if len(best) >= AX_WARM_ENOUGH:
                break
            await asyncio.sleep(0.15)
        return best

    async def click(self, wcid: int, name: str, role: str | None) -> dict:
        body = (
            _MAIN_CLICK.replace("__WCID__", str(wcid))
            .replace("__WANT__", json.dumps({"name": name}))
            .replace("__ROLE__", json.dumps(role))
        )
        v = await self.main_eval(body)
        if not isinstance(v, dict) or v.get("error"):
            return {"ok": False, "error": (v or {}).get("error", "click failed"), "name": name}
        return v

    async def fill(self, wcid: int, name: str | None, text: str) -> dict:
        find_expr = _JS_FIND.replace("__NEEDLE__", json.dumps(name)) if name else "null"
        body = (
            _MAIN_FILL.replace("__WCID__", str(wcid))
            .replace("__WANT_NAME__", "true" if name else "false")
            .replace("__JSFIND__", json.dumps(find_expr))
            .replace("__TEXT__", json.dumps(text))
        )
        v = await self.main_eval(body)
        if not isinstance(v, dict) or v.get("error"):
            return {"ok": False, "error": (v or {}).get("error", "fill failed")}
        return {"ok": v.get("observed") == text, "observed": v.get("observed")}


# ── Matching ─────────────────────────────────────────────────────────────────

def _ax_val(v: Any) -> str:
    if isinstance(v, dict):
        return str(v.get("value") or "")
    return str(v or "")


def _normalise(nodes: list[dict]) -> list[dict]:
    out = []
    for n in nodes[:AX_BUDGET]:
        out.append(
            {
                "_role": _ax_val(n.get("role")),
                "name": _ax_val(n.get("name")),
                "backendDOMNodeId": n.get("backendDOMNodeId") or 0,
            }
        )
    return out


def _match(nodes: list[dict], name: str, role: str | None) -> dict | None:
    """Exact accessible-name match; role filters when given. Falls back to name-only."""
    fallback = None
    for n in _normalise(nodes):
        if n["name"] != name:
            continue
        if role and n["_role"] != role:
            if fallback is None:
                fallback = n
            continue
        return n
    return fallback


# ── HTTP ─────────────────────────────────────────────────────────────────────

def _err(reason: str, code: int, **extra):
    return JSONResponse({"error": reason, **extra}, status_code=code)


@app.get("/health")
async def health() -> JSONResponse:
    if not HAVE_WEBSOCKETS:
        return JSONResponse(
            {"status": "unavailable", "reason": "pip install websockets"}, status_code=503
        )
    r = await run_in_threadpool(reachable)
    return JSONResponse(
        {
            "status": "ok" if r["any"] else "idle",
            "service": "nexus-cdp",
            "direct_ports": r["direct_ports"],
            "electron_ports": r["electron_ports"],
            "probed": probe_ports(),
            "note": (
                "idle means no browser is listening on a debug port. Chromium must be "
                "launched with --remote-debugging-port; Electron apps with --inspect "
                "(NOT --remote-debugging-port, which Electron's CDP gate treats as "
                "unauthenticated and terminates on). An already-running Electron app "
                "cannot be attached after the fact."
            ),
        }
    )


@app.get("/targets")
async def targets() -> JSONResponse:
    r = await run_in_threadpool(reachable)
    out = []
    for p in r["direct_ports"]:
        for t in http_list(p):
            out.append({"port": p, "mode": "direct", "type": t.get("type"),
                        "title": t.get("title"), "url": (t.get("url") or "")[:140]})
    for p in r["electron_ports"]:
        try:
            async with Electron(p) as e:
                for rc in await e.renderers():
                    out.append({"port": p, "mode": "electron", "type": rc.get("type"),
                                "title": rc.get("title"), "url": (rc.get("url") or "")[:140]})
        except CdpError as e:
            out.append({"port": p, "mode": "electron", "error": str(e)})
    return JSONResponse({"targets": out, "any": r["any"]})


async def _collect(port: int, target: str | None) -> list[dict]:
    if port in reachable()["direct_ports"]:
        return await direct_ax(port, target)
    async with Electron(port) as e:
        wcid = await e.pick_renderer(target)
        if wcid is None:
            raise CdpError(f"no renderer on port {port}")
        return await e.ax(wcid)


@app.get("/tree")
async def tree(request: Request) -> JSONResponse:
    if not HAVE_WEBSOCKETS:
        return _err("websockets not installed", 503)
    q = request.query_params
    port = int(q.get("port") or 0)
    target = q.get("target")
    r = await run_in_threadpool(reachable)
    ports = [port] if port else (r["direct_ports"] + r["electron_ports"])
    if not ports:
        return _err("no CDP endpoint reachable", 503, detail="launch the app with a debug port")
    t0 = time.monotonic()
    try:
        nodes = await _collect(ports[0], target)
    except CdpError as e:
        return _err(str(e), 502)
    norm = _normalise(nodes)
    return JSONResponse(
        {
            "port": ports[0],
            "nodes": norm,
            "count": len(norm),
            "total": len(nodes),
            "truncated": len(nodes) > AX_BUDGET,
            "latency_ms": int((time.monotonic() - t0) * 1000),
        }
    )


@app.get("/find")
async def find(request: Request) -> JSONResponse:
    if not HAVE_WEBSOCKETS:
        return _err("websockets not installed", 503)
    q = request.query_params
    name = q.get("name", "")
    if not name:
        return _err("name required", 400)
    role = q.get("role") or None
    port = int(q.get("port") or 0)
    target = q.get("target")
    r = await run_in_threadpool(reachable)
    ports = [port] if port else (r["direct_ports"] + r["electron_ports"])
    if not ports:
        return _err("no CDP endpoint reachable", 503)
    t0 = time.monotonic()
    try:
        nodes = await _collect(ports[0], target)
    except CdpError as e:
        return _err(str(e), 502)
    hit = _match(nodes, name, role)
    if hit is None:
        buttons = sorted({n["_role"] for n in _normalise(nodes) if n["_role"]})
        return _err(f"not found: {name}", 404, name=name, roles_seen=buttons[:24])
    return JSONResponse(
        {
            "port": ports[0],
            "name": hit["name"],
            "role": hit["_role"],
            "backendDOMNodeId": hit["backendDOMNodeId"],
            "latency_ms": int((time.monotonic() - t0) * 1000),
        }
    )


@app.post("/activate")
async def activate(request: Request) -> JSONResponse:
    """Invoke an element's click. No pixels, no compositor, no portal."""
    if not HAVE_WEBSOCKETS:
        return _err("websockets not installed", 503)
    body = await _json(request)
    name = body.get("name", "")
    if not name:
        return _err("name required", 400)
    role = body.get("role") or None
    port = int(body.get("port") or 0)
    target = body.get("target")
    r = await run_in_threadpool(reachable)
    ports = [port] if port else (r["direct_ports"] + r["electron_ports"])
    if not ports:
        return _err("no CDP endpoint reachable", 503)

    t0 = time.monotonic()
    p = ports[0]
    try:
        if p in r["direct_ports"]:
            out = await direct_activate(p, target, name, role)
        else:
            async with Electron(p) as e:
                wcid = await e.pick_renderer(target)
                if wcid is None:
                    return _err("no renderer", 502)
                out = await e.click(wcid, name, role)
    except CdpError as e:
        return _err(str(e), 502)

    if not out.get("ok"):
        return _err(out.get("error", "click failed"), 404, name=name,
                    available=out.get("availableButtons"))
    out["port"] = p
    out["latency_ms"] = int((time.monotonic() - t0) * 1000)
    return JSONResponse(out)


@app.post("/fill")
async def fill(request: Request) -> JSONResponse:
    """Set an input's value via the prototype's native setter.

    Assigning `.value` directly does not notify React/Vue, which track their own
    state — the value would appear set and the framework would ignore it. Walking
    to the native setter and dispatching input/change is what makes the write
    real.
    """
    if not HAVE_WEBSOCKETS:
        return _err("websockets not installed", 503)
    body = await _json(request)
    if "text" not in body:
        return _err("text required", 400)
    name = body.get("name") or None
    text = str(body["text"])
    port = int(body.get("port") or 0)
    target = body.get("target")
    r = await run_in_threadpool(reachable)
    ports = [port] if port else (r["direct_ports"] + r["electron_ports"])
    if not ports:
        return _err("no CDP endpoint reachable", 503)
    p = ports[0]
    t0 = time.monotonic()
    try:
        if p in r["direct_ports"]:
            out = await _direct_fill(p, target, name, text)
        else:
            async with Electron(p) as e:
                wcid = await e.pick_renderer(target)
                if wcid is None:
                    return _err("no renderer", 502)
                out = await e.fill(wcid, name, text)
    except CdpError as e:
        return _err(str(e), 502)
    out["port"] = p
    out["latency_ms"] = int((time.monotonic() - t0) * 1000)
    return JSONResponse(out)


_SET_JS = (
    "function(v){ var el=this;"
    " var proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype"
    "   : (el instanceof HTMLSelectElement ? HTMLSelectElement.prototype"
    "   : HTMLInputElement.prototype);"
    " var d = Object.getOwnPropertyDescriptor(proto,'value');"
    " if(!d||!d.set) return 'NO_SETTER';"
    " d.set.call(el,v);"
    " el.dispatchEvent(new Event('input',{bubbles:true}));"
    " el.dispatchEvent(new Event('change',{bubbles:true}));"
    " return el.value; }"
)


async def _direct_fill(port: int, target: str | None, name: str | None, text: str) -> dict:
    tgts = http_list(port)
    t = _pick_page_target(tgts, target)
    if t is None:
        raise CdpError(f"no page target on port {port}")
    async with Cdp(t["webSocketDebuggerUrl"]) as c:
        find_expr = (
            "[...document.querySelectorAll('input,textarea,[contenteditable=\\\"true\\\"]')]"
            ".find(e => (e.getAttribute('placeholder')||e.getAttribute('aria-label')||"
            "e.name||e.id||'') === " + json.dumps(name) + ") || null"
        ) if name else "document.activeElement"
        r = await c.call("Runtime.evaluate", {"expression": find_expr, "returnByValue": True})
        oid = (r.get("result") or {}).get("objectId")
        if not oid:
            return {"ok": False, "error": "no field matched"}
        try:
            o = await c.call(
                "Runtime.callFunctionOn",
                {
                    "objectId": oid,
                    "functionDeclaration": _SET_JS,
                    "arguments": [{"value": text}],
                    "returnByValue": True,
                },
            )
        finally:
            try:
                await c.call("Runtime.releaseObject", {"objectId": oid})
            except CdpError:
                pass
        if o.get("exceptionDetails"):
            return {"ok": False, "error": "fill threw"}
        observed = (o.get("result") or {}).get("value")
        return {"ok": observed == text, "observed": observed}


async def _json(request: Request) -> dict:
    try:
        return await request.json()
    except Exception:
        return {}


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(app, host="127.0.0.1", port=int(os.environ.get("CDP_SERVER_PORT", "39222")))
