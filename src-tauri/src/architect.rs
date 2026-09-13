//! Architecture Mapper — Phase 1, 2 & 3 Backend Engine.
//!
//! Provides:
//!   - Active repo detection from foreground OS window (`get_active_repo_url`)
//!   - Window control for the `architect` window (`open_architect_window`)
//!   - Phase 1 fast architectural layer clustering (`analyze_repo_phase1`)
//!   - Phase 2 deep dependency graph extraction via shallow clone, parallel AST scanning, and petgraph analysis (`analyze_repo_deep`)
//!   - Phase 3 sub-10ms reverse BFS impact & blast radius engine (`query_impact`)

use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use parking_lot::Mutex;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, Runtime};

// ─── Pending architect repo ────────────────────────────────────────
//
// Same pattern as PENDING_SIDEBAR in commands.rs: when the architect window
// is created on-demand, the React app needs time to load before it can
// receive Tauri events. Instead of emitting `architect:set-repo` with a
// fragile delay, we store the repo here and let the frontend fetch it
// on mount via `get_pending_architect_repo`.
//
// Extended to also carry the screenshot backdrop data URI for the
// liquid-glass effect (same as the response sidebar).

struct PendingArchitect {
    owner: Option<String>,
    repo: Option<String>,
    backdrop: Option<String>,
    /// Pre-computed Phase 1 data — when set, the frontend skips
    /// `analyze_repo_phase1` and renders immediately. Used by the
    /// "hidden until ready" flow: analysis runs before the window opens.
    phase1_data: Option<Phase1Response>,
}

static PENDING_ARCHITECT_REPO: Mutex<Option<PendingArchitect>> = parking_lot::const_mutex(None);

// ─── Last foreground window cache ──────────────────────────────────
//
// When the user says "open architecture mapper", the STT + intent parsing
// takes 0.6-16s. During that time, the NEXUS orb or terminal may become
// the foreground window, so `get_active_repo_url()` can't detect the
// user's browser/GitHub Desktop anymore.
//
// This cache stores the last known foreground window title + process name
// that was NOT a NEXUS window. A background thread updates it every 2s.
// `get_active_repo_url()` checks the current foreground first, then falls
// back to this cache.

struct ForegroundCache {
    title: String,
    /// Process name (e.g. "chrome.exe", "GitHub.exe"). Stored for
    /// debugging and future browser-URL extraction. Currently the
    /// title-based fallback is sufficient for repo detection.
    #[allow(dead_code)]
    process_name: String,
}

static LAST_FOREGROUND: Mutex<Option<ForegroundCache>> = parking_lot::const_mutex(None);

