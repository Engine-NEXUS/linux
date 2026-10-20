//! Graceful "close everything" step of the voice shutdown ("Nexus, shut down my laptop").
//!
//! Every ordinary top-level window gets `WM_CLOSE` (what the X button sends), never a kill. An app with
//! unsaved work answers with its own "save?" prompt and keeps its window: that window is still there
//! after the wait, so the shutdown is stopped and announced instead of throwing the work away.
//!
//! The decision logic is pure and tested against a fake window list (`CloseEnv`); only `RealEnv`
//! touches Win32.

/// One top-level window as the closer sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Win {
    pub hwnd: isize,
    /// Lower-case executable file name ("notepad.exe"); empty = unreadable (protected / elevated).
    pub exe: String,
    pub class: String,
    pub title: String,
}

/// Never closed: the shell, system UI, security and logon processes, and NEXUS itself.
const NEVER_CLOSE_EXES: &[&str] = &[
    "nexus.exe",
    "searchhost.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "textinputhost.exe",
    "lockapp.exe",
    "logonui.exe",
    "winlogon.exe",
    "csrss.exe",
    "dwm.exe",
    "securityhealthsystray.exe",
    "sihost.exe",
    "ctfmon.exe",
];

/// Shell window classes (desktop, taskbar, tray overflow): never closed whatever process owns them.
const SHELL_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "NotifyIconOverflowWindow",
    "TopLevelWindowForOverflowXamlIsland",
    "XamlExplorerHostIslandWindow",
];

/// Is this window one the shutdown may ask to close?
pub fn closable(w: &Win) -> bool {
    if w.exe.is_empty() || w.title.trim().is_empty() {
        return false;
    }
    if NEVER_CLOSE_EXES.contains(&w.exe.as_str()) || SHELL_CLASSES.contains(&w.class.as_str()) {
        return false;
    }
    // explorer.exe owns the desktop and taskbar too: only real folder windows may be closed.
    if w.exe == "explorer.exe" && w.class != "CabinetWClass" {
        return false;
    }
    true
}

/// "notepad.exe" -> "Notepad".
pub fn friendly(exe: &str) -> String {
    let stem = exe.trim_end_matches(".exe");
    let mut c = stem.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Everything that was asked to close has closed.
    AllClosed { closed: usize },
    /// Some windows are still open after the wait (usually a "save your work?" prompt).
    Refused { closed: usize, still_open: Vec<String> },
}

pub trait CloseEnv {
    fn list(&self) -> Vec<Win>;
    fn close(&self, w: &Win);
    fn sleep_ms(&self, ms: u64);
    /// How often the window set is re-checked while waiting for the closes
    /// to land. Windows polls fast (a `EnumWindows` call is free); Linux
    /// spawns `wmctrl`, so it polls slower. The default keeps the Windows
    /// behaviour exactly as it was.
    fn poll_ms(&self) -> u64 {
        POLL_MS
    }
}

const POLL_MS: u64 = 250;

/// Ask every closable window to close, then wait up to `wait_ms` for them to go.
pub fn close_all(env: &dyn CloseEnv, wait_ms: u64) -> Outcome {
    let targets: Vec<Win> = env.list().into_iter().filter(closable).collect();
    for w in &targets {
        env.close(w);
    }
    let poll = env.poll_ms();
    let mut waited = 0u64;
    loop {
        env.sleep_ms(poll);
        waited += poll;
        let now = env.list();
        let remaining: Vec<&Win> = targets.iter().filter(|t| now.iter().any(|n| n.hwnd == t.hwnd)).collect();
        if remaining.is_empty() {
            return Outcome::AllClosed { closed: targets.len() };
        }
        if waited >= wait_ms {
            let mut names: Vec<String> = vec![];
            for r in &remaining {
                let f = friendly(&r.exe);
                if !names.contains(&f) {
                    names.push(f);
                }
            }
            return Outcome::Refused { closed: targets.len() - remaining.len(), still_open: names };
        }
    }
}

