//! Display-session detection.
//!
//! Why this exists: NEXUS could not tell what kind of session it was running in.
//! Every input path just tried X11 and, on failure, either did nothing or told
//! the user it had worked. `enigo` resolves to its `x11rb` backend by default
//! (`Cargo.toml:112` declares no features), so keyboard synthesis only ever
//! attempted XTEST — which under XWayland translates to libei and therefore
//! *requires portal consent*, and under native Wayland reaches nothing at all.
//!
//! `docs/features/research/22-…` recorded the consequence: no mouse actuation on
//! Linux, keyboard effectively X11-only, and `focus_window` returning `true`
//! while doing nothing.
//!
//! This module is the first fix: make the session type explicit and cheap to
//! query, so callers can choose a backend and explain a failure instead of
//! going silent.
//!
//! Deliberately detection-only. It does **not** synthesise input, and it does
//! not decide policy — see [`SessionKind::input_capability`] for what each
//! session type can actually do, and [`SessionKind::describe`] for a
//! user-facing explanation.
//!
//! Cached because it reads env vars that cannot change during a session, and it
//! is consulted on every input attempt.
//!
//! `docs/features/research/21-…` §4 is the source for the per-compositor
//! capability claims: the `org.freedesktop.portal.RemoteDesktop` backend is
//! implemented by `xdg-desktop-portal-gnome` and `-kde` only, and there is no
//! working backend on wlroots compositors (Sway, HyPrland).

use std::sync::OnceLock;

/// Which display-server protocol this process is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    /// Native Wayland. Compositor mediates all input; no XTEST.
    Wayland,
    /// X11. XTEST available. Under XWayland this maps to libei via the portal.
    X11,
    /// Neither variable is set. Typically a TTY, a service, or a broken env.
    Headless,
}

impl SessionKind {
    /// True when XTEST-style input synthesis could plausibly work.
    ///
    /// This means *attempt it*, not *it will work*. Under XWayland the call is
    /// intercepted and routed through the RemoteDesktop portal, so the first
    /// attempt may raise a consent dialog.
    pub fn has_x11_input(self) -> bool {
        matches!(self, SessionKind::X11)
    }

    /// True when native Wayland input is in play, so synthetic input needs
    /// libei/portal or `/dev/uinput` rather than XTEST.
    pub fn is_wayland(self) -> bool {
        matches!(self, SessionKind::Wayland)
    }

    /// What input synthesis can be attempted here, for error messages.
    pub fn input_capability(self) -> &'static str {
        match self {
            SessionKind::Wayland => {
                "native Wayland — XTEST is not available; needs libei via the \
                 RemoteDesktop portal, or /dev/uinput on wlroots compositors"
            }
            SessionKind::X11 => "X11 — XTEST available (under XWayland this routes through the portal)",
            SessionKind::Headless => {
                "no display — neither WAYLAND_DISPLAY nor DISPLAY is set"
            }
        }
    }

    /// Short user-facing description, for TTS and logs.
    pub fn describe(self) -> &'static str {
        match self {
            SessionKind::Wayland => "a native Wayland session",
            SessionKind::X11 => "an X11 session",
            SessionKind::Headless => "a session with no display",
        }
    }
}

/// Where a trustworthy "which window is in front" answer can come from.
///
/// This is NOT the same question as [`SessionKind`]. `SessionKind` answers
/// "which input backend can I use"; this answers "can I name the foreground
/// window, and from what". The distinction matters because a Wayland session
/// that also exports an X11 display (XWayland) is an X11 *input* session whose
/// X11 *window list* is a lie: `xdotool` and `wmctrl` see only the X11
/// compatibility windows, so a native Wayland window in front of them is
/// invisible, and their "active window" is not the foreground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForegroundSource {
    /// A real X server: X11 tools see every window in the session.
    X11,
    /// XWayland — a Wayland compositor exporting an X11 compatibility display.
    /// X11 tools see only the X11 windows, so they cannot name the foreground.
    XWayland,
    /// Native Wayland. Only the compositor (or AT-SPI) can name the foreground.
    Wayland,
    /// No display at all.
    Headless,
}

