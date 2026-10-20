//! Linux keep-awake: the freedesktop screen-saver inhibit over D-Bus.
//!
//! Linux has no per-process "keep the screen on" API like Win32's
//! `SetThreadExecutionState`. The standard way to hold the screen awake is
//! the freedesktop screen-saver interface:
//!
//! ```text
//! org.freedesktop.ScreenSaver.Inhibit(app, reason) -> u32 cookie
//! org.freedesktop.ScreenSaver.UnInhibit(cookie)
//! ```
//!
//! It is implemented by every modern desktop (GNOME's gnome-shell, KDE's
//! kscreenlocker, COSMIC's cosmic-session, xfce4-screensaver), and the
//! inhibit is released automatically when the requesting D-Bus connection
//! goes away — so a crashed NEXUS never leaves the screen pinned on.
//!
//! # The caller never waits for the desktop
//!
//! The desktop is a *different program*, and it may never answer: on the
//! target machine `org.freedesktop.ScreenSaver` is owned by `cosmic-idle`,
//! and an unanswered `Inhibit` parks the D-Bus worker thread for good. A
//! voice turn must never be able to wait for that, so this module is
//! **fire-and-forget**:
//!
//! * [`inhibit`] / [`uninhibit`] only queue a request on an unbounded
//!   channel and return — they never make a D-Bus call and never wait for a
//!   reply, so they cannot block the caller (a lease, a TTS turn, a test);
//! * the worker thread owns the bus connection *and* the cookie, so there is
//!   no shared cookie state to lock and no reply channel to wait on;
//! * every D-Bus call the worker makes is bounded by an explicit timeout, so
//!   a silent desktop costs one timeout instead of the life of the worker.
//!
//! Requests that would do the same thing to the desktop are collapsed by the
//! worker (see [`inhibitor_action`]): the keep-awake watchdog re-asserts
//! every 500 ms, and every `Inhibit` returns a *new* cookie — the desktop
//! keeps the screen pinned for every cookie it was given, so an un-collapsed
//! re-assert would slowly leak cookies and the screen would never come back.
//!
//! # Unit tests never touch the real bus
//!
//! Under `cfg(test)` the whole transport is replaced by a counting no-op
//! (see `test_double`): a unit test must never inhibit the *user's* screen,
//! and it must never be able to wedge the session bus for the rest of the
//! test binary. Same pattern as `auth_vault::kr()` (keyring mock) and
//! `memcore::crypto::process_key()` (fixed key).

/// What the worker must ask the desktop to do.
///
/// Pure decision — the D-Bus calls live in the transport module, so this
/// stays unit-testable without a session bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InhibitorAction {
    /// Ask the desktop to hold the screen awake (and keep the cookie).
    Acquire,
    /// Hand the cookie back to the desktop that granted it.
    Release,
    /// Nothing to do: the screen is already held, or nothing is held.
    Idle,
}

/// Decide the D-Bus action for one request.
///
/// `acquire` = the caller wants the screen held, `held` = the worker already
/// holds a cookie. A repeated acquire must NOT ask the desktop again: every
/// `Inhibit` returns a *new* cookie and the desktop pins the screen for each
/// one, so the watchdog's re-assert (every 500 ms) would leak cookies and the
/// screen would never be released.
pub fn inhibitor_action(acquire: bool, held: bool) -> InhibitorAction {
    match (acquire, held) {
        (true, false) => InhibitorAction::Acquire,
        (false, true) => InhibitorAction::Release,
        _ => InhibitorAction::Idle,
    }
}

/// The real D-Bus transport. Never compiled into the unit-test binary.
#[cfg(not(test))]
mod transport {
    use super::{
        InhibitorAction, APP_NAME, CALL_TIMEOUT, CONNECT_TIMEOUT, GNOME_SCREENSAVER_NAME, REASON,
        SCREENSAVER_IFACE, SCREENSAVER_NAME, SCREENSAVER_PATH,
    };
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::OnceLock;

    /// A request for the D-Bus worker thread. There is no reply channel: the
    /// caller must never wait for the desktop.
    enum Request {
        /// Ask the desktop to hold the screen awake.
        Inhibit,
        /// Give the cookie back to the desktop that granted it.
        Uninhibit,
    }

    /// The channel to the D-Bus worker, started on first use.
    static WORKER: OnceLock<Option<Sender<Request>>> = OnceLock::new();

    /// Hold the screen awake until [`uninhibit`] is called.
    ///
    /// Fire-and-forget: the request is queued for the worker thread and this
    /// function returns immediately. The desktop is never waited for here.
    pub fn inhibit() -> Result<(), String> {
        let tx = worker_channel().ok_or("keep_awake: no D-Bus worker thread")?;
        tx.send(Request::Inhibit)
            .map_err(|e| format!("keep_awake: D-Bus worker is gone: {e}"))?;
        Ok(())
    }

