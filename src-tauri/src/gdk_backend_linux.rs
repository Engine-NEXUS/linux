//! Linux display-backend bootstrap (GTK).
//!
//! NEXUS's overlay is a fullscreen transparent window that has to sit on top
//! of everything and must not appear in the taskbar. Under **native Wayland**
//! a client cannot ask the compositor for either: `always_on_top` and
//! `skip_taskbar` are ignored, and the window may not be positioned at all.
//! Under **X11** (or XWayland) both work, which is why the Linux side
//! shipped `GDK_BACKEND=x11` in the `.desktop` file its installer writes.
//!
//! That only covered one way in. `nexus start`, the dev script, a desktop
//! shortcut the user made by hand, the autostart entry — every one of them
//! started a native Wayland client and got a degraded overlay (the audit's
//! "Overlay window: degraded"). The decision has to be made where it covers
//! EVERY launch: `main()`, before Tauri initialises GTK.
//!
//! For that to work the launchers must not decide it either: `nexus.mjs`
//! and the installed `.desktop` entries used to export `GDK_BACKEND=x11`
//! unconditionally, which would point GTK at an X server that may not exist
//! and stop the app from starting. They no longer set it — this module is
//! the single source of truth.
//!
//! ## An inherited `GDK_BACKEND` is not a choice for NEXUS
//!
//! A Wayland desktop exports `GDK_BACKEND` for the **whole session**, so
//! every app in it inherits the value whether it wants Wayland or not. COSMIC
//! exports `GDK_BACKEND=wayland,x11`. Treating that as "the user asked for
//! Wayland" put the overlay on the native Wayland backend, where it cannot
//! be kept above the other windows — the orb was invisible behind VS Code.
//!
//! The rule is therefore:
//!
//! - `NEXUS_GDK_BACKEND` is set → use exactly that value as `GDK_BACKEND`.
//!   This is the explicit, NEXUS-scoped override for a user who really wants
//!   the native Wayland backend.
//! - otherwise `DISPLAY` is set (XWayland is available) → `GDK_BACKEND=x11`,
//!   **replacing** whatever the session exported. Pointing GTK at an X
//!   server that does not exist would stop the app from starting, which is
//!   why `DISPLAY` has to be there.
//! - otherwise → leave `GDK_BACKEND` alone: there is no X server, so GTK
//!   keeps whatever it finds (native Wayland).
//!
//! The decision is printed to stdout so the log says which backend the
//! overlay is actually running on and what it replaced.
//!
//! The decision itself is a pure function (`choose`) so it can be unit-tested
//! without touching the environment.

/// What to do about the GTK backend for this process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// `NEXUS_GDK_BACKEND` is set — use exactly this value as `GDK_BACKEND`.
    Override(String),
    /// Leave `GDK_BACKEND` exactly as it is: there is no X display, so GTK
    /// keeps whatever it finds (native Wayland).
    Keep,
    /// Force X11 (XWayland) so always-on-top / skip-taskbar / positioning
    /// work the same way they do on Windows. The value it replaces is kept
    /// for the log — a session-wide `GDK_BACKEND` is worth naming.
    ForceX11 {
        /// The inherited `GDK_BACKEND` that is being replaced, if any.
        replaced: Option<String>,
    },
}

impl Backend {
    /// One-line description for the startup log.
    pub fn describe(&self) -> String {
        match self {
            Backend::Override(value) => {
                format!("NEXUS_GDK_BACKEND={value} — using it as GDK_BACKEND")
            }
            Backend::Keep => {
                "no X display — leaving GDK_BACKEND alone (native GTK backend)".to_string()
            }
            Backend::ForceX11 {
                replaced: Some(old),
            } => format!("XWayland found — replacing the session's GDK_BACKEND='{old}' with x11"),
            Backend::ForceX11 { replaced: None } => {
                "XWayland found — running the overlay on GDK_BACKEND=x11".to_string()
            }
        }
    }
}

/// A variable that is set but empty counts as unset — some launchers export
/// empty strings rather than unsetting the variable.
fn is_set(value: Option<&str>) -> bool {
    value.map(str::trim).filter(|s| !s.is_empty()).is_some()
}