/// Start the background thread that tracks the last non-NEXUS foreground window.
/// Called once from `lib.rs` at startup.
pub fn start_foreground_tracker() {
    std::thread::spawn(|| {
        loop {
            if let Some((title, proc_name)) = get_foreground_window_info() {
                // Skip NEXUS windows (orb, sidebar, architect, loading, settings, setup)
                let is_nexus = proc_name == "nexus.exe"
                    || title.contains("NEXUS")
                    || title.contains("architect")
                    || title.contains("Task Switching");
                if !is_nexus && !title.is_empty() {
                    let mut cache = LAST_FOREGROUND.lock();
                    *cache = Some(ForegroundCache {
                        title: title.clone(),
                        process_name: proc_name.clone(),
                    });
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
}

/// Get foreground window title + process name (cross-platform).
/// Returns None if no foreground window or detection fails.
fn get_foreground_window_info() -> Option<(String, String)> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0 == 0 {
                return None;
            }
            let mut buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut buf);
            if len == 0 {
                return None;
            }
            let title = String::from_utf16_lossy(&buf[..len as usize]);

            // Get process name
            use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
            use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
            use windows::Win32::System::ProcessStatus::K32GetModuleBaseNameW;
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid as *mut u32);
            let proc_name = if pid > 0 {
                let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid).ok();
                if let Some(h) = handle {
                    let mut name_buf = [0u16; 260];
                    let name_len = K32GetModuleBaseNameW(h, None, &mut name_buf);
                    let _ = windows::Win32::Foundation::CloseHandle(h);
                    if name_len > 0 {
                        String::from_utf16_lossy(&name_buf[..name_len as usize])
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            Some((title, proc_name))
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// Try to extract a browser URL from the cached foreground window's process.
/// Get the cached foreground window title (for repo detection fallback).
/// Used by `get_active_repo_url()` when the current foreground is NEXUS
/// but the user was previously in a browser/GitHub Desktop.
fn get_cached_foreground_title() -> Option<String> {
    let cache = LAST_FOREGROUND.lock();
    cache.as_ref().map(|c| c.title.clone())
}

// ─── Data Models ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoIdentity {
    pub owner: String,
    pub repo: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectLayer {
    pub id: String,
    pub label: String,
    pub layer_type: String, // "frontend" | "backend" | "database" | "infra" | "shared"
    pub dirs: Vec<String>,
    pub tech_stack: String,
    pub file_count: usize,
    pub sample_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub label: Option<String>,
    pub edge_type: Option<String>, // "imports" | "calls" | "configures"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase1Response {
    pub owner: String,
    pub repo: String,
    pub default_branch: String,
    pub primary_language: String,
    pub description: String,
    pub summary: String,
    pub layers: Vec<ArchitectLayer>,
    pub edges: Vec<ArchitectEdge>,
    pub entry_points: Vec<String>,
    pub total_files: usize,
    /// Sample of file paths (capped at 300) for the LLM enrichment pass.
    /// Not used for rendering — only passed to `enrich_phase1`.
    #[serde(default)]
    pub sample_file_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNodeInfo {
    pub file_path: String,
    pub layer_id: Option<String>,
    pub in_degree: usize,
    pub out_degree: usize,
    pub imports: Vec<String>,
    pub imported_by: Vec<String>,
    pub is_hotspot: bool,
    pub risk_level: String, // "normal" | "medium" | "high" | "critical"
    pub is_circular: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircularDependency {
    pub chain: Vec<String>,
    pub risk: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotspotItem {
    pub file: String,
    pub in_degree: usize,
    pub risk: String, // "high" | "critical"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase2Response {
    pub owner: String,
    pub repo: String,
    pub total_files: usize,
    pub files_analyzed: usize,
    pub nodes: HashMap<String, FileNodeInfo>,
    pub circular_deps: Vec<CircularDependency>,
    pub hotspots: Vec<HotspotItem>,
    pub isolated: Vec<String>,
    pub entry_points: Vec<String>,
    pub summary: String,
}

/// Progressive analysis status messages sent via Tauri Channel.
///
/// Channels are ordered, scoped to the invoking webview, and auto-cleaned
/// on drop — ideal for per-invocation progress streams. The frontend matches
/// on the `type` discriminator (serde tag = "type").
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum ArchitectProgress {
    Detecting { owner: String, repo: String, message: String },
    Indexing { total_files: usize, message: String },
    GraphReady { node_count: usize, edge_count: usize },
    HotspotsReady { hotspots: Vec<HotspotItem> },
    CyclesReady { circular_deps: Vec<CircularDependency> },
    #[allow(dead_code)]
    AiExplanation { summary: String },
    Complete { stage: String },
    Failed { stage: String, error: String },
}

/// Helper: send progress if a channel is present. Ignores send errors
/// (channel may have been closed by the frontend).
fn send_progress(channel: &Option<Channel<ArchitectProgress>>, msg: ArchitectProgress) {
    if let Some(ch) = channel {
        let _ = ch.send(msg);
    }
}

/// Cancellation registry for in-progress architecture analyses.
/// Keyed by `Channel::id()` — the frontend passes `channel.id` to
/// `cancel_architect_analysis` to set the cancel flag.
#[derive(Default)]
pub struct ArchitectCancels {
    flags: parking_lot::Mutex<HashMap<u32, Arc<std::sync::atomic::AtomicBool>>>,
}

impl ArchitectCancels {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new cancel flag for the given channel ID.
    /// Returns the flag to check during analysis.
    pub fn add(&self, id: u32) -> Arc<std::sync::atomic::AtomicBool> {
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.flags.lock().insert(id, flag.clone());
        flag
    }

    /// Set the cancel flag for the given channel ID. Returns true if found.
    pub fn cancel(&self, id: u32) -> bool {
        if let Some(flag) = self.flags.lock().get(&id) {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
            return true;
        }
        false
    }

    /// Remove the cancel flag after analysis completes.
    pub fn remove(&self, id: u32) {
        self.flags.lock().remove(&id);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactResult {
    pub target_file: String,
    pub affected_files: Vec<String>,
    pub dependency_paths: Vec<Vec<String>>,
    pub max_depth: usize,
    pub direct_count: usize,
    pub transitive_count: usize,
    pub test_files_affected: Vec<String>,
    pub explanation: String,
}

// ─── In-Memory Graph Storage ──────────────────────────────────────

#[allow(dead_code)]
pub struct CachedGraphState {
    pub owner: String,
    pub repo: String,
    pub graph: DiGraph<String, ()>,
    pub node_indices: HashMap<String, NodeIndex>,
    pub index_to_file: HashMap<NodeIndex, String>,
    pub phase2_response: Phase2Response,
}

static CACHED_GRAPH: once_cell::sync::Lazy<parking_lot::Mutex<Option<Arc<CachedGraphState>>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

// ─── Active Window Detection ──────────────────────────────────────

/// IPC: Extract active GitHub owner and repository using a 4-layer cascade:
/// 1. Browser URL extraction (UI Automation / AppleScript / xdotool) — current foreground
/// 2. Window title parsing (works for GitHub Desktop app too) — current foreground
/// 3. Cached foreground window title (from background tracker — handles focus changes)
/// 4. Fallback: None
///
/// Cross-platform: Windows, macOS, Linux.
#[tauri::command]
pub fn get_active_repo_url() -> Option<RepoIdentity> {
    // Layer 1: Try to read the browser address bar URL directly.
    // This is the most reliable method — gets the exact URL, not just the title.
    if let Some(url) = crate::browser_url::get_active_browser_url() {
        tracing::info!("[architect] browser URL detected: {}", url);
        if let Some(repo) = extract_github_repo_from_url(&url) {
            return Some(repo);
        }
    }

    // Layer 2: Fall back to parsing the foreground window title.
    // This works for GitHub Desktop app and browsers where URL extraction failed.
    if let Some(title) = get_foreground_window_title() {
        tracing::info!("[architect] window title: {}", title);
        if let Some(repo) = extract_github_repo_from_title(&title) {
            return Some(repo);
        }
    }

    // Layer 3: Use the cached foreground window title from the background tracker.
    // When the user says "open architecture mapper", STT + parsing takes time.
    // During that time, the NEXUS orb or terminal may steal foreground focus.
    // The background tracker remembers the last non-NEXUS foreground window.
    if let Some(cached_title) = get_cached_foreground_title() {
        tracing::info!("[architect] cached foreground title: {}", cached_title);
        if let Some(repo) = extract_github_repo_from_title(&cached_title) {
            return Some(repo);
        }
    }

    None
}

/// Extract owner/repo from a GitHub URL.
/// Handles: https://github.com/owner/repo, https://github.com/owner/repo/tree/main, etc.
pub fn extract_github_repo_from_url(url: &str) -> Option<RepoIdentity> {
    let t = url.trim();

    if let Some(pos) = t.find("github.com/") {
        let after = &t[pos + "github.com/".len()..];
        let parts: Vec<&str> = after.split('/').take(2).collect();
        if parts.len() == 2 {
            let owner = sanitize_github_name(parts[0]);
            let repo = sanitize_github_name(parts[1]);
            if !owner.is_empty() && !repo.is_empty() {
                return Some(RepoIdentity { owner, repo });
            }
        }
    }

    None
}

/// Get the foreground window title (cross-platform).
fn get_foreground_window_title() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0 == 0 {
                return None;
            }
            let mut buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut buf);
            if len == 0 {
                return None;
            }
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        }
    }
    #[cfg(target_os = "macos")]
    {
        // macOS: use AppleScript to get the frontmost app's window title
        let script = r#"
            tell application "System Events"
                set frontApp to name of first process whose frontmost is true
                try
                    set winTitle to title of front window of process frontApp
                on error
                    set winTitle to ""
                end try
                return frontApp & " | " & winTitle
            end tell
        "#;
        let output = std::process::Command::new("osascript")
            .args(["-e", script])
            .output()
            .ok()?;
        if output.status.success() {
            let title = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !title.is_empty() {
                return Some(title);
            }
        }
        None
    }
    #[cfg(target_os = "linux")]
    {
        // Linux: use xdotool to get the active window name
        let output = std::process::Command::new("xdotool")
            .args(["getactivewindow", "getwindowname"])
            .output()
            .ok()?;
        if output.status.success() {
            let title = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !title.is_empty() {
                return Some(title);
            }
        }
        None
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// Parse window title or URL string for GitHub owner/repo format.
pub fn extract_github_repo_from_title(title: &str) -> Option<RepoIdentity> {
    let t = title.trim();

    // Check for https://github.com/owner/repo
    if let Some(pos) = t.find("github.com/") {
        let after = &t[pos + "github.com/".len()..];
        let parts: Vec<&str> = after.split('/').take(2).collect();
        if parts.len() == 2 {
            let owner = sanitize_github_name(parts[0]);
            let repo = sanitize_github_name(parts[1]);
            if !owner.is_empty() && !repo.is_empty() {
                return Some(RepoIdentity { owner, repo });
            }
        }
    }

    // Scan the FULL title for owner/repo patterns FIRST.
    // This handles titles like "GitHub - Engine-NEXUS/NEXUS-Agent at merge-ak"
    // where splitting by '-' would break "Engine-NEXUS" and "NEXUS-Agent" apart.
    // We look for a token containing '/' where both sides are valid GitHub slugs
    // (which may contain hyphens, underscores, dots).
    if let Some((owner, repo)) = scan_for_owner_repo(t) {
        return Some(RepoIdentity { owner, repo });
    }

    // Fallback: split by separators (for titles like "owner/repo · description")
    let parts: Vec<&str> = t.split(&['·', '—', '|', ':'][..]).collect();
    for part in parts {
        let trimmed = part.trim();
        if let Some((owner, repo)) = scan_for_owner_repo(trimmed) {
            return Some(RepoIdentity { owner, repo });
        }
    }

    None
}

/// Scan a string for a `owner/repo` token where both parts can contain
/// hyphens, underscores, dots, and alphanumerics (valid GitHub slug chars).
/// This avoids splitting "Engine-NEXUS/NEXUS-Agent" by '-' first.
fn scan_for_owner_repo(s: &str) -> Option<(String, String)> {
    // Find the first '/' that is surrounded by valid slug characters
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();

    for i in 0..n {
        if chars[i] == '/' {
            // Expand left to find the start of the owner slug
            let mut start = i;
            while start > 0 && is_slug_char(chars[start - 1]) {
                start -= 1;
            }
            // Expand right to find the end of the repo slug
            let mut end = i + 1;
            while end < n && is_slug_char(chars[end]) {
                end += 1;
            }

            if start < i && end > i + 1 {
                let owner: String = chars[start..i].iter().collect();
                let repo: String = chars[i + 1..end].iter().collect();
                if is_valid_github_slug(&owner) && is_valid_github_slug(&repo) {
                    return Some((owner, repo));
                }
            }
        }
    }
    None
}

fn is_slug_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_' || c == '.'
}

fn sanitize_github_name(name: &str) -> String {
    name.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.')
        .to_string()
}

fn is_valid_github_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        && s != "github"
        && s != "login"
        && s != "settings"
        && s != "pulls"
        && s != "issues"
}

// ─── Window Management ────────────────────────────────────────────

/// IPC: Open the Architect sidebar window (900px, transparent, always-on-top).
/// EXACT carbon copy of the response sidebar's show logic:
///   1. Pre-position at bottom-right BEFORE capture (so monitor detection works)
///   2. Capture desktop backdrop via GDI BitBlt + fast_blur (before show)
///   3. Store pending data (repo + backdrop) for frontend to fetch on mount
///   4. Call show_architect_sidebar_inner which:
///      a. Re-positions at bottom-right
///      b. Captures backdrop again if not already done (fallback)
///      c. Calls win.show()
///      d. Starts live blur loop (1 FPS + change detection)
///   5. Emit fast-path events for existing windows
#[tauri::command]
pub async fn open_architect_window<R: Runtime>(
    app: AppHandle<R>,
    owner: Option<String>,
    repo: Option<String>,
) -> Result<(), String> {
    let window_existed = app.get_webview_window("architect-sidebar").is_some();

    // Use the sidebar-style architect window (900px, transparent, undecorated)
    let win = crate::dyn_windows::get_or_create_window(
        &app,
        crate::dyn_windows::WindowConfig::architect_sidebar(),
    )?;

    // ── Step 1: Store pending data IMMEDIATELY (before backdrop capture) ──
    // The window was just created above, which means WebView2 is already
    // loading architect.html. React will mount within ~100-200ms and call
    // `get_pending_architect_repo`. If we wait until after the backdrop
    // capture (~164ms) to store the pending data, React will get `null`
    // and no analysis will start — the user sees "Waiting for repository..."
    // forever.
    //
    // Fix: store owner/repo NOW (with backdrop=None). The backdrop arrives
    // later via the `sidebar:backdrop` event, which is already emitted in
    // Step 4 below. This is race-free.
    {
        let mut pending = PENDING_ARCHITECT_REPO.lock();
        *pending = Some(PendingArchitect {
            owner: owner.clone(),
            repo: repo.clone(),
            backdrop: None, // Will arrive via sidebar:backdrop event
            phase1_data: None,
        });
    }

    // ── Step 2: Pre-position at bottom-right BEFORE capture ──
    // A freshly-created window sits at physical (0, 0). On Windows, a window
    // at (0, 0) may be partially off-screen or on the wrong monitor, which
    // causes `win.current_monitor()` inside `capture_backdrop` to return None,
    // silently aborting the capture. We run the same positioning math used by
    // `show_sidebar_inner` here first, so the window is in its final position
    // on the correct monitor before we call `BitBlt`. This is safe to do even
    // before `win.show()` — `set_position` works on hidden windows.
    if let Ok(Some(monitor)) = win.current_monitor().or_else(|_| {
        win.primary_monitor()
    }) {
        let scale = monitor.scale_factor();
        let screen = monitor.size();
        let sidebar_w = 900i32;
        let sidebar_h = 1000i32;
        let phys_w = (sidebar_w as f64 * scale) as i32;
        let phys_h = (sidebar_h as f64 * scale) as i32;
        #[cfg(target_os = "windows")]
        let taskbar = (48.0 * scale) as i32;
        #[cfg(not(target_os = "windows"))]
        let taskbar = (48.0 * scale) as i32;
        let gap = (12.0 * scale) as i32;
        let x = screen.width as i32 - phys_w - gap;
        let y = (screen.height as i32 - phys_h - taskbar - gap).max(0);
        let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
    }

    // ── Step 3: Capture backdrop BEFORE show (liquid glass) ──
    // Skip if window is already visible (would capture ourselves).
    let backdrop = if window_existed && win.is_visible().unwrap_or(false) {
        None
    } else {
        capture_architect_backdrop(&app, &win)
    };

    // ── Step 3b: Update pending data with backdrop (if still pending) ──
    // If React hasn't fetched yet, include the backdrop so it gets everything
    // in one call. If React already fetched (got backdrop=None), the backdrop
    // will arrive via the sidebar:backdrop event emitted below.
    if backdrop.is_some() {
        let mut pending = PENDING_ARCHITECT_REPO.lock();
        if let Some(ref mut p) = *pending {
            p.backdrop = backdrop.clone();
        }
    }

    // ── Step 4: Show window via show_architect_sidebar_inner ──
    // This re-positions, captures backdrop if not already done, calls
    // win.show(), and starts the live blur loop — exactly like the sidebar.
    show_architect_sidebar_inner(&app, &win, backdrop.is_some())?;

    // Emit backdrop for ALL windows (fresh + existing).
    // For fresh windows, React's get_pending_architect_repo may have raced
    // and fetched backdrop=None before the capture completed. This emit
    // ensures the backdrop reaches the frontend immediately on show.
    // Emit twice: immediately + after 300ms (for fresh windows where React
    // hasn't registered its listen() handler yet).
    if let Some(ref uri) = backdrop {
        let _ = app.emit("sidebar:backdrop", uri.clone());

        let app_clone = app.clone();
        let uri_clone = uri.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let _ = app_clone.emit("sidebar:backdrop", uri_clone);
        });
    }

    // ── Step 5: Fast-path events for existing windows ──
    if window_existed {
        if let (Some(o), Some(r)) = (owner.as_ref(), repo.as_ref()) {
            let _ = app.emit("architect:set-repo",
                serde_json::json!({ "owner": o, "repo": r }),
            );
        }
    }

    // ── Step 5b: Safety-net delayed emit for fresh windows ──
    // Even though pending data is now stored before backdrop capture,
    // there's still a tiny window where React could call
    // get_pending_architect_repo and get None (if the window was created
    // but the store hasn't executed yet — e.g. the lock was contended).
    // This delayed emit ensures the frontend ALWAYS receives the repo.
    if !window_existed {
        if let (Some(o), Some(r)) = (owner.as_ref(), repo.as_ref()) {
            let app_clone = app.clone();
            let o_clone = o.clone();
            let r_clone = r.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                let _ = app_clone.emit("architect:set-repo",
                    serde_json::json!({ "owner": o_clone, "repo": r_clone }),
                );
                tracing::info!("architect-sidebar: safety-net set-repo emit ({}.{})", o_clone, r_clone);
            });
        }
    }

    // Unconditional logging (previously gated behind owner/repo, which
    // meant NOTHING was logged when no repo was detected — making this
    // bug invisible in the logs).
    tracing::info!(
        "architect-sidebar: open (owner={:?}, repo={:?}, window_existed={}, has_backdrop={})",
        owner, repo, window_existed, backdrop.is_some()
    );

    Ok(())
}

/// IPC: Auto-detect GitHub repo from active browser, run Phase 1 analysis
/// in the background, and ONLY show the architect window when the map is ready.
///
/// Flow:
/// 1. Detect repo URL from browser (UI Automation / AppleScript / xdotool)
/// 2. If repo found → run analyze_repo_phase1 (2-5s) → store result in pending
/// 3. Open architect window with pre-computed Phase 1 data → instant render
/// 4. If no repo → open architect window immediately (user types manually)
///
/// Emits `architect:loading` events so the frontend can show a loading animation
/// on the orb while analysis runs in the background.
#[tauri::command]
pub async fn open_architect_with_auto_detect<R: Runtime>(
    app: AppHandle<R>,
) -> Result<i32, String> {
    // Step 1: Auto-detect repo from active browser/window
    // get_active_repo_url uses UI Automation + GetForegroundWindow (blocking sync),
    // so run it on a blocking thread to avoid stalling the async runtime.
    let detected = tokio::task::spawn_blocking(get_active_repo_url)
        .await
        .unwrap_or(None);

    let (owner, repo) = match &detected {
        Some(r) => {
            tracing::info!("[architect] auto-detected repo: {}/{}", r.owner, r.repo);
            (Some(r.owner.clone()), Some(r.repo.clone()))
        }
        None => {
            tracing::info!("[architect] no active GitHub repo detected");
            (None, None)
        }
    };

    // Step 2: If repo detected, run Phase 1 analysis + AI enrichment BEFORE opening the window.
    // The user wants the full AI-enriched architecture in one shot (3-5 seconds),
    // not a generic diagram that updates later.
    let phase1_data = if let (Some(o), Some(r)) = (&owner, &repo) {
        let _ = app.emit(
            "architect:loading",
            serde_json::json!({
                "stage": "analyzing",
                "message": format!("Analyzing {}/{}...", o, r),
                "owner": o,
                "repo": r,
            }),
        );

        let dummy_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        match analyze_repo_phase1_inner(app.clone(), o.clone(), r.clone(), None, None, dummy_cancel).await {
            Ok(mut result) => {
                tracing::info!(
                    "[architect] Phase 1 complete ({} layers), enriching with AI before opening window",
                    result.layers.len()
                );

                // ── Inline AI enrichment ──
                // Call the Worker's Gemini/Groq cascade to rewrite generic layer
                // labels into repo-specific ones. This adds ~2-3 seconds but
                // ensures the user sees the final enriched architecture in one shot.
                let _ = app.emit(
                    "architect:loading",
                    serde_json::json!({
                        "stage": "enriching",
                        "message": format!("AI enriching {}/{} architecture...", o, r),
                    }),
                );

                match enrich_phase1_inline(&app, &result).await {
                    Ok(Some(enrichment)) => {
                        tracing::info!(
                            "[architect] AI enrichment applied (summary={} chars, {} layer updates) — opening window",
                            enrichment.summary.len(),
                            enrichment.layers.len()
                        );
                        // Merge enrichment into Phase 1 data
                        if !enrichment.summary.is_empty() {
                            result.summary = enrichment.summary;
                        }
                        for enriched_layer in &enrichment.layers {
                            if let Some(layer) = result.layers.iter_mut().find(|l| l.id == enriched_layer.id) {
                                layer.label = enriched_layer.label.clone();
                                layer.tech_stack = enriched_layer.tech_stack.clone();
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::info!("[architect] AI enrichment returned no result — using heuristic labels");
                    }
                    Err(e) => {
                        tracing::warn!("[architect] AI enrichment failed (non-fatal): {} — using heuristic labels", e);
                    }
                }

                Some(result)
            }
            Err(e) => {
                tracing::warn!("[architect] Phase 1 failed: {}, opening window anyway", e);
                let _ = app.emit(
                    "architect:loading",
                    serde_json::json!({
                        "stage": "error",
                        "message": format!("Analysis failed: {}", e),
                    }),
                );
                None
            }
        }
    } else {
        None
    };

    // If no repo was detected, do NOT open the architect window.
    // The user would see "Waiting for repository..." forever with no way
    // to auto-start analysis. Instead, return an error so the frontend
    // can speak "No repository found sir" and hide the loading indicator.
    if owner.is_none() || repo.is_none() {
        tracing::info!("[architect] no repo detected — NOT opening architect window");
        let _ = app.emit(
            "architect:loading",
            serde_json::json!({
                "stage": "error",
                "message": "No GitHub repository detected. Open a repo in your browser or GitHub Desktop.",
            }),
        );
        return Err("No GitHub repository detected. Open a repo in your browser or GitHub Desktop.".to_string());
    }

    // Step 3: Open the architect window with the pre-computed data
    let window_existed = app.get_webview_window("architect-sidebar").is_some();

    // CRITICAL: Store pending data BEFORE creating the window.
    // The React app calls get_pending_architect_repo on mount — if the
    // window is created before the pending data is stored, the frontend
    // gets null and stays stuck on "Waiting for repository...".
    // Store a placeholder first (without backdrop), then update after capture.
    {
        let mut pending = PENDING_ARCHITECT_REPO.lock();
        *pending = Some(PendingArchitect {
            owner: owner.clone(),
            repo: repo.clone(),
            backdrop: None, // will be updated after capture
            phase1_data: phase1_data.clone(),
        });
    }

    let win = crate::dyn_windows::get_or_create_window(
        &app,
        crate::dyn_windows::WindowConfig::architect_sidebar(),
    )?;

    // Pre-position
    if let Ok(Some(monitor)) = win.current_monitor().or_else(|_| win.primary_monitor()) {
        let scale = monitor.scale_factor();
        let screen = monitor.size();
        let sidebar_w = 900i32;
        let sidebar_h = 1000i32;
        let phys_w = (sidebar_w as f64 * scale) as i32;
        let phys_h = (sidebar_h as f64 * scale) as i32;
        #[cfg(target_os = "windows")]
        let taskbar = (48.0 * scale) as i32;
        #[cfg(not(target_os = "windows"))]
        let taskbar = (48.0 * scale) as i32;
        let gap = (12.0 * scale) as i32;
        let x = screen.width as i32 - phys_w - gap;
        let y = (screen.height as i32 - phys_h - taskbar - gap).max(0);
        let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
    }

    // Capture backdrop
    let backdrop = if window_existed && win.is_visible().unwrap_or(false) {
        None
    } else {
        capture_architect_backdrop(&app, &win)
    };

    // Update pending data with backdrop (now that capture is done)
    if backdrop.is_some() {
        let mut pending = PENDING_ARCHITECT_REPO.lock();
        if let Some(ref mut p) = *pending {
            p.backdrop = backdrop.clone();
        }
    }

    // Show window
    show_architect_sidebar_inner(&app, &win, backdrop.is_some())?;

    // Emit backdrop for ALL windows (fresh + existing).
    // For fresh windows, React's get_pending_architect_repo may have raced
    // and fetched backdrop=None before the capture completed. This emit
    // ensures the backdrop reaches the frontend immediately on show,
    // instead of waiting ~1.3s for the live blur loop's first frame.
    //
    // We emit twice: once immediately (catches existing windows where the
    // listener is already registered), and once after a 300ms delay (catches
    // fresh windows where React hasn't mounted + registered its listen()
    // handler yet). The 300ms delay is enough for WebView2 to load the JS,
    // React to mount, and the listen() promise to resolve.
    if let Some(ref uri) = backdrop {
        let _ = app.emit("sidebar:backdrop", uri.clone());

        let app_clone = app.clone();
        let uri_clone = uri.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let _ = app_clone.emit("sidebar:backdrop", uri_clone);
        });
    }

    // Fast-path events for existing windows
    if window_existed {
        if let (Some(o), Some(r)) = (owner.as_ref(), repo.as_ref()) {
            let _ = app.emit("architect:set-repo",
                serde_json::json!({ "owner": o, "repo": r }),
            );
        }
        if let Some(ref data) = phase1_data {
            let _ = app.emit("architect:phase1-ready", data);
        }
    }

    let _ = app.emit("architect:loading", serde_json::json!({ "stage": "done" }));

    tracing::info!(
        "architect-sidebar: auto-detect open (owner={:?}, repo={:?}, has_phase1={})",
        owner, repo, phase1_data.is_some()
    );

    // Return code:
    //   0 = no repo detected (window opened for manual input)
    //   1 = repo detected + Phase 1 succeeded (architecture is ready)
    //   2 = repo detected + Phase 1 failed (repo found but analysis error)
    let result_code = match (&owner, &repo, &phase1_data) {
        (Some(_), Some(_), Some(_)) => 1,
        (Some(_), Some(_), None) => 2,
        _ => 0,
    };

    tracing::info!(
        "architect-sidebar: auto-detect open (owner={:?}, repo={:?}, has_phase1={}, code={})",
        owner, repo, phase1_data.is_some(), result_code
    );

    Ok(result_code)
}

/// Capture the desktop region behind the architect sidebar window (Windows only).
/// Must be called BEFORE `win.show()` so we don't capture the sidebar itself.
/// Returns the blurred backdrop as a `data:image/png;base64,...` URI, or None.
/// EXACT copy of `capture_backdrop` in commands.rs.
fn capture_architect_backdrop<R: Runtime>(
    _app: &AppHandle<R>,
    win: &tauri::WebviewWindow<R>,
) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        let monitor = win.current_monitor().ok()??;
        let scale = monitor.scale_factor();
        let screen = monitor.size();
        let logical = win.inner_size().map(|s| s.to_logical::<f64>(scale)).unwrap_or(
            tauri::LogicalSize::new(900.0, 1000.0)
        );
        let sidebar_w = logical.width;
        let sidebar_h = logical.height;
        let phys_w = (sidebar_w * scale) as i32;
        let phys_h = (sidebar_h * scale) as i32;
        let taskbar = (48.0 * scale) as i32;
        let gap = (12.0 * scale) as i32;
        let x = screen.width as i32 - phys_w - gap;
        let y = (screen.height as i32 - phys_h - taskbar - gap).max(0);

        match crate::sidebar_backdrop::capture_and_blur_jpeg(x, y, phys_w, phys_h, 32.0) {
            Some(data_uri) => {
                tracing::info!("architect-sidebar: backdrop captured ({} bytes)", data_uri.len());
                Some(data_uri)
            }
            None => {
                tracing::warn!("architect-sidebar: backdrop capture failed (x={x}, y={y}, w={phys_w}, h={phys_h})");
                None
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// Shared inner logic for showing the architect sidebar — EXACT carbon copy of
/// `show_sidebar_inner` in commands.rs.
/// `backdrop_already_captured`: if true, skip the backdrop capture (it was
/// already done by the caller and stored in the pending content). This prevents
/// re-capturing the window itself when the window is already visible.
fn show_architect_sidebar_inner<R: Runtime>(
    _app: &AppHandle<R>,
    win: &tauri::WebviewWindow<R>,
    backdrop_already_captured: bool,
) -> Result<(), String> {
    // Position at bottom-right of the screen, above the taskbar.
    // Read the ACTUAL window size (logical) instead of hardcoding.
    use tauri::PhysicalPosition;
    if let Ok(Some(monitor)) = win.current_monitor() {
        let scale = monitor.scale_factor();
        let screen = monitor.size();
        let logical = win.inner_size().map(|s| s.to_logical::<f64>(scale)).unwrap_or(
            tauri::LogicalSize::new(900.0, 1000.0)
        );
        let sidebar_w = logical.width;
        let sidebar_h = logical.height;
        let phys_w = (sidebar_w * scale) as i32;
        let phys_h = (sidebar_h * scale) as i32;

        #[cfg(target_os = "macos")]
        let taskbar = (70.0 * scale) as i32;
        #[cfg(target_os = "windows")]
        let taskbar = (48.0 * scale) as i32;
        #[cfg(target_os = "linux")]
        let taskbar = (36.0 * scale) as i32;
        let gap = (12.0 * scale) as i32;

        let x = screen.width as i32 - phys_w - gap;
        let y = (screen.height as i32 - phys_h - taskbar - gap).max(0);
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }

    // ─── "Fake blur" backdrop capture (Windows only) ───────────────
    // Only capture if the caller hasn't already done so. This prevents
    // capturing the sidebar itself when the window is already visible.
    if !backdrop_already_captured {
        if let Some(data_uri) = capture_architect_backdrop(_app, win) {
            let _ = _app.emit("sidebar:backdrop", data_uri);
        }
    }

    win.show().map_err(|e| e.to_string())?;

    #[cfg(target_os = "linux")]
    {
        use tauri::PhysicalPosition;
        if let Ok(Some(monitor)) = win.current_monitor() {
            let scale = monitor.scale_factor();
            let screen = monitor.size();
            let sidebar_w = 900i32;
            let sidebar_h = 1000i32;
            let phys_w = (sidebar_w as f64 * scale) as i32;
            let phys_h = (sidebar_h as f64 * scale) as i32;
            let taskbar = (36.0 * scale) as i32;
            let gap = (12.0 * scale) as i32;
            let x = screen.width as i32 - phys_w - gap;
            let y = (screen.height as i32 - phys_h - taskbar - gap).max(0);
            let _ = win.set_position(PhysicalPosition::new(x, y));
        }
    }

    #[cfg(target_os = "windows")]
    {
        // No window-vibrancy re-apply here — see the detailed comment in
        // lib.rs's setup hook for why this window intentionally never calls
        // apply_blur/apply_acrylic/apply_mica. The window's transparency
        // comes from tao's own material-free DWM registration (done once at
        // window creation) and does not need re-applying on every show.
        //
        // Corner rounding is a plain window-shape attribute (not a material)
        // so it's safe/cheap to re-assert on every show in case it was lost.
        crate::dwm_corners::round_corners(&win);

        // ── Live blur: 1 FPS + change detection ──────────────────────
        // Carbon copy of the sidebar's live blur loop in commands.rs.
        static ARCH_LIVE_BLUR_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        static ARCH_LAST_FRAME_HASH: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);
        if !ARCH_LIVE_BLUR_ACTIVE.load(std::sync::atomic::Ordering::SeqCst) {
            ARCH_LIVE_BLUR_ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);
            // Reset hash so the first frame after show always emits
            *ARCH_LAST_FRAME_HASH.lock().unwrap() = None;

            let win_clone = win.clone();
            let app_clone = _app.clone();
            tauri::async_runtime::spawn(async move {
                // Wait for window to fully appear
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                while win_clone.is_visible().unwrap_or(false) {
                    // 1 FPS — 1000ms between captures
                    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

                    if !win_clone.is_visible().unwrap_or(false) {
                        break;
                    }

                    if let Ok(Some(monitor)) = win_clone.current_monitor() {
                        let scale = monitor.scale_factor();
                        let screen = monitor.size();
                        let sidebar_w = 900i32;
                        let sidebar_h = 1000i32;
                        let phys_w = (sidebar_w as f64 * scale) as i32;
                        let phys_h = (sidebar_h as f64 * scale) as i32;
                        let taskbar = (48.0 * scale) as i32;
                        let gap = (12.0 * scale) as i32;
                        let x = screen.width as i32 - phys_w - gap;
                        let y = (screen.height as i32 - phys_h - taskbar - gap).max(0);

                        // Step 1: cheap raw capture for hashing (~1ms)
                        let raw_bgra = match crate::sidebar_backdrop::capture_region_bgra_public(x, y, phys_w, phys_h) {
                            Some(bgra) => bgra,
                            None => continue,
                        };

                        // Step 2: hash and compare to previous frame
                        let current_hash = crate::sidebar_backdrop::frame_hash(&raw_bgra);
                        let mut prev_hash_guard = ARCH_LAST_FRAME_HASH.lock().unwrap();
                        let should_emit = match *prev_hash_guard {
                            Some(prev) => prev != current_hash,
                            None => true, // First frame after show — always emit
                        };
                        *prev_hash_guard = Some(current_hash);
                        drop(prev_hash_guard);

                        // Step 3: only run the expensive pipeline if changed
                        if should_emit {
                            // We already have the raw BGRA — blur + encode it
                            // without re-capturing (reuse the bytes we have).
                            if let Some(data_uri) = crate::sidebar_backdrop::blur_bgra_to_jpeg(&raw_bgra, phys_w, phys_h, 32.0) {
                                tracing::debug!("architect-sidebar: live blur frame changed, emitting ({} bytes)", data_uri.len());
                                let _ = app_clone.emit("sidebar:backdrop", data_uri);
                            }
                        }
                    } else {
                        tracing::debug!("architect-sidebar: live blur loop tick — current_monitor() returned None, skipping");
                    }
                }

                tracing::info!("architect-sidebar: live blur loop exiting (window no longer visible)");
                // Clean up: reset hash so next show captures fresh
                *ARCH_LAST_FRAME_HASH.lock().unwrap() = None;
                ARCH_LIVE_BLUR_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
            });
        }
    }

    #[cfg(target_os = "macos")]
    {
        // Re-apply vibrancy after show — the effect can be lost if the
        // window was hidden for a long time or the app was backgrounded.
        use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};
        let _ = apply_vibrancy(
            win,
            NSVisualEffectMaterial::Sidebar,
            Some(NSVisualEffectState::Active),
            Some(20.0),
        );
    }

    Ok(())
}

/// IPC: Fetch pending architect repo (called by the frontend on mount).
/// Returns { owner, repo, backdrop } if a repo was passed to open_architect_window,
/// or null if no repo is pending. Clears the pending data after returning.
#[tauri::command]
pub fn get_pending_architect_repo() -> Result<Option<serde_json::Value>, String> {
    let mut pending = PENDING_ARCHITECT_REPO.lock();
    let data = pending.take();
    match data {
        Some(p) => {
            tracing::info!("architect-sidebar: pending content fetched (owner={:?}, repo={:?}, has_backdrop={}, has_phase1={})", p.owner, p.repo, p.backdrop.is_some(), p.phase1_data.is_some());
            Ok(Some(serde_json::json!({
                "owner": p.owner,
                "repo": p.repo,
                "backdrop": p.backdrop,
                "phase1_data": p.phase1_data,
            })))
        }
        None => Ok(None),
    }
}

/// IPC: Cancel an in-progress architecture analysis by channel ID.
/// Sets the cancel flag associated with the given channel, which the
/// analysis loop checks between stages.
#[tauri::command]
pub fn cancel_architect_analysis(
    channel_id: u32,
    cancels: tauri::State<'_, ArchitectCancels>,
) -> Result<bool, String> {
    Ok(cancels.cancel(channel_id))
}

// ─── Phase 1: Fast Architecture Map ──────────────────────────────

/// IPC: Phase 1 fast architectural analysis. Accepts a Tauri Channel for
/// progressive status updates (ordered, scoped to this invocation).
/// The final result is returned as the command's return value.
#[tauri::command]
pub async fn analyze_repo_phase1<R: Runtime>(
    app: AppHandle<R>,
    cancels: tauri::State<'_, ArchitectCancels>,
    owner: String,
    repo: String,
    github_token: Option<String>,
    on_progress: Channel<ArchitectProgress>,
) -> Result<Phase1Response, String> {
    let cancel = cancels.add(on_progress.id());
    let channel_id = on_progress.id();
    let result = analyze_repo_phase1_inner(app, owner, repo, github_token, Some(on_progress), cancel).await;
    cancels.remove(channel_id);
    result
}

/// Inner Phase 1 logic — accepts Option<Channel> so internal callers
/// (like open_architect_with_auto_detect) can pass None for no progress.
async fn analyze_repo_phase1_inner<R: Runtime>(
    app: AppHandle<R>,
    owner: String,
    repo: String,
    github_token: Option<String>,
    on_progress: Option<Channel<ArchitectProgress>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) -> Result<Phase1Response, String> {
    tracing::info!("Phase 1: starting architectural analysis for {}/{}", owner, repo);
    send_progress(&on_progress, ArchitectProgress::Detecting {
        owner: owner.clone(),
        repo: repo.clone(),
        message: format!("Fetching repository metadata for {}/{}...", owner, repo),
    });

    let client = reqwest::Client::builder()
        .user_agent("NEXUS-Architecture-Mapper/1.0")
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    // ─── Parallelized GitHub API calls ───────────────────────────────
    send_progress(&on_progress, ArchitectProgress::Detecting {
        owner: owner.clone(),
        repo: repo.clone(),
        message: format!("Fetching metadata + file tree for {}/{} in parallel...", owner, repo),
    });

    let repo_url = format!("https://api.github.com/repos/{owner}/{repo}");
    let tree_url = format!("https://api.github.com/repos/{owner}/{repo}/git/trees/HEAD?recursive=1");

    let build_meta_req = || {
        let mut r = client.get(&repo_url);
        if let Some(tok) = &github_token {
            if !tok.trim().is_empty() {
                r = r.bearer_auth(tok);
            }
        }
        r
    };
    let build_tree_req = || {
        let mut r = client.get(&tree_url);
        if let Some(tok) = &github_token {
            if !tok.trim().is_empty() {
                r = r.bearer_auth(tok);
            }
        }
        r
    };

    let (meta_result, tree_result): (
        Result<serde_json::Value, String>,
        Result<serde_json::Value, String>,
    ) = tokio::join!(
        async {
            let resp = build_meta_req()
                .send()
                .await
                .map_err(|e| format!("GitHub repo request failed: {e}"))?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("GitHub API error {status}: {body}"));
            }
            resp.json::<serde_json::Value>()
                .await
                .map_err(|e| format!("Failed to parse repo JSON: {e}"))
        },
        async {
            let resp = build_tree_req()
                .send()
                .await
                .map_err(|e| format!("GitHub tree request failed: {e}"))?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("GitHub tree API error {status}: {body}"));
            }
            resp.json::<serde_json::Value>()
                .await
                .map_err(|e| format!("Failed to parse tree JSON: {e}"))
        },
    );

    let repo_json = meta_result?;
    let tree_json = tree_result?;

    // Check for cancellation after network calls
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        send_progress(&on_progress, ArchitectProgress::Failed { stage: "phase1".into(), error: "Cancelled by user".into() });
        return Err("Cancelled by user".into());
    }

    let default_branch = repo_json["default_branch"].as_str().unwrap_or("main").to_string();
    let primary_language = repo_json["language"].as_str().unwrap_or("TypeScript").to_string();
    let description = repo_json["description"].as_str().unwrap_or("No description provided.").to_string();

    let mut file_paths: Vec<String> = Vec::new();
    if let Some(tree_arr) = tree_json["tree"].as_array() {
        for item in tree_arr {
            if item["type"].as_str() == Some("blob") {
                if let Some(path) = item["path"].as_str() {
                    file_paths.push(path.to_string());
                }
            }
        }
    }

    let total_files = file_paths.len();
    send_progress(&on_progress, ArchitectProgress::Indexing {
        total_files,
        message: format!("Clustering {} files into architectural layers...", total_files),
    });

    // 3. Cluster into architectural layers
    let (layers, edges, entry_points) = cluster_files_into_layers(&file_paths, &primary_language);

    let summary = format!(
        "{} is a {} repository structured across {} architectural layers with {} source files.",
        repo,
        primary_language,
        layers.len(),
        total_files
    );

    let response = Phase1Response {
        owner,
        repo,
        default_branch,
        primary_language,
        description,
        summary,
        layers,
        edges,
        entry_points,
        total_files,
        sample_file_paths: file_paths.iter().take(300).cloned().collect(),
    };

    send_progress(&on_progress, ArchitectProgress::Complete { stage: "phase1".into() });
    // The result is returned directly — no event needed.
    let _ = app; // app may be used for future non-progress events
    Ok(response)
}

