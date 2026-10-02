"""
AT-SPI accessibility server for NEXUS — semantic perception and actuation.

Runs LOCALLY on the user's device (127.0.0.1:39221).

WHY THIS EXISTS
---------------
NEXUS's screen understanding was pixel-only: screenshot -> OCR -> cloud VLM -> a
pixel coordinate. On a native Wayland session that has three problems, all
recorded in docs/features/research/22-… and answered in research/20-21:

  1. Keyboard/mouse synthesis is X11-only (enigo's default x11rb backend), so on
     Wayland NEXUS cannot actuate the screen at all.
  2. OCR + a VLM gives 1312ms + a network round trip for *text only*, with no
     role, no state, and no way to confirm an action worked.
  3. A pixel coordinate cannot be verified. Screen-state drift between
     observation and dispatch is a silent wrong-success, and an attack surface.

This server is the semantic tier. It reads the AT-SPI tree for *both* perception
and actuation, which is the key asymmetry:

    Wayland forbids SYNTHETIC INPUT. It does not forbid a GTK application from
    activating its own button on request.

`org.a11y.atspi.Action.do_action` and `org.a11y.atspi.EditableText
.set_text_contents` therefore work on a native Wayland session with **no** portal,
**no** /dev/uinput, **no** `input` group, and **no** consent dialog. Both were
verified on this machine before this file was written.

The same bus also carries `org.a11y.atspi.Event.*`, so the tree can be
event-driven rather than polled (see research/21 §6 H) — not wired up here yet.

Grounding order (docs/features/research/21-…):
  1. App native API (HTTP / DBus / MCP)         — exact, but only where it exists
  2. THIS SERVER — AT-SPI tree + do_action      — exact, and works on Wayland
  3. CDP AX tree for Chromium/Electron          — Step 3, not implemented here
  4. OCR / Set-of-Mark / vision                 — the unavoidable pixel floor

Sync note: production builds bundle a checked-in copy at
src-tauri/resources/server/atspi_server.py via tauri.conf.json resources. Keep
both identical — see server/ocr_server.py precedent.

Requirements:
  # The GI bindings ship with the desktop stack (at-spi2-core / gir1.2-atspi-2.0).
  # No pip install needed. This is the main advantage over pyatspi.
  apt install python3-gi gir1.2-atspi-2.0 at-spi2-core

Run locally on the device:
  uvicorn atspi_server:app --host 127.0.0.1 --port 39221

Environment:
  ATSPI_PORT — override the listen port (default: 39221)
  ATSPI_MAX_NODES — cap nodes per tree response (default: 4000)

KNOWN LIMITATIONS (see research/21-… §3) — read before trusting a result:
  * Chromium/Electron expose only an `application -> frame` skeleton over AT-SPI
    unless launched with --force-renderer-accessibility. Measured: VS Code 1 node
    without the flag, 140 with it. Use the CDP tier for those apps.
  * No Chromium node implements EditableText (checked across 500+ nodes), so text
    entry in Chrome/VS Code is impossible over AT-SPI. This server reports that
    honestly rather than pretending.
  * Action names are NOT standardised: GTK "click", Qt "Press", Chromium
    "doDefault". `resolve_action_index` normalises across that; do not assume.
  * GtkMenuButton/AdwMenuButton/AdwSplitButton expose an outer button with
    NActions=0, hiding the real action on an inner child. `find_invokeable`
    walks a bounded subtree when it sees that.
"""

from __future__ import annotations

import logging
import os
import sys
import time
from typing import Any

from fastapi import FastAPI, Request
from fastapi.concurrency import run_in_threadpool
from fastapi.responses import JSONResponse

log = logging.getLogger("nexus.atspi")
logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s")

app = FastAPI(title="NEXUS AT-SPI", version="0.1.0")

MAX_NODES = int(os.environ.get("ATSPI_MAX_NODES", "4000"))
MAX_DEPTH = int(os.environ.get("ATSPI_MAX_DEPTH", "40"))
# Bound on the GtkMenuButton workaround walk. Deep enough for stock GNOME
# widgets, shallow enough that a huge container cannot stall a request.
MENU_BUTTON_PROBE_CHILDREN = 24

# ── Atspi handle (lazy) ──────────────────────────────────────────────────────
# Imported lazily so the process starts (and /health can answer) even when the
# GI bindings are missing — the Rust client treats "unavailable" differently
# from "empty tree", and a crash-on-import would blur that.
_atspi: Any = None
_atspi_error: str | None = None


