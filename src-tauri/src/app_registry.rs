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

/// Linux platform layer: `.desktop` discovery, EWMH focus and the process
/// table. Split out so the shared registry stays platform neutral.
///
/// `#[path]` is needed because a child of a non-`mod.rs` module is looked up
/// under `src/app_registry/` — the same idiom `screen.rs` uses for
/// `screen_linux.rs`.
#[cfg(target_os = "linux")]
#[path = "app_registry_linux.rs"]
pub(crate) mod app_registry_linux;

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
pub(crate) struct ResolutionEntry {
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

    // Self-heal data poisoned by the old fuzzy launches (inflated installer
    // use_counts, unrelated phrases pinned to an installer). Idempotent: a
    // clean registry changes nothing. The resolution file is backed up once
    // before anything is dropped.
    {
        let mut cache = registry.cache.write();
        let mut res = registry.resolution.write();
        let (reset, dropped) = heal_poisoned_entries(&mut cache, &mut res);
        if reset > 0 || dropped > 0 {
            tracing::warn!(
                "app registry: healed {reset} inflated installer usage count(s) and dropped {dropped} poisoned resolution entr{}",
                if dropped == 1 { "y" } else { "ies" }
            );
            if dropped > 0 {
                let src = resolution_cache_path();
                let bak = src.with_file_name("app_resolution_cache.pre-heal.json");
                if src.exists() && !bak.exists() {
                    let _ = std::fs::copy(&src, &bak);
                }
                let snapshot: HashMap<String, ResolutionEntry> = res.clone();
                std::thread::spawn(move || save_resolution_cache(&snapshot));
            }
        }
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

/// Returns the number of cached app entries.
pub fn cached_app_count() -> usize {
    REGISTRY.cache.read().len()
}

/// Look up an app by name. Returns the best match or None.
/// This is the hot path — must be <1ms.
///
/// Resolution order:
///   1. Resolution cache (phrase → app, remembers user's previous choice) — O(1)
///   2. Exact match in app cache — O(1)
///   3. Prefix/contains/fuzzy match — O(n) but only on cache miss
pub fn lookup(query: &str) -> Option<AppEntry> {
    lookup_with_key(query).map(|(entry, _)| entry)
}

/// How a registry hit was found. Only `Resolved`, `Exact` and a well-covered
/// `Prefix` may launch without asking; `Contains` and `Fuzzy` are guesses
/// ("Did you mean …?"). Live misfire 2026-10-12: guesses launched an Intel
/// installer and Google Chat from garbled WhatsApp phrases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    /// A phrase the user (or a confirmation) previously mapped to this app.
    Resolved,
    /// The query IS a registry key.
    Exact,
    /// The query is a prefix of a key ("calc" -> "calculator"), or a key plus
    /// generic filler ("brave browser" -> "brave").
    Prefix,
    /// The query contains a key, or a key contains the query ("mummy chat" -> "chat").
    Contains,
    /// Within two edits of a key ("chroem" -> "chrome").
    Fuzzy,
}

/// Words that may trail an app key without changing which app is meant.
const GENERIC_SUFFIX_WORDS: &[&str] = &[
    "app", "application", "browser", "program", "software", "desktop", "client", "window", "windows",
];

/// True when `rest` (what follows a key inside a query) is only filler words.
fn only_generic_suffix(rest: &str) -> bool {
    let mut any = false;
    for w in rest.split_whitespace() {
        if !GENERIC_SUFFIX_WORDS.contains(&w) {
            return false;
        }
        any = true;
    }
    any
}

/// Same as [`lookup`], but also returns the registry key that matched,
/// so callers can measure match confidence (see [`assess_open_target`]).
pub fn lookup_with_key(query: &str) -> Option<(AppEntry, String)> {
    lookup_with_kind(query).map(|(entry, key, _)| (entry, key))
}

/// Tiered lookup that also reports HOW the app was found ([`MatchKind`]).
pub fn lookup_with_kind(query: &str) -> Option<(AppEntry, String, MatchKind)> {
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
                    return Some((entry.clone(), resolved.matched_name.clone(), MatchKind::Resolved));
                }
            }
            // Fallback: try the matched_name
            if let Some(entry) = cache.get(&resolved.matched_name) {
                return Some((entry.clone(), resolved.matched_name.clone(), MatchKind::Resolved));
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
    lookup_in(&cache, &q)
}

/// Strict lookup for callers that REWRITE the spoken text (the intent parser):
/// only hits that are safe to treat as "the user named this app" — a
/// previously resolved phrase, an exact key, or a well-covered prefix.
/// Guesses return None so the spoken words survive to the launch gate, which
/// asks instead of launching.
pub fn lookup_strict(query: &str) -> Option<AppEntry> {
    let (entry, key, kind) = lookup_with_kind(query)?;
    match kind {
        MatchKind::Resolved | MatchKind::Exact => Some(entry),
        MatchKind::Prefix if match_confidence(query, &key) >= 0.5 => Some(entry),
        _ => None,
    }
}

