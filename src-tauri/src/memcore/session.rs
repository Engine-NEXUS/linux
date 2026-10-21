//! Session snapshot (plan D): what was open on the PC, so that after a
//! power-off NEXUS can say "you had VS Code on ULTRON and Brave open — shall I
//! reopen them?" and actually do it once the user says yes.
//!
//! * `capture` runs every minute while NEXUS is up (and once more when Windows
//!   announces a logoff/shutdown, or when the user asks NEXUS to power off), so
//!   a manual power-off, a power cut and a voice shutdown are all covered: the
//!   periodic snapshots are the backbone, the end-of-session write only makes
//!   the timestamp exact.
//! * Everything here is pure data + rules except the Windows glue at the
//!   bottom. The rules decide what is *never* stored: NEXUS itself, shells and
//!   terminals, system chrome, sensitive windows (banks, password managers),
//!   private/incognito browser windows, protected (elevated) processes, and any
//!   command-line argument that is not an existing, non-sensitive path.
//! * Stored in the sealed Memory Core (`meta`), never sent to the cloud, and
//!   erased by "forget everything".
//! * Restoring is always voice-confirmed (see `restore.rs`).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::resume;

/// A snapshot older than this is not offered any more.
pub const MAX_AGE_SECS: i64 = 3 * 24 * 3600;
/// NEXUS was away at least this long, or the previous run said it ended the
/// session: otherwise this is just NEXUS restarting while the PC kept running.
pub const MIN_GAP_SECS: i64 = 180;
/// Unchanged snapshots are re-written at most this often (each write re-seals the db).
pub const REFRESH_SECS: i64 = 10 * 60;
const SNAPSHOT_EVERY_SECS: u64 = 60;
const MAX_APPS: usize = 24;
const MAX_FOLDERS: usize = 8;
const META_LAST: &str = "session_last";
const META_PENDING: &str = "session_pending";

pub const REASON_PERIODIC: &str = "periodic";
pub const REASON_SESSION_END: &str = "session_end";
pub const REASON_VOICE_POWER: &str = "voice_power";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionApp {
    /// Executable file name, lower-case ("code.exe").
    pub exe: String,
    /// Full path to the executable ("" for Store apps, launched by name).
    #[serde(default)]
    pub path: String,
    /// Existing, non-sensitive file/folder arguments only.
    #[serde(default)]
    pub args: Vec<String>,
    /// What to say: "VS Code on ULTRON", "Brave".
    pub label: String,
    #[serde(default)]
    pub windows: u32,
    #[serde(default)]
    pub packaged: bool,
    #[serde(default)]
    pub browser: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Unix seconds when the PC was last seen in this state.
    pub ts: i64,
    pub reason: String,
    pub apps: Vec<SessionApp>,
    /// Explorer folders that were open.
    pub folders: Vec<String>,
}

impl Snapshot {
    pub fn is_empty(&self) -> bool {
        self.apps.is_empty() && self.folders.is_empty()
    }

    /// Same content, ignoring the timestamp and the reason.
    pub fn same_content(&self, other: &Snapshot) -> bool {
        self.apps == other.apps && self.folders == other.folders
    }

    pub fn ended_session(&self) -> bool {
        self.reason == REASON_SESSION_END || self.reason == REASON_VOICE_POWER
    }
}

/// One top-level window with its process, as read from Windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawWindow {
    pub exe: String,
    pub path: String,
    pub cmd: Vec<String>,
    pub title: String,
}

// ─── Rules ───────────────────────────────────────────────────────────

/// Processes that are never part of "what was open".
const IGNORED_EXES: &[&str] = &[
    "nexus.exe",
    "explorer.exe", // folders are captured separately
    "applicationframehost.exe",
    "textinputhost.exe",
    "searchhost.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "systemsettings.exe",
    "lockapp.exe",
    "widgets.exe",
    "taskmgr.exe",
    "dwm.exe",
    "ctfmon.exe",
    // Terminals: restoring them would mean re-running commands.
    "windowsterminal.exe",
    "wt.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "conhost.exe",
    "openconsole.exe",
];

const INSTALLER_HINTS: &[&str] = &["setup", "install", "uninstall", "update", "msiexec"];

fn exe_is_ignored(exe: &str) -> bool {
    let e = exe.to_lowercase();
    IGNORED_EXES.contains(&e.as_str()) || INSTALLER_HINTS.iter().any(|h| e.contains(h))
}

