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
import re
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


def browser_ws_url(port: int) -> str | None:
    """The BROWSER-level websocket for a DevTools port.

    `Browser.getVersion` and `SystemInfo.getInfo` are browser-level domains:
    a page session answers "domain not found" for them.
    """
    try:
        data = _http_json(f"http://127.0.0.1:{port}/json/version")
    except (urllib.error.URLError, OSError, ValueError, TimeoutError):
        return None
    return data.get("webSocketDebuggerUrl") if isinstance(data, dict) else None


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


_FOCUS_STATE_JS = (
    "JSON.stringify({focus: document.hasFocus(), vis: document.visibilityState})"
)

# The focus sweep: a port's candidate targets are probed under ONE deadline
# for the WHOLE sweep. HOW they are probed depends on the transport:
#
#   * Chromium — CONCURRENTLY. Every probe opens its OWN CDP connection, so
#     a browser with N tabs costs one sweep rather than N sequential probes.
#     A background tab the browser has FROZEN (Chrome/Brave Memory Saver
#     freezes background tabs, and they stop answering `Runtime.evaluate`)
#     can therefore never hold the request hostage.
#
#   * Electron — ONE AFTER ANOTHER, because every probe shares the single
#     inspector websocket, and a socket without reply demultiplexing cannot
#     serve two requests at once (the replies would be stolen). Each
#     candidate gets its OWN SLICE of the remaining budget —
#     `remaining / candidates_left`, recomputed before every probe — so a
#     frozen window can only eat its own share and the window the user is
#     actually looking at is still probed.
FOCUS_SWEEP_DEADLINE = float(os.environ.get("CDP_FOCUS_SWEEP", "6"))

# The whole request, across every reachable port.
#
# The Rust client gives the request 20 s (`src-tauri/src/cdp.rs`), and the
# request has to finish inside 15 s: the sweep is repeated per port, so the
# ports share one budget.
ACTIVATE_BUDGET = float(os.environ.get("CDP_ACTIVATE_BUDGET", "12"))

# "I could not tell": the probe did not answer, or the transport failed.
_FOCUS_UNKNOWN = {
    "focus": False,
    "visible": False,
    "known": False,
    "timed_out": False,
}

# "The probe did not answer IN TIME" — a probe that never finished has proved
# nothing, exactly like a vanished target.
_FOCUS_TIMED_OUT = {
    "focus": False,
    "visible": False,
    "known": False,
    "timed_out": True,
}


async def _page_focus_state_inner(t: dict) -> dict:
    """The probe itself — see `_page_focus_state` for the contract."""
    async with Cdp(t["webSocketDebuggerUrl"]) as c:
        r = await c.call(
            "Runtime.evaluate",
            {"expression": _FOCUS_STATE_JS, "returnByValue": True},
        )
    v = (r.get("result") or {}).get("value")
    try:
        d = json.loads(v) if isinstance(v, str) else (v or {})
    except (TypeError, ValueError):
        return dict(_FOCUS_UNKNOWN)
    if not isinstance(d, dict):
        return dict(_FOCUS_UNKNOWN)
    return {
        "focus": bool(d.get("focus")),
        "visible": d.get("vis") == "visible",
        "known": True,
        "timed_out": False,
    }


async def _page_focus_state(t: dict, deadline: float = FOCUS_SWEEP_DEADLINE) -> dict:
    """`hasFocus()` / `visibilityState` for one page target.

    A BACKGROUND tab of the same browser answers hasFocus() = false and
    visibilityState = "hidden"; the tab the user is looking at answers
    true / "visible". That is how the foreground target is found.

    The probe is BOUNDED and never fails the request: a frozen target, a
    vanished target or a refused connection are all "I cannot tell", which
    is reported as `known: False`.
    """
    try:
        return await asyncio.wait_for(
            _page_focus_state_inner(t), timeout=deadline
        )
    except asyncio.TimeoutError:
        # A frozen background tab, or a busy renderer that did not answer in
        # time. It has proved nothing.
        return dict(_FOCUS_TIMED_OUT)
    except CdpError:
        # The target is gone, or the transport failed.
        return dict(_FOCUS_UNKNOWN)
    except Exception as e:  # noqa: BLE001 — a connect failure is "unknown" too
        # `websockets.connect` raises OSError / WebSocketException, neither of
        # which is a CdpError. Those must not escape and turn the intended
        # 409 into a 500.
        log.warning("focus probe failed for %s: %s", t.get("url", ""), e)
        return dict(_FOCUS_UNKNOWN)


