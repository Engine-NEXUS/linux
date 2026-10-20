"""Unit tests for the CDP tier's endpoint-identity rules.

The CDP tier is scoped by DEBUG PORT, not by application: it sees whichever
app holds the lowest reachable port. These tests cover the rule that stops a
click meant for the window in front from landing in a background app that
happens to hold a debug port.

THE RULE (no heuristics, because every extra heuristic opened a new
background-window case):

  1. the endpoint must name itself and the name must match the expected
     application — checked first, because it is cheap;
  2. the candidate set is the targets whose title matches the expected
     foreground title (an EMPTY title cannot contradict the expected one, so
     it is kept and the focus probe decides);
  3. ALL candidates are probed CONCURRENTLY, under ONE deadline for the whole
     sweep, and the ports share one total budget;
  4. a target is chosen ONLY when EXACTLY ONE candidate answered `known` and
     `focus` — zero focused, more than one focused, or everything unknown
     all refuse.

They are pure decision tests with fakes — no browser, no websocket, no
accessibility bus — so they run anywhere the sidecar's dependencies are
installed.

Run:  python3 -m pytest server/test_cdp_identity.py -v
"""

from __future__ import annotations

import asyncio
import json
import os
import sys
import time

import pytest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

# The sidecar imports fastapi at module level. Where it is not installed the
# tests skip rather than fail: the sidecar cannot run there either.
pytest.importorskip("fastapi")

import cdp_server  # noqa: E402


# ── The application-name rule ────────────────────────────────────────────────


def test_the_same_application_matches_under_its_own_names():
    assert cdp_server.app_names_match("brave", "brave")
    assert cdp_server.app_names_match("code", "Visual Studio Code")
    assert cdp_server.app_names_match("Visual Studio Code", "code")


def test_a_name_contained_in_another_matches():
    # "code" is inside "vscode".
    assert cdp_server.app_names_match("code", "vscode")
    assert cdp_server.app_names_match("brave", "brave-browser")


def test_different_applications_do_not_match():
    assert not cdp_server.app_names_match("brave", "code")
    assert not cdp_server.app_names_match("code", "brave-browser")


def test_an_unnamed_application_never_matches():
    # An unidentified application is not evidence of anything.
    assert not cdp_server.app_names_match("", "brave")
    assert not cdp_server.app_names_match("brave", "")
    assert not cdp_server.app_names_match("", "")


def test_an_unnamed_endpoint_matches_nothing():
    # The endpoint has to NAME itself: an endpoint that cannot prove which
    # application it belongs to proves nothing.
    ident = {"kind": "chromium", "app": "", "title": "", "url": "", "product": ""}
    assert not cdp_server.app_matches("brave", ident)
    # No application was expected: nothing to contradict.
    assert cdp_server.app_matches("", ident)


# ── The title rule ──────────────────────────────────────────────────────────


def test_a_target_title_inside_the_window_title_matches():
    # A browser window is titled "<page title> - <app>".
    assert cdp_server.title_matches("GitHub - Brave", "GitHub")
    assert cdp_server.title_matches("GitHub", "GitHub")


def test_a_different_target_does_not_match():
    assert not cdp_server.title_matches("GitHub - Brave", "GitHub - Notifications")


def test_empty_titles_cannot_contradict_each_other():
    assert cdp_server.title_matches("", "GitHub")
    assert cdp_server.title_matches("GitHub - Brave", "")


# ── The command line → application name ──────────────────────────────────────


def test_the_application_name_comes_from_the_command_line():
    assert cdp_server._app_name_from_command_line(
        "/usr/bin/brave --remote-debugging-port=9222"
    ) == "brave"
    assert cdp_server._app_name_from_command_line(
        "C:\\Program Files\\Brave\\brave.exe --remote-debugging-port=9222"
    ) == "brave"
    assert cdp_server._app_name_from_command_line("") == ""
    assert cdp_server._app_name_from_command_line("   ") == ""


def test_a_linux_path_with_spaces_is_parsed():
    # An AppImage under a directory whose name contains a space. Splitting on
    # the first space would yield "/home/me/My" — which matches no
    # application and would make the endpoint look unidentified.
    assert cdp_server._app_name_from_command_line(
        "/home/me/My Apps/brave.AppImage --remote-debugging-port=9222"
    ) == "brave.appimage"


