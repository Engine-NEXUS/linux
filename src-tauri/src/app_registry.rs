//! Pre-indexed app registry with disk cache and fuzzy matching.
//!
//! Architecture (same pattern as Raycast, Alfred, PowerToys Run):
//!   1. STARTUP: Load disk cache → build in-memory HashMap → background refresh
//!   2. ON COMMAND: O(1) HashMap lookup + fuzzy match → direct platform launch
//!   3. BACKGROUND: Periodic re-scan of installed apps (every 5 minutes)
//!
//! This eliminates the ~566ms Get-StartApps call on every command (Windows) and
//! replaces `cmd /c start` with direct ShellExecuteW / open -b / exec calls.
//!
//! Cross-platform:
//!   - Windows: Get-StartApps + App Paths registry + URL fallback
//!   - macOS: /Applications scan + bundle ID + `open -b`
//!   - Linux: XDG .desktop files + direct exec

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ─── Types ─────────────────────────────────────────────────────────────────

/// How to launch an app on each platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LaunchMethod {
    /// Windows: shell:AppsFolder\{aumid} via ShellExecuteW
    #[serde(rename = "aumid")]
    Aumid { aumid: String },
    /// Windows: direct exe path via ShellExecuteW
    #[serde(rename = "exe")]
    Exe { path: String },
    /// macOS: bundle ID via `open -b`
    #[serde(rename = "bundle")]
    Bundle { bundle_id: String },
    /// macOS: app path via `open -a`
    #[serde(rename = "app_path")]
    AppPath { path: String },
    /// Linux: Exec line from .desktop file
    #[serde(rename = "desktop_exec")]
    DesktopExec { exec: String },
    /// All platforms: URL fallback
    #[serde(rename = "url")]
    Url { url: String },
}

/// A cached app entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEntry {
    /// Display name (e.g. "Google Chrome")
    pub display_name: String,
    /// Normalized search names (lowercase, e.g. ["chrome", "google chrome"])
    pub search_names: Vec<String>,
    /// How to launch this app
    pub launch: LaunchMethod,
    /// Usage count (for ranking)
    #[serde(default)]
    pub use_count: u32,
    /// Last used timestamp (unix seconds)
    #[serde(default)]
    pub last_used: u64,
}

/// The disk cache format.
#[derive(Debug, Serialize, Deserialize)]
struct DiskCache {
    version: u32,
    updated_at: u64,
    /// Date string (YYYY-MM-DD) of the last OS scan.
    /// Used to skip re-scanning if the cache is already from today.
    #[serde(default)]
    last_scan_date: String,
    entries: Vec<AppEntry>,
}

const CACHE_VERSION: u32 = 2;
/// Check hourly whether it's a new day (for daily scan).
const REFRESH_CHECK_INTERVAL: Duration = Duration::from_secs(3600); // 1 hour

// ─── Resolution cache (phrase → app mapping, remembers user choices) ───────

/// A resolved app mapping — remembers which app was chosen for a spoken phrase.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResolutionEntry {
    /// The display name of the matched app
    display_name: String,
    /// The search name that was matched
    matched_name: String,
    /// How many times this phrase has been used
    use_count: u32,
    /// Last used timestamp (unix seconds)
    last_used: u64,
}

/// Disk format for the resolution cache.
#[derive(Debug, Serialize, Deserialize)]
struct ResolutionDiskCache {
    version: u32,
    entries: HashMap<String, ResolutionEntry>,
}

const RESOLUTION_CACHE_VERSION: u32 = 2;

// ─── Global singleton ──────────────────────────────────────────────────────

static REGISTRY: Lazy<Arc<AppRegistry>> = Lazy::new(|| Arc::new(AppRegistry::new()));

pub struct AppRegistry {
    /// name → AppEntry lookup (key = lowercase normalized name)
    cache: RwLock<HashMap<String, AppEntry>>,
    /// When the cache was last refreshed from OS sources
    last_refresh: RwLock<Option<Instant>>,
    /// Resolution cache: spoken phrase → resolved app (avoids re-lookup)
    resolution: RwLock<HashMap<String, ResolutionEntry>>,
    /// Last scan date (YYYY-MM-DD) — used for daily scan logic
    last_scan_date: RwLock<Option<String>>,
}

impl AppRegistry {
    fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            last_refresh: RwLock::new(None),
            resolution: RwLock::new(HashMap::new()),
            last_scan_date: RwLock::new(None),
        }
    }
}

/// Get today's date as a string (YYYY-MM-DD) for daily scan comparison.
fn today_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Simple date calculation from unix timestamp
    let days = secs / 86400;
    let (year, month, day) = days_to_date(days);
    format!("{:04}-{:02}-{:02}", year, month, day)
}

/// Convert days since epoch (1970-01-01) to (year, month, day).
/// Uses the Howard Hinnant algorithm (civil_from_days).
fn days_to_date(days: u64) -> (u32, u32, u32) {
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (year as u32, m as u32, d as u32)
}

/// Initialize the registry: load disk cache, start background refresh.
/// Call this once at app startup (non-blocking).
///
/// DAILY SCAN LOGIC:
///   - Load disk cache synchronously (instant — just reads JSON)
///   - If cache was scanned TODAY → skip scan (apps haven't changed)
///   - If cache is from a previous day → background scan (catches installs/uninstalls)
///   - Hourly check: if it's a new day → one scan
///   - Maximum 1 scan per day (down from 288 with the old 5-min interval)
pub fn init() {
    let registry = REGISTRY.clone();

    // Load disk cache synchronously (fast — just reading a JSON file)
    let mut cache_from_today = false;
    if let Some(disk) = load_disk_cache_with_date() {
        *registry.last_scan_date.write() = Some(disk.last_scan_date.clone());
        let today = today_string();
        if disk.last_scan_date == today {
            cache_from_today = true;
            tracing::info!(
                "app registry: disk cache is from today ({}), skipping scan",
                today
            );
        }

        let mut cache = registry.cache.write();
        for entry in disk.entries {
            for name in &entry.search_names {
                // Skip stop-words from stale disk caches — generic words like
                // "the" (from long PWA titles) used to claim an entire hash
                // key via last-write-wins, hijacking unrelated queries.
                if is_stopword(name) {
                    continue;
                }
                cache.insert(name.clone(), entry.clone());
            }
        }
        tracing::info!(
            "app registry: loaded {} entries from disk cache",
            cache.len()
        );
    }

    // Load resolution cache (phrase → app mapping)
    if let Some(entries) = load_resolution_cache() {
        let mut res = registry.resolution.write();
        for (phrase, entry) in entries {
            res.insert(phrase, entry);
        }
        tracing::info!("app registry: loaded {} resolution cache entries", res.len());
    }

    // Background thread: scan if needed, then check hourly for new day
    std::thread::Builder::new()
        .name("app-registry-refresh".into())
        .spawn(move || {
            // Only scan if the cache is NOT from today
            if !cache_from_today {
                refresh_from_os(&registry);
            }

            // Hourly check: is it a new day?
            loop {
                std::thread::sleep(REFRESH_CHECK_INTERVAL);
                let today = today_string();
                let last = registry.last_scan_date.read().clone();
                if last.as_deref() != Some(today.as_str()) {
                    tracing::info!("app registry: new day detected ({}), refreshing...", today);
                    refresh_from_os(&registry);
                }
            }
        })
        .ok();

    // Start background window cache (for instant focus-existing-app)
    init_window_cache();
}

