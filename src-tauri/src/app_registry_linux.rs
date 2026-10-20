//! Linux platform layer for the app registry — `.desktop` discovery, launch
//! and window focus.
//!
//! Split out of `app_registry.rs` so the shared registry stays platform
//! neutral and every Linux-only rule lives in one place. Declared from the
//! parent module and compiled only on Linux:
//!
//! ```ignore
//! #[cfg(target_os = "linux")]
//! #[path = "app_registry_linux.rs"]
//! pub(crate) mod app_registry_linux;
//! ```
//!
//! The `#[path]` is required: a child of a non-`mod.rs` module is looked up
//! under `src/app_registry/`, not next to the parent file. `screen.rs` uses
//! the same idiom for `screen_linux.rs`.
//!
//! Three things the shared registry could not do on Linux:
//!
//!   1. **Discovery.** `.desktop` files live in the XDG data dirs plus the
//!      Flatpak and Snap export trees, and they carry per-desktop filters
//!      (`Hidden`, `NoDisplay`, `OnlyShowIn`, `NotShowIn`, `TryExec`) that the
//!      spec says must be honoured. An entry that only exists for another
//!      desktop environment is not an app the user can launch here.
//!   2. **Focus.** X11 has EWMH, so `wmctrl` can raise any window. Native
//!      Wayland has no equivalent — no EWMH, no `Shell.Eval` (disabled in
//!      GNOME 41), no get-active-window portal. The honest answer there is an
//!      error the caller speaks, never a silent "Ok sir".
//!   3. **"Is it already running?"** The window list does not exist on
//!      Wayland, so the process table answers that instead — which is what
//!      stops a second copy of an already-open app from being launched.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::{build_search_names, AppEntry, FocusOutcome, LaunchMethod};

// ─── Desktop entry model ───────────────────────────────────────────────────

/// A parsed `[Desktop Entry]` group.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopEntry {
    /// `Type=` — only `Application` entries can be launched.
    pub kind: String,
    /// `Name=` — the unlocalised name.
    pub name: String,
    /// `Exec=` — the command line, field codes still present.
    pub exec: String,
    /// `Hidden=true` — an internal application that is never shown.
    pub hidden: bool,
    /// `NoDisplay=true` — installed, but deliberately not in the menus.
    pub no_display: bool,
    /// `OnlyShowIn=` — the desktop ids this entry is limited to.
    pub only_show_in: Vec<String>,
    /// `NotShowIn=` — the desktop ids this entry is excluded from.
    pub not_show_in: Vec<String>,
    /// `TryExec=` — a binary that must exist for the entry to be valid.
    pub try_exec: Option<String>,
}

/// Parse the `[Desktop Entry]` group of a `.desktop` file.
///
/// Returns `None` when the file has no such group. Localised keys
/// (`Name[de]=`) are ignored — the unlocalised value is the one to use.
/// Duplicate keys follow the spec: the last one wins.
pub fn parse_desktop_entry(content: &str) -> Option<DesktopEntry> {
    let mut entry: Option<DesktopEntry> = None;
    let mut in_group = false;
    let mut saw_key = false;

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_group = line == "[Desktop Entry]";
            continue;
        }
        if !in_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || key.contains('[') {
            // A localised key such as `Name[de]`.
            continue;
        }
        let value = unescape_value(value.trim());
        let e = entry.get_or_insert_with(DesktopEntry::default);
        saw_key = true;
        match key {
            "Type" => e.kind = value,
            "Name" => e.name = value,
            "Exec" => e.exec = value,
            "Hidden" => e.hidden = value.eq_ignore_ascii_case("true"),
            "NoDisplay" => e.no_display = value.eq_ignore_ascii_case("true"),
            "OnlyShowIn" => e.only_show_in = split_desktop_list(&value),
            "NotShowIn" => e.not_show_in = split_desktop_list(&value),
            "TryExec" => e.try_exec = Some(value),
            _ => {}
        }
    }

    if !saw_key {
        return None;
    }
    entry
}