def test_a_quoted_path_is_parsed():
    assert cdp_server._app_name_from_command_line(
        '"C:\\Program Files\\Brave\\brave.exe" --inspect=9229'
    ) == "brave"
    assert cdp_server._app_name_from_command_line(
        '"/home/me/My Apps/brave" --remote-debugging-port=9222'
    ) == "brave"


def test_a_path_with_no_arguments_is_parsed():
    assert cdp_server._app_name_from_command_line("/usr/bin/brave") == "brave"


def test_a_double_dash_inside_a_path_is_not_an_argument():
    # "--" only starts an argument at a word boundary, so a directory named
    # "--weird" does not truncate the path.
    assert cdp_server._app_name_from_command_line(
        "/home/me/--weird/brave --remote-debugging-port=9222"
    ) == "brave"


# ── The focus probe ────────────────────────────────────────────────────────


def test_the_focus_script_calls_execute_javascript_on_the_webcontents():
    # `w` IS the WebContents (it comes from getAllWebContents().find), so it
    # has no `.webContents` property. Calling `w.webContents.executeJavaScript`
    # threw a TypeError, the probe reported "unknown", and EVERY Electron
    # endpoint was refused — the CDP tier is the only tier that can act
    # inside an Electron app, so the feature was dead.
    script = cdp_server._MAIN_FOCUS
    assert "w.executeJavaScript(" in script
    assert "w.webContents.executeJavaScript(" not in script
    # The renderer is looked up the same way the other bodies do it.
    assert "webContents.getAllWebContents().find(x => x.id === __WCID__)" in script


def test_a_target_that_never_answers_is_unknown_and_does_not_stall(monkeypatch):
    # A background tab the browser has FROZEN does not run renderer tasks, so
    # the probe never answers. It must be bounded and reported as unknown.
    class _NeverAnswers:
        def __init__(self, ws_url):
            self.ws_url = ws_url

        async def __aenter__(self):
            await asyncio.sleep(3600)  # a frozen renderer
            return self

        async def __aexit__(self, *exc):
            return None

    monkeypatch.setattr(cdp_server, "Cdp", _NeverAnswers)

    state = asyncio.run(
        cdp_server._page_focus_state({"webSocketDebuggerUrl": "ws://x"}, 0.05)
    )
    assert state["known"] is False
    assert state["focus"] is False


def test_a_refused_connection_is_unknown_not_a_server_error(monkeypatch):
    # `websockets.connect` raises OSError, which is not a CdpError: it used to
    # escape the probe and turn the intended 409 into an HTTP 500.
    class _Refuses:
        def __init__(self, ws_url):
            self.ws_url = ws_url

        async def __aenter__(self):
            raise OSError("connection refused")

        async def __aexit__(self, *exc):
            return None

    monkeypatch.setattr(cdp_server, "Cdp", _Refuses)

    state = asyncio.run(
        cdp_server._page_focus_state({"webSocketDebuggerUrl": "ws://x"}, 0.05)
    )
    assert state["known"] is False


# ── The candidate set ───────────────────────────────────────────────────────


def test_candidates_are_narrowed_by_the_expected_title_before_any_probe():
    # The narrowing is renderer-free and happens FIRST, so a browser with N
    # tabs costs one probe instead of N — and a frozen background tab is never
    # probed at all.
    tabs = [
        {"title": "GitHub - Notifications", "url": "https://github.com/notifications"},
        {"title": "GitHub", "url": "https://github.com/"},
    ]
    narrowed = cdp_server.narrow_by_title(tabs, "GitHub — Brave")
    assert len(narrowed) == 1
    assert narrowed[0]["title"] == "GitHub"


def test_an_empty_expected_title_keeps_every_candidate():
    tabs = [{"title": "A"}, {"title": "B"}]
    assert len(cdp_server.narrow_by_title(tabs, "")) == 2
    assert len(cdp_server.narrow_by_title(tabs, "   ")) == 2


def test_the_narrowing_uses_the_same_predicate_as_the_identity_gate():
    # The narrowing must never be STRICTER than the gate it pre-filters: it
    # uses the very same `title_matches`, so a candidate whose own title is
    # empty cannot contradict the expected one and is kept.
    tabs = [{"title": ""}, {"title": "GitHub"}]
    narrowed = cdp_server.narrow_by_title(tabs, "GitHub — Brave")
    assert len(narrowed) == 2
    assert all(
        cdp_server.title_matches("GitHub — Brave", str(t.get("title") or ""))
        for t in narrowed
    )