/// Shared Phase 1 enrichment logic — sends the Phase 1 data to the Worker LLM
/// and returns the parsed enrichment. Used by both `enrich_phase1_inline`
/// (pre-window-open path) and the `enrich_phase1` IPC command (manual re-enrich).
///
/// Returns `Ok(Some(enrichment))` on success, `Ok(None)` if the Worker returned
/// nothing, or `Err` on failure.
async fn do_enrich_phase1(
    phase1: &Phase1Response,
    file_paths: &[String],
    timeout_secs: u64,
) -> Result<Option<Phase1Enrichment>, String> {
    let session_info = crate::network::get_session_info();
    let (worker_url, user_id, device_id) = match session_info {
        Some(s) => s,
        None => {
            tracing::debug!("do_enrich_phase1: no Worker session, skipping");
            return Ok(None);
        }
    };

    tracing::info!(
        "do_enrich_phase1: asking Worker to enrich {}/{} ({} layers, {} files)",
        phase1.owner, phase1.repo, phase1.layers.len(), file_paths.len()
    );

    let payload = serde_json::json!({
        "request_id": crate::network::uuid_v4(),
        "requester": {
            "id": user_id,
            "device_id": device_id,
        },
        "task": {
            "type": "architect_enrich",
            "request": "enrich phase1",
            "intent": "phase1_enrich",
            "owner": phase1.owner,
            "repo": phase1.repo,
            "primary_language": phase1.primary_language,
            "description": phase1.description,
            "total_files": phase1.total_files,
            "layers": phase1.layers,
            "file_paths": file_paths,
        },
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    let resp = client
        .post(&worker_url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Worker request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Worker returned {}", resp.status()));
    }

    let body: serde_json::Value = resp.json().await
        .map_err(|e| format!("Failed to parse Worker response: {e}"))?;

    let reply_text = body["reply_text"].as_str().unwrap_or("");
    if reply_text.is_empty() {
        return Ok(None);
    }

    let enrichment: Phase1Enrichment = serde_json::from_str(reply_text)
        .map_err(|e| format!("Failed to parse enrichment JSON: {e}"))?;

    Ok(Some(enrichment))
}

/// Inline AI enrichment — called during `open_architect_with_auto_detect`
/// BEFORE the window opens. Returns the enrichment directly instead of
/// emitting an event (the frontend gets it via `get_pending_architect_repo`).
async fn enrich_phase1_inline<R: Runtime>(
    _app: &AppHandle<R>,
    phase1: &Phase1Response,
) -> Result<Option<Phase1Enrichment>, String> {
    do_enrich_phase1(phase1, &phase1.sample_file_paths, 30).await
}

// ─── Phase 1 LLM Enrichment (IPC command — kept for manual re-enrich) ────

/// Enrichment payload returned by the Worker LLM.
/// Only the fields the LLM rewrites are included — everything else
/// (dirs, file_count, sample_files, edges, entry_points) stays from the
/// instant Rust heuristic pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase1Enrichment {
    pub summary: String,
    /// One entry per layer, matched by `id`.
    pub layers: Vec<EnrichedLayer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichedLayer {
    pub id: String,
    pub label: String,
    pub tech_stack: String,
}

/// IPC: Call the Worker LLM to enrich the instant Phase 1 diagram with
/// repo-specific layer labels and a real summary. This runs AFTER the
/// diagram is already shown — it never blocks first paint. The result is
/// emitted as an `architect:phase1-enriched` event that the frontend
/// merges into the existing `phase1Data` in-place.
///
/// If the Worker is unreachable or the LLM fails, this silently emits
/// nothing — the heuristic diagram remains as-is (graceful degradation).
#[tauri::command]
pub async fn enrich_phase1<R: Runtime>(
    app: AppHandle<R>,
    phase1: Phase1Response,
    file_paths: Vec<String>,
) -> Result<(), String> {
    // Delegate to the shared helper (60s timeout for manual re-enrich).
    // Graceful degradation: on any failure, emit nothing and return Ok(())
    // so the heuristic diagram remains as-is.
    match do_enrich_phase1(&phase1, &file_paths, 60).await {
        Ok(Some(enrichment)) => {
            tracing::info!(
                "phase1_enrich: received enrichment — summary={} chars, {} layer updates",
                enrichment.summary.len(),
                enrichment.layers.len()
            );
            let _ = app.emit("architect:phase1-enriched", &enrichment);
        }
        Ok(None) => {
            tracing::debug!("phase1_enrich: no enrichment returned (non-fatal)");
        }
        Err(e) => {
            tracing::warn!("phase1_enrich: enrichment failed (non-fatal): {e}");
        }
    }
    Ok(())
}

fn cluster_files_into_layers(
    paths: &[String],
    primary_language: &str,
) -> (Vec<ArchitectLayer>, Vec<ArchitectEdge>, Vec<String>) {
    let mut frontend_files = Vec::new();
    let mut backend_files = Vec::new();
    let mut data_files = Vec::new();
    let mut infra_files = Vec::new();
    let mut shared_files = Vec::new();
    let mut entry_points = Vec::new();

    for path in paths {
        let p_lower = path.to_lowercase();

        if p_lower.ends_with("main.tsx")
            || p_lower.ends_with("index.tsx")
            || p_lower.ends_with("main.rs")
            || p_lower.ends_with("main.go")
            || p_lower.ends_with("server.ts")
            || p_lower.ends_with("app.tsx")
            || p_lower.ends_with("index.js")
            || p_lower.ends_with("manage.py")
        {
            if entry_points.len() < 5 {
                entry_points.push(path.clone());
            }
        }

        if p_lower.contains("client")
            || p_lower.contains("ui")
            || p_lower.contains("frontend")
            || p_lower.contains("components")
            || p_lower.contains("pages")
            || p_lower.contains("views")
            || p_lower.contains("styles")
            || p_lower.ends_with(".tsx")
            || p_lower.ends_with(".jsx")
            || p_lower.ends_with(".vue")
            || p_lower.ends_with(".svelte")
            || p_lower.ends_with(".html")
            || p_lower.ends_with(".css")
        {
            frontend_files.push(path.clone());
        } else if p_lower.contains("server")
            || p_lower.contains("api")
            || p_lower.contains("backend")
            || p_lower.contains("routes")
            || p_lower.contains("controllers")
            || p_lower.contains("handlers")
            || p_lower.contains("services")
            || p_lower.contains("grpc")
        {
            backend_files.push(path.clone());
        } else if p_lower.contains("db")
            || p_lower.contains("database")
            || p_lower.contains("models")
            || p_lower.contains("schema")
            || p_lower.contains("migrations")
            || p_lower.contains("queries")
            || p_lower.contains("store")
            || p_lower.ends_with(".sql")
            || p_lower.ends_with(".prisma")
        {
            data_files.push(path.clone());
        } else if p_lower.contains("docker")
            || p_lower.contains(".github")
            || p_lower.contains("k8s")
            || p_lower.contains("helm")
            || p_lower.contains("terraform")
            || p_lower.contains("deploy")
            || p_lower.contains("scripts")
            || p_lower.ends_with(".yml")
            || p_lower.ends_with(".yaml")
            || p_lower.ends_with(".toml")
        {
            infra_files.push(path.clone());
        } else {
            shared_files.push(path.clone());
        }
    }

    let mut layers = Vec::new();

    if !frontend_files.is_empty() {
        layers.push(ArchitectLayer {
            id: "layer_frontend".to_string(),
            label: "Client / Presentation Layer".to_string(),
            layer_type: "frontend".to_string(),
            dirs: extract_top_dirs(&frontend_files),
            tech_stack: if primary_language == "TypeScript" { "React / Web UI".into() } else { primary_language.into() },
            file_count: frontend_files.len(),
            sample_files: frontend_files.iter().take(4).cloned().collect(),
        });
    }

    if !backend_files.is_empty() {
        layers.push(ArchitectLayer {
            id: "layer_backend".to_string(),
            label: "Server / API Services".to_string(),
            layer_type: "backend".to_string(),
            dirs: extract_top_dirs(&backend_files),
            tech_stack: format!("{} Core Runtime", primary_language),
            file_count: backend_files.len(),
            sample_files: backend_files.iter().take(4).cloned().collect(),
        });
    }

    if !data_files.is_empty() {
        layers.push(ArchitectLayer {
            id: "layer_data".to_string(),
            label: "Data & State Management".to_string(),
            layer_type: "database".to_string(),
            dirs: extract_top_dirs(&data_files),
            tech_stack: "Database / Store Models".into(),
            file_count: data_files.len(),
            sample_files: data_files.iter().take(4).cloned().collect(),
        });
    }

    if !shared_files.is_empty() {
        layers.push(ArchitectLayer {
            id: "layer_shared".to_string(),
            label: "Shared Utilities & Types".to_string(),
            layer_type: "shared".to_string(),
            dirs: extract_top_dirs(&shared_files),
            tech_stack: "Utilities / Lib / Config".into(),
            file_count: shared_files.len(),
            sample_files: shared_files.iter().take(4).cloned().collect(),
        });
    }

    if !infra_files.is_empty() {
        layers.push(ArchitectLayer {
            id: "layer_infra".to_string(),
            label: "Infrastructure & CI/CD".to_string(),
            layer_type: "infra".to_string(),
            dirs: extract_top_dirs(&infra_files),
            tech_stack: "Docker / Workflows / Config".into(),
            file_count: infra_files.len(),
            sample_files: infra_files.iter().take(4).cloned().collect(),
        });
    }

    let mut edges = Vec::new();
    if layers.iter().any(|l| l.id == "layer_frontend") && layers.iter().any(|l| l.id == "layer_backend") {
        edges.push(ArchitectEdge {
            id: "e_frontend_backend".into(),
            source: "layer_frontend".into(),
            target: "layer_backend".into(),
            label: Some("API requests".into()),
            edge_type: Some("calls".into()),
        });
    }
    if layers.iter().any(|l| l.id == "layer_backend") && layers.iter().any(|l| l.id == "layer_data") {
        edges.push(ArchitectEdge {
            id: "e_backend_data".into(),
            source: "layer_backend".into(),
            target: "layer_data".into(),
            label: Some("queries".into()),
            edge_type: Some("imports".into()),
        });
    }
    if layers.iter().any(|l| l.id == "layer_backend") && layers.iter().any(|l| l.id == "layer_shared") {
        edges.push(ArchitectEdge {
            id: "e_backend_shared".into(),
            source: "layer_backend".into(),
            target: "layer_shared".into(),
            label: Some("imports".into()),
            edge_type: Some("imports".into()),
        });
    }
    if layers.iter().any(|l| l.id == "layer_frontend") && layers.iter().any(|l| l.id == "layer_shared") {
        edges.push(ArchitectEdge {
            id: "e_frontend_shared".into(),
            source: "layer_frontend".into(),
            target: "layer_shared".into(),
            label: Some("imports".into()),
            edge_type: Some("imports".into()),
        });
    }

    (layers, edges, entry_points)
}

fn extract_top_dirs(files: &[String]) -> Vec<String> {
    let mut dir_counts: HashMap<String, usize> = HashMap::new();
    for f in files {
        let parts: Vec<&str> = f.split('/').collect();
        if parts.len() > 1 {
            let dir = parts[..parts.len() - 1].join("/");
            *dir_counts.entry(dir).or_insert(0) += 1;
        }
    }
    let mut sorted: Vec<(String, usize)> = dir_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    sorted.into_iter().take(3).map(|(d, _)| format!("{}/", d)).collect()
}

// ─── Phase 2: Deep AST Dependency Graph ──────────────────────────

/// Resolve base cache directory for cloned repos: `%APPDATA%\com.nexus.assistant\repos\`
fn get_repos_cache_dir<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    if let Ok(dir) = app.path().app_data_dir() {
        dir.join("repos")
    } else if let Some(dir) = dirs_next::data_dir() {
        dir.join("com.nexus.assistant").join("repos")
    } else {
        std::env::temp_dir().join("nexus_repos")
    }
}

/// IPC: Phase 2 deep dependency scan (~60s background path).
/// Performs shallow clone (`git clone --depth=1`), scans imports with rayon in parallel,
/// constructs petgraph dependency graph, calculates cycle chains and centrality hotspots.
/// Accepts a Tauri Channel for progressive status updates.
#[tauri::command]
pub async fn analyze_repo_deep<R: Runtime>(
    app: AppHandle<R>,
    cancels: tauri::State<'_, ArchitectCancels>,
    owner: String,
    repo: String,
    github_token: Option<String>,
    on_progress: Channel<ArchitectProgress>,
) -> Result<Phase2Response, String> {
    let cancel = cancels.add(on_progress.id());
    let channel_id = on_progress.id();
    let result = analyze_repo_deep_inner(app, owner, repo, github_token, Some(on_progress), cancel).await;
    cancels.remove(channel_id);
    result
}

/// Inner Phase 2 logic — accepts Option<Channel> for internal callers.
async fn analyze_repo_deep_inner<R: Runtime>(
    app: AppHandle<R>,
    owner: String,
    repo: String,
    github_token: Option<String>,
    on_progress: Option<Channel<ArchitectProgress>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) -> Result<Phase2Response, String> {
    tracing::info!("Phase 2: Starting deep graph scan for {}/{}", owner, repo);

    // ─── Cache check: return cached result if < 24h old ────────────
    {
        let guard = CACHED_GRAPH.lock();
        if let Some(cached) = guard.as_ref() {
            if cached.owner == owner && cached.repo == repo {
                tracing::info!("Phase 2: returning cached result for {}/{}", owner, repo);
                send_progress(&on_progress, ArchitectProgress::Complete { stage: "phase2-cached".into() });
                return Ok(cached.phase2_response.clone());
            }
        }
    }

    let repos_dir = get_repos_cache_dir(&app);
    let repo_target_dir = repos_dir.join(format!("{}-{}", owner, repo));
    let _ = std::fs::create_dir_all(&repos_dir);

    // 1. Shallow Git Clone (or reuse existing directory)
    send_progress(&on_progress, ArchitectProgress::Detecting {
        owner: owner.clone(),
        repo: repo.clone(),
        message: format!("Shallow cloning {}/{} to local cache...", owner, repo),
    });

    let clone_needed = !repo_target_dir.exists() || !repo_target_dir.join(".git").exists();
    if clone_needed {
        let mut clone_url = format!("https://github.com/{}/{}.git", owner, repo);
        if let Some(tok) = &github_token {
            if !tok.trim().is_empty() {
                clone_url = format!("https://{}@github.com/{}/{}.git", tok, owner, repo);
            }
        }

        let mut cmd = std::process::Command::new("git");
        cmd.args([
            "clone",
            "--depth=1",
            "--single-branch",
            &clone_url,
            repo_target_dir.to_str().unwrap_or_default(),
        ]);

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        // Run clone with a 60s timeout — if it takes too long, fall back
        let clone_result = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            tokio::task::spawn_blocking(move || cmd.output()),
        ).await;

        match clone_result {
            Ok(Ok(Ok(out))) if out.status.success() => {
                tracing::info!("Phase 2: shallow clone succeeded at {}", repo_target_dir.display());
            }
            Ok(Ok(Ok(out))) => {
                let err_str = String::from_utf8_lossy(&out.stderr);
                tracing::warn!("Phase 2: git clone exited with code: {err_str}");
            }
            Ok(Ok(Err(e))) => {
                tracing::warn!("Phase 2: git clone command failed: {e}");
            }
            Ok(Err(e)) => {
                tracing::warn!("Phase 2: git clone task panicked: {e}");
            }
            Err(_) => {
                tracing::warn!("Phase 2: git clone timed out after 60s — falling back");
                send_progress(&on_progress, ArchitectProgress::Failed {
                    stage: "clone".into(),
                    error: "Clone timed out after 60s".into(),
                });
                return Err("Clone timed out after 60s. Use 'analyse owner/repo' for a fast no-clone analysis instead.".into());
            }
        }
    }

    // Check for cancellation after clone
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        send_progress(&on_progress, ArchitectProgress::Failed { stage: "phase2".into(), error: "Cancelled by user".into() });
        return Err("Cancelled by user".into());
    }

    // 2. Discover Source Files
    send_progress(&on_progress, ArchitectProgress::Indexing {
        total_files: 0, // unknown yet
        message: "Walking file tree and extracting import statements in parallel...".into(),
    });

    let mut candidate_files = Vec::new();
    if repo_target_dir.exists() {
        for entry in ignore::WalkBuilder::new(&repo_target_dir)
            .hidden(false)
            .git_ignore(true)
            .build()
            .flatten()
        {
            let path = entry.path();
            if path.is_file() && is_source_file(path) {
                if let Ok(rel) = path.strip_prefix(&repo_target_dir) {
                    let rel_str = rel.to_string_lossy().replace('\\', "/");
                    if !is_ignored_path(&rel_str) {
                        candidate_files.push((rel_str, path.to_path_buf()));
                    }
                }
            }
        }
    }

    let total_files = candidate_files.len();
    tracing::info!("Phase 2: found {} candidate source files to parse", total_files);

    // 3. Parallel Import Extraction with Rayon
    let known_file_set: HashSet<String> = candidate_files.iter().map(|(r, _)| r.clone()).collect();

    let parsed_results: Vec<(String, Vec<String>)> = candidate_files
        .par_iter()
        .map(|(rel_path, abs_path)| {
            let content = std::fs::read_to_string(abs_path).unwrap_or_default();
            // Use tree-sitter for AST-aware import extraction.
            // Falls back to regex if tree-sitter can't parse the file.
            let raw_imports = crate::symbol_extractor::extract_imports(rel_path, &content);
            let raw_imports = if raw_imports.is_empty() {
                extract_imports_from_source(rel_path, &content)
            } else {
                raw_imports
            };
            let resolved_imports = resolve_imported_files(rel_path, &raw_imports, &known_file_set);
            (rel_path.clone(), resolved_imports)
        })
        .collect();

    // Check for cancellation after parsing
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        send_progress(&on_progress, ArchitectProgress::Failed { stage: "phase2".into(), error: "Cancelled by user".into() });
        return Err("Cancelled by user".into());
    }

    // 4. Construct Directed Graph in Petgraph
    send_progress(&on_progress, ArchitectProgress::Indexing {
        total_files,
        message: "Building dependency graph and calculating cycles...".into(),
    });

    let mut graph = DiGraph::<String, ()>::new();
    let mut node_indices: HashMap<String, NodeIndex> = HashMap::new();
    let mut index_to_file: HashMap<NodeIndex, String> = HashMap::new();

    // Add nodes
    for (rel_path, _) in &parsed_results {
        let idx = graph.add_node(rel_path.clone());
        node_indices.insert(rel_path.clone(), idx);
        index_to_file.insert(idx, rel_path.clone());
    }

    // Add edges (A imports B -> directed edge A -> B)
    for (source_file, imports) in &parsed_results {
        if let Some(&src_idx) = node_indices.get(source_file) {
            for target_file in imports {
                if let Some(&tgt_idx) = node_indices.get(target_file) {
                    if src_idx != tgt_idx {
                        graph.add_edge(src_idx, tgt_idx, ());
                    }
                }
            }
        }
    }

    // 5. Detect Circular Dependencies (Tarjan's SCC)
    let sccs = tarjan_scc(&graph);
    let mut circular_deps = Vec::new();
    let mut circular_file_set = HashSet::new();

    for scc in sccs {
        if scc.len() > 1 {
            let mut chain: Vec<String> = scc.iter().filter_map(|idx| index_to_file.get(idx).cloned()).collect();
            for f in &chain {
                circular_file_set.insert(f.clone());
            }
            if let Some(first) = chain.first().cloned() {
                chain.push(first);
            }
            circular_deps.push(CircularDependency {
                chain,
                risk: "Circular coupling prevents isolated unit testing and tree-shaking.".to_string(),
            });
        }
    }

    // 6. Compute Centrality, In-Degree, Out-Degree & Hotspots
    let mut node_map = HashMap::new();
    let mut hotspots = Vec::new();
    let mut isolated = Vec::new();
    let mut entry_points = Vec::new();

    for (file_path, idx) in &node_indices {
        let in_deg = graph.neighbors_directed(*idx, Direction::Incoming).count();
        let out_deg = graph.neighbors_directed(*idx, Direction::Outgoing).count();

        let imported_by: Vec<String> = graph
            .neighbors_directed(*idx, Direction::Incoming)
            .filter_map(|n_idx| index_to_file.get(&n_idx).cloned())
            .collect();

        let imports: Vec<String> = graph
            .neighbors_directed(*idx, Direction::Outgoing)
            .filter_map(|n_idx| index_to_file.get(&n_idx).cloned())
            .collect();

        let risk_level = if in_deg >= 20 {
            "critical"
        } else if in_deg >= 8 {
            "high"
        } else if in_deg >= 3 {
            "medium"
        } else {
            "normal"
        };

        let is_hotspot = in_deg >= 8;
        if is_hotspot {
            hotspots.push(HotspotItem {
                file: file_path.clone(),
                in_degree: in_deg,
                risk: risk_level.to_string(),
            });
        }

        if in_deg == 0 && out_deg == 0 {
            isolated.push(file_path.clone());
        }

        if in_deg == 0 && out_deg > 0 && is_likely_entrypoint(file_path) {
            entry_points.push(file_path.clone());
        }

        node_map.insert(
            file_path.clone(),
            FileNodeInfo {
                file_path: file_path.clone(),
                layer_id: None,
                in_degree: in_deg,
                out_degree: out_deg,
                imports,
                imported_by,
                is_hotspot,
                risk_level: risk_level.to_string(),
                is_circular: circular_file_set.contains(file_path),
            },
        );
    }

    hotspots.sort_by(|a, b| b.in_degree.cmp(&a.in_degree));

    // Send progressive results before the final response
    send_progress(&on_progress, ArchitectProgress::GraphReady {
        node_count: graph.node_count(),
        edge_count: graph.edge_count(),
    });
    send_progress(&on_progress, ArchitectProgress::HotspotsReady {
        hotspots: hotspots.clone(),
    });
    send_progress(&on_progress, ArchitectProgress::CyclesReady {
        circular_deps: circular_deps.clone(),
    });

    let summary = format!(
        "Deep scan complete for {}/{}. Analyzed {} files with {} import dependencies. Found {} circular dependency chains and {} high-coupling hotspots.",
        owner,
        repo,
        total_files,
        graph.edge_count(),
        circular_deps.len(),
        hotspots.len()
    );

    let phase2_resp = Phase2Response {
        owner: owner.clone(),
        repo: repo.clone(),
        total_files,
        files_analyzed: parsed_results.len(),
        nodes: node_map,
        circular_deps,
        hotspots,
        isolated,
        entry_points,
        summary,
    };

    // Cache graph in memory for sub-10ms Phase 3 impact queries
    *CACHED_GRAPH.lock() = Some(Arc::new(CachedGraphState {
        owner,
        repo,
        graph,
        node_indices,
        index_to_file,
        phase2_response: phase2_resp.clone(),
    }));

    send_progress(&on_progress, ArchitectProgress::Complete { stage: "phase2".into() });
    Ok(phase2_resp)
}