fn is_private_browser_title(title: &str) -> bool {
    let t = title.to_lowercase();
    t.contains("incognito") || t.contains("inprivate") || t.contains("private browsing") || t.contains("(private)")
}

/// Paths that must never be written into a snapshot or passed back to an app.
pub fn path_is_sensitive(path: &str) -> bool {
    let p = path.to_lowercase();
    [
        ".ssh", "password", "passwd", "credential", "secret", ".kdbx", "keepass", "wallet", "bank", ".env",
        "id_rsa", "token", "private key", "1password", "bitwarden",
    ]
    .iter()
    .any(|k| p.contains(k))
}

fn is_packaged(path: &str) -> bool {
    path.to_lowercase().contains("\\windowsapps\\")
}

/// Keep only arguments that are existing, non-sensitive paths. Flags and
/// anything else (tokens, URLs, session ids) are dropped.
pub fn sanitize_args(cmd: &[String], exists: &dyn Fn(&str) -> bool) -> Vec<String> {
    cmd.iter()
        .skip(1)
        .map(|a| a.trim().trim_matches('"').to_string())
        .filter(|a| !a.is_empty() && !a.starts_with('-') && !a.starts_with('/'))
        .filter(|a| !path_is_sensitive(a) && exists(a))
        .take(3)
        .collect()
}

fn base_name(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_string()
}

fn is_editor_with_folders(exe: &str) -> bool {
    matches!(exe, "code.exe" | "cursor.exe" | "antigravity.exe" | "windsurf.exe" | "vscodium.exe")
}

/// Turn raw windows + side information into a snapshot. Pure.
pub fn build(
    raws: &[RawWindow],
    explorer: &[String],
    editor_folders: &[String],
    now: i64,
    reason: &str,
    exists: &dyn Fn(&str) -> bool,
) -> Snapshot {
    // exe -> (first path, first cmd, window count, titles)
    let mut order: Vec<String> = vec![];
    let mut groups: std::collections::HashMap<String, (String, Vec<String>, u32, Vec<String>)> = Default::default();

    for w in raws {
        let exe = w.exe.to_lowercase();
        if exe.is_empty() || exe_is_ignored(&exe) {
            continue;
        }
        if w.title.trim().is_empty() {
            continue;
        }
        if crate::screen_context::is_sensitive(&exe, &w.title, None) {
            continue;
        }
        if is_browser_exe(&exe) && is_private_browser_title(&w.title) {
            continue;
        }
        let g = groups.entry(exe.clone()).or_insert_with(|| {
            order.push(exe.clone());
            (w.path.clone(), w.cmd.clone(), 0, vec![])
        });
        g.2 += 1;
        g.3.push(w.title.clone());
    }

    let mut apps: Vec<SessionApp> = vec![];
    for exe in &order {
        let Some((path, cmd, windows, titles)) = groups.get(exe) else { continue };
        let friendly = resume::friendly_app(exe);
        let packaged = is_packaged(path);
        if is_browser_exe(exe) {
            apps.push(SessionApp {
                exe: exe.clone(),
                path: path.clone(),
                args: vec![],
                label: friendly,
                windows: *windows,
                packaged: false,
                browser: true,
            });
        } else if is_editor_with_folders(exe) {
            // One entry per folder whose name appears in a window title.
            let open: Vec<&String> = editor_folders
                .iter()
                .filter(|f| {
                    let b = base_name(f).to_lowercase();
                    !b.is_empty() && titles.iter().any(|t| t.to_lowercase().contains(&b))
                })
                .take(4)
                .collect();
            if open.is_empty() {
                apps.push(SessionApp {
                    exe: exe.clone(),
                    path: path.clone(),
                    args: vec![],
                    label: friendly.clone(),
                    windows: *windows,
                    packaged: false,
                    browser: false,
                });
            }
            for f in open {
                apps.push(SessionApp {
                    exe: exe.clone(),
                    path: path.clone(),
                    args: vec![f.clone()],
                    label: format!("{friendly} on {}", base_name(f)),
                    windows: 1,
                    packaged: false,
                    browser: false,
                });
            }
        } else {
            apps.push(SessionApp {
                exe: exe.clone(),
                path: if packaged { String::new() } else { path.clone() },
                args: if packaged { vec![] } else { sanitize_args(cmd, exists) },
                label: friendly,
                windows: *windows,
                packaged,
                browser: false,
            });
        }
    }
    apps.truncate(MAX_APPS);

    let mut seen = HashSet::new();
    let folders: Vec<String> = explorer
        .iter()
        .map(|f| f.trim().trim_end_matches(['\\', '/']).to_string())
        .filter(|f| f.len() > 3 && !f.starts_with("::") && !path_is_sensitive(f) && exists(f))
        .filter(|f| seen.insert(f.to_lowercase()))
        .take(MAX_FOLDERS)
        .collect();

    Snapshot { ts: now, reason: reason.to_string(), apps, folders }
}