/// The pure decision (unit-tested): which GTK backend should this process use?
///
/// * an explicit `NEXUS_GDK_BACKEND` always wins — it is scoped to NEXUS,
///   so it is the user's own decision and not the desktop's;
/// * otherwise `DISPLAY` means XWayland is available → force `x11`, even
///   when the session exported a `GDK_BACKEND` of its own (COSMIC exports
///   `wayland,x11` for every app in the session; honouring it would put the
///   overlay on a backend that cannot keep it on top);
/// * otherwise there is no X server → leave `GDK_BACKEND` alone.
pub fn choose(
    nexus_override: Option<&str>,
    existing: Option<&str>,
    display: Option<&str>,
) -> Backend {
    if let Some(value) = nexus_override.map(str::trim).filter(|s| !s.is_empty()) {
        return Backend::Override(value.to_string());
    }
    if is_set(display) {
        let replaced = existing
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        return Backend::ForceX11 { replaced };
    }
    Backend::Keep
}

/// Apply the decision to this process' environment and log it.
///
/// Called from `main()` before Tauri/GTK starts. Idempotent: a second call
/// makes the same decision again.
pub fn apply() -> Backend {
    let nexus_override = std::env::var("NEXUS_GDK_BACKEND").ok();
    let existing = std::env::var("GDK_BACKEND").ok();
    let display = std::env::var("DISPLAY").ok();
    let choice = choose(
        nexus_override.as_deref(),
        existing.as_deref(),
        display.as_deref(),
    );
    match &choice {
        Backend::Override(value) => {
            // Safe here: main() is single-threaded at this point and GTK has
            // not been initialised yet, so nothing else can be reading the
            // environment concurrently.
            std::env::set_var("GDK_BACKEND", value);
            println!("[GDK] {}", choice.describe());
        }
        Backend::ForceX11 { .. } => {
            std::env::set_var("GDK_BACKEND", "x11");
            println!(
                "[GDK] {} — always-on-top, taskbar hiding and window placement now work",
                choice.describe()
            );
        }
        Backend::Keep => {
            println!(
                "[GDK] {} — the overlay cannot be kept on top or hidden from the taskbar",
                choice.describe()
            );
        }
    }
    choice
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nexus_override_wins() {
        // An explicit, NEXUS-scoped override beats everything else — even a
        // session that exports its own GDK_BACKEND and even the X display.
        assert_eq!(
            choose(Some("wayland"), Some("wayland,x11"), Some(":0")),
            Backend::Override("wayland".into())
        );
        // It is honoured even without an X display.
        assert_eq!(
            choose(Some("wayland"), None, None),
            Backend::Override("wayland".into())
        );
        assert_eq!(
            choose(Some("x11"), Some("wayland,x11"), Some(":0")),
            Backend::Override("x11".into())
        );
    }

    #[test]
    fn an_empty_override_is_not_an_override() {
        // An empty variable is treated as unset, like `session::detect_from_env`.
        assert_eq!(
            choose(Some(""), Some("wayland,x11"), Some(":0")),
            Backend::ForceX11 {
                replaced: Some("wayland,x11".into())
            }
        );
        assert_eq!(choose(Some("   "), None, None), Backend::Keep);
    }

    #[test]
    fn a_session_wayland_backend_is_replaced_when_xwayland_is_there() {
        // COSMIC exports GDK_BACKEND=wayland,x11 for the WHOLE session. That
        // is not a choice for NEXUS: on the native Wayland backend the
        // overlay cannot be kept on top of the other windows.
        assert_eq!(
            choose(None, Some("wayland,x11"), Some(":0")),
            Backend::ForceX11 {
                replaced: Some("wayland,x11".into())
            }
        );
        // Same when the session exported only "wayland".
        assert_eq!(
            choose(None, Some("wayland"), Some(":0")),
            Backend::ForceX11 {
                replaced: Some("wayland".into())
            }
        );
        // Nothing inherited — plain X11.
        assert_eq!(
            choose(None, None, Some(":0")),
            Backend::ForceX11 { replaced: None }
        );
        assert_eq!(
            choose(None, None, Some("host:10.0")),
            Backend::ForceX11 { replaced: None }
        );
    }

    #[test]
    fn no_display_means_the_backend_is_left_alone() {
        // No X server: pointing GTK at X11 would stop the app from starting,
        // so we must not — whatever the session set stays.
        assert_eq!(choose(None, None, None), Backend::Keep);
        assert_eq!(choose(None, Some("wayland,x11"), None), Backend::Keep);
        assert_eq!(choose(None, Some("wayland,x11"), Some("")), Backend::Keep);
        assert_eq!(choose(None, Some("wayland,x11"), Some("  ")), Backend::Keep);
    }

    #[test]
    fn every_choice_has_a_description() {
        for b in [
            Backend::Override("wayland".into()),
            Backend::Keep,
            Backend::ForceX11 {
                replaced: Some("wayland,x11".into()),
            },
            Backend::ForceX11 { replaced: None },
        ] {
            assert!(!b.describe().is_empty(), "{b:?}");
        }
    }
}