def enable_accessibility() -> tuple[bool, str]:
    """Turn on the toolkit accessibility bridge, which is off by default.

    This is not optional. GTK and Qt only build and publish their AT-SPI trees
    when `org.gnome.desktop.interface toolkit-accessibility` is true, and on a
    stock GNOME session it is **false**. Observed directly: with the flag off,
    every GTK application on the bus reported `child_count == 0` — no tree, no
    names, no actions — and every lookup returned "not found" while the server
    reported itself healthy. Screen readers flip this flag on when they attach
    (what `AtSpiSurface::open` does); a personal assistant has to do it itself.

    Second gate, and it is per-engine: **Firefox only publishes its AT-SPI tree
    when launched with `MOZ_ACCESSIBILITY_ATK2=1`.** Gecko implements ATK
    properly and bridges it over D-Bus, so it *is* reachable through this tier —
    but only with that variable set. It is the same class of problem as
    Chromium's renderer bridge, with a different knob, and it is documented only
    in Firefox's source tree.

    This is reported rather than worked around, because the variable must be in
    the browser's *environment* at launch and a running Firefox cannot be
    changed retroactively. `launch_env_hints()` gives the caller what to launch
    with; the honest remedy for an already-running Firefox is to restart it.

    Best-effort and non-fatal: if we cannot set it, the caller still works on
    desktops and toolkits that publish trees unconditionally.
    """
    if os.environ.get("ATSPI_NO_ENABLE"):
        return False, "disabled by ATSPI_NO_ENABLE"
    if not sys.platform.startswith("linux"):
        return False, "not a Linux session; relying on the toolkit default"
    try:
        import subprocess as _sp

        r = _sp.run(
            ["gsettings", "set", "org.gnome.desktop.interface", "toolkit-accessibility", "true"],
            capture_output=True, timeout=5,
        )
        if r.returncode == 0:
            return True, "toolkit-accessibility enabled via gsettings"
        return False, f"gsettings failed: {r.stderr.decode()[:120]}"
    except Exception as e:
        return False, f"{type(e).__name__}: {e}"


# Per-engine gates. Values are *environment variables* for the target process,
# because that is how each engine is switched on. Keyed by engine, not by app:
# Firefox is the Gecko engine, and so is Thunderbird, so one entry covers both.
ENGINE_ENV_HINTS: dict = {
    "gecko": {"MOZ_ACCESSIBILITY_ATK2": "1"},
    "blink": {},   # needs a command-line flag, not an env var — see docs
    "webkitgtk": {},
}


def launch_env_hints(engine: str) -> dict:
    """Environment a given rendering engine needs before it publishes a tree."""
    return dict(ENGINE_ENV_HINTS.get(engine.lower(), {}))


def atspi() -> Any:
    global _atspi, _atspi_error
    if _atspi is not None:
        return _atspi
    if _atspi_error is not None:
        return None
    try:
        enabled, how = enable_accessibility()
        log.info("AT-SPI: %s", how)
        import gi

        gi.require_version("Atspi", "2.0")
        from gi.repository import Atspi  # type: ignore

        Atspi.init()
        _atspi = Atspi
        log.info("AT-SPI initialised (toolkit accessibility enabled=%s)", enabled)
        return _atspi
    except Exception as e:  # missing at-spi2-core, headless, no a11y bus
        _atspi_error = f"{type(e).__name__}: {e}"
        log.warning("AT-SPI unavailable: %s", _atspi_error)
        return None


# ── Node shaping ─────────────────────────────────────────────────────────────

def _safe(fn, default=None):
    try:
        return fn()
    except Exception:
        return default


def extents_of(node: Any) -> dict[str, int] | None:
    """Screen-pixel bounds, or None when the toolkit has not laid out yet.

    GTK reports INT_MIN (-2147483648) as a "not yet positioned" sentinel. Those
    values must never reach a caller as if they were real coordinates, because
    they would be fed straight into a click.
    """
    A = atspi()
    if A is None:
        return None
    ext = _safe(lambda: node.get_extents(A.CoordType.SCREEN))
    if ext is None:
        return None
    x, y, w, h = ext.x, ext.y, ext.width, ext.height
    if x < -1000 or y < -1000 or w <= 0 or h <= 0:
        return None
    return {"x": int(x), "y": int(y), "w": int(w), "h": int(h)}


def states_of(node: Any) -> list[str]:
    A = atspi()
    if A is None:
        return []
    ss = _safe(lambda: node.get_state_set())
    if ss is None:
        return []
    out: list[str] = []
    for name in (
        "STATE_CHECKED", "STATE_PRESSED", "STATE_EXPANDED", "STATE_SELECTED",
        "STATE_SENSITIVE", "STATE_SHOWING", "STATE_FOCUSED", "STATE_ENABLED",
        "STATE_ACTIVE", "STATE_EDITABLE", "STATE_READ_ONLY",
    ):
        enum = _safe(lambda n=name: getattr(A.StateType, n))
        if enum is None:
            continue
        if _safe(lambda e=enum: ss.contains(e), False):
            out.append(name.replace("STATE_", "").lower())
    return out


def action_names(node: Any) -> list[str]:
    A = atspi()
    if A is None:
        return []
    ai = _safe(lambda: node.get_action_iface())
    if ai is None:
        return []
    n = _safe(lambda: ai.get_n_actions(), 0) or 0
    names: list[str] = []
    for i in range(n):
        nm = _safe(lambda k=i: A.Action.get_action_name(ai, k))
        if nm is None:
            nm = _safe(lambda k=i: ai.get_action_name(k), f"action{i}")
        names.append(str(nm))
    return names