/// Look up an app by name. Returns the best match or None.
/// This is the hot path — must be <1ms.
///
/// Resolution order:
///   1. Resolution cache (phrase → app, remembers user's previous choice) — O(1)
///   2. Exact match in app cache — O(1)
///   3. Prefix/contains/fuzzy match — O(n) but only on cache miss
pub fn lookup(query: &str) -> Option<AppEntry> {
    let registry = &*REGISTRY;
    let q = query.to_lowercase();

    // 1. Resolution cache — if we've resolved this phrase before, use the saved result.
    // This is the fastest path (~0.01ms) and remembers the user's preferred app
    // when multiple apps match the same name.
    {
        let res = registry.resolution.read();
        if let Some(resolved) = res.get(&q) {
            // Find the actual AppEntry by display name
            let cache = registry.cache.read();
            // Try exact match on the resolved display name first
            for entry in cache.values() {
                if entry.display_name.to_lowercase() == resolved.display_name.to_lowercase() {
                    return Some(entry.clone());
                }
            }
            // Fallback: try the matched_name
            if let Some(entry) = cache.get(&resolved.matched_name) {
                return Some(entry.clone());
            }
            // Resolution cache is stale (app may have been uninstalled) — fall through
            tracing::debug!(
                "resolution cache stale for '{}': app '{}' not found",
                q,
                resolved.display_name
            );
        }
    }

    let cache = registry.cache.read();

    // 2. Exact match (O(1) HashMap lookup)
    if let Some(entry) = cache.get(&q) {
        return Some(entry.clone());
    }

    // 3. Prefix match — "calc" matches "calculator"
    let mut best: Option<(&AppEntry, usize)> = None;
    for (name, entry) in cache.iter() {
        if name.starts_with(&q) || q.starts_with(name.as_str()) {
            let score = score_match(&q, name, entry);
            if best.is_none() || score > best.unwrap().1 {
                best = Some((entry, score));
            }
        }
    }

    // 4. Contains match — "chrome" in "google chrome".
    // Guard: keys shorter than 4 chars never participate — a 3-letter key
    // like "the"/"top" would otherwise match almost any query via
    // q.contains(name) and hijack it (last-write-wins key ownership).
    if best.is_none() {
        for (name, entry) in cache.iter() {
            if name.len() < 4 {
                continue;
            }
            if name.contains(&q) || q.contains(name.as_str()) {
                let score = score_match(&q, name, entry);
                if best.is_none() || score > best.unwrap().1 {
                    best = Some((entry, score));
                }
            }
        }
    }

    // 5. Levenshtein fuzzy match (for typos: "chroem" → "chrome")
    if best.is_none() && q.len() >= 3 {
        for (name, entry) in cache.iter() {
            let dist = levenshtein(&q, name);
            if dist <= 2 && dist < q.len() / 2 {
                let score = score_match(&q, name, entry);
                if best.is_none() || score > best.unwrap().1 {
                    best = Some((entry, score));
                }
            }
        }
    }

    best.map(|(e, _)| e.clone())
}

/// Force a manual refresh of the app registry (e.g. "NEXUS, refresh apps").
/// Runs synchronously so the caller knows when it's done.
pub fn force_refresh() {
    let registry = &*REGISTRY;
    refresh_from_os(registry);
}

/// Get all search names from the registry (for phonetic/fuzzy matching
/// in the intent parser). Returns a Vec of lowercase app name strings.
pub fn all_search_names() -> Vec<String> {
    let registry = &*REGISTRY;
    let cache = registry.cache.read();
    let mut names = Vec::with_capacity(cache.len());
    for (name, _entry) in cache.iter() {
        names.push(name.clone());
    }
    names
}

/// Record that an app was used (for usage-weighted ranking + resolution cache).
/// Also saves the phrase → app mapping so future lookups are instant.
pub fn record_usage(query: &str) {
    let registry = &*REGISTRY;
    let q = query.to_lowercase();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Update use_count in the app cache
    let matched_entry: Option<AppEntry> = {
        let mut cache = registry.cache.write();
        let mut matched = None;
        for entry in cache.values_mut() {
            if entry.search_names.contains(&q) {
                entry.use_count += 1;
                entry.last_used = now;
                if matched.is_none() {
                    matched = Some(entry.clone());
                }
            }
        }
        matched
    };

    // Update resolution cache (phrase → app mapping)
    if let Some(entry) = &matched_entry {
        let mut res = registry.resolution.write();
        let res_entry = res.entry(q.clone()).or_insert_with(|| ResolutionEntry {
            display_name: entry.display_name.clone(),
            matched_name: q.clone(),
            use_count: 0,
            last_used: 0,
        });
        res_entry.use_count += 1;
        res_entry.last_used = now;
        res_entry.display_name = entry.display_name.clone();

        // Save resolution cache to disk in background
        let res_snapshot: HashMap<String, ResolutionEntry> = res.clone();
        std::thread::spawn(move || {
            save_resolution_cache(&res_snapshot);
        });
    }

    // Save app cache to disk in background
    let entries: Vec<AppEntry> = {
        let cache = registry.cache.read();
        deduplicated_entries(&cache)
    };
    let last_scan = registry.last_scan_date.read().clone();
    std::thread::spawn(move || {
        save_disk_cache_with_date(&entries, last_scan.as_deref());
    });
}

