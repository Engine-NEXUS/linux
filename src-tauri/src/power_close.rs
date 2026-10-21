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
    /// A listing failure, if one happened. A failed listing means we no
    /// longer know what is open, so the closer must not report a clean
    /// shutdown. Only Linux can fail to list (`wmctrl` talks to the X
    /// server); Windows' `EnumWindows` has no failure mode, so the method
    /// does not exist there at all.
    #[cfg(target_os = "linux")]
    fn failure(&self) -> Option<String> {
        None
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
        let strat = strategy(crate::session::is_wayland(), wmctrl::installed());
        let probe = match strat {
            Strategy::Wmctrl => wmctrl::list().map(|_| ()),
            _ => Ok(()),
        };
        if let Err(detail) = &probe {
            // The raw command error is for the log, never for the user's ear.
            tracing::warn!("power_close: the window tool failed: {detail}");
        }
        can_close_from(strat, probe)
    }

    /// The pure mapping (unit-tested): can the apps be closed here, and if
    /// not, what is the user told?
    ///
    /// The tool's raw error is deliberately dropped — the user is told what
    /// it means for them, the error text goes to the log.
    pub fn can_close_from(strategy: Strategy, probe: Result<(), String>) -> Result<(), String> {
        match strategy {
            Strategy::Wmctrl => match probe {
                Ok(()) => Ok(()),
                Err(_) => Err(TOOL_BROKEN_REASON.to_string()),
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
        /// The tool that can close windows is not working — we do NOT know
        /// whether the apps closed, so the shutdown must not go ahead.
        ToolFailed { reason: String },
    }

    /// What NEXUS says when it could not even find out whether the other
    /// apps closed. The shutdown stops here, so the line says what to do.
    pub fn tool_failed_line() -> String {
        "I couldn't check whether your other apps closed, sir, so I've stopped the shutdown — please save your work and turn it off yourself."
            .to_string()
    }

    // ── The "I can't close your other apps" warning ──────────────────────
    //
    // On Linux the shutdown may only go past the close step when the user
    // has EXPLICITLY confirmed that warning in THIS shutdown flow. Several
    // paths reach the shutdown without ever showing it — the loose-mishearing
    // offer ("Did you want me to shut down your laptop, sir?") and the
    // meeting/drill early returns in the blocker — and there the apps are
    // still open when the machine powers off.
    //
    // The state is tracked here so the rule cannot depend on which path the
    // shutdown came from.

    /// The state of the Linux close-warning inside the current flow.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum WarningState {
        /// The warning was not asked in this flow.
        None,
        /// The warning was asked; the answer has not arrived yet.
        Asked,
        /// The warning was asked and the user answered "yes" in this flow.
        Confirmed,
    }

    impl WarningState {
        fn as_code(self) -> u8 {
            match self {
                WarningState::None => 0,
                WarningState::Asked => 1,
                WarningState::Confirmed => 2,
            }
        }

        fn from_code(code: u8) -> WarningState {
            match code {
                1 => WarningState::Asked,
                2 => WarningState::Confirmed,
                _ => WarningState::None,
            }
        }
    }

    static WARNING_STATE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

    /// The current state of the warning.
    pub fn warning_state() -> WarningState {
        WarningState::from_code(WARNING_STATE.load(std::sync::atomic::Ordering::SeqCst))
    }

    /// The warning was asked (the question is on its way to the user).
    pub fn note_warning_asked() {
        WARNING_STATE.store(WarningState::Asked.as_code(), std::sync::atomic::Ordering::SeqCst);
    }

    /// The user answered "yes" to the pending power question. Only a pending
    /// Linux warning becomes a confirmation; anything else clears the state
    /// (the "yes" was for a different question).
    pub fn confirm_warning() {
        let next = match warning_state() {
            WarningState::Asked => WarningState::Confirmed,
            _ => WarningState::None,
        };
        WARNING_STATE.store(next.as_code(), std::sync::atomic::Ordering::SeqCst);
    }

    /// Forget the warning — a new flow started, or the question was answered
    /// with anything but "yes".
    pub fn clear_warning() {
        WARNING_STATE.store(WarningState::None.as_code(), std::sync::atomic::Ordering::SeqCst);
    }

    /// Consume the confirmation: true when the user confirmed the warning in
    /// this flow. The state is cleared, so a later shutdown cannot inherit it.
    pub fn take_warning_confirmed() -> bool {
        if warning_state() == WarningState::Confirmed {
            clear_warning();
            true
        } else {
            false
        }
    }

    /// What happens after the close step (unit-tested).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Proceed {
        /// The shutdown may go ahead.
        Continue,
        /// The shutdown stops and the user is told why.
        Stop { line: String },
    }

    /// The pure decision (unit-tested): may the shutdown go on after the
    /// close step?
    ///
    /// `confirmed` = the user explicitly confirmed the "I can't close your
    /// other apps" warning in THIS shutdown flow. Without it, a session that
    /// cannot close the apps (or a window tool that failed) stops the
    /// shutdown instead of quietly dropping unsaved work.
    pub fn after_close(close: &CloseResult, confirmed: bool) -> Proceed {
        match close {
            // Everything asked to close did close.
            CloseResult::AllClosed { .. } => Proceed::Continue,
            // A window stayed open — the shutdown stops, as it always has.
            CloseResult::Refused { still_open, .. } => {
                Proceed::Stop { line: super::refusal_line(still_open) }
            }
            // The apps could not be closed (or we cannot even tell): the
            // shutdown may only go ahead if the user was warned and said yes.
            CloseResult::CannotClose { reason } => {
                if confirmed {
                    Proceed::Continue
                } else {
                    Proceed::Stop { line: format!("{reason} I've stopped the shutdown.") }
                }
            }
            CloseResult::ToolFailed { .. } => {
                if confirmed {
                    Proceed::Continue
                } else {
                    Proceed::Stop { line: tool_failed_line() }
                }
            }
        }
    }

    /// A sticky listing failure.
    ///
    /// Once a listing has failed we no longer know what is open, so a later
    /// SUCCESSFUL listing must not erase the failure — the first one is
    /// kept, because the failed listing may have been the one that built
    /// the target set (an empty target set means nothing was ever asked to
    /// close).
    #[derive(Default)]
    pub struct FailureFlag {
        error: std::cell::RefCell<Option<String>>,
    }

    impl FailureFlag {
        /// Record a listing failure. The FIRST failure wins.
        pub fn record(&self, error: String) {
            let mut slot = self.error.borrow_mut();
            if slot.is_none() {
                *slot = Some(error);
            }
        }

        /// The sticky failure, if any listing failed.
        pub fn get(&self) -> Option<String> {
            self.error.borrow().clone()
        }
    }

    /// The X11 windows `wmctrl` can see, as the closer sees them.
    ///
    /// `failure` remembers a failed listing: an empty window list and a
    /// broken `wmctrl` look the same from the outside, and the difference is
    /// the difference between "nothing was open" and "we closed nothing".
    pub struct LinuxEnv {
        failure: FailureFlag,
    }

    impl LinuxEnv {
        pub fn new() -> Self {
            Self { failure: FailureFlag::default() }
        }

        /// Turn a listing result into the windows the closer works on.
        ///
        /// A failed listing is recorded (sticky) and looks like an empty
        /// room — the caller checks `failure()` before believing it.
        pub fn absorb(&self, result: Result<Vec<wmctrl::Window>, String>) -> Vec<Win> {
            let windows = match result {
                Ok(w) => w,
                Err(e) => {
                    // Sticky: a later successful listing does NOT erase this.
                    self.failure.record(e);
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
    }

    impl CloseEnv for LinuxEnv {
        fn list(&self) -> Vec<Win> {
            self.absorb(wmctrl::list())
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

        fn failure(&self) -> Option<String> {
            self.failure.get()
        }
    }

    /// Close everything with the given environment, and report honestly.
    ///
    /// A listing that failed is a FAILURE, not an empty room: the caller
    /// must not power the machine off while believing the apps were closed.
    /// (The failure is sticky — see [`FailureFlag`] — so a failed FIRST
    /// listing, which is the one that builds the target set, cannot be
    /// erased by a later successful poll.)
    pub fn close_with(env: &dyn CloseEnv, wait_ms: u64) -> CloseResult {
        let outcome = super::close_all(env, wait_ms);
        if let Some(reason) = env.failure() {
            return CloseResult::ToolFailed {
                reason: format!("{TOOL_BROKEN_REASON} ({reason})"),
            };
        }
        match outcome {
            super::Outcome::AllClosed { closed } => CloseResult::AllClosed { closed },
            super::Outcome::Refused { closed, still_open } => CloseResult::Refused { closed, still_open },
        }
    }

    /// Close everything, and report honestly what happened.
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
        close_with(&env, wait_ms)
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
    fn a_failed_listing_is_a_tool_failure_not_an_empty_room() {
        // The Linux failure mode: `list()` cannot talk to the X server, so
        // it returns nothing — which, without the failure flag, would look
        // like "no windows were open" and the machine would power off with
        // the user's work still running.
        struct FailingEnv {
            failure: Option<String>,
        }
        impl super::CloseEnv for FailingEnv {
            fn list(&self) -> Vec<super::Win> {
                vec![] // a failed listing looks like an empty room
            }
            fn close(&self, _w: &super::Win) {}
            fn sleep_ms(&self, _ms: u64) {}
            fn failure(&self) -> Option<String> {
                self.failure.clone()
            }
        }

        let broken = FailingEnv {
            failure: Some("wmctrl -l failed: Cannot open display".into()),
        };
        let result = super::linux::close_with(&broken, 0);
        assert!(
            matches!(result, super::linux::CloseResult::ToolFailed { .. }),
            "a failed listing must never be reported as a clean close: {result:?}"
        );

        // The same env without a failure IS a clean close — the mapping
        // distinguishes the two instead of always refusing.
        let healthy = FailingEnv { failure: None };
        let result = super::linux::close_with(&healthy, 0);
        assert!(
            matches!(result, super::linux::CloseResult::AllClosed { .. }),
            "{result:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_failed_first_listing_is_not_erased_by_a_later_success() {
        use super::CloseEnv;
        let env = super::linux::LinuxEnv::new();
        // The FIRST listing fails — it is the one that builds the target
        // set, so nothing was ever asked to close.
        assert!(env.absorb(Err("Cannot open display".into())).is_empty());
        // A later listing works. The failure must still stand.
        let later = env.absorb(Ok(vec![super::linux::wmctrl::Window {
            id: "0x03a00003".into(),
            title: "Firefox".into(),
        }]));
        assert_eq!(later.len(), 1);
        assert!(
            env.failure().is_some(),
            "the sticky failure must survive a later successful listing"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_list_error_makes_the_close_step_refuse_without_reading_the_error_aloud() {
        // The blocker asks the user once; the raw command error is for the
        // log, not for the user's ear.
        let reason = super::linux::can_close_from(
            super::linux::Strategy::Wmctrl,
            Err("wmctrl -l failed: Cannot open display".to_string()),
        )
        .expect_err("a broken window tool must refuse to close silently");
        assert!(!reason.contains("Cannot open display"), "the raw error must not be spoken: {reason}");
        assert!(reason.contains("window manager"), "{reason}");
        // A working tool still allows closing; Wayland / a missing tool keep
        // their own reasons.
        assert!(super::linux::can_close_from(super::linux::Strategy::Wmctrl, Ok(())).is_ok());
        assert!(super::linux::can_close_from(super::linux::Strategy::Wayland, Ok(())).is_err());
        assert!(super::linux::can_close_from(super::linux::Strategy::NoTool, Ok(())).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_close_step_polls_at_the_envs_own_rate() {
        // Every Linux poll spawns a `wmctrl` process, so the loop must use
        // the environment's interval, not the 250 ms Windows default.
        struct SlowEnv {
            lists: std::cell::Cell<u32>,
        }
        impl super::CloseEnv for SlowEnv {
            fn list(&self) -> Vec<super::Win> {
                self.lists.set(self.lists.get() + 1);
                vec![super::Win {
                    hwnd: 1,
                    exe: "x".into(),
                    class: String::new(),
                    title: "never closes".into(),
                }]
            }
            fn close(&self, _w: &super::Win) {}
            fn sleep_ms(&self, _ms: u64) {}
            fn poll_ms(&self) -> u64 {
                1_000
            }
        }
        let env = SlowEnv { lists: std::cell::Cell::new(0) };
        // The window never goes away, so the step waits out its budget.
        let _ = super::close_all(&env, 2_000);
        assert!(
            env.lists.get() <= 3,
            "the window set must not be polled every 250 ms: {} polls",
            env.lists.get()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_shutdown_only_goes_on_when_the_warning_was_confirmed() {
        use super::linux::{after_close, CloseResult, Proceed};

        let cannot = CloseResult::CannotClose {
            reason: "I can't close your other apps on Wayland, sir — anything unsaved will be lost.".to_string(),
        };
        // No confirmation in this flow (the loose-mishearing offer, the
        // meeting/drill early returns) -> the shutdown stops and says why.
        match after_close(&cannot, false) {
            Proceed::Stop { line } => {
                assert!(line.contains("stopped the shutdown"), "{line}");
                assert!(line.contains("Wayland"), "{line}");
            }
            other => panic!("expected a stop, got {other:?}"),
        }
        // Confirmed in this flow -> the shutdown goes ahead, and nothing
        // extra is spoken (the user already heard the warning).
        assert_eq!(after_close(&cannot, true), Proceed::Continue);

        // The same rule for a window tool that failed.
        let failed = CloseResult::ToolFailed { reason: "wmctrl -l failed".to_string() };
        assert!(matches!(after_close(&failed, false), Proceed::Stop { .. }));
        assert_eq!(after_close(&failed, true), Proceed::Continue);

        // Everything that was asked to close did close -> always go ahead.
        assert_eq!(after_close(&CloseResult::AllClosed { closed: 3 }, false), Proceed::Continue);

        // A window that stayed open always stops the shutdown.
        let refused = CloseResult::Refused { closed: 1, still_open: vec!["Notepad".to_string()] };
        match after_close(&refused, true) {
            Proceed::Stop { line } => assert!(line.contains("Notepad"), "{line}"),
            other => panic!("expected a stop, got {other:?}"),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_warning_confirmation_is_tracked_per_flow() {
        use super::linux::{
            clear_warning, confirm_warning, note_warning_asked, take_warning_confirmed, warning_state,
            WarningState,
        };

        clear_warning();
        assert_eq!(warning_state(), WarningState::None);
        // A "yes" to a question that carried no warning confirms nothing.
        confirm_warning();
        assert!(!take_warning_confirmed());
        // The warning was asked and answered "yes" -> confirmed, once.
        note_warning_asked();
        assert_eq!(warning_state(), WarningState::Asked);
        confirm_warning();
        assert_eq!(warning_state(), WarningState::Confirmed);
        assert!(take_warning_confirmed());
        assert!(!take_warning_confirmed(), "the confirmation is consumed");
        assert_eq!(warning_state(), WarningState::None);
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
