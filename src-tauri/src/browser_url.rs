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

    let resolved_url = url.map(|u| {
        let trimmed = u.trim().to_string();
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            trimmed
        } else if trimmed.contains("mail.google.com") || trimmed.contains("github.com") || trimmed.contains('.') {
            format!("https://{}", trimmed)
        } else {
            trimmed
        }
    });

    if resolved_url.is_some() {
        return resolved_url;
    }

    // Fallback: inspect foreground window title for Gmail
    if let Some(title) = get_foreground_window_title() {
        let lower = title.to_lowercase();
        if lower.contains("gmail") {
            tracing::info!("[browser_url] detected Gmail in window title: {}", title);
            return Some(format!(
                "https://mail.google.com/mail/u/0/#inbox?title={}",
                url_encode(&title)
            ));
        }
    }

    None
}

fn url_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            b' ' => result.push('+'),
            _ => result.push_str(&format!("%{:02X}", byte)),
        }
    }
    result
}

#[cfg(target_os = "windows")]
fn read_url_from_edit(edit: &uiautomation::UIElement) -> Option<String> {
    use uiautomation::types::UIProperty;
    if let Ok(url_variant) = edit.get_property_value(UIProperty::ValueValue) {
        if let Ok(url) = url_variant.get_string() {
            let trimmed = url.trim();
            if !trimmed.is_empty() && (trimmed.contains('.') || trimmed.starts_with("http")) {
                return Some(trimmed.to_string());
            }
        }
    }
    if let Ok(name) = edit.get_name() {
        let trimmed = name.trim();
        if !trimmed.is_empty()
            && (trimmed.contains('.') || trimmed.starts_with("http"))
            && !trimmed.contains("Address and search bar")
        {
            return Some(trimmed.to_string());
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn get_foreground_window_title() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0 == 0 {
            return None;
        }
        let mut title_buf = vec![0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title_buf);
        if len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&title_buf[..len as usize]))
    }
}

// ─── Screen-tour context accessors (pub(crate)) ──────────────────────

/// Foreground window title (screen-tour context).
#[cfg(target_os = "windows")]
pub(crate) fn foreground_window_title() -> Option<String> {
    get_foreground_window_title()
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn foreground_window_title() -> Option<String> {
    crate::live::commands::window::get_foreground_window_title()
}

/// Foreground process executable name, lower-case (e.g. `brave.exe`).
#[cfg(target_os = "windows")]
pub(crate) fn foreground_process_name() -> Option<String> {
    get_process_name(get_foreground_process_id()?)
}

/// Full path of the foreground process executable (to open a URL in the SAME browser the user is
/// looking at instead of whatever the system default is).
#[cfg(target_os = "windows")]
pub(crate) fn foreground_process_path() -> Option<std::path::PathBuf> {
    get_process_path(get_foreground_process_id()?)
}

/// True for the browsers whose page content we can locate.
pub(crate) fn is_browser_process(name: &str) -> bool {
    matches!(
        name,
        "chrome.exe" | "brave.exe" | "msedge.exe" | "firefox.exe" | "opera.exe" | "vivaldi.exe"
    )
}

/// Foreground window rect in physical px: (x, y, w, h).
#[cfg(target_os = "windows")]
pub(crate) fn foreground_window_rect() -> Option<(i32, i32, i32, i32)> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0 == 0 {
            return None;
        }
        let mut r = RECT::default();
        if !GetWindowRect(hwnd, &mut r).as_bool() {
            return None;
        }
        Some((r.left, r.top, r.right - r.left, r.bottom - r.top))
    }
}