/// Decode the escape sequences a desktop-entry value may contain
/// (`\\`, `\s`, `\n`, `\t`, `\r`).
pub fn unescape_value(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                's' => {
                    out.push(' ');
                    i += 2;
                    continue;
                }
                'n' => {
                    out.push('\n');
                    i += 2;
                    continue;
                }
                't' => {
                    out.push('\t');
                    i += 2;
                    continue;
                }
                'r' => {
                    out.push('\r');
                    i += 2;
                    continue;
                }
                '\\' => {
                    out.push('\\');
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Split a `;`-separated desktop-id list (`"GNOME;KDE;"` → `["gnome", "kde"]`).
fn split_desktop_list(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// True when the entry should be offered to the user in the given desktop
/// environment.
///
/// * `Hidden=true` — an internal application; never shown, never launched.
/// * `NoDisplay=true` — installed, but not in the menus.
/// * `OnlyShowIn=` — shown only in the listed desktops. When the current
///   desktop is unknown the entry is hidden: we cannot claim to be a desktop
///   we are not.
/// * `NotShowIn=` — hidden in the listed desktops.
pub fn visible_in(entry: &DesktopEntry, current: &[String]) -> bool {
    if entry.hidden || entry.no_display {
        return false;
    }
    if !entry.only_show_in.is_empty()
        && !entry
            .only_show_in
            .iter()
            .any(|d| current.iter().any(|c| c == d))
    {
        return false;
    }
    if entry
        .not_show_in
        .iter()
        .any(|d| current.iter().any(|c| c == d))
    {
        return false;
    }
    true
}

/// Split the colon-separated desktop-id list of `XDG_CURRENT_DESKTOP`
/// (`"ubuntu:GNOME"` → `["ubuntu", "gnome"]`).
pub fn parse_desktops(raw: &str) -> Vec<String> {
    raw.split(':')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The desktop ids of the current session, from `XDG_CURRENT_DESKTOP`
/// (colon-separated). Lower-cased for comparison.
pub fn current_desktops() -> Vec<String> {
    parse_desktops(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default())
}

// ─── Exec line handling ────────────────────────────────────────────────────

/// The characters a desktop-entry field code may use after the `%`.
const FIELD_CODE_CHARS: &str = "fFuUickdDnNvm";

/// Remove the field codes (`%f`, `%U`, `%i`, …) from an `Exec=` line.
///
/// The remaining text is what actually gets executed, so quotes must survive:
/// `Exec=foo --title="Hello World"` has to keep its quoted argument. Field
/// codes are dropped as whole tokens and stripped out of the middle of an
/// argument (`--file=%f`); `%%` is a literal percent sign.
pub fn strip_field_codes(exec: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for token in tokenize(exec) {
        if is_field_code(&token) {
            continue;
        }
        out.push(remove_embedded_field_codes(&token));
    }
    out.join(" ")
}

/// True when a whole token is a field code (`%U`, `%i`, …).
fn is_field_code(token: &str) -> bool {
    // A quoted field code (`"%f"`) is still a field code.
    let token = token.trim_matches(|c| c == '"' || c == '\'');
    let Some(rest) = token.strip_prefix('%') else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|c| FIELD_CODE_CHARS.contains(c))
}

/// Remove field codes that sit inside an argument, keeping `%%` literal.
fn remove_embedded_field_codes(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    let mut out = String::with_capacity(token.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' && i + 1 < chars.len() {
            let next = chars[i + 1];
            if next == '%' {
                out.push('%');
                i += 2;
                continue;
            }
            if FIELD_CODE_CHARS.contains(next) {
                i += 2;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Split a command line into tokens, keeping the original text (and therefore
/// the quotes) of each token. Single and double quotes both group, because
/// the line is executed through a shell.
fn tokenize(line: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut in_single = false;
    let mut in_double = false;
    for ch in line.chars() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
                current.push(ch);
                started = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                current.push(ch);
                started = true;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if started {
                    tokens.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        tokens.push(current);
    }
    tokens
}

// ─── TryExec ───────────────────────────────────────────────────────────────

/// True when the `TryExec=` binary can be found.
///
/// An empty `TryExec` constrains nothing, so it passes. A value with a `/` is
/// an absolute path; anything else is looked up on `PATH`.
pub fn try_exec_available(try_exec: &str) -> bool {
    try_exec_available_in(try_exec, &std::env::var("PATH").unwrap_or_default())
}

/// `PATH`-injectable variant of [`try_exec_available`] (unit-tested).
pub fn try_exec_available_in(try_exec: &str, path_var: &str) -> bool {
    let bin = try_exec.split_whitespace().next().unwrap_or("").trim();
    if bin.is_empty() {
        return true;
    }
    if bin.contains('/') {
        return is_executable(Path::new(bin));
    }
    path_var
        .split(':')
        .filter(|d| !d.is_empty())
        .any(|dir| is_executable(&Path::new(dir).join(bin)))
}

/// True when the path is a regular file with an executable bit set.
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// Find an executable on `PATH`.
pub fn which(bin: &str) -> Option<PathBuf> {
    if bin.is_empty() {
        return None;
    }
    if bin.contains('/') {
        let path = PathBuf::from(bin);
        return if is_executable(&path) { Some(path) } else { None };
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(bin))
        .find(|p| is_executable(p))
}

/// True when `wmctrl` is installed — the only tool here that can raise a
/// window through EWMH.
pub fn wmctrl_available() -> bool {
    which("wmctrl").is_some()
}

// ─── Discovery ─────────────────────────────────────────────────────────────

/// The user's own data directory (`XDG_DATA_HOME`, `$HOME/.local/share`).
fn data_home() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(".local/share"))
}

/// Every directory that may hold `.desktop` files.
///
/// The user's own directories come last: the registry indexes by name and
/// writes the last entry it sees, so the user's own copy of an entry has to
/// be scanned after the system one to win.
pub fn desktop_dirs() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = Vec::new();

    // XDG_DATA_DIRS: the system directories.
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    for dir in data_dirs.split(':').filter(|d| !d.trim().is_empty()) {
        dirs.push(PathBuf::from(dir).join("applications"));
    }

    // Flatpak (system-wide) and Snap exports.
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    dirs.push(PathBuf::from("/var/lib/snapd/desktop/applications"));

    // The user's own applications, then the user's own Flatpak exports.
    dirs.push(data_home().join("applications"));
    dirs.push(PathBuf::from(&home).join(".local/share/flatpak/exports/share/applications"));

    dirs
}

/// The browser directories that hold installed PWAs. They sit one level below
/// the applications directory, so the normal scan never sees them.
fn pwa_dirs() -> Vec<PathBuf> {
    let base = data_home().join("applications");
    ["chrome", "chromium", "brave", "microsoft-edge", "opera", "vivaldi"]
        .iter()
        .map(|b| base.join(b))
        .collect()
}

/// Scan every desktop directory and append the launchable apps.
///
/// Entries are filtered the way the desktop-entry spec says: `Hidden`,
/// `NoDisplay`, `OnlyShowIn`, `NotShowIn` and `TryExec` are all honoured.
pub fn discover(entries: &mut Vec<AppEntry>) {
    let current = current_desktops();
    if current.is_empty() {
        tracing::debug!(
            "app registry: XDG_CURRENT_DESKTOP is unset — entries limited to another \
             desktop (OnlyShowIn) stay hidden"
        );
    }
    let mut collected: Vec<AppEntry> = Vec::new();

    for dir in desktop_dirs() {
        let Ok(readdir) = std::fs::read_dir(&dir) else {
            continue;
        };
        for item in readdir.flatten() {
            let path = item.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some(desktop) = parse_desktop_entry(&content) else {
                continue;
            };
            if let Some(app) = desktop_entry_to_app(&desktop, &current) {
                collected.push(app);
            }
        }
    }

    // One entry per name, and the LAST copy wins: the user's own directory is
    // scanned last, so the user's entry beats the system one — and it is also
    // last in the vector, so the registry's own last-write-wins keeps it for
    // the word keys two apps share ("files" in "Files" and "Files Manager").
    let collected = keep_last_per_name(collected);

    // Flatpak apps whose export is missing, and browser PWAs: both skip the
    // names the desktop entries already cover.
    let known: HashSet<String> = collected
        .iter()
        .map(|e| e.display_name.to_lowercase())
        .collect();
    let mut extra: Vec<AppEntry> = Vec::new();
    discover_flatpak_apps(&known, &mut extra);
    discover_pwas(&known, &mut extra);

    let count = collected.len() + extra.len();
    entries.extend(collected);
    entries.extend(extra);

    tracing::info!("discovered {} apps from Linux desktop entries", count);
}

/// Keep only the last entry of every duplicated display name.
fn keep_last_per_name(apps: Vec<AppEntry>) -> Vec<AppEntry> {
    let mut last_of_name: HashMap<String, usize> = HashMap::new();
    for (i, app) in apps.iter().enumerate() {
        last_of_name.insert(app.display_name.to_lowercase(), i);
    }
    apps.into_iter()
        .enumerate()
        .filter(|(i, app)| last_of_name.get(&app.display_name.to_lowercase()) == Some(i))
        .map(|(_, app)| app)
        .collect()
}

/// Turn a parsed desktop entry into a registry entry, or `None` when the
/// entry must not be offered.
fn desktop_entry_to_app(desktop: &DesktopEntry, current: &[String]) -> Option<AppEntry> {
    // Only applications can be launched; `Type=Link` entries are URLs and
    // `Type=Directory` entries are folders.
    if !desktop.kind.is_empty() && !desktop.kind.eq_ignore_ascii_case("application") {
        return None;
    }
    if !visible_in(desktop, current) {
        return None;
    }
    if let Some(try_exec) = &desktop.try_exec {
        if !try_exec_available(try_exec) {
            tracing::debug!("desktop entry skipped: TryExec '{try_exec}' not found");
            return None;
        }
    }
    if desktop.name.is_empty() || desktop.exec.is_empty() {
        return None;
    }
    let exec = strip_field_codes(&desktop.exec);
    if exec.is_empty() {
        return None;
    }
    let search_names = build_search_names(&desktop.name);
    Some(AppEntry {
        display_name: desktop.name.clone(),
        search_names,
        launch: LaunchMethod::DesktopExec { exec },
        use_count: 0,
        last_used: 0,
    })
}

/// Discover PWAs installed via Chrome, Brave, Edge, Opera or Vivaldi.
///
/// They live in a browser sub-directory of the applications directory, so the
/// normal scan never reaches them.
fn discover_pwas(known: &HashSet<String>, out: &mut Vec<AppEntry>) {
    let current = current_desktops();
    let mut collected: Vec<AppEntry> = Vec::new();

    for dir in pwa_dirs() {
        let Ok(readdir) = std::fs::read_dir(&dir) else {
            continue;
        };
        for item in readdir.flatten() {
            let path = item.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some(desktop) = parse_desktop_entry(&content) else {
                continue;
            };
            if desktop.name.is_empty() || desktop.exec.is_empty() {
                continue;
            }
            if known.contains(&desktop.name.to_lowercase()) {
                continue;
            }
            if let Some(app) = desktop_entry_to_app(&desktop, &current) {
                collected.push(app);
            }
        }
    }
    let count = collected.len();
    out.extend(keep_last_per_name(collected));
    if count > 0 {
        tracing::debug!("discovered {} PWAs from Linux browsers", count);
    }
}

/// Find the command line of the desktop entry whose file name matches
/// `query`.
///
/// The legacy resolver in `command_executor` used to grep the raw file names
/// of a handful of directories. This does the same through the real scanner,
/// so the desktop-entry filters apply to it as well.
pub fn find_desktop_exec(query: &str) -> Option<String> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }
    let query_no_spaces = query.replace(' ', "-");
    let current = current_desktops();

    for dir in desktop_dirs() {
        let Ok(readdir) = std::fs::read_dir(&dir) else {
            continue;
        };
        for item in readdir.flatten() {
            let path = item.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let file_name = item.file_name().to_string_lossy().to_lowercase();
            if !file_name.contains(&query) && !file_name.contains(&query_no_spaces) {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some(desktop) = parse_desktop_entry(&content) else {
                continue;
            };
            if let Some(app) = desktop_entry_to_app(&desktop, &current) {
                return match app.launch {
                    LaunchMethod::DesktopExec { exec } => Some(exec),
                    _ => None,
                };
            }
        }
    }
    None
}

/// Use `flatpak list` to find installed Flatpak apps whose export is missing.
fn discover_flatpak_apps(known: &HashSet<String>, out: &mut Vec<AppEntry>) {
    let output = std::process::Command::new("flatpak")
        .args(["list", "--columns=application,name"])
        .output();
    let stdout = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return, // flatpak not installed
    };

    let mut count = 0usize;
    for line in stdout.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        // flatpak separates the columns with a tab.
        let (app_id, name) = match line.split_once('\t') {
            Some((id, name)) => (id.trim(), name.trim()),
            None => {
                let mut parts = line.split_whitespace();
                match (parts.next(), parts.next()) {
                    (Some(id), Some(name)) => (id, name),
                    _ => continue,
                }
            }
        };
        if app_id.is_empty() || name.is_empty() {
            continue;
        }
        if known.contains(&name.to_lowercase()) {
            continue;
        }
        let exec = format!("flatpak run {}", app_id);
        let search_names = build_search_names(name);
        out.push(AppEntry {
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

// ─── Window list (X11) ─────────────────────────────────────────────────────

/// One row of `wmctrl -l -p`.
///
/// The desktop and host columns of the raw output are consumed by the parser
/// (they have to be, or the title would start with the host name) but are not
/// kept: NEXUS only ever needs the id, the pid and the title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WmWindow {
    /// The EWMH window id (hexadecimal in the raw output).
    pub id: u64,
    /// The owning process id.
    pub pid: u32,
    /// The window title. May contain spaces.
    pub title: String,
}

/// List the X11 windows through `wmctrl`.
///
/// Returns an empty list when `wmctrl` is not installed or the session is
/// native Wayland — there is no window list there at all.
pub fn list_windows() -> Vec<WmWindow> {
    let output = std::process::Command::new("wmctrl")
        .args(["-l", "-p"])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            parse_wmctrl_list(&String::from_utf8_lossy(&out.stdout))
        }
        Ok(_) => Vec::new(),
        Err(_) => Vec::new(),
    }
}

/// Parse the whole output of `wmctrl -l -p`.
pub fn parse_wmctrl_list(stdout: &str) -> Vec<WmWindow> {
    stdout.lines().filter_map(parse_wmctrl_line).collect()
}

/// Parse one line of `wmctrl -l -p`.
///
/// The columns are `<id> <desktop> <pid> <host> <title>`, separated by runs
/// of whitespace (wmctrl pads them). The id is hexadecimal, so it must be
/// read with radix 16 — `parse::<isize>()` fails on `0x03e00006` and the
/// window was then never focused. The title is everything after the host
/// column and keeps its internal spacing.
pub fn parse_wmctrl_line(line: &str) -> Option<WmWindow> {
    let mut rest = line;
    let id = take_column(&mut rest)?;
    // The desktop and host columns are consumed but not kept.
    let _desktop = take_column(&mut rest)?;
    let pid = take_column(&mut rest)?;
    let _host = take_column(&mut rest)?;
    let title = rest.trim().to_string();

    let id = parse_window_id(id)?;
    let pid: u32 = pid.trim().parse().ok()?;
    if title.is_empty() {
        // A window without a title cannot be matched by one.
        return None;
    }

    Some(WmWindow { id, pid, title })
}

/// Take the next whitespace-delimited column, skipping the run of spaces
/// before it.
fn take_column<'a>(rest: &mut &'a str) -> Option<&'a str> {
    let trimmed = rest.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let (column, tail) = trimmed.split_at(end);
    *rest = tail;
    Some(column)
}