async def _probe_all(targets: list[dict], deadline: float) -> list[dict]:
    """Probe every candidate CONCURRENTLY, under ONE deadline for the sweep.

    The probes run in parallel, so the sweep costs one deadline and not one
    deadline per candidate. Whatever answered in time is kept; a probe that
    did not finish is unknown.
    """
    if not targets:
        return []

    async def one(t: dict) -> dict:
        return await _page_focus_state(t, deadline)

    tasks = [asyncio.ensure_future(one(t)) for t in targets]
    try:
        gathered = list(
            await asyncio.wait_for(
                asyncio.gather(*tasks, return_exceptions=True), timeout=deadline
            )
        )
        # A cancelled child comes back as an exception, not a state: it is
        # "I could not tell", like any other non-answer.
        return [
            s if isinstance(s, dict) else dict(_FOCUS_UNKNOWN)
            for s in gathered
        ]
    except asyncio.TimeoutError:
        pass

    # The sweep ran out of time: whatever answered is kept, the rest never
    # finished and has proved nothing.
    states: list[dict] = []
    for t, task in zip(targets, tasks):
        if task.done() and not task.cancelled():
            exc = task.exception()
            if exc is None:
                states.append(task.result())
            else:
                states.append(dict(_FOCUS_UNKNOWN))
        else:
            task.cancel()
            states.append(dict(_FOCUS_TIMED_OUT))
    return states


def narrow_by_title(candidates: list[dict], expected_title: str) -> list[dict]:
    """The candidate set: the targets whose title matches the expected
    foreground window.

    The predicate is the SAME one the identity gate uses (`title_matches`),
    so the narrowing can never be stricter than the gate it pre-filters: a
    candidate whose own title is EMPTY cannot contradict the expected one, so
    it is kept and the focus probe decides.

    Pure, so the rule is unit-tested with fakes.
    """
    if not (expected_title or "").strip():
        return list(candidates)
    return [
        t
        for t in candidates
        if title_matches(expected_title, str(t.get("title") or ""))
    ]


async def chromium_foreground_target(
    port: int,
    want: str | None,
    expected_title: str = "",
    deadline: float = FOCUS_SWEEP_DEADLINE,
) -> tuple[dict | None, str]:
    """The page target the user is actually looking at, and why not.

    The accessibility tree is per TARGET, so the identity check and the
    action have to use the SAME one: the first non-blank page target is
    usually a BACKGROUND tab, and clicking in it is exactly the bug the
    identity check exists to prevent.

    The rule (see `choose_focused_target`): the candidate set is the targets
    whose title matches the expected foreground title, ALL of them are probed
    concurrently under one deadline, and a target is chosen ONLY when exactly
    one of them proved it has the focus.

    The probes may run concurrently here because EACH ONE opens its own CDP
    connection: there is no shared socket whose replies could be mixed up
    (the Electron transport shares one — see `Electron._probe_renderers`).

    Returns (None, reason) when the foreground cannot be established.
    """
    pages = [
        t
        for t in http_list(port)
        if t.get("type") == "page" and t.get("webSocketDebuggerUrl")
    ]
    if not pages:
        return None, "the browser exposes no page target"
    if want:
        pages = [
            t
            for t in pages
            if want in (t.get("url") or "") or want == (t.get("title") or "")
        ]
        if not pages:
            return None, "the requested target is not open"
    candidates = narrow_by_title(pages, expected_title)
    if not candidates:
        return None, "no target matched the application in front"
    states = await _probe_all(candidates, deadline)
    chosen = choose_focused_target(candidates, states)
    if chosen is None:
        return None, foreground_reason(candidates, states)
    return chosen, ""


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


