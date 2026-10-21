//! Pure hand-pointer state machine: landmarks + a clock in, mouse intents out. No I/O, no globals.
//!
//! Design (each rule exists because every public hand-mouse project hit the problem it solves):
//! * The pointer is driven by the PALM centre, not a fingertip, and smoothed with a One Euro filter
//!   (jitter of a still hand; the fingers moving during a pinch do not move the palm).
//! * Movement is RELATIVE (trackpad-like): the hand can rest and re-centre; hand lost = no jump.
//! * A click lands where the cursor was `rewind_ms` BEFORE the pinch closed, then the cursor stays
//!   locked until release: the pinch itself can no longer pull the click off target.
//! * Quick pinch = click, held pinch = drag, thumb+middle = right click, two fingers up = scroll.
//! * The pointer only acts while ARMED. Arming toggles with palm -> fist -> palm -> fist (twice).
//! * Losing the hand releases any held button; staying lost disarms.

use super::filter::{OneEuro, OneEuroParams};
use super::gesture::{self, Gestures};
use super::pose::{self, Hand, Pose};
use serde::Serialize;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Button {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Move { x: i32, y: i32 },
    Click { x: i32, y: i32, button: Button },
    DragStart { x: i32, y: i32 },
    DragEnd { x: i32, y: i32 },
    /// Wheel notches, positive = scroll down (content moves up).
    Scroll { lines: i32 },
    ArmChanged { armed: bool, reason: &'static str },
    /// The stable pose changed (debounced). Logged with the finger ratios and the fingertip spread for threshold tuning.
    PoseChanged { from: &'static str, to: &'static str, ratios: [f64; 4], spread: f64 },
    /// The user asked to leave hand mode without voice (long fist after disarming, or idle for too long).
    ExitMode { reason: &'static str },
    /// A touchpad-style gesture command (tab / window / desktop switching).
    Gesture(gesture::Cmd),
    /// The gesture pose (3 or 4 fingers) was engaged or released; logged so the thresholds can be tuned.
    BladeChanged { to: &'static str, spread: f64 },
}

/// One monitor in the virtual desktop, physical pixels (a left-hand monitor has a negative x).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mon {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Mon {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w: w.max(1.0), h: h.max(1.0) }
    }

    fn contains(&self, p: (f64, f64)) -> bool {
        p.0 >= self.x && p.0 <= self.x + self.w - 1.0 && p.1 >= self.y && p.1 <= self.y + self.h - 1.0
    }

    /// Nearest point inside the monitor.
    fn clamp(&self, p: (f64, f64)) -> (f64, f64) {
        (p.0.clamp(self.x, self.x + self.w - 1.0), p.1.clamp(self.y, self.y + self.h - 1.0))
    }

    fn dist2(&self, p: (f64, f64)) -> f64 {
        let c = self.clamp(p);
        (c.0 - p.0).powi(2) + (c.1 - p.1).powi(2)
    }
}

/// A point on the desktop: kept where it is if some monitor contains it, otherwise moved to the nearest
/// monitor (the black gap between two monitors of different sizes is not a place a cursor can be).
fn clamp_to_monitors(mons: &[Mon], p: (f64, f64)) -> (f64, f64) {
    if mons.iter().any(|m| m.contains(p)) {
        return p;
    }
    mons.iter()
        .min_by(|a, b| a.dist2(p).partial_cmp(&b.dist2(p)).unwrap())
        .map(|m| m.clamp(p))
        .unwrap_or(p)
}

fn monitor_at(mons: &[Mon], p: (f64, f64)) -> Mon {
    mons.iter()
        .find(|m| m.contains(p))
        .or_else(|| mons.iter().min_by(|a, b| a.dist2(p).partial_cmp(&b.dist2(p)).unwrap()))
        .copied()
        .unwrap_or(Mon::new(0.0, 0.0, 1920.0, 1080.0))
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Every monitor of the virtual desktop. Hand motion maps to the monitor the cursor is on, so a
    /// 4K screen and a 1080p screen both feel the same size to the hand, and the cursor can cross over.
    pub monitors: Vec<Mon>,
    /// Webcam images are not mirrored: hand moving to the user's right appears on the image's left.
    pub mirror: bool,
    /// Camera frame height / width (4:3 = 0.75), so a vertical hand move maps to the same distance as a horizontal one.
    pub aspect: f64,
    pub base_gain: f64,
    /// How much faster a quick hand moves the cursor than a slow one (1.0 = up to double).
    pub accel_gain: f64,
    /// The user's own pointer speed setting as a multiplier (Windows speed 10 = 1.0). Scales the whole mapping.
    pub sensitivity: f64,
    /// "Enhance pointer precision" (mouse acceleration) is on in the OS: keep the acceleration curve; off = nearly linear.
    pub precision: bool,
    pub pinch_close: f64,
    pub pinch_open: f64,
    /// A press shorter than this is a click; longer starts a drag.
    pub click_max_ms: u64,
    pub rewind_ms: u64,
    /// The index finger counts as "pressed" below this tip/PIP ratio and "released" above `curl_release`.
    pub curl_press: f64,
    pub curl_release: f64,
    /// After a click ends, a new press within this long is ignored (one action = one click).
    pub click_refractory_ms: u64,
    /// A fast pinch/flick at low camera fps can have the distance drop below threshold and the pose
    /// gate pass on two DIFFERENT nearby frames rather than the same one: the press signal is bridged
    /// across this many recent ms instead of requiring exact-frame overlap ("pinch slowly or it misses").
    pub press_catch_ms: u64,
    pub lost_release_ms: u64,
    pub lost_disarm_ms: u64,
    /// A pose counts once it has been seen this long AND on at least two frames in a row.
    pub arm_debounce_ms: u64,
    /// Each step of palm-fist-palm-fist must follow the previous one within this long (a slow camera
    /// makes the whole gesture slow; a total-time window punished exactly that).
    pub arm_step_ms: u64,
    /// Holding a fist this long while armed disarms (the arm gesture itself only ever arms).
    pub fist_disarm_ms: u64,
    /// After a fist-hold disarm, keeping the fist closed this much longer ends hand mode (a way out without voice).
    pub fist_exit_ms: u64,
    /// No hand at all for this long while disarmed ends hand mode (the camera must not stay on forever).
    pub idle_exit_ms: u64,
    /// Tab / window gestures (3 fingers). Off in tests unless asked for; the glue turns it on from settings.
    pub gestures: bool,
    /// Desktop gestures (4 fingers together). Needs the spread threshold checked on a real hand, so default off.
    pub gestures_desktop: bool,
    pub filter: OneEuroParams,
}

impl Config {
    pub fn new(screen: (f64, f64)) -> Self {
        Self {
            monitors: vec![Mon::new(0.0, 0.0, screen.0, screen.1)],
            mirror: true,
            aspect: 0.75,
            base_gain: 2.4,
            accel_gain: 2.2,
            sensitivity: 1.0,
            precision: true,
            pinch_close: 0.38,
            pinch_open: 0.6,
            click_max_ms: 450,
            rewind_ms: 150,
            curl_press: 1.02,
            curl_release: 1.15,
            click_refractory_ms: 150,
            press_catch_ms: 150,
            lost_release_ms: 600,
            lost_disarm_ms: 5000,
            arm_debounce_ms: 80,
            arm_step_ms: 3000,
            fist_disarm_ms: 1200,
            fist_exit_ms: 2000,
            idle_exit_ms: 600_000,
            gestures: false,
            gestures_desktop: false,
            filter: OneEuroParams { min_cutoff: 1.5, beta: 18.0, d_cutoff: 1.0 },
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Snapshot {
    pub tracking: bool,
    pub armed: bool,
    /// Size of the monitor the cursor is on (physical px); `cursor` below is inside it.
    pub screen: (f64, f64),
    /// True when that monitor is the primary one (origin 0,0): the stage overlay lives there.
    pub primary: bool,
    pub pose: &'static str,
    pub pinch: f64,
    pub pressed: bool,
    pub dragging: bool,
    pub scrolling: bool,
    /// "" or the engaged gesture pose: "tabs" (3 fingers), "windows" (switcher open), "desktops" (4 fingers).
    pub gesture: &'static str,
    pub cursor: (i32, i32),
    /// 0..=3: how far through palm-fist-palm-fist the user is.
    pub arm_stage: u8,
}

fn pose_name(p: Option<Pose>) -> &'static str {
    match p {
        Some(Pose::Palm) => "palm",
        Some(Pose::Fist) => "fist",
        Some(Pose::Peace) => "peace",
        Some(Pose::Other) => "other",
        None => "none",
    }
}

// ── arming gesture ────────────────────────────────────────────────────────────────────────────

/// Debounces the raw per-frame pose into a stable one. Frame-count aware: at 10 fps a frame is 100 ms,
/// so a pure time rule would demand three frames of a quick fist; two frames and 80 ms is enough.
#[derive(Debug, Default)]
struct PoseTracker {
    cand: Option<(Pose, u64, u32)>,
    stable: Option<Pose>,
}

impl PoseTracker {
    fn reset(&mut self) {
        *self = Self::default();
    }

    /// Some((previous stable, new stable)) when the stable pose just changed.
    fn update(&mut self, t: u64, pose: Pose, debounce: u64) -> Option<(Option<Pose>, Pose)> {
        self.cand = match self.cand {
            Some((p, since, n)) if p == pose => Some((p, since, n + 1)),
            _ => Some((pose, t, 1)),
        };
        let (cp, since, n) = self.cand.expect("set above");
        if t.saturating_sub(since) < debounce || n < 2 || self.stable == Some(cp) {
            return None;
        }
        let prev = self.stable;
        self.stable = Some(cp);
        Some((prev, cp))
    }
}

#[derive(Debug, Default)]
struct ArmDetector {
    stage: u8,
    last_step: u64,
}

impl ArmDetector {
    fn reset(&mut self) {
        *self = Self::default();
    }

    /// A new stable pose `cp` at `t`; true when palm-fist-palm-fist has just completed.
    fn on_stable(&mut self, t: u64, cp: Pose, step_ms: u64) -> bool {
        if self.stage > 0 && t.saturating_sub(self.last_step) > step_ms {
            self.stage = 0;
        }
        match (self.stage, cp) {
            (0, Pose::Palm) | (1, Pose::Palm) => {
                self.stage = 1;
                self.last_step = t;
            }
            (1, Pose::Fist) => {
                self.stage = 2;
                self.last_step = t;
            }
            (2, Pose::Palm) => {
                self.stage = 3;
                self.last_step = t;
            }
            (3, Pose::Fist) => {
                self.stage = 0;
                return true;
            }
            (_, Pose::Other) | (_, Pose::Peace) => {}
            _ => self.stage = 0,
        }
        false
    }
}

// ── pinch tracking ────────────────────────────────────────────────────────────────────────────

/// Records the last time `now` was true into `slot`, and reports true if `now` is true OR `slot`
/// held a true within `catch` ms of `t` — bridging a condition across a short recent window instead
/// of requiring it to hold on this exact frame.
fn bridge(now: bool, slot: &mut Option<u64>, t: u64, catch: u64) -> bool {
    if now {
        *slot = Some(t);
        true
    } else if slot.is_some_and(|t0| t.saturating_sub(t0) <= catch) {
        true
    } else {
        *slot = None;
        false
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct Pinch {
    closed: bool,
    since: u64,
    lock: (f64, f64),
    long: bool,
}

pub struct Engine {
    cfg: Config,
    armed: bool,
    tracker: PoseTracker,
    arm: ArmDetector,
    fist_since: Option<u64>,
    /// Set when a fist-hold just disarmed the pointer: the fist kept closed from here on exits the mode.
    exit_fist_since: Option<u64>,
    /// After arming (which ends on a fist) the hand must open once before a fist can disarm.
    fist_ready: bool,
    fx: OneEuro,
    fy: OneEuro,
    cursor: (f64, f64),
    last_sent: (i32, i32),
    last_palm: Option<(f64, f64)>,
    last_t: Option<u64>,
    history: VecDeque<(u64, (f64, f64))>,
    left: Pinch,
    right: Pinch,
    /// Press-catch bridging state for the left pinch/flick (see `bridge`): last time each half of the
    /// press condition (distance-below-threshold, pose-gate-ok) was independently true.
    left_r_below_t: Option<u64>,
    left_idx_below_t: Option<u64>,
    /// Same bridging state for the right (thumb-middle) pinch.
    right_r_below_t: Option<u64>,
    gest: Gestures,
    dragging: bool,
    last_release: Option<u64>,
    lost_since: Option<u64>,
    scroll_anchor: Option<f64>,
    scroll_acc: f64,
    last_pose: Option<Pose>,
    last_pinch: f64,
    /// Measured capture-to-landmarks delay (ms) the pointer is led by (set by the glue, 0 = off).
    predict_ms: f64,
    prev_filtered: Option<(f64, f64, u64)>,
    vel: (f64, f64),
}

/// Never lead the hand by more than this fraction of the camera frame, whatever the speed.
const MAX_LEAD: f64 = 0.04;

impl Engine {
    pub fn new(cfg: Config) -> Self {
        let f = OneEuro::new(cfg.filter);
        let first = cfg.monitors.first().copied().unwrap_or(Mon::new(0.0, 0.0, 1920.0, 1080.0));
        let cursor = (first.x + first.w / 2.0, first.y + first.h / 2.0);
        let mirror = cfg.mirror;
        Self {
            fx: f.clone(),
            fy: f,
            cursor,
            last_sent: (cursor.0 as i32, cursor.1 as i32),
            cfg,
            armed: false,
            tracker: PoseTracker::default(),
            arm: ArmDetector::default(),
            fist_since: None,
            exit_fist_since: None,
            fist_ready: false,
            last_palm: None,
            last_t: None,
            history: VecDeque::new(),
            left: Pinch::default(),
            right: Pinch::default(),
            left_r_below_t: None,
            left_idx_below_t: None,
            right_r_below_t: None,
            gest: Gestures::new(gesture::Params { mirror, ..gesture::Params::default() }),
            dragging: false,
            last_release: None,
            lost_since: None,
            scroll_anchor: None,
            scroll_acc: 0.0,
            last_pose: None,
            last_pinch: 1.0,
            predict_ms: 0.0,
            prev_filtered: None,
            vel: (0.0, 0.0),
        }
    }

    /// The user's pointer settings (read from the OS when arming, so a change in Settings is picked up).
    pub fn set_pointer(&mut self, sensitivity: f64, precision: bool) {
        self.cfg.sensitivity = sensitivity.clamp(0.1, 4.0);
        self.cfg.precision = precision;
    }

    /// Lead the pointer by the measured pipeline delay so it lands where the hand IS, not where it was.
    pub fn set_predict_ms(&mut self, ms: f64) {
        self.predict_ms = ms.clamp(0.0, 90.0);
    }

    /// Snapshot of an engine that has seen no hand yet (the HUD shows it while the camera starts).
    pub fn idle_snapshot(&self) -> Snapshot {
        self.snapshot(false)
    }

    /// Constant-velocity lead on the filtered palm position (smoothed velocity, clamped).
    fn lead(&mut self, fx: f64, fy: f64, t_ms: u64) -> (f64, f64) {
        let v = match self.prev_filtered {
            Some((px, py, pt)) if t_ms > pt => {
                let dt = (t_ms - pt) as f64 / 1000.0;
                ((fx - px) / dt, (fy - py) / dt)
            }
            _ => (0.0, 0.0),
        };
        self.prev_filtered = Some((fx, fy, t_ms));
        self.vel = (0.5 * self.vel.0 + 0.5 * v.0, 0.5 * self.vel.1 + 0.5 * v.1);
        let lead = self.predict_ms / 1000.0;
        (
            fx + (self.vel.0 * lead).clamp(-MAX_LEAD, MAX_LEAD),
            fy + (self.vel.1 * lead).clamp(-MAX_LEAD, MAX_LEAD),
        )
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    /// Where the real cursor is right now (called when arming, so relative motion starts from it).
    pub fn seed_cursor(&mut self, x: i32, y: i32) {
        self.cursor = (x as f64, y as f64);
        self.last_sent = (x, y);
        self.history.clear();
    }

    /// Arm/disarm from outside (voice, physical mouse takeover). Releases anything held.
    pub fn set_armed(&mut self, armed: bool, reason: &'static str) -> Vec<Action> {
        let mut out = Vec::new();
        if armed == self.armed {
            return out;
        }
        if !armed {
            self.release_all(&mut out);
        }
        self.armed = armed;
        self.last_palm = None;
        self.scroll_anchor = None;
        self.fist_since = None;
        self.fist_ready = false;
        self.prev_filtered = None;
        self.vel = (0.0, 0.0);
        self.arm.reset();
        out.push(Action::ArmChanged { armed, reason });
        out
    }

    fn pos(&self) -> (i32, i32) {
        (self.cursor.0.round() as i32, self.cursor.1.round() as i32)
    }

    /// Forget the recent-true memory of the press conditions. Without this a short click leaves them
    /// "recently true" and the very next frames count as a second press (double click).
    fn clear_press_bridges(&mut self) {
        self.left_r_below_t = None;
        self.left_idx_below_t = None;
        self.right_r_below_t = None;
    }

    fn release_all(&mut self, out: &mut Vec<Action>) {
        if self.dragging {
            let (x, y) = self.pos();
            out.push(Action::DragEnd { x, y });
        }
        self.dragging = false;
        self.left = Pinch::default();
        self.right = Pinch::default();
        self.left_r_below_t = None;
        self.left_idx_below_t = None;
        self.right_r_below_t = None;
        self.scroll_anchor = None;
        self.scroll_acc = 0.0;
        // An open window switcher holds Alt down: every way of stopping must let it go.
        if let Some(c) = self.gest.cancel() {
            out.push(Action::Gesture(c));
        }
    }

    fn snapshot(&self, tracking: bool) -> Snapshot {
        let m = monitor_at(&self.cfg.monitors, self.cursor);
        let (px, py) = self.pos();
        Snapshot {
            tracking,
            armed: self.armed,
            screen: (m.w, m.h),
            primary: m.x == 0.0 && m.y == 0.0,
            pose: pose_name(if tracking { self.last_pose } else { None }),
            pinch: self.last_pinch,
            pressed: self.left.closed || self.right.closed,
            dragging: self.dragging,
            scrolling: self.scroll_anchor.is_some(),
            gesture: match (self.gest.engaged(), self.gest.switcher_open()) {
                (Some(pose::Blade::Three), true) => "windows",
                (Some(pose::Blade::Three), false) => "tabs",
                (Some(pose::Blade::Four), _) => "desktops",
                (None, _) => "",
            },
            cursor: (px - m.x.round() as i32, py - m.y.round() as i32),
            arm_stage: self.arm.stage,
        }
    }

    pub fn step(&mut self, t_ms: u64, hand: Option<&Hand>) -> (Vec<Action>, Snapshot) {
        let mut out = Vec::new();
        let hand = hand.filter(|h| pose::plausible(h));

        let Some(h) = hand else {
            let since = *self.lost_since.get_or_insert(t_ms);
            let gone = t_ms.saturating_sub(since);
            if gone >= self.cfg.lost_release_ms {
                self.release_all(&mut out);
                self.fx.reset();
                self.fy.reset();
                self.last_palm = None;
                self.arm.reset();
                self.tracker.reset();
                self.fist_since = None;
                self.prev_filtered = None;
                self.vel = (0.0, 0.0);
                self.last_pose = None;
            }
            if gone >= self.cfg.lost_disarm_ms && self.armed {
                out.extend(self.set_armed(false, "hand-lost"));
            }
            if !self.armed && gone >= self.cfg.idle_exit_ms {
                self.lost_since = Some(t_ms); // one request, then count again if the session somehow survives
                out.push(Action::ExitMode { reason: "idle" });
            }
            self.last_t = Some(t_ms);
            return (out, self.snapshot(false));
        };
        self.lost_since = None;

        let p = pose::pose(h);
        self.last_pose = Some(p);

        // Stable-pose changes: logged for tuning; while DISARMED they also drive the arm gesture
        // (palm, fist, palm, fist). The gesture only ever ARMS: repeating it can no longer flip the
        // pointer back off by accident.
        if let Some((from, to)) = self.tracker.update(t_ms, p, self.cfg.arm_debounce_ms) {
            out.push(Action::PoseChanged { from: pose_name(from), to: pose_name(Some(to)), ratios: pose::finger_ratios(h), spread: pose::spread(h) });
            if !self.armed && self.arm.on_stable(t_ms, to, self.cfg.arm_step_ms) {
                out.extend(self.set_armed(true, "gesture"));
            }
        }
        if self.armed {
            // A fist held for `fist_disarm_ms` disarms, but only after the hand has opened once since arming.
            if p != Pose::Fist {
                self.fist_ready = true;
                self.fist_since = None;
            } else if self.fist_ready {
                let since = *self.fist_since.get_or_insert(t_ms);
                if t_ms.saturating_sub(since) >= self.cfg.fist_disarm_ms {
                    out.extend(self.set_armed(false, "fist-hold"));
                    self.exit_fist_since = Some(t_ms);
                }
            }
        }

        let dt_ms = t_ms.saturating_sub(self.last_t.unwrap_or(t_ms));
        self.last_t = Some(t_ms);
        let t_s = t_ms as f64 / 1000.0;
        let (px, py) = pose::palm_center(h);
        let (fx, fy) = (self.fx.filter(px, t_s), self.fy.filter(py, t_s));

        if !self.armed {
            // A fist kept closed after a fist-hold disarm leaves hand mode altogether.
            match self.exit_fist_since {
                Some(s0) if p == Pose::Fist => {
                    if t_ms.saturating_sub(s0) >= self.cfg.fist_exit_ms {
                        self.exit_fist_since = None;
                        out.push(Action::ExitMode { reason: "fist" });
                    }
                }
                Some(_) => self.exit_fist_since = None,
                None => {}
            }
            self.last_palm = Some((fx, fy));
            return (out, self.snapshot(true));
        }

        self.last_pinch = pose::pinch_ratio(h, 8);

        let left_before = self.left.closed;
        self.update_left(t_ms, h, p, &mut out);
        if !left_before && !self.left.closed {
            self.update_right(t_ms, h, &mut out);
        }

        let scrolling = p == Pose::Peace && !self.left.closed && !self.right.closed;
        if scrolling {
            self.update_scroll(fy, dt_ms, &mut out);
        } else {
            self.scroll_anchor = None;
            self.scroll_acc = 0.0;
        }

        // Touchpad-style gestures: only while no click / drag / scroll is in progress. The pointer stays
        // put while a gesture pose is held, otherwise the flick itself would drag the cursor along.
        let blade = if self.cfg.gestures && !self.left.closed && !self.right.closed && !self.dragging && !scrolling {
            pose::blade_pose(h).filter(|b| self.cfg.gestures_desktop || *b != pose::Blade::Four)
        } else {
            None
        };
        let g = self.gest.step(t_ms, blade, (fx, fy));
        if let Some(e) = g.engaged {
            let to = match e {
                Some(pose::Blade::Three) => "three",
                Some(pose::Blade::Four) => "four",
                None => "none",
            };
            out.push(Action::BladeChanged { to, spread: pose::spread(h) });
        }
        if let Some(c) = g.cmd {
            out.push(Action::Gesture(c));
        }
        let gesturing = blade.is_some() || self.gest.engaged().is_some();

        let locked = (self.left.closed && !self.dragging) || self.right.closed || scrolling || p == Pose::Fist || gesturing;
        let (ax, ay) = self.lead(fx, fy, t_ms);
        if let (Some((lx, ly)), false) = (self.last_palm, locked) {
            self.move_cursor(ax - lx, ay - ly, dt_ms);
        }
        self.last_palm = Some((ax, ay));

        self.history.push_back((t_ms, self.cursor));
        while self.history.front().is_some_and(|(t, _)| t_ms.saturating_sub(*t) > 600) {
            self.history.pop_front();
        }
        let now = self.pos();
        if now != self.last_sent {
            self.last_sent = now;
            out.push(Action::Move { x: now.0, y: now.1 });
        }
        (out, self.snapshot(true))
    }

    fn move_cursor(&mut self, dx_n: f64, dy_n: f64, dt_ms: u64) {
        let dx_n = if self.cfg.mirror { -dx_n } else { dx_n };
        let dt = (dt_ms.max(1)) as f64 / 1000.0;
        let speed = (dx_n * dx_n + (dy_n * self.cfg.aspect).powi(2)).sqrt() / dt;
        // Mouse-like ballistics: slow hand = precise (base), quick hand = covers the screen. The user's own
        // pointer speed scales everything; with "enhance pointer precision" off the curve is nearly flat.
        let ramp = ((speed - 0.12) / 1.3).clamp(0.0, 1.0);
        let curve = ramp * ramp * (3.0 - 2.0 * ramp);
        let accel = if self.cfg.precision { self.cfg.accel_gain } else { self.cfg.accel_gain * 0.25 };
        let gain = self.cfg.base_gain * self.cfg.sensitivity.clamp(0.1, 4.0) * (1.0 + accel * curve);
        // Scale by the monitor the cursor is on, then let the result cross onto a neighbour if it lands there.
        let m = monitor_at(&self.cfg.monitors, self.cursor);
        let next = (self.cursor.0 + dx_n * m.w * gain, self.cursor.1 + dy_n * self.cfg.aspect * m.w * gain);
        self.cursor = clamp_to_monitors(&self.cfg.monitors, next);
    }

    /// The monitor layout changed (plugged, unplugged, resolution or scale change): adopt it and keep
    /// the cursor on a real monitor. An empty list is ignored.
    pub fn set_monitors(&mut self, mons: Vec<Mon>) {
        if mons.is_empty() || mons == self.cfg.monitors {
            return;
        }
        self.cfg.monitors = mons;
        self.cursor = clamp_to_monitors(&self.cfg.monitors, self.cursor);
        self.history.clear();
    }

    /// Cursor position `rewind_ms` ago: where the user was aiming before the fingers started to close.
    fn rewound(&self, t_ms: u64) -> (f64, f64) {
        self.history
            .iter()
            .find(|(t, _)| t + self.cfg.rewind_ms >= t_ms)
            .map(|(_, p)| *p)
            .unwrap_or(self.cursor)
    }

    fn lock_at(&mut self, t_ms: u64) -> (f64, f64) {
        let lock = self.rewound(t_ms);
        self.cursor = lock;
        lock
    }

    fn update_left(&mut self, t: u64, h: &Hand, p: Pose, out: &mut Vec<Action>) {
        // A fist brings the thumb to the index and bends it: those frames must never be bridged into a press
        // when the hand opens again a moment later (fist -> palm used to read as a click).
        if p == Pose::Fist {
            self.clear_press_bridges();
        }
        let r = self.last_pinch;
        let free_hand = pose::index_pinch_pose_ok(h) && p != Pose::Fist;
        // Thumb nearer the index than the middle finger, or this is a right-click pinch, not a left one.
        let pinch_ok = free_hand && r <= pose::pinch_ratio(h, 12);
        let idx = pose::finger_ratios(h)[0];
        let was = self.left;
        let flick_hand = p != Pose::Fist && pose::index_flick_ok(h);
        // Two ways to press, one state machine: thumb+index pinch, or just bending the index finger
        // (the other fingers stay open). Doing both at once is still ONE click.
        let (want_pinch, want_curl) = if was.closed {
            (r < self.cfg.pinch_open && pinch_ok, flick_hand && idx < self.cfg.curl_release)
        } else {
            let catch = self.cfg.press_catch_ms;
            // A fast pinch/flick at low or uneven camera fps can have the distance drop below
            // threshold on one frame and the pose gate (thumb nearer index than middle; ring+pinky
            // open) pass on a DIFFERENT nearby frame, with no single frame satisfying both at once —
            // the press was silently dropped, never delayed ("I have to pinch slowly or it misses").
            // Bridge each half of the press condition across a short recent window instead of
            // requiring exact-frame overlap. The `was.closed` (hold/release) branch above is untouched.
            // A bridged "recently below the threshold" only counts while the fingers are STILL near it (inside the
            // release band): an open hand a moment after a brush is not a press.
            let r_below = bridge(r < self.cfg.pinch_close && p != Pose::Fist, &mut self.left_r_below_t, t, catch) && r < self.cfg.pinch_open;
            // Only the DISTANCE half is bridged. The pose gate must hold on this very frame: an open palm satisfies
            // it almost always, so bridging it would let a brush from a frame ago press by itself.
            let gate_ok = pinch_ok;
            let idx_below = bridge(idx < self.cfg.curl_press && p != Pose::Fist, &mut self.left_idx_below_t, t, catch) && idx < self.cfg.curl_release;
            let flick_ok = flick_hand;
            // The thumb clearly nearer the MIDDLE fingertip is a right-click pinch, never a left one (a hair
            // nearer is jitter and still counts as left).
            let clearly_middle = pose::pinch_ratio(h, 12) < 0.75 * r;
            (r_below && gate_ok && !clearly_middle, idx_below && flick_ok)
        };
        let mut want = want_pinch || want_curl;
        // The raw signals (thumb still near the index / index still bent) versus the pose gate. A press that
        // ENDS because the fingers genuinely opened is a click even if the pose gate blinked on that very
        // frame (field test 4: thumbs drifting nearer the middle finger on release cancelled real clicks,
        // so people had to pinch three times). It is only cancelled when the signal is still there but the
        // hand turned into something else (a fist closing).
        let signal_still_on = r < self.cfg.pinch_open || idx < self.cfg.curl_release;
        let pose_ok = pinch_ok || flick_hand;
        if !was.closed && want && self.last_release.is_some_and(|t0| t.saturating_sub(t0) < self.cfg.click_refractory_ms) {
            want = false; // chatter right after a click: not a new press
        }
        if !was.closed && want {
            let lock = self.lock_at(t);
            self.clear_press_bridges();
            self.left = Pinch { closed: true, since: t, lock, long: false };
        } else if was.closed && want && !was.long && t.saturating_sub(was.since) >= self.cfg.click_max_ms {
            self.left.long = true;
            self.dragging = true;
            let (x, y) = (was.lock.0.round() as i32, was.lock.1.round() as i32);
            out.push(Action::DragStart { x, y });
        } else if was.closed && !want {
            let cancelled = signal_still_on && !pose_ok;
            if self.dragging {
                let (x, y) = self.pos();
                out.push(Action::DragEnd { x, y });
            } else if !cancelled {
                out.push(Action::Click { x: was.lock.0.round() as i32, y: was.lock.1.round() as i32, button: Button::Left });
            }
            self.dragging = false;
            self.left = Pinch::default();
            self.clear_press_bridges();
            self.last_release = Some(t);
        }
    }

    fn update_right(&mut self, t: u64, h: &Hand, out: &mut Vec<Action>) {
        let r = pose::pinch_ratio(h, 12);
        // The thumb must be CLEARLY nearer the middle fingertip: a hair nearer is jitter between two touching
        // fingertips and belongs to the left (index) pinch.
        let ok = pose::middle_pinch_pose_ok(h) && r < 0.75 * pose::pinch_ratio(h, 8);
        let was = self.right;
        let want = if was.closed {
            r < self.cfg.pinch_open && ok
        } else {
            // Same press-catch bridging as the left pinch (see `update_left`): the distance threshold
            // and the pose gate can each land true on a different nearby frame during a fast pinch.
            let catch = self.cfg.press_catch_ms;
            let in_fist = pose::pose(h) == Pose::Fist; // a fist's thumb-near-finger frames are never a press
            let r_below = bridge(r < self.cfg.pinch_close && !in_fist, &mut self.right_r_below_t, t, catch);
            let gate_ok = ok;
            r_below && gate_ok
        };
        if !was.closed && want {
            let lock = self.lock_at(t);
            self.clear_press_bridges();
            self.right = Pinch { closed: true, since: t, lock, long: false };
        } else if was.closed && !want {
            if t.saturating_sub(was.since) < self.cfg.click_max_ms {
                out.push(Action::Click { x: was.lock.0.round() as i32, y: was.lock.1.round() as i32, button: Button::Right });
            }
            self.right = Pinch::default();
            self.clear_press_bridges();
        }
    }

    fn update_scroll(&mut self, fy: f64, dt_ms: u64, out: &mut Vec<Action>) {
        let anchor = *self.scroll_anchor.get_or_insert(fy);
        let off = fy - anchor;
        let rate = ((off.abs() - 0.03).max(0.0) * 160.0).min(25.0) * off.signum();
        self.scroll_acc += rate * dt_ms as f64 / 1000.0;
        let whole = self.scroll_acc.trunc();
        if whole != 0.0 {
            self.scroll_acc -= whole;
            out.push(Action::Scroll { lines: whole as i32 });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::pose::fixtures::*;
    use super::super::pose::Pt;
    use super::*;

    const STEP: u64 = 33;

    fn shifted(h: &Hand, dx: f64, dy: f64) -> Hand {
        let mut o = *h;
        for p in o.iter_mut() {
            *p = Pt { x: p.x + dx, y: p.y + dy, z: p.z };
        }
        o
    }

    fn thumb_at(h: &Hand, x: f64, y: f64) -> Hand {
        let mut o = *h;
        o[4] = Pt { x, y, z: 0.0 };
        o
    }

    struct Rig {
        e: Engine,
        t: u64,
        actions: Vec<Action>,
        snap: Option<Snapshot>,
    }

    impl Rig {
        fn new() -> Self {
            Self { e: Engine::new(Config::new((1920.0, 1080.0))), t: 1000, actions: vec![], snap: None }
        }
        fn frame(&mut self, h: Option<&Hand>) {
            let (a, s) = self.e.step(self.t, h);
            self.actions.extend(a);
            self.snap = Some(s);
            self.t += STEP;
        }
        fn hold(&mut self, h: &Hand, ms: u64) {
            let n = ms / STEP + 1;
            for _ in 0..n {
                self.frame(Some(h));
            }
        }
        fn gone(&mut self, ms: u64) {
            for _ in 0..(ms / STEP + 1) {
                self.frame(None);
            }
        }
        fn arm(&mut self) {
            for _ in 0..2 {
                self.hold(&palm(), 300);
                self.hold(&fist(), 300);
            }
            assert!(self.e.is_armed(), "arming gesture should arm");
            self.e.seed_cursor(960, 540);
            self.actions.clear();
        }
        fn clicks(&self) -> Vec<(i32, i32, Button)> {
            self.actions
                .iter()
                .filter_map(|a| if let Action::Click { x, y, button } = a { Some((*x, *y, *button)) } else { None })
                .collect()
        }
    }

    #[test]
    fn palm_fist_palm_fist_arms_and_repeating_it_does_not_disarm() {
        let mut r = Rig::new();
        assert!(!r.e.is_armed());
        for _ in 0..2 {
            r.hold(&palm(), 300);
            r.hold(&fist(), 300);
        }
        assert!(r.e.is_armed());
        assert!(r.actions.contains(&Action::ArmChanged { armed: true, reason: "gesture" }));
        // "More than twice" used to flip it off again. It must stay armed.
        for _ in 0..3 {
            r.hold(&palm(), 300);
            r.hold(&fist(), 300);
        }
        assert!(r.e.is_armed());
    }

    #[test]
    fn a_held_fist_disarms_but_not_straight_after_arming() {
        let mut r = Rig::new();
        for _ in 0..2 {
            r.hold(&palm(), 300);
            r.hold(&fist(), 300);
        }
        assert!(r.e.is_armed());
        // The arm gesture ended on a fist: keeping it closed for 2 s must NOT disarm.
        r.hold(&fist(), 2000);
        assert!(r.e.is_armed(), "fist right after arming is the end of the gesture, not a disarm");
        // Open the hand once, then hold a fist: disarms with its own reason.
        r.hold(&palm(), 300);
        r.hold(&fist(), 1000);
        assert!(r.e.is_armed(), "1 s is not long enough");
        r.hold(&fist(), 400);
        assert!(!r.e.is_armed());
        assert!(r.actions.contains(&Action::ArmChanged { armed: false, reason: "fist-hold" }));
    }

    #[test]
    fn pose_changes_are_reported_once_debounced_with_finger_ratios() {
        let mut r = Rig::new();
        r.hold(&palm(), 300);
        r.hold(&fist(), 300);
        let changes: Vec<_> = r.actions.iter().filter_map(|a| if let Action::PoseChanged { from, to, ratios, .. } = a { Some((*from, *to, *ratios)) } else { None }).collect();
        assert_eq!(changes.len(), 2, "{changes:?}");
        assert_eq!((changes[0].0, changes[0].1), ("none", "palm"));
        assert_eq!((changes[1].0, changes[1].1), ("palm", "fist"));
        assert!(changes[0].2.iter().all(|x| *x > 1.2) && changes[1].2.iter().all(|x| *x < 1.0));
    }

    #[test]
    fn arming_works_at_ten_frames_per_second_with_quick_poses() {
        let mut r = Rig::new();
        // 100 ms frames, each pose held for just 2 frames (200 ms).
        let frames = |r: &mut Rig, h: &Hand, n: u32| {
            for _ in 0..n {
                let (a, s) = r.e.step(r.t, Some(h));
                r.actions.extend(a);
                r.snap = Some(s);
                r.t += 100;
            }
        };
        for _ in 0..2 {
            frames(&mut r, &palm(), 2);
            frames(&mut r, &fist(), 2);
        }
        frames(&mut r, &palm(), 1);
        assert!(r.e.is_armed(), "slow cameras must still arm: {:?}", r.actions);
    }

    fn two_monitors() -> Vec<Mon> {
        // A 1920x1080 primary and a 2560x1440 monitor to its LEFT, top-aligned: negative x, taller.
        vec![Mon::new(0.0, 0.0, 1920.0, 1080.0), Mon::new(-2560.0, 0.0, 2560.0, 1440.0)]
    }

    #[test]
    fn the_cursor_crosses_to_a_monitor_on_the_left_and_never_lands_in_the_gap() {
        let mut r = Rig::new();
        r.e.set_monitors(two_monitors());
        r.arm();
        r.e.seed_cursor(100, 500);
        r.hold(&palm(), 300);
        // Hand moves to the user's left (larger x in the un-mirrored image) for a long way.
        for i in 0..80 {
            r.frame(Some(&shifted(&palm(), 0.004 * i as f64, 0.0)));
        }
        let s = r.snap.clone().unwrap();
        assert!(!s.primary, "should now be on the left monitor: {s:?}");
        assert_eq!(s.screen, (2560.0, 1440.0));
        assert!(s.cursor.0 >= 0 && s.cursor.0 < 2560 && s.cursor.1 >= 0 && s.cursor.1 < 1440);
        // The absolute position is a real point of the left monitor (x < 0).
        let moves: Vec<_> = r.actions.iter().filter_map(|a| if let Action::Move { x, y } = a { Some((*x, *y)) } else { None }).collect();
        let (lx, ly) = *moves.last().unwrap();
        assert!(lx < 0 && lx >= -2560 && ly >= 0 && ly < 1440, "{moves:?}");
    }

    #[test]
    fn a_point_in_the_gap_between_monitors_snaps_to_the_nearest_one() {
        // Right monitor is shorter: rows 1080..1440 to the right of x=0 are empty desktop.
        let mons = two_monitors();
        let p = clamp_to_monitors(&mons, (500.0, 1300.0));
        assert!(mons.iter().any(|m| m.contains(p)), "{p:?}");
        assert_eq!(p, (500.0, 1079.0));
    }

    #[test]
    fn hand_travel_feels_the_same_size_on_a_big_and_a_small_monitor() {
        let go = |mons: Vec<Mon>, start: (i32, i32)| {
            let mut r = Rig::new();
            r.e.set_monitors(mons);
            r.arm();
            r.e.seed_cursor(start.0, start.1);
            r.hold(&palm(), 300);
            let before = r.snap.clone().unwrap().cursor;
            for i in 0..12 {
                r.frame(Some(&shifted(&palm(), -0.004 * i as f64, 0.0)));
            }
            let after = r.snap.unwrap().cursor;
            (after.0 - before.0) as f64
        };
        let small = go(vec![Mon::new(0.0, 0.0, 1920.0, 1080.0)], (200, 300));
        let big = go(vec![Mon::new(0.0, 0.0, 3840.0, 2160.0)], (200, 300));
        let ratio = big / small;
        assert!((1.8..=2.2).contains(&ratio), "4K should move about twice as many pixels: {small} vs {big}");
    }

    #[test]
    fn changing_the_monitor_layout_keeps_the_cursor_on_a_real_monitor() {
        let mut r = Rig::new();
        r.e.set_monitors(two_monitors());
        r.arm();
        r.e.seed_cursor(-2000, 1300);
        r.e.set_monitors(vec![Mon::new(0.0, 0.0, 1920.0, 1080.0)]); // the left monitor was unplugged
        r.hold(&palm(), 100);
        let s = r.snap.unwrap();
        assert!(s.cursor.0 >= 0 && s.cursor.0 < 1920 && s.cursor.1 >= 0 && s.cursor.1 < 1080, "{s:?}");
    }

    #[test]
    fn a_slow_gesture_arms_when_every_step_is_prompt() {
        // Field test 2: at 5 fps the four poses took 8+ s in total, which a 6 s total window rejected.
        let mut r = Rig::new();
        for _ in 0..2 {
            r.hold(&palm(), 2500);
            r.hold(&fist(), 2500);
        }
        assert!(r.e.is_armed(), "each step within 3 s must arm no matter how long the whole gesture took");
    }

    #[test]
    fn a_long_pause_between_steps_restarts_the_count() {
        let mut r = Rig::new();
        r.hold(&palm(), 300);
        r.hold(&fist(), 4000); // longer than one step allows
        r.hold(&palm(), 300);
        r.hold(&fist(), 300);
        assert!(!r.e.is_armed());
    }

    fn rig_with_predict(ms: f64) -> Rig {
        let mut r = Rig::new();
        r.e.set_predict_ms(ms);
        r
    }

    #[test]
    fn prediction_leads_a_moving_hand_and_leaves_a_still_one_alone() {
        let mut still = rig_with_predict(80.0);
        let mut off = Rig::new();
        for r in [&mut still, &mut off] {
            r.arm();
            r.hold(&palm(), 600);
        }
        assert_eq!(still.snap.clone().unwrap().cursor, off.snap.clone().unwrap().cursor, "a still hand must not drift");

        let (mut a, mut b) = (rig_with_predict(80.0), Rig::new());
        for r in [&mut a, &mut b] {
            r.arm();
            r.hold(&palm(), 300);
            for i in 0..30 {
                r.frame(Some(&shifted(&palm(), -0.006 * i as f64, 0.0)));
            }
        }
        let (ca, cb) = (a.snap.unwrap().cursor, b.snap.unwrap().cursor);
        assert!(ca.0 > cb.0 + 30, "predicted cursor must be ahead of the unpredicted one: {ca:?} vs {cb:?}");
    }

    #[test]
    fn the_lead_is_clamped_whatever_the_speed() {
        let (mut a, mut b) = (rig_with_predict(90.0), Rig::new());
        for r in [&mut a, &mut b] {
            r.arm();
            r.hold(&palm(), 300);
            for i in 0..6 {
                r.frame(Some(&shifted(&palm(), -0.09 * i as f64, 0.0))); // absurdly fast
            }
        }
        let (ca, cb) = (a.snap.unwrap().cursor, b.snap.unwrap().cursor);
        // 0.04 of the frame, mirrored, times gain (<= 4.4) times the screen width, plus slack for the filter.
        assert!((ca.0 - cb.0).abs() as f64 <= 0.04 * 1920.0 * 4.4 + 60.0, "{ca:?} vs {cb:?}");
    }

    #[test]
    fn a_single_palm_fist_or_a_slow_sequence_does_not_arm() {
        let mut r = Rig::new();
        r.hold(&palm(), 300);
        r.hold(&fist(), 300);
        assert!(!r.e.is_armed());
        // A second half that comes too late (> 4 s window) restarts instead of completing.
        r.hold(&fist(), 7000);
        r.hold(&palm(), 300);
        r.hold(&fist(), 300);
        assert!(!r.e.is_armed());
    }

    #[test]
    fn flicker_shorter_than_the_debounce_does_not_count() {
        let mut r = Rig::new();
        for _ in 0..6 {
            r.hold(&palm(), 60);
            r.hold(&fist(), 60);
        }
        assert!(!r.e.is_armed());
    }

    #[test]
    fn nothing_is_sent_while_disarmed() {
        let mut r = Rig::new();
        r.hold(&hand([true; 4], true), 600);
        r.hold(&palm(), 300);
        r.hold(&shifted(&palm(), -0.1, 0.0), 300);
        assert!(r.actions.iter().all(|a| !matches!(a, Action::Move { .. } | Action::Click { .. } | Action::DragStart { .. })));
    }

    #[test]
    fn moving_the_hand_right_moves_the_cursor_right_mirrored() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 300);
        let before = r.snap.clone().unwrap().cursor;
        // The user's right = smaller x in the un-mirrored image.
        for i in 0..20 {
            let h = shifted(&palm(), -0.005 * i as f64, 0.0);
            r.frame(Some(&h));
        }
        let after = r.snap.clone().unwrap().cursor;
        assert!(after.0 > before.0 + 100, "cursor should move right: {before:?} -> {after:?}");
        assert!((after.1 - before.1).abs() < 15, "no vertical drift: {before:?} -> {after:?}");
    }

    #[test]
    fn a_still_hand_does_not_move_the_cursor() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 300);
        let before = r.snap.clone().unwrap().cursor;
        r.hold(&palm(), 2000);
        assert_eq!(r.snap.unwrap().cursor, before);
    }

    #[test]
    fn a_quick_pinch_clicks_where_the_cursor_was_before_the_fingers_closed() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 300);
        // The hand sweeps toward a target; the pinch closes while it is still moving.
        let mut cursors = Vec::new();
        for i in 0..14 {
            r.frame(Some(&shifted(&palm(), -0.005 * i as f64, 0.0)));
            cursors.push(r.snap.clone().unwrap().cursor);
        }
        let p = 14;
        let aim = cursors[p - 4]; // rewind = 150 ms = 4 frames before the first pinch frame
        let at_close = cursors[p - 1];
        assert!((at_close.0 - aim.0).abs() > 30, "test must move enough to tell the two apart: {aim:?} vs {at_close:?}");
        for i in 0..4 {
            r.frame(Some(&shifted(&hand([true; 4], true), -0.005 * (p + i) as f64, 0.0)));
        }
        r.hold(&shifted(&palm(), -0.1, 0.0), 200);
        let clicks = r.clicks();
        assert_eq!(clicks.len(), 1, "exactly one click: {:?}", r.actions);
        assert_eq!(clicks[0].2, Button::Left);
        assert!(
            (clicks[0].0 - aim.0).abs() <= 3 && (clicks[0].1 - aim.1).abs() <= 3,
            "click {:?} should be the rewound position {aim:?}, not where the cursor ended up {at_close:?}",
            clicks[0]
        );
    }

    #[test]
    fn a_fast_pinch_still_clicks_when_the_distance_and_the_gate_land_on_different_frames() {
        // Field report: pinching fast sometimes registers nothing at all, only a slow pinch works.
        // Root cause: at low/uneven camera fps a fast pinch can have the raw thumb-index distance
        // drop below the close threshold on ONE frame while the pose gate (thumb nearer index than
        // middle) only passes on the NEXT frame — no single frame ever satisfies both halves of the
        // press condition at once, so the press was silently dropped, not merely delayed. Reproduce
        // that exact split across two consecutive frames.
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 400);
        let open = palm();
        // Frame A: thumb already close to both tips, but a hair closer to the middle (jitter) — the
        // raw distance reads "closed" but the pose gate fails.
        let frame_a = thumb_at(&open, 0.481, 0.43);
        // Frame B, one camera frame later: thumb has drifted toward the index side (gate now passes),
        // but the raw distance alone has crept back just above the close threshold.
        let frame_b = thumb_at(&open, 0.41, 0.43);
        r.frame(Some(&frame_a));
        r.frame(Some(&frame_b));
        // Release almost immediately (well under click_max_ms), thumb clearly away from both tips.
        r.hold(&open, 100);
        let clicks = r.clicks();
        assert_eq!(clicks.len(), 1, "a fast pinch split across two frames must still click: {:?}", r.actions);
        assert_eq!(clicks[0].2, Button::Left);
        assert!(!r.actions.iter().any(|a| matches!(a, Action::DragStart { .. })), "must be a click, not a drag");
    }

    #[test]
    fn the_press_catch_window_expires_and_does_not_bridge_forever() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 400);
        let open = palm();
        let frame_a = thumb_at(&open, 0.481, 0.43); // distance below threshold, gate fails
        r.frame(Some(&frame_a));
        // Hand stays open (gate false, distance back above threshold) well past the catch window.
        r.hold(&open, 300);
        let frame_b = thumb_at(&open, 0.41, 0.43); // gate now passes, far too late to bridge to A
        r.frame(Some(&frame_b));
        r.hold(&open, 100);
        assert!(r.clicks().is_empty(), "a gate arriving long after the distance dipped must not bridge into a click: {:?}", r.actions);
    }

    #[test]
    fn a_held_pinch_drags_and_does_not_click() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 300);
        r.hold(&hand([true; 4], true), 700);
        assert!(r.actions.iter().any(|a| matches!(a, Action::DragStart { .. })));
        for i in 0..10 {
            let h = shifted(&hand([true; 4], true), -0.01 * i as f64, 0.0);
            r.frame(Some(&h));
        }
        r.hold(&palm(), 200);
        assert!(r.actions.iter().any(|a| matches!(a, Action::DragEnd { .. })));
        assert!(r.clicks().is_empty());
    }

    #[test]
    fn just_bending_the_index_finger_is_one_click() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 400);
        let flick = hand([false, true, true, true], false); // index bent, the other three open
        assert_eq!(pose::pose(&flick), Pose::Other);
        r.hold(&flick, 130);
        r.hold(&palm(), 300);
        let c = r.clicks();
        assert_eq!(c.len(), 1, "{:?}", r.actions);
        assert_eq!(c[0].2, Button::Left);
    }

    #[test]
    fn a_pinch_that_also_bends_the_index_is_still_one_click() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 400);
        let mut both = hand([false, true, true, true], false);
        both[4] = Pt { x: both[8].x - 0.01, y: both[8].y + 0.01, z: 0.0 }; // thumb on the bent index tip too
        r.hold(&both, 130);
        r.hold(&palm(), 300);
        assert_eq!(r.clicks().len(), 1, "{:?}", r.actions);
    }

    #[test]
    fn chatter_right_after_a_click_is_not_a_second_click() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 400);
        let flick = hand([false, true, true, true], false);
        // press, release, press again 66 ms later (inside the 150 ms refractory), release
        for h in [&flick, &flick, &palm(), &flick, &palm(), &palm(), &palm(), &palm(), &palm()] {
            r.frame(Some(h));
        }
        assert_eq!(r.clicks().len(), 1, "one action, one click: {:?}", r.actions);
        // A real second click later is allowed.
        r.hold(&flick, 130);
        r.hold(&palm(), 300);
        assert_eq!(r.clicks().len(), 2);
    }

    #[test]
    fn a_fist_or_a_pointing_hand_is_not_an_index_click() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 400);
        r.hold(&hand([true, false, false, false], false), 400); // pointing: index straight
        r.hold(&palm(), 300);
        // The frames of a hand closing into a fist: index, middle and ring already bent, pinky not yet.
        r.hold(&hand([false, false, false, true], false), 300);
        r.hold(&fist(), 300);
        r.hold(&palm(), 300);
        assert!(r.clicks().is_empty(), "{:?}", r.actions);
    }

    #[test]
    fn closing_into_a_fist_while_pinched_is_not_a_click() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 300);
        r.hold(&hand([true; 4], true), 100);
        r.hold(&fist(), 300);
        assert!(r.clicks().is_empty(), "{:?}", r.actions);
    }

    #[test]
    fn thumb_middle_pinch_is_a_right_click() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 300);
        let mut h = palm();
        h[4] = Pt { x: h[12].x - 0.012, y: h[12].y + 0.01, z: 0.0 };
        r.hold(&h, 150);
        r.hold(&palm(), 200);
        let c = r.clicks();
        assert_eq!(c.len(), 1, "{:?}", r.actions);
        assert_eq!(c[0].2, Button::Right);
    }

    #[test]
    fn two_fingers_up_scrolls_with_the_hand_direction_and_never_moves_the_cursor() {
        let mut r = Rig::new();
        r.arm();
        let peace = hand([true, true, false, false], false);
        r.hold(&peace, 300);
        let before = r.snap.clone().unwrap().cursor;
        for _ in 0..30 {
            r.frame(Some(&shifted(&peace, 0.0, 0.12)));
        }
        let lines: i32 = r.actions.iter().filter_map(|a| if let Action::Scroll { lines } = a { Some(*lines) } else { None }).sum();
        assert!(lines > 3, "hand moved down -> scroll down, got {lines}");
        assert_eq!(r.snap.unwrap().cursor, before);
    }

    #[test]
    fn losing_the_hand_mid_drag_releases_the_button() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 300);
        r.hold(&hand([true; 4], true), 500);
        assert!(r.snap.clone().unwrap().dragging);
        r.gone(800);
        assert!(r.actions.iter().any(|a| matches!(a, Action::DragEnd { .. })), "{:?}", r.actions);
        assert!(!r.snap.unwrap().dragging);
    }

    #[test]
    fn a_long_absence_disarms_and_a_short_one_does_not() {
        let mut r = Rig::new();
        r.arm();
        r.gone(1500);
        assert!(r.e.is_armed());
        r.gone(5500);
        assert!(!r.e.is_armed());
        assert!(r.actions.contains(&Action::ArmChanged { armed: false, reason: "hand-lost" }));
    }

    #[test]
    fn the_cursor_does_not_jump_when_the_hand_comes_back_somewhere_else() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 300);
        let before = r.snap.clone().unwrap().cursor;
        r.gone(900);
        r.hold(&shifted(&palm(), -0.2, 0.1), 100);
        let after = r.snap.unwrap().cursor;
        assert!((after.0 - before.0).abs() < 30 && (after.1 - before.1).abs() < 30, "{before:?} -> {after:?}");
    }

    #[test]
    fn a_fist_pauses_the_pointer() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&fist(), 300);
        let before = r.snap.clone().unwrap().cursor;
        for i in 0..15 {
            r.frame(Some(&shifted(&fist(), -0.01 * i as f64, 0.0)));
        }
        assert_eq!(r.snap.unwrap().cursor, before);
    }

    #[test]
    fn the_cursor_stays_on_screen() {
        let mut r = Rig::new();
        r.arm();
        for i in 0..200 {
            r.frame(Some(&shifted(&palm(), -0.004 * i as f64, 0.0)));
        }
        let c = r.snap.unwrap().cursor;
        assert!(c.0 <= 1919 && c.0 >= 0 && c.1 >= 0 && c.1 <= 1079);
    }

    #[test]
    fn external_disarm_releases_a_held_drag() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 300);
        r.hold(&hand([true; 4], true), 500);
        let out = r.e.set_armed(false, "mouse");
        assert!(out.iter().any(|a| matches!(a, Action::DragEnd { .. })));
        assert!(out.contains(&Action::ArmChanged { armed: false, reason: "mouse" }));
    }

    // ── field test 4: reliable quick clicks, a way out, pointer speed, gestures ─────────────────────

    #[test]
    fn a_quick_pinch_still_clicks_when_the_thumb_drifts_toward_the_middle_finger_as_it_opens() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 200);
        r.hold(&hand([true; 4], true), 100); // thumb and index touch
        // Release frame: thumb opens but ends nearer the middle fingertip than the index tip. The old pose
        // gate read that as "not an index pinch" and threw the click away (people pinched three times).
        let mut opening = palm();
        opening[4] = Pt { x: 0.50, y: 0.50, z: 0.0 };
        assert!(pose::pinch_ratio(&opening, 8) > 0.6, "this frame is a genuine release");
        r.hold(&opening, 100);
        r.hold(&palm(), 100);
        assert_eq!(r.clicks().len(), 1, "{:?}", r.actions);
    }

    #[test]
    fn closing_into_a_fist_while_the_index_is_bent_is_still_not_a_click() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 200);
        r.hold(&hand([false, true, true, true], false), 100); // index bent, others open: press
        r.hold(&hand([false, false, false, false], false), 100); // the whole hand closes: not a click
        r.hold(&fist(), 200);
        assert!(r.clicks().is_empty(), "{:?}", r.actions);
    }

    #[test]
    fn keeping_the_fist_closed_after_a_fist_hold_disarm_leaves_hand_mode() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 300);
        r.hold(&fist(), 1500);
        assert!(!r.e.is_armed());
        assert!(!r.actions.iter().any(|a| matches!(a, Action::ExitMode { .. })), "not yet");
        r.hold(&fist(), 2200);
        assert!(r.actions.contains(&Action::ExitMode { reason: "fist" }), "{:?}", r.actions);
    }

    #[test]
    fn opening_the_hand_after_a_fist_hold_disarm_cancels_the_exit() {
        let mut r = Rig::new();
        r.arm();
        r.hold(&palm(), 300);
        r.hold(&fist(), 1500);
        r.hold(&palm(), 300);
        r.hold(&fist(), 4000);
        assert!(!r.actions.iter().any(|a| matches!(a, Action::ExitMode { .. })), "{:?}", r.actions);
    }

    #[test]
    fn hand_mode_ends_itself_when_no_hand_is_seen_for_ten_minutes() {
        let mut r = Rig::new();
        r.frame(None);
        r.t += 599_000;
        r.frame(None);
        assert!(!r.actions.iter().any(|a| matches!(a, Action::ExitMode { .. })));
        r.t += 2000;
        r.frame(None);
        assert!(r.actions.contains(&Action::ExitMode { reason: "idle" }), "{:?}", r.actions);
    }

    fn travel(sens: f64, precision: bool, total: f64, ms: u64) -> i32 {
        let mut r = Rig::new();
        r.e.set_pointer(sens, precision);
        r.arm();
        r.e.seed_cursor(960, 540);
        let mut h = palm();
        r.hold(&h, 300);
        let n = (ms / STEP).max(1);
        let x0 = r.snap.as_ref().unwrap().cursor.0;
        for _ in 0..n {
            h = shifted(&h, -total / n as f64, 0.0); // user's right = image left
            r.frame(Some(&h));
        }
        r.snap.as_ref().unwrap().cursor.0 - x0
    }

    #[test]
    fn a_quick_hand_moves_the_cursor_further_than_a_slow_one_for_the_same_distance() {
        let slow = travel(1.0, true, 0.09, 4000);
        let fast = travel(1.0, true, 0.09, 130);
        assert!(slow > 20, "slow {slow}");
        assert!(fast as f64 > slow as f64 * 1.5, "mouse-like acceleration: fast {fast} vs slow {slow}");
    }

    #[test]
    fn the_users_pointer_speed_scales_the_whole_mapping() {
        let one = travel(1.0, true, 0.05, 2500);
        let two = travel(2.0, true, 0.05, 2500);
        let ratio = two as f64 / one as f64;
        assert!((1.8..2.2).contains(&ratio), "x2 sensitivity -> {ratio:.2} (one {one}, two {two})");
    }

    #[test]
    fn with_pointer_precision_off_the_acceleration_is_nearly_flat() {
        let slow = travel(1.0, false, 0.09, 4000);
        let fast = travel(1.0, false, 0.09, 130);
        assert!((fast as f64) < slow as f64 * 1.35, "flat curve: fast {fast} vs slow {slow}");
    }

    fn gesture_rig() -> Rig {
        let mut r = Rig::new();
        let mut cfg = Config::new((1920.0, 1080.0));
        cfg.gestures = true;
        cfg.gestures_desktop = true;
        r.e = Engine::new(cfg);
        r.arm();
        r
    }

    fn gestures_of(r: &Rig) -> Vec<gesture::Cmd> {
        r.actions.iter().filter_map(|a| if let Action::Gesture(c) = a { Some(*c) } else { None }).collect()
    }

    fn flick(r: &mut Rig, h: &Hand, dx: f64, dy: f64) -> Hand {
        let mut h = *h;
        for _ in 0..8 {
            h = shifted(&h, dx / 8.0, dy / 8.0);
            r.frame(Some(&h));
        }
        h
    }

    #[test]
    fn three_fingers_flick_changes_tab_and_the_cursor_stays_put() {
        let mut r = gesture_rig();
        let h = hand([true, true, true, false], false);
        r.hold(&h, 500);
        let before = r.snap.as_ref().unwrap().cursor;
        flick(&mut r, &h, -0.14, 0.0);
        assert_eq!(gestures_of(&r), vec![gesture::Cmd::NextTab], "{:?}", r.actions);
        assert_eq!(r.snap.as_ref().unwrap().cursor, before, "the pointer is locked while a gesture pose is held");
        assert_eq!(r.snap.as_ref().unwrap().gesture, "tabs");
    }

    #[test]
    fn gestures_do_nothing_when_switched_off_or_while_disarmed() {
        let mut r = Rig::new(); // gestures off in the default config
        r.arm();
        let h = hand([true, true, true, false], false);
        r.hold(&h, 500);
        flick(&mut r, &h, -0.14, 0.0);
        assert!(gestures_of(&r).is_empty());

        let mut d = Rig::new();
        let mut cfg = Config::new((1920.0, 1080.0));
        cfg.gestures = true;
        d.e = Engine::new(cfg);
        d.hold(&h, 500);
        flick(&mut d, &h, -0.14, 0.0);
        assert!(gestures_of(&d).is_empty(), "a disarmed pointer never gestures");
    }

    #[test]
    fn four_fingers_are_off_unless_desktop_gestures_are_enabled() {
        let mut r = Rig::new();
        let mut cfg = Config::new((1920.0, 1080.0));
        cfg.gestures = true; // desktops stay off
        r.e = Engine::new(cfg);
        r.arm();
        let h = palm(); // flat fingers together reads as Four
        r.hold(&h, 500);
        flick(&mut r, &h, -0.14, 0.0);
        assert!(gestures_of(&r).is_empty(), "{:?}", gestures_of(&r));
    }

    #[test]
    fn disarming_with_the_window_switcher_open_always_releases_alt() {
        let mut r = gesture_rig();
        let h = hand([true, true, true, false], false);
        r.hold(&h, 500);
        flick(&mut r, &h, 0.0, -0.14); // flick UP
        assert!(gestures_of(&r).contains(&gesture::Cmd::SwitcherOpen), "{:?}", r.actions);
        let out = r.e.set_armed(false, "esc");
        assert!(out.contains(&Action::Gesture(gesture::Cmd::SwitcherCancel)), "{out:?}");
    }

    #[test]
    fn losing_the_hand_with_the_switcher_open_releases_alt() {
        let mut r = gesture_rig();
        let h = hand([true, true, true, false], false);
        r.hold(&h, 500);
        flick(&mut r, &h, 0.0, -0.14);
        r.gone(900);
        assert!(gestures_of(&r).contains(&gesture::Cmd::SwitcherCancel), "{:?}", r.actions);
    }
}