fn is_browser_exe(exe: &str) -> bool {
    crate::browser_url::is_browser_process(exe)
}

// ─── Speech ──────────────────────────────────────────────────────────

fn join_and(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        2 => format!("{} and {}", items[0], items[1]),
        _ => format!("{} and {}", items[..items.len() - 1].join(", "), items[items.len() - 1]),
    }
}

/// "VS Code on ULTRON, Brave and 2 folders" from app names + a folder count. Pure.
pub fn names_phrase(mut names: Vec<String>, folders: usize) -> String {
    let mut seen = HashSet::new();
    names.retain(|n| seen.insert(n.to_lowercase()));
    let extra = names.len().saturating_sub(4);
    names.truncate(4);
    if extra > 0 {
        names.push(format!("{extra} more app{}", if extra == 1 { "" } else { "s" }));
    }
    if folders > 0 {
        names.push(format!("{folders} folder{}", if folders == 1 { "" } else { "s" }));
    }
    join_and(&names)
}

/// "VS Code on ULTRON, Brave and 2 folders". Pure.
pub fn summary(s: &Snapshot) -> String {
    let names: Vec<String> = s
        .apps
        .iter()
        .map(|a| if a.browser && a.windows > 1 { format!("{} with {} windows", a.label, a.windows) } else { a.label.clone() })
        .collect();
    names_phrase(names, s.folders.len())
}

// ─── Editor storage (VS Code / Cursor) ───────────────────────────────

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16)) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// "file:///c%3A/PROJECTS/ULTRON" -> "c:\PROJECTS\ULTRON". Remote schemes -> None.
pub fn uri_to_path(uri: &str) -> Option<String> {
    let u = uri.trim();
    if let Some(rest) = u.strip_prefix("file:///") {
        let p = percent_decode(rest);
        return Some(if cfg!(windows) { p.replace('/', "\\") } else { format!("/{p}") });
    }
    if u.contains("://") {
        return None; // vscode-remote:// etc.
    }
    if u.is_empty() { None } else { Some(u.to_string()) }
}

/// Folders VS Code's `storage.json` lists as open (state at its last save). Pure.
pub fn parse_editor_folders(json: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return vec![] };
    let mut out: Vec<String> = vec![];
    let mut push = |s: Option<&str>| {
        if let Some(p) = s.and_then(uri_to_path) {
            if !out.iter().any(|o| o.eq_ignore_ascii_case(&p)) {
                out.push(p);
            }
        }
    };
    let ws = &v["windowsState"];
    push(ws["lastActiveWindow"]["folder"].as_str());
    if let Some(arr) = ws["openedWindows"].as_array() {
        for w in arr {
            push(w["folder"].as_str());
        }
    }
    if let Some(arr) = v["backupWorkspaces"]["folders"].as_array() {
        for f in arr {
            push(f["folderUri"].as_str());
        }
    }
    out
}

// ─── Storage + boot decision ─────────────────────────────────────────

pub fn encode(s: &Snapshot) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

pub fn decode(v: &str) -> Option<Snapshot> {
    serde_json::from_str(v).ok()
}

fn load(dir: &std::path::Path, key: &str) -> Option<Snapshot> {
    super::with_store(dir, |s| s.meta_get(key)).flatten().and_then(|v| decode(&v))
}

pub fn load_last(dir: &std::path::Path) -> Option<Snapshot> {
    load(dir, META_LAST)
}

pub fn load_pending(dir: &std::path::Path) -> Option<Snapshot> {
    load(dir, META_PENDING).filter(|s| !s.is_empty())
}

pub fn clear_pending(dir: &std::path::Path) {
    super::with_store(dir, |s| s.meta_set(META_PENDING, ""));
}