    /// Release the screen-saver inhibition (the cookie goes back to the desktop).
    /// Idempotent: releasing when nothing is held is a no-op.
    pub fn uninhibit() -> Result<(), String> {
        let tx = worker_channel().ok_or("keep_awake: no D-Bus worker thread")?;
        tx.send(Request::Uninhibit)
            .map_err(|e| format!("keep_awake: D-Bus worker is gone: {e}"))?;
        Ok(())
    }

    /// The channel to the D-Bus worker, started on first use.
    ///
    /// `OnceLock` instead of a mutex: there is no state to protect, and the
    /// caller must not take a lock on the way to the queue.
    fn worker_channel() -> Option<Sender<Request>> {
        WORKER
            .get_or_init(|| {
                let (tx, rx) = channel::<Request>();
                match std::thread::Builder::new()
                    .name("keep-awake-dbus".into())
                    .spawn(move || worker(rx))
                {
                    Ok(_) => Some(tx),
                    Err(e) => {
                        tracing::warn!("keep_awake: could not start the D-Bus worker: {e}");
                        None
                    }
                }
            })
            .clone()
    }

    /// The worker thread: owns the session-bus connection and the cookie for
    /// the whole process.
    ///
    /// Every call it makes is bounded by a timeout, so a desktop that never
    /// answers can cost one timeout — never the worker. That matters: the
    /// worker is the only thing that can release the cookie, and a parked
    /// worker would leave the screen pinned for the rest of the session.
    fn worker(rx: Receiver<Request>) {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                tracing::warn!("keep_awake: no runtime for the D-Bus calls: {e}");
                return;
            }
        };

        let mut conn: Option<zbus::Connection> = None;
        // The cookie the desktop gave us (bus name + cookie). The worker owns
        // it: no other thread ever locks anything to release it.
        let mut held: Option<(&'static str, u32)> = None;

        while let Ok(req) = rx.recv() {
            let acquire = matches!(req, Request::Inhibit);
            match super::inhibitor_action(acquire, held.is_some()) {
                InhibitorAction::Idle => continue,
                InhibitorAction::Acquire => {
                    if conn.is_none() {
                        match rt.block_on(connect_session()) {
                            Ok(c) => conn = Some(c),
                            Err(e) => {
                                tracing::warn!("keep_awake: {e}");
                                continue;
                            }
                        }
                    }
                    let Some(c) = conn.clone() else { continue };
                    match rt.block_on(call_inhibit(&c)) {
                        Ok((bus, cookie)) => {
                            tracing::debug!("keep_awake: screen inhibited on {bus} (cookie {cookie})");
                            held = Some((bus, cookie));
                        }
                        Err(e) => tracing::warn!("keep_awake: {e}"),
                    }
                }
                InhibitorAction::Release => {
                    let Some((bus, cookie)) = held.take() else { continue };
                    let Some(c) = conn.clone() else { continue };
                    match rt.block_on(call_uninhibit(&c, bus, cookie)) {
                        Ok(()) => tracing::debug!("keep_awake: screen released (cookie {cookie})"),
                        Err(e) => tracing::warn!("keep_awake: {e}"),
                    }
                }
            }
        }
        // The connection dies with this thread → the desktop releases the
        // inhibition on its own.
    }

    /// Open the session bus, bounded: a missing or wedged bus must not park
    /// the worker.
    async fn connect_session() -> Result<zbus::Connection, String> {
        match tokio::time::timeout(CONNECT_TIMEOUT, zbus::Connection::session()).await {
            Ok(Ok(c)) => Ok(c),
            Ok(Err(e)) => Err(format!("keep_awake: no D-Bus session bus: {e}")),
            Err(_) => Err("keep_awake: the session bus did not answer in time".to_string()),
        }
    }

    /// Ask the desktop to hold the screen awake. Tries the freedesktop
    /// interface first, then the older GNOME one (same methods, different bus
    /// name). Bounded: the desktop may never answer.
    async fn call_inhibit(conn: &zbus::Connection) -> Result<(&'static str, u32), String> {
        for bus in [SCREENSAVER_NAME, GNOME_SCREENSAVER_NAME] {
            match tokio::time::timeout(CALL_TIMEOUT, try_inhibit(conn, bus)).await {
                Ok(Ok(cookie)) => return Ok((bus, cookie)),
                Ok(Err(e)) => tracing::debug!("keep_awake: {e}"),
                Err(_) => tracing::warn!("keep_awake: {bus}.Inhibit did not answer in time"),
            }
        }
        Err(format!(
            "keep_awake: no screensaver interface granted an inhibit ({SCREENSAVER_NAME})"
        ))
    }

    async fn try_inhibit(conn: &zbus::Connection, bus: &'static str) -> Result<u32, String> {
        let reply = conn
            .call_method(
                Some(bus),
                SCREENSAVER_PATH,
                Some(SCREENSAVER_IFACE),
                "Inhibit",
                &(APP_NAME, REASON),
            )
            .await
            .map_err(|e| format!("keep_awake: {bus}.Inhibit failed: {e}"))?;
        let cookie: u32 = reply
            .body()
            .deserialize()
            .map_err(|e| format!("keep_awake: {bus}.Inhibit returned an unreadable cookie: {e}"))?;
        Ok(cookie)
    }

    /// Give the cookie back. Bounded: the desktop may never answer, and the
    /// desktop releases the inhibit on its own when the connection dies.
    async fn call_uninhibit(
        conn: &zbus::Connection,
        bus: &'static str,
        cookie: u32,
    ) -> Result<(), String> {
        match tokio::time::timeout(
            CALL_TIMEOUT,
            conn.call_method(
                Some(bus),
                SCREENSAVER_PATH,
                Some(SCREENSAVER_IFACE),
                "UnInhibit",
                &(cookie,),
            ),
        )
        .await
        {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(format!("keep_awake: {bus}.UnInhibit failed: {e}")),
            Err(_) => Err(format!("keep_awake: {bus}.UnInhibit did not answer in time")),
        }
    }
}