/// Pure tiered lookup over an app cache (unit-tested). The resolution cache
/// is consulted by the caller first.
fn lookup_in(
    cache: &HashMap<String, AppEntry>,
    q: &str,
) -> Option<(AppEntry, String, MatchKind)> {
    // 2. Exact match (O(1) HashMap lookup)
    if let Some(entry) = cache.get(q) {
        return Some((entry.clone(), q.to_string(), MatchKind::Exact));
    }
    let qlen = q.chars().count();

    // 3. Prefix match — "calc" matches "calculator"; "brave browser" matches
    // "brave" (key + generic filler only).
    let mut best: Option<(&AppEntry, String, usize)> = None;
    for (name, entry) in cache.iter() {
        let query_is_prefix = qlen >= 3 && name.starts_with(q);
        let key_plus_filler = name.chars().count() >= 4
            && q.starts_with(name.as_str())
            && only_generic_suffix(&q[name.len()..]);
        if query_is_prefix || key_plus_filler {
            let score = score_match(q, name, entry);
            if best.is_none() || score > best.as_ref().unwrap().2 {
                best = Some((entry, name.clone(), score));
            }
        }
    }
    if let Some((e, k, _)) = best {
        return Some((e.clone(), k, MatchKind::Prefix));
    }

    // 4. Contains match — "chrome" in "google chrome", or a key inside a longer
    // phrase. This tier is a GUESS. Keys shorter than 4 chars never take part:
    // a 3-letter key like "the"/"top" would otherwise match almost any query
    // (last-write-wins key ownership).
    for (name, entry) in cache.iter() {
        if name.chars().count() < 4 {
            continue;
        }
        if (qlen >= 3 && name.contains(q)) || q.contains(name.as_str()) {
            let score = score_match(q, name, entry);
            if best.is_none() || score > best.as_ref().unwrap().2 {
                best = Some((entry, name.clone(), score));
            }
        }
    }
    if let Some((e, k, _)) = best {
        return Some((e.clone(), k, MatchKind::Contains));
    }

    // 5. Levenshtein fuzzy match (typos: "chroem" -> "chrome"). A GUESS.
    if qlen >= 4 {
        for (name, entry) in cache.iter() {
            let dist = levenshtein(q, name);
            if dist <= 2 && dist < qlen / 2 {
                let score = score_match(q, name, entry);
                if best.is_none() || score > best.as_ref().unwrap().2 {
                    best = Some((entry, name.clone(), score));
                }
            }
        }
    }
    best.map(|(e, k, _)| (e.clone(), k, MatchKind::Fuzzy))
}

/// Stopwords ignored when measuring query coverage (a leading "the"
/// must not dilute "the brave" → brave, nor rescue "the brief").
const QUERY_STOPWORDS: &[&str] = &["the", "a", "an"];

/// Words marking installer-class software. Matches against BOTH the query
/// and the matched app name (see [`assess_open_target`]).
const INSTALLER_WORDS: &[&str] = &[
    "install",
    "installer",
    "setup",
    "update",
    "uninstall",
    "patch",
];

/// Query-coverage confidence of a registry match: fraction of the query's
/// content tokens covered by the matched key. 1.0 = the query names the
/// app outright ("brave" → brave); 0.0 = no overlap ("the brief" →
/// anything). Pure (unit-tested).
pub fn match_confidence(query: &str, matched_name: &str) -> f32 {
    // Owned buffers first: token slices borrow these, not temporaries.
    let query_lower = query.to_lowercase();
    let tokens: Vec<&str> = query_lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty() && !QUERY_STOPWORDS.contains(s))
        .collect();
    if tokens.is_empty() {
        return 0.0;
    }
    let name_lower = matched_name.to_lowercase();
    let name_tokens: Vec<&str> = name_lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    let mut hit = 0usize;
    for t in tokens.iter() {
        let t: &str = t;
        for n in name_tokens.iter() {
            let n: &str = n;
            if t == n || t.contains(n) || n.contains(t) {
                hit += 1;
                break;
            }
        }
    }
    hit as f32 / tokens.len() as f32
}

/// True when an app name looks like installer-class software.
/// Prefix-aware ("uninstaller" matches "uninstall") — installer words
/// essentially never appear in everyday app names.
pub fn is_installer_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    INSTALLER_WORDS.iter().any(|w| {
        lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|tok| tok == *w || tok.starts_with(*w))
    })
}

/// Verdict for opening an app by spoken phrase.
pub enum OpenAssessment {
    /// Confident match — execute normally.
    Proceed(AppEntry),
    /// No registry match at all — caller falls back to legacy resolvers.
    ProceedLegacy,
    /// A guess (contains / fuzzy / low coverage): speak `message` and arm an
    /// "open this app?" offer for `entry`. Nothing is launched.
    Confirm { entry: AppEntry, message: String },
    /// Unconfirmed installer — caller must speak this message and execute
    /// NOTHING (no launch, no Win-search drill).
    Refuse { message: String },
}

/// True when an exe path looks like installer/setup software even though its
/// Start-menu display name is friendly (the live "Intel® Driver & Support
/// Assistant" entry launches `...\Package Cache\{..}\...-Installer.exe`).
pub(crate) fn path_looks_like_installer(path: &str) -> bool {
    let lower = path.to_lowercase().replace('\\', "/");
    if lower.contains("/package cache/") || lower.contains("/$patchcache$/") || lower.contains("/installer/") {
        return true;
    }
    let file = lower.rsplit('/').next().unwrap_or("");
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
    is_installer_name(stem)
}

pub(crate) fn launch_looks_like_installer(entry: &AppEntry) -> bool {
    match &entry.launch {
        LaunchMethod::Exe { path } | LaunchMethod::AppPath { path } => path_looks_like_installer(path),
        _ => false,
    }
}