# ── The selection rule ──────────────────────────────────────────────────────
#
# The accessibility tree is per TARGET, so the identity check and the action
# have to use the SAME one. These tests use fake targets and fake focus
# states — no browser, no websocket.


def _state(focus: bool, visible: bool, known: bool = True) -> dict:
    return {"focus": focus, "visible": visible, "known": known, "timed_out": False}


def _timed_out() -> dict:
    return {"focus": False, "visible": False, "known": False, "timed_out": True}


def test_one_focused_tab_among_several_is_chosen():
    # Three tabs of the same browser; only the SECOND one has focus. Picking
    # the first (the old first-non-blank rule) would act in a background tab.
    tabs = [
        {"title": "Background tab", "url": "https://example.com/one"},
        {"title": "GitHub", "url": "https://github.com/"},
        {"title": "Another tab", "url": "https://example.com/two"},
    ]
    states = [_state(False, True), _state(True, True), _state(False, True)]
    chosen = cdp_server.choose_focused_target(tabs, states)
    assert chosen is not None
    assert chosen["title"] == "GitHub"


def test_a_visible_but_unfocused_target_is_refused():
    # "Visible" is NOT evidence of being in front: a background window's
    # active tab is visible, it is just not focused. Selecting on visibility
    # is what put a click in a background window.
    tabs = [{"title": "GitHub"}]
    assert cdp_server.choose_focused_target(tabs, [_state(False, True)]) is None


def test_two_visible_targets_without_focus_refuse():
    tabs = [{"title": "A"}, {"title": "B"}]
    states = [_state(False, True), _state(False, True)]
    assert cdp_server.choose_focused_target(tabs, states) is None


def test_two_focused_targets_refuse():
    # Ambiguity is a refusal, not a coin toss.
    tabs = [{"title": "A"}, {"title": "B"}]
    states = [_state(True, True), _state(True, True)]
    assert cdp_server.choose_focused_target(tabs, states) is None


def test_an_unknown_focus_state_does_not_qualify():
    tabs = [{"title": "GitHub"}]
    assert cdp_server.choose_focused_target(tabs, [_state(False, False, known=False)]) is None


def test_an_empty_titled_focused_tab_is_chosen():
    # A tab whose page has no <title> (a raw JSON view, the built-in PDF
    # viewer) must not be filtered out: the identity gate treats an empty
    # target title as a match, so the narrowing must too — and the focus
    # probe then decides.
    tabs = [{"title": "", "url": "https://example.com/data.json"}]
    kept = cdp_server.narrow_by_title(tabs, "Example — Brave")
    assert len(kept) == 1
    chosen = cdp_server.choose_focused_target(kept, [_state(True, True)])
    assert chosen is not None
    assert chosen["url"] == "https://example.com/data.json"


# ── A probe that did not answer in time proves nothing ─────────────────────


def test_a_title_matched_candidate_that_timed_out_is_refused():
    # The renderer was too busy to answer in time. That is NOT proof that it
    # is the foreground: nothing was proven, so nothing is chosen.
    tabs = [{"title": "GitHub", "url": "https://github.com/"}]
    kept = cdp_server.narrow_by_title(tabs, "GitHub — Brave")
    assert len(kept) == 1
    assert cdp_server.choose_focused_target(kept, [_timed_out()]) is None


def test_a_single_empty_titled_candidate_that_timed_out_is_refused():
    # Nothing proved which target is in front: an empty title cannot
    # contradict the expected one, and the probe never answered, so the
    # foreground is simply unknown.
    tabs = [{"title": "", "url": "https://example.com/data.json"}]
    kept = cdp_server.narrow_by_title(tabs, "Example — Brave")
    assert len(kept) == 1
    assert cdp_server.choose_focused_target(kept, [_timed_out()]) is None


def test_two_candidates_where_one_timed_out_are_refused():
    # Two survivors and no focus report: the timeout singles nothing out.
    tabs = [
        {"title": "GitHub", "url": "https://github.com/"},
        {"title": "GitHub - Notifications", "url": "https://github.com/n"},
    ]
    kept = cdp_server.narrow_by_title(tabs, "GitHub")
    assert len(kept) == 2
    assert (
        cdp_server.choose_focused_target(kept, [_timed_out(), _state(False, False)])
        is None
    )


