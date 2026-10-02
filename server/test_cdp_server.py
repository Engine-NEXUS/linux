"""
Tests for the CDP tier.

Two groups:

  * Pure logic (always runs) — AX-value normalisation, node budget/truncation,
    name+role matching, endpoint classification, and the "no endpoint reachable"
    contract. These are the parts that can silently produce a wrong answer, and
    they need no browser.

  * Live integration (skipped when no debug port answers) — drives a real
    Chromium or Electron app over CDP.

The skip is honest rather than convenient: a browser not launched with a debug
port is genuinely not visible to this tier, which is the same fact /health
reports. Asserting against a non-existent endpoint would test nothing.

Run:  python3 -m pytest server/test_cdp_server.py -v
"""

from __future__ import annotations

import os
import sys

import pytest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import cdp_server as m  # noqa: E402


# ── Pure logic ───────────────────────────────────────────────────────────────


def test_ax_val_handles_both_shapes():
    # CDP wraps values in {"value": ...}; our own Electron bridge flattens them.
    assert m._ax_val({"value": "Confirm"}) == "Confirm"
    assert m._ax_val("Confirm") == "Confirm"
    assert m._ax_val({"value": None}) == ""
    assert m._ax_val(None) == ""
    assert m._ax_val({}) == ""


def test_normalise_flattens_and_budgets():
    nodes = [
        {"role": {"value": "button"}, "name": {"value": "Go"}, "backendDOMNodeId": 7},
        {"role": {"value": "textbox"}, "name": {"value": "Search"}, "backendDOMNodeId": 8},
    ]
    out = m._normalise(nodes)
    assert [n["_role"] for n in out] == ["button", "textbox"]
    assert [n["name"] for n in out] == ["Go", "Search"]
    assert out[0]["backendDOMNodeId"] == 7

    # A node with no backendDOMNodeId must not be handed out as actionable.
    junk = m._normalise([{"role": {"value": "StaticText"}, "name": {"value": "x"}}])
    assert junk[0]["backendDOMNodeId"] == 0


def test_normalise_respects_the_budget():
    many = [{"role": {"value": "StaticText"}, "name": {"value": str(i)}, "backendDOMNodeId": i}
            for i in range(m.AX_BUDGET + 50)]
    out = m._normalise(many)
    assert len(out) == m.AX_BUDGET


def test_match_requires_exact_name():
    nodes = [
        {"role": {"value": "button"}, "name": {"value": "Save"}, "backendDOMNodeId": 1},
        {"role": {"value": "button"}, "name": {"value": "Save As"}, "backendDOMNodeId": 2},
    ]
    # Substring matching would click "Save As" when asked for "Save".
    assert m._match(nodes, "Save", "button")["backendDOMNodeId"] == 1
    assert m._match(nodes, "Save", None)["backendDOMNodeId"] == 1
    assert m._match(nodes, "sav", "button") is None


def test_match_honours_role_and_falls_back_visibly():
    nodes = [
        {"role": {"value": "textbox"}, "name": {"value": "Search"}, "backendDOMNodeId": 1},
        {"role": {"value": "button"}, "name": {"value": "Search"}, "backendDOMNodeId": 2},
    ]
    assert m._match(nodes, "Search", "button")["backendDOMNodeId"] == 2
    assert m._match(nodes, "Search", "textbox")["backendDOMNodeId"] == 1
    # An unmatched role returns the name-match rather than nothing, so the caller
    # can see the element exists but is not of the expected kind.
    assert m._match(nodes, "Search", "checkbox")["backendDOMNodeId"] == 1


def test_match_missing_returns_none():
    nodes = [{"role": {"value": "button"}, "name": {"value": "Go"}, "backendDOMNodeId": 1}]
    assert m._match(nodes, "Nope", None) is None


def test_probe_ports_defaults_and_override():
    old = os.environ.get("CDP_PORT")
    os.environ.pop("CDP_PORT", None)
    assert m.probe_ports() == m.DEFAULT_PORTS
    os.environ["CDP_PORT"] = "9999,1234"
    assert m.probe_ports() == [9999, 1234]
    os.environ["CDP_PORT"] = "9999,notaport,"
    assert m.probe_ports() == [9999], "non-numeric entries must be dropped, not crash"
    if old is None:
        os.environ.pop("CDP_PORT", None)
    else:
        os.environ["CDP_PORT"] = old


def test_reachable_classifies_by_target_shape(monkeypatch):
    """A port is `direct` only if it serves a `page` target.

    An Electron inspector port answers /json/list with a `node` target and no
    page. Classifying it as direct made every lookup fail with "no page target",
    which is what the first version did.

    Pins CDP_PORT rather than DEFAULT_PORTS: probe_ports() reads the env var
    first, so mutating the module constant alone leaks into every later test and
    makes this one fail for reasons that have nothing to do with classification.
    """
    fake = {
        9222: [
            {"type": "page", "webSocketDebuggerUrl": "ws://x", "title": "t", "url": "u"},
            {"type": "background_page", "webSocketDebuggerUrl": "ws://y"},
        ],
        9229: [{"type": "node", "webSocketDebuggerUrl": "ws://z"}],
        9333: [],  # nothing listening
    }
    monkeypatch.setenv("CDP_PORT", "9222,9229,9333")
    monkeypatch.setattr(m, "http_list", lambda p, *a, **k: fake.get(p, []))
    monkeypatch.setattr(
        m,
        "inspector_main_target",
        lambda p: next((t for t in fake.get(p, []) if t["type"] == "node"), None),
    )
    r = m.reachable()
    assert 9222 in r["direct_ports"]
    assert 9229 not in r["direct_ports"], "inspector port must not be classified direct"
    assert 9229 in r["electron_ports"]
    assert 9333 not in r["direct_ports"] and 9333 not in r["electron_ports"]
    assert r["any"] is True