/// Whether `last` was a session that ended (as opposed to NEXUS restarting
/// while the PC kept running). Pure.
pub fn should_hold_for_boot(last: &Snapshot, now: i64) -> bool {
    if last.is_empty() || now - last.ts > MAX_AGE_SECS {
        return false;
    }
    last.ended_session() || now - last.ts >= MIN_GAP_SECS
}

/// Call once at startup BEFORE the first periodic snapshot overwrites
/// `session_last`: a snapshot of an ended session becomes the pending offer.
pub fn at_boot(dir: &std::path::Path, now: i64) {
    let Some(last) = load_last(dir) else { return };
    if !should_hold_for_boot(&last, now) {
        return;
    }
    let newer = load_pending(dir).map(|p| last.ts > p.ts).unwrap_or(true);
    if newer {
        super::with_store(dir, |s| s.meta_set(META_PENDING, &encode(&last)));
    }
}

/// Write `snap` as the rolling snapshot unless it is unchanged and recent.
/// Returns whether it wrote.
pub fn save_last(dir: &std::path::Path, snap: &Snapshot, previous: Option<&Snapshot>, force: bool) -> bool {
    if let Some(p) = previous {
        if !force && p.same_content(snap) && p.reason == snap.reason && snap.ts - p.ts < REFRESH_SECS {
            return false;
        }
    }
    super::with_store(dir, |s| s.meta_set(META_LAST, &encode(snap))).is_some()
}

// ─── Windows glue ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod win {
    use super::*;
    use std::sync::Mutex;

    /// Explorer folder paths are read through PowerShell (slow, ~0.5 s), so
    /// they are cached and refreshed at most every 5 minutes.
    static EXPLORER_CACHE: Mutex<Option<(i64, Vec<String>)>> = Mutex::new(None);

    pub fn enumerate_windows() -> Vec<RawWindow> {
        use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindow, GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
            IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_TOOLWINDOW,
        };

        struct Item {
            pid: u32,
            title: String,
        }
        let mut items: Vec<Item> = vec![];

        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let items = &mut *(lparam.0 as *mut Vec<Item>);
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            // Dialogs and owned popups belong to another window.
            if GetWindow(hwnd, GW_OWNER).0 != 0 {
                return BOOL(1);
            }
            if (GetWindowLongW(hwnd, GWL_EXSTYLE) as u32) & WS_EX_TOOLWINDOW.0 != 0 {
                return BOOL(1);
            }
            let _ = IsIconic(hwnd); // minimised windows still count as open
            let mut buf = [0u16; 512];
            let n = GetWindowTextW(hwnd, &mut buf);
            if n <= 0 {
                return BOOL(1);
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            items.push(Item { pid, title: String::from_utf16_lossy(&buf[..n as usize]) });
            BOOL(1)
        }

        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut items as *mut Vec<Item> as isize));
        }

        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let pids: Vec<sysinfo::Pid> = items.iter().map(|i| sysinfo::Pid::from_u32(i.pid)).collect();
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::new().with_exe(UpdateKind::Always).with_cmd(UpdateKind::Always),
        );

        let mut out = vec![];
        for it in items {
            let Some(p) = sys.process(sysinfo::Pid::from_u32(it.pid)) else { continue };
            // No readable executable = a protected / elevated process: never restored.
            let Some(path) = p.exe() else { continue };
            let exe = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            out.push(RawWindow {
                exe,
                path: path.to_string_lossy().into_owned(),
                cmd: p.cmd().iter().map(|a| a.to_string_lossy().into_owned()).collect(),
                title: it.title,
            });
        }
        out
    }

    fn editor_storage_json() -> String {
        let Ok(appdata) = std::env::var("APPDATA") else { return String::new() };
        let mut all = String::new();
        for sub in ["Code", "Cursor", "Antigravity", "Windsurf", "VSCodium"] {
            let p = std::path::Path::new(&appdata).join(sub).join("User").join("globalStorage").join("storage.json");
            if let Ok(s) = std::fs::read_to_string(p) {
                // Merge by parsing separately and concatenating folder lists later.
                all.push_str(&s);
                all.push('\u{1}');
            }
        }
        all
    }

    pub fn editor_folders() -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for part in editor_storage_json().split('\u{1}').filter(|p| !p.trim().is_empty()) {
            for f in parse_editor_folders(part) {
                if !out.iter().any(|o| o.eq_ignore_ascii_case(&f)) {
                    out.push(f);
                }
            }
        }
        out
    }

    fn read_explorer_folders() -> Vec<String> {
        use std::os::windows::process::CommandExt;
        let script = "$s=New-Object -ComObject Shell.Application; \
                      $s.Windows() | ForEach-Object { try { $_.Document.Folder.Self.Path } catch {} }";
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let out = std::process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", script])
                .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
                .output();
            let _ = tx.send(out);
        });
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Ok(o)) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
            _ => vec![],
        }
    }

    pub fn explorer_folders(now: i64, has_explorer_windows: bool, force: bool) -> Vec<String> {
        if !has_explorer_windows {
            *EXPLORER_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
            return vec![];
        }
        {
            let g = EXPLORER_CACHE.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((ts, v)) = g.as_ref() {
                if !force && now - *ts < 300 {
                    return v.clone();
                }
            }
        }
        let v = read_explorer_folders();
        *EXPLORER_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((now, v.clone()));
        v
    }

    /// Whether any File Explorer window exists (cheap: class-name check via title heuristics is
    /// unreliable, so use the process: explorer.exe with a visible top-level window that is not the
    /// desktop/taskbar).
    pub fn has_explorer_windows() -> bool {
        use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, IsWindowVisible};
        let mut found = false;
        unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let found = &mut *(lparam.0 as *mut bool);
            if IsWindowVisible(hwnd).as_bool() {
                let mut buf = [0u16; 64];
                let n = GetClassNameW(hwnd, &mut buf);
                if n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == "CabinetWClass" {
                    *found = true;
                    return BOOL(0);
                }
            }
            BOOL(1)
        }
        unsafe {
            let _ = EnumWindows(Some(cb), LPARAM(&mut found as *mut bool as isize));
        }
        found
    }
}

