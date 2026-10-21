//! Linux display-backend bootstrap (GTK).
//!
//! NEXUS's overlay is a fullscreen transparent window that has to sit on top
//! of everything and must not appear in the taskbar. Under **native Wayland**
//! with a compositor that supports `zwlr_layer_shell_v1` (COSMIC, KDE Plasma,
//! Hyprland, Sway, river, niri, Wayfire, labwc) AND when `libgtk-layer-shell.so.0`
//! is installed on the machine, GTK runs natively on Wayland and turns the stage
//! into an overlay layer surface using `gtk-layer-shell`.
//!
//! If `libgtk-layer-shell.so.0` is missing, or under compositors that do not support
//! layer-shell (such as GNOME Mutter) or pure **X11**, the fallback path forces
//! `GDK_BACKEND=x11` (XWayland) and configures the stage as a non-focusable
//! notification window with input shape.
//!
//! The decision has to be made where it covers EVERY launch: `main()`, before
//! Tauri initialises GTK.
//!
//! The rule is therefore:
//!
//! - `NEXUS_GDK_BACKEND` is set → use exactly that value as `GDK_BACKEND`.
//!   This is the explicit, NEXUS-scoped override for a user.
//! - otherwise a Wayland session whose `XDG_CURRENT_DESKTOP` supports layer-shell
//!   AND `libgtk-layer-shell.so.0` is installed → `GDK_BACKEND=wayland` (native Wayland).
//! - otherwise `DISPLAY` is set (XWayland or X11 is available) → `GDK_BACKEND=x11`,
//!   replacing whatever the session exported.
//! - otherwise → leave `GDK_BACKEND` alone: there is no X server.
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
    /// Native Wayland: session is Wayland and compositor supports zwlr_layer_shell_v1.
    Wayland {
        desktop: String,
    },
    /// Leave `GDK_BACKEND` exactly as it is: there is no X display, so GTK
    /// keeps whatever it finds (native Wayland).
    Keep,
    /// Force X11 (XWayland) so always-on-top / skip-taskbar / positioning
    /// work the same way they do on Windows. The value it replaces is kept
    /// for the log — a session-wide `GDK_BACKEND` is worth naming.
    ForceX11 {
        /// The inherited `GDK_BACKEND` that is being replaced, if any.
        replaced: Option<String>,
        /// Whether layer-shell was supported by the desktop but skipped because
        /// libgtk-layer-shell.so.0 was missing.
        missing_layer_shell: bool,
    },
}

impl Backend {
    /// One-line description for the startup log.
    pub fn describe(&self) -> String {
        match self {
            Backend::Override(value) => {
                format!("NEXUS_GDK_BACKEND={value} — using it as GDK_BACKEND")
            }
            Backend::Wayland { desktop } => {
                format!("Wayland session on {desktop} (layer-shell supported) — running natively on GDK_BACKEND=wayland")
            }
            Backend::Keep => {
                "no X display — leaving GDK_BACKEND alone (native GTK backend)".to_string()
            }
            Backend::ForceX11 {
                replaced: Some(old),
                missing_layer_shell: true,
            } => {
                format!(
                    "layer-shell compositor detected but libgtk-layer-shell.so.0 is missing — replacing the session's GDK_BACKEND='{old}' with x11 (hint: sudo apt install libgtk-layer-shell0)"
                )
            }
            Backend::ForceX11 {
                replaced: None,
                missing_layer_shell: true,
            } => {
                "layer-shell compositor detected but libgtk-layer-shell.so.0 is missing — running on GDK_BACKEND=x11 (hint: sudo apt install libgtk-layer-shell0)".to_string()
            }
            Backend::ForceX11 {
                replaced: Some(old),
                missing_layer_shell: false,
            } => format!("XWayland found — replacing the session's GDK_BACKEND='{old}' with x11"),
            Backend::ForceX11 {
                replaced: None,
                missing_layer_shell: false,
            } => "XWayland found — running the overlay on GDK_BACKEND=x11".to_string(),
        }
    }
}