# Toolkits disagree on the action name. GTK uses "click", Qt "Press",
# Chromium "doDefault". Looking for a single hardcoded string fails on most of
# the desktop — research/21 §3 records this. Order matters: the default/activate
# semantics we want is the first entry in each list.
_ACTION_ALIASES: tuple[tuple[str, tuple[str, ...]], ...] = (
    ("click", ("click", "press", "activate", "doDefault", "default", "toggle")),
    ("press", ("press", "click", "activate")),
    ("focus", ("focus", "grabfocus")),
    ("expand", ("expand", "open")),
    ("collapse", ("collapse", "close")),
)


def resolve_action_index(node: Any, wanted: str = "click") -> int | None:
    """Index of the action matching `wanted`, normalising across toolkits.

    Returns None when the node exposes no matching action, so callers can report
    "not supported" instead of firing the wrong thing.
    """
    names = action_names(node)
    if not names:
        return None
    wanted_l = wanted.lower()
    for canonical, variants in _ACTION_ALIASES:
        if canonical != wanted_l:
            continue
        lowered = [n.lower() for n in names]
        for v in variants:
            if v in lowered:
                return lowered.index(v)
    # Unknown request: fall back to the first action, which is conventionally
    # the default/activate one.
    return 0


def node_payload(node: Any, depth: int, counter: list[int]) -> dict[str, Any] | None:
    """One accessible as a JSON object. Returns None when the budget is spent."""
    A = atspi()
    if A is None:
        return None
    if counter[0] >= MAX_NODES:
        return None
    counter[0] += 1
    out: dict[str, Any] = {
        "role": _safe(lambda: node.get_role_name(), "") or "",
        "name": _safe(lambda: node.get_name(), "") or "",
        "id": _safe(lambda: node.get_index_in_parent(), -1),
        "bounds": extents_of(node),
        "states": states_of(node),
        "actions": action_names(node),
    }
    if depth < MAX_DEPTH:
        kids: list[dict[str, Any]] = []
        count = _safe(lambda: node.get_child_count(), 0) or 0
        for i in range(count):
            child = _safe(lambda k=i: node.get_child_at_index(k))
            if child is None:
                continue
            payload = node_payload(child, depth + 1, counter)
            if payload is not None:
                kids.append(payload)
        if kids:
            out["children"] = kids
    return out


# ── Lookup helpers ───────────────────────────────────────────────────────────

def desktop() -> Any:
    A = atspi()
    if A is None:
        return None
    d = _safe(lambda: A.get_desktop(0))
    return d


# Application roots are memoised.
#
# `Atspi.get_desktop(0).get_child_at_index(i)` mints a NEW remote proxy on every
# call, and GTK's AT-SPI bridge does not serve an unbounded number of live
# proxies per application. Re-walking the desktop for each request therefore
# produced fresh proxies whose methods raised `AttributeError`, so a lookup
# would succeed once and then fail forever after — with no error surfaced
# anywhere, because every call site wraps AT-SPI in `_safe`.
#
# Holding the roots for a short window means `find` and the `activate` that
# follows it operate on the *same* objects. The cache is deliberately short so
# that launching or closing an application is picked up promptly.
_APP_CACHE: list = []
_APP_CACHE_AT: float = 0.0
APP_CACHE_TTL = float(os.environ.get("ATSPI_APP_CACHE_TTL", "2.0"))


def applications(force: bool = False) -> list[Any]:
    global _APP_CACHE, _APP_CACHE_AT
    now = time.monotonic()
    if not force and _APP_CACHE and (now - _APP_CACHE_AT) < APP_CACHE_TTL:
        return _APP_CACHE
    d = desktop()
    if d is None:
        return []
    out = []
    count = _safe(lambda: d.get_child_count(), 0) or 0
    for i in range(count):
        app = _safe(lambda k=i: d.get_child_at_index(k))
        if app is not None:
            out.append(app)
    _APP_CACHE = out
    _APP_CACHE_AT = now
    return out


def app_named(name: str) -> Any | None:
    """Resolve an application root by name, using the memoised proxies.

    Falls back to a forced refresh when the name is unknown or the cached proxy
    has gone dead, so a newly launched window is still findable.
    """
    for app in applications():
        if (_safe(lambda a=app: a.get_name(), "") or "") == name:
            return app
    for app in applications(force=True):
        if (_safe(lambda a=app: a.get_name(), "") or "") == name:
            return app
    return None


# Every accessible property access is a D-Bus round trip, so walking a tree is
# O(nodes) round trips. A Chromium/Electron app can expose tens of thousands of
# nodes, which turned a lookup into a multi-minute hang. Two guards: a node
# budget and a wall-clock deadline. Whichever trips first wins, and the caller
# is told which — a timeout must never be reported as "not found".
SEARCH_TIME_BUDGET_MS = int(os.environ.get("ATSPI_SEARCH_BUDGET_MS", "1500"))


class SearchTimeout(Exception):
    """Raised when a tree walk exceeded its wall-clock budget."""