// ─── Import Extraction Helpers ────────────────────────────────────

fn is_source_file(path: &Path) -> bool {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or_default().to_lowercase();
    matches!(
        ext.as_str(),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "rs" | "go" | "java" | "kt" | "php"
    )
}

fn is_ignored_path(rel_path: &str) -> bool {
    let p = rel_path.to_lowercase();
    p.contains("node_modules/")
        || p.contains("dist/")
        || p.contains("build/")
        || p.contains("target/")
        || p.contains(".git/")
        || p.contains("vendor/")
        || p.contains("__pycache__/")
        || p.ends_with(".d.ts")
        || p.ends_with(".min.js")
        || p.ends_with(".test.ts")
        || p.ends_with(".test.tsx")
        || p.ends_with(".test.js")
        || p.ends_with(".spec.ts")
        || p.ends_with(".spec.tsx")
        || p.ends_with(".spec.js")
}

fn is_likely_entrypoint(p: &str) -> bool {
    let l = p.to_lowercase();
    l.ends_with("main.tsx")
        || l.ends_with("index.tsx")
        || l.ends_with("main.rs")
        || l.ends_with("main.go")
        || l.ends_with("server.ts")
        || l.ends_with("app.tsx")
        || l.ends_with("index.js")
        || l.ends_with("app.py")
        || l.ends_with("manage.py")
}