impl ForegroundSource {
    /// Whether an X11 tool (`xdotool`, `wmctrl`) can be trusted to report the
    /// foreground window.
    ///
    /// `false` for XWayland on purpose: those tools answer with the active
    /// *X11* window, which on an XWayland session is frequently not the window
    /// the user is looking at. The policy gate compares the OBSERVED identity
    /// against a deny-list, so a wrong identity is a wrong decision.
    pub fn trusts_x11_tools(self) -> bool {
        matches!(self, ForegroundSource::X11)
    }

    /// Whether the compositor itself has to be asked for the foreground
    /// (there is no portable X11 shortcut on those sessions).
    pub fn needs_compositor(self) -> bool {
        matches!(self, ForegroundSource::Wayland | ForegroundSource::XWayland)
    }
}

/// Classify where the foreground identity can come from. Pure + unit-tested.
///
/// The rules, in order:
/// 1. `XDG_SESSION_TYPE=wayland` → native Wayland, whatever else is set.
/// 2. A Wayland socket AND an X display → XWayland. The display manager says
///    "x11" there, but a compositor is running: the X11 tools only see the
///    compatibility windows.
/// 3. An X display only → a real X server.
/// 4. A Wayland socket only → native Wayland.
pub fn classify_foreground_source(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> ForegroundSource {
    let non_empty = |v: Option<&str>| v.map(str::trim).filter(|s| !s.is_empty()).is_some();

    if session_type
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
    {
        return ForegroundSource::Wayland;
    }
    let has_wl = non_empty(wayland_display);
    let has_x11 = non_empty(display);
    match (has_wl, has_x11) {
        (true, true) => ForegroundSource::XWayland,
        (true, false) => ForegroundSource::Wayland,
        (false, true) => ForegroundSource::X11,
        (false, false) => ForegroundSource::Headless,
    }
}

static FOREGROUND_SOURCE: OnceLock<ForegroundSource> = OnceLock::new();

/// Where this session's foreground identity can come from. Detected once.
pub fn foreground_source() -> ForegroundSource {
    *FOREGROUND_SOURCE.get_or_init(|| {
        classify_foreground_source(
            std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
            std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
            std::env::var("DISPLAY").ok().as_deref(),
        )
    })
}

/// True on the COSMIC desktop (Pop!_OS), where the compositor publishes the
/// activated toplevel through `ext-foreign-toplevel-list-v1` +
/// `zcosmic_toplevel_info_v1` rather than through AT-SPI.
///
/// COSMIC's own applications are absent from the accessibility bus, so on
/// that desktop AT-SPI cannot name the foreground at all — the compositor is
/// the only source. See `server/atspi_server.py::cosmic_active_toplevel`.
pub fn is_cosmic() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .map(|d| d.to_ascii_uppercase().contains("COSMIC"))
        .unwrap_or(false)
}

/// Detect the session kind from the environment.
///
/// `XDG_SESSION_TYPE` is authoritative when set — it is what the display
/// manager sets and is the only variable that distinguishes native Wayland from
/// XWayland. `WAYLAND_DISPLAY` is the fallback. `DISPLAY` alone means X11 (or
/// XWayland, which still provides XTEST).
///
/// A variable that is set but empty is treated as unset, matching shell
/// convention — some launchers export empty strings.
fn detect_from_env(
    xdg_session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> SessionKind {
    // `trim` returns a borrow of the input, so the predicate must borrow from
    // the same lifetime as the argument.
    fn non_empty(v: Option<&str>) -> Option<&str> {
        v.map(str::trim).filter(|s| !s.is_empty())
    }

    match non_empty(xdg_session_type).map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("wayland") => return SessionKind::Wayland,
        Some("x11") => return SessionKind::X11,
        // A display manager advertising something else (e.g. "tty", or a
        // Mir session) — fall through to the socket variables rather than
        // trusting it blindly.
        _ => {}
    }

    if non_empty(wayland_display).is_some() {
        return SessionKind::Wayland;
    }
    if non_empty(display).is_some() {
        return SessionKind::X11;
    }
    SessionKind::Headless
}