/// What NEXUS says when the shutdown was stopped because windows stayed open.
pub fn refusal_line(still_open: &[String]) -> String {
    match still_open {
        [] => "Something is still open, sir, so I've stopped the shutdown.".to_string(),
        [one] => format!("{one} is asking to save, sir. I've left it open and stopped the shutdown."),
        [a, b] => format!("{a} and {b} are asking to save, sir. I've left them open and stopped the shutdown."),
        [a, b, rest @ ..] => format!(
            "{a}, {b} and {} more are asking to save, sir. I've left them open and stopped the shutdown.",
            rest.len()
        ),
    }
}

// ─── Windows glue ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub struct RealEnv;

#[cfg(target_os = "windows")]
impl CloseEnv for RealEnv {
    fn list(&self) -> Vec<Win> {
        use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetWindow, GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId,
            IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_TOOLWINDOW,
        };

        struct Item {
            hwnd: isize,
            pid: u32,
            class: String,
            title: String,
        }
        let mut items: Vec<Item> = vec![];

        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let items = &mut *(lparam.0 as *mut Vec<Item>);
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            // Dialogs and owned popups belong to another window (a "save?" prompt must not count as an app).
            if GetWindow(hwnd, GW_OWNER).0 != 0 {
                return BOOL(1);
            }
            if (GetWindowLongW(hwnd, GWL_EXSTYLE) as u32) & WS_EX_TOOLWINDOW.0 != 0 {
                return BOOL(1);
            }
            let mut tbuf = [0u16; 512];
            let n = GetWindowTextW(hwnd, &mut tbuf);
            if n <= 0 {
                return BOOL(1);
            }
            let mut cbuf = [0u16; 128];
            let cn = GetClassNameW(hwnd, &mut cbuf);
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            items.push(Item {
                hwnd: hwnd.0,
                pid,
                class: if cn > 0 { String::from_utf16_lossy(&cbuf[..cn as usize]) } else { String::new() },
                title: String::from_utf16_lossy(&tbuf[..n as usize]),
            });
            BOOL(1)
        }

        // SAFETY: `items` outlives the synchronous EnumWindows call; the callback only pushes to it.
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut items as *mut Vec<Item> as isize));
        }

        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let pids: Vec<sysinfo::Pid> = items.iter().map(|i| sysinfo::Pid::from_u32(i.pid)).collect();
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::new().with_exe(UpdateKind::Always),
        );
        let own = std::process::id();
        items
            .into_iter()
            .map(|it| {
                let exe = if it.pid == own {
                    "nexus.exe".to_string()
                } else {
                    sys.process(sysinfo::Pid::from_u32(it.pid))
                        .and_then(|p| p.exe())
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_lowercase())
                        .unwrap_or_default()
                };
                Win { hwnd: it.hwnd, exe, class: it.class, title: it.title }
            })
            .collect()
    }

    fn close(&self, w: &Win) {
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        // SAFETY: posting a message to a window handle is always safe; a stale handle simply fails.
        unsafe {
            let _ = PostMessageW(HWND(w.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }

    fn sleep_ms(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

// ─── Linux ────────────────────────────────────────────────────────────
//
// The Windows path above asks every top-level window to close the way its X
// button does. Linux has no equivalent API: the closest thing is `wmctrl -c`,
// which asks an *X11* window to close, and only X11 windows are visible to
// it. On a native Wayland session (COSMIC, GNOME Wayland) no client may ask
// another application to close at all — so the honest answer there is "I can't
// close your other apps", not a silent skip.
//
// `wmctrl` is optional on most desktops (it is not installed on the target
// machine), so its absence is a first-class case too.

#[cfg(target_os = "linux")]
pub mod linux {
    use super::{CloseEnv, Win};

    /// What the "close everything" step can do in this session.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Strategy {
        /// X11 session with `wmctrl`: the windows can be asked to close.
        Wmctrl,
        /// Native Wayland: no client may ask another application to close.
        Wayland,
        /// X11 session, but `wmctrl` is not installed.
        NoTool,
    }

    /// Why the other apps cannot be closed — spoken to the user.
    pub const WAYLAND_REASON: &str =
        "I can't close your other apps on Wayland, sir — anything unsaved will be lost.";
    pub const NO_TOOL_REASON: &str =
        "I can't ask your other apps to close, sir — anything unsaved will be lost.";
    /// `wmctrl` is installed but cannot talk to the X server.
    pub const TOOL_BROKEN_REASON: &str =
        "I can't reach your window manager, sir — anything unsaved will be lost.";

    /// The pure decision (unit-tested): can we ask the other apps to close?
    ///
    /// A Wayland session cannot, whatever is installed — `wmctrl` only sees
    /// X11 windows, and the native Wayland apps (which is what the user runs)
    /// are invisible to it. An X11 session can, provided `wmctrl` is there.
    pub fn strategy(session_is_wayland: bool, has_wmctrl: bool) -> Strategy {
        if session_is_wayland {
            Strategy::Wayland
        } else if has_wmctrl {
            Strategy::Wmctrl
        } else {
            Strategy::NoTool
        }
    }

    /// Can this session ask the other apps to close? `Err(reason)` when it
    /// cannot — the caller speaks the reason and only continues on the
    /// user's confirmation.
    ///
    /// This is a real probe, not a "is the binary installed" check: `wmctrl`
    /// can be present while the X server is unreachable, and then it closes
    /// nothing at all. That must read as "cannot", never as "nothing open".
    pub fn can_close_apps() -> Result<(), String> {
        match strategy(crate::session::is_wayland(), wmctrl::installed()) {
            Strategy::Wmctrl => match wmctrl::list() {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("{TOOL_BROKEN_REASON} ({e})")),
            },
            Strategy::Wayland => Err(WAYLAND_REASON.to_string()),
            Strategy::NoTool => Err(NO_TOOL_REASON.to_string()),
        }
    }

    /// The result of the Linux close step.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum CloseResult {
        /// Everything that was asked to close has closed.
        AllClosed { closed: usize },
        /// Some windows are still open (usually a "save your work?" prompt).
        Refused { closed: usize, still_open: Vec<String> },
        /// This session cannot ask the other apps to close at all (native
        /// Wayland, or no `wmctrl`) — nothing was closed.
        CannotClose { reason: String },
        /// The tool that can close windows is not working — nothing was
        /// closed and we must say so instead of reporting a clean shutdown.
        ToolFailed { reason: String },
    }

    /// What NEXUS says when the other apps could not be closed at all.
    pub fn cannot_close_line() -> String {
        "I couldn't close your other apps, sir.".to_string()
    }

    /// What NEXUS says when the window tool itself failed, so we cannot even
    /// tell whether the apps closed.
    pub fn tool_failed_line() -> String {
        "I couldn't check your other apps, sir.".to_string()
    }

    /// The X11 windows `wmctrl` can see, as the closer sees them.
    ///
    /// `last_error` remembers a failed listing: an empty window list and a
    /// broken `wmctrl` look the same from the outside, and the difference is
    /// the difference between "nothing was open" and "we closed nothing".
    pub struct LinuxEnv {
        last_error: std::cell::RefCell<Option<String>>,
    }

    impl LinuxEnv {
        pub fn new() -> Self {
            Self { last_error: std::cell::RefCell::new(None) }
        }

        /// The last listing failure, if the last listing failed.
        pub fn last_error(&self) -> Option<String> {
            self.last_error.borrow().clone()
        }
    }

    impl CloseEnv for LinuxEnv {
        fn list(&self) -> Vec<Win> {
            let windows = match wmctrl::list() {
                Ok(w) => {
                    *self.last_error.borrow_mut() = None;
                    w
                }
                Err(e) => {
                    *self.last_error.borrow_mut() = Some(e);
                    return vec![];
                }
            };
            windows
                .into_iter()
                .filter_map(|w| {
                    let title = w.title.trim();
                    if title.is_empty() {
                        return None;
                    }
                    // NEXUS's own windows are never closed — the stage hosts
                    // the orb, and closing it would take the overlay down.
                    if title.starts_with("NEXUS") {
                        return None;
                    }
                    let hwnd = parse_window_id(&w.id)?;
                    Some(Win {
                        hwnd,
                        // wmctrl reports no executable name; the title is the
                        // only thing a user would recognise ("Firefox is
                        // asking to save, sir.").
                        exe: title.to_string(),
                        class: String::new(),
                        title: title.to_string(),
                    })
                })
                .collect()
        }

        fn close(&self, w: &Win) {
            wmctrl::close(w.hwnd);
        }

        fn sleep_ms(&self, ms: u64) {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }

        /// Every poll on Linux spawns a `wmctrl` process, so it polls far
        /// less often than the (free) Windows `EnumWindows` poll.
        fn poll_ms(&self) -> u64 {
            1_000
        }
    }

    /// Close everything, and report honestly what happened.
    ///
    /// A `wmctrl` that cannot list the windows is a FAILURE, not an empty
    /// room: the caller must not power the machine off while believing the
    /// apps were closed.
    pub fn close_everything(wait_ms: u64) -> CloseResult {
        match strategy(crate::session::is_wayland(), wmctrl::installed()) {
            // Native Wayland: no client may ask another application to close.
            Strategy::Wayland => {
                return CloseResult::CannotClose { reason: WAYLAND_REASON.to_string() }
            }
            // No `wmctrl`: there is no way to ask an X11 window to close.
            Strategy::NoTool => {
                return CloseResult::CannotClose { reason: NO_TOOL_REASON.to_string() }
            }
            Strategy::Wmctrl => {}
        }
        let env = LinuxEnv::new();
        let outcome = super::close_all(&env, wait_ms);
        if let Some(reason) = env.last_error() {
            return CloseResult::ToolFailed {
                reason: format!("{TOOL_BROKEN_REASON} ({reason})"),
            };
        }
        match outcome {
            super::Outcome::AllClosed { closed } => CloseResult::AllClosed { closed },
            super::Outcome::Refused { closed, still_open } => CloseResult::Refused { closed, still_open },
        }
    }

    /// "0x03a00003" -> the window id as a number.
    pub fn parse_window_id(id: &str) -> Option<isize> {
        let hex = id.trim().trim_start_matches("0x").trim_start_matches("0X");
        i64::from_str_radix(hex, 16).ok().map(|v| v as isize)
    }

    /// The `wmctrl` command line.
    pub mod wmctrl {
        /// One window as `wmctrl -l` reports it.
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct Window {
            pub id: String,
            pub title: String,
        }

        /// Pure parser for one `wmctrl -l` line:
        /// `0x03a00003  0  hostname  Firefox - Private` -> id + title.
        ///
        /// The line is `id desktop hostname title`, the title being the rest
        /// of the line (it may contain spaces). The id is hexadecimal.
        pub fn parse_line(line: &str) -> Option<Window> {
            let mut rest = line.trim_start();
            let id = take_field(&mut rest)?;
            if !id.starts_with("0x") {
                return None;
            }
            let _desktop = take_field(&mut rest)?;
            let _host = take_field(&mut rest)?;
            let title = rest.trim().to_string();
            if title.is_empty() {
                return None;
            }
            Some(Window { id, title })
        }

        /// The next whitespace-delimited field, advancing `rest`.
        fn take_field(rest: &mut &str) -> Option<String> {
            let s = rest.trim_start();
            if s.is_empty() {
                return None;
            }
            let end = s.find(char::is_whitespace).unwrap_or(s.len());
            let field = s[..end].to_string();
            *rest = &s[end..];
            Some(field)
        }

        /// The windows the X server knows about.
        ///
        /// A failure is an ERROR, never an empty list: "wmctrl could not talk
        /// to the X server" and "no windows are open" must not look the same,
        /// or the shutdown would power the machine off while believing the
        /// apps had been closed.
        pub fn list() -> Result<Vec<Window>, String> {
            let out = std::process::Command::new("wmctrl")
                .arg("-l")
                .output()
                .map_err(|e| format!("wmctrl could not be started: {e}"))?;
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let detail = stderr.lines().next().unwrap_or("").trim();
                return Err(if detail.is_empty() {
                    format!("wmctrl -l exited with {}", out.status)
                } else {
                    format!("wmctrl -l failed: {detail}")
                });
            }
            Ok(String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(parse_line)
                .collect())
        }

        /// Ask one window to close (what its X button does).
        pub fn close(id: isize) -> bool {
            std::process::Command::new("wmctrl")
                .arg("-c")
                .arg(format!("0x{id:x}"))
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }

        /// Is `wmctrl` installed?
        pub fn installed() -> bool {
            let Some(path) = std::env::var_os("PATH") else {
                return false;
            };
            std::env::split_paths(&path).any(|dir| is_executable(&dir.join("wmctrl")))
        }

        fn is_executable(path: &std::path::Path) -> bool {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(path)
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        }
    }
}