# ── The refusal reason ──────────────────────────────────────────────────────


def test_the_reason_says_why_no_target_was_chosen():
    tabs = [{"title": "A"}, {"title": "B"}]
    # Nothing focused.
    assert "no target reports the focus" in cdp_server.foreground_reason(
        tabs, [_state(False, True), _state(False, True)]
    )
    # Two focused: ambiguity.
    assert "claim the focus" in cdp_server.foreground_reason(
        tabs, [_state(True, True), _state(True, True)]
    )
    # Everything unknown because nothing answered in time.
    assert "in time" in cdp_server.foreground_reason(tabs, [_timed_out(), _timed_out()])
    # No candidate at all.
    assert "no target matched" in cdp_server.foreground_reason([], [])


# ── The concurrent sweep ────────────────────────────────────────────────────


def test_the_sweep_probes_every_candidate_concurrently_under_one_deadline(monkeypatch):
    # Four frozen background tabs. Four SEQUENTIAL probes would cost four
    # deadlines; the sweep costs ONE, because the probes run in parallel.
    class _NeverAnswers:
        def __init__(self, ws_url):
            self.ws_url = ws_url

        async def __aenter__(self):
            await asyncio.sleep(3600)  # a frozen renderer
            return self

        async def __aexit__(self, *exc):
            return None

    monkeypatch.setattr(cdp_server, "Cdp", _NeverAnswers)

    targets = [{"webSocketDebuggerUrl": f"ws://x/{i}"} for i in range(4)]
    deadline = 0.5
    t0 = time.monotonic()
    states = asyncio.run(cdp_server._probe_all(targets, deadline))
    elapsed = time.monotonic() - t0

    assert len(states) == 4
    assert all(st["known"] is False for st in states)
    assert all(st["focus"] is False for st in states)
    # One deadline, not one per candidate.
    assert elapsed < deadline * 2
    # And nothing was chosen: no candidate proved it was in front.
    assert cdp_server.choose_focused_target(targets, states) is None


def test_a_frozen_candidate_does_not_discard_the_answers_of_the_others(monkeypatch):
    # One tab answers, one is frozen. The frozen one proves nothing, but it
    # must not throw away the answer of the tab that DID reply.
    async def _fake_inner(t: dict) -> dict:
        if t.get("frozen"):
            await asyncio.sleep(3600)
        return {"focus": True, "visible": True, "known": True, "timed_out": False}

    monkeypatch.setattr(cdp_server, "_page_focus_state_inner", _fake_inner)

    targets = [
        {"webSocketDebuggerUrl": "ws://x/1", "frozen": True},
        {"webSocketDebuggerUrl": "ws://x/2", "frozen": False},
    ]
    states = asyncio.run(cdp_server._probe_all(targets, 0.3))

    assert states[0]["known"] is False  # the frozen one proved nothing
    assert states[1]["known"] is True  # the answer survived
    chosen = cdp_server.choose_focused_target(targets, states)
    assert chosen is not None
    assert chosen["webSocketDebuggerUrl"] == "ws://x/2"


# ── The whole rule, with a fake endpoint ────────────────────────────────────


class _FakeCdp:
    """A CDP session that answers the focus probe from a table."""

    FOCUS: dict = {}

    def __init__(self, ws_url: str):
        self.ws_url = ws_url

    async def __aenter__(self):
        return self

    async def __aexit__(self, *exc):
        return None

    async def call(self, method: str, params: dict | None = None) -> dict:
        if method == "Runtime.evaluate":
            d = _FakeCdp.FOCUS.get(self.ws_url, {"focus": False, "vis": "hidden"})
            return {"result": {"value": json.dumps(d)}}
        raise AssertionError(f"unexpected CDP method {method}")