/// Gate every spoken app-open through match-kind, confidence and installer
/// rules. Live ghost misfires: 2026-10-07 "open the brief" fuzzy-matched an
/// Intel installer; 2026-10-12 garbled "open my chat with mummy in whatsapp"
/// launched the Intel Driver & Support Assistant installer twice and Google
/// Chat once, each followed by "Ok sir.". Rules:
/// - only Resolved / Exact / well-covered Prefix matches proceed;
/// - Contains / Fuzzy matches and low coverage are guesses: ask, never launch;
/// - installer-class match (display name OR exe path) without installer words
///   in the query itself ("open intel software installer" is explicit) → refuse;
/// - registry miss → ProceedLegacy (legacy resolvers keep working).
/// Pure decision apart from the registry read itself (unit-tested).
pub fn assess_open_target(query: &str) -> OpenAssessment {
    let Some((entry, matched, kind)) = lookup_with_kind(query) else {
        return OpenAssessment::ProceedLegacy;
    };
    let query_has_installer_word = query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|tok| INSTALLER_WORDS.contains(&tok));
    if (is_installer_name(&entry.display_name) || launch_looks_like_installer(&entry))
        && !query_has_installer_word
    {
        tracing::warn!(
            "app-match: installer '{}' without explicit installer words in '{}', refusing",
            entry.display_name,
            query
        );
        return OpenAssessment::Refuse {
            message: format!(
                "That's '{}', which looks like an installer — say the full name to confirm.",
                entry.display_name
            ),
        };
    }
    let confidence = match_confidence(query, &matched);
    let confident = match kind {
        MatchKind::Resolved | MatchKind::Exact => true,
        MatchKind::Prefix => confidence >= 0.5,
        MatchKind::Contains | MatchKind::Fuzzy => false,
    };
    if !confident {
        tracing::warn!(
            "app-match: guess ({:?}, coverage {:.2}) '{}' → '{}', asking first",
            kind,
            confidence,
            query,
            entry.display_name
        );
        let message = format!(
            "I couldn't find '{}' exactly — did you mean {}?",
            query.trim(),
            entry.display_name
        );
        return OpenAssessment::Confirm { entry, message };
    }
    OpenAssessment::Proceed(entry)
}

/// Remember that the user confirmed `spoken` means `display_name`, so the
/// next time it resolves directly (a `Resolved` hit). Only called after an
/// explicit "yes" — never from an unconfirmed fuzzy match.
pub fn confirm_resolution(spoken: &str, display_name: &str) {
    let q = spoken.trim().to_lowercase();
    if q.is_empty() {
        return;
    }
    let registry = &*REGISTRY;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let snapshot = {
        let mut res = registry.resolution.write();
        let e = res.entry(q.clone()).or_insert_with(|| ResolutionEntry {
            display_name: display_name.to_string(),
            matched_name: q.clone(),
            use_count: 0,
            last_used: 0,
        });
        e.display_name = display_name.to_string();
        e.use_count += 1;
        e.last_used = now;
        res.clone()
    };
    std::thread::spawn(move || save_resolution_cache(&snapshot));
}

/// One-time self-healing of data poisoned by the old fuzzy launches:
/// - installer-class apps (by name or exe path) get `use_count` reset to 0
///   (it had been inflated to 40 by wrong launches, which then won ties);
/// - resolution entries that pin an unrelated phrase to an installer-class
///   app are dropped (a phrase unrelated to the app by name and not one of
///   its own keys).
/// Legit user aliases for ordinary apps are never touched. Returns
/// (usage counts reset, resolution entries dropped).
pub(crate) fn heal_poisoned_entries(
    cache: &mut HashMap<String, AppEntry>,
    res: &mut HashMap<String, ResolutionEntry>,
) -> (usize, usize) {
    let mut reset = 0;
    for e in cache.values_mut() {
        if e.use_count > 0 && (is_installer_name(&e.display_name) || launch_looks_like_installer(e)) {
            e.use_count = 0;
            reset += 1;
        }
    }
    let installer_displays: std::collections::HashSet<String> = cache
        .values()
        .filter(|e| is_installer_name(&e.display_name) || launch_looks_like_installer(e))
        .map(|e| e.display_name.to_lowercase())
        .collect();
    let before = res.len();
    res.retain(|phrase, r| {
        let target = r.display_name.to_lowercase();
        if !installer_displays.contains(&target) {
            return true;
        }
        let is_own_key = cache.values().any(|e| {
            e.display_name.to_lowercase() == target && e.search_names.iter().any(|n| n == phrase)
        });
        is_own_key || match_confidence(phrase, &target) >= 0.5
    });
    (reset, before - res.len())
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

/// What happened when NEXUS tried to focus an already-running app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusOutcome {
    /// A window was found and actually raised.
    Focused,
    /// The app's process is running, but its window is not mapped yet. The
    /// caller should wait for the window — not start a second copy.
    WindowNotReady,
    /// The app IS running, but this session cannot raise its window. The
    /// string is a speakable explanation for the user.
    RunningCannotFocus(String),
    /// Nothing matching is open.
    NotRunning,
}