def test_native_setter_js_does_not_assign_value_directly():
    """React/Vue track their own state; a plain `.value =` is silently ignored.

    The fill path must go through the prototype's native setter and dispatch
    input/change, or the write appears to succeed and the framework never sees it.
    """
    js = m._SET_JS
    assert "getOwnPropertyDescriptor" in js
    assert "d.set.call" in js
    assert "input" in js and "change" in js
    assert "el.value = v" not in js, "must not assign .value directly"


# ── HTTP contract ────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def tc():
    from fastapi.testclient import TestClient

    return TestClient(m.app)


def test_health_always_answers_and_explains_idle(tc):
    """Idle is a state, not a failure, and must carry the launch instructions.

    A 503 here would be read by the Rust side as "tier broken" when the truth is
    "no browser has a debug port open" — a different problem with a different fix.
    """
    r = tc.get("/health")
    assert r.status_code == 200
    b = r.json()
    assert b["status"] in ("ok", "idle")
    assert b["note"], "an idle health report must say how to make it reachable"
    assert "--inspect" in b["note"] and "--remote-debugging-port" in b["note"]


def test_endpoints_report_instead_of_hanging_when_nothing_is_reachable(tc):
    """With no endpoint, answer 503 fast. Never block.

    A silent hang here would sit on the pre-capture path, which is the exact
    failure the whole bounded-search design exists to prevent.
    """
    import time

    r = m.reachable()
    if r["any"]:
        pytest.skip("a CDP endpoint is live; the nothing-reachable path cannot be exercised")
    t0 = time.time()
    for path, params in (("/tree", {}), ("/find", {"name": "x"})):
        resp = tc.get(path, params=params)
        assert resp.status_code == 503
        assert resp.json()["error"]
    resp = tc.post("/activate", json={"name": "x"})
    assert resp.status_code == 503
    assert time.time() - t0 < 15, "no-endpoint responses must be immediate"


def test_missing_parameters_are_rejected(tc):
    r = m.reachable()
    if not r["any"]:
        pytest.skip("no endpoint; parameter validation is checked below anyway")
    assert tc.get("/find", params={}).status_code == 400
    assert tc.post("/activate", json={}).status_code == 400
    assert tc.post("/fill", json={}).status_code == 400


# ── Live integration ─────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def live(tc):
    r = m.reachable()
    if not r["any"]:
        pytest.skip(
            "no CDP endpoint reachable — launch chromium with --remote-debugging-port "
            "or an Electron app with --inspect"
        )
    return r


def test_live_tree_is_rich(live, tc):
    import time

    """A real app must expose a usable tree.

    The number that matters: VS Code exposes 1 node over AT-SPI and ~700-1000
    here. Anything in single digits means the tier is connected to a skeleton and
    is not doing its job.
    """
    # Poll and take the best. A live app's accessibility tree genuinely
    # fluctuates: VS Code measured 1026 nodes steady, then 11 mid-reload when the
    # quick-access palette was animating, then 1026 again. A single snapshot is
    # therefore not a sound test of "connected to a real surface" — but a
    # consistently tiny tree is, since a skeleton is small *all* the time.
    #
    # This fluctuation is also the argument for identity-addressed actuation: a
    # cached tree goes stale, so the target is re-resolved immediately before it
    # is acted on rather than remembered.
    best = None
    for _ in range(5):
        r = tc.get("/tree")
        assert r.status_code == 200, r.text
        body = r.json()
        if best is None or body["count"] > best["count"]:
            best = body
        if best["count"] > 200:
            break
        time.sleep(0.5)

    assert best["count"] > 20, (
        f"best tree across 5 polls was only {best['count']} nodes — "
        f"this looks like a skeleton, not a live surface"
    )
    assert best["truncated"] is False or best["total"] > best["count"]
    assert "latency_ms" in best


def test_live_find_then_activate(live, tc):
    """The headline capability: resolve by name, then act on it.

    Asserts the two properties that make this worth having over pixels: the
    target is addressed by *identity* (a name plus a role), and the act step
    needs no coordinates, no compositor and no portal.
    """
    tree = tc.get("/tree").json()
    buttons = [n for n in tree["nodes"] if n["_role"] == "button" and n["name"]]
    if not buttons:
        pytest.skip("no buttons in the live tree")
    # A stable, unremarkable target. Deliberately does not pick the first button:
    # that may be a close/dismiss control.
    target = next(
        (b for b in buttons if any(
            k in b["name"].lower() for k in ("open", "new", "run", "search", "file", "view"))),
        buttons[-1],
    )

    found = tc.get("/find", params={"name": target["name"], "role": "button"})
    assert found.status_code == 200, f"{target['name']!r} not resolvable: {found.text}"
    f = found.json()
    assert f["name"] == target["name"]
    assert f["role"] == "button"
    assert f["backendDOMNodeId"] > 0, "an unresolvable node must not look actionable"

    # The wrong role must not silently match a real element.
    wrong = tc.get("/find", params={"name": target["name"], "role": "checkbox"})
    assert wrong.status_code in (200, 404)
    if wrong.status_code == 200:
        assert wrong.json()["role"] == "button", "role filter was ignored"


def test_live_missing_name_is_404_with_diagnostics(live, tc):
    r = tc.get("/find", params={"name": "NEXUS_NO_SUCH_BUTTON_XYZ"})
    assert r.status_code == 404
    b = r.json()
    assert "roles_seen" in b, "a miss should report what roles exist, for diagnosis"
