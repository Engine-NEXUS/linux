//! Touchpad-style hand gestures for switching tabs, windows and virtual desktops. Pure: poses + a clock
//! in, commands out. No I/O, no globals (the glue in `mod.rs` turns a command into key presses).
//!
//! Model copied from the touchpad (docs/features/116):
//! * The pose is the clutch: nothing happens until a `Blade` pose has been held `hold_ms` (a hand that is
//!   only changing shape, pointing or talking never gets that far).
//! * A deliberate FLICK (the palm travelling `min_travel` of the camera frame inside `window_ms`, mostly
//!   along one axis) is one command; then a refractory period, and the way BACK is ignored for a while
//!   (the return stroke of a flick must not fire the opposite command).
//! * The window switcher holds Alt like the real Alt+Tab. It is committed by dropping the pose or a
//!   downward flick, cancelled by `cancel()` (disarm, lost hand, Esc, mouse takeover) and by a timeout,
//!   so Alt can never stay down.

use super::pose::Blade;
use std::collections::VecDeque;

/// What the glue should do. `id()` is the log / HUD label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    NextTab,
    PrevTab,
    /// Alt down + Tab (selects the previous window, like the keyboard).
    SwitcherOpen,
    /// Tab while Alt is held.
    SwitcherNext,
    /// Shift+Tab while Alt is held.
    SwitcherPrev,
    /// Release Alt: go to the highlighted window.
    SwitcherCommit,
    /// Escape, then release Alt: stay where we were.
    SwitcherCancel,
    DesktopPrev,
    DesktopNext,
    TaskView,
    ShowDesktop,
}

impl Cmd {
    pub fn id(self) -> &'static str {
        match self {
            Cmd::NextTab => "next_tab",
            Cmd::PrevTab => "prev_tab",
            Cmd::SwitcherOpen => "window_switcher",
            Cmd::SwitcherNext => "window_next",
            Cmd::SwitcherPrev => "window_prev",
            Cmd::SwitcherCommit => "window_commit",
            Cmd::SwitcherCancel => "window_cancel",
            Cmd::DesktopPrev => "desktop_prev",
            Cmd::DesktopNext => "desktop_next",
            Cmd::TaskView => "task_view",
            Cmd::ShowDesktop => "show_desktop",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    Right,
    Left,
    Up,
    Down,
}

impl Dir {
    fn opposite(self) -> Dir {
        match self {
            Dir::Right => Dir::Left,
            Dir::Left => Dir::Right,
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// A blade pose counts after this long (and on at least two frames in a row).
    pub hold_ms: u64,
    /// A short loss of the pose (a jittery frame) does not end it; this long does.
    pub leave_ms: u64,
    /// Flick = this much palm travel (fraction of the camera frame) within `window_ms`.
    pub min_travel: f64,
    pub window_ms: u64,
    /// The main axis must beat the other axis by this factor.
    pub dominance: f64,
    pub refractory_ms: u64,
    /// After a flick the opposite direction is ignored this long (the return stroke).
    pub return_block_ms: u64,
    /// The switcher closes (cancel) after this long open.
    pub switcher_timeout_ms: u64,
    /// The camera image is not mirrored: a hand moving to the user's right moves left in the image.
    pub mirror: bool,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            hold_ms: 250,
            leave_ms: 200,
            min_travel: 0.10,
            window_ms: 400,
            dominance: 1.5,
            refractory_ms: 400,
            return_block_ms: 600,
            switcher_timeout_ms: 10_000,
            mirror: true,
        }
    }
}

/// What one frame produced.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct StepOut {
    pub cmd: Option<Cmd>,
    /// The engaged pose just changed (Some(None) = left the pose). For logging and the HUD.
    pub engaged: Option<Option<Blade>>,
}

#[derive(Debug)]
pub struct Gestures {
    p: Params,
    cand: Option<(Blade, u64, u32)>,
    engaged: Option<Blade>,
    none_since: Option<u64>,
    hist: VecDeque<(u64, f64, f64)>,
    last_fire: Option<(u64, Dir)>,
    switcher_since: Option<u64>,
}

impl Gestures {
    pub fn new(p: Params) -> Self {
        Self { p, cand: None, engaged: None, none_since: None, hist: VecDeque::new(), last_fire: None, switcher_since: None }
    }

    pub fn engaged(&self) -> Option<Blade> {
        self.engaged
    }

    pub fn switcher_open(&self) -> bool {
        self.switcher_since.is_some()
    }

    /// Everything stops now. Returns the cancel command when the window switcher was open (Alt is down).
    pub fn cancel(&mut self) -> Option<Cmd> {
        self.cand = None;
        self.engaged = None;
        self.none_since = None;
        self.hist.clear();
        self.last_fire = None;
        self.switcher_since.take().map(|_| Cmd::SwitcherCancel)
    }

