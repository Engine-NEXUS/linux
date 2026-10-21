//! Linux: bounded subprocess runs for the desktop tools we shell out to.
//!
//! The Linux helpers talk to the desktop through small command-line tools —
//! `wpctl` / `pactl` / `amixer` for the volume, `pactl` for the meeting
//! detector. Those tools talk to the audio server, and a **wedged** audio
//! server (PipeWire/PulseAudio hanging is a known failure mode) makes them
//! block forever.
//!
//! An unbounded tool call is not just slow: the volume lease is taken on
//! every spoken reply and the volume state mutex is held across the call, so
//! one hung `wpctl` means no reply is ever spoken and the user's volume stays
//! pinned at the TTS level.
//!
//! So every tool call goes through [`run_bounded`]: the child is spawned with
//! a piped stdout, polled against a deadline, and **killed** when the deadline
//! passes. A hung tool then costs one timeout instead of the caller.
//!
//! The decision logic is separated from the process handling ([`TimeoutChild`])
//! so it can be unit-tested with a fake child — no real process involved.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How often a running child is polled.
const POLL: Duration = Duration::from_millis(20);

/// The outcome of a bounded child-process run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundedOutcome {
    /// The child finished on its own.
    Finished {
        /// Whether it exited successfully.
        success: bool,
        /// Everything it wrote to stdout.
        stdout: String,
    },
    /// The child ran out of time and was killed.
    TimedOut,
    /// The child could not be started at all (the tool is not installed).
    NotStarted,
}

impl BoundedOutcome {
    /// Did the child finish successfully?
    #[cfg(test)]
    pub fn is_success(&self) -> bool {
        matches!(self, BoundedOutcome::Finished { success: true, .. })
    }

    /// The child's stdout, or an empty string when it did not finish.
    #[cfg(test)]
    pub fn stdout(&self) -> &str {
        match self {
            BoundedOutcome::Finished { stdout, .. } => stdout,
            _ => "",
        }
    }
}

/// The parts of a child process the timeout helper needs.
///
/// Implemented for `std::process::Child` (see [`ProcessChild`]); the unit test
/// drives a fake, so the deadline logic is testable without a real process.
pub trait TimeoutChild {
    /// `Some(true)` = exited successfully, `Some(false)` = exited with a
    /// failure, `None` = still running.
    fn try_exit(&mut self) -> Option<bool>;
    /// Kill a child that ran out of time.
    fn kill(&mut self);
    /// The child's stdout (read after it exited).
    fn take_stdout(&mut self) -> String;
    /// How long the child has been running, in milliseconds.
    fn elapsed_ms(&self) -> u64;
}

/// Wait for `child`, but never longer than `timeout_ms`.
///
/// Returns [`BoundedOutcome::Finished`] when the child exits on its own and
/// [`BoundedOutcome::TimedOut`] when the deadline passes — the child is
/// killed first, so it can never outlive the call.
pub fn wait_bounded<C: TimeoutChild>(
    child: &mut C,
    timeout_ms: u64,
    poll: Duration,
) -> BoundedOutcome {
    loop {
        if let Some(success) = child.try_exit() {
            return BoundedOutcome::Finished {
                success,
                stdout: child.take_stdout(),
            };
        }
        if child.elapsed_ms() >= timeout_ms {
            child.kill();
            return BoundedOutcome::TimedOut;
        }
        std::thread::sleep(poll);
    }
}

/// A real `std::process::Child` the timeout helper can drive.
struct ProcessChild {
    child: std::process::Child,
    started: Instant,
    /// A thread that drains stdout while the child runs — otherwise a chatty
    /// child fills the pipe buffer and blocks on its own write.
    reader: Option<std::thread::JoinHandle<Vec<u8>>>,
}

impl TimeoutChild for ProcessChild {
    fn try_exit(&mut self) -> Option<bool> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.success()),
            Ok(None) => None,
            Err(_) => Some(false),
        }
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn take_stdout(&mut self) -> String {
        let bytes = match self.reader.take() {
            Some(h) => h.join().unwrap_or_default(),
            None => Vec::new(),
        };
        String::from_utf8_lossy(&bytes).to_string()
    }

    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}

/// Run `cmd` with a hard deadline.
///
/// Never blocks longer than `timeout_ms`: a tool that hangs is killed and
/// reported as [`BoundedOutcome::TimedOut`]. A tool that is not installed is
/// reported as [`BoundedOutcome::NotStarted`].
pub fn run_bounded(cmd: &mut Command, timeout_ms: u64) -> BoundedOutcome {
    // Captured before the spawn: the mutable borrow of `cmd` lives until the
    // end of the statement below.
    let program = cmd.get_program().to_string_lossy().to_string();
    let started = Instant::now();
    let mut child = match cmd.stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("proc: could not start '{program}': {e}");
            return BoundedOutcome::NotStarted;
        }
    };
    let reader = child.stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });
    let mut child = ProcessChild {
        child,
        started,
        reader,
    };
    wait_bounded(&mut child, timeout_ms, POLL)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A child that never touches a real process: it counts polls and a
    /// virtual clock, so the deadline logic is testable on its own.
    struct FakeChild {
        polls: u32,
        exit_after: u32,
        exit_success: bool,
        killed: bool,
        stdout: String,
        clock_ms: u64,
    }

    impl FakeChild {
        fn new(exit_after: u32, exit_success: bool, stdout: &str) -> Self {
            Self {
                polls: 0,
                exit_after,
                exit_success,
                killed: false,
                stdout: stdout.to_string(),
                clock_ms: 0,
            }
        }
    }

    impl TimeoutChild for FakeChild {
        fn try_exit(&mut self) -> Option<bool> {
            self.polls += 1;
            self.clock_ms += 10;
            if self.polls >= self.exit_after {
                Some(self.exit_success)
            } else {
                None
            }
        }

        fn kill(&mut self) {
            self.killed = true;
        }

        fn take_stdout(&mut self) -> String {
            self.stdout.clone()
        }

        fn elapsed_ms(&self) -> u64 {
            self.clock_ms
        }
    }

    #[test]
    fn a_child_that_exits_is_finished_with_its_output() {
        let mut child = FakeChild::new(2, true, "Volume: 0.50");
        let out = wait_bounded(&mut child, 5_000, Duration::from_millis(0));
        assert_eq!(
            out,
            BoundedOutcome::Finished {
                success: true,
                stdout: "Volume: 0.50".to_string()
            }
        );
        assert!(!child.killed, "a child that finished must not be killed");
    }

    #[test]
    fn a_child_that_exits_with_a_failure_is_reported_as_a_failure() {
        let mut child = FakeChild::new(1, false, "");
        let out = wait_bounded(&mut child, 5_000, Duration::from_millis(0));
        assert!(!out.is_success());
        assert_eq!(out.stdout(), "");
    }

    #[test]
    fn a_child_that_never_exits_is_killed_at_the_deadline() {
        // The tool hangs: the caller must get its timeout, not the hang.
        let mut child = FakeChild::new(u32::MAX, true, "");
        let out = wait_bounded(&mut child, 50, Duration::from_millis(0));
        assert_eq!(out, BoundedOutcome::TimedOut);
        assert!(child.killed, "the child must be killed, not left running");
    }

    #[test]
    fn a_tool_that_is_not_installed_is_reported_as_not_started() {
        let mut cmd = Command::new("nexus-no-such-tool-xyz");
        cmd.arg("--version");
        assert_eq!(run_bounded(&mut cmd, 5_000), BoundedOutcome::NotStarted);
    }
}
