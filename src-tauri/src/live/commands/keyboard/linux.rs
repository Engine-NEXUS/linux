//! Linux specifics for synthetic keyboard input.
//!
//! `enigo` is built with the `x11rb` backend only (`Cargo.toml` declares
//! `enigo = "0.5"` with no features), so every key NEXUS synthesises goes
//! through XTEST on the X server. What that reaches depends on the display
//! session:
//!
//! * **X11** — XTEST reaches the focused window. Typing works.
//! * **native Wayland** — the X server only knows XWayland clients, so a
//!   native Wayland window never receives the event. Typing into one used to
//!   be a silent no-op that was reported as success.
//!
//! Everything here is detection and honest refusal; nothing synthesises input
//! itself.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use super::InputSession;

/// `xdotool` is optional. Once we know it is not installed we stop looking for
/// it, so a Wayland session without it does not pay a failed exec on every
/// dictated line.
static XDOTOOL_MISSING: AtomicBool = AtomicBool::new(false);

/// `wl-copy` is optional in the same way.
static WLCOPY_MISSING: AtomicBool = AtomicBool::new(false);

/// The facts for the input decision, read from the session and the X server.
pub fn current_session() -> InputSession {
    match crate::session::kind() {
        // X11 (or an XWayland session): XTEST reaches the focused window.
        crate::session::SessionKind::X11 => InputSession {
            display: true,
            wayland: false,
            xwayland_focus: None,
        },
        // Native Wayland: only XWayland windows can be reached.
        crate::session::SessionKind::Wayland => InputSession {
            display: true,
            wayland: true,
            xwayland_focus: xwayland_focus(),
        },
        // Neither WAYLAND_DISPLAY nor DISPLAY: nothing to type into.
        crate::session::SessionKind::Headless => InputSession {
            display: false,
            wayland: false,
            xwayland_focus: None,
        },
    }
}

/// Is the focused window an XWayland (X11) window?
///
/// * `Some(true)` — yes, XTEST reaches it.
/// * `Some(false)` — no: the X server has the focus on no client at all, which
///   under XWayland means a native Wayland window has the focus.
/// * `None` — we could not tell (no `$DISPLAY`, or `xdotool` is not
///   installed).
///
/// XWayland keeps the X input focus on the X client that holds the Wayland
/// focus and drops it (PointerRoot) when a native client is focused, so "the X
/// server has a focused client window" answers the same question as "is the
/// focused window an X11 one".
///
/// `xdotool` is used because it is the same tool the rest of the Linux input
/// code uses (`live/commands/window.rs`); it is optional, and its absence
/// means "unknown", never "yes".
fn xwayland_focus() -> Option<bool> {
    if std::env::var("DISPLAY").ok().filter(|d| !d.trim().is_empty()).is_none() {
        // No X server at all: nothing can be XWayland.
        return None;
    }
    if XDOTOOL_MISSING.load(Ordering::Relaxed) {
        return None;
    }
    let out = match Command::new("xdotool").args(["getactivewindow"]).output() {
        Ok(o) => o,
        Err(_) => {
            XDOTOOL_MISSING.store(true, Ordering::Relaxed);
            return None;
        }
    };
    if !out.status.success() {
        return None;
    }
    let window = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if window.is_empty() {
        return None;
    }
    let out = match Command::new("xdotool").args(["getwindowpid", window.as_str()]).output() {
        Ok(o) => o,
        Err(_) => {
            XDOTOOL_MISSING.store(true, Ordering::Relaxed);
            return None;
        }
    };
    let pid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if pid.is_empty() {
        // The X focus sits on the root window: no X client is focused, so the
        // Wayland focus is on a native window.
        return Some(false);
    }
    if pid.parse::<u32>().is_ok() {
        return Some(true);
    }
    None
}

/// Put text on the clipboard so the user can paste it.
///
/// `arboard` first (it is already a dependency), then `wl-copy` when it is
/// installed. Returns whether the text is on the clipboard.
///
/// On a Wayland session `arboard` writes the XWayland clipboard, which native
/// Wayland apps never read, so `wl-copy` — which owns the Wayland clipboard —
/// is used there as well when it is installed. Without that, "paste it with
/// Ctrl+V" would be a promise the paste cannot keep.
pub fn copy_to_clipboard(text: &str) -> bool {
    let mut copied = arboard_copy(text);
    if !copied || crate::session::is_wayland() {
        copied = wl_copy(text) || copied;
    }
    copied
}

/// Copy through `arboard` (the X11 clipboard; on a Wayland session this is the
/// XWayland clipboard).
fn arboard_copy(text: &str) -> bool {
    match arboard::Clipboard::new() {
        Ok(mut clipboard) => match clipboard.set_text(text.to_string()) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("live: arboard could not set the clipboard: {e}");
                false
            }
        },
        Err(e) => {
            // A session with no X server at all has no arboard clipboard; this
            // repeats on every dictated line, so it stays out of the log.
            tracing::debug!("live: arboard clipboard unavailable on Linux: {e}");
            false
        }
    }
}

/// Copy through `wl-copy`, which reads the text from stdin and then keeps
/// running in the background to own the clipboard. The parent exiting is the
/// signal that the text arrived.
fn wl_copy(text: &str) -> bool {
    if WLCOPY_MISSING.load(Ordering::Relaxed) {
        return false;
    }
    let mut child = match Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => {
            WLCOPY_MISSING.store(true, Ordering::Relaxed);
            return false;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(text.as_bytes()).is_err() {
            tracing::warn!("live: wl-copy did not take the text");
            return false;
        }
        // Closing stdin is what tells wl-copy the text is complete.
        drop(stdin);
    }
    match child.wait() {
        Ok(status) => status.success(),
        Err(e) => {
            tracing::warn!("live: wl-copy did not finish: {e}");
            false
        }
    }
}