static CACHED: OnceLock<SessionKind> = OnceLock::new();

/// The session kind for this process. Detected once, then cached.
pub fn kind() -> SessionKind {
    *CACHED.get_or_init(|| {
        detect_from_env(
            std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
            std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
            std::env::var("DISPLAY").ok().as_deref(),
        )
    })
}

/// Clear the cache. Tests only — the environment cannot change in production.
#[cfg(test)]
pub fn reset_cache_for_test() {
    // OnceLock has no reset; tests call `detect_from_env` directly instead.
    // This exists so a future refactor that moves to a RwLock has a seam.
    let _ = &CACHED;
}

/// Convenience: are we running under a Wayland session?
pub fn is_wayland() -> bool {
    kind().is_wayland()
}

/// Convenience: is XTEST worth attempting?
pub fn has_x11_input() -> bool {
    kind().has_x11_input()
}

/// One-line diagnostic, appended to `nexus check` output.
pub fn diagnostic_line() -> String {
    let k = kind();
    let src = foreground_source();
    format!(
        "display session: {} ({}) [XDG_SESSION_TYPE={} WAYLAND_DISPLAY={} DISPLAY={}{}{}]",
        match k {
            SessionKind::Wayland => "wayland",
            SessionKind::X11 => "x11",
            SessionKind::Headless => "headless",
        },
        k.input_capability(),
        env_or_dash("XDG_SESSION_TYPE"),
        env_or_dash("WAYLAND_DISPLAY"),
        env_or_dash("DISPLAY"),
        if is_cosmic() {
            " desktop=COSMIC"
        } else {
            ""
        },
        if src.needs_compositor() {
            " foreground=compositor-only"
        } else {
            ""
        },
    )
}