async def direct_activate(t: dict, name: str, role: str | None) -> dict:
    """Click by name inside a RESOLVED page target.

    The target is passed in, not chosen here: the caller resolved the
    foreground target first so that the identity check and this action
    operate on the same one.
    """
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


_MAIN_FOCUS = """
  const { webContents } = process.mainModule.require('electron');
  const w = webContents.getAllWebContents().find(x => x.id === __WCID__);
  if (!w) return null;
  const raw = await w.executeJavaScript(
    "JSON.stringify({focus: document.hasFocus(), vis: document.visibilityState})"
  );
  return JSON.parse(raw);
"""


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

    async def app_name(self) -> str:
        """The Electron application's own name ("Visual Studio Code").

        The main process knows it; nothing else does. It is what proves that
        an inspector port belongs to the application the user has in front.
        """
        v = await self.main_eval(
            "process.mainModule.require('electron').app.getName()"
        )
        return str(v or "")

    @staticmethod
    def pick_renderer_info(rs: list[dict], target: str | None) -> dict | None:
        """The renderer to act on, from the renderer's own list."""
        if not rs:
            return None
        if target:
            for r in rs:
                if target in (r.get("url") or "") or target == (r.get("title") or ""):
                    return r
            return None
        for r in rs:
            if r.get("type") == "window":
                return r
        return rs[0]

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
        r = self.pick_renderer_info(rs, target)
        return r["id"] if r is not None else None

    async def renderer_focus(self, wcid: int, deadline: float = FOCUS_SWEEP_DEADLINE) -> dict:
        """`hasFocus()` / `visibilityState` for one renderer.

        A renderer of a background window answers hasFocus() = false; the
        window the user is looking at answers true / "visible".

        Bounded, like the Chromium probe: a renderer that does not answer
        (a frozen window, a vanished target, a refused connection) is
        "I cannot tell", never a stall and never a failed request.
        """
        body = _MAIN_FOCUS.replace("__WCID__", str(wcid))
        try:
            v = await asyncio.wait_for(
                self.main_eval(body), timeout=deadline
            )
        except asyncio.TimeoutError:
            # A busy renderer that did not answer in time. It has proved
            # nothing.
            return dict(_FOCUS_TIMED_OUT)
        except CdpError:
            # The renderer is gone, or the transport failed.
            return dict(_FOCUS_UNKNOWN)
        except Exception as e:  # noqa: BLE001 — a connect failure is "unknown" too
            log.warning("renderer focus probe failed for %s: %s", wcid, e)
            return dict(_FOCUS_UNKNOWN)
        if not isinstance(v, dict):
            return dict(_FOCUS_UNKNOWN)
        return {
            "focus": bool(v.get("focus")),
            "visible": v.get("vis") == "visible",
            "known": True,
            "timed_out": False,
        }

    async def foreground_renderer(
        self,
        want: str | None,
        expected_title: str = "",
        deadline: float = FOCUS_SWEEP_DEADLINE,
    ) -> tuple[dict | None, str]:
        """The renderer the user is actually looking at, and why not.

        The accessibility tree is per RENDERER, so the identity check and
        the action have to use the same one: the first renderer of type
        "window" is the first VS Code window, not the front one.

        The rule is the same as for Chromium (see `choose_focused_target`):
        the candidate set is the renderers whose title matches the expected
        foreground title, ALL of them are probed under ONE deadline, and a
        renderer is chosen ONLY when exactly one of them proved it has the
        focus.

        The probes run ONE AFTER ANOTHER, not concurrently: they share a
        single CDP websocket that cannot demultiplex two replies (see
        `_probe_renderers`). Each candidate gets its own slice of the
        remaining budget, so a frozen window cannot starve the one in front.

        Returns (None, reason) when the foreground cannot be established.
        """
        rs = await self.renderers()
        windows = [r for r in rs if r.get("type") == "window"] or rs
        if not windows:
            return None, "the application exposes no window"
        if want:
            windows = [
                r
                for r in windows
                if want in (r.get("url") or "") or want == (r.get("title") or "")
            ]
            if not windows:
                return None, "the requested window is not open"
        candidates = narrow_by_title(windows, expected_title)
        if not candidates:
            return None, "no window matched the application in front"
        states = await self._probe_renderers(candidates, deadline)
        chosen = choose_focused_target(candidates, states)
        if chosen is None:
            return None, foreground_reason(candidates, states)
        return chosen, ""

    async def _probe_renderers(self, candidates: list[dict], deadline: float) -> list[dict]:
        """Probe the candidate renderers ONE AFTER ANOTHER, under one deadline.

        The Electron transport shares ONE CDP websocket with the whole
        process (`self._cdp`), and `Cdp.call` has NO reply demultiplexing:
        a message whose id is not the caller's is DISCARDED, and the
        websockets library refuses two concurrent `recv()`s on one
        connection. Two probes in flight at once would therefore steal each
        other's replies — the first waiter would consume the other probe's
        answer, throw it away, and wait for a reply that no longer exists.
        Both would be reported unknown, `choose_focused_target` would
        return nothing, and the click would be refused even though the
        control IS in the focused window.

        So the probes run sequentially, under the SAME overall sweep
        deadline: the moment the deadline is reached, the remaining
        candidates are unknown — nothing is proved — and the sweep is over.

        Each candidate gets its OWN SLICE of what is left —
        `remaining / candidates_left`, recomputed before every probe. The
        candidates are NOT focus-ordered (`webContents.getAllWebContents()`
        is not), so without a slice the FIRST candidate would take the whole
        budget: one hung window would starve every window after it, and the
        window the user is actually looking at would never be probed. With a
        slice, a frozen window can only eat its own share.
        """
        if not candidates:
            return []

        end = time.monotonic() + deadline
        states: list[dict] = []
        for index, r in enumerate(candidates):
            remaining = end - time.monotonic()
            if remaining <= 0:
                # Out of time: this candidate and every one after it are
                # unknown. Nothing is proved about them.
                rest = candidates[len(states):]
                states.extend(dict(_FOCUS_TIMED_OUT) for _ in rest)
                break
            # This candidate's own share of what is left.
            left = len(candidates) - index
            states.append(await self.renderer_focus(r["id"], remaining / left))
        return states

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


