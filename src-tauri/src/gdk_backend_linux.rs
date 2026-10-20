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
//! The choice is deliberate, never silent:
//! - an explicit `GDK_BACKEND` from the user always wins;
//! - `DISPLAY` must be set (XWayland is available) before X11 is forced —
//!   pointing GTK at an X server that does not exist would stop the app from
//!   starting at all;
//! - the decision is printed to stdout so the log says which backend the
//!   overlay is actually running on.
//!
//! The decision itself is a pure function (`choose`) so it can be unit-tested
//! without touching the environment.

/// What to do about the GTK backend for this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// The user set `GDK_BACKEND` themselves — leave it exactly as it is.
    RespectUser,
    /// No X display: there is no XWayland to fall back to, so GTK uses
    /// whatever it finds (native Wayland, or nothing at all).
    NativeOnly,
    /// Force X11 (XWayland) so always-on-top / skip-taskbar / positioning
    /// work the same way they do on Windows.
    ForceX11,
}

impl Backend {
    /// One-line description for the startup log.
    pub fn describe(self) -> &'static str {
        match self {
            Backend::RespectUser => "keeping the user's GDK_BACKEND",
            Backend::NativeOnly => "no X display — staying on the native GTK backend",
            Backend::ForceX11 => "XWayland found — running the overlay on GDK_BACKEND=x11",
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
/// * an existing `GDK_BACKEND` always wins (the user knows best);
/// * otherwise `DISPLAY` means XWayland is available → force `x11`;
/// * otherwise there is no X server → stay on whatever GTK finds.
pub fn choose(existing: Option<&str>, display: Option<&str>) -> Backend {
    if is_set(existing) {
        return Backend::RespectUser;
    }
    if is_set(display) {
        return Backend::ForceX11;
    }
    Backend::NativeOnly
}

/// Apply the decision to this process' environment and log it.
///
/// Called from `main()` before Tauri/GTK starts. Idempotent: a second call
/// sees the value we set and keeps it.
pub fn apply() -> Backend {
    let existing = std::env::var("GDK_BACKEND").ok();
    let display = std::env::var("DISPLAY").ok();
    let choice = choose(existing.as_deref(), display.as_deref());
    match choice {
        Backend::RespectUser => {
            let value = existing.as_deref().unwrap_or("").trim().to_string();
            println!("[GDK] GDK_BACKEND={value} was set — {}", choice.describe());
        }
        Backend::ForceX11 => {
            // Safe here: main() is single-threaded at this point and GTK has
            // not been initialised yet, so nothing else can be reading the
            // environment concurrently.
            std::env::set_var("GDK_BACKEND", "x11");
            println!(
                "[GDK] {} — always-on-top, taskbar hiding and window placement now work",
                choice.describe()
            );
        }
        Backend::NativeOnly => {
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
    fn an_explicit_backend_always_wins() {
        assert_eq!(choose(Some("wayland"), Some(":0")), Backend::RespectUser);
        assert_eq!(choose(Some("x11"), None), Backend::RespectUser);
        // Even an empty value is a deliberate "no opinion"? No — an empty
        // variable is treated as unset, like `session::detect_from_env` does.
        assert_eq!(choose(Some(""), Some(":0")), Backend::ForceX11);
        assert_eq!(choose(Some("   "), Some(":0")), Backend::ForceX11);
    }

    #[test]
    fn xwayland_means_x11_is_forced() {
        // The XWayland case: DISPLAY is set, so GTK can talk to the X server.
        assert_eq!(choose(None, Some(":0")), Backend::ForceX11);
        assert_eq!(choose(None, Some("host:10.0")), Backend::ForceX11);
    }

    #[test]
    fn no_display_means_native_only() {
        // A native Wayland session without XWayland: pointing GTK at X11
        // would stop the app from starting, so we must not.
        assert_eq!(choose(None, None), Backend::NativeOnly);
        assert_eq!(choose(None, Some("")), Backend::NativeOnly);
        assert_eq!(choose(None, Some("  ")), Backend::NativeOnly);
    }

    #[test]
    fn every_choice_has_a_description() {
        for b in [Backend::RespectUser, Backend::NativeOnly, Backend::ForceX11] {
            assert!(!b.describe().is_empty(), "{b:?}");
        }
    }
}