    /// One frame. `blade` is the pose seen now (None = no gesture pose), `pos` the filtered palm centre in
    /// camera-image coordinates.
    pub fn step(&mut self, t: u64, blade: Option<Blade>, pos: (f64, f64)) -> StepOut {
        let mut out = StepOut::default();

        if let Some(s) = self.switcher_since {
            if t.saturating_sub(s) >= self.p.switcher_timeout_ms {
                self.switcher_since = None;
                self.engaged = None;
                self.cand = None;
                self.hist.clear();
                out.cmd = Some(Cmd::SwitcherCancel);
                out.engaged = Some(None);
                return out;
            }
        }

        let Some(b) = blade else {
            self.cand = None;
            let since = *self.none_since.get_or_insert(t);
            if self.engaged.is_some() && t.saturating_sub(since) >= self.p.leave_ms {
                self.engaged = None;
                self.hist.clear();
                self.none_since = None;
                out.engaged = Some(None);
                // Dropping the pose is how the switcher is committed.
                if self.switcher_since.take().is_some() {
                    out.cmd = Some(Cmd::SwitcherCommit);
                }
            }
            return out;
        };
        self.none_since = None;

        if self.engaged != Some(b) {
            self.cand = match self.cand {
                Some((cb, since, n)) if cb == b => Some((cb, since, n + 1)),
                _ => Some((b, t, 1)),
            };
            let (_, since, n) = self.cand.expect("set above");
            if t.saturating_sub(since) >= self.p.hold_ms && n >= 2 {
                // Going from one blade to another commits an open switcher first.
                if self.switcher_since.take().is_some() {
                    out.cmd = Some(Cmd::SwitcherCommit);
                }
                self.engaged = Some(b);
                self.hist.clear();
                self.hist.push_back((t, pos.0, pos.1));
                out.engaged = Some(Some(b));
            }
            return out;
        }

        // Engaged: look for a flick.
        self.hist.push_back((t, pos.0, pos.1));
        while self.hist.front().is_some_and(|(t0, _, _)| t.saturating_sub(*t0) > self.p.window_ms) {
            self.hist.pop_front();
        }
        if self.last_fire.is_some_and(|(tf, _)| t.saturating_sub(tf) < self.p.refractory_ms) {
            // The rest of the same stroke must not count once the refractory ends: start a fresh window.
            self.hist.clear();
            self.hist.push_back((t, pos.0, pos.1));
            return out;
        }
        let Some(&(_, x0, y0)) = self.hist.front() else { return out };
        let dx = pos.0 - x0;
        let dy = pos.1 - y0;
        let ux = if self.p.mirror { -dx } else { dx }; // positive = the user's right
        let uy = -dy; // positive = up
        let dir = if ux.abs() >= self.p.min_travel && ux.abs() >= self.p.dominance * uy.abs() {
            if ux > 0.0 { Dir::Right } else { Dir::Left }
        } else if uy.abs() >= self.p.min_travel && uy.abs() >= self.p.dominance * ux.abs() {
            if uy > 0.0 { Dir::Up } else { Dir::Down }
        } else {
            return out;
        };
        if self.last_fire.is_some_and(|(tf, d)| d == dir.opposite() && t.saturating_sub(tf) < self.p.return_block_ms) {
            return out; // the return stroke of the previous flick
        }

        let cmd = match (b, dir, self.switcher_open()) {
            (Blade::Three, Dir::Right, false) => Some(Cmd::NextTab),
            (Blade::Three, Dir::Left, false) => Some(Cmd::PrevTab),
            (Blade::Three, Dir::Up, false) => Some(Cmd::SwitcherOpen),
            (Blade::Three, Dir::Right, true) => Some(Cmd::SwitcherNext),
            (Blade::Three, Dir::Left, true) => Some(Cmd::SwitcherPrev),
            (Blade::Three, Dir::Down, true) => Some(Cmd::SwitcherCommit),
            (Blade::Four, Dir::Right, _) => Some(Cmd::DesktopNext),
            (Blade::Four, Dir::Left, _) => Some(Cmd::DesktopPrev),
            (Blade::Four, Dir::Up, _) => Some(Cmd::TaskView),
            (Blade::Four, Dir::Down, _) => Some(Cmd::ShowDesktop),
            _ => None,
        };
        let Some(cmd) = cmd else { return out };
        match cmd {
            Cmd::SwitcherOpen => self.switcher_since = Some(t),
            Cmd::SwitcherCommit => self.switcher_since = None,
            _ => {}
        }
        self.last_fire = Some((t, dir));
        self.hist.clear();
        self.hist.push_back((t, pos.0, pos.1));
        out.cmd = Some(cmd);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: u64 = 33;

    struct Rig {
        g: Gestures,
        t: u64,
        x: f64,
        y: f64,
        cmds: Vec<Cmd>,
        engaged: Vec<Option<Blade>>,
    }

    impl Rig {
        fn new() -> Self {
            Self { g: Gestures::new(Params::default()), t: 1000, x: 0.5, y: 0.5, cmds: vec![], engaged: vec![] }
        }
        fn frame(&mut self, b: Option<Blade>) {
            let o = self.g.step(self.t, b, (self.x, self.y));
            self.cmds.extend(o.cmd);
            self.engaged.extend(o.engaged);
            self.t += STEP;
        }
        fn hold(&mut self, b: Option<Blade>, ms: u64) {
            for _ in 0..(ms / STEP + 1) {
                self.frame(b);
            }
        }
        /// Move the hand by (dx, dy) over `ms` while holding `b` (image coordinates, not mirrored).
        fn sweep(&mut self, b: Option<Blade>, dx: f64, dy: f64, ms: u64) {
            let n = (ms / STEP).max(1);
            for _ in 0..n {
                self.x += dx / n as f64;
                self.y += dy / n as f64;
                self.frame(b);
            }
        }
        /// The user's RIGHT is image-left because the camera is not mirrored.
        fn flick_right(&mut self, b: Blade) {
            self.sweep(Some(b), -0.14, 0.0, 250);
        }
        fn flick_left(&mut self, b: Blade) {
            self.sweep(Some(b), 0.14, 0.0, 250);
        }
        fn engage(&mut self, b: Blade) {
            self.hold(Some(b), 400);
            assert_eq!(self.g.engaged(), Some(b));
        }
    }

    #[test]
    fn a_pose_flashed_for_less_than_the_hold_time_never_engages() {
        let mut r = Rig::new();
        r.hold(Some(Blade::Three), 150);
        r.sweep(Some(Blade::Three), -0.3, 0.0, 120);
        assert_eq!(r.g.engaged(), None);
        assert!(r.cmds.is_empty(), "{:?}", r.cmds);
    }

    #[test]
    fn three_fingers_flick_right_and_left_change_tabs() {
        let mut r = Rig::new();
        r.engage(Blade::Three);
        r.flick_right(Blade::Three);
        assert_eq!(r.cmds, vec![Cmd::NextTab]);
        // Wait out the return-stroke guard, then flick the other way.
        r.hold(Some(Blade::Three), 700);
        r.flick_left(Blade::Three);
        assert_eq!(r.cmds, vec![Cmd::NextTab, Cmd::PrevTab]);
    }

    #[test]
    fn the_return_stroke_of_a_flick_does_not_fire_the_opposite_command() {
        let mut r = Rig::new();
        r.engage(Blade::Three);
        r.flick_right(Blade::Three);
        // Hand comes straight back at once (a natural re-centre).
        r.flick_left(Blade::Three);
        assert_eq!(r.cmds, vec![Cmd::NextTab], "the way back is not a command");
    }

    #[test]
    fn a_slow_drift_or_a_small_shake_is_not_a_flick() {
        let mut r = Rig::new();
        r.engage(Blade::Three);
        r.sweep(Some(Blade::Three), -0.2, 0.0, 3000); // 0.2 over 3 s: drifting, not flicking
        r.sweep(Some(Blade::Three), 0.05, 0.03, 200); // small jitter
        assert!(r.cmds.is_empty(), "{:?}", r.cmds);
    }

    #[test]
    fn a_diagonal_move_is_ignored_because_neither_axis_dominates() {
        let mut r = Rig::new();
        r.engage(Blade::Three);
        r.sweep(Some(Blade::Three), -0.12, -0.11, 250);
        assert!(r.cmds.is_empty(), "{:?}", r.cmds);
    }

    #[test]
    fn a_flick_is_one_command_and_a_second_flick_needs_a_pause() {
        let mut r = Rig::new();
        r.engage(Blade::Three);
        // One long continuous stroke is still ONE command (the refractory swallows the rest of it).
        r.sweep(Some(Blade::Three), -0.3, 0.0, 350);
        assert_eq!(r.cmds, vec![Cmd::NextTab], "{:?}", r.cmds);
        r.hold(Some(Blade::Three), 600);
        r.flick_right(Blade::Three);
        assert_eq!(r.cmds, vec![Cmd::NextTab, Cmd::NextTab]);
    }

    #[test]
    fn the_window_switcher_opens_steps_and_commits_by_dropping_the_pose() {
        let mut r = Rig::new();
        r.engage(Blade::Three);
        r.sweep(Some(Blade::Three), 0.0, -0.14, 250); // flick UP
        assert_eq!(r.cmds, vec![Cmd::SwitcherOpen]);
        assert!(r.g.switcher_open());
        r.hold(Some(Blade::Three), 700);
        r.flick_right(Blade::Three);
        assert_eq!(*r.cmds.last().unwrap(), Cmd::SwitcherNext);
        r.hold(Some(Blade::Three), 700);
        r.flick_left(Blade::Three);
        assert_eq!(*r.cmds.last().unwrap(), Cmd::SwitcherPrev);
        // Drop the pose: commit.
        r.hold(None, 400);
        assert_eq!(*r.cmds.last().unwrap(), Cmd::SwitcherCommit);
        assert!(!r.g.switcher_open());
    }

    #[test]
    fn a_one_frame_flicker_of_the_pose_does_not_commit_the_switcher() {
        let mut r = Rig::new();
        r.engage(Blade::Three);
        r.sweep(Some(Blade::Three), 0.0, -0.14, 250);
        assert!(r.g.switcher_open());
        r.frame(None);
        r.frame(Some(Blade::Three));
        assert!(r.g.switcher_open(), "a single missed frame is not letting go");
        assert_eq!(r.cmds, vec![Cmd::SwitcherOpen]);
    }

    #[test]
    fn the_return_stroke_after_opening_the_switcher_does_not_commit_it() {
        let mut r = Rig::new();
        r.engage(Blade::Three);
        r.sweep(Some(Blade::Three), 0.0, -0.14, 250);
        r.sweep(Some(Blade::Three), 0.0, 0.14, 250); // hand comes back down
        assert_eq!(r.cmds, vec![Cmd::SwitcherOpen], "{:?}", r.cmds);
        assert!(r.g.switcher_open());
        // A deliberate downward flick later does commit.
        r.hold(Some(Blade::Three), 700);
        r.sweep(Some(Blade::Three), 0.0, 0.14, 250);
        assert_eq!(*r.cmds.last().unwrap(), Cmd::SwitcherCommit);
    }

    #[test]
    fn cancel_releases_an_open_switcher_and_only_then() {
        let mut r = Rig::new();
        assert_eq!(r.g.cancel(), None);
        r.engage(Blade::Three);
        assert_eq!(r.g.cancel(), None, "tab mode holds no key");
        r.engage(Blade::Three);
        r.sweep(Some(Blade::Three), 0.0, -0.14, 250);
        assert!(r.g.switcher_open());
        assert_eq!(r.g.cancel(), Some(Cmd::SwitcherCancel));
        assert!(!r.g.switcher_open());
        assert_eq!(r.g.engaged(), None);
    }

    #[test]
    fn the_switcher_cancels_itself_after_the_timeout() {
        let mut r = Rig::new();
        r.engage(Blade::Three);
        r.sweep(Some(Blade::Three), 0.0, -0.14, 250);
        assert!(r.g.switcher_open());
        r.hold(Some(Blade::Three), 10_500);
        assert_eq!(*r.cmds.last().unwrap(), Cmd::SwitcherCancel);
        assert!(!r.g.switcher_open());
    }

    #[test]
    fn four_fingers_switch_desktops_task_view_and_show_desktop() {
        let mut r = Rig::new();
        r.engage(Blade::Four);
        r.flick_right(Blade::Four);
        assert_eq!(r.cmds, vec![Cmd::DesktopNext]);
        r.hold(Some(Blade::Four), 700);
        r.flick_left(Blade::Four);
        assert_eq!(*r.cmds.last().unwrap(), Cmd::DesktopPrev);
        r.hold(Some(Blade::Four), 700);
        r.sweep(Some(Blade::Four), 0.0, -0.14, 250);
        assert_eq!(*r.cmds.last().unwrap(), Cmd::TaskView);
        r.hold(Some(Blade::Four), 700);
        r.sweep(Some(Blade::Four), 0.0, 0.14, 250);
        assert_eq!(*r.cmds.last().unwrap(), Cmd::ShowDesktop);
    }

    #[test]
    fn switching_from_three_to_four_commits_an_open_switcher() {
        let mut r = Rig::new();
        r.engage(Blade::Three);
        r.sweep(Some(Blade::Three), 0.0, -0.14, 250);
        assert!(r.g.switcher_open());
        r.hold(Some(Blade::Four), 400);
        assert_eq!(*r.cmds.last().unwrap(), Cmd::SwitcherCommit);
        assert_eq!(r.g.engaged(), Some(Blade::Four));
    }

    #[test]
    fn mirror_off_flips_the_horizontal_direction() {
        let mut r = Rig::new();
        r.g = Gestures::new(Params { mirror: false, ..Params::default() });
        r.engage(Blade::Three);
        r.sweep(Some(Blade::Three), 0.14, 0.0, 250); // image right = user right when not mirrored
        assert_eq!(r.cmds, vec![Cmd::NextTab]);
    }

    #[test]
    fn engagement_changes_are_reported_once_each_way() {
        let mut r = Rig::new();
        r.engage(Blade::Three);
        r.hold(None, 400);
        assert_eq!(r.engaged, vec![Some(Blade::Three), None]);
    }
}