/// Capture the current session. Blocking (process enumeration; the first call
/// may start PowerShell for Explorer folders).
#[cfg(target_os = "windows")]
pub fn capture(reason: &str, now: i64, force_explorer: bool) -> Snapshot {
    let raws = win::enumerate_windows();
    let explorer = win::explorer_folders(now, win::has_explorer_windows(), force_explorer);
    let editors = win::editor_folders();
    build(&raws, &explorer, &editors, now, reason, &|p| std::path::Path::new(p).exists())
}

#[cfg(not(target_os = "windows"))]
pub fn capture(reason: &str, now: i64, _force_explorer: bool) -> Snapshot {
    Snapshot { ts: now, reason: reason.to_string(), apps: vec![], folders: vec![] }
}

static LAST_WRITTEN: std::sync::Mutex<Option<Snapshot>> = std::sync::Mutex::new(None);

/// Capture + store now. `reason` other than periodic always writes.
pub fn snapshot_now(dir: &std::path::Path, reason: &str) -> Option<Snapshot> {
    if !super::enabled(dir) || !super::flag(dir, "memcoreSession", true) {
        return None;
    }
    let now = chrono::Utc::now().timestamp();
    let force = reason != REASON_PERIODIC;
    let snap = capture(reason, now, force);
    // A momentarily empty desktop (lock screen, screen transition, shutdown in progress)
    // must never erase the last real session.
    if snap.is_empty() {
        return None;
    }
    let prev = LAST_WRITTEN.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if save_last(dir, &snap, prev.as_ref(), force) {
        *LAST_WRITTEN.lock().unwrap_or_else(|e| e.into_inner()) = Some(snap.clone());
    }
    Some(snap)
}

/// Start the one-minute snapshot thread (std thread: enumeration is blocking).
pub fn start_recorder(dir: std::path::PathBuf) {
    std::thread::Builder::new()
        .name("session-snapshot".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(SNAPSHOT_EVERY_SECS));
            // Do not record while the user is away for a long time: nothing changed and
            // the lock screen would only look like an empty desktop.
            if resume::user_idle_secs().map(|s| s >= 30 * 60).unwrap_or(false) {
                continue;
            }
            let _ = snapshot_now(&dir, REASON_PERIODIC);
        })
        .ok();
}

