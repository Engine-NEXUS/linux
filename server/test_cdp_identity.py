"""Unit tests for the CDP tier's endpoint-identity rules.

The CDP tier is scoped by DEBUG PORT, not by application: it sees whichever
app holds the lowest reachable port. These tests cover the rule that stops a
click meant for the window in front from landing in a background app that
happens to hold a debug port.

They are pure decision tests with fakes — no browser, no websocket, no
accessibility bus — so they run anywhere the sidecar's dependencies are
installed.

Run:  python3 -m pytest server/test_cdp_identity.py -v
"""

from __future__ import annotations

import os
import sys

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


# ── The whole decision, with a fake endpoint ────────────────────────────────


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