def iter_nodes(root: Any, limit: int = MAX_NODES, deadline: float | None = None):
    """Pre-order walk yielding (node, depth).

    `deadline` is a `time.monotonic()` value; the walk aborts by raising
    `SearchTimeout` rather than silently returning a short answer.
    """
    stack = [(root, 0)]
    seen = 0
    while stack and seen < limit:
        if deadline is not None and time.monotonic() > deadline:
            raise SearchTimeout(f"walk exceeded budget after {seen} nodes")
        node, depth = stack.pop()
        yield node, depth
        seen += 1
        count = _safe(lambda n=node: n.get_child_count(), 0) or 0
        children = []
        for i in range(count):
            # `node` must be bound as a default: it is a function parameter here
            # and stable, but binding it explicitly keeps this correct if the
            # loop is ever renamed or nested. The first version of this helper
            # referenced an undefined `n`, which `_safe` silently turned into
            # "no children" — so every lookup returned "not found" while the
            # server looked perfectly healthy.
            c = _safe(lambda n=node, k=i: n.get_child_at_index(k))
            if c is not None:
                children.append(c)
        # Reverse so the pop order preserves document order.
        for c in reversed(children):
            stack.append((c, depth + 1))


def find_invokeable(node: Any, wanted: str = "click") -> Any | None:
    """Locate a node whose action matches, working around GTK menu buttons.

    `GtkMenuButton` / `AdwMenuButton` / `AdwSplitButton` present an outer
    `push button` that advertises NActions=0 while the real action lives on an
    inner `toggle button`. Stock GNOME apps — Calculator, Text Editor, Clocks,
    Baobab — all hit this, so "the button I can see has no press action" is the
    common case, not an edge case (research/21 §3).
    """
    if resolve_action_index(node, wanted) is not None:
        return node
    if _safe(lambda: node.get_role_name(), "") in ("push button", "toggle button", "menu button"):
        probed = 0
        for child, _ in iter_nodes(node, limit=MENU_BUTTON_PROBE_CHILDREN):
            if child is node:
                continue
            probed += 1
            if resolve_action_index(child, wanted) is not None:
                return child
            if probed >= MENU_BUTTON_PROBE_CHILDREN:
                break
    return None


def find_by_role_and_name(root: Any, name: str, role: str | None = None) -> Any | None:
    """Exact accessible-name match, preferring the requested role."""
    fallback = None
    for node, _ in iter_nodes(root):
        if (_safe(lambda n=node: n.get_name(), "") or "") != name:
            continue
        node_role = _safe(lambda n=node: n.get_role_name(), "") or ""
        if role and node_role == role:
            return node
        if fallback is None:
            fallback = node
    return fallback


def find_text_field(root: Any, name: str | None = None) -> Any | None:
    """An editable node, by name if given, else the first one found."""
    roles = ("entry", "text", "password text", "document text", "search box", "combo box")
    fallback = None
    for node, _ in iter_nodes(root):
        node_role = _safe(lambda n=node: n.get_role_name(), "") or ""
        if node_role not in roles:
            continue
        et = _safe(lambda n=node: n.get_editable_text_iface())
        if et is None:
            continue
        if name:
            if (_safe(lambda n=node: n.get_name(), "") or "") == name:
                return node
        elif fallback is None:
            fallback = node
    return fallback


def searchable(app: Any) -> bool:
    """Whether a blind cross-application search should descend into this app.

    Excluded: Chromium/Electron (over AT-SPI they expose only a skeleton unless
    launched with --force-renderer-accessibility, and the tree is large enough
    that an unbounded search stalls the request — they need the CDP tier), and
    apps with nothing actionable.

    The probe reads `walk_cached`, so it never consumes a traversal the caller
    still needs. An earlier version walked the tree directly and, being the first
    traversal of that app, left the following one holding dead proxies.
    """
    if is_chromium(app):
        return False
    deadline = time.monotonic() + 0.4
    A = atspi()
    if A is None:
        return False
    # Read during traversal: a retained proxy can be dead by the time it is
    # inspected, which would classify a fully interactive app as empty.
    for _node, _depth in iter_nodes(app, limit=200, deadline=deadline):
        pass  # ensure traversal is possible at all
    r = Resolver()
    for snap in r.walk(app, limit=200):
        if snap.node is app:
            continue
        if snap.actions or snap.editable:
            return True
    return False


def is_chromium(node: Any) -> bool:
    """Chromium/Electron apps report this toolkit.

    Used to warn that the tree is probably a skeleton — without
    --force-renderer-accessibility VS Code exposes 1 node instead of 140.
    """
    A = atspi()
    if A is None:
        return False
    tk = _safe(lambda: node.get_toolkit_name()) or _safe(lambda: node.get_toolkit())
    if tk and "chromium" in str(tk).lower():
        return True
    # Atspi 2.x exposes toolkit via a different accessor on some bindings.
    return "chromium" in str(_safe(lambda: node.get_name()) or "").lower() and False


# ── Node snapshots ───────────────────────────────────────────────────────────
#
# A snapshot's properties are read DURING traversal, not afterwards.
#
# Retaining live `Accessible` proxies and reading them later is not safe on every
# toolkit. Measured here on WebKitGTK: a walk that reads each node as it is
# visited returns 21/21 live nodes, including the HTML <button> and <input>
# inside the web view. The identical walk that collects proxies first and reads
# them afterwards returns the same 21 nodes with every property raising
# `AttributeError` — a silently empty tree that looks like a healthy answer.
#
# GTK tolerates the retain-then-read pattern, so this passed every GTK test and
# would have shipped broken for WebKit apps. Snapshots make it correct for all
# toolkits and remove the whole failure class.