/// A variable that is set but empty counts as unset — some launchers export
/// empty strings rather than unsetting the variable.
fn is_set(value: Option<&str>) -> bool {
    value.map(str::trim).filter(|s| !s.is_empty()).is_some()
}

/// Check if a desktop environment / compositor name belongs to the allowlist of
/// compositors supporting `zwlr_layer_shell_v1`.
///
/// Supported: COSMIC, KDE / Plasma, Hyprland, Sway, river, niri, Wayfire, labwc.
pub fn is_layer_shell_desktop(desktop: &str) -> bool {
    let lower = desktop.to_ascii_lowercase();
    for part in lower.split(':') {
        let part = part.trim();
        match part {
            "cosmic" | "kde" | "plasma" | "hyprland" | "sway" | "river" | "niri" | "wayfire" | "labwc" => return true,
            _ => {}
        }
    }
    for token in lower.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
        match token {
            "cosmic" | "kde" | "plasma" | "hyprland" | "sway" | "river" | "niri" | "wayfire" | "labwc" => return true,
            _ => {}
        }
    }
    false
}

/// True if this session is running on Wayland.
pub fn is_wayland_session(session_type: Option<&str>, wayland_display: Option<&str>) -> bool {
    fn non_empty(v: Option<&str>) -> Option<&str> {
        v.map(str::trim).filter(|s| !s.is_empty())
    }
    if let Some(st) = non_empty(session_type) {
        if st.eq_ignore_ascii_case("wayland") {
            return true;
        }
        if st.eq_ignore_ascii_case("x11") {
            return false;
        }
    }
    non_empty(wayland_display).is_some()
}