/// Page-content rect of the foreground browser (UI Automation `Document`
/// control = the web view, i.e. without tab strip / URL bar / bookmarks).
/// Physical px (x, y, w, h). None when the tree does not expose it (the
/// caller then falls back to the window rect minus a toolbar inset).
#[cfg(target_os = "windows")]
pub(crate) fn browser_document_rect() -> Option<(i32, i32, i32, i32)> {
    use uiautomation::controls::ControlType;
    use uiautomation::types::Handle;
    use uiautomation::UIAutomation;
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0 == 0 {
        return None;
    }
    let automation = UIAutomation::new().ok()?;
    let window = automation.element_from_handle(Handle::from(hwnd.0 as isize)).ok()?;
    let doc = automation
        .create_matcher()
        .from(window)
        .timeout(700)
        .control_type(ControlType::Document)
        .find_first()
        .ok()?;
    let r = doc.get_bounding_rectangle().ok()?;
    let (x, y) = (r.get_left(), r.get_top());
    Some((x, y, r.get_right() - x, r.get_bottom() - y))
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

#[cfg(target_os = "windows")]
fn get_process_path(pid: u32) -> Option<std::path::PathBuf> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::K32GetModuleFileNameExW;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let len = K32GetModuleFileNameExW(handle, None, &mut buf);
        let _ = CloseHandle(handle);
        if len == 0 {
            return None;
        }
        Some(std::path::PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
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

// ─── Gmail URL Thread Extraction ─────────────────────────────────────

/// Extracts a Gmail thread ID or message identifier from an active Gmail URL.
/// Matches URLs containing `mail.google.com` with a fragment ending in a thread/message ID:
/// e.g. `https://mail.google.com/mail/u/0/#inbox/FMfcgzQVzQ...` -> `Some("FMfcgzQVzQ...")`
/// e.g. `https://mail.google.com/mail/u/0/#all/18ac5d7e3` -> `Some("18ac5d7e3")`
pub fn extract_gmail_thread_id_from_url(url: &str) -> Option<String> {
    if !url.contains("mail.google.com") {
        return None;
    }

    // Split on '#' to get the hash fragment
    let hash = url.split('#').nth(1)?;

    // Common paths: inbox/<id>, all/<id>, search/<query>/<id>, label/<name>/<id>
    let parts: Vec<&str> = hash.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 2 {
        return None;
    }

    let last_segment = parts.last()?.trim();
    // Valid Gmail thread IDs are alphanumeric strings usually between 8 and 40 characters
    if last_segment.len() >= 8 && last_segment.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        Some(last_segment.to_string())
    } else {
        None
    }
}

/// Extracts the numeric account index `/u/<N>/` from an active Gmail URL.
/// e.g. `https://mail.google.com/mail/u/1/#inbox/...` -> `Some(1)`
pub fn extract_gmail_account_index_from_url(url: &str) -> Option<usize> {
    if let Some(pos) = url.find("/mail/u/") {
        let rem = &url[pos + 8..];
        if let Some(slash) = rem.find('/') {
            return rem[..slash].parse::<usize>().ok();
        }
    }
    None
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

    #[test]
    fn test_extract_gmail_thread_id_from_url() {
        // Standard inbox thread
        let u1 = "https://mail.google.com/mail/u/0/#inbox/FMfcgzQVzQZlPkrXFw";
        assert_eq!(
            extract_gmail_thread_id_from_url(u1),
            Some("FMfcgzQVzQZlPkrXFw".to_string())
        );

        // All mail thread with hex ID
        let u2 = "https://mail.google.com/mail/u/1/#all/18ac5d7e3901";
        assert_eq!(
            extract_gmail_thread_id_from_url(u2),
            Some("18ac5d7e3901".to_string())
        );

        // Search thread
        let u3 = "https://mail.google.com/mail/u/0/#search/project/FMfcgzQ1234567";
        assert_eq!(
            extract_gmail_thread_id_from_url(u3),
            Some("FMfcgzQ1234567".to_string())
        );

        // Main inbox list without opened email
        let u_list = "https://mail.google.com/mail/u/0/#inbox";
        assert_eq!(extract_gmail_thread_id_from_url(u_list), None);

        // Non-Gmail site
        let u_github = "https://github.com/facebook/react/pull/12345";
        assert_eq!(extract_gmail_thread_id_from_url(u_github), None);
    }

    #[test]
    fn test_extract_gmail_account_index_from_url() {
        assert_eq!(
            extract_gmail_account_index_from_url("https://mail.google.com/mail/u/0/#inbox/FMfcgzQ123"),
            Some(0)
        );
        assert_eq!(
            extract_gmail_account_index_from_url("https://mail.google.com/mail/u/1/#all/18ac5d7e3"),
            Some(1)
        );
        assert_eq!(
            extract_gmail_account_index_from_url("https://mail.google.com/mail/u/5/#search/test/FMfcgz"),
            Some(5)
        );
        assert_eq!(
            extract_gmail_account_index_from_url("https://mail.google.com/mail/#inbox"),
            None
        );
        assert_eq!(
            extract_gmail_account_index_from_url("https://news.ycombinator.com"),
            None
        );
    }
}