fn env_or_dash(name: &str) -> String {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => "-".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_session_type_wayland_wins() {
        // Even with DISPLAY set — which is exactly the XWayland case — an
        // explicit wayland session type must win, otherwise every XWayland user
        // would be told they are on native Wayland.
        assert_eq!(
            detect_from_env(Some("wayland"), Some("wayland-0"), Some(":0")),
            SessionKind::Wayland
        );
    }

    #[test]
    fn xdg_session_type_x11_wins_over_wayland_socket() {
        // XWayland sets both WAYLAND_DISPLAY and XDG_SESSION_TYPE=x11 for
        // X11 clients in some setups.
        assert_eq!(
            detect_from_env(Some("x11"), Some("wayland-0"), Some(":0")),
            SessionKind::X11
        );
    }

    #[test]
    fn session_type_is_case_insensitive() {
        assert_eq!(
            detect_from_env(Some("WayLand"), None, Some(":0")),
            SessionKind::Wayland
        );
    }

    #[test]
    fn falls_back_to_wayland_socket() {
        assert_eq!(detect_from_env(None, Some("wayland-0"), Some(":0")), SessionKind::Wayland);
        assert_eq!(detect_from_env(None, Some("wayland-1"), None), SessionKind::Wayland);
    }

    #[test]
    fn falls_back_to_x11_socket() {
        assert_eq!(detect_from_env(None, None, Some(":0")), SessionKind::X11);
        assert_eq!(detect_from_env(None, None, Some("host:10.0")), SessionKind::X11);
    }

    #[test]
    fn empty_values_are_treated_as_unset() {
        // Some launchers export empty strings rather than unsetting.
        assert_eq!(detect_from_env(Some(""), Some(""), Some("")), SessionKind::Headless);
        assert_eq!(detect_from_env(Some("  "), None, Some(":0")), SessionKind::X11);
    }

    #[test]
    fn headless_when_nothing_is_set() {
        assert_eq!(detect_from_env(None, None, None), SessionKind::Headless);
    }

    #[test]
    fn unknown_session_type_falls_through_to_sockets() {
        // A Mir or tty session: don't trust the label, ask the sockets.
        assert_eq!(detect_from_env(Some("mir"), None, Some(":0")), SessionKind::X11);
        assert_eq!(detect_from_env(Some("tty"), Some("wayland-0"), None), SessionKind::Wayland);
        assert_eq!(detect_from_env(Some("weird"), None, None), SessionKind::Headless);
    }

    #[test]
    fn capability_predicates_agree_with_variants() {
        assert!(SessionKind::X11.has_x11_input());
        assert!(!SessionKind::Wayland.has_x11_input());
        assert!(!SessionKind::Headless.has_x11_input());

        assert!(SessionKind::Wayland.is_wayland());
        assert!(!SessionKind::X11.is_wayland());
        assert!(!SessionKind::Headless.is_wayland());
    }

    #[test]
    fn every_variant_has_a_non_empty_description() {
        // Guards against a new variant being added without prose.
        for k in [SessionKind::Wayland, SessionKind::X11, SessionKind::Headless] {
            assert!(!k.describe().is_empty());
            assert!(!k.input_capability().is_empty());
        }
    }

    #[test]
    fn diagnostic_line_mentions_all_three_variables() {
        let line = diagnostic_line();
        for needle in ["XDG_SESSION_TYPE", "WAYLAND_DISPLAY", "DISPLAY", "display session"] {
            assert!(line.contains(needle), "missing {needle} in {line}");
        }
    }

    // ── Foreground source: which tool may name the foreground window ──────

    #[test]
    fn plain_x11_session_trusts_x11_tools() {
        let s = classify_foreground_source(Some("x11"), None, Some(":0"));
        assert_eq!(s, ForegroundSource::X11);
        assert!(s.trusts_x11_tools());
        assert!(!s.needs_compositor());
    }

    #[test]
    fn native_wayland_never_trusts_x11_tools() {
        let s = classify_foreground_source(Some("wayland"), Some("wayland-0"), Some(":0"));
        assert_eq!(s, ForegroundSource::Wayland);
        assert!(!s.trusts_x11_tools());
        assert!(s.needs_compositor());
    }

    #[test]
    fn xwayland_is_not_plain_x11() {
        // The case that matters: a Wayland session that ALSO exports DISPLAY.
        // `xdotool` answers with the active X11 window, which is not the
        // foreground of the session — the compositor or AT-SPI is.
        let s = classify_foreground_source(Some("x11"), Some("wayland-0"), Some(":0"));
        assert_eq!(s, ForegroundSource::XWayland);
        assert!(
            !s.trusts_x11_tools(),
            "XWayland must not be treated as a plain X11 session for foreground identity"
        );
        assert!(s.needs_compositor());
    }

    #[test]
    fn xwayland_is_detected_without_a_session_type() {
        let s = classify_foreground_source(None, Some("wayland-0"), Some(":0"));
        assert_eq!(s, ForegroundSource::XWayland);
    }

    #[test]
    fn wayland_socket_without_x11_is_native_wayland() {
        let s = classify_foreground_source(None, Some("wayland-0"), None);
        assert_eq!(s, ForegroundSource::Wayland);
        assert!(!s.trusts_x11_tools());
    }

    #[test]
    fn headless_has_no_foreground_source() {
        let s = classify_foreground_source(None, None, None);
        assert_eq!(s, ForegroundSource::Headless);
        assert!(!s.trusts_x11_tools());
        assert!(!s.needs_compositor());
    }

    #[test]
    fn foreground_source_session_type_is_case_insensitive() {
        assert_eq!(
            classify_foreground_source(Some("WayLand"), None, Some(":0")),
            ForegroundSource::Wayland
        );
    }
}