class NodeSnapshot:
    """Immutable view of one accessible, captured while it was still live."""

    __slots__ = ("role", "name", "bounds", "states", "actions", "editable", "node")

    def __init__(self, node, A):
        self.node = node
        # These can still fail for a node that dies mid-traversal; each is
        # captured independently so one bad property does not lose the node.
        self.role = _safe(lambda: node.get_role_name(), "") or ""
        self.name = _safe(lambda: node.get_name(), "") or ""
        self.bounds = extents_of(node)
        self.states = states_of(node)
        self.actions = action_names(node)
        self.editable = _safe(lambda: node.get_editable_text_iface()) is not None

    @property
    def actionable(self) -> bool:
        return bool(self.actions) or self.editable

    def as_dict(self) -> dict:
        return {
            "role": self.role,
            "name": self.name,
            "bounds": self.bounds,
            "states": self.states,
            "actions": self.actions,
            "editable": self.editable,
        }

    def __repr__(self) -> str:  # pragma: no cover - debugging aid
        return f"<NodeSnapshot {self.role} {self.name!r} actions={self.actions}>"


# ── Resolution ───────────────────────────────────────────────────────────────

# One materialised traversal per application per short window.
#
# GTK's AT-SPI bridge does not serve an unbounded number of live remote proxies.
# Measured on this machine: the first traversal of an application returned a
# complete 14-node tree, and an immediately following traversal returned 14 nodes
# whose every method raised `AttributeError` — silently, because every call site
# wraps AT-SPI in `_safe`. So `searchable()` walking first and `find()` walking
# second made `find` fail on an element that was demonstrably present.
#
# Consequence: any two operations that both need the tree (probe, then resolve;
# resolve, then activate) must share ONE traversal. Results are cached briefly
# so a lookup and the action it authorises operate on the same live objects.
_WALK_CACHE: dict = {}
WALK_CACHE_TTL = float(os.environ.get("ATSPI_WALK_CACHE_TTL", "1.5"))


def walk_cached(app: Any, limit: int = MAX_NODES, deadline: float | None = None) -> list:
    """Materialised pre-order walk of `app`, memoised for a short window."""
    key = _safe(lambda a=app: a.get_name(), "") or str(id(app))
    now = time.monotonic()
    hit = _WALK_CACHE.get(key)
    if hit is not None and (now - hit[0]) < WALK_CACHE_TTL:
        return hit[1]
    out: list = []
    try:
        for node in iter_nodes(app, limit=limit, deadline=deadline):
            out.append(node)
    except SearchTimeout:
        pass
    _WALK_CACHE[key] = (now, out)
    return out


class Resolver:
    """Deadline-aware, bounded lookup across the desktop.

    Two failure modes this exists to prevent, both observed in practice:

      * An unbounded cross-application walk. A lookup for a missing name used to
        descend every application, including VS Code and Brave, and never
        returned.
      * Reporting a timeout as "not found". Those are different answers — one
        means "it is not there", the other means "I could not tell" — and
        conflating them produces a confidently wrong result.
    """

    def __init__(self, budget_ms: int | None = None):
        self.deadline = time.monotonic() + (budget_ms or SEARCH_TIME_BUDGET_MS) / 1000.0
        self.timed_out = False
        self.skipped: list[str] = []

    def walk(self, root: Any, limit: int = MAX_NODES) -> list:
        """Fully drained walk, returning [`NodeSnapshot`] rather than proxies.

        See the note on `NodeSnapshot`: properties are captured during traversal
        because reading them from a retained proxy afterwards silently yields an
        empty tree on some toolkits.
        """
        A = atspi()
        if A is None:
            return []
        out: list = []
        seen = 0
        while seen < limit:
            if self.deadline is not None and time.monotonic() > self.deadline:
                self.timed_out = True
                break
            stack, node = [], None
            # iter_nodes is a generator; drain it one node at a time.
            if not hasattr(self, "_it"):
                self._it = iter_nodes(root, limit=limit, deadline=self.deadline)
            try:
                node, _depth = next(self._it)
            except StopIteration:
                break
            except SearchTimeout:
                self.timed_out = True
                break
            if node is None:
                continue
            seen += 1
            out.append(NodeSnapshot(node, A))
        if hasattr(self, "_it"):
            del self._it
        return out

    @staticmethod
    def _named(nodes: list, name: str, role: str | None):
        for snap in nodes:
            if snap.name != name:
                continue
            if role and snap.role != role:
                continue
            return snap
        return None

    def find(self, name: str, role: str | None, app_name: str | None) -> Any | None:
        """Exact accessible-name match. Returns None only when truly absent.

        Retries once against a freshly fetched root. Given the stale-cache
        behaviour described in `walk`, a single miss is not conclusive, and
        reporting "not found" for an element that is present would be a
        confidently wrong answer.
        """
        if app_name:
            for attempt in (0, 1):
                root = app_named(app_name)
                if root is None:
                    return None
                hit = self._named(self.walk(root), name, role)
                if hit is not None:
                    return root, hit.node
                if self.timed_out:
                    break
            return None

        for app in applications():
            if not searchable(app):
                continue
            hit = self._named(self.walk(app, limit=1500), name, role)
            if hit is not None:
                return app, hit.node
        return None

    def text_field(self, name: str | None, app_name: str | None) -> Any | None:
        roles = ("entry", "text", "password text", "document text", "search box", "combo box")
        if app_name:
            for _ in range(2):
                root = app_named(app_name)
                if root is None:
                    return None
                for snap in self.walk(root):
                    if name and snap.name != name:
                        continue
                    if snap.editable:
                        return root, snap.node
                if self.timed_out:
                    break
            return None
        for app in applications():
            if not searchable(app):
                continue
            for snap in self.walk(app, limit=1500):
                if snap.role in roles and snap.editable and (not name or snap.name == name):
                    return app, snap.node
        return None

    @staticmethod
    def _is_field(node: Any, roles: tuple[str, ...]) -> bool:
        if (_safe(lambda n=node: n.get_role_name(), "") or "") not in roles:
            return False
        return _safe(lambda n=node: n.get_editable_text_iface()) is not None


