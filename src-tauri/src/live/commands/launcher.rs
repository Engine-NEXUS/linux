//! App launcher — open apps via Win-search (keyboard-only, no grounding).
//!
//! The keyboard-first rule: Win → type name → Enter needs zero pixels,
//! works at any DPI/monitor, and completes in ~3s. UIA/vision grounding
//! is reserved for clicks no key sequence can reach (Phase 2).
//!
//! Windows-only for now (per Ghost plan); other platforms return a clear
//! error. The `macos_impl`/`linux_impl` seams slot in here later.

/// Open an app by name through Windows search.
/// Validates input, presses Win, types the name, presses Enter, then
/// verifies the window actually focused (no silent lies).
pub fn open_app_via_search(app_name: &str) -> Result<(), String> {
    let name = app_name.trim();
    if name.is_empty() {
        return Err("open_app_via_search: empty app name".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        use super::keyboard;
        use std::thread;
        use std::time::Duration;

        tracing::info!("launcher: opening '{name}' via Win-search");
        // Win opens Start/search.
        keyboard::press_key("win")?;
        thread::sleep(Duration::from_millis(600));
        // Type the app name into search.
        keyboard::type_text(name)?;
        thread::sleep(Duration::from_millis(400));
        // Launch the top hit.
        keyboard::press_key("enter")?;
        thread::sleep(Duration::from_millis(1500));
        // Verify: the app window must actually be foreground now.
        // Focus failure is reported, never silently swallowed — the
        // drill aborts the session on this error.
        if super::window::focus_app_by_title(name) {
            tracing::info!("launcher: '{name}' focused");
            Ok(())
        } else {
            Err(format!("launched '{name}' but couldn't confirm its window"))
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = name;
        Err("launcher: Win-search open is Windows-only for now".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_rejects_blank_names() {
        assert!(open_app_via_search("").is_err());
        assert!(open_app_via_search("   ").is_err());
        #[cfg(not(target_os = "windows"))]
        assert!(open_app_via_search("notepad").is_err());
    }
}