/// The unit-test double: counts the calls and never touches the bus.
#[cfg(test)]
mod test_double {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static INHIBITS: AtomicUsize = AtomicUsize::new(0);
    static RELEASES: AtomicUsize = AtomicUsize::new(0);

    /// Ask the desktop to hold the screen awake. A no-op in tests: the real
    /// session bus must never be touched from a unit test.
    pub fn inhibit() -> Result<(), String> {
        INHIBITS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    /// Release the inhibition. A no-op in tests.
    pub fn uninhibit() -> Result<(), String> {
        RELEASES.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    /// How many times the inhibitor was asked to hold the screen.
    pub fn inhibits() -> usize {
        INHIBITS.load(Ordering::SeqCst)
    }

    /// How many times the inhibitor was asked to release the screen.
    pub fn releases() -> usize {
        RELEASES.load(Ordering::SeqCst)
    }
}

/// The application name reported to the desktop.
#[cfg(not(test))]
const APP_NAME: &str = "nexus";
/// The reason reported to the desktop.
#[cfg(not(test))]
const REASON: &str = "NEXUS is speaking or working";

/// The freedesktop screen-saver interface (GNOME, KDE, COSMIC, Xfce).
#[cfg(not(test))]
const SCREENSAVER_NAME: &str = "org.freedesktop.ScreenSaver";
/// The older GNOME interface — same methods, different bus name.
#[cfg(not(test))]
const GNOME_SCREENSAVER_NAME: &str = "org.gnome.ScreenSaver";
/// The object path both interfaces live on.
#[cfg(not(test))]
const SCREENSAVER_PATH: &str = "/org/freedesktop/ScreenSaver";
/// The interface that declares Inhibit/UnInhibit.
#[cfg(not(test))]
const SCREENSAVER_IFACE: &str = "org.freedesktop.ScreenSaver";

/// How long a single D-Bus call may take before it is abandoned.
#[cfg(not(test))]
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
/// How long connecting to the session bus may take.
#[cfg(not(test))]
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

#[cfg(not(test))]
pub use transport::{inhibit, uninhibit};
#[cfg(test)]
pub use test_double::{inhibit, uninhibit};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releasing_without_an_inhibition_is_a_no_op() {
        // Nothing is held in a fresh test process → releasing must succeed
        // instead of erroring (the caller releases on every lease drop).
        assert!(uninhibit().is_ok());
    }

    #[test]
    fn the_unit_test_double_never_touches_the_session_bus() {
        // The transport is not even compiled into the test binary: the
        // inhibitor is a counting no-op, so a unit test can neither inhibit
        // the user's real screensaver nor park the D-Bus worker.
        let before = (super::test_double::releases(), super::test_double::inhibits());
        assert!(inhibit().is_ok());
        assert!(uninhibit().is_ok());
        let after = (super::test_double::releases(), super::test_double::inhibits());
        assert_eq!(after.1, before.1 + 1, "the inhibitor was not called");
        assert_eq!(after.0, before.0 + 1, "the release was not counted");
    }

    #[test]
    fn a_repeated_acquire_does_not_ask_the_desktop_again() {
        // Every Inhibit returns a NEW cookie and the desktop pins the screen
        // for each one — so the watchdog's re-assert must collapse to a
        // no-op instead of leaking cookies.
        assert_eq!(inhibitor_action(true, false), InhibitorAction::Acquire);
        assert_eq!(inhibitor_action(true, true), InhibitorAction::Idle);
    }

    #[test]
    fn a_release_without_a_cookie_does_nothing() {
        assert_eq!(inhibitor_action(false, false), InhibitorAction::Idle);
        assert_eq!(inhibitor_action(false, true), InhibitorAction::Release);
    }
}