/// Hidden top-level window that receives Windows' logoff/shutdown broadcast
/// (message-only windows do not get it) and writes the final snapshot.
#[cfg(target_os = "windows")]
pub fn install_session_end_hook(dir: std::path::PathBuf) {
    use std::sync::OnceLock;
    static DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    let _ = DIR.set(dir);

    std::thread::Builder::new()
        .name("session-end-hook".into())
        .spawn(|| unsafe {
            use windows::core::PCWSTR;
            use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
            use windows::Win32::System::LibraryLoader::GetModuleHandleW;
            use windows::Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage, MSG,
                WINDOW_EX_STYLE, WINDOW_STYLE, WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW,
            };

            unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
                if msg == WM_QUERYENDSESSION {
                    // Windows is about to log off / shut down: write the snapshot while the
                    // windows still exist. Never block the shutdown (return TRUE).
                    if let Some(dir) = DIR.get() {
                        let _ = snapshot_now(dir, REASON_SESSION_END);
                    }
                    println!("[SESSION] session end announced - snapshot written");
                    return LRESULT(1);
                }
                if msg == WM_ENDSESSION {
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }

            let class: Vec<u16> = "NexusSessionEnd\0".encode_utf16().collect();
            let hinst = GetModuleHandleW(PCWSTR(std::ptr::null())).unwrap_or(HINSTANCE(0));
            let wc = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: hinst,
                lpszClassName: PCWSTR(class.as_ptr()),
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                tracing::warn!("session-end hook: RegisterClassW failed");
                return;
            }
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class.as_ptr()),
                PCWSTR(class.as_ptr()),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                HWND(0),
                None,
                hinst,
                std::ptr::null(),
            );
            if hwnd.0 == 0 {
                tracing::warn!("session-end hook: CreateWindowExW failed");
                return;
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND(0), 0, 0).as_bool() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .ok();
}