/// Parse a wmctrl window id (`0x03e00006` → 65177606).
pub fn parse_window_id(raw: &str) -> Option<u64> {
    let s = raw.trim();
    let hex = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    if hex.is_empty() {
        return None;
    }
    u64::from_str_radix(hex, 16).ok()
}

// ─── Focus ─────────────────────────────────────────────────────────────────

/// Raise an existing window by its EWMH id.
///
/// Only X11 (including XWayland) can do this. On native Wayland there is no
/// EWMH, so the call fails honestly instead of pretending.
pub fn focus_window_id(id: isize) -> bool {
    if !crate::session::has_x11_input() {
        tracing::warn!(
            "focus_window: {} — EWMH unavailable, not attempting",
            crate::session::diagnostic_line()
        );
        return false;
    }
    if id <= 0 {
        return false;
    }
    let hex = format!("0x{:x}", id as u64);
    // Restore first: activating a hidden (minimised) window does nothing.
    let restore = std::process::Command::new("wmctrl")
        .args(["-i", "-r", hex.as_str(), "-b", "remove,hidden"])
        .output();
    if let Ok(out) = &restore {
        if !out.status.success() {
            tracing::debug!(
                "focus_window: wmctrl restore failed ({})",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
    }
    match std::process::Command::new("wmctrl")
        .args(["-i", "-a", hex.as_str()])
        .output()
    {
        Ok(out) if out.status.success() => {
            tracing::debug!("focus_window: wmctrl -i -a {hex} ok");
            true
        }
        Ok(out) => {
            tracing::debug!(
                "focus_window: wmctrl -i -a {hex} failed ({})",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            false
        }
        Err(e) => {
            tracing::debug!("focus_window: wmctrl spawn failed: {e}");
            false
        }
    }
}

/// Focus a window by (partial) title through `wmctrl`.
///
/// Separate from [`focus_window_id`] because that one takes the id from the
/// `wmctrl`-populated cache, whereas callers here only have a display name.
pub fn focus_by_title(partial_title: &str) -> bool {
    if partial_title.trim().is_empty() || !crate::session::has_x11_input() {
        return false;
    }
    if !is_wmctrl_pattern_safe(partial_title) {
        tracing::debug!("focus_by_title: '{partial_title}' is not a safe wmctrl pattern");
        return false;
    }
    match std::process::Command::new("wmctrl")
        .args(["-a", partial_title])
        .status()
    {
        Ok(st) => st.success(),
        Err(e) => {
            tracing::debug!("focus_by_title: wmctrl unavailable: {e}");
            false
        }
    }
}

/// True when the text is safe to hand to `wmctrl -a`.
///
/// `wmctrl` matches the window title as a *regular expression*, so a name
/// containing regex metacharacters ("C++", "Rock & Roll") either fails to
/// match or matches the wrong window. Only plain names are passed on.
pub fn is_wmctrl_pattern_safe(text: &str) -> bool {
    !text.trim().is_empty()
        && text.chars().all(|c| {
            c.is_alphanumeric()
                || c.is_whitespace()
                || matches!(c, '-' | '_' | '.' | ',' | '\'' | '!' | '#')
        })
}

/// Speakable explanation for why another app's window cannot be raised here.
///
/// `None` means focusing may work. Pure (unit-tested): the session and the
/// presence of `wmctrl` are passed in.
pub fn focus_blocked_reason(has_x11: bool, wmctrl_present: bool) -> Option<String> {
    if !has_x11 {
        return Some(
            "this is a native Wayland session — nothing outside the compositor can raise \
             another app's window"
                .to_string(),
        );
    }
    if !wmctrl_present {
        return Some("wmctrl is not installed, so I cannot raise windows here".to_string());
    }
    None
}

/// The same reason, read from the live session.
pub fn focus_blocked_reason_now() -> Option<String> {
    focus_blocked_reason(crate::session::has_x11_input(), wmctrl_available())
}

// ─── "Is it already running?" ──────────────────────────────────────────────

/// Commands whose whole point is to start *another* window or process when
/// they are run again.
///
/// A shell, an interpreter or a terminal emulator that is already running
/// does NOT mean "the app the user asked for is open" — running it again is
/// exactly how the user asks for a new window. Treating them as "already
/// running" would make NEXUS refuse to open a terminal at all.
const COMMANDS_THAT_SPAWN_A_NEW_WINDOW: &[&str] = &[
    "sh", "bash", "zsh", "dash", "ksh", "env", "setsid", "nohup", "dbus-run-session",
    "dbus-launch", "gapplication", "appservice", "gtk-launch", "xdg-open", "exo-open",
    "kde-open", "gnome-open", "xterm", "uxterm", "x-terminal-emulator", "gnome-terminal",
    "kgx", "konsole", "mate-terminal", "xfce4-terminal", "tilix", "kitty", "alacritty",
    "wezterm", "terminator", "foot", "python", "python2", "python3", "perl", "ruby", "node",
    "nodejs", "java", "mono", "wine", "wine64",
];

/// The process names an `Exec=` line can produce.
///
/// Empty when the command cannot be identified (a shell wrapper, an
/// interpreter, a desktop-open helper) — see
/// [`COMMANDS_THAT_SPAWN_A_NEW_WINDOW`].
pub fn exec_process_names(exec: &str) -> Vec<String> {
    let tokens = tokenize(exec);
    let mut idx = 0usize;

    // `env VAR=value … command`
    if tokens.get(0).map(|t| t == "env").unwrap_or(false) {
        idx = 1;
        while idx < tokens.len()
            && tokens[idx].contains('=')
            && !tokens[idx].starts_with('-')
        {
            idx += 1;
        }
    }

    let Some(first) = tokens.get(idx) else {
        return Vec::new();
    };

    if first == "flatpak" {
        // `flatpak run [--branch=… --arch=…] <app-id>` — the app id is the
        // first argument that looks like one (it contains a dot and is not an
        // option).
        if tokens.get(idx + 1).map(|t| t == "run").unwrap_or(false) {
            for token in tokens.iter().skip(idx + 2) {
                let token = token.trim_matches(|c| c == '"' || c == '\'');
                if token.starts_with('-') || !token.contains('.') {
                    continue;
                }
                let mut names = vec![token.to_lowercase()];
                if let Some(tail) = token.rsplit('.').next() {
                    names.push(tail.to_lowercase());
                }
                return names;
            }
        }
        return Vec::new();
    }

    let base = first.trim_matches(|c| c == '"' || c == '\'');
    let base = base.rsplit('/').next().unwrap_or(base);
    if base.is_empty() || COMMANDS_THAT_SPAWN_A_NEW_WINDOW.contains(&base) {
        return Vec::new();
    }
    vec![base.to_lowercase()]
}

/// True when a running process belongs to the app.
///
/// ONE rule for both questions — "is it running?" and "which window is
/// its?" — so the two can never disagree.
///
/// The process name must be the app's own name, or that name with a
/// version/architecture decoration (`gimp-2.10`, `firefox-bin`). A
/// different product never matches: `code-insiders` is not `code`, so
/// "open code" has to launch VS Code instead of reporting that it is
/// already running.
pub fn is_same_app(process_name: &str, base: &str) -> bool {
    if base.is_empty() {
        return false;
    }
    if process_name == base {
        return true;
    }
    match process_name.strip_prefix(base) {
        Some(tail) => is_decoration(tail),
        None => false,
    }
}

/// True when the tail after the app's name is a version or a build suffix.
///
/// `2.10` and `-bin` count; `-insiders`, `-esr` and `-oss` do not — those
/// are different products that merely start with the same word.
fn is_decoration(tail: &str) -> bool {
    let (rest, had_separator) = match tail.chars().next() {
        Some(c) if matches!(c, '-' | '_' | '.') => (&tail[c.len_utf8()..], true),
        Some(c) if c.is_ascii_digit() => (tail, false),
        _ => return false,
    };
    if rest.is_empty() {
        return false;
    }
    // A version: digits, then optional ".digits" groups.
    if rest.chars().all(|c| c.is_ascii_digit() || c == '.')
        && rest.chars().any(|c| c.is_ascii_digit())
    {
        return true;
    }
    // A build suffix — only with a separator, so "codebin" is not "code".
    had_separator && (rest == "bin" || rest == "wrapped")
}

/// The pids of the app's own running processes, or `None` when the app is
/// not running at all.
///
/// Pure (unit-tested). The Insiders case: with only `code-insiders`
/// running, "code" has no process of its own, so this is `None` and the
/// caller launches VS Code — it must not wait for a window of an app that
/// is not running.
pub fn app_pids(snapshot: &ProcessSnapshot, names: &[String]) -> Option<Vec<u32>> {
    let pids = snapshot.pids_for(names);
    if pids.is_empty() {
        None
    } else {
        Some(pids)
    }
}

/// A snapshot of the process table.
///
/// The focus path asks "is this app running?" on every attempt, and a full
/// /proc scan is not free — the snapshot is read once and the answer is then
/// read off it.
#[derive(Debug, Clone)]
pub struct ProcessSnapshot {
    /// (lowercase process name, pid) for every running process.
    processes: Vec<(String, u32)>,
}

impl ProcessSnapshot {
    /// Read the whole process table once.
    pub fn capture() -> Self {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            false,
            ProcessRefreshKind::new(),
        );
        let processes = sys
            .processes()
            .iter()
            .map(|(pid, process)| {
                (
                    process.name().to_string_lossy().to_lowercase(),
                    pid.as_u32(),
                )
            })
            .collect();
        ProcessSnapshot { processes }
    }

    /// The pids of the processes that belong to the app.
    ///
    /// One rule for both "is it running?" and "which window is its?" — see
    /// [`is_same_app`]. The name must match exactly, or carry a version or
    /// build decoration (`gimp-2.10`, `firefox-bin`); a different product
    /// (`code-insiders`) never matches.
    pub fn pids_for(&self, names: &[String]) -> Vec<u32> {
        if names.is_empty() {
            return Vec::new();
        }
        let mut pids: Vec<u32> = Vec::new();
        for (process_name, pid) in &self.processes {
            if names
                .iter()
                .any(|candidate| is_same_app(process_name, candidate))
            {
                pids.push(*pid);
            }
        }
        pids
    }
}

/// The EWMH id of the window owned by one of `pids`.
///
/// `windows` is the `wmctrl -l -p` output as (id, pid) pairs. The pid is
/// exact while the title is not: a `.desktop` entry is called "Text Editor"
/// while its window is called "gedit", and "Firefox" while its window is
/// "Mozilla Firefox".
pub fn window_id_for_pids(windows: &[(u64, u32)], pids: &[u32]) -> Option<u64> {
    if pids.is_empty() {
        return None;
    }
    windows
        .iter()
        .find(|(_, pid)| pids.contains(pid))
        .map(|(id, _)| *id)
}

/// What a focus attempt found, before anything was raised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusFacts {
    /// A window belonging to the app was found.
    WindowFound,
    /// The app's process is running, but no window was found for it.
    ProcessOnly,
    /// The app has no process of its own — it is not running.
    NotRunning,
}

/// Map the facts of a focus attempt onto the outcome the caller sees.
///
/// Pure (unit-tested). `raised` says whether the window actually came up;
/// `blocked` is the speakable reason when this session cannot raise another
/// app's window at all (native Wayland has no EWMH).
pub fn focus_outcome(facts: FocusFacts, raised: bool, blocked: Option<&str>) -> FocusOutcome {
    match facts {
        FocusFacts::WindowFound => {
            if raised {
                FocusOutcome::Focused
            } else {
                FocusOutcome::RunningCannotFocus(blocked_reason_message(blocked))
            }
        }
        FocusFacts::ProcessOnly => match blocked {
            Some(reason) => {
                FocusOutcome::RunningCannotFocus(blocked_reason_message(Some(reason)))
            }
            // The app is up but its window is not mapped yet: the caller
            // waits for it instead of starting a second copy.
            None => FocusOutcome::WindowNotReady,
        },
        // No process of its own: the caller launches it.
        FocusFacts::NotRunning => FocusOutcome::NotRunning,
    }
}

/// The spoken sentence for "this session cannot raise another app's window".
pub fn blocked_reason_message(blocked: Option<&str>) -> String {
    match blocked {
        Some(reason) => format!("I can't bring it to the front, sir — {reason}."),
        None => "I couldn't raise that window, sir.".to_string(),
    }
}

/// What the legacy "the app is already running" resolver should do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyFocusAction {
    /// The window was raised: report the app as already open.
    AlreadyOpen,
    /// This session cannot raise another app's window: say so, and do NOT
    /// launch a copy of an app that is already running.
    CannotFocus(String),
    /// The session can raise windows, but none carried that title: fall
    /// through and let the caller launch a fresh instance.
    LaunchCopy,
}