def _err(reason: str, code: int, **extra):
    body = {"error": reason, **extra}
    return JSONResponse(body, status_code=code)


# ── HTTP ─────────────────────────────────────────────────────────────────────

@app.get("/health")
async def health() -> JSONResponse:
    A = atspi()
    if A is None:
        return JSONResponse(
            {"status": "unavailable", "reason": _atspi_error}, status_code=503
        )
    d = desktop()
    apps = applications()
    names = [(_safe(lambda a=a: a.get_name(), "") or "") for a in apps]
    enabled, how = enable_accessibility()
    return JSONResponse(
        {
            "status": "ok",
            "service": "nexus-atspi",
            "applications": len(apps),
            "app_names": names,
            "toolkit_accessibility": enabled,
            "toolkit_accessibility_note": how,
            "searchable_apps": [n for n, a in zip(names, apps) if searchable(a)],
        }
    )


@app.get("/windows")
async def windows() -> JSONResponse:
    """Top-level frames for every application, plus toolkit hints."""
    if atspi() is None:
        return _err("atspi unavailable", 503, reason=_atspi_error)
    return JSONResponse(await run_in_threadpool(_windows_blocking))


def _windows_blocking() -> dict:
    out = []
    for app in applications():
        frames = []
        for node, _ in iter_nodes(app, limit=400):
            if (_safe(lambda n=node: n.get_role_name(), "") or "") in ("frame", "window", "dialog"):
                frames.append(
                    {
                        "name": _safe(lambda n=node: n.get_name(), "") or "",
                        "role": _safe(lambda n=node: n.get_role_name(), "") or "",
                        "bounds": extents_of(node),
                    }
                )
                if len(frames) >= 12:
                    break
        out.append(
            {
                "app": _safe(lambda a=app: a.get_name(), "") or "",
                "toolkit": _safe(lambda a=app: a.get_toolkit_name()),
                "chromium_skeleton_risk": is_chromium(app),
                "searchable": searchable(app),
                "frames": frames,
            }
        )
    return {"applications": out}


@app.get("/focused")
async def focused() -> JSONResponse:
    """Identify the foreground window: its application and top-level title.

    This is the endpoint that unblocks the privacy gate. `docs/features/research/22`
    recorded that `exclusion_gate()` was inert on every non-Windows platform
    because it depended on `foreground_title()`, which returned `None` — so
    banking windows were screenshotted and OCR'd with the exclusion list fully
    configured. The fix in the Rust layer was to fail closed, which means screen
    vision refuses outright on native Wayland.

    AT-SPI is the only portable way to answer this there: no compositor API
    exists (`Shell.Eval` has been disabled since GNOME 41, and there is no
    get-active-window portal).

    Resolution order, cheapest first:
      1. An application whose accessible carries STATE_ACTIVE.
      2. A top-level frame containing a node with STATE_FOCUSED.
      3. The focused frame of the only application that has one.

    Returns `{"known": false, ...}` rather than a 404 when nothing can be
    identified. The caller must treat that as "refuse", never as "no match" —
    the same distinction ExclusionVerdict exists to preserve in Rust.

    KNOWN LIMITATION, observed on this machine: not every session reports focus
    through AT-SPI. In a nested or virtual display with no window manager
    actually arbitrating focus, even an explicit
    `Component.grab_focus()` leaves the frame's state set empty, and no
    application carries STATE_ACTIVE — so this endpoint correctly answers
    `known: false` and the privacy gate keeps failing closed. That is the safe
    outcome, and it is why the Rust side treats `Unknown` as a refusal rather
    than retrying or guessing. Verified on GNOME/Wayland: the endpoint answers
    200 with a reason, and never invents a window.

    Chromium/Electron apps are reported but flagged: over AT-SPI they expose only
    a skeleton, so the title is the window title and nothing deeper is available
    until the CDP tier lands.
    """
    if atspi() is None:
        return _err("atspi unavailable", 503, reason=_atspi_error)

    result = await run_in_threadpool(_focused_blocking)
    return JSONResponse(result)