/// Check if an app is already running and focus its window.
///
/// Returns what happened, so the caller can tell "focused" from "the process
/// is up but its window is not mapped yet" from "running but this session
/// cannot raise it" from "not running". This is the FIRST priority in the
/// launch flow.
///
/// The distinction matters on Linux. A native Wayland session has no way to
/// raise another app's window, but the app may still be running: reporting
/// "not running" would make the caller launch a second copy of an app the
/// user already has open, and reporting "focused" would be a lie the user
/// sees as a window that never came up.
pub fn focus_existing(entry: &AppEntry) -> FocusOutcome {
    // The cached list is refreshed only periodically, so a window opened a moment ago was invisible
    // here and every repeat of "open notepad" launched ANOTHER one (7 windows in a live test).
    // A live scan costs a few ms and is the only honest answer to "is it already open?".
    refresh_window_cache();

    // Linux: the process table is read ONCE per call and every question
    // below asks that one snapshot. A cached table would report "not
    // running" for an app that started a moment ago, and the caller would
    // launch a second copy of it. Only desktop entries have a process to
    // look for, so a URL entry skips the scan entirely.
    #[cfg(target_os = "linux")]
    let snapshot = if linux_process_names(entry).is_empty() {
        None
    } else {
        Some(app_registry_linux::ProcessSnapshot::capture())
    };

    // Linux first: the window that belongs to the app's running process is
    // the exact match. The title rarely equals the `.desktop` name ("Text
    // Editor" vs "gedit", "Firefox" vs "Mozilla Firefox"), so the PID is
    // what identifies the window.
    #[cfg(target_os = "linux")]
    {
        if let Some(outcome) = linux_focus_running_window(entry, snapshot.as_ref()) {
            return outcome;
        }
    }

    {
        let cache = WINDOW_CACHE.read();
        let own_pid = std::process::id();

        // Process identity first (most reliable), then title identity. Both
        // require a real name — a generic word like "chat"/"assistant" never
        // identifies a window (it focused a ChatGPT window and NEXUS's own stage).
        for pass_title in [false, true] {
            for w in cache.iter() {
                if !window_matches_entry(&w.title, &w.process_name, entry, w.pid == own_pid) {
                    continue;
                }
                let by_process = process_identity_matches(&w.process_name, entry);
                if by_process == pass_title {
                    continue; // handled in the other pass
                }
                tracing::info!(
                    "focus hit: '{}' matched window '{}' / process '{}' (pid={})",
                    entry.display_name,
                    w.title,
                    w.process_name,
                    w.pid
                );
                if focus_window(w.hwnd) {
                    return FocusOutcome::Focused;
                }
                return FocusOutcome::RunningCannotFocus(cannot_focus_reason());
            }
        }
    }

    // The app's process is up, but no window of it could be raised.
    #[cfg(target_os = "linux")]
    {
        if let Some(outcome) = linux_window_not_ready(entry, snapshot.as_ref()) {
            return outcome;
        }
    }

    // Nothing matching is open: the caller launches the app.
    #[cfg(target_os = "linux")]
    let outcome = app_registry_linux::focus_outcome(
        app_registry_linux::FocusFacts::NotRunning,
        false,
        None,
    );
    #[cfg(not(target_os = "linux"))]
    let outcome = FocusOutcome::NotRunning;
    outcome
}

/// Linux: the process names the app can run under.
#[cfg(target_os = "linux")]
fn linux_process_names(entry: &AppEntry) -> Vec<String> {
    match entry_exec(entry) {
        Some(exec) => app_registry_linux::exec_process_names(exec),
        None => Vec::new(),
    }
}

/// Linux: raise the window that belongs to the app's running process.
///
/// `None` when the app is not running, or when it runs without a window of
/// its own — the caller then tries the name-based paths.
#[cfg(target_os = "linux")]
fn linux_focus_running_window(
    entry: &AppEntry,
    snapshot: Option<&app_registry_linux::ProcessSnapshot>,
) -> Option<FocusOutcome> {
    let names = linux_process_names(entry);
    if names.is_empty() {
        return None;
    }
    let snapshot = snapshot?;
    // The SAME rule as "is it running?" — an app with no process of its own
    // has no window to raise either.
    let pids = app_registry_linux::app_pids(snapshot, &names)?;
    let windows: Vec<(u64, u32)> = WINDOW_CACHE
        .read()
        .iter()
        .map(|w| (w.hwnd.max(0) as u64, w.pid))
        .collect();
    let id = app_registry_linux::window_id_for_pids(&windows, &pids)?;
    if focus_window(id as isize) {
        return Some(FocusOutcome::Focused);
    }
    let blocked = app_registry_linux::focus_blocked_reason_now();
    Some(app_registry_linux::focus_outcome(
        app_registry_linux::FocusFacts::WindowFound,
        false,
        blocked.as_deref(),
    ))
}

/// Linux: the app runs, but no window of it could be raised.
///
/// The window may simply not be mapped yet — the app is still starting — so
/// the caller waits for it instead of starting a second copy.
#[cfg(target_os = "linux")]
fn linux_window_not_ready(
    entry: &AppEntry,
    snapshot: Option<&app_registry_linux::ProcessSnapshot>,
) -> Option<FocusOutcome> {
    let names = linux_process_names(entry);
    if names.is_empty() {
        return None;
    }
    let snapshot = snapshot?;
    // The SAME rule as the window pick above: only the app's own process
    // counts. A different product with a similar name ("code-insiders" for
    // "code") must not make the app look like it is running — the caller
    // would then wait for a window that will never come instead of
    // launching the app the user asked for.
    let _pids = app_registry_linux::app_pids(snapshot, &names)?;
    // Last chance to raise it: the display-name regex.
    if app_registry_linux::focus_by_title(&entry.display_name) {
        return Some(FocusOutcome::Focused);
    }
    let blocked = app_registry_linux::focus_blocked_reason_now();
    Some(app_registry_linux::focus_outcome(
        app_registry_linux::FocusFacts::ProcessOnly,
        false,
        blocked.as_deref(),
    ))
}

