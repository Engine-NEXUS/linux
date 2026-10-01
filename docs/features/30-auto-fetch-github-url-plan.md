# Auto-Fetch GitHub Repo URL — Architecture Mapper

## Problem Statement

When the user opens a GitHub repository in their browser (or GitHub Desktop app),
NEXUS should automatically detect the repo URL and populate the Architecture Mapper
search bar — no manual copy-paste needed.

The user specifically mentioned "fetch the link from the green Code button" — but
reading the browser address bar URL is equivalent and simpler. The "Code" button
just shows the clone URL derived from the page URL (`https://github.com/owner/repo`).

## Current State

`get_active_repo_url()` in `architect.rs` only reads the **window title** of the
foreground window. This has major limitations:

1. **Windows only** — returns `None` on macOS and Linux
2. **Title-dependent** — fails if the browser title doesn't contain "owner/repo"
3. **No address bar reading** — doesn't extract the actual URL
4. **No GitHub Desktop support** — only looks at browser titles

## Research Summary

### Windows — UI Automation API (BEST)

The `uiautomation` Rust crate provides direct access to the Windows UI Automation
framework. It can traverse the browser's accessibility tree and read the address
bar text element directly.

- **Crate:** `uiautomation` (v0.25+, Windows-only)
- **How:** Find the foreground window → identify browser process (chrome.exe,
  msedge.exe, firefox.exe) → traverse UI tree → find Edit control (address bar)
  → read Value property
- **Speed:** Sub-millisecond
- **Permissions:** No admin/UAC required
- **Browsers:** Chrome, Edge, Firefox, Brave (Chromium-based browsers share the
  same UI tree structure)
- **Reference:** `browser-url` crate, `extract-browser-url` crate

### macOS — AppleScript (BEST)

macOS has native AppleScript support for querying browser tabs:

```applescript
-- Chrome / Edge / Brave (Chromium)
tell application "Google Chrome" to get URL of active tab of front window

-- Safari
tell application "Safari" to get URL of current tab of front window

-- Firefox (no AppleScript support — use Accessibility API fallback)
```

- **How:** Run `osascript -e '...'` via `std::process::Command`
- **Permissions:** Requires Automation permission (TCC prompt on first use)
- **Browsers:** Chrome, Safari, Edge, Brave (all support AppleScript)
- **Firefox:** No AppleScript URL access — fall back to window title parsing
- **Reference:** `x-win` crate uses AppleScript internally

### Linux — xdotool + xclip (BEST AVAILABLE)

Linux has no native "read browser URL" API. The most reliable approach:

```bash
# 1. Find the active browser window
window_id=$(xdotool search --onlyvisible --class "chrome|chromium|firefox")
# 2. Focus the address bar and copy the URL
xdotool key --window $window_id --delay 20 --clearmodifiers ctrl+l ctrl+c Escape
# 3. Read from clipboard
url=$(xclip -selection clipboard -o)
```