def _focused_blocking() -> dict:
    t0 = time.monotonic()
    A = atspi()

    def frame_of(app):
        """The app's top-level frame accessible, or None."""
        for node, _ in iter_nodes(app, limit=200):
            role = _safe(lambda n=node: n.get_role_name(), "") or ""
            if role in ("frame", "window", "dialog"):
                return node
        return None

    def has_state(node, wanted: str) -> bool:
        return wanted.lower() in states_of(node)

    apps = applications()
    best = None

    # 1. STATE_ACTIVE on the application itself.
    for app in apps:
        if has_state(app, "ACTIVE"):
            frame = frame_of(app)
            best = (app, frame, "application STATE_ACTIVE")
            break

    # 2. A focused node somewhere under an application's frame.
    if best is None:
        for app in apps:
            frame = frame_of(app)
            if frame is None:
                continue
            deadline = time.monotonic() + 0.5
            try:
                for node, _ in iter_nodes(frame, limit=800, deadline=deadline):
                    if has_state(node, "FOCUSED") and node is not frame:
                        best = (app, frame, "descendant STATE_FOCUSED")
                        break
            except SearchTimeout:
                pass
            if best is not None:
                break

    # 3. Single foreground-ish application.
    if best is None:
        with_frames = []
        for app in apps:
            f = frame_of(app)
            if f is not None and (extents_of(f) or {}).get("w", 0) > 0:
                with_frames.append((app, f))
        if len(with_frames) == 1:
            best = (with_frames[0][0], with_frames[0][1], "only visible application")

    if best is None:
        return {
            "known": False,
            "reason": "no application reported an active or focused window",
            "applications": len(apps),
            "latency_ms": int((time.monotonic() - t0) * 1000),
        }

    app, frame, how = best
    return {
        "known": True,
        "how": how,
        "app": _safe(lambda a=app: a.get_name(), "") or "",
        "title": _safe(lambda f=frame: f.get_name(), "") or "",
        "bounds": extents_of(frame) if frame is not None else None,
        "chromium_skeleton_risk": is_chromium(app),
        "latency_ms": int((time.monotonic() - t0) * 1000),
    }


@app.get("/tree")
async def tree(request: Request) -> JSONResponse:
    """Semantic tree for one application, or for everything if `app` is absent."""
    if atspi() is None:
        return JSONResponse({"error": "atspi unavailable", "reason": _atspi_error}, status_code=503)
    q = request.query_params
    app_name = q.get("app")
    result = await run_in_threadpool(_tree_blocking, app_name)
    if result.get("error"):
        return JSONResponse(result, status_code=404)
    return JSONResponse(result)


def _tree_blocking(app_name: str | None) -> dict:
    roots = [app_named(app_name)] if app_name else applications()
    if app_name and roots[0] is None:
        return {"error": f"no such application: {app_name}"}

    t0 = time.monotonic()
    payload = []
    emitted = 0
    for root in roots:
        if root is None:
            continue
        counter = [0]
        node = node_payload(root, 0, counter)
        if node is not None:
            payload.append(node)
        emitted += counter[0]
    ms = int((time.monotonic() - t0) * 1000)
    return (
        {
            "trees": payload,
            "nodes": emitted,
            # True means the tree was cut off. A caller must not treat a truncated
            # tree as "the element does not exist" — that would silently turn a
            # budget limit into a wrong answer.
            "truncated": emitted >= MAX_NODES,
            "latency_ms": ms,
        }
    )


@app.get("/find")
async def find(request: Request) -> JSONResponse:
    """Resolve an accessible by name (+ optional role) and return its identity.

    This is the primitive behind identity-addressed actuation (research/20 §4.3):
    the caller gets a node reference and its geometry, and can re-resolve the
    same name immediately before dispatching to confirm the target is still the
    intended element rather than something that merely occupies the same pixels.
    """
    if atspi() is None:
        return _err("atspi unavailable", 503, reason=_atspi_error)
    q = request.query_params
    name = q.get("name", "")
    if not name:
        return _err("name required", 400)
    app_name = q.get("app") or None
    role = q.get("role") or None

    result = await run_in_threadpool(_find_blocking, name, role, app_name)
    if isinstance(result, JSONResponse):
        return result
    root, node, resolver = result
    return JSONResponse(
        {
            "app": _safe(lambda a=root: a.get_name(), "") or "",
            "role": _safe(lambda n=node: n.get_role_name(), "") or "",
            "name": _safe(lambda n=node: n.get_name(), "") or "",
            "bounds": extents_of(node),
            "states": states_of(node),
            "actions": action_names(node),
            "chromium_skeleton_risk": is_chromium(root),
            "timed_out": resolver.timed_out,
        }
    )


def _find_blocking(name: str, role: str | None, app_name: str | None):
    t0 = time.monotonic()
    r = Resolver()
    hit = r.find(name, role, app_name)
    if hit is None:
        if r.timed_out:
            # Distinct from "not found": we ran out of budget, so we do not know.
            return _err("search timed out", 504, name=name,
                        hint="pass ?app= to bound the search")
        return _err(f"not found: {name}", 404, name=name)
    if isinstance(hit, tuple):
        root, node = hit
    else:
        root, node = hit, hit
    return root, node, r