/// Launch an app using the most direct platform API available.
/// Returns Ok(()) on successful spawn, Err on failure.
pub fn launch(entry: &AppEntry) -> Result<(), String> {
    tracing::info!("launching app: {} via {:?}", entry.display_name, entry.launch);
    match &entry.launch {
        #[cfg(target_os = "windows")]
        LaunchMethod::Aumid { aumid } => launch_shell_execute(&format!("shell:AppsFolder\\{}", aumid)),
        #[cfg(target_os = "windows")]
        LaunchMethod::Exe { path } => launch_shell_execute(path),
        #[cfg(target_os = "macos")]
        LaunchMethod::Bundle { bundle_id } => {
            std::process::Command::new("open")
                .args(["-b", bundle_id])
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        #[cfg(target_os = "macos")]
        LaunchMethod::AppPath { path } => {
            std::process::Command::new("open")
                .args(["-a", path])
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        #[cfg(target_os = "linux")]
        LaunchMethod::DesktopExec { exec } => {
            std::process::Command::new("sh")
                .args(["-c", exec])
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        LaunchMethod::Url { url } => {
            open::that(url).map_err(|e| e.to_string())
        }
        // Cross-compile: non-matching platform variants
        #[allow(unreachable_patterns)]
        _ => {
            tracing::warn!("launch method not supported on this platform: {:?}", entry.launch);
            Err("launch method not supported on this platform".to_string())
        }
    }
}

// ─── Window cache: background enumeration for instant focus ────────────────

/// A cached window entry, refreshed every 2 seconds in the background.
#[derive(Debug, Clone)]
struct CachedWindow {
    hwnd: isize,
    pid: u32,
    title: String,
    process_name: String,
}

static WINDOW_CACHE: Lazy<RwLock<Vec<CachedWindow>>> = Lazy::new(|| RwLock::new(Vec::new()));

/// Start the background window cache refresh thread.
/// Call this once at startup (non-blocking).
pub fn init_window_cache() {
    std::thread::Builder::new()
        .name("window-cache".into())
        .spawn(|| {
            loop {
                refresh_window_cache();
                std::thread::sleep(Duration::from_secs(2));
            }
        })
        .ok();
}

/// Check if an app is already running and focus its window.
/// Returns true if an existing window was found and focused.
/// This is the FIRST priority in the launch flow.
pub fn try_focus_existing(entry: &AppEntry) -> bool {
    let cache = WINDOW_CACHE.read();
    if cache.is_empty() {
        return false;
    }

    // Build search terms from the entry
    let search_terms: Vec<&str> = entry.search_names.iter().map(|s| s.as_str()).collect();
    let display_lower = entry.display_name.to_lowercase();

    // Priority 1: Match by window title (most reliable)
    // e.g. "YouTube - Brave" matches "youtube"
    for w in cache.iter() {
        let title_lower = w.title.to_lowercase();
        if search_terms.iter().any(|term| title_lower.contains(term)) {
            tracing::info!(
                "focus hit: '{}' matched window title '{}' (pid={})",
                entry.display_name,
                w.title,
                w.pid
            );
            focus_window(w.hwnd);
            return true;
        }
    }

    // Priority 2: Match by process name
    // e.g. "notepad.exe" matches "notepad"
    for w in cache.iter() {
        let proc_lower = &w.process_name;
        if search_terms.iter().any(|term| proc_lower.contains(term)) {
            tracing::info!(
                "focus hit: '{}' matched process '{}' (pid={})",
                entry.display_name,
                w.process_name,
                w.pid
            );
            focus_window(w.hwnd);
            return true;
        }
    }

    // Priority 3: Match by display name in title
    for w in cache.iter() {
        let title_lower = w.title.to_lowercase();
        if title_lower.contains(&display_lower) {
            tracing::info!(
                "focus hit: '{}' matched display name in title '{}' (pid={})",
                entry.display_name,
                w.title,
                w.pid
            );
            focus_window(w.hwnd);
            return true;
        }
    }

    false
}

/// Focus a window: restore if minimized, bring to foreground.
/// Uses AttachThreadInput trick to bypass Windows' focus-stealing prevention
/// (NEXUS is a background process, so SetForegroundWindow would normally be rejected).
fn focus_window(hwnd_val: isize) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId,
            IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
        };
        use windows::Win32::System::Threading::{
            AttachThreadInput, GetCurrentThreadId,
        };

        let hwnd = HWND(hwnd_val);
        unsafe {
            // Restore if minimized
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }

            // AttachThreadInput trick: attach our input queue to the current
            // foreground thread's, so the OS treats our SetForegroundWindow
            // request as coming from the active app.
            let fg = GetForegroundWindow();
            let mut fg_pid: u32 = 0;
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

            tracing::debug!("focused window hwnd={}", hwnd_val);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd_val;
    }
}

/// Refresh the window cache by enumerating all visible windows.
fn refresh_window_cache() {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
        };

        let mut windows: Vec<CachedWindow> = Vec::new();

        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let windows = &mut *(lparam.0 as *mut Vec<CachedWindow>);
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }

            let mut title_buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut title_buf);
            if len <= 0 {
                return BOOL(1); // Skip windows with no title
            }

            let title = String::from_utf16_lossy(&title_buf[..len as usize]);
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);

            windows.push(CachedWindow {
                hwnd: hwnd.0,
                pid,
                title,
                process_name: String::new(), // filled in below
            });
            BOOL(1)
        }

        unsafe {
            let lparam = LPARAM(&mut windows as *mut Vec<CachedWindow> as isize);
            let _ = EnumWindows(Some(collect), lparam);
        }

        // Fill in process names using sysinfo (in background, so speed is fine)
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            false,
            ProcessRefreshKind::new(),
        );

        for w in &mut windows {
            if let Some(proc) = sys.process(sysinfo::Pid::from_u32(w.pid)) {
                w.process_name = proc.name().to_string_lossy().to_lowercase();
            }
        }

        let count = windows.len();
        *WINDOW_CACHE.write() = windows;
        tracing::trace!("window cache refreshed: {} windows", count);
    }

    #[cfg(target_os = "macos")]
    {
        // macOS: `open -a` already focuses existing instances, so we don't
        // need a window cache. But we could use NSWorkspace if needed.
    }

    #[cfg(target_os = "linux")]
    {
        // Linux: `wmctrl -a` focuses by title. We could cache `wmctrl -l` output.
        use std::process::Command;
        let output = Command::new("wmctrl").args(["-l", "-p"]).output();
        if let Ok(out) = output {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let mut windows: Vec<CachedWindow> = Vec::new();
                for line in stdout.lines() {
                    // wmctrl -l -p format: <hwnd> <desktop> <pid> <host> <title>
                    let parts: Vec<&str> = line.splitn(5, char::is_whitespace).collect();
                    if parts.len() < 5 { continue; }
                    let hwnd = parts[0].parse::<isize>().unwrap_or(0);
                    let pid = parts[2].parse::<u32>().unwrap_or(0);
                    let title = parts[4..].join(" ");
                    windows.push(CachedWindow {
                        hwnd,
                        pid,
                        title,
                        process_name: String::new(),
                    });
                }
                *WINDOW_CACHE.write() = windows;
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn launch_shell_execute(target: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::PCWSTR;

    let wide_open: Vec<u16> = "open\0".encode_utf16().collect();
    let wide_target: Vec<u16> = format!("{}\0", target).encode_utf16().collect();
    let null = PCWSTR(std::ptr::null());

    unsafe {
        let result = ShellExecuteW(
            None,                                    // hwnd
            PCWSTR(wide_open.as_ptr()),              // lpOperation = "open"
            PCWSTR(wide_target.as_ptr()),            // lpFile = target
            null,                                    // lpParameters
            null,                                    // lpDirectory
            SW_SHOWNORMAL.0 as i32,                // nShowCmd
        );
        // ShellExecuteW returns HINSTANCE; values > 32 indicate success
        let code = result.0 as usize;
        if code > 32 {
            Ok(())
        } else {
            Err(format!("ShellExecuteW failed with code {}", code))
        }
    }
}

// ─── Scoring ───────────────────────────────────────────────────────────────

fn score_match(query: &str, name: &str, entry: &AppEntry) -> usize {
    let mut score = 0;

    // Exact match gets highest score
    if query == name {
        score += 1000;
    }
    // Shorter names are preferred (more specific)
    score += 100usize.saturating_sub(name.len());
    // Usage-weighted: frequently used apps rank higher
    score += (entry.use_count as usize) * 10;
    // Recency bonus
    if entry.last_used > 0 {
        score += 5;
    }

    score
}

fn levenshtein(a: &str, b: &str) -> usize {
    let m = a.len();
    let n = b.len();
    if m == 0 { return n; }
    if n == 0 { return m; }

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0; n + 1];

    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1)
                .min(curr[j] + 1)
                .min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

// ─── Disk cache ────────────────────────────────────────────────────────────

fn cache_path() -> PathBuf {
    let base = dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("nexus");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("app_cache.json")
}

fn resolution_cache_path() -> PathBuf {
    let base = dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("nexus");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("app_resolution_cache.json")
}

fn load_resolution_cache() -> Option<HashMap<String, ResolutionEntry>> {
    let path = resolution_cache_path();
    let data = std::fs::read_to_string(&path).ok()?;
    let cache: ResolutionDiskCache = serde_json::from_str(&data).ok()?;
    if cache.version != RESOLUTION_CACHE_VERSION {
        tracing::warn!("resolution cache version mismatch, ignoring");
        return None;
    }
    Some(cache.entries)
}

fn save_resolution_cache(entries: &HashMap<String, ResolutionEntry>) {
    let cache = ResolutionDiskCache {
        version: RESOLUTION_CACHE_VERSION,
        entries: entries.clone(),
    };
    let path = resolution_cache_path();
    match serde_json::to_string_pretty(&cache) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                tracing::warn!("failed to save resolution cache: {}", e);
            } else {
                tracing::debug!("saved resolution cache: {} entries to {}", entries.len(), path.display());
            }
        }
        Err(e) => tracing::warn!("failed to serialize resolution cache: {}", e),
    }
}