# ── Endpoint identity ───────────────────────────────────────────────────────
#
# The CDP tier is scoped by DEBUG PORT, not by application: it sees whichever
# app holds the lowest reachable port. That is not enough — a background app
# launched with --inspect would answer a click meant for the app in front, and
# the assistant would report success for an action in an application the user
# never referenced.
#
# So every endpoint reports WHO it is, and a request that names the expected
# application is refused unless the endpoint proves it belongs to it.

def _app_name_from_command_line(cmdline: str) -> str:
    """The application name from a browser command line.

        /usr/bin/brave --remote-debugging-port=9222            -> "brave"
        /home/me/My Apps/brave --remote-debugging-port=9222    -> "brave"
        "C:\\Program Files\\Brave\\brave.exe" --inspect=9229    -> "brave"

    The executable is the path up to the first ARGUMENT (a token that starts
    with "--"), NOT up to the first space: a path may contain spaces, and
    splitting on the first space would yield "C:\\Program" or "/home/me/My".
    A double-quoted path is taken as-is, quotes included.
    """
    s = (cmdline or "").strip()
    if not s:
        return ""
    if s.startswith('"'):
        # A quoted path: the executable is what is inside the quotes.
        end = s.find('"', 1)
        exe = s[1:end] if end > 0 else s[1:]
    else:
        # The executable is everything before the first "--" argument.
        cut = len(s)
        idx = 0
        while True:
            idx = s.find("--", idx)
            if idx < 0:
                break
            # Only a token START counts: the "--" has to begin a word.
            if idx == 0 or s[idx - 1].isspace():
                cut = idx
                break
            idx += 1
        exe = s[:cut].strip()
    base = exe.replace("\\", "/").rsplit("/", 1)[-1]
    return base.removesuffix(".exe").lower()


def _name_tokens(name: str) -> set[str]:
    return {t for t in re.split(r"[^a-z0-9]+", (name or "").lower()) if t}