/// Extract import specifiers from source content across multiple languages.
fn extract_imports_from_source(file_path: &str, content: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let ext = file_path.split('.').last().unwrap_or_default().to_lowercase();

    match ext.as_str() {
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => {
            for line in content.lines() {
                let trimmed = line.trim();
                // import ... from "path"
                if (trimmed.starts_with("import ") || trimmed.starts_with("import{") || trimmed.starts_with("export "))
                    && trimmed.contains(" from ")
                {
                    if let Some(spec) = extract_quoted_specifier(trimmed) {
                        imports.push(spec);
                    }
                }
                // require("path") or import("path")
                else if trimmed.contains("require(") || trimmed.contains("import(") {
                    if let Some(spec) = extract_quoted_specifier(trimmed) {
                        imports.push(spec);
                    }
                }
            }
        }
        "py" => {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("import ") {
                    let parts: Vec<&str> = trimmed["import ".len()..].split(',').collect();
                    for part in parts {
                        let mod_name = part.trim().split_whitespace().next().unwrap_or_default();
                        if !mod_name.is_empty() {
                            imports.push(mod_name.replace('.', "/"));
                        }
                    }
                } else if trimmed.starts_with("from ") {
                    // Use rfind to get the LAST " import" (the actual keyword),
                    // not a substring match inside a module name like "importlib".
                    if let Some(pos) = trimmed.rfind(" import") {
                        if pos > "from ".len() {
                            let mod_name = trimmed["from ".len()..pos].trim();
                            imports.push(mod_name.replace('.', "/"));
                        }
                    }
                }
            }
        }
        "rs" => {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("use crate::") {
                    let path_part = &trimmed["use crate::".len()..];
                    let clean = path_part.trim_end_matches(';').split('{').next().unwrap_or_default();
                    let formatted = clean.trim().trim_end_matches("::").replace("::", "/");
                    if !formatted.is_empty() {
                        imports.push(formatted);
                    }
                } else if trimmed.starts_with("mod ") && trimmed.ends_with(';') {
                    let mod_name = trimmed["mod ".len()..trimmed.len() - 1].trim();
                    imports.push(mod_name.to_string());
                }
            }
        }
        "go" => {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("import \"") || trimmed.starts_with('"') {
                    if let Some(spec) = extract_quoted_specifier(trimmed) {
                        imports.push(spec);
                    }
                }
            }
        }
        _ => {}
    }

    imports
}