fn load_disk_cache_with_date() -> Option<DiskCache> {
    let path = cache_path();
    let data = std::fs::read_to_string(&path).ok()?;
    let cache: DiskCache = serde_json::from_str(&data).ok()?;
    // Accept version 1 (old format without last_scan_date) and version 2 (current)
    if cache.version > CACHE_VERSION {
        tracing::warn!("disk cache version {} > {}, ignoring", cache.version, CACHE_VERSION);
        return None;
    }
    tracing::info!(
        "loaded disk cache: {} entries, last scan: {}",
        cache.entries.len(),
        if cache.last_scan_date.is_empty() { "unknown".to_string() } else { cache.last_scan_date.clone() }
    );
    Some(cache)
}

fn save_disk_cache_with_date(entries: &[AppEntry], last_scan_date: Option<&str>) {
    let cache = DiskCache {
        version: CACHE_VERSION,
        updated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        last_scan_date: last_scan_date.unwrap_or("").to_string(),
        entries: entries.to_vec(),
    };
    let path = cache_path();
    match serde_json::to_string_pretty(&cache) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                tracing::warn!("failed to save disk cache: {}", e);
            } else {
                tracing::debug!("saved disk cache: {} entries to {}", entries.len(), path.display());
            }
        }
        Err(e) => tracing::warn!("failed to serialize disk cache: {}", e),
    }
}

fn deduplicated_entries(cache: &HashMap<String, AppEntry>) -> Vec<AppEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();
    for entry in cache.values() {
        let key = entry.display_name.to_lowercase();
        if seen.insert(key) {
            entries.push(entry.clone());
        }
    }
    entries
}

// ─── OS-specific app discovery ─────────────────────────────────────────────

fn refresh_from_os(registry: &AppRegistry) {
    let start = Instant::now();
    tracing::info!("app registry: refreshing from OS...");

    let mut entries: Vec<AppEntry> = Vec::new();

    // Platform-specific discovery
    #[cfg(target_os = "windows")]
    discover_windows(&mut entries);
    #[cfg(target_os = "macos")]
    discover_macos(&mut entries);
    #[cfg(target_os = "linux")]
    discover_linux(&mut entries);

    // Always add URL fallbacks
    add_url_fallbacks(&mut entries);

    // Merge into the cache (preserve usage stats from existing entries)
    let mut cache = registry.cache.write();
    let old_usage: HashMap<String, (u32, u64)> = cache
        .iter()
        .map(|(k, v)| (k.clone(), (v.use_count, v.last_used)))
        .collect();

    cache.clear();
    for mut entry in entries {
        // Restore usage stats from previous cache
        for name in &entry.search_names {
            if let Some(&(count, last)) = old_usage.get(name) {
                entry.use_count = entry.use_count.max(count);
                entry.last_used = entry.last_used.max(last);
            }
        }
        for name in entry.search_names.clone() {
            // Never index stop-words (see build_search_names) — a stale
            // disk cache or fresh scan must not reintroduce them.
            if is_stopword(&name) {
                continue;
            }
            cache.insert(name, entry.clone());
        }
    }

    *registry.last_refresh.write() = Some(Instant::now());

    // Update last_scan_date to today
    let today = today_string();
    *registry.last_scan_date.write() = Some(today.clone());

    let elapsed = start.elapsed();
    tracing::info!(
        "app registry: refresh complete ({} entries in {:.1}ms, scan date: {})",
        cache.len(),
        elapsed.as_secs_f64() * 1000.0,
        today
    );

    // Save to disk for cold starts (with scan date)
    let disk_entries = deduplicated_entries(&cache);
    std::thread::spawn(move || save_disk_cache_with_date(&disk_entries, Some(&today)));
}

// ─── Windows discovery ─────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn discover_windows(entries: &mut Vec<AppEntry>) {
    let before = entries.len();

    // 1. Get-StartApps (universal: native Win32, UWP, PWA, Squirrel apps)
    discover_start_apps(entries);

    // 2. Known native apps with direct exe paths (fastest launch)
    discover_known_windows_apps(entries);

    // 3. App Paths registry (Run dialog resolver: winword.exe → Word)
    discover_app_paths_registry(entries);

    // 4. Uninstall registry (install location + display name for all installed apps)
    discover_uninstall_registry(entries);

    // 5. Start Menu .lnk resolution (direct exe paths for Win32 apps)
    discover_lnk_targets(entries);

    // 6. PATH scan for CLI tools (git, python, node, etc.)
    discover_path_executables(entries);

    tracing::info!(
        "Windows discovery: {} total apps ({} from additional sources)",
        entries.len(),
        entries.len() - before
    );
}

#[cfg(target_os = "windows")]
fn discover_start_apps(entries: &mut Vec<AppEntry>) {
    use std::process::Command;

    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", "Get-StartApps | ConvertTo-Json -Compress"])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output();

    let json_str = match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        Ok(out) => {
            tracing::warn!("Get-StartApps failed: {}", String::from_utf8_lossy(&out.stderr));
            return;
        }
        Err(e) => {
            tracing::warn!("failed to run Get-StartApps: {}", e);
            return;
        }
    };

    // Parse JSON: [{"Name":"...", "AppID":"..."}]
    // Use serde_json for reliable parsing
    let items: Vec<serde_json::Value> = match serde_json::from_str(&json_str) {
        Ok(serde_json::Value::Array(arr)) => arr,
        Ok(serde_json::Value::Object(obj)) => vec![serde_json::Value::Object(obj)],
        _ => {
            tracing::warn!("failed to parse Get-StartApps JSON");
            return;
        }
    };

    for item in items {
        let name = item["Name"].as_str().unwrap_or("").to_string();
        let app_id = item["AppID"].as_str().unwrap_or("").to_string();
        if name.is_empty() || app_id.is_empty() {
            continue;
        }

        let search_names = build_search_names(&name);
        entries.push(AppEntry {
            display_name: name,
            search_names,
            launch: LaunchMethod::Aumid { aumid: app_id },
            use_count: 0,
            last_used: 0,
        });
    }

    tracing::debug!("discovered {} apps from Get-StartApps", entries.len());
}

#[cfg(target_os = "windows")]
fn discover_known_windows_apps(entries: &mut Vec<AppEntry>) {
    // Well-known native apps with direct exe paths.
    // These are faster than AUMID because ShellExecuteW(exe) doesn't go through
    // the shell:AppsFolder layer.
    let known: &[(&str, &str, &[&str])] = &[
        ("Notepad", "notepad.exe", &["notepad"]),
        ("Calculator", "calc.exe", &["calculator", "calc"]),
        ("Paint", "mspaint.exe", &["paint", "mspaint"]),
        ("Task Manager", "taskmgr.exe", &["task manager", "taskmgr"]),
        ("Command Prompt", "cmd.exe", &["command prompt", "cmd"]),
        ("PowerShell", "powershell.exe", &["powershell"]),
    ];

    for (display, exe, names) in known {
        entries.push(AppEntry {
            display_name: display.to_string(),
            search_names: names.iter().map(|s| s.to_string()).collect(),
            launch: LaunchMethod::Exe { path: exe.to_string() },
            use_count: 0,
            last_used: 0,
        });
    }
}

