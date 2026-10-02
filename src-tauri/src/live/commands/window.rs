//! Window focus management — bring an app to the foreground.
//!
//! On Windows, `SetForegroundWindow` silently fails when called from a
//! background process (Windows' foreground lock). The workaround is the
//! `AttachThreadInput` trick (from ghost-hands window.rs):
//!
//!   1. Get the foreground window's thread ID
//!   2. Attach our input queue to that thread
//!   3. Call SetForegroundWindow (now succeeds because we share input state)
//!   4. Detach the input queue
//!
//! This is the same pattern used by ghost-hands, nuphus-mcp, and many
//! other Windows automation tools.

#[cfg(target_os = "windows")]
pub use windows_impl::*;

#[cfg(not(target_os = "windows"))]
pub use unix_impl::*;

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::sync::Mutex;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindowTextW,
        GetWindowThreadProcessId, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    // Thread-local storage for the search target and result.
    // EnumWindows requires a C callback, so we can't capture closures.
    static SEARCH_TARGET: Mutex<Option<String>> = Mutex::new(None);
    static FOUND_HWND: Mutex<Option<HWND>> = Mutex::new(None);

    unsafe extern "system" fn enum_proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        let mut title = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title);
        if len <= 0 {
            return BOOL(1); // continue enumeration
        }
        let title_str = String::from_utf16_lossy(&title[..len as usize]);
        let lower = title_str.to_lowercase();

        let target = SEARCH_TARGET.lock().unwrap();
        if let Some(search) = target.as_ref() {
            if lower.contains(&search.to_lowercase()) {
                drop(target); // release lock before acquiring FOUND_HWND
                *FOUND_HWND.lock().unwrap() = Some(hwnd);
                return BOOL(0); // stop enumeration
            }
        }
        BOOL(1) // continue
    }

    /// Find a window whose title contains `partial_title` (case-insensitive).
    pub fn find_window(partial_title: &str) -> Option<HWND> {
        *SEARCH_TARGET.lock().unwrap() = Some(partial_title.to_string());
        *FOUND_HWND.lock().unwrap() = None;

        unsafe {
            let _ = EnumWindows(Some(enum_proc), LPARAM(0));
        }

        FOUND_HWND.lock().unwrap().take()
    }

    /// Bring a window to the foreground using the AttachThreadInput trick.
    /// Returns true if the window is now in the foreground.
    pub fn focus_window(hwnd: HWND) -> bool {
        unsafe {
            // Restore if minimized
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }

            // AttachThreadInput trick: attach our input queue to the
            // foreground thread's so SetForegroundWindow succeeds.
            let fg = GetForegroundWindow();
            let mut fg_pid = 0u32;
            let fg_thread = GetWindowThreadProcessId(fg, &mut fg_pid as *mut u32);
            let this_thread = GetCurrentThreadId();

            let attached = fg_thread != 0
                && fg_thread != this_thread
                && AttachThreadInput(this_thread, fg_thread, true).as_bool();

            let _ = SetForegroundWindow(hwnd);
            let _ = BringWindowToTop(hwnd);

            if attached {
                let _ = AttachThreadInput(this_thread, fg_thread, false);
            }

            // Verify success
            GetForegroundWindow() == hwnd
        }
    }

    /// Find a window by partial title and bring it to the foreground.
    /// Returns true if the window was found and focused.
    pub fn focus_app_by_title(partial_title: &str) -> bool {
        if let Some(hwnd) = find_window(partial_title) {
            focus_window(hwnd)
        } else {
            false
        }
    }
}

#[cfg(target_os = "linux")]
mod unix_impl {
    /// Focus a window by partial title match.
    ///
    /// Implemented for real on X11/XWayland via EWMH (`wmctrl -a`), which any
    /// client may invoke. The previous implementation was:
    ///
    /// ```ignore
    /// pub fn focus_app_by_title(_: &str) -> bool { tracing::warn!("…"); true }
    /// ```
    ///
    /// i.e. it warned and then returned `true`, so callers and TTS reported
    /// success for an action that had not happened.
    ///
    /// Native Wayland returns `false`: EWMH does not exist there, `Shell.Eval`
    /// has been disabled since GNOME 41, and there is no get-active-window
    /// portal. The compositor tier (GNOME shell extension / KWin scripting) is
    /// the real answer, per `docs/features/research/21-…` §3.
    pub fn focus_app_by_title(partial_title: &str) -> bool {
        if partial_title.trim().is_empty() {
            return false;
        }
        if !crate::session::has_x11_input() {
            tracing::warn!(
                "live: cannot focus {:?} — {}",
                partial_title,
                crate::session::diagnostic_line()
            );
            return false;
        }
        for cmd in [
            vec!["-a", partial_title],
            vec!["-r", partial_title],
            vec!["-a", partial_title], // retry after refresh
        ] {
            match std::process::Command::new("wmctrl").args(&cmd).output() {
                Ok(out) if out.status.success() => {
                    tracing::debug!("live: wmctrl {} ok", cmd.join(" "));
                    return true;
                }
                Ok(out) => tracing::debug!(
                    "live: wmctrl {} failed ({})",
                    cmd.join(" "),
                    String::from_utf8_lossy(&out.stderr).trim()
                ),
                Err(e) => tracing::debug!("live: wmctrl spawn failed: {e}"),
            }
        }
        tracing::warn!("live: no window matched {:?}", partial_title);
        false
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
mod unix_impl {
    /// No window-management mechanism on this platform.
    ///
    /// Returns `false` rather than `true`. A caller that is told the truth can
    /// fall back or explain; a caller told `true` will claim success for a no-op,
    /// which is how "Ok sir" came to be printed after nothing happened.
    pub fn focus_app_by_title(_partial_title: &str) -> bool {
        tracing::warn!("live: window focus not implemented on this platform");
        false
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_loads() {
        // Just verify the module compiles
    }
}