fn extract_quoted_specifier(s: &str) -> Option<String> {
    let mut in_quote = false;
    let mut quote_char = '"';
    let mut start = 0;

    for (i, c) in s.char_indices() {
        if !in_quote {
            if c == '"' || c == '\'' || c == '`' {
                in_quote = true;
                quote_char = c;
                start = i + 1;
            }
        } else if c == quote_char {
            let spec = &s[start..i];
            if !spec.is_empty() {
                return Some(spec.to_string());
            }
            in_quote = false;
        }
    }
    None
}

/// Resolve relative import paths (e.g. `./client`, `../utils/http`, `@/components/App`)
/// to normalized file paths matching `known_files`.
fn resolve_imported_files(
    current_file: &str,
    import_specs: &[String],
    known_files: &HashSet<String>,
) -> Vec<String> {
    let mut resolved = Vec::new();
    let current_dir = Path::new(current_file).parent().unwrap_or_else(|| Path::new(""));

    for spec in import_specs {
        // Skip external package modules (e.g. "react", "tokio", "express")
        if !spec.starts_with('.') && !spec.starts_with('@') && !spec.starts_with('/') {
            // Check if it directly matches a local module path
            if known_files.contains(spec) {
                resolved.push(spec.clone());
            }
            continue;
        }

        // Relative path resolution
        let clean_spec = spec.strip_prefix("@/").unwrap_or(spec);
        let target_path = if spec.starts_with('@') {
            PathBuf::from("src").join(clean_spec)
        } else {
            current_dir.join(clean_spec)
        };

        // Normalize path
        let normalized = normalize_path(&target_path);
        let candidates = [
            normalized.clone(),
            format!("{}.ts", normalized),
            format!("{}.tsx", normalized),
            format!("{}.js", normalized),
            format!("{}.jsx", normalized),
            format!("{}/index.ts", normalized),
            format!("{}/index.tsx", normalized),
            format!("{}/index.js", normalized),
            format!("{}.rs", normalized),
            format!("{}/mod.rs", normalized),
            format!("{}.py", normalized),
            format!("{}/__init__.py", normalized),
        ];

        for cand in &candidates {
            if known_files.contains(cand) && cand != current_file {
                resolved.push(cand.clone());
                break;
            }
        }
    }

    resolved
}

fn normalize_path(path: &Path) -> String {
    let mut parts = Vec::new();
    for comp in path.components() {
        match comp {
            std::path::Component::Normal(c) => parts.push(c.to_string_lossy().to_string()),
            std::path::Component::ParentDir => {
                parts.pop();
            }
            _ => {}
        }
    }
    parts.join("/")
}

// ─── Phase 3: Reverse BFS Impact Engine ───────────────────────────

/// IPC: Sub-10ms reverse BFS impact analysis on the cached petgraph dependency graph.
/// Traces all upstream files that depend on `target_file` (directly or transitively),
/// reconstructs exact shortest paths from target to affected root, and details risk.
#[tauri::command]
pub fn query_impact(
    target_file: String,
    max_depth: Option<usize>,
) -> Result<ImpactResult, String> {
    let depth_limit = max_depth.unwrap_or(6);
    let guard = CACHED_GRAPH.lock();
    let cached = guard.as_ref().ok_or_else(|| "No graph cached. Run Phase 2 deep scan first.".to_string())?;

    // Fuzzy match target_file if exact match is not found
    let resolved_file = if cached.node_indices.contains_key(&target_file) {
        target_file.clone()
    } else {
        let needle = target_file.to_lowercase();
        cached
            .node_indices
            .keys()
            .find(|k| {
                let kl = k.to_lowercase();
                kl.ends_with(&needle) || kl.contains(&needle)
            })
            .cloned()
            .ok_or_else(|| format!("File '{}' not found in dependency graph.", target_file))?
    };

    let target_idx = *cached
        .node_indices
        .get(&resolved_file)
        .ok_or_else(|| format!("File '{}' not found in dependency graph node index.", resolved_file))?;

    // Reverse BFS traversal on incoming edges (files that import target)
    let mut visited: HashSet<NodeIndex> = HashSet::new();
    let mut queue: VecDeque<(NodeIndex, usize, Vec<String>)> = VecDeque::new();
    let mut affected_files = Vec::new();
    let mut dependency_paths = Vec::new();
    let mut max_reached_depth = 0;
    let mut direct_count = 0;
    let mut transitive_count = 0;
    let mut test_files_affected = Vec::new();

    visited.insert(target_idx);
    queue.push_back((target_idx, 0, vec![resolved_file.clone()]));

    while let Some((curr_idx, curr_depth, curr_path)) = queue.pop_front() {
        if curr_depth > 0 {
            let curr_file = cached.index_to_file.get(&curr_idx).cloned().unwrap_or_default();
            affected_files.push(curr_file.clone());
            dependency_paths.push(curr_path.clone());

            if curr_depth == 1 {
                direct_count += 1;
            } else {
                transitive_count += 1;
            }

            if curr_file.contains("test") || curr_file.contains("spec") {
                test_files_affected.push(curr_file);
            }

            max_reached_depth = max_reached_depth.max(curr_depth);
        }

        if curr_depth < depth_limit {
            // Incoming neighbors import curr_idx
            for neighbor_idx in cached.graph.neighbors_directed(curr_idx, Direction::Incoming) {
                if !visited.contains(&neighbor_idx) {
                    visited.insert(neighbor_idx);
                    if let Some(neighbor_file) = cached.index_to_file.get(&neighbor_idx) {
                        let mut next_path = curr_path.clone();
                        next_path.push(neighbor_file.clone());
                        queue.push_back((neighbor_idx, curr_depth + 1, next_path));
                    }
                }
            }
        }
    }

    let explanation = if affected_files.is_empty() {
        format!(
            "'{}' is a leaf node or isolated module with 0 dependents. Changes to this file have minimal blast radius.",
            target_file
        )
    } else {
        format!(
            "Changing '{}' affects {} files ({} direct, {} transitive across depth {}). High-risk path: {}",
            target_file,
            affected_files.len(),
            direct_count,
            transitive_count,
            max_reached_depth,
            dependency_paths.first().map(|p| p.join(" → ")).unwrap_or_default()
        )
    };

    Ok(ImpactResult {
        target_file: resolved_file,
        affected_files,
        dependency_paths,
        max_depth: max_reached_depth,
        direct_count,
        transitive_count,
        test_files_affected,
        explanation,
    })
}