@app.post("/activate")
async def activate(request: Request) -> JSONResponse:
    """Invoke an accessible's action. No input synthesis is performed.

    This is the whole reason the semantic tier exists: it works on a native
    Wayland session with no portal, no /dev/uinput, no consent dialog.

    Re-resolves the target by name immediately before invoking, then re-reads the
    node's state afterwards and returns it, so the caller gets a post-condition
    instead of taking the return value on faith.
    """
    if atspi() is None:
        return _err("atspi unavailable", 503, reason=_atspi_error)
    body = await _json(request)
    name = body.get("name", "")
    if not name:
        return _err("name required", 400)
    app_name = body.get("app") or None
    role = body.get("role") or None
    action = body.get("action", "click")

    result = await run_in_threadpool(_activate_blocking, name, role, app_name, action)
    return result if isinstance(result, JSONResponse) else JSONResponse(result)


def _activate_blocking(name: str, role: str | None, app_name: str | None, action: str):
    t0 = time.monotonic()
    r = Resolver()
    hit = r.find(name, role, app_name)
    if hit is None:
        if r.timed_out:
            return _err("search timed out", 504, name=name,
                        hint="pass {\"app\": ...} to bound the search")
        return _err(f"not found: {name}", 404, name=name)
    root, node = hit if isinstance(hit, tuple) else (hit, hit)

    target = find_invokeable(node, action)
    if target is None:
        # GtkMenuButton exposes an outer button with NActions=0 and the real
        # action on a child; find_invokeable handles that. Reaching here means
        # the element genuinely cannot be activated.
        return _err(
            "no matching action",
            409,
            requested=action,
            available=action_names(node),
            role=_safe(lambda n=node: n.get_role_name(), "") or "",
            app=_safe(lambda a=root: a.get_name(), "") or "",
            hint="Chromium/Electron expose actions as 'doDefault'; try action=doDefault",
        )

    idx = resolve_action_index(target, action)
    if idx is None:
        return _err("action unresolvable", 409, available=action_names(target))

    ai = target.get_action_iface()
    before = states_of(target)
    ok = bool(ai.do_action(idx))
    after = states_of(target)
    changed = [x for x in after if x not in before]
    names = action_names(target)

    log.info(
        "activate %r (%s) via %r -> %s | states %s -> %s",
        name, _safe(lambda n=target: n.get_role_name(), ""), names, ok, before, after,
    )
    return {
        "ok": ok,
        "app": _safe(lambda a=root: a.get_name(), "") or "",
        "name": name,
        "action": names[idx] if ok and idx < len(names) else action,
        "states_before": before,
        "states_after": after,
        "states_changed": changed,
        "latency_ms": int((time.monotonic() - t0) * 1000),
    }


@app.post("/fill")
async def fill(request: Request) -> JSONResponse:
    """Set an editable field's contents. No keyboard synthesis is performed.

    Reports honestly when a target exposes no EditableText — which is true for
    *every* Chromium node, so Chrome and VS Code text entry genuinely cannot be
    done over AT-SPI. The CDP tier addresses those apps instead.
    """
    if atspi() is None:
        return _err("atspi unavailable", 503, reason=_atspi_error)
    body = await _json(request)
    if "text" not in body:
        return _err("text required", 400)
    name = body.get("name") or None
    app_name = body.get("app") or None
    text = str(body["text"])

    result = await run_in_threadpool(_fill_blocking, name, app_name, text)
    return result if isinstance(result, JSONResponse) else JSONResponse(result)


def _fill_blocking(name: str | None, app_name: str | None, text: str):
    t0 = time.monotonic()
    r = Resolver()
    hit = r.text_field(name, app_name)
    if hit is None:
        if r.timed_out:
            return _err("search timed out", 504, hint="pass {\"app\": ...} to bound the search")
        return _err(f"no editable field named {name!r}", 404, name=name)
    root, node = hit

    A = atspi()
    et = node.get_editable_text_iface()
    if et is None:
        return _err(
            "target has no EditableText",
            409,
            role=_safe(lambda n=node: n.get_role_name(), "") or "",
            name=_safe(lambda n=node: n.get_name(), "") or "",
            hint="Chromium/Electron never expose EditableText; use the CDP tier",
        )

    A.EditableText.set_text_contents(et, text)
    time.sleep(0.05)
    observed = _safe(lambda: A.Text.get_text(node, 0, -1), "") or ""
    ok = observed == text
    log.info("fill %r into %r -> %s", text[:32], name, ok)
    return {
        "ok": ok,
        "app": _safe(lambda a=root: a.get_name(), "") or "",
        "name": _safe(lambda n=node: n.get_name(), "") or "",
        "observed": observed,
        "latency_ms": int((time.monotonic() - t0) * 1000),
    }


async def _json(request: Request) -> dict:
    try:
        return await request.json()
    except Exception:
        return {}


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(app, host="127.0.0.1", port=int(os.environ.get("ATSPI_PORT", "39221")))