def app_names_match(expected: str, actual: str) -> bool:
    """Whether two application names can refer to the same application.

    "Visual Studio Code" and "code" are the same application; "brave" and
    "code" are not. A shared word is enough, and so is one name contained in
    the other ("code" inside "vscode").

    An empty name on either side never matches: an unidentified application
    is not evidence of anything.
    """
    e, a = _name_tokens(expected), _name_tokens(actual)
    if not e or not a:
        return False
    if e & a:
        return True
    en = re.sub(r"[^a-z0-9]+", "", (expected or "").lower())
    an = re.sub(r"[^a-z0-9]+", "", (actual or "").lower())
    return bool(en) and bool(an) and (en in an or an in en)


def title_matches(foreground_title: str, target_title: str) -> bool:
    """Whether the target's title belongs to the foreground window.

    A browser window is titled "<page title> - <app>", so the target's title
    is contained in the window title. Two empty titles cannot contradict each
    other, so they match; a mismatch in either direction does not.
    """
    f = (foreground_title or "").strip().lower()
    t = (target_title or "").strip().lower()
    if not f or not t:
        return True
    return f == t or t in f or f in t


def choose_focused_target(candidates: list[dict], states: list[dict]) -> dict | None:
    """The target the user is looking at: the one that PROVED it has focus.

    The rule is deliberately narrow, because every extra heuristic opened a
    new background-window case:

      * exactly one candidate answered `known` AND `focus` → that one;
      * zero focused, more than one focused, or everything unknown → None,
        and the caller refuses with 409.

    Two things that are NOT evidence of being in front:

      * "visible" — a background window's active tab is visible, it is just
        not focused. Selecting on visibility is what put a click in a
        background window;
      * a probe that timed out — it proved nothing at all.

    Pure, so the rule is unit-tested with fakes.
    """
    focused = [
        t
        for t, st in zip(candidates, states)
        if st.get("known") and st.get("focus")
    ]
    if len(focused) == 1:
        return focused[0]
    return None


def foreground_reason(candidates: list[dict], states: list[dict]) -> str:
    """Why no foreground target could be chosen — for the refusal message."""
    if not candidates:
        return "no target matched the application in front"
    focused = [
        t for t, st in zip(candidates, states) if st.get("known") and st.get("focus")
    ]
    if len(focused) > 1:
        return f"{len(focused)} targets claim the focus"
    if not any(st.get("known") for st in states):
        if all(st.get("timed_out") for st in states):
            return "no target answered the focus probe in time"
        return "no target answered the focus probe"
    return "no target reports the focus"


def app_matches(expected_app: str, identity: dict) -> bool:
    """Whether a debug endpoint PROVES it belongs to the application in front.

    The endpoint has to name itself, and the name has to match. An endpoint
    that cannot name itself proves nothing and matches nothing.

    This is the cheap half of the gate: it does not need a renderer, so it
    runs BEFORE the focus sweep and an endpoint belonging to another program
    is never probed.
    """
    if not expected_app:
        return True
    return app_names_match(expected_app, str(identity.get("app") or ""))


def endpoint_matches(
    expected_app: str,
    expected_title: str,
    identity: dict,
    target: dict | None,
) -> bool:
    """Whether an endpoint, and the target resolved on it, may act.

    `target` is None when the foreground target could not be established,
    which is a refusal.
    """
    if target is None:
        return False
    if not app_matches(expected_app, identity):
        return False
    if expected_title and not title_matches(expected_title, str(target.get("title") or "")):
        return False
    return True


async def chromium_identity(port: int, target: str | None) -> dict:
    """Who a Chromium debug port belongs to.

    The APPLICATION part only: the title and URL belong to a target, and the
    target is resolved separately (the foreground one).
    """
    out = {"kind": "chromium", "app": "", "title": "", "url": "", "product": ""}
    ws = browser_ws_url(port)
    if ws:
        try:
            async with Cdp(ws) as c:
                try:
                    info = await c.call("SystemInfo.getInfo")
                    out["app"] = _app_name_from_command_line(str(info.get("commandLine") or ""))
                except CdpError:
                    pass
                if not out["app"]:
                    ver = await c.call("Browser.getVersion")
                    out["product"] = str(ver.get("product") or "")
                    out["app"] = out["product"].split("/")[0].lower()
        except CdpError:
            pass
        except Exception as e:  # noqa: BLE001 — a connect failure is "unknown"
            # `websockets.connect` raises OSError / WebSocketException, and
            # neither is a CdpError. An endpoint we cannot even connect to
            # cannot prove anything, and it must not turn the intended 409
            # into a 500.
            log.warning("could not read the browser identity on port %s: %s", port, e)
    return out


