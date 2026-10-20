"""Unit tests for the AT-SPI tier's foreground-identity rules.

These are pure decision tests: they need no accessibility bus, which is what
makes them runnable on a build machine as well as on the desktop.

The rule under test is the one the audit of the Linux build found broken: on
the target machine **two** applications reported an ACTIVE frame at the same
time, and the old first-match rule picked one of them at random. The privacy
gate then compared *that* window against the deny-list, so which window was
protected depended on enumeration order.

Run:  python3 -m pytest server/test_atspi_foreground.py -v
"""

from __future__ import annotations

import os
import sys

import pytest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

# The sidecar imports fastapi at module level. Where it is not installed the
# tests skip rather than fail: the sidecar cannot run there either.
pytest.importorskip("fastapi")

import atspi_server  # noqa: E402


def _c(app: str, title: str) -> atspi_server.WindowIdentity:
    return atspi_server.WindowIdentity(app, title)


def test_one_active_frame_is_the_foreground():
    candidates = [_c("code", "main.rs — nexus")]
    assert atspi_server.pick_foreground(candidates) == candidates[0]


def test_several_active_frames_are_ambiguous_not_the_first_match():
    # The bug the audit found. Taking the first match here is what made the
    # deny-list check depend on enumeration order.
    candidates = [
        _c("portal-gtk", "Settings"),
        _c("code", "main.rs — nexus"),
    ]
    assert atspi_server.pick_foreground(candidates) is None


def test_three_active_frames_are_ambiguous_too():
    candidates = [_c("a", "1"), _c("b", "2"), _c("c", "3")]
    assert atspi_server.pick_foreground(candidates) is None


def test_no_candidates_is_no_foreground():
    assert atspi_server.pick_foreground([]) is None


def test_window_identity_carries_app_and_title():
    w = _c("firefox", "Example — Mozilla Firefox")
    assert w.app == "firefox"
    assert w.title == "Example — Mozilla Firefox"
    assert tuple(w) == ("firefox", "Example — Mozilla Firefox")


def test_cosmic_stub_returns_nothing_rather_than_guessing():
    # Until a Wayland client for `ext-foreign-toplevel-list-v1` +
    # `zcosmic_toplevel_info_v1` exists, the compositor source is a stub. It
    # must return None, never a guess — a wrong identity would mean the wrong
    # window is compared against the privacy deny-list.
    assert atspi_server.cosmic_active_toplevel() is None


def test_compositor_source_is_only_consulted_on_cosmic(monkeypatch):
    monkeypatch.delenv("XDG_CURRENT_DESKTOP", raising=False)
    assert atspi_server.is_cosmic_session() is False
    assert atspi_server.foreground_from_compositor() is None

    monkeypatch.setenv("XDG_CURRENT_DESKTOP", "COSMIC")
    assert atspi_server.is_cosmic_session() is True
    # The stub is honest, so the compositor source still answers "unknown".
    assert atspi_server.foreground_from_compositor() is None


def test_cosmic_detection_is_case_insensitive():
    # The variable is conventionally uppercase, but a session manager is free
    # to spell it otherwise.
    os.environ["XDG_CURRENT_DESKTOP"] = "cosmic"
    try:
        assert atspi_server.is_cosmic_session() is True
    finally:
        os.environ.pop("XDG_CURRENT_DESKTOP", None)