/// The command line of a desktop entry, if that is how the app is launched.
#[cfg(target_os = "linux")]
fn entry_exec(entry: &AppEntry) -> Option<&str> {
    match &entry.launch {
        LaunchMethod::DesktopExec { exec } => Some(exec.as_str()),
        _ => None,
    }
}

/// Speakable explanation for why an already-running app could not be raised.
#[cfg(target_os = "linux")]
fn cannot_focus_reason() -> String {
    app_registry_linux::blocked_reason_message(
        app_registry_linux::focus_blocked_reason_now().as_deref(),
    )
}

/// Speakable explanation for why an already-running app could not be raised.
#[cfg(not(target_os = "linux"))]
fn cannot_focus_reason() -> String {
    "I couldn't raise that window, sir.".to_string()
}

/// How long a launch is considered "still starting": a Store app or a slow exe can take seconds to
/// show its first window, and a repeated command inside that time must not launch it again.
pub const LAUNCH_SETTLE: std::time::Duration = std::time::Duration::from_secs(8);

static RECENT_LAUNCH: Lazy<parking_lot::Mutex<std::collections::HashMap<String, std::time::Instant>>> =
    Lazy::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

/// Remember that `entry` was just launched.
pub fn note_launched(entry: &AppEntry) {
    RECENT_LAUNCH.lock().insert(entry.display_name.to_lowercase(), std::time::Instant::now());
}

/// True when `entry` was launched less than `LAUNCH_SETTLE` ago.
pub fn recently_launched(entry: &AppEntry) -> bool {
    RECENT_LAUNCH
        .lock()
        .get(&entry.display_name.to_lowercase())
        .map_or(false, |t| t.elapsed() < LAUNCH_SETTLE)
}

/// Single words too generic to identify a window or an app on their own.
const GENERIC_IDENTITY_WORDS: &[&str] = &[
    "chat", "support", "assistant", "driver", "drivers", "manager", "center", "centre", "studio",
    "browser", "helper", "service", "services", "update", "updater", "settings", "setup", "tools",
    "tool", "player", "viewer", "editor", "console", "terminal", "launcher", "client", "desktop",
    "cloud", "online", "mobile", "messenger", "store", "music", "video", "photos", "camera",
    "notes", "clock", "file", "files", "web", "mail", "security", "system", "device", "devices",
    "free", "pro", "lite", "new", "code",
];

fn process_stem(process_name: &str) -> String {
    let lower = process_name.to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

/// The window's process IS this app: its exe name equals one of the app's
/// keys (spaces ignored: "vs code" ↔ "vscode"/"code" is NOT assumed, only equality).
fn process_identity_matches(process_name: &str, entry: &AppEntry) -> bool {
    let stem = process_stem(process_name);
    if stem.is_empty() {
        return false;
    }
    entry.search_names.iter().any(|t| {
        let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect();
        t == &stem || compact == stem
    })
}

/// Pure: does this window belong to `entry`? (`is_own_window` = it belongs to
/// the NEXUS process itself and is never a candidate.)
pub(crate) fn window_matches_entry(
    title: &str,
    process_name: &str,
    entry: &AppEntry,
    is_own_window: bool,
) -> bool {
    if is_own_window {
        return false;
    }
    if process_identity_matches(process_name, entry) {
        return true;
    }
    let title_lower = title.to_lowercase();
    if title_lower.is_empty() {
        return false;
    }
    // The full display name appears in the title.
    let display_lower = entry.display_name.to_lowercase();
    if display_lower.chars().count() >= 4 && title_lower.contains(&display_lower) {
        return true;
    }
    let words: Vec<&str> = title_lower
        .split(|c: char| !(c.is_alphanumeric() || c == '®'))
        .filter(|w| !w.is_empty())
        .collect();
    entry.search_names.iter().any(|term| {
        let term_words: Vec<&str> = term.split_whitespace().collect();
        if term_words.len() >= 2 {
            // A multi-word key ("google chat") is identity on its own.
            return title_lower.contains(term.as_str());
        }
        // A single word counts only if it is long, distinctive and whole-word.
        term.chars().count() >= 6
            && !GENERIC_IDENTITY_WORDS.contains(&term.as_str())
            && words.iter().any(|w| w == &term.as_str())
    })
}

/// Focus a window: restore if minimized, bring to foreground.
/// Uses AttachThreadInput trick to bypass Windows' focus-stealing prevention
/// (NEXUS is a background process, so SetForegroundWindow would normally be rejected).
///
/// Returns whether the window was actually raised. The previous signature
/// made "did nothing" indistinguishable from "succeeded": `try_focus_existing`
/// returned `true` unconditionally on every platform, so on Linux the app
/// told the user "Ok sir" after a no-op.
fn focus_window(hwnd_val: isize) -> bool {
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
        true
    }

    // Linux/X11 (including XWayland): EWMH lets any client raise and focus a
    // window, so this genuinely works — as long as `wmctrl` is installed.
    // Native Wayland has no EWMH at all, and the call fails honestly there.
    #[cfg(target_os = "linux")]
    {
        app_registry_linux::focus_window_id(hwnd_val)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = hwnd_val;
        false
    }
}