#[cfg(not(target_os = "windows"))]
pub fn install_session_end_hook(_dir: std::path::PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(exe: &str, path: &str, cmd: &[&str], title: &str) -> RawWindow {
        RawWindow {
            exe: exe.into(),
            path: path.into(),
            cmd: cmd.iter().map(|s| s.to_string()).collect(),
            title: title.into(),
        }
    }

    fn all_exist(_: &str) -> bool {
        true
    }

    #[test]
    fn nexus_shells_terminals_and_chrome_are_never_stored() {
        let raws = vec![
            raw("nexus.exe", r"C:\n\nexus.exe", &[], "NEXUS"),
            raw("explorer.exe", r"C:\Windows\explorer.exe", &[], "Downloads"),
            raw("WindowsTerminal.exe", r"C:\wt.exe", &[], "pwsh"),
            raw("cmd.exe", r"C:\cmd.exe", &[], "cmd"),
            raw("ApplicationFrameHost.exe", r"C:\a.exe", &[], "Settings"),
            raw("setup.exe", r"C:\x\setup.exe", &[], "Installer"),
            raw("notepad.exe", r"C:\Windows\notepad.exe", &["notepad.exe"], "todo.txt - Notepad"),
        ];
        let s = build(&raws, &[], &[], 100, REASON_PERIODIC, &all_exist);
        assert_eq!(s.apps.len(), 1, "{:?}", s.apps);
        assert_eq!(s.apps[0].exe, "notepad.exe");
    }

    #[test]
    fn sensitive_and_private_windows_are_dropped() {
        let raws = vec![
            raw("1password.exe", r"C:\1p.exe", &[], "1Password"),
            raw("brave.exe", r"C:\b\brave.exe", &[], "Sign in - PayPal"),
            raw("brave.exe", r"C:\b\brave.exe", &[], "Secret stuff - Brave (Private)"),
        ];
        let s = build(&raws, &[], &[], 100, REASON_PERIODIC, &all_exist);
        // 1Password dropped; the PayPal window is sensitive; the private window is private.
        assert!(s.apps.iter().all(|a| a.exe != "1password.exe"), "{:?}", s.apps);
        assert!(s.apps.iter().all(|a| !(a.browser && a.windows > 0)), "browser kept only for private/sensitive windows: {:?}", s.apps);
    }

    #[test]
    fn browser_windows_are_counted_not_listed() {
        let raws = vec![
            raw("brave.exe", r"C:\b\brave.exe", &["brave.exe", "--flag=secret"], "Docs - Brave"),
            raw("brave.exe", r"C:\b\brave.exe", &[], "YouTube - Brave"),
        ];
        let s = build(&raws, &[], &[], 100, REASON_PERIODIC, &all_exist);
        assert_eq!(s.apps.len(), 1);
        assert!(s.apps[0].browser && s.apps[0].windows == 2 && s.apps[0].args.is_empty());
        assert_eq!(summary(&s), "Brave with 2 windows");
    }

    #[test]
    fn arguments_keep_only_existing_non_sensitive_paths() {
        let cmd: Vec<String> = ["app.exe", "--token=abc", r"C:\work\a.txt", r"C:\.ssh\id_rsa", r"C:\gone.txt", "https://x.y/z"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let exists = |p: &str| p != r"C:\gone.txt" && !p.starts_with("http");
        assert_eq!(sanitize_args(&cmd, &exists), vec![r"C:\work\a.txt".to_string()]);
    }

    #[test]
    fn editors_get_one_entry_per_open_folder() {
        let raws = vec![raw("Code.exe", r"C:\vs\Code.exe", &[], "main.rs - ULTRON - Visual Studio Code")];
        let folders = vec![r"c:\PROJECTS\ULTRON".to_string(), r"c:\PROJECTS\OTHER".to_string()];
        let s = build(&raws, &[], &folders, 100, REASON_PERIODIC, &all_exist);
        assert_eq!(s.apps.len(), 1, "only the folder named in a title is open: {:?}", s.apps);
        assert_eq!(s.apps[0].label, "VS Code on ULTRON");
        assert_eq!(s.apps[0].args, vec![r"c:\PROJECTS\ULTRON".to_string()]);
    }

    #[test]
    fn store_apps_are_launched_by_name_not_path() {
        let raws = vec![raw(
            "WhatsApp.Root.exe",
            r"C:\Program Files\WindowsApps\5319275A.WhatsAppDesktop_1\WhatsApp.Root.exe",
            &[],
            "WhatsApp",
        )];
        let s = build(&raws, &[], &[], 100, REASON_PERIODIC, &all_exist);
        assert!(s.apps[0].packaged && s.apps[0].path.is_empty());
    }

    #[test]
    fn folders_are_checked_cleaned_and_deduped() {
        let exp = vec![
            r"C:\Users\me\Documents\".to_string(),
            r"c:\users\me\documents".to_string(),
            "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}".to_string(),
            r"C:\Users\me\.ssh".to_string(),
            r"C:\gone".to_string(),
        ];
        let exists = |p: &str| p != r"C:\gone";
        let s = build(&[], &exp, &[], 1, REASON_PERIODIC, &exists);
        assert_eq!(s.folders, vec![r"C:\Users\me\Documents".to_string()]);
    }

    #[test]
    fn summary_reads_naturally() {
        let mk = |label: &str, browser: bool, windows: u32| SessionApp {
            exe: format!("{label}.exe"),
            path: String::new(),
            args: vec![],
            label: label.into(),
            windows,
            packaged: false,
            browser,
        };
        let s = Snapshot {
            ts: 1,
            reason: REASON_PERIODIC.into(),
            apps: vec![mk("VS Code on ULTRON", false, 1), mk("Brave", true, 1)],
            folders: vec!["a".into(), "b".into()],
        };
        assert_eq!(summary(&s), "VS Code on ULTRON, Brave and 2 folders");
        let big = Snapshot {
            apps: (0..7).map(|i| mk(&format!("App{i}"), false, 1)).collect(),
            folders: vec![],
            ..s.clone()
        };
        assert_eq!(summary(&big), "App0, App1, App2, App3 and 3 more apps");
    }

    #[test]
    fn editor_storage_is_parsed_and_remote_folders_skipped() {
        let json = r#"{
          "windowsState": {
            "lastActiveWindow": {"folder": "file:///c%3A/PROJECTS/ULTRON"},
            "openedWindows": [{"folder": "file:///c%3A/PROJECTS/ULTRON"}, {"folder": "vscode-remote://ssh-remote+x/home/me"}]
          },
          "backupWorkspaces": {"folders": [{"folderUri": "file:///d%3A/My%20Stuff/app"}]}
        }"#;
        let f = parse_editor_folders(json);
        assert_eq!(f.len(), 2, "{f:?}");
        if cfg!(windows) {
            assert_eq!(f[0], r"c:\PROJECTS\ULTRON");
            assert_eq!(f[1], r"d:\My Stuff\app");
        }
        assert!(parse_editor_folders("not json").is_empty());
    }

    #[test]
    fn boot_decision_distinguishes_a_power_off_from_a_nexus_restart() {
        let mk = |ts: i64, reason: &str, with_app: bool| Snapshot {
            ts,
            reason: reason.into(),
            apps: if with_app {
                vec![SessionApp { exe: "a.exe".into(), path: "x".into(), args: vec![], label: "A".into(), windows: 1, packaged: false, browser: false }]
            } else {
                vec![]
            },
            folders: vec![],
        };
        let now = 1_000_000;
        assert!(should_hold_for_boot(&mk(now - 3600, REASON_PERIODIC, true), now), "an hour ago: PC was off");
        assert!(!should_hold_for_boot(&mk(now - 60, REASON_PERIODIC, true), now), "a minute ago: only NEXUS restarted");
        assert!(should_hold_for_boot(&mk(now - 30, REASON_SESSION_END, true), now), "an announced session end always counts");
        assert!(should_hold_for_boot(&mk(now - 30, REASON_VOICE_POWER, true), now));
        assert!(!should_hold_for_boot(&mk(now - 3600, REASON_PERIODIC, false), now), "nothing to restore");
        assert!(!should_hold_for_boot(&mk(now - MAX_AGE_SECS - 1, REASON_SESSION_END, true), now), "too old");
    }

    #[test]
    fn unchanged_snapshots_do_not_rewrite_the_database_every_minute() {
        let dir = std::env::temp_dir().join(format!("nexus_session_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = Snapshot { ts: 1000, reason: REASON_PERIODIC.into(), apps: vec![], folders: vec!["x".into()] };
        assert!(save_last(&dir, &a, None, false));
        let b = Snapshot { ts: 1060, ..a.clone() };
        assert!(!save_last(&dir, &b, Some(&a), false), "same content, 60 s later");
        let after_cancel = Snapshot { reason: REASON_VOICE_POWER.into(), ..b.clone() };
        assert!(save_last(&dir, &a, Some(&after_cancel), false), "a periodic snapshot replaces a voice_power one (shutdown was cancelled)");
        let c = Snapshot { ts: 1000 + REFRESH_SECS, ..a.clone() };
        assert!(save_last(&dir, &c, Some(&a), false), "same content but stale timestamp");
        let d = Snapshot { folders: vec!["y".into()], ..b.clone() };
        assert!(save_last(&dir, &d, Some(&a), false), "changed content");
        assert!(save_last(&dir, &b, Some(&a), true), "forced");
        assert_eq!(load_last(&dir).unwrap().ts, 1060);
    }

    #[test]
    fn at_boot_moves_an_ended_session_to_pending_and_never_older_over_newer() {
        let dir = std::env::temp_dir().join(format!("nexus_session_boot_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let app = SessionApp { exe: "a.exe".into(), path: "x".into(), args: vec![], label: "A".into(), windows: 1, packaged: false, browser: false };
        let old = Snapshot { ts: 1_000, reason: REASON_SESSION_END.into(), apps: vec![app.clone()], folders: vec![] };
        assert!(save_last(&dir, &old, None, true));
        at_boot(&dir, 5_000);
        assert_eq!(load_pending(&dir).unwrap().ts, 1_000);

        // The next boot only has a thin session: it must not replace the richer pending one with an OLDER ts,
        // but a newer ended session does replace it.
        let newer = Snapshot { ts: 4_000, reason: REASON_PERIODIC.into(), apps: vec![app], folders: vec![] };
        assert!(save_last(&dir, &newer, None, true));
        at_boot(&dir, 9_000);
        assert_eq!(load_pending(&dir).unwrap().ts, 4_000);
        clear_pending(&dir);
        assert!(load_pending(&dir).is_none());
    }
}

/// Dev probe: `cargo test --lib -- --ignored session_live_probe --nocapture`.
/// Prints only app labels and counts - never window titles or arguments.
#[cfg(all(test, target_os = "windows"))]
mod live_probe {
    use super::*;

    #[test]
    #[ignore]
    fn session_live_probe() {
        let t = std::time::Instant::now();
        let s = capture(REASON_PERIODIC, chrono::Utc::now().timestamp(), true);
        println!("captured in {} ms", t.elapsed().as_millis());
        println!("apps: {}", s.apps.len());
        for a in &s.apps {
            println!(
                "  {:<28} exe={:<26} windows={} args={} packaged={} browser={}",
                a.label, a.exe, a.windows, a.args.len(), a.packaged, a.browser
            );
        }
        println!("folders: {}", s.folders.len());
        println!("summary: {}", summary(&s));
    }
}