#[cfg(test)]
mod linux_tests {
    #[cfg(target_os = "linux")]
    use super::linux::{parse_window_id, strategy, wmctrl, Strategy};

    #[cfg(target_os = "linux")]
    #[test]
    fn wayland_can_never_close_the_other_apps() {
        // Even with wmctrl installed, a native Wayland session has no way to
        // ask another application to close.
        assert_eq!(strategy(true, true), Strategy::Wayland);
        assert_eq!(strategy(true, false), Strategy::Wayland);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn x11_needs_wmctrl() {
        assert_eq!(strategy(false, true), Strategy::Wmctrl);
        assert_eq!(strategy(false, false), Strategy::NoTool);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn wmctrl_lines_are_parsed() {
        let w = wmctrl::parse_line("0x03a00003  0  hostname  Firefox - Private").unwrap();
        assert_eq!(w.id, "0x03a00003");
        assert_eq!(w.title, "Firefox - Private");
        // Tabs and extra spaces are tolerated.
        let w = wmctrl::parse_line("0x03a00004\t0\thost\tA  B").unwrap();
        assert_eq!(w.id, "0x03a00004");
        assert_eq!(w.title, "A  B");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rubbish_lines_are_ignored() {
        assert!(wmctrl::parse_line("").is_none());
        assert!(wmctrl::parse_line("   ").is_none());
        assert!(wmctrl::parse_line("03000003 0 host Title").is_none(), "no 0x prefix");
        assert!(wmctrl::parse_line("0x03a00003 0 host").is_none(), "no title");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn window_ids_are_hexadecimal() {
        assert_eq!(parse_window_id("0x03a00003"), Some(0x03a00003));
        assert_eq!(parse_window_id("0X03A00003"), Some(0x03a00003));
        assert_eq!(parse_window_id("not-an-id"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_broken_or_missing_wmctrl_is_an_error_not_an_empty_room() {
        // Either wmctrl is missing (spawn error) or it cannot reach the X
        // server (non-zero exit). Both must be ERRORS: an empty list would
        // tell the shutdown that nothing was open, and the machine would
        // power off with the user's work still running.
        match wmctrl::list() {
            Ok(windows) => {
                assert!(windows.iter().all(|w| w.id.starts_with("0x")), "{windows:?}");
            }
            Err(e) => {
                assert!(!e.trim().is_empty(), "the error must say what went wrong");
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_linux_poll_is_slower_than_the_windows_one() {
        // Every Linux poll spawns a `wmctrl` process, so it must not poll
        // as often as the (free) Windows EnumWindows poll.
        use super::CloseEnv;
        let env = super::linux::LinuxEnv::new();
        assert!(env.poll_ms() >= 500, "the Linux poll must not be the 250 ms default");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_broken_window_tool_is_announced_not_hidden() {
        let line = super::linux::tool_failed_line();
        assert!(!line.trim().is_empty(), "the failure must be spoken, not hidden");
        assert!(line.contains("couldn't check"), "{line}");
        let line = super::linux::cannot_close_line();
        assert!(line.contains("couldn't close"), "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn win(h: isize, exe: &str, class: &str, title: &str) -> Win {
        Win { hwnd: h, exe: exe.into(), class: class.into(), title: title.into() }
    }

    /// Windows listed per poll; `close_after[i]` = the poll index at which window i disappears once asked.
    struct Fake {
        windows: Vec<Win>,
        /// hwnd -> number of polls after the close request until it is gone (None = never goes).
        goes_after: Vec<(isize, Option<u32>)>,
        asked: RefCell<Vec<isize>>,
        polls_since_ask: RefCell<u32>,
        slept: RefCell<u64>,
    }

    impl CloseEnv for Fake {
        fn list(&self) -> Vec<Win> {
            let asked = self.asked.borrow();
            let polls = *self.polls_since_ask.borrow();
            self.windows
                .iter()
                .filter(|w| {
                    let gone = asked.contains(&w.hwnd)
                        && self
                            .goes_after
                            .iter()
                            .find(|(h, _)| *h == w.hwnd)
                            .and_then(|(_, a)| *a)
                            .map(|a| polls >= a)
                            .unwrap_or(false);
                    !gone
                })
                .cloned()
                .collect()
        }
        fn close(&self, w: &Win) {
            self.asked.borrow_mut().push(w.hwnd);
        }
        fn sleep_ms(&self, ms: u64) {
            *self.slept.borrow_mut() += ms;
            *self.polls_since_ask.borrow_mut() += 1;
        }
    }

    fn fake(windows: Vec<Win>, goes_after: Vec<(isize, Option<u32>)>) -> Fake {
        Fake {
            windows,
            goes_after,
            asked: RefCell::new(vec![]),
            polls_since_ask: RefCell::new(0),
            slept: RefCell::new(0),
        }
    }

    #[test]
    fn the_shell_nexus_and_system_ui_are_never_asked_to_close() {
        assert!(!closable(&win(1, "explorer.exe", "Shell_TrayWnd", "Taskbar")));
        assert!(!closable(&win(2, "explorer.exe", "Progman", "Program Manager")));
        assert!(!closable(&win(3, "nexus.exe", "Tauri Window", "NEXUS")));
        assert!(!closable(&win(4, "searchhost.exe", "Windows.UI.Core.CoreWindow", "Search")));
        assert!(!closable(&win(5, "", "Chrome_WidgetWin_1", "Protected app")), "unreadable process = protected");
        assert!(!closable(&win(6, "notepad.exe", "Notepad", "  ")), "untitled windows are not apps");
        // ...but a real folder window and ordinary apps are.
        assert!(closable(&win(7, "explorer.exe", "CabinetWClass", "Downloads")));
        assert!(closable(&win(8, "notepad.exe", "Notepad", "notes - Notepad")));
        assert!(closable(&win(9, "brave.exe", "Chrome_WidgetWin_1", "YouTube - Brave")));
    }

    #[test]
    fn everything_that_closes_promptly_is_reported_closed_and_the_shell_is_untouched() {
        let env = fake(
            vec![
                win(1, "explorer.exe", "Shell_TrayWnd", "Taskbar"),
                win(2, "notepad.exe", "Notepad", "a - Notepad"),
                win(3, "brave.exe", "Chrome_WidgetWin_1", "Brave"),
            ],
            vec![(2, Some(1)), (3, Some(3))],
        );
        assert_eq!(close_all(&env, 8_000), Outcome::AllClosed { closed: 2 });
        assert_eq!(*env.asked.borrow(), vec![2, 3], "only the two apps were asked, never the taskbar");
        assert!(*env.slept.borrow() < 2_000, "it must not wait the full budget when everything closes");
    }

    #[test]
    fn a_window_that_stays_open_stops_the_shutdown_and_is_named() {
        let env = fake(
            vec![
                win(2, "notepad.exe", "Notepad", "unsaved - Notepad"),
                win(3, "brave.exe", "Chrome_WidgetWin_1", "Brave"),
            ],
            vec![(2, None), (3, Some(1))],
        );
        match close_all(&env, 2_000) {
            Outcome::Refused { closed, still_open } => {
                assert_eq!(closed, 1);
                assert_eq!(still_open, vec!["Notepad".to_string()]);
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(*env.slept.borrow() >= 2_000, "it waits out the budget before giving up");
    }

    #[test]
    fn nothing_open_is_an_instant_success() {
        let env = fake(vec![win(1, "explorer.exe", "Shell_TrayWnd", "Taskbar")], vec![]);
        assert_eq!(close_all(&env, 8_000), Outcome::AllClosed { closed: 0 });
    }

    #[test]
    fn refusal_lines_name_up_to_two_apps_and_count_the_rest() {
        assert!(refusal_line(&["Notepad".into()]).starts_with("Notepad is asking to save, sir."));
        assert!(refusal_line(&["Notepad".into(), "Word".into()]).starts_with("Notepad and Word are asking"));
        let many = refusal_line(&["A".into(), "B".into(), "C".into(), "D".into()]);
        assert!(many.starts_with("A, B and 2 more"), "{many}");
        assert!(refusal_line(&[]).contains("stopped the shutdown"));
        assert_eq!(friendly("notepad.exe"), "Notepad");
    }
}