/// Focus a window by (partial) title on Linux/X11.
///
/// Separate from [`focus_window`] because that one takes a native handle from
/// the `wmctrl`-populated cache, whereas callers here only have a display name.
/// Both go through EWMH; the title form is what `command_executor` needs when it
/// has detected a running process but not its window id.
#[cfg(target_os = "linux")]
pub fn focus_by_title(partial_title: &str) -> bool {
    app_registry_linux::focus_by_title(partial_title)
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
        // X11 only: `wmctrl -l -p` lists the X11 windows. On native Wayland
        // there is no such list, so the cache stays empty and the focus path
        // falls back to the process table (which does work on Wayland).
        //
        // The ids in that output are hexadecimal (`0x03e00006`) and the
        // columns are padded with runs of spaces — reading them as decimal
        // and splitting on single spaces is what made the cache empty and
        // every focus attempt fail.
        let windows: Vec<CachedWindow> = app_registry_linux::list_windows()
            .into_iter()
            .map(|w| CachedWindow {
                hwnd: w.id as isize,
                pid: w.pid,
                title: w.title,
                process_name: String::new(),
            })
            .collect();
        let count = windows.len();
        *WINDOW_CACHE.write() = windows;
        tracing::trace!("window cache refreshed: {} windows", count);
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
    // Operate on chars throughout. The old version took `len()` (bytes) for
    // the matrix width but iterated `chars()`, so a key containing `®` (2
    // bytes) read an unwritten column and returned 0 for odd-length queries.
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let m = a.len();
    let n = b.len();
    if m == 0 { return n; }
    if n == 0 { return m; }

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0; n + 1];

    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
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
//
// The Linux implementation lives in `app_registry_linux.rs`: XDG data dirs
// plus the Flatpak and Snap exports, with the desktop-entry filters
// (`Hidden`, `NoDisplay`, `OnlyShowIn`, `NotShowIn`, `TryExec`) honoured.