def test_the_whole_rule_picks_the_focused_tab_of_the_expected_window(monkeypatch):
    pages = [
        {
            "type": "page",
            "title": "GitHub - Notifications",
            "url": "https://github.com/n",
            "webSocketDebuggerUrl": "ws://x/1",
        },
        {
            "type": "page",
            "title": "GitHub",
            "url": "https://github.com/",
            "webSocketDebuggerUrl": "ws://x/2",
        },
        {
            "type": "page",
            "title": "GitHub - Issues",
            "url": "https://github.com/i",
            "webSocketDebuggerUrl": "ws://x/3",
        },
    ]
    monkeypatch.setattr(cdp_server, "http_list", lambda port: pages)
    monkeypatch.setattr(cdp_server, "Cdp", _FakeCdp)
    _FakeCdp.FOCUS = {
        "ws://x/1": {"focus": False, "vis": "hidden"},
        "ws://x/2": {"focus": True, "vis": "visible"},
        "ws://x/3": {"focus": False, "vis": "hidden"},
    }
    try:
        target, reason = asyncio.run(
            cdp_server.chromium_foreground_target(9222, None, "GitHub", 2.0)
        )
    finally:
        _FakeCdp.FOCUS = {}

    assert target is not None
    assert target["url"] == "https://github.com/"
    assert reason == ""


def test_the_whole_rule_refuses_when_no_tab_has_the_focus(monkeypatch):
    pages = [
        {
            "type": "page",
            "title": "GitHub",
            "url": "https://github.com/",
            "webSocketDebuggerUrl": "ws://x/1",
        },
        {
            "type": "page",
            "title": "GitHub - Issues",
            "url": "https://github.com/i",
            "webSocketDebuggerUrl": "ws://x/2",
        },
    ]
    monkeypatch.setattr(cdp_server, "http_list", lambda port: pages)
    monkeypatch.setattr(cdp_server, "Cdp", _FakeCdp)
    _FakeCdp.FOCUS = {
        "ws://x/1": {"focus": False, "vis": "visible"},
        "ws://x/2": {"focus": False, "vis": "visible"},
    }
    try:
        target, reason = asyncio.run(
            cdp_server.chromium_foreground_target(9222, None, "GitHub", 2.0)
        )
    finally:
        _FakeCdp.FOCUS = {}

    assert target is None
    assert reason


# ── The whole endpoint decision, with fakes ────────────────────────────────


def _identity(kind: str, app: str, title: str) -> dict:
    return {"kind": kind, "app": app, "title": title, "url": "", "product": ""}


def test_the_endpoint_must_prove_the_same_application():
    idn = _identity("chromium", "brave", "GitHub")
    assert cdp_server.app_names_match("brave", idn["app"])
    assert cdp_server.title_matches("GitHub - Brave", idn["title"])


def test_a_background_endpoint_does_not_prove_anything():
    # Brave in front; a background VS Code holding a debug port.
    idn = _identity("electron", "Visual Studio Code", "main.rs - nexus")
    assert not cdp_server.app_names_match("brave", idn["app"])


def test_an_app_name_mismatch_refuses_the_endpoint():
    # Brave in front; the debug port belongs to VS Code.
    ident = _identity("electron", "Visual Studio Code", "")
    target = {"title": "main.rs - nexus", "url": ""}
    assert not cdp_server.endpoint_matches("brave", "GitHub", ident, target)


def test_a_title_mismatch_refuses_the_endpoint():
    ident = _identity("chromium", "brave", "")
    target = {"title": "Some other tab", "url": ""}
    assert not cdp_server.endpoint_matches("brave", "GitHub", ident, target)


def test_an_unknown_foreground_target_refuses_the_endpoint():
    ident = _identity("chromium", "brave", "")
    assert not cdp_server.endpoint_matches("brave", "GitHub", ident, None)


def test_a_matching_endpoint_is_allowed():
    ident = _identity("chromium", "brave", "")
    target = {"title": "GitHub", "url": "https://github.com/"}
    assert cdp_server.endpoint_matches("brave", "GitHub - Brave", ident, target)


# ── The budgets ─────────────────────────────────────────────────────────────


def test_the_request_budget_stays_inside_the_client_timeout():
    # The Rust client gives the request 20 s; the whole request (all ports)
    # has to finish inside 15 s, so the ports share one budget that is
    # smaller than that.
    assert cdp_server.ACTIVATE_BUDGET < 15
    # And one port's sweep never gets more than its own deadline.
    assert cdp_server.FOCUS_SWEEP_DEADLINE <= cdp_server.ACTIVATE_BUDGET