/// The pure decision (unit-tested): which GTK backend should this process use?
///
/// * an explicit `NEXUS_GDK_BACKEND` always wins — it is scoped to NEXUS;
/// * otherwise, if running under a Wayland session whose `XDG_CURRENT_DESKTOP`
///   supports `zwlr_layer_shell_v1` (COSMIC, KDE Plasma, Hyprland, Sway, river,
///   niri, Wayfire, labwc) AND `layer_shell_lib` is true, select native Wayland (`Backend::Wayland`);
/// * otherwise if `DISPLAY` is available, force `x11` (recording whether layer-shell
///   was skipped due to a missing library);
/// * otherwise leave `GDK_BACKEND` alone.
pub fn choose(
    nexus_override: Option<&str>,
    existing: Option<&str>,
    display: Option<&str>,
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    desktop: Option<&str>,
    layer_shell_lib: bool,
) -> Backend {
    if let Some(value) = nexus_override.map(str::trim).filter(|s| !s.is_empty()) {
        return Backend::Override(value.to_string());
    }

    let is_wayland = is_wayland_session(session_type, wayland_display);
    let mut missing_layer_shell = false;
    if is_wayland {
        if let Some(d) = desktop.map(str::trim).filter(|s| !s.is_empty()) {
            if is_layer_shell_desktop(d) {
                if layer_shell_lib {
                    return Backend::Wayland { desktop: d.to_string() };
                } else {
                    missing_layer_shell = true;
                }
            }
        }
    }

    if is_set(display) {
        let replaced = existing
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        return Backend::ForceX11 {
            replaced,
            missing_layer_shell,
        };
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
    let session_type = std::env::var("XDG_SESSION_TYPE").ok();
    let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    let layer_shell_lib = unsafe {
        libloading::Library::new("libgtk-layer-shell.so.0")
            .or_else(|_| libloading::Library::new("libgtk-layer-shell.so"))
            .is_ok()
    };
    let choice = choose(
        nexus_override.as_deref(),
        existing.as_deref(),
        display.as_deref(),
        session_type.as_deref(),
        wayland_display.as_deref(),
        desktop.as_deref(),
        layer_shell_lib,
    );
    match &choice {
        Backend::Override(value) => {
            // Safe here: main() is single-threaded at this point and GTK has
            // not been initialised yet, so nothing else can be reading the
            // environment concurrently.
            std::env::set_var("GDK_BACKEND", value);
            println!("[GDK] {}", choice.describe());
        }
        Backend::Wayland { .. } => {
            std::env::set_var("GDK_BACKEND", "wayland");
            println!(
                "[GDK] {} — layer-shell overlay surface will be used",
                choice.describe()
            );
        }
        Backend::ForceX11 { missing_layer_shell, .. } => {
            std::env::set_var("GDK_BACKEND", "x11");
            println!(
                "[GDK] {} — always-on-top, taskbar hiding and window placement now work",
                choice.describe()
            );
            if *missing_layer_shell {
                static PRINT_HINT_ONCE: std::sync::Once = std::sync::Once::new();
                PRINT_HINT_ONCE.call_once(|| {
                    println!("[GDK] Install hint: sudo apt install libgtk-layer-shell0");
                });
            }
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
            choose(Some("wayland"), Some("wayland,x11"), Some(":0"), Some("wayland"), Some("wayland-0"), Some("COSMIC"), true),
            Backend::Override("wayland".into())
        );
        // It is honoured even without an X display.
        assert_eq!(
            choose(Some("wayland"), None, None, None, None, None, false),
            Backend::Override("wayland".into())
        );
        assert_eq!(
            choose(Some("x11"), Some("wayland,x11"), Some(":0"), Some("wayland"), Some("wayland-0"), Some("COSMIC"), true),
            Backend::Override("x11".into())
        );
    }

    #[test]
    fn an_empty_override_is_not_an_override() {
        // An empty variable is treated as unset.
        assert_eq!(
            choose(Some(""), Some("wayland,x11"), Some(":0"), Some("wayland"), Some("wayland-0"), Some("COSMIC"), true),
            Backend::Wayland { desktop: "COSMIC".into() }
        );
        assert_eq!(choose(Some("   "), None, None, None, None, None, false), Backend::Keep);
    }

    #[test]
    fn cosmic_wayland_lib_present_selects_wayland() {
        // COSMIC on Wayland with lib present selects native Wayland even when DISPLAY is present.
        assert_eq!(
            choose(None, Some("wayland,x11"), Some(":0"), Some("wayland"), Some("wayland-0"), Some("COSMIC"), true),
            Backend::Wayland { desktop: "COSMIC".into() }
        );
        // Pop!_OS colon-separated desktop
        assert_eq!(
            choose(None, None, Some(":0"), Some("wayland"), Some("wayland-0"), Some("pop:COSMIC"), true),
            Backend::Wayland { desktop: "pop:COSMIC".into() }
        );
        // Other layer-shell compositors
        for name in &["KDE", "Plasma", "KDE Plasma", "Hyprland", "sway", "Sway", "river", "niri", "Wayfire", "labwc"] {
            assert_eq!(
                choose(None, None, Some(":0"), Some("wayland"), Some("wayland-0"), Some(name), true),
                Backend::Wayland { desktop: (*name).into() },
                "failed for compositor {name}"
            );
        }
    }

    #[test]
    fn cosmic_wayland_lib_missing_falls_back_to_x11() {
        // When libgtk-layer-shell.so.0 is missing on COSMIC, fall back to ForceX11 with hint flag.
        assert_eq!(
            choose(None, Some("wayland,x11"), Some(":0"), Some("wayland"), Some("wayland-0"), Some("COSMIC"), false),
            Backend::ForceX11 {
                replaced: Some("wayland,x11".into()),
                missing_layer_shell: true,
            }
        );
    }

    #[test]
    fn gnome_wayland_lib_present_forces_x11() {
        // GNOME Mutter does NOT support layer-shell: even with the library present,
        // it falls back to X11 when DISPLAY is available (missing_layer_shell = false).
        assert_eq!(
            choose(None, Some("wayland,x11"), Some(":0"), Some("wayland"), Some("wayland-0"), Some("GNOME"), true),
            Backend::ForceX11 {
                replaced: Some("wayland,x11".into()),
                missing_layer_shell: false,
            }
        );
        assert_eq!(
            choose(None, None, Some(":0"), Some("wayland"), Some("wayland-0"), Some("ubuntu:GNOME"), true),
            Backend::ForceX11 {
                replaced: None,
                missing_layer_shell: false,
            }
        );
    }

    #[test]
    fn a_session_wayland_backend_is_replaced_when_xwayland_is_there() {
        // Restored old test: when layer-shell is not usable (or no layer-shell desktop),
        // COSMIC or another desktop exporting GDK_BACKEND=wayland,x11 has it replaced by x11.
        assert_eq!(
            choose(None, Some("wayland,x11"), Some(":0"), None, None, None, false),
            Backend::ForceX11 {
                replaced: Some("wayland,x11".into()),
                missing_layer_shell: false,
            }
        );
        // Same when the session exported only "wayland" without layer-shell usability.
        assert_eq!(
            choose(None, Some("wayland"), Some(":0"), None, None, None, false),
            Backend::ForceX11 {
                replaced: Some("wayland".into()),
                missing_layer_shell: false,
            }
        );
        // Nothing inherited — plain X11.
        assert_eq!(
            choose(None, None, Some(":0"), None, None, None, false),
            Backend::ForceX11 {
                replaced: None,
                missing_layer_shell: false,
            }
        );
    }

    #[test]
    fn pure_x11_session_forces_x11() {
        // When session type is x11 or no wayland socket is available.
        assert_eq!(
            choose(None, None, Some(":0"), Some("x11"), None, Some("COSMIC"), true),
            Backend::ForceX11 {
                replaced: None,
                missing_layer_shell: false,
            }
        );
        assert_eq!(
            choose(None, Some("x11"), Some(":0"), None, None, Some("XFCE"), false),
            Backend::ForceX11 {
                replaced: Some("x11".into()),
                missing_layer_shell: false,
            }
        );
    }

    #[test]
    fn no_display_means_the_backend_is_left_alone() {
        // No X server and not a layer-shell Wayland session.
        assert_eq!(choose(None, None, None, None, None, None, false), Backend::Keep);
        assert_eq!(choose(None, Some("wayland,x11"), None, None, None, None, false), Backend::Keep);
        assert_eq!(choose(None, Some("wayland,x11"), Some(""), None, None, None, false), Backend::Keep);
        assert_eq!(choose(None, Some("wayland,x11"), Some("  "), None, None, None, false), Backend::Keep);
    }

    #[test]
    fn every_choice_has_a_description() {
        for b in [
            Backend::Override("wayland".into()),
            Backend::Wayland { desktop: "COSMIC".into() },
            Backend::Keep,
            Backend::ForceX11 {
                replaced: Some("wayland,x11".into()),
                missing_layer_shell: false,
            },
            Backend::ForceX11 {
                replaced: None,
                missing_layer_shell: false,
            },
            Backend::ForceX11 {
                replaced: Some("wayland,x11".into()),
                missing_layer_shell: true,
            },
            Backend::ForceX11 {
                replaced: None,
                missing_layer_shell: true,
            },
        ] {
            assert!(!b.describe().is_empty(), "{b:?}");
        }
    }

    #[test]
    fn test_desktop_allowlist() {
        assert!(is_layer_shell_desktop("COSMIC"));
        assert!(is_layer_shell_desktop("cosmic"));
        assert!(is_layer_shell_desktop("pop:COSMIC"));
        assert!(is_layer_shell_desktop("KDE"));
        assert!(is_layer_shell_desktop("KDE:Plasma"));
        assert!(is_layer_shell_desktop("Hyprland"));
        assert!(is_layer_shell_desktop("sway"));
        assert!(is_layer_shell_desktop("river"));
        assert!(is_layer_shell_desktop("niri"));
        assert!(is_layer_shell_desktop("wayfire"));
        assert!(is_layer_shell_desktop("labwc"));

        assert!(!is_layer_shell_desktop("GNOME"));
        assert!(!is_layer_shell_desktop("ubuntu:GNOME"));
        assert!(!is_layer_shell_desktop("XFCE"));
        assert!(!is_layer_shell_desktop("MATE"));
        assert!(!is_layer_shell_desktop(""));
    }
}
