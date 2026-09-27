//! Cross-platform active browser URL extraction.
//!
//! Used by the Architecture Mapper to auto-detect the GitHub repo URL
//! from the user's active browser tab.
//!
//! Platform approaches:
//! - Windows: UI Automation API (reads address bar text directly)
//! - macOS: AppleScript (queries active tab URL)
//! - Linux: xdotool + xclip (keyboard shortcut to copy URL)

/// Extract the URL from the currently focused browser tab.
///
/// Returns `Some(url)` if a browser is foreground and the URL was extracted,
/// or `None` if no browser is foreground or extraction failed.
pub fn get_active_browser_url() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        get_browser_url_windows()
    }
    #[cfg(target_os = "macos")]
    {
        get_browser_url_macos()
    }
    #[cfg(target_os = "linux")]
    {
        get_browser_url_linux()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

// ─── Windows: UI Automation ──────────────────────────────────────────

#[cfg(target_os = "windows")]
fn get_browser_url_windows() -> Option<String> {
    use uiautomation::UIAutomation;
    use uiautomation::controls::ControlType;

    // 1. Get the foreground window and its process ID
    let pid = get_foreground_process_id()?;
    let proc_name = get_process_name(pid)?;
    tracing::debug!("[browser_url] foreground process: {} (pid={})", proc_name, pid);

    // 2. Check if it's a known browser
    let is_chrome = proc_name == "chrome.exe" || proc_name == "brave.exe";
    let is_edge = proc_name == "msedge.exe";
    let is_firefox = proc_name == "firefox.exe";

    if !is_chrome && !is_edge && !is_firefox {
        return None;
    }

    // 3. Use UI Automation to find the address bar.
    //    Use a short timeout (500ms) — if the UI tree structure doesn't match,
    //    we fall back to window title parsing quickly.
    let automation = UIAutomation::new().ok()?;
    let root = automation.get_root_element().ok()?;

    // Find the browser element by process ID.
    // uiautomation v0.25 doesn't have .process_id() on UIMatcher, so we
    // use get_focused_element() which returns the focused element in the
    // foreground window (the browser). We then walk up to find the root
    // browser element, or just search for Edit controls from there.
    let browser = automation.get_focused_element().ok()
        .or_else(|| {
            // Fallback: find the first window element from root
            automation
                .create_matcher()
                .from(root.clone())
                .timeout(500)
                .control_type(ControlType::Window)
                .find_first()
                .ok()
        })?;

    // 4. Navigate the UI tree to find the address bar Edit control
    //    Chrome/Brave: ToolbarView → LocationBarView → Edit
    //    Edge: EdgeToolbarView → LocationBarView → Edit
    //    Firefox: Edit (directly under browser)
    let url = if is_firefox {
        // Firefox: find Edit control directly
        let edit = automation
            .create_matcher()
            .from(browser)
            .timeout(500)
            .control_type(ControlType::Edit)
            .find_first()
            .ok()?;
        read_url_from_edit(&edit)
    } else {
        // Chrome/Edge/Brave: try direct Edit search first (fastest),
        // then fall back to toolbar → address bar → edit path.
        let edit_direct = automation
            .create_matcher()
            .from(browser.clone())
            .timeout(500)
            .control_type(ControlType::Edit)
            .find_first();

        if let Ok(edit) = edit_direct {
            if let Some(url) = read_url_from_edit(&edit) {
                return Some(url);
            }
        }

        // Slower path: toolbar → address bar → edit
        let toolbar_class = if is_edge { "EdgeToolbarView" } else { "ToolbarView" };

        let toolbar = automation
            .create_matcher()
            .from(browser.clone())
            .timeout(500)
            .classname(toolbar_class)
            .find_first();

        if let Ok(toolbar) = toolbar {
            let address_bar = automation
                .create_matcher()
                .from(toolbar)
                .timeout(500)
                .classname("LocationBarView")
                .find_first();

            if let Ok(address_bar) = address_bar {
                let edit = automation
                    .create_matcher()
                    .from(address_bar)
                    .timeout(500)
                    .control_type(ControlType::Edit)
                    .find_first()
                    .ok()?;
                read_url_from_edit(&edit)
            } else {
                None
            }
        } else {
            None
        }
    };

    url.filter(|u| u.starts_with("http"))
}

#[cfg(target_os = "windows")]
fn read_url_from_edit(edit: &uiautomation::UIElement) -> Option<String> {
    use uiautomation::types::UIProperty;
    let url_variant = edit.get_property_value(UIProperty::ValueValue).ok()?;
    let url = url_variant.get_string().ok()?;
    if url.is_empty() {
        None
    } else {
        Some(url)
    }
}

#[cfg(target_os = "windows")]
fn get_foreground_process_id() -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0 == 0 {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid as *mut u32);
        if pid == 0 {
            None
        } else {
            Some(pid)
        }
    }
}

#[cfg(target_os = "windows")]
fn get_process_name(pid: u32) -> Option<String> {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
    use windows::Win32::System::ProcessStatus::K32GetModuleBaseNameW;
    unsafe {
        let process_handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid).ok()?;
        let mut name_buf = vec![0u16; 256];
        let len = K32GetModuleBaseNameW(process_handle, None, &mut name_buf);
        if len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&name_buf[..len as usize]).to_lowercase())
    }
}

// ─── macOS: AppleScript ──────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn get_browser_url_macos() -> Option<String> {
    // Try Chrome-family browsers (Chrome, Edge, Brave) first
    let chrome_script = r#"
        tell application "Google Chrome"
            if (count of windows) > 0 then
                return URL of active tab of front window
            end if
        end tell
    "#;
    if let Some(url) = run_applescript(chrome_script) {
        if url.starts_with("http") {
            return Some(url);
        }
    }

    // Try Safari
    let safari_script = r#"
        tell application "Safari"
            if (count of windows) > 0 then
                return URL of current tab of front window
            end if
        end tell
    "#;
    if let Some(url) = run_applescript(safari_script) {
        if url.starts_with("http") {
            return Some(url);
        }
    }

    None
}

#[cfg(target_os = "macos")]
fn run_applescript(script: &str) -> Option<String> {
    let output = std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()?;
    if output.status.success() {
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !url.is_empty() {
            return Some(url);
        }
    }
    None
}

// ─── Linux: xdotool + xclip ──────────────────────────────────────────

#[cfg(target_os = "linux")]
fn get_browser_url_linux() -> Option<String> {
    // 1. Get the active window ID
    let wid_output = std::process::Command::new("xdotool")
        .args(["getactivewindow"])
        .output()
        .ok()?;
    if !wid_output.status.success() {
        return None;
    }
    let wid = String::from_utf8_lossy(&wid_output.stdout).trim().to_string();
    if wid.is_empty() {
        return None;
    }

    // 2. Send Ctrl+L (focus address bar) + Ctrl+C (copy) + Escape (close)
    let _ = std::process::Command::new("xdotool")
        .args([
            "key", "--window", &wid, "--delay", "20",
            "--clearmodifiers", "ctrl+l", "ctrl+c", "Escape",
        ])
        .output();

    // 3. Read the URL from clipboard
    let clip_output = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .output()
        .ok()?;
    let url = String::from_utf8_lossy(&clip_output.stdout).trim().to_string();
    if url.starts_with("http") {
        Some(url)
    } else {
        None
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_active_browser_url_does_not_crash() {
        // This test just verifies the function doesn't panic.
        // It may return None if no browser is foreground.
        let _ = get_active_browser_url();
    }
}