- **How:** `std::process::Command` to run xdotool + xclip
- **Dependencies:** `xdotool`, `xclip` (must be installed)
- **Limitations:** X11 only (Wayland doesn't support active window detection)
- **Fallback:** Window title parsing (works when title contains "owner/repo")
- **Reference:** Multiple StackOverflow solutions confirm this approach

### GitHub Desktop App (ALL OSes)

GitHub Desktop's window title shows "owner/repo" format:
- Windows: `owner/repo - GitHub Desktop`
- macOS: `owner/repo — GitHub Desktop`
- Linux: Not officially supported

The existing `extract_github_repo_from_title()` already handles this format.

### Chrome DevTools Protocol (CDP) — NOT RECOMMENDED

CDP can read tab URLs via `http://localhost:9222/json/list`, but requires Chrome
to be launched with `--remote-debugging-port=9222`. This is not practical for end
users — we can't force them to relaunch Chrome with a special flag.

## Implementation Plan

### Architecture: 3-Layer Detection Cascade

```
get_active_repo_url()
  │
  ├─ Layer 1: Browser URL Extraction (primary)
  │   ├─ Windows: UI Automation API (uiautomation crate)
  │   ├─ macOS: AppleScript (osascript)
  │   └─ Linux: xdotool + xclip
  │
  ├─ Layer 2: Window Title Parsing (fallback)
  │   ├─ All OSes: Get foreground window title
  │   └─ Parse for "github.com/owner/repo" or "owner/repo" patterns
  │
  └─ Layer 3: GitHub Desktop Detection (fallback)
      └─ Check if foreground window is GitHub Desktop
      └─ Parse "owner/repo" from title
```

### Files to Change

#### 1. `src-tauri/Cargo.toml`
- Add `uiautomation = "0.25"` (Windows-only, behind `cfg`)

#### 2. `src-tauri/src/browser_url.rs` (NEW)
- Platform-specific browser URL extraction module
- `pub fn get_active_browser_url() -> Option<String>`
- Windows: UI Automation tree traversal
- macOS: AppleScript via `osascript`
- Linux: `xdotool` + `xclip`
- Returns the full URL string (e.g., `https://github.com/owner/repo`)

#### 3. `src-tauri/src/architect.rs`
- Rewrite `get_active_repo_url()` to use the 3-layer cascade:
  1. Call `browser_url::get_active_browser_url()`
  2. If URL contains `github.com`, parse owner/repo
  3. If no URL or not GitHub, fall back to window title parsing
  4. If title parsing fails, check for GitHub Desktop
- Add macOS and Linux support for window title retrieval:
  - macOS: `osascript -e 'tell application "System Events" to get name of first process whose frontmost is true'`
  - Linux: `xdotool getactivewindow getwindowname`

#### 4. `src-tauri/src/lib.rs`
- Register `browser_url` module

#### 5. `frontend/src/architect/ArchitectApp.tsx`
- Auto-trigger detection on mount (when architect window opens)
- If a repo is detected, auto-populate the search bar and start analysis
- Show a brief "Detected: owner/repo" toast/notification
- Keep the "Detect Window" button as manual fallback

### Detailed Implementation

#### Windows — UI Automation

```rust
#[cfg(target_os = "windows")]
fn get_browser_url_windows() -> Option<String> {
    use uiautomation::UIAutomation;
    use uiautomation::controls::ControlType;
    use uiautomation::types::UIProperty;

    let automation = UIAutomation::new().ok()?;
    let root = automation.get_focused_element().ok()?;

    // Walk up to find the window element
    let window = walk_to_window(&root)?;

    // Check if it's a browser (Chrome, Edge, Firefox, Brave)
    let class = window.get_classname().ok()?;
    if !is_browser_class(&class) {
        return None;
    }

    // Find the address bar (Edit control with "Edit" or "AddressBar" class)
    let edit = window.find_first(
        ControlType::Edit,
        false,
    )?;

    // Read the URL from the Edit control's Value property
    let url = edit.get_property_value(UIProperty::ValueValue).ok()?;
    url.to_string().into()
}
```

#### macOS — AppleScript

```rust
#[cfg(target_os = "macos")]
fn get_browser_url_macos() -> Option<String> {
    // Try Chrome-family browsers first (Chrome, Edge, Brave)
    let chrome_script = r#"
        tell application "Google Chrome"
            if (count of windows) > 0 then
                return URL of active tab of front window
            end if
        end tell
    "#;
    if let Ok(output) = std::process::Command::new("osascript")
        .args(["-e", chrome_script])
        .output()
    {
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
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
    if let Ok(output) = std::process::Command::new("osascript")
        .args(["-e", safari_script])
        .output()
    {
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if url.starts_with("http") {
            return Some(url);
        }
    }

    None
}
```

#### Linux — xdotool + xclip

```rust
#[cfg(target_os = "linux")]
fn get_browser_url_linux() -> Option<String> {
    // Find the active browser window
    let window_id = std::process::Command::new("xdotool")
        .args(["getactivewindow"])
        .output()
        .ok()?;
    let wid = String::from_utf8_lossy(&window_id.stdout).trim().to_string();

    // Focus address bar, copy URL, press Escape to close
    std::process::Command::new("xdotool")
        .args(["key", "--window", &wid, "--delay", "20",
               "--clearmodifiers", "ctrl+l", "ctrl+c", "Escape"])
        .output()
        .ok()?;

    // Read from clipboard
    let clip = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .output()
        .ok()?;
    let url = String::from_utf8_lossy(&clip.stdout).trim().to_string();
    if url.starts_with("http") {
        Some(url)
    } else {
        None
    }
}
```

### Auto-Detection Flow

```
1. User opens Architecture Mapper (via voice "open architecture mapper"
   or Ctrl+Alt+A debug hotkey)

2. ArchitectApp.tsx mounts → calls get_active_repo_url()

3. Rust tries Layer 1 (browser URL):
   - Windows: UI Automation reads address bar → "https://github.com/owner/repo"
   - macOS: AppleScript reads active tab URL
   - Linux: xdotool copies address bar to clipboard

4. If URL contains "github.com/", parse owner/repo → auto-populate search bar

5. If Layer 1 fails, try Layer 2 (window title):
   - Get foreground window title
   - Parse for "github.com/owner/repo" or "owner/repo · GitHub"
   - Works for GitHub Desktop app too

6. If a repo is detected:
   - Auto-fill the search input with "owner/repo"
   - Auto-start Phase 1 analysis
   - Show "Detected: owner/repo from browser" in UI

7. If no repo detected:
   - Search bar stays empty, user can type manually
   - "Detect Window" button still available for manual retry
```

### Browser Support Matrix

| Browser   | Windows         | macOS           | Linux           |
|-----------|-----------------|-----------------|-----------------|
| Chrome    | UI Automation ✅ | AppleScript ✅   | xdotool ✅      |
| Edge      | UI Automation ✅ | AppleScript ✅   | xdotool ✅      |
| Firefox   | UI Automation ✅ | Title fallback  | xdotool ✅      |
| Brave     | UI Automation ✅ | AppleScript ✅   | xdotool ✅      |
| Safari    | N/A             | AppleScript ✅   | N/A             |
| GitHub Desktop | Title parse ✅ | Title parse ✅ | N/A             |

### Permissions

| OS      | Permission needed           | When prompted              |
|---------|-----------------------------|----------------------------|
| Windows | None (UI Automation is open)| Never                      |
| macOS   | Automation (TCC)            | First AppleScript call     |
| Linux   | None (xdotool is user-level)| Never (if xdotool installed)|

### Dependencies to Add

| Dependency | OS       | Crate/Tool      | Purpose                     |
|------------|----------|-----------------|-----------------------------|
| uiautomation | Windows | Rust crate      | Read browser address bar    |
| xdotool    | Linux    | System package  | Simulate keyboard to copy URL|
| xclip      | Linux    | System package  | Read clipboard              |

### Testing Plan

1. **Windows + Chrome:** Open `github.com/owner/repo` → open architect → verify auto-detection
2. **Windows + Edge:** Same test with Edge
3. **Windows + Firefox:** Same test with Firefox
4. **Windows + GitHub Desktop:** Open a repo in GitHub Desktop → open architect
5. **macOS + Chrome:** Open repo → open architect → verify AppleScript works
6. **macOS + Safari:** Same test with Safari
7. **Linux + Chrome:** Open repo → open architect → verify xdotool works
8. **Non-GitHub page:** Open google.com → open architect → verify no false detection
9. **No browser open:** Open architect → verify graceful fallback to empty search bar
10. **Rapid open/close:** Open architect multiple times → verify no crashes

### What We Are NOT Doing

- **Screen OCR:** Not needed — the URL is available via accessibility APIs
- **Clicking the green Code button:** Not needed — the address bar URL is equivalent
- **Browser extension:** Not needed — native OS APIs are sufficient
- **Chrome DevTools Protocol:** Not practical (requires special Chrome launch flag)
- **Reading page DOM content:** Not needed — we only need the URL