// ─── Fast Analysis (No Clone) ────────────────────────────────────
//
// A lightweight, no-clone repo analysis that fetches key file contents
// via the GitHub Contents API in parallel. Designed to complete in <10s.
// Triggered by voice: "analyse this repo" or "analyse owner/repo".

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyFile {
    pub path: String,
    pub content: String,
    pub size_bytes: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechStackInfo {
    pub language: String,
    pub framework: Option<String>,
    pub build_tool: Option<String>,
    pub package_manager: Option<String>,
    pub deploy_target: Option<String>,
    pub has_tests: bool,
    pub has_ci: bool,
    pub has_docker: bool,
    pub dependencies_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FastAnalysisResponse {
    pub owner: String,
    pub repo: String,
    pub description: String,
    pub default_branch: String,
    pub total_files: usize,
    pub primary_language: String,
    pub tech_stack: TechStackInfo,
    pub key_files: Vec<KeyFile>,
    pub layers: Vec<ArchitectLayer>,
    pub entry_points: Vec<String>,
    pub concerns: Vec<String>,
    pub summary: String,
    /// Spoken summary for TTS (short, conversational)
    pub spoken_summary: String,
}

/// Files to fetch contents for (in priority order).
/// We fetch the first ones that exist, up to a max of 10.
const KEY_FILE_PATTERNS: &[&str] = &[
    "README.md",
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    "requirements.txt",
    "go.mod",
    "tsconfig.json",
    "vite.config.ts",
    "vite.config.js",
    "webpack.config.js",
    "Dockerfile",
    "docker-compose.yml",
    "docker-compose.yaml",
    ".github/workflows/ci.yml",
    ".github/workflows/build.yml",
    "src/main.tsx",
    "src/main.ts",
    "src/index.tsx",
    "src/index.ts",
    "src/main.rs",
    "src/lib.rs",
    "main.go",
    "app.py",
    "src/app.py",
    "manage.py",
];

/// IPC: Fast no-clone repo analysis.
/// Fetches repo metadata + file tree + contents of key files (README,
/// package.json/Cargo.toml, entry points, config) via GitHub API.
/// Completes in <10s. No git clone.
/// Accepts a Tauri Channel for progressive status updates.
#[tauri::command]
pub async fn analyze_repo_fast<R: Runtime>(
    app: AppHandle<R>,
    owner: String,
    repo: String,
    github_token: Option<String>,
    on_progress: Channel<ArchitectProgress>,
) -> Result<FastAnalysisResponse, String> {
    tracing::info!("Fast analysis: starting for {}/{}", owner, repo);
    let on_progress = Some(on_progress);
    send_progress(&on_progress, ArchitectProgress::Detecting {
        owner: owner.clone(),
        repo: repo.clone(),
        message: format!("Fast-scanning {}/{}...", owner, repo),
    });

    let client = reqwest::Client::builder()
        .user_agent("NEXUS-Fast-Analyzer/1.0")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    // ─── Step 1: Fetch repo metadata + file tree in parallel ───────
    let repo_url = format!("https://api.github.com/repos/{owner}/{repo}");
    let tree_url = format!("https://api.github.com/repos/{owner}/{repo}/git/trees/HEAD?recursive=1");

    let auth_header = |r: reqwest::RequestBuilder| -> reqwest::RequestBuilder {
        if let Some(tok) = &github_token {
            if !tok.trim().is_empty() {
                return r.bearer_auth(tok);
            }
        }
        r
    };

    let (meta_result, tree_result) = tokio::join!(
        async {
            let resp = auth_header(client.get(&repo_url))
                .header("Accept", "application/vnd.github+json")
                .send()
                .await
                .map_err(|e| format!("GitHub repo request failed: {e}"))?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("GitHub API error {status}: {body}"));
            }
            resp.json::<serde_json::Value>()
                .await
                .map_err(|e| format!("Failed to parse repo JSON: {e}"))
        },
        async {
            let resp = auth_header(client.get(&tree_url))
                .header("Accept", "application/vnd.github+json")
                .send()
                .await
                .map_err(|e| format!("GitHub tree request failed: {e}"))?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("GitHub tree API error {status}: {body}"));
            }
            resp.json::<serde_json::Value>()
                .await
                .map_err(|e| format!("Failed to parse tree JSON: {e}"))
        },
    );

    let repo_json = meta_result?;
    let tree_json = tree_result?;

    let default_branch = repo_json["default_branch"]
        .as_str()
        .unwrap_or("main")
        .to_string();
    let primary_language = repo_json["language"]
        .as_str()
        .unwrap_or("Unknown")
        .to_string();
    let description = repo_json["description"]
        .as_str()
        .unwrap_or("No description provided.")
        .to_string();

    // Collect all file paths from the tree
    let mut file_paths: Vec<String> = Vec::new();
    if let Some(tree_arr) = tree_json["tree"].as_array() {
        for item in tree_arr {
            if item["type"].as_str() == Some("blob") {
                if let Some(path) = item["path"].as_str() {
                    file_paths.push(path.to_string());
                }
            }
        }
    }
    let total_files = file_paths.len();

    send_progress(&on_progress, ArchitectProgress::Indexing {
        total_files,
        message: format!("Fetching key file contents from {}/{}...", owner, repo),
    });

    // ─── Step 2: Determine which key files exist in the repo ────────
    let file_set: HashSet<&str> = file_paths.iter().map(|s| s.as_str()).collect();
    let mut files_to_fetch: Vec<String> = Vec::new();
    for pattern in KEY_FILE_PATTERNS {
        if file_set.contains(*pattern) {
            files_to_fetch.push(pattern.to_string());
        }
        if files_to_fetch.len() >= 10 {
            break;
        }
    }

    // Also look for entry points by pattern (src/main.*, etc.)
    if files_to_fetch.len() < 10 {
        for path in &file_paths {
            let p_lower = path.to_lowercase();
            if (p_lower.ends_with("main.tsx")
                || p_lower.ends_with("main.ts")
                || p_lower.ends_with("main.rs")
                || p_lower.ends_with("main.go")
                || p_lower.ends_with("index.tsx")
                || p_lower.ends_with("index.ts"))
                && !files_to_fetch.contains(path)
            {
                files_to_fetch.push(path.clone());
                if files_to_fetch.len() >= 10 {
                    break;
                }
            }
        }
    }

    // ─── Step 3: Fetch key file contents in parallel ────────────────
    // GitHub Contents API returns base64-encoded content for files < 1MB.
    // We limit to files < 100KB to avoid huge payloads.
    // Uses tokio::task::JoinSet for parallel fetches (no extra dependency).
    let mut key_files: Vec<KeyFile> = Vec::new();

    let mut join_set = tokio::task::JoinSet::new();
    for path in &files_to_fetch {
        let client_clone = client.clone();
        let path_clone = path.clone();
        let token_clone = github_token.clone();
        let owner_clone = owner.clone();
        let repo_clone = repo.clone();
        join_set.spawn(async move {
            let url = format!(
                "https://api.github.com/repos/{owner_clone}/{repo_clone}/contents/{path_clone}"
            );
            let mut req = client_clone
                .get(&url)
                .header("Accept", "application/vnd.github+json");
            if let Some(tok) = &token_clone {
                if !tok.trim().is_empty() {
                    req = req.bearer_auth(tok);
                }
            }
            let resp = req.send().await;
            match resp {
                Ok(r) if r.status().is_success() => {
                    let json: serde_json::Value = r.json().await.ok()?;
                    let size = json["size"].as_u64().unwrap_or(0) as usize;
                    if size > 100_000 {
                        return Some(KeyFile {
                            path: path_clone,
                            content: format!("[File too large: {}KB]", size / 1024),
                            size_bytes: size,
                            truncated: true,
                        });
                    }
                    let encoded = json["content"].as_str().unwrap_or("");
                    let decoded = {
                        use base64::Engine;
                        match base64::engine::general_purpose::STANDARD.decode(encoded) {
                            Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
                            Err(_) => String::new(),
                        }
                    };
                    let truncated = decoded.len() > 50_000;
                    let content = if truncated {
                        decoded.chars().take(50_000).collect()
                    } else {
                        decoded
                    };
                    Some(KeyFile {
                        path: path_clone,
                        content,
                        size_bytes: size,
                        truncated,
                    })
                }
                _ => None,
            }
        });
    }

    while let Some(result) = join_set.join_next().await {
        if let Ok(Some(kf)) = result {
            key_files.push(kf);
        }
    }

    send_progress(&on_progress, ArchitectProgress::Indexing {
        total_files: key_files.len(),
        message: format!("Analyzing tech stack + architecture from {} key files...", key_files.len()),
    });

    // ─── Step 4: Detect tech stack from key file contents ───────────
    let tech_stack = detect_tech_stack(&key_files, &primary_language, &file_paths);

    // ─── Step 5: Cluster files into layers (reuse existing heuristic) ─
    let (layers, _edges, entry_points) = cluster_files_into_layers(&file_paths, &primary_language);

    // ─── Step 6: Detect concerns ────────────────────────────────────
    let concerns = detect_concerns(&file_paths, &tech_stack, &key_files);

    // ─── Step 7: Build summaries ────────────────────────────────────
    let summary = build_fast_summary(
        &repo,
        &description,
        &primary_language,
        &tech_stack,
        &layers,
        total_files,
        &concerns,
    );

    let spoken_summary = build_spoken_summary(
        &repo,
        &primary_language,
        &tech_stack,
        &layers,
        total_files,
        &concerns,
    );

    let response = FastAnalysisResponse {
        owner: owner.clone(),
        repo: repo.clone(),
        description,
        default_branch,
        total_files,
        primary_language: primary_language.clone(),
        tech_stack,
        key_files,
        layers,
        entry_points,
        concerns,
        summary,
        spoken_summary,
    };

    send_progress(&on_progress, ArchitectProgress::Complete { stage: "fast".into() });
    tracing::info!("Fast analysis: complete for {}/{}", owner, repo);
    let _ = app;
    Ok(response)
}

/// Simple base64 decoder (avoids pulling a base64 crate).
/// Detect tech stack from key file contents + file tree.
fn detect_tech_stack(
    key_files: &[KeyFile],
    primary_language: &str,
    file_paths: &[String],
) -> TechStackInfo {
    let mut framework: Option<String> = None;
    let mut build_tool: Option<String> = None;
    let mut package_manager: Option<String> = None;
    let mut deploy_target: Option<String> = None;
    let mut dependencies_count: Option<usize> = None;

    let has_tests = file_paths.iter().any(|p| {
        let pl = p.to_lowercase();
        pl.contains("test") || pl.contains("spec") || pl.contains("__tests__")
    });

    let has_ci = file_paths.iter().any(|p| p.contains(".github/workflows"));

    let has_docker = file_paths
        .iter()
        .any(|p| p.to_lowercase().contains("dockerfile") || p.to_lowercase().contains("docker-compose"));

    // Parse key files for tech stack info
    for kf in key_files {
        let path_lower = kf.path.to_lowercase();
        let content = &kf.content;

        if path_lower == "package.json" {
            // Parse dependencies
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(content) {
                let deps = json.get("dependencies").and_then(|d| d.as_object());
                let dev_deps = json.get("devDependencies").and_then(|d| d.as_object());
                let total = deps.map(|d| d.len()).unwrap_or(0)
                    + dev_deps.map(|d| d.len()).unwrap_or(0);
                dependencies_count = Some(total);

                // Detect framework
                let dep_names: Vec<String> = deps
                    .map(|d| d.keys().cloned().collect())
                    .unwrap_or_default();
                if dep_names.iter().any(|d| d == "next") {
                    framework = Some("Next.js".into());
                } else if dep_names.iter().any(|d| d == "react") {
                    framework = Some("React".into());
                } else if dep_names.iter().any(|d| d == "vue") {
                    framework = Some("Vue".into());
                } else if dep_names.iter().any(|d| d == "svelte") {
                    framework = Some("Svelte".into());
                } else if dep_names.iter().any(|d| d == "express") {
                    framework = Some("Express".into());
                } else if dep_names.iter().any(|d| d == "fastify") {
                    framework = Some("Fastify".into());
                } else if dep_names.iter().any(|d| d == "@tauri-apps/api") {
                    framework = Some("Tauri".into());
                }

                // Detect build tool
                let dev_dep_names: Vec<String> = dev_deps
                    .map(|d| d.keys().cloned().collect())
                    .unwrap_or_default();
                if dev_dep_names.iter().any(|d| d == "vite") {
                    build_tool = Some("Vite".into());
                } else if dev_dep_names.iter().any(|d| d == "webpack") {
                    build_tool = Some("Webpack".into());
                } else if dev_dep_names.iter().any(|d| d == "rollup") {
                    build_tool = Some("Rollup".into());
                }

                // Detect package manager from scripts
                if content.contains("pnpm") {
                    package_manager = Some("pnpm".into());
                } else if content.contains("yarn") {
                    package_manager = Some("yarn".into());
                } else {
                    package_manager = Some("npm".into());
                }
            }
        } else if path_lower == "cargo.toml" {
            framework = detect_rust_framework(content);
            build_tool = Some("cargo".into());
            package_manager = Some("cargo".into());

            // Count dependencies
            let dep_count = content
                .lines()
                .filter(|l| l.trim().starts_with('\"') && l.contains('='))
                .count();
            dependencies_count = Some(dep_count);
        } else if path_lower == "pyproject.toml" || path_lower == "requirements.txt" {
            if path_lower == "pyproject.toml" {
                if content.contains("fastapi") {
                    framework = Some("FastAPI".into());
                } else if content.contains("flask") {
                    framework = Some("Flask".into());
                } else if content.contains("django") {
                    framework = Some("Django".into());
                }
                build_tool = Some("poetry".into());
            } else {
                if content.contains("fastapi") {
                    framework = Some("FastAPI".into());
                } else if content.contains("flask") {
                    framework = Some("Flask".into());
                } else if content.contains("django") {
                    framework = Some("Django".into());
                }
                build_tool = Some("pip".into());
            }
            package_manager = Some("pip".into());
            dependencies_count = Some(
                content
                    .lines()
                    .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
                    .count(),
            );
        } else if path_lower == "go.mod" {
            if content.contains("gin-gonic/gin") {
                framework = Some("Gin".into());
            } else if content.contains("labstack/echo") {
                framework = Some("Echo".into());
            } else if content.contains("gorilla/mux") {
                framework = Some("Gorilla Mux".into());
            }
            build_tool = Some("go build".into());
            package_manager = Some("go modules".into());
            dependencies_count = Some(
                content
                    .lines()
                    .filter(|l| l.trim().starts_with("require") || l.contains('\t'))
                    .count(),
            );
        } else if path_lower == "dockerfile" || path_lower.contains("docker-compose") {
            deploy_target = Some("Docker".into());
        } else if path_lower.contains(".github/workflows/") {
            if content.contains("vercel") {
                deploy_target = Some("Vercel".into());
            } else if content.contains("netlify") {
                deploy_target = Some("Netlify".into());
            } else if content.contains("cloudflare") {
                deploy_target = Some("Cloudflare".into());
            } else if content.contains("aws") {
                deploy_target = Some("AWS".into());
            }
        }
    }

    // Fallback: detect framework from file extensions
    if framework.is_none() {
        let has_tauri = file_paths
            .iter()
            .any(|p| p.contains("src-tauri") || p.contains("tauri.conf"));
        if has_tauri {
            framework = Some("Tauri".into());
        }
    }

    TechStackInfo {
        language: primary_language.to_string(),
        framework,
        build_tool,
        package_manager,
        deploy_target,
        has_tests,
        has_ci,
        has_docker,
        dependencies_count,
    }
}