async def electron_identity(port: int, target: str | None) -> dict:
    """Who an Electron inspector port belongs to.

    The APPLICATION part only: the app name comes from the main process, and
    the target (renderer) is resolved separately.
    """
    out = {"kind": "electron", "app": "", "title": "", "url": "", "product": ""}
    try:
        async with Electron(port) as e:
            out["app"] = str(await e.app_name() or "")
    except CdpError:
        pass
    except Exception as e:  # noqa: BLE001 — a connect failure is "unknown"
        log.warning("could not read the electron identity on port %s: %s", port, e)
    return out


async def endpoint_identity(port: int, kind: str, target: str | None) -> dict:
    """The identity of one endpoint, by kind.

    The APPLICATION part does not depend on the target: it is the browser
    process itself. The TARGET part is filled in by the caller, which
    resolves the foreground target first.
    """
    if kind == "electron":
        return await electron_identity(port, target)
    return await chromium_identity(port, target)


async def foreground_target(
    port: int,
    kind: str,
    want: str | None,
    expected_title: str = "",
    deadline: float = FOCUS_SWEEP_DEADLINE,
) -> tuple[dict | None, str]:
    """The target the user is actually looking at, on this endpoint, and why not.

    (None, reason) means "not established" — zero or more than one candidate
    qualifies, or no candidate answered. The caller must treat that as a
    refusal.

    `deadline` is the budget for the WHOLE sweep on this port: every
    candidate is probed concurrently under it.
    """
    if kind == "electron":
        try:
            async with Electron(port) as e:
                return await e.foreground_renderer(want, expected_title, deadline)
        except CdpError:
            return None, "the inspector endpoint could not be reached"
        except Exception as e:  # noqa: BLE001 — a connect failure is "unknown"
            log.warning("electron foreground lookup failed on port %s: %s", port, e)
            return None, "the inspector endpoint could not be reached"
    return await chromium_foreground_target(port, want, expected_title, deadline)


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
    """Invoke an element's click. No pixels, no compositor, no portal.

    The request may name the application and the window title the caller
    believes are in front (`app`, `title`). The endpoint then acts ONLY on a
    debug port that PROVES it belongs to that same application: it refuses
    with 409 when no reachable endpoint can prove it, and it reports the
    identity it acted on so the caller can check it too.

    THE RULE (no heuristics, because every heuristic opened a new
    background-window case):

      1. the endpoint must name itself and the name must match the expected
         application (`app_matches`) — checked FIRST, because it is cheap and
         an endpoint of another program is never probed;
      2. the candidate set is the targets whose title matches the expected
         foreground title (`narrow_by_title`, the same `title_matches` rule
         the identity gate uses, so an EMPTY title cannot contradict the
         expected one and is kept);
      3. all candidates are probed under ONE deadline for the whole sweep
         per port — CONCURRENTLY for Chromium, where every probe has its
         own connection, and ONE AFTER ANOTHER for Electron, where the
         probes share a single CDP websocket that cannot demultiplex two
         replies (each candidate there gets its own slice of the remaining
         budget, so a frozen window cannot starve the one in front) — and
         the ports share one total budget (`ACTIVATE_BUDGET`) so the
         request stays inside 15 s;
      4. a target is chosen ONLY when EXACTLY ONE candidate answered
         `known` and `focus` — zero focused, more than one focused, or
         everything unknown all REFUSE with 409 and a reason.

    Without those fields the behaviour is the old one (the first reachable
    port) — but every current caller sends them.
    """
    if not HAVE_WEBSOCKETS:
        return _err("websockets not installed", 503)
    body = await _json(request)
    name = body.get("name", "")
    if not name:
        return _err("name required", 400)
    role = body.get("role") or None
    port = int(body.get("port") or 0)
    target = body.get("target")
    expected_app = str(body.get("app") or "").strip()
    expected_title = str(body.get("title") or "").strip()
    r = await run_in_threadpool(reachable)
    ports = [port] if port else (r["direct_ports"] + r["electron_ports"])
    if not ports:
        return _err("no CDP endpoint reachable", 503)

    # Pick the endpoint AND the target that belong to the application in front.
    #
    # The order matters: the FOREGROUND TARGET is resolved FIRST, and the
    # same target is used for the identity check and for the action. The
    # accessibility tree is per target, so checking the title of one target
    # and clicking another is not a check at all.
    #
    # The ports share ONE budget (`ACTIVATE_BUDGET`): the focus sweep is
    # repeated per port, and the whole request has to finish inside 15 s
    # (the Rust client gives up at 20 s). Each port's sweep gets what is
    # left of the budget, capped at one sweep deadline.
    budget_deadline = time.monotonic() + ACTIVATE_BUDGET
    chosen: int | None = None
    fg_target: dict | None = None
    identity: dict = {}
    reason = "no reachable debug endpoint belongs to the application in front"
    for p in ports:
        remaining = budget_deadline - time.monotonic()
        if remaining <= 0:
            reason = (
                "the request ran out of time before an endpoint could be checked"
            )
            break
        sweep = min(FOCUS_SWEEP_DEADLINE, remaining)
        kind = "chromium" if p in r["direct_ports"] else "electron"
        try:
            ident = await endpoint_identity(p, kind, target)
        except Exception as e:  # noqa: BLE001 — one bad endpoint is not fatal
            # A port that cannot be reached or driven is skipped, not fatal:
            # the next port may be the one in front, and if none is, the
            # refusal below is the answer.
            log.warning("skipping endpoint on port %s: %s", p, e)
            continue
        # The APPLICATION check comes first: it is cheap, and an endpoint
        # that belongs to another program must never be probed.
        if not app_matches(expected_app, ident):
            reason = (
                f"the debug endpoint on port {p} belongs to "
                f"{ident.get('app') or 'another application'}, not {expected_app}"
            )
            continue
        try:
            fg, why = await foreground_target(p, kind, target, expected_title, sweep)
        except Exception as e:  # noqa: BLE001 — one bad endpoint is not fatal
            log.warning("skipping endpoint on port %s: %s", p, e)
            continue
        if fg is None:
            # No target proved it is in front: a refusal, never a guess.
            reason = why or "the foreground target could not be established"
            continue
        # The TARGET must prove it is the one in front. It came from the
        # title-matched candidate set and it proved the focus; the title is
        # checked once more against the expected window.
        if not endpoint_matches(expected_app, expected_title, ident, fg):
            reason = "the focused target is not the window in front"
            continue
        chosen = p
        fg_target = fg
        identity = {
            **ident,
            "title": str(fg.get("title") or ""),
            "url": str(fg.get("url") or ""),
        }
        break
    if chosen is None or fg_target is None:
        return _err(
            "no reachable debug endpoint proves it is the application in front",
            409,
            expected_app=expected_app,
            expected_title=expected_title,
            reason=reason,
        )
    p = chosen

    t0 = time.monotonic()
    try:
        if p in r["direct_ports"]:
            out = await direct_activate(fg_target, name, role)
        else:
            async with Electron(p) as e:
                wcid = fg_target.get("id")
                if wcid is None:
                    return _err("no renderer", 502)
                out = await e.click(wcid, name, role)
    except CdpError as e:
        return _err(str(e), 502)
    except Exception as e:  # noqa: BLE001 — a transport failure is a 502, not a 500
        log.warning("activate failed on port %s: %s", p, e)
        return _err(f"the debug endpoint could not be driven: {e}", 502)

    if not out.get("ok"):
        return _err(out.get("error", "click failed"), 404, name=name,
                    available=out.get("availableButtons"))
    out["port"] = p
    out["identity"] = identity
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