/// The pure decision of the legacy "the app is already running" path.
///
/// `focused` says whether the window was actually raised; `blocked` is the
/// speakable reason when this session cannot raise another app's window at
/// all (native Wayland has no EWMH).
///
/// A blocked session never yields [`LegacyFocusAction::LaunchCopy`]: that is
/// what used to start a second GIMP (the "7 Notepad windows" regression).
pub fn legacy_focus_action(focused: bool, blocked: Option<&str>) -> LegacyFocusAction {
    if focused {
        LegacyFocusAction::AlreadyOpen
    } else if let Some(reason) = blocked {
        LegacyFocusAction::CannotFocus(blocked_reason_message(Some(reason)))
    } else {
        LegacyFocusAction::LaunchCopy
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Desktop entry filters ──────────────────────────────────────────────

    fn entry_with(body: &str) -> DesktopEntry {
        let content = format!("[Desktop Entry]\nType=Application\nName=Test App\nExec=test-app\n{body}");
        parse_desktop_entry(&content).expect("entry must parse")
    }

    #[test]
    fn parses_the_desktop_entry_group_only() {
        let content = "[Desktop Entry]\nType=Application\nName=Files\nExec=nautilus\n\n[Desktop Action new]\nName=New Window\nExec=nautilus --new-window\n";
        let entry = parse_desktop_entry(content).expect("entry must parse");
        assert_eq!(entry.name, "Files");
        assert_eq!(entry.exec, "nautilus");
        assert_eq!(entry.kind, "Application");
    }

    #[test]
    fn ignores_localised_names() {
        let content = "[Desktop Entry]\nType=Application\nName=Files\nName[de]=Dateien\nExec=nautilus\n";
        let entry = parse_desktop_entry(content).expect("entry must parse");
        assert_eq!(entry.name, "Files");
    }

    #[test]
    fn hidden_entries_are_never_visible() {
        let entry = entry_with("Hidden=true\n");
        assert!(entry.hidden);
        assert!(!visible_in(&entry, &["gnome".to_string()]));
    }

    #[test]
    fn nodisplay_entries_are_never_visible() {
        let entry = entry_with("NoDisplay=true\n");
        assert!(entry.no_display);
        assert!(!visible_in(&entry, &["gnome".to_string()]));
    }

    #[test]
    fn only_show_in_limits_the_desktops() {
        let entry = entry_with("OnlyShowIn=GNOME;KDE;\n");
        assert_eq!(entry.only_show_in, vec!["gnome".to_string(), "kde".to_string()]);
        // Case-insensitive.
        assert!(visible_in(&entry, &["gnome".to_string()]));
        assert!(visible_in(&entry, &["kde".to_string()]));
        assert!(!visible_in(&entry, &["cosmic".to_string()]));
        // An unknown desktop cannot claim to be one of the listed ones.
        assert!(!visible_in(&entry, &[]));
    }

    #[test]
    fn not_show_in_excludes_the_desktops() {
        let entry = entry_with("NotShowIn=GNOME;\n");
        assert_eq!(entry.not_show_in, vec!["gnome".to_string()]);
        assert!(!visible_in(&entry, &["gnome".to_string()]));
        assert!(visible_in(&entry, &["cosmic".to_string()]));
    }

    #[test]
    fn a_plain_entry_is_visible_everywhere() {
        let entry = entry_with("");
        assert!(visible_in(&entry, &["cosmic".to_string()]));
        assert!(visible_in(&entry, &[]));
    }

    #[test]
    fn current_desktops_are_split_and_lowercased() {
        assert_eq!(
            parse_desktops("ubuntu:GNOME"),
            vec!["ubuntu".to_string(), "gnome".to_string()]
        );
        assert_eq!(
            parse_desktops("COSMIC:"),
            vec!["cosmic".to_string()]
        );
        assert!(parse_desktops("").is_empty());
    }

    // ── TryExec ────────────────────────────────────────────────────────────

    /// A throwaway directory holding one file with the given mode. The name
    /// is unique per test so parallel tests cannot share it.
    fn probe_dir(name: &str, executable: bool) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nexus-tryexec-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("probe-bin");
        let _ = std::fs::write(&file, b"#!/bin/sh\nexit 0\n");
        use std::os::unix::fs::PermissionsExt;
        let mode = if executable { 0o755 } else { 0o644 };
        let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode));
        dir
    }

    #[test]
    fn try_exec_is_skipped_when_the_binary_is_missing() {
        let dir = probe_dir("missing", true);
        let path = dir.to_string_lossy().to_string();
        assert!(try_exec_available_in("probe-bin", &path));
        assert!(!try_exec_available_in("probe-bin-not-here", &path));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn try_exec_ignores_a_file_that_is_not_executable() {
        let dir = probe_dir("noexec", false);
        let path = dir.to_string_lossy().to_string();
        assert!(!try_exec_available_in("probe-bin", &path));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_try_exec_constrains_nothing() {
        assert!(try_exec_available_in("", "/nowhere"));
        assert!(try_exec_available_in("   ", "/nowhere"));
    }

    #[test]
    fn try_exec_accepts_an_absolute_path() {
        let dir = probe_dir("absolute", true);
        let file = dir.join("probe-bin");
        let path = file.to_string_lossy().to_string();
        assert!(try_exec_available_in(&path, "/nowhere"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Exec lines ─────────────────────────────────────────────────────────

    #[test]
    fn field_codes_are_removed_and_quotes_survive() {
        assert_eq!(strip_field_codes("firefox %u"), "firefox");
        assert_eq!(strip_field_codes("firefox %U"), "firefox");
        assert_eq!(strip_field_codes("gimp %U %f"), "gimp");
        assert_eq!(
            strip_field_codes("foo --title=\"Hello World\" %f"),
            "foo --title=\"Hello World\""
        );
        assert_eq!(strip_field_codes("app --file=%f"), "app --file=");
        assert_eq!(strip_field_codes("app 100%%"), "app 100%");
        // A quoted field code is still a field code.
        assert_eq!(strip_field_codes("app \"%f\""), "app");
    }

    #[test]
    fn process_names_come_from_the_exec_line() {
        assert_eq!(exec_process_names("firefox %u"), vec!["firefox".to_string()]);
        assert_eq!(
            exec_process_names("/usr/bin/firefox %u"),
            vec!["firefox".to_string()]
        );
        assert_eq!(
            exec_process_names("env FOO=bar brave-browser %u"),
            vec!["brave-browser".to_string()]
        );
        assert_eq!(
            exec_process_names("flatpak run --branch=stable org.mozilla.Firefox"),
            vec!["org.mozilla.firefox".to_string(), "firefox".to_string()]
        );
    }

    #[test]
    fn generic_commands_are_not_treated_as_one_running_app() {
        assert!(exec_process_names("sh -c something").is_empty());
        assert!(exec_process_names("python3 /opt/app/main.py").is_empty());
        assert!(exec_process_names("gnome-terminal").is_empty());
    }

    // ── wmctrl parsing ─────────────────────────────────────────────────────

    #[test]
    fn wmctrl_ids_are_hexadecimal() {
        let line = "0x03e00006  0   21337  hostname  Brave";
        let window = parse_wmctrl_line(line).expect("line must parse");
        assert_eq!(window.id, 0x03e0_0006);
        assert_eq!(window.pid, 21337);
        // The host column must be consumed, not folded into the title.
        assert_eq!(window.title, "Brave");
    }

    #[test]
    fn wmctrl_columns_may_be_padded_with_spaces() {
        let line = "0x04000003   0    1234   host    Spaced   Out   Title";
        let window = parse_wmctrl_line(line).expect("line must parse");
        assert_eq!(window.id, 0x0400_0003);
        assert_eq!(window.pid, 1234);
        assert_eq!(window.title, "Spaced   Out   Title");
    }

    #[test]
    fn wmctrl_lines_without_a_title_are_skipped() {
        assert!(parse_wmctrl_line("0x04000003  0  1234  host").is_none());
        assert!(parse_wmctrl_line("").is_none());
        assert!(parse_wmctrl_line("not-a-window").is_none());
    }

    #[test]
    fn the_whole_list_parses() {
        let stdout = "0x03e00006  0   21337  hostname  Brave\n0x03e00007  0   21337  hostname  Brave\n";
        let windows = parse_wmctrl_list(stdout);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[1].id, 0x03e0_0007);
    }

    #[test]
    fn window_ids_parse_with_and_without_the_prefix() {
        assert_eq!(parse_window_id("0x03e00006"), Some(0x03e0_0006));
        assert_eq!(parse_window_id("03e00006"), Some(0x03e0_0006));
        assert_eq!(parse_window_id("nonsense"), None);
        assert_eq!(parse_window_id(""), None);
    }

    // ── Focus ──────────────────────────────────────────────────────────────

    #[test]
    fn wayland_sessions_get_an_explicit_reason() {
        let reason = focus_blocked_reason(false, true).expect("wayland is blocked");
        assert!(reason.contains("Wayland"));
        assert!(focus_blocked_reason(false, false).is_some());
    }

    #[test]
    fn x11_without_wmctrl_gets_an_explicit_reason() {
        let reason = focus_blocked_reason(true, false).expect("no wmctrl is blocked");
        assert!(reason.contains("wmctrl"));
    }

    #[test]
    fn x11_with_wmctrl_is_not_blocked() {
        assert!(focus_blocked_reason(true, true).is_none());
    }

    #[test]
    fn wmctrl_patterns_reject_regex_metacharacters() {
        assert!(is_wmctrl_pattern_safe("Google Chrome"));
        assert!(is_wmctrl_pattern_safe("Visual Studio Code"));
        assert!(!is_wmctrl_pattern_safe("C++"));
        assert!(!is_wmctrl_pattern_safe("Rock & Roll"));
        assert!(!is_wmctrl_pattern_safe(""));
        assert!(!is_wmctrl_pattern_safe("   "));
    }

    // ── PID → window ───────────────────────────────────────────────────────

    #[test]
    fn the_window_of_the_running_process_is_found_by_pid() {
        // The window title is useless here: the .desktop entry says
        // "Text Editor" while the window says "gedit". The pid is exact.
        let windows = [(0x03e0_0006u64, 21337u32), (0x03e0_0007, 21338)];
        assert_eq!(window_id_for_pids(&windows, &[21338]), Some(0x03e0_0007));
        assert_eq!(window_id_for_pids(&windows, &[21337]), Some(0x03e0_0006));
        // Several pids: the first window of any of them wins.
        assert_eq!(
            window_id_for_pids(&windows, &[999, 21337]),
            Some(0x03e0_0006)
        );
    }

    #[test]
    fn a_process_without_a_window_has_no_window_id() {
        let windows = [(0x03e0_0006u64, 21337u32)];
        assert_eq!(window_id_for_pids(&windows, &[4242]), None);
        assert_eq!(window_id_for_pids(&windows, &[]), None);
        assert_eq!(window_id_for_pids(&[], &[21337]), None);
    }

    #[test]
    fn the_window_pick_needs_the_apps_own_process() {
        // `code` and `code-insiders` both running: `code-insiders` is a
        // different product, so it must not own the window that "open code"
        // raises.
        let snapshot = ProcessSnapshot {
            processes: vec![
                ("code".to_string(), 100),
                ("code-insiders".to_string(), 200),
            ],
        };
        let names = vec!["code".to_string()];
        assert_eq!(snapshot.pids_for(&names), vec![100]);
        let windows = [(0x03e0_0006u64, 100u32), (0x03e0_0007, 200)];
        assert_eq!(
            window_id_for_pids(&windows, &snapshot.pids_for(&names)),
            Some(0x03e0_0006)
        );
    }

    #[test]
    fn a_different_product_is_not_the_app() {
        // The Insiders case: a different product with a similar name is not
        // the app the user asked for.
        assert!(!is_same_app("code-insiders", "code"));
        assert!(!is_same_app("code-oss", "code"));
        assert!(!is_same_app("codium", "code"));
        // Same product, exact name.
        assert!(is_same_app("code", "code"));
    }

    #[test]
    fn a_version_decoration_is_the_app() {
        // "gimp-2.10" is GIMP; "gimp-2.10" as a process name with the
        // Exec base "gimp".
        assert!(is_same_app("gimp-2.10", "gimp"));
        assert!(is_same_app("gimp2.10", "gimp"));
        assert!(is_same_app("gimp-2", "gimp"));
        // …but not a different product that merely starts the same way.
        assert!(!is_same_app("gimp-2.10-insiders", "gimp"));
    }

    #[test]
    fn a_build_suffix_is_the_app() {
        assert!(is_same_app("firefox-bin", "firefox"));
        assert!(is_same_app("firefox-wrapped", "firefox"));
        // "firefox-esr" is a different build of Firefox, but the reviewer
        // asked for only version/build decorations to count — it does not.
        assert!(!is_same_app("firefox-esr", "firefox"));
    }

    #[test]
    fn the_insiders_case_launches_instead_of_waiting() {
        // Only code-insiders is running: "code" has no process of its own,
        // so the outcome is "not running" and the caller launches VS Code.
        let snapshot = ProcessSnapshot {
            processes: vec![("code-insiders".to_string(), 200)],
        };
        let names = vec!["code".to_string()];
        assert!(app_pids(&snapshot, &names).is_none());

        // It must never come out as "running but the window isn't showing":
        // that refuses to launch an app that is not running at all.
        let outcome = focus_outcome(FocusFacts::NotRunning, false, None);
        assert_eq!(outcome, FocusOutcome::NotRunning);
        assert_ne!(outcome, FocusOutcome::WindowNotReady);
    }

    #[test]
    fn a_decorated_process_is_the_app_and_is_found() {
        // "gimp-2.10" running: the app IS running, so the caller must not
        // launch a second copy.
        let snapshot = ProcessSnapshot {
            processes: vec![("gimp-2.10".to_string(), 4242)],
        };
        let names = vec!["gimp".to_string()];
        assert_eq!(app_pids(&snapshot, &names), Some(vec![4242]));
        let windows = [(0x03e0_0009u64, 4242u32)];
        assert_eq!(
            window_id_for_pids(&windows, &snapshot.pids_for(&names)),
            Some(0x03e0_0009)
        );
    }

    // ── Legacy "app is already running" decision ───────────────────────────

    #[test]
    fn a_blocked_session_never_launches_a_copy() {
        // The regression this guards: falling through to "launch" while the
        // session cannot raise windows started a SECOND copy of a running
        // app (the duplicate-GIMP report).
        let action = legacy_focus_action(false, Some("this is a native Wayland session"));
        assert_ne!(action, LegacyFocusAction::LaunchCopy);
        match action {
            LegacyFocusAction::CannotFocus(message) => {
                assert!(message.contains("Wayland"), "message was: {message}");
            }
            other => panic!("expected CannotFocus, got {other:?}"),
        }
    }

    #[test]
    fn a_focused_window_is_reported_as_already_open() {
        assert_eq!(
            legacy_focus_action(true, None),
            LegacyFocusAction::AlreadyOpen
        );
    }

    #[test]
    fn a_focusable_session_without_a_matching_window_launches_a_copy() {
        // X11 with wmctrl: the app runs but no window carries that title, so
        // a fresh instance is the right answer.
        assert_eq!(
            legacy_focus_action(false, None),
            LegacyFocusAction::LaunchCopy
        );
    }

    // ── Outcome mapping ────────────────────────────────────────────────────

    #[test]
    fn a_raised_window_is_focused() {
        assert_eq!(
            focus_outcome(FocusFacts::WindowFound, true, None),
            FocusOutcome::Focused
        );
    }

    #[test]
    fn a_window_that_could_not_be_raised_says_why() {
        let outcome = focus_outcome(
            FocusFacts::WindowFound,
            false,
            Some("wmctrl is not installed, so I cannot raise windows here"),
        );
        match outcome {
            FocusOutcome::RunningCannotFocus(reason) => {
                assert!(reason.contains("wmctrl"), "reason was: {reason}");
            }
            other => panic!("expected RunningCannotFocus, got {other:?}"),
        }
    }

    #[test]
    fn a_running_process_without_a_window_waits_for_it() {
        // X11 with wmctrl: the window may simply not be mapped yet, so the
        // caller waits instead of starting a second copy.
        assert_eq!(
            focus_outcome(FocusFacts::ProcessOnly, false, None),
            FocusOutcome::WindowNotReady
        );
    }

    #[test]
    fn a_running_process_on_wayland_is_refused_with_a_reason() {
        // Native Wayland: nothing can raise the window, now or later.
        let outcome = focus_outcome(
            FocusFacts::ProcessOnly,
            false,
            Some("this is a native Wayland session"),
        );
        match outcome {
            FocusOutcome::RunningCannotFocus(reason) => {
                assert!(reason.contains("Wayland"), "reason was: {reason}");
            }
            other => panic!("expected RunningCannotFocus, got {other:?}"),
        }
    }

    #[test]
    fn the_mapping_never_claims_the_app_is_not_running() {
        // "Not running" needs no mapping: it is simply NotRunning.
        assert_ne!(
            focus_outcome(FocusFacts::ProcessOnly, false, None),
            FocusOutcome::NotRunning
        );
        assert_ne!(
            focus_outcome(FocusFacts::WindowFound, true, None),
            FocusOutcome::NotRunning
        );
        assert_eq!(
            focus_outcome(FocusFacts::NotRunning, false, None),
            FocusOutcome::NotRunning
        );
    }
}