/// Scan App Paths registry: HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths
/// This is what the Windows "Run" dialog uses to resolve app names to exe paths.
/// Finds apps like: winword.exe → Word, excel.exe → Excel, etc.
#[cfg(target_os = "windows")]
fn discover_app_paths_registry(entries: &mut Vec<AppEntry>) {
    use winreg::enums::*;
    use winreg::RegKey;

    let paths = [
        (RegKey::predef(HKEY_LOCAL_MACHINE), r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths"),
        (RegKey::predef(HKEY_LOCAL_MACHINE), r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths"),
        (RegKey::predef(HKEY_CURRENT_USER), r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths"),
    ];

    let mut count = 0;
    for (root, path) in &paths {
        if let Ok(app_paths) = root.open_subkey(path) {
            for key_name in app_paths.enum_keys().flatten() {
                if let Ok(subkey) = app_paths.open_subkey(&key_name) {
                    // The default value is the exe path
                    if let Ok(exe_path) = subkey.get_value::<String, _>("") {
                        if exe_path.is_empty() { continue; }
                        // Clean up the path (remove quotes, environment variables)
                        let clean_path = exe_path.trim_matches('"').to_string();
                        if !clean_path.to_lowercase().ends_with(".exe") { continue; }

                        // Display name from the key (e.g. "winword.exe" → "Winword")
                        let display = key_name.trim_end_matches(".exe")
                            .replace('_', " ");
                        let display = capitalize_words(&display);

                        let search_names = build_search_names(&display);
                        // Also add the raw key name (e.g. "winword.exe" → "winword")
                        let mut all_names = search_names;
                        let key_lower = key_name.trim_end_matches(".exe").to_lowercase();
                        if !all_names.contains(&key_lower) {
                            all_names.push(key_lower);
                        }

                        entries.push(AppEntry {
                            display_name: display,
                            search_names: all_names,
                            launch: LaunchMethod::Exe { path: clean_path },
                            use_count: 0,
                            last_used: 0,
                        });
                        count += 1;
                    }
                }
            }
        }
    }
    tracing::debug!("discovered {} apps from App Paths registry", count);
}

/// Scan Uninstall registry: HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall
/// Finds install location + display name for apps that don't have Start Menu shortcuts.
/// Examples: games, custom installers, portable apps.
#[cfg(target_os = "windows")]
fn discover_uninstall_registry(entries: &mut Vec<AppEntry>) {
    use winreg::enums::*;
    use winreg::RegKey;

    let paths = [
        (RegKey::predef(HKEY_LOCAL_MACHINE), r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (RegKey::predef(HKEY_LOCAL_MACHINE), r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (RegKey::predef(HKEY_CURRENT_USER), r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    ];

    let mut count = 0;
    let existing_names: std::collections::HashSet<String> = entries
        .iter()
        .flat_map(|e| e.search_names.iter().cloned())
        .collect();

    for (root, path) in &paths {
        if let Ok(uninstall) = root.open_subkey(path) {
            for key in uninstall.enum_keys().flatten() {
                if let Ok(subkey) = uninstall.open_subkey(&key) {
                    let display_name: String = match subkey.get_value("DisplayName") {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    if display_name.is_empty() { continue; }

                    // Skip if we already have this app
                    let lower = display_name.to_lowercase();
                    if existing_names.contains(&lower) { continue; }

                    // Try to find the exe path
                    // InstallLocation gives us the directory; we look for the main exe
                    let install_loc: String = subkey.get_value("InstallLocation").unwrap_or_default();
                    let display_icon: String = subkey.get_value("DisplayIcon").unwrap_or_default();

                    // DisplayIcon often has the exe path (sometimes with ",0" suffix)
                    let exe_path = if !display_icon.is_empty() {
                        let clean = display_icon.split(',').next().unwrap_or("")
                            .trim_matches('"').trim();
                        if clean.to_lowercase().ends_with(".exe") {
                            Some(clean.to_string())
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    // If no DisplayIcon, try to find exe in InstallLocation
                    let exe_path = exe_path.or_else(|| {
                        if install_loc.is_empty() { return None; }
                        find_main_exe_in_dir(std::path::Path::new(&install_loc))
                    });

                    if let Some(exe) = exe_path {
                        let search_names = build_search_names(&display_name);
                        entries.push(AppEntry {
                            display_name,
                            search_names,
                            launch: LaunchMethod::Exe { path: exe },
                            use_count: 0,
                            last_used: 0,
                        });
                        count += 1;
                    }
                }
            }
        }
    }
    tracing::debug!("discovered {} apps from Uninstall registry", count);
}

/// Find the main .exe in a directory (the one whose name matches the directory name,
/// or the largest exe, or the only exe).
#[cfg(target_os = "windows")]
fn find_main_exe_in_dir(dir: &std::path::Path) -> Option<String> {
    let readdir = std::fs::read_dir(dir).ok()?;
    let mut exes: Vec<(std::path::PathBuf, u64)> = Vec::new();
    for entry in readdir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("exe") {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            exes.push((path, size));
        }
    }
    if exes.is_empty() { return None; }
    // Prefer exe whose name matches the directory name
    let dir_name = dir.file_name()?.to_string_lossy().to_lowercase();
    for (path, _) in &exes {
        let stem = path.file_stem()?.to_string_lossy().to_lowercase();
        if stem == dir_name {
            return Some(path.to_string_lossy().to_string());
        }
    }
    // Fall back to the largest exe
    exes.sort_by(|a, b| b.1.cmp(&a.1));
    Some(exes[0].0.to_string_lossy().to_string())
}

/// Scan PATH for common CLI tools that users might want to launch.
/// Only includes well-known tools to avoid polluting the registry.
#[cfg(target_os = "windows")]
fn discover_path_executables(entries: &mut Vec<AppEntry>) {
    let path = std::env::var("PATH").unwrap_or_default();
    let known_tools: &[(&str, &[&str])] = &[
        ("Git", &["git", "git bash"]),
        ("Python", &["python", "python3", "py"]),
        ("Node.js", &["node", "nodejs"]),
        ("npm", &["npm"]),
        ("Cargo", &["cargo"]),
        ("Rustc", &["rustc", "rust"]),
        ("Docker", &["docker"]),
        ("Go", &["go", "golang"]),
        ("Java", &["java"]),
        ("Make", &["make"]),
        ("CMake", &["cmake"]),
        ("SSH", &["ssh"]),
        ("SCP", &["scp"]),
        ("Curl", &["curl"]),
        ("Wget", &["wget"]),
        ("Vim", &["vim"]),
        ("Nano", &["nano"]),
        ("FFmpeg", &["ffmpeg"]),
    ];

    let mut count = 0;
    let existing: std::collections::HashSet<String> = entries
        .iter()
        .flat_map(|e| e.search_names.iter().cloned())
        .collect();

    for dir in path.split(';') {
        if dir.is_empty() { continue; }
        let dir_path = std::path::Path::new(dir);
        if !dir_path.exists() { continue; }

        for (display, search) in known_tools {
            for s in *search {
                // Skip if already discovered
                if existing.contains(*s) { continue; }
                let exe_name = format!("{}.exe", s);
                let exe_path = dir_path.join(&exe_name);
                if exe_path.exists() {
                    entries.push(AppEntry {
                        display_name: display.to_string(),
                        search_names: search.iter().map(|x| x.to_string()).collect(),
                        launch: LaunchMethod::Exe {
                            path: exe_path.to_string_lossy().to_string(),
                        },
                        use_count: 0,
                        last_used: 0,
                    });
                    count += 1;
                    break; // Only add once per tool
                }
            }
        }
    }
    tracing::debug!("discovered {} CLI tools from PATH", count);
}

/// Resolve Start Menu .lnk shortcuts to actual exe paths.
/// This gives us direct exe launch (faster than AUMID) for Win32 apps.
#[cfg(target_os = "windows")]
fn discover_lnk_targets(entries: &mut Vec<AppEntry>) {
    use std::path::PathBuf;

    let start_menu_dirs: Vec<PathBuf> = {
        let mut dirs = Vec::new();
        if let Ok(progdata) = std::env::var("ProgramData") {
            dirs.push(PathBuf::from(&progdata)
                .join("Microsoft\\Windows\\Start Menu\\Programs"));
        }
        if let Ok(appdata) = std::env::var("APPDATA") {
            dirs.push(PathBuf::from(&appdata)
                .join("Microsoft\\Windows\\Start Menu\\Programs"));
        }
        dirs
    };

    let mut count = 0;
    let existing: std::collections::HashSet<String> = entries
        .iter()
        .map(|e| e.display_name.to_lowercase())
        .collect();

    for dir in &start_menu_dirs {
        count += scan_lnk_directory(dir, &existing, entries);
    }
    tracing::debug!("discovered {} apps from Start Menu .lnk files", count);
}

#[cfg(target_os = "windows")]
fn scan_lnk_directory(
    dir: &std::path::Path,
    existing: &std::collections::HashSet<String>,
    entries: &mut Vec<AppEntry>,
) -> usize {
    let mut count = 0;
    let readdir = match std::fs::read_dir(dir) { Ok(r) => r, Err(_) => return 0 };

    for entry in readdir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            count += scan_lnk_directory(&path, existing, entries);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("lnk") { continue; }

        let display_name = path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if display_name.is_empty() { continue; }

        // Skip if we already have this app (from Get-StartApps or elsewhere)
        if existing.contains(&display_name.to_lowercase()) { continue; }

        // Parse the .lnk file to find the target exe
        if let Some(exe_path) = resolve_lnk_target(&path) {
            if exe_path.to_lowercase().ends_with(".exe") {
                let search_names = build_search_names(&display_name);
                entries.push(AppEntry {
                    display_name,
                    search_names,
                    launch: LaunchMethod::Exe { path: exe_path },
                    use_count: 0,
                    last_used: 0,
                });
                count += 1;
            }
        }
    }
    count
}

/// Resolve a .lnk shortcut to its target path.
/// Uses the Windows Shell API via PowerShell (fast enough at startup, not at launch time).
#[cfg(target_os = "windows")]
fn resolve_lnk_target(lnk_path: &std::path::Path) -> Option<String> {
    use std::process::Command;

    // Use PowerShell to resolve the shortcut — runs once at startup, not at launch time
    let script = format!(
        "$s=(New-Object -COM WScript.Shell).CreateShortcut('{}'); $s.TargetPath",
        lnk_path.to_string_lossy().replace('\'', "''")
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .ok()?;

    if !output.status.success() { return None; }
    let target = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if target.is_empty() { return None; }
    Some(target)
}

/// Capitalize each word in a string.
#[cfg(target_os = "windows")]
fn capitalize_words(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ─── macOS discovery ───────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn discover_macos(entries: &mut Vec<AppEntry>) {
    let home = std::env::var("HOME").unwrap_or_default();
    let dirs = [
        "/Applications".to_string(),
        "/System/Applications".to_string(),
        "/System/Applications/Utilities".to_string(),
        "/Applications/Utilities".to_string(),
        format!("{}/Applications", home),
        format!("{}/Applications/Utilities", home),
    ];

    for dir in &dirs {
        let path = std::path::Path::new(dir);
        if !path.exists() { continue; }
        if let Ok(readdir) = std::fs::read_dir(path) {
            for entry in readdir.flatten() {
                let fname = entry.file_name().to_string_lossy().to_string();
                if !fname.ends_with(".app") { continue; }

                let app_name = fname.trim_end_matches(".app").to_string();
                let app_path = entry.path().to_string_lossy().to_string();

                // Try to extract bundle ID from Info.plist
                let bundle_id = read_macos_bundle_id(&entry.path());
                let launch = if let Some(bid) = bundle_id {
                    LaunchMethod::Bundle { bundle_id: bid }
                } else {
                    LaunchMethod::AppPath { path: app_path }
                };

                let search_names = build_search_names(&app_name);
                entries.push(AppEntry {
                    display_name: app_name,
                    search_names,
                    launch,
                    use_count: 0,
                    last_used: 0,
                });
            }
        }
    }

    // Spotlight: find apps in non-standard locations (e.g. /Developer, /opt)
    discover_macos_spotlight(entries);

    // PWAs installed via Chrome/Brave/Edge
    discover_macos_pwas(entries);

    tracing::info!("discovered {} apps from macOS", entries.len());
}

/// Discover PWAs installed via Chrome, Brave, or Edge on macOS.
/// These are stored as .app bundles in the browser's Web Applications directory.
#[cfg(target_os = "macos")]
fn discover_macos_pwas(entries: &mut Vec<AppEntry>) {
    let home = std::env::var("HOME").unwrap_or_default();
    let pwa_dirs = [
        // Chrome PWAs
        format!("{}/Applications/Chrome Apps", home),
        // Brave PWAs
        format!("{}/Applications/Brave Apps", home),
        // Edge PWAs
        format!("{}/Applications/Microsoft Edge Apps", home),
    ];

    let existing: std::collections::HashSet<String> = entries
        .iter()
        .map(|e| e.display_name.to_lowercase())
        .collect();

    let mut count = 0;
    for dir in &pwa_dirs {
        let path = std::path::Path::new(dir);
        if !path.exists() { continue; }
        if let Ok(readdir) = std::fs::read_dir(path) {
            for entry in readdir.flatten() {
                let fname = entry.file_name().to_string_lossy().to_string();
                if !fname.ends_with(".app") { continue; }

                let app_name = fname.trim_end_matches(".app").to_string();
                if app_name == "Chrome Apps" || app_name == "Brave Apps" { continue; }
                if existing.contains(&app_name.to_lowercase()) { continue; }

                let app_path = entry.path().to_string_lossy().to_string();
                let bundle_id = read_macos_bundle_id(&entry.path());
                let launch = if let Some(bid) = bundle_id {
                    LaunchMethod::Bundle { bundle_id: bid }
                } else {
                    LaunchMethod::AppPath { path: app_path }
                };

                let search_names = build_search_names(&app_name);
                entries.push(AppEntry {
                    display_name: app_name,
                    search_names,
                    launch,
                    use_count: 0,
                    last_used: 0,
                });
                count += 1;
            }
        }
    }
    if count > 0 {
        tracing::debug!("discovered {} PWAs from macOS browsers", count);
    }
}

/// Use Spotlight's mdfind to find .app bundles in non-standard locations.
/// Spotlight maintains a pre-built index, so this is very fast (~20ms).
#[cfg(target_os = "macos")]
fn discover_macos_spotlight(entries: &mut Vec<AppEntry>) {
    use std::process::Command;

    let output = Command::new("mdfind")
        .args(["kMDItemKind == 'Application'"])
        .output();

    let stdout = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return,
    };

    let existing: std::collections::HashSet<String> = entries
        .iter()
        .map(|e| e.display_name.to_lowercase())
        .collect();

    let mut count = 0;
    for line in stdout.lines() {
        let path = std::path::Path::new(line);
        if !path.ends_with(".app") { continue; }

        let app_name = path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if app_name.is_empty() { continue; }
        if existing.contains(&app_name.to_lowercase()) { continue; }

        let bundle_id = read_macos_bundle_id(path);
        let launch = if let Some(bid) = bundle_id {
            LaunchMethod::Bundle { bundle_id: bid }
        } else {
            LaunchMethod::AppPath { path: line.to_string() }
        };

        let search_names = build_search_names(&app_name);
        entries.push(AppEntry {
            display_name: app_name,
            search_names,
            launch,
            use_count: 0,
            last_used: 0,
        });
        count += 1;
    }
    tracing::debug!("discovered {} additional apps from Spotlight", count);
}

#[cfg(target_os = "macos")]
fn read_macos_bundle_id(app_path: &std::path::Path) -> Option<String> {
    let plist_path = app_path.join("Contents/Info.plist");
    let content = std::fs::read_to_string(&plist_path).ok()?;
    // Simple extraction of CFBundleIdentifier from plist XML
    let key = "<key>CFBundleIdentifier</key>";
    let key_pos = content.find(key)?;
    let after = &content[key_pos + key.len()..];
    let start = after.find("<string>")? + 8;
    let end = after[start..].find("</string>")?;
    Some(after[start..start + end].to_string())
}

// ─── Linux discovery ───────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn discover_linux(entries: &mut Vec<AppEntry>) {
    let home = std::env::var("HOME").unwrap_or_default();

    // XDG data directories
    let xdg_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/share:/usr/local/share".to_string());

    let mut dirs: Vec<String> = xdg_dirs
        .split(':')
        .map(|d| format!("{}/applications", d))
        .collect();
    dirs.push(format!("{}/.local/share/applications", home));
    // Flatpak
    dirs.push("/var/lib/flatpak/exports/share/applications".to_string());
    dirs.push(format!("{}/.local/share/flatpak/exports/share/applications", home));
    // Snap
    dirs.push("/var/lib/snapd/desktop/applications".to_string());

    for dir in &dirs {
        let path = std::path::Path::new(dir);
        if !path.exists() { continue; }
        if let Ok(readdir) = std::fs::read_dir(path) {
            for entry in readdir.flatten() {
                let fname = entry.file_name().to_string_lossy().to_string();
                if !fname.ends_with(".desktop") { continue; }

                if let Some(app_entry) = parse_desktop_file(&entry.path()) {
                    entries.push(app_entry);
                }
            }
        }
    }

    // Also try `flatpak list` for installed Flatpak apps
    discover_flatpak_apps(entries);

    // PWAs installed via Chrome/Brave/Edge
    discover_linux_pwas(entries);

    tracing::info!("discovered {} apps from Linux", entries.len());
}

/// Discover PWAs installed via Chrome, Brave, or Edge on Linux.
/// These are stored as .desktop files in the browser's profile directory
/// with Type=Application and WebApp=true.
#[cfg(target_os = "linux")]
fn discover_linux_pwas(entries: &mut Vec<AppEntry>) {
    let home = std::env::var("HOME").unwrap_or_default();

    // Chrome/Brave/Edge store PWAs in their profile directories
    let pwa_dirs = [
        // Chrome
        format!("{}/.local/share/applications/chrome", home),
        // Brave
        format!("{}/.local/share/applications/brave", home),
        // Edge
        format!("{}/.local/share/applications/microsoft-edge", home),
        // Generic — some browsers put PWAs directly in applications/
        format!("{}/.local/share/applications", home),
    ];

    let existing: std::collections::HashSet<String> = entries
        .iter()
        .map(|e| e.display_name.to_lowercase())
        .collect();

    let mut count = 0;
    for dir in &pwa_dirs {
        let path = std::path::Path::new(dir);
        if !path.exists() { continue; }
        if let Ok(readdir) = std::fs::read_dir(path) {
            for entry in readdir.flatten() {
                let fname = entry.file_name().to_string_lossy().to_string();
                if !fname.ends_with(".desktop") { continue; }

                // Check if this is a PWA (has WebApp=true or StartupWMClass)
                if let Some(app_entry) = parse_pwa_desktop_file(&entry.path()) {
                    if !existing.contains(&app_entry.display_name.to_lowercase()) {
                        entries.push(app_entry);
                        count += 1;
                    }
                }
            }
        }
    }
    if count > 0 {
        tracing::debug!("discovered {} PWAs from Linux browsers", count);
    }
}

/// Parse a .desktop file that might be a PWA.
/// Returns Some(AppEntry) if the file has WebApp=true or is from a browser profile.
#[cfg(target_os = "linux")]
fn parse_pwa_desktop_file(path: &std::path::Path) -> Option<AppEntry> {
    let content = std::fs::read_to_string(path).ok()?;
    if !content.contains("[Desktop Entry]") { return None; }

    let mut is_webapp = false;
    let mut name = String::new();
    let mut exec = String::new();
    let mut icon = String::new();

    for line in content.lines() {
        if let Some(val) = line.strip_prefix("Name=") {
            name = val.to_string();
        } else if let Some(val) = line.strip_prefix("Exec=") {
            exec = val.to_string();
        } else if let Some(val) = line.strip_prefix("Icon=") {
            icon = val.to_string();
        } else if line.starts_with("Type=Application") {
            // Check for WebApp indicator
        } else if line.contains("WebApp=true") || line.contains("StartupWMClass=") {
            is_webapp = true;
        }
    }

    // Only include if it looks like a PWA
    if !is_webapp || name.is_empty() || exec.is_empty() { return None; }

    let _ = icon; // icon not used yet
    let search_names = build_search_names(&name);
    Some(AppEntry {
        display_name: name,
        search_names,
        launch: LaunchMethod::DesktopExec { exec },
        use_count: 0,
        last_used: 0,
    })
}

/// Use `flatpak list` to find installed Flatpak apps.
/// These may not have .desktop files in the standard XDG directories.
#[cfg(target_os = "linux")]
fn discover_flatpak_apps(entries: &mut Vec<AppEntry>) {
    use std::process::Command;

    let output = Command::new("flatpak")
        .args(["list", "--columns=application,name"])
        .output();

    let stdout = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return, // flatpak not installed
    };

    let existing: std::collections::HashSet<String> = entries
        .iter()
        .map(|e| e.display_name.to_lowercase())
        .collect();

    let mut count = 0;
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() < 2 { continue; }
        let app_id = parts[0];
        let name = parts[1];
        if name.is_empty() || existing.contains(&name.to_lowercase()) { continue; }

        // Launch via `flatpak run <app_id>`
        let exec = format!("flatpak run {}", app_id);
        let search_names = build_search_names(name);
        entries.push(AppEntry {
            display_name: name.to_string(),
            search_names,
            launch: LaunchMethod::DesktopExec { exec },
            use_count: 0,
            last_used: 0,
        });
        count += 1;
    }
    if count > 0 {
        tracing::debug!("discovered {} additional apps from flatpak", count);
    }
}

#[cfg(target_os = "linux")]
fn parse_desktop_file(path: &std::path::Path) -> Option<AppEntry> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut name = String::new();
    let mut exec = String::new();
    let mut no_display = false;
    let mut in_desktop_entry = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[Desktop Entry]" {
            in_desktop_entry = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_desktop_entry = false;
            continue;
        }
        if !in_desktop_entry { continue; }

        if let Some(val) = trimmed.strip_prefix("Name=") {
            if name.is_empty() { name = val.to_string(); }
        } else if let Some(val) = trimmed.strip_prefix("Exec=") {
            exec = val.to_string();
        } else if trimmed.starts_with("NoDisplay=true") {
            no_display = true;
        }
    }

    if name.is_empty() || exec.is_empty() || no_display {
        return None;
    }

    // Clean exec line: remove field codes like %u, %U, %f, %F
    let clean_exec = exec
        .split_whitespace()
        .filter(|w| !w.starts_with('%'))
        .collect::<Vec<_>>()
        .join(" ");

    let search_names = build_search_names(&name);
    Some(AppEntry {
        display_name: name,
        search_names,
        launch: LaunchMethod::DesktopExec { exec: clean_exec },
        use_count: 0,
        last_used: 0,
    })
}

// ─── URL fallbacks (all platforms) ─────────────────────────────────────────

fn add_url_fallbacks(entries: &mut Vec<AppEntry>) {
    let urls: &[(&str, &str, &[&str])] = &[
        ("Gmail", "https://mail.google.com", &["gmail", "google mail"]),
        ("YouTube", "https://www.youtube.com", &["youtube", "you tube"]),
        ("GitHub", "https://github.com", &["github", "git hub"]),
        ("Twitter", "https://twitter.com", &["twitter"]),
        ("X", "https://x.com", &["x"]),
        ("Facebook", "https://facebook.com", &["facebook"]),
        ("Instagram", "https://instagram.com", &["instagram"]),
        ("Reddit", "https://reddit.com", &["reddit"]),
        ("LinkedIn", "https://linkedin.com", &["linkedin"]),
        ("WhatsApp", "https://web.whatsapp.com", &["whatsapp", "whatsapp web"]),
        ("Spotify", "https://open.spotify.com", &["spotify"]),
        ("Netflix", "https://netflix.com", &["netflix"]),
        ("Amazon", "https://amazon.com", &["amazon"]),
        ("Google Drive", "https://drive.google.com", &["google drive", "drive", "my drive"]),
        ("Google Docs", "https://docs.google.com", &["google docs", "docs"]),
        ("Google Sheets", "https://sheets.google.com", &["google sheets", "sheets"]),
        ("Google Slides", "https://slides.google.com", &["google slides", "slides"]),
        ("Google Maps", "https://maps.google.com", &["google maps", "maps"]),
        ("Google Calendar", "https://calendar.google.com", &["google calendar", "calendar"]),
        ("Google Translate", "https://translate.google.com", &["google translate", "translate"]),
        ("Google Photos", "https://photos.google.com", &["google photos", "photos"]),
        ("Google News", "https://news.google.com", &["google news"]),
        ("Google Meet", "https://meet.google.com", &["google meet"]),
        ("Google Chat", "https://chat.google.com", &["google chat", "chat"]),
        ("ChatGPT", "https://chat.openai.com", &["chatgpt", "chat gpt", "openai", "open ai"]),
        ("Claude", "https://claude.ai", &["claude"]),
        ("Gemini", "https://gemini.google.com", &["gemini", "google gemini"]),
        ("Figma", "https://figma.com", &["figma"]),
        ("Notion", "https://notion.so", &["notion"]),
        ("Slack", "https://slack.com", &["slack"]),
        ("Discord", "https://discord.com/app", &["discord"]),
        ("Twitch", "https://twitch.tv", &["twitch"]),
        ("Stack Overflow", "https://stackoverflow.com", &["stack overflow", "stackoverflow"]),
        ("Wikipedia", "https://wikipedia.org", &["wikipedia"]),
    ];

    for (name, url, search) in urls {
        // Only add URL fallback if no native app was found with this name
        let already_exists = entries.iter().any(|e| {
            e.search_names.iter().any(|s| search.contains(&s.as_str()))
        });
        if already_exists { continue; }

        entries.push(AppEntry {
            display_name: name.to_string(),
            search_names: search.iter().map(|s| s.to_string()).collect(),
            launch: LaunchMethod::Url { url: url.to_string() },
            use_count: 0,
            last_used: 0,
        });
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Build normalized search names from a display name.
/// "Google Chrome" → ["google chrome", "chrome", "googlechrome"]
/// Generic words that must never become registry lookup keys.
/// A long-titled app (e.g. a PWA like "Dribbble - Discover the World's Top
/// Designers…") contributes each word as a key; without this filter the word
/// "the" becomes a key owned by whichever app was scanned last, and then
/// every query containing "the" ("open the chrome") resolves to that app.
fn is_stopword(word: &str) -> bool {
    matches!(
        word,
        "the" | "and" | "for" | "with" | "from" | "top" | "app" | "new" | "all"
            | "web" | "free" | "pro" | "my" | "your" | "our" | "discover"
    )
}

fn build_search_names(display_name: &str) -> Vec<String> {
    let lower = display_name.to_lowercase();
    let mut names = vec![lower.clone()];

    // Add individual words (for "Google Chrome" → "chrome"), skipping
    // stop-words — generic words ("the", "top", "app") as lookup keys let
    // one long-titled app (e.g. a PWA) hijack unrelated queries through
    // last-write-wins key ownership + substring matching.
    let words: Vec<&str> = lower.split_whitespace().collect();
    if words.len() > 1 {
        for word in &words {
            if word.len() >= 3 && !is_stopword(word) {
                names.push(word.to_string());
            }
        }
    }

    // Add concatenated version ("google chrome" → "googlechrome")
    let no_spaces = lower.replace(' ', "");
    if no_spaces != lower {
        names.push(no_spaces);
    }

    // Remove dashes ("vs-code" → "vscode")
    let no_dashes = lower.replace('-', "");
    if no_dashes != lower && !names.contains(&no_dashes) {
        names.push(no_dashes);
    }

    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopwords_never_become_search_names() {
        // Regression: the word "the" from a long PWA title
        // ("Dribbble - Discover the World's Top Designers…") used to become
        // a registry key owned by last-write-wins, hijacking every query
        // containing "the" ("open the chrome" → Dribbble).
        let names = build_search_names(
            "Dribbble - Discover the World's Top Designers & Creative Professionals",
        );
        for stop in ["the", "top", "discover"] {
            assert!(
                !names.contains(&stop.to_string()),
                "stop-word '{stop}' must not be a search name"
            );
        }
        // Real words still indexed.
        for keep in ["dribbble", "designers", "creative"] {
            assert!(
                names.contains(&keep.to_string()),
                "'{keep}' must stay a search name"
            );
        }
    }

    #[test]
    fn stopword_predicate() {
        assert!(is_stopword("the"));
        assert!(is_stopword("app"));
        assert!(!is_stopword("chrome"));
        assert!(!is_stopword("creative"));
    }
}