/// Detect Rust framework from Cargo.toml content.
fn detect_rust_framework(content: &str) -> Option<String> {
    if content.contains("actix-web") {
        Some("Actix Web".into())
    } else if content.contains("axum") {
        Some("Axum".into())
    } else if content.contains("rocket") {
        Some("Rocket".into())
    } else if content.contains("warp") {
        Some("Warp".into())
    } else if content.contains("tauri") {
        Some("Tauri".into())
    } else if content.contains("iced") {
        Some("Iced".into())
    } else if content.contains("egui") {
        Some("egui".into())
    } else if content.contains("tokio") {
        Some("Tokio (async runtime)".into())
    } else {
        None
    }
}

/// Detect potential concerns in the repository.
fn detect_concerns(
    file_paths: &[String],
    tech_stack: &TechStackInfo,
    _key_files: &[KeyFile],
) -> Vec<String> {
    let mut concerns = Vec::new();

    if !tech_stack.has_tests {
        concerns.push("No test files detected — consider adding tests.".into());
    }
    if !tech_stack.has_ci {
        concerns.push("No CI/CD workflows found in .github/workflows/.".into());
    }
    if !tech_stack.has_docker {
        concerns.push("No Dockerfile found — no containerization setup.".into());
    }

    // Check for very large repos
    if file_paths.len() > 5000 {
        concerns.push(format!(
            "Large repository ({} files) — deep analysis may be slow.",
            file_paths.len()
        ));
    }

    // Check for license
    let has_license = file_paths.iter().any(|p| {
        let pl = p.to_lowercase();
        pl == "license" || pl == "license.md" || pl == "license.txt" || pl == "copying"
    });
    if !has_license {
        concerns.push("No license file found.".into());
    }

    // Check for .gitignore
    let has_gitignore = file_paths.iter().any(|p| p == ".gitignore");
    if !has_gitignore {
        concerns.push("No .gitignore file found.".into());
    }

    concerns
}

/// Build a detailed text summary for display in the sidebar.
fn build_fast_summary(
    repo: &str,
    description: &str,
    language: &str,
    tech_stack: &TechStackInfo,
    layers: &[ArchitectLayer],
    total_files: usize,
    concerns: &[String],
) -> String {
    let mut parts = Vec::new();

    parts.push(format!("## {} — Fast Analysis\n", repo));
    parts.push(format!("**Description:** {}\n", description));
    parts.push(format!("**Language:** {}", language));

    if let Some(fw) = &tech_stack.framework {
        parts.push(format!(" | Framework: {}", fw));
    }
    if let Some(bt) = &tech_stack.build_tool {
        parts.push(format!(" | Build: {}", bt));
    }
    if let Some(pm) = &tech_stack.package_manager {
        parts.push(format!(" | Package manager: {}", pm));
    }
    if let Some(dt) = &tech_stack.deploy_target {
        parts.push(format!(" | Deploy: {}", dt));
    }
    if let Some(dc) = tech_stack.dependencies_count {
        parts.push(format!(" | Dependencies: {}", dc));
    }
    parts.push(String::new());

    parts.push(format!("**Total files:** {}\n", total_files));
    parts.push(format!("**Tests:** {} | CI: {} | Docker: {}\n",
        if tech_stack.has_tests { "yes" } else { "no" },
        if tech_stack.has_ci { "yes" } else { "no" },
        if tech_stack.has_docker { "yes" } else { "no" },
    ));

    if !layers.is_empty() {
        parts.push("\n### Architectural Layers\n".into());
        for layer in layers {
            parts.push(format!(
                "- **{}** ({}): {} files in {}",
                layer.label,
                layer.layer_type,
                layer.file_count,
                layer.dirs.join(", ")
            ));
        }
    }

    if !concerns.is_empty() {
        parts.push("\n### Concerns\n".into());
        for c in concerns {
            parts.push(format!("- {}", c));
        }
    }

    parts.join("\n")
}

/// Build a short, conversational summary for TTS.
fn build_spoken_summary(
    repo: &str,
    language: &str,
    tech_stack: &TechStackInfo,
    layers: &[ArchitectLayer],
    total_files: usize,
    concerns: &[String],
) -> String {
    let mut parts = Vec::new();

    parts.push(format!("{} is a {} repository", repo, language));

    if let Some(fw) = &tech_stack.framework {
        parts.push(format!(" built with {}", fw));
    }
    parts.push(format!(", containing {} files across {} architectural layers", total_files, layers.len()));

    if let Some(bt) = &tech_stack.build_tool {
        parts.push(format!(", using {} as the build tool", bt));
    }

    parts.push(".".into());

    // Add top concern if any
    if let Some(concern) = concerns.first() {
        parts.push(format!(" {}", concern));
    }

    parts.join("")
}

// ─── Unit Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_github_repo_from_title() {
        let t1 = "vercel/next.js: The React Framework · GitHub – Google Chrome";
        let res1 = extract_github_repo_from_title(t1).expect("Should parse vercel/next.js");
        assert_eq!(res1.owner, "vercel");
        assert_eq!(res1.repo, "next.js");

        let t2 = "https://github.com/facebook/react/tree/main";
        let res2 = extract_github_repo_from_title(t2).expect("Should parse facebook/react");
        assert_eq!(res2.owner, "facebook");
        assert_eq!(res2.repo, "react");

        let t3 = "Untitled - Notepad";
        assert!(extract_github_repo_from_title(t3).is_none());

        // Hyphenated owner/repo — must NOT split by '-' before finding '/'
        let t4 = "GitHub - Engine-NEXUS/NEXUS-Agent at merge-ak";
        let res4 = extract_github_repo_from_title(t4).expect("Should parse Engine-NEXUS/NEXUS-Agent");
        assert_eq!(res4.owner, "Engine-NEXUS");
        assert_eq!(res4.repo, "NEXUS-Agent");

        // Brave browser title format
        let t5 = "react/react: The library for web and native user interfaces. - Brave";
        let res5 = extract_github_repo_from_title(t5).expect("Should parse react/react");
        assert_eq!(res5.owner, "react");
        assert_eq!(res5.repo, "react");

        // Edge browser title format
        let t6 = "vercel/next.js: The React Framework - Microsoft Edge";
        let res6 = extract_github_repo_from_title(t6).expect("Should parse vercel/next.js");
        assert_eq!(res6.owner, "vercel");
        assert_eq!(res6.repo, "next.js");

        // GitHub Desktop app title format: "owner/repo - GitHub Desktop"
        let t7 = "Engine-NEXUS/NEXUS-Agent - GitHub Desktop";
        let res7 = extract_github_repo_from_title(t7).expect("Should parse from GitHub Desktop");
        assert_eq!(res7.owner, "Engine-NEXUS");
        assert_eq!(res7.repo, "NEXUS-Agent");

        // GitHub Desktop variant: "GitHub Desktop - owner/repo"
        let t8 = "GitHub Desktop - vercel/next.js";
        let res8 = extract_github_repo_from_title(t8).expect("Should parse from GitHub Desktop variant");
        assert_eq!(res8.owner, "vercel");
        assert_eq!(res8.repo, "next.js");
    }

    #[test]
    fn test_cluster_files_into_layers() {
        let files = vec![
            "src/client/App.tsx".to_string(),
            "src/client/components/Button.tsx".to_string(),
            "src/server/routes/api.ts".to_string(),
            "src/server/handlers/user.ts".to_string(),
            "src/db/models/user.prisma".to_string(),
            "src/utils/crypto.ts".to_string(),
            ".github/workflows/ci.yml".to_string(),
        ];

        let (layers, edges, entry_points) = cluster_files_into_layers(&files, "TypeScript");

        assert!(layers.iter().any(|l| l.id == "layer_frontend"));
        assert!(layers.iter().any(|l| l.id == "layer_backend"));
        assert!(layers.iter().any(|l| l.id == "layer_data"));
        assert!(layers.iter().any(|l| l.id == "layer_infra"));
        assert!(layers.iter().any(|l| l.id == "layer_shared"));
        assert!(!edges.is_empty());
        assert!(entry_points.contains(&"src/client/App.tsx".to_string()));
    }

    #[test]
    fn test_extract_imports_from_source_ts() {
        let code = r#"
            import React, { useState } from 'react';
            import { Header } from './components/Header';
            import { calculateScore } from '@/utils/math';
            const config = require('../config/env');
            export * from './types';
        "#;

        let imports = extract_imports_from_source("src/App.tsx", code);
        assert!(imports.contains(&"react".to_string()));
        assert!(imports.contains(&"./components/Header".to_string()));
        assert!(imports.contains(&"@/utils/math".to_string()));
        assert!(imports.contains(&"../config/env".to_string()));
        assert!(imports.contains(&"./types".to_string()));
    }

    #[test]
    fn test_extract_imports_from_source_py_rs() {
        let py_code = r#"
            import os, sys
            from services.auth import verify_token
        "#;
        let py_imports = extract_imports_from_source("app.py", py_code);
        assert!(py_imports.contains(&"os".to_string()));
        assert!(py_imports.contains(&"services/auth".to_string()));

        let rs_code = r#"
            use crate::network::Client;
            use crate::commands::{show_sidebar, hide_sidebar};
            mod helper;
        "#;
        let rs_imports = extract_imports_from_source("src/main.rs", rs_code);
        assert!(rs_imports.contains(&"network/Client".to_string()));
        assert!(rs_imports.contains(&"commands".to_string()));
        assert!(rs_imports.contains(&"helper".to_string()));
    }

    #[test]
    fn test_resolve_imported_files() {
        let known_files: HashSet<String> = [
            "src/App.tsx".to_string(),
            "src/components/Header.tsx".to_string(),
            "src/utils/math.ts".to_string(),
            "src/config/env.ts".to_string(),
        ]
        .into_iter()
        .collect();

        let imports = vec![
            "./components/Header".to_string(),
            "@/utils/math".to_string(),
            "react".to_string(),
        ];

        let resolved = resolve_imported_files("src/App.tsx", &imports, &known_files);
        assert!(resolved.contains(&"src/components/Header.tsx".to_string()));
        assert!(resolved.contains(&"src/utils/math.ts".to_string()));
        assert!(!resolved.contains(&"react".to_string()));
    }

    #[test]
    fn test_reverse_bfs_impact_query() {
        // Setup a mock cached dependency graph:
        // App.tsx -> Dashboard.tsx -> client.ts -> http.ts
        //                             auth.ts  -> http.ts
        let mut graph = DiGraph::<String, ()>::new();
        let mut node_indices = HashMap::new();
        let mut index_to_file = HashMap::new();

        let files = vec![
            "src/App.tsx".to_string(),
            "src/Dashboard.tsx".to_string(),
            "src/client.ts".to_string(),
            "src/auth.ts".to_string(),
            "src/http.ts".to_string(),
        ];

        for f in &files {
            let idx = graph.add_node(f.clone());
            node_indices.insert(f.clone(), idx);
            index_to_file.insert(idx, f.clone());
        }

        // App imports Dashboard
        graph.add_edge(node_indices["src/App.tsx"], node_indices["src/Dashboard.tsx"], ());
        // Dashboard imports client
        graph.add_edge(node_indices["src/Dashboard.tsx"], node_indices["src/client.ts"], ());
        // client imports http
        graph.add_edge(node_indices["src/client.ts"], node_indices["src/http.ts"], ());
        // auth imports http
        graph.add_edge(node_indices["src/auth.ts"], node_indices["src/http.ts"], ());

        let phase2_resp = Phase2Response {
            owner: "test".into(),
            repo: "mock".into(),
            total_files: files.len(),
            files_analyzed: files.len(),
            nodes: HashMap::new(),
            circular_deps: Vec::new(),
            hotspots: Vec::new(),
            isolated: Vec::new(),
            entry_points: Vec::new(),
            summary: "Mock graph".into(),
        };

        *CACHED_GRAPH.lock() = Some(Arc::new(CachedGraphState {
            owner: "test".into(),
            repo: "mock".into(),
            graph,
            node_indices,
            index_to_file,
            phase2_response: phase2_resp,
        }));

        // Query impact of changing "src/http.ts"
        let impact = query_impact("src/http.ts".to_string(), Some(5)).expect("Impact query should succeed");

        assert_eq!(impact.target_file, "src/http.ts");
        assert_eq!(impact.direct_count, 2); // client.ts and auth.ts
        assert!(impact.affected_files.contains(&"src/client.ts".to_string()));
        assert!(impact.affected_files.contains(&"src/auth.ts".to_string()));
        assert!(impact.affected_files.contains(&"src/Dashboard.tsx".to_string()));
        assert!(impact.affected_files.contains(&"src/App.tsx".to_string()));
        assert_eq!(impact.max_depth, 3); // http -> client -> Dashboard -> App

        // Test fuzzy lookup (passing "http" instead of "src/http.ts")
        let fuzzy_impact = query_impact("http".to_string(), Some(5)).expect("Fuzzy impact query should succeed");
        assert_eq!(fuzzy_impact.target_file, "src/http.ts");
        assert_eq!(fuzzy_impact.direct_count, 2);
    }
}