#[cfg(target_os = "linux")]
fn discover_linux(entries: &mut Vec<AppEntry>) {
    app_registry_linux::discover(entries);
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

/// Test-only: install a known app list into the process-global registry.
/// The registry is global state, so installs are serialised by a lock held
/// for the lifetime of the returned guard (dropping it empties the registry).
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static LOCK: Mutex<()> = Mutex::new(());

    pub(crate) struct Installed(#[allow(dead_code)] MutexGuard<'static, ()>);

    pub(crate) fn install(entries: Vec<AppEntry>) -> Installed {
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut cache = REGISTRY.cache.write();
        cache.clear();
        for e in entries {
            for n in e.search_names.clone() {
                cache.insert(n, e.clone());
            }
        }
        REGISTRY.resolution.write().clear();
        Installed(guard)
    }

    impl Drop for Installed {
        fn drop(&mut self) {
            REGISTRY.cache.write().clear();
            REGISTRY.resolution.write().clear();
        }
    }
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

    #[test]
    fn test_match_confidence_tiers() {
        // Exact and alias matches are full confidence.
        assert_eq!(match_confidence("brave", "brave"), 1.0);
        assert_eq!(match_confidence("vs code", "vs code"), 1.0);
        // Leading stopwords don't dilute ("the brave" → brave).
        assert_eq!(match_confidence("the brave", "brave"), 1.0);
        // Partial coverage passes the 0.5 floor ("brave browser").
        assert!(match_confidence("brave browser", "brave") >= 0.5);
        // The live misfire: "the brief" covers nothing installer-like.
        assert_eq!(match_confidence("the brief", "intel software installer"), 0.0);
        assert_eq!(match_confidence("the brief", "brave"), 0.0);
        // Empty-after-stopwords is zero, never NaN.
        assert_eq!(match_confidence("the", "brave"), 0.0);
        assert_eq!(match_confidence("", "brave"), 0.0);
    }

    #[test]
    fn test_is_installer_name() {
        for name in [
            "Intel Software Installer",
            "Node.js Setup",
            "Windows Update Assistant",
            "App Uninstaller",
            "Security Patch",
        ] {
            assert!(is_installer_name(name), "{name} should be installer-class");
        }
        for name in ["Brave", "Brave Browser", "Visual Studio Code", "Spotify", "Notepad"] {
            assert!(!is_installer_name(name), "{name} should not be installer-class");
        }
    }

    #[test]
    fn test_assess_gibberish_falls_to_legacy() {
        // No registry key can match a 24-char random token (contains,
        // prefix, and levenshtein gates all fail) — legacy chain keeps
        // working for true misses on every machine.
        match assess_open_target("zzzqqqxkcdnonexistentapp") {
            OpenAssessment::ProceedLegacy => {}
            _ => panic!("gibberish must fall through to legacy resolvers"),
        }
    }

    // ── Live ghost misfires 2026-10-12 (docs/changes/126) ─────────────────

    fn app(display: &str, names: &[&str], path: &str) -> AppEntry {
        AppEntry {
            display_name: display.to_string(),
            search_names: names.iter().map(|s| s.to_string()).collect(),
            launch: LaunchMethod::Exe { path: path.to_string() },
            use_count: 0,
            last_used: 0,
        }
    }

    /// A registry shaped like the user's real one: five `®` keys (the Intel
    /// installer family), a bare `chat` key (Google Chat), and normal apps.
    fn live_like_cache() -> HashMap<String, AppEntry> {
        let intel = app(
            "Intel® Driver & Support Assistant",
            &["intel® driver & support assistant", "intel® driver", "intel®"],
            r"C:\ProgramData\Package Cache\{abc}\Intel-Driver-and-Support-Assistant-Installer.exe",
        );
        let apps = vec![
            intel,
            app("Intel® Software Installer", &["intel® software installer"], r"C:\x\setup.exe"),
            app("Google Chat", &["google chat", "chat"], r"C:\x\chat.exe"),
            app("WhatsApp", &["whatsapp"], r"C:\x\whatsapp.exe"),
            app("Brave", &["brave", "brave browser"], r"C:\x\brave.exe"),
            app("Calculator", &["calculator"], r"C:\x\calc.exe"),
            app("Google Chrome", &["google chrome", "chrome"], r"C:\x\chrome.exe"),
        ];
        let mut m = HashMap::new();
        for a in apps {
            for n in &a.search_names {
                m.insert(n.clone(), a.clone());
            }
        }
        m
    }

    #[test]
    fn levenshtein_is_char_based_not_byte_based() {
        // The bug: lengths were taken in BYTES but columns iterated in CHARS,
        // so any key containing a multi-byte char (`®`) read garbage and a
        // 0-distance "match" for odd-length queries.
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("intel®", "intel®"), 0);
        assert_eq!(levenshtein("intel®", "intel"), 1);
        assert_eq!(levenshtein("intel®", "intelx"), 1);
        assert!(levenshtein("my chart with mommy and what so", "intel® driver & support assistant") > 10);
    }

    #[test]
    fn garbled_whatsapp_chat_phrases_do_not_match_the_intel_installer() {
        let c = live_like_cache();
        for q in [
            "my chart with mommy and what so", // 31 chars (odd)
            "mommychart",                       // even; its space variant is odd
            "m ommychart",
        ] {
            let hit = lookup_in(&c, q);
            assert!(
                hit.as_ref().map_or(true, |(e, _, _)| !e.display_name.contains('®')),
                "{q:?} must never reach an Intel® entry, got {:?}",
                hit.map(|(e, k, kind)| (e.display_name, k, kind))
            );
        }
    }

    #[test]
    fn exact_and_filler_matches_may_launch() {
        let c = live_like_cache();
        let kind = |q: &str| lookup_in(&c, q).map(|(e, _, k)| (e.display_name, k));
        assert_eq!(kind("brave"), Some(("Brave".into(), MatchKind::Exact)));
        assert_eq!(kind("whatsapp"), Some(("WhatsApp".into(), MatchKind::Exact)));
        // Key + generic filler word is still the same app.
        assert_eq!(kind("whatsapp app"), Some(("WhatsApp".into(), MatchKind::Prefix)));
        // Query is a prefix of the key ("calc" -> "calculator").
        assert_eq!(kind("calc"), Some(("Calculator".into(), MatchKind::Prefix)));
    }

    #[test]
    fn guesses_are_never_prefix_or_exact() {
        let c = live_like_cache();
        // "mummy chat" merely CONTAINS the generic key `chat`: a guess.
        let (e, _, kind) = lookup_in(&c, "mummy chat").expect("contains tier still finds it");
        assert_eq!(e.display_name, "Google Chat");
        assert_eq!(kind, MatchKind::Contains);
        // Key + non-generic words ("chat with mummy") is also only a guess.
        let (_, _, kind) = lookup_in(&c, "chat with mummy").expect("found");
        assert_ne!(kind, MatchKind::Prefix);
        assert_ne!(kind, MatchKind::Exact);
        // A typo is a guess too.
        let (e, _, kind) = lookup_in(&c, "chroem").expect("fuzzy tier");
        assert_eq!(e.display_name, "Google Chrome");
        assert_eq!(kind, MatchKind::Fuzzy);
    }

    #[test]
    fn exe_path_installers_are_detected_even_with_a_friendly_display_name() {
        // Display name is "Intel® Driver & Support Assistant" (no installer
        // word) but it launches ...\Package Cache\...-Installer.exe.
        let c = live_like_cache();
        let intel = c.get("intel®").unwrap();
        assert!(!is_installer_name(&intel.display_name));
        assert!(launch_looks_like_installer(intel));
        assert!(!launch_looks_like_installer(c.get("brave").unwrap()));
        assert!(path_looks_like_installer(r"C:\Users\x\AppData\Local\Temp\Node-Setup.exe"));
        assert!(path_looks_like_installer(r"C:\ProgramData\Package Cache\{1}\a.exe"));
        assert!(!path_looks_like_installer(r"C:\Program Files\Brave\brave.exe"));
    }

    #[test]
    fn window_focus_needs_a_real_identity_not_a_generic_word() {
        let chat = app("Google Chat", &["google chat", "chat"], r"C:\x\chat.exe");
        // The live bug: "chat" matched a ChatGPT window.
        assert!(!window_matches_entry("ChatGPT Classic", "chatgpt.exe", &chat, false));
        // ...and our own window titled com.nexus.assistant-siw matched "assistant".
        let intel = app("Intel® Driver & Support Assistant", &["intel®", "driver", "support", "assistant"], r"C:\x\i.exe");
        assert!(!window_matches_entry("com.nexus.assistant-siw", "nexus.exe", &intel, true));
        assert!(!window_matches_entry("Assistant", "other.exe", &intel, false));
        // Real identities still focus.
        assert!(window_matches_entry("Google Chat - Brave", "brave.exe", &chat, false));
        let brave = app("Brave", &["brave", "brave browser"], r"C:\x\brave.exe");
        assert!(window_matches_entry("New Tab", "brave.exe", &brave, false));
        let yt = app("YouTube", &["youtube"], r"C:\x\yt.exe");
        assert!(window_matches_entry("YouTube - Brave", "brave.exe", &yt, false));
        let notepad = app("Notepad", &["notepad"], r"C:\x\notepad.exe");
        assert!(window_matches_entry("Untitled - Notepad", "notepad.exe", &notepad, false));
    }

    fn live_like_entries() -> Vec<AppEntry> {
        let mut seen = std::collections::HashSet::new();
        live_like_cache()
            .into_values()
            .filter(|e| seen.insert(e.display_name.clone()))
            .collect()
    }

    fn label(a: &OpenAssessment) -> &'static str {
        match a {
            OpenAssessment::Proceed(_) => "Proceed",
            OpenAssessment::ProceedLegacy => "ProceedLegacy",
            OpenAssessment::Confirm { .. } => "Confirm",
            OpenAssessment::Refuse { .. } => "Refuse",
        }
    }

    #[test]
    fn gate_asks_instead_of_launching_guesses() {
        let _g = test_support::install(live_like_entries());
        // The live misfire: "open mummy chat" launched Google Chat (+ "Ok sir.").
        match assess_open_target("mummy chat") {
            OpenAssessment::Confirm { entry, message } => {
                assert_eq!(entry.display_name, "Google Chat");
                assert!(message.contains("did you mean Google Chat"), "{message}");
            }
            other => panic!("expected Confirm, got {}", label(&other)),
        }
        // A typo is a guess too.
        assert!(matches!(assess_open_target("chroem"), OpenAssessment::Confirm { .. }));
        // Named apps still open at once.
        for q in ["whatsapp", "brave", "calculator", "calc", "brave browser"] {
            let a = assess_open_target(q);
            assert!(matches!(a, OpenAssessment::Proceed(_)), "{q} must proceed, got {}", label(&a));
        }
        // The Intel installer is refused by its exe path even for its friendly name,
        // and a garble that used to reach it now matches nothing (legacy path).
        assert!(matches!(assess_open_target("intel®"), OpenAssessment::Refuse { .. }));
        assert!(matches!(
            assess_open_target("my chart with mommy and what so"),
            OpenAssessment::ProceedLegacy
        ));
    }

    #[test]
    fn confirmed_phrase_resolves_directly_next_time() {
        let _g = test_support::install(live_like_entries());
        assert!(matches!(assess_open_target("mummy chat"), OpenAssessment::Confirm { .. }));
        confirm_resolution("mummy chat", "Google Chat");
        match lookup_with_kind("mummy chat") {
            Some((e, _, MatchKind::Resolved)) => assert_eq!(e.display_name, "Google Chat"),
            other => panic!("expected a Resolved hit, got {:?}", other.map(|(e, _, k)| (e.display_name, k))),
        }
        assert!(matches!(assess_open_target("mummy chat"), OpenAssessment::Proceed(_)));
    }

    #[test]
    fn heal_resets_installer_usage_and_drops_unrelated_resolutions() {
        let mut cache = live_like_cache();
        for e in cache.values_mut() {
            if e.display_name.contains('®') {
                e.use_count = 40;
            }
            if e.display_name == "Brave" {
                e.use_count = 7;
            }
        }
        let mk = |display: &str| ResolutionEntry {
            display_name: display.to_string(),
            matched_name: String::new(),
            use_count: 1,
            last_used: 1,
        };
        let mut res: HashMap<String, ResolutionEntry> = HashMap::new();
        // Poisoned: an unrelated phrase pinned to the installer.
        res.insert("my chart with mommy and what so".into(), mk("Intel® Driver & Support Assistant"));
        // Legit: a key of that app.
        res.insert("brave".into(), mk("Brave"));
        // Legit alias the user taught ("my browser" -> Brave has no word overlap, but Brave is not installer-class).
        res.insert("my browser".into(), mk("Brave"));
        let (reset, dropped) = heal_poisoned_entries(&mut cache, &mut res);
        assert!(reset >= 1, "installer usage counts reset");
        assert_eq!(dropped, 1);
        assert!(!res.contains_key("my chart with mommy and what so"));
        assert!(res.contains_key("brave"));
        assert!(res.contains_key("my browser"));
        assert!(cache.values().filter(|e| e.display_name.contains('®')).all(|e| e.use_count == 0));
        assert_eq!(cache.get("brave").unwrap().use_count, 7, "legit usage untouched");
    }

    #[test]
    fn a_just_launched_app_is_not_launched_again() {
        let e = app("ZzzDebounceProbe", &["zzzdebounceprobe"], "C:/x/a.exe");
        assert!(!recently_launched(&e));
        note_launched(&e);
        assert!(recently_launched(&e));
        let other = app("Other Thing", &["other thing"], "C:/x/b.exe");
        assert!(!recently_launched(&other));
    }
}
