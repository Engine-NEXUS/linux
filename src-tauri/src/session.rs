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
    format!(
        "display session: {} ({}) [XDG_SESSION_TYPE={} WAYLAND_DISPLAY={} DISPLAY={}]",
        match k {
            SessionKind::Wayland => "wayland",
            SessionKind::X11 => "x11",
            SessionKind::Headless => "headless",
        },
        k.input_capability(),
        env_or_dash("XDG_SESSION_TYPE"),
        env_or_dash("WAYLAND_DISPLAY"),
        env_or_dash("DISPLAY"),
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
}