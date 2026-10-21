//! Hand geometry from the 21 MediaPipe landmarks: which fingers are extended, which pose the hand
//! is in, how closed a pinch is, and the steady point the pointer is driven from.
//!
//! Everything is scale-free (ratios of distances inside the hand), so it works the same near or
//! far from the camera. Landmark indices: 0 wrist; thumb 1-4; index 5-8; middle 9-12; ring 13-16;
//! pinky 17-20 (MCP, PIP, DIP, TIP for the four fingers).

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub type Hand = [Pt; 21];

fn dist(a: Pt, b: Pt) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// 2-D distance: z from a single camera is the noisiest coordinate, so pose logic avoids it.
fn dist2(a: Pt, b: Pt) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pose {
    /// All four fingers extended.
    Palm,
    /// No finger extended.
    Fist,
    /// Index + middle up, ring + pinky folded (scroll).
    Peace,
    Other,
}

const EXTENDED_RATIO: f64 = 1.12;
/// A fist where one finger sticks out a little is still a fist if the four fingers are curled on average.
const FIST_AVG_RATIO: f64 = 1.15;
/// ...but only if that finger is barely out. A clearly straight finger (pointing) is not a fist.
const LAZY_FINGER_MAX: f64 = 1.28;
/// Palm tolerance (field test 2: a tilted palm read 1.36/1.33/1.13/1.08 and flickered to "other").
const PALM_MIN_RATIO: f64 = 1.0;
const PALM_AVG_RATIO: f64 = 1.2;

/// Tip-to-wrist over PIP-to-wrist for one finger: well above 1 = straight, below 1 = curled in.
fn finger_ratio(h: &Hand, mcp: usize) -> f64 {
    dist2(h[mcp + 3], h[0]) / dist2(h[mcp + 1], h[0]).max(1e-6)
}

/// The four ratios (index, middle, ring, pinky). Logged on every pose change so thresholds can be
/// tuned on a real hand instead of guessed.
pub fn finger_ratios(h: &Hand) -> [f64; 4] {
    [finger_ratio(h, 5), finger_ratio(h, 9), finger_ratio(h, 13), finger_ratio(h, 17)]
}

/// Finger is extended when its tip is clearly farther from the wrist than its PIP joint.
fn finger_extended(h: &Hand, mcp: usize) -> bool {
    finger_ratio(h, mcp) > EXTENDED_RATIO
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingers {
    pub index: bool,
    pub middle: bool,
    pub ring: bool,
    pub pinky: bool,
}

pub fn fingers(h: &Hand) -> Fingers {
    Fingers {
        index: finger_extended(h, 5),
        middle: finger_extended(h, 9),
        ring: finger_extended(h, 13),
        pinky: finger_extended(h, 17),
    }
}

pub fn pose(h: &Hand) -> Pose {
    let f = fingers(h);
    let n = [f.index, f.middle, f.ring, f.pinky].iter().filter(|b| **b).count();
    let r = finger_ratios(h);
    let avg = r.iter().sum::<f64>() / 4.0;
    let min = r.iter().cloned().fold(f64::MAX, f64::min);
    match n {
        4 => Pose::Palm,
        // A slightly tilted open palm: three clearly straight fingers and the fourth not curled.
        3 if min > PALM_MIN_RATIO && avg >= PALM_AVG_RATIO => Pose::Palm,
        0 => Pose::Fist,
        1 if avg <= FIST_AVG_RATIO && r.iter().cloned().fold(0.0, f64::max) < LAZY_FINGER_MAX => Pose::Fist,
        2 if f.index && f.middle => Pose::Peace,
        _ => Pose::Other,
    }
}

/// The touchpad-style "blade" poses used by gestures (tab / window / desktop switching). They are a
/// separate classification from `Pose` so the arm gesture and the click logic never see them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blade {
    /// Index, middle and ring straight, pinky folded: tabs and windows.
    Three,
    /// All four straight AND held together: virtual desktops, Task View, show desktop.
    Four,
}

/// Index tip to pinky tip in palm widths. Fingers together measure about 1.0 or less, a spread open
/// hand clearly more. The Four threshold below is a first guess: the value is logged on every stable
/// pose change (`[HAND] pose ... spread=`) so it can be set from a real hand.
pub fn spread(h: &Hand) -> f64 {
    dist2(h[8], h[20]) / palm_width(h)
}

/// Fingers-together limit for `Blade::Four` (palm widths). Tune from the logged spreads.
pub const FOUR_SPREAD_MAX: f64 = 1.15;

pub fn blade_pose(h: &Hand) -> Option<Blade> {
    let f = fingers(h);
    match (f.index, f.middle, f.ring, f.pinky) {
        (true, true, true, false) => Some(Blade::Three),
        (true, true, true, true) if spread(h) < FOUR_SPREAD_MAX => Some(Blade::Four),
        _ => None,
    }
}

/// Palm width (index MCP to pinky MCP): the unit every distance is measured in.
fn palm_width(h: &Hand) -> f64 {
    dist2(h[5], h[17]).max(1e-6)
}

/// Thumb tip to `tip` (8 = index, 12 = middle) in palm widths. ~0.1-0.25 touching, 0.8+ apart.
pub fn pinch_ratio(h: &Hand, tip: usize) -> f64 {
    dist2(h[4], h[tip]) / palm_width(h)
}

/// A real index pinch needs the other fingers free: a fist also brings the thumb to the index.
pub fn index_pinch_pose_ok(h: &Hand) -> bool {
    let f = fingers(h);
    [f.middle, f.ring, f.pinky].iter().filter(|b| **b).count() >= 2
}

/// An index-finger click is only real when the OUTER two fingers stay open (ring and pinky). Field test 3
/// data: genuine flicks had ring 1.24-1.37 and pinky 1.29-1.37, while the frames of a hand closing into a
/// fist (which also bend the index) had ring <= 1.01. The middle finger is left out: it follows the index.
pub fn index_flick_ok(h: &Hand) -> bool {
    let r = finger_ratios(h);
    r[2] > 1.18 && r[3] > 1.12
}

/// A middle pinch needs the index and pinky out (otherwise it is a fist or a mid-gesture blur).
pub fn middle_pinch_pose_ok(h: &Hand) -> bool {
    let f = fingers(h);
    f.index && f.pinky
}

/// Steady pointer anchor: mean of wrist + the four finger-base knuckles. Moves very little when the
/// fingers pinch, which is exactly what stops the cursor jumping during a click.
pub fn palm_center(h: &Hand) -> (f64, f64) {
    let ids = [0usize, 5, 9, 13, 17];
    let n = ids.len() as f64;
    let (sx, sy) = ids.iter().fold((0.0, 0.0), |(sx, sy), &i| (sx + h[i].x, sy + h[i].y));
    (sx / n, sy / n)
}

/// Reject landmark sets that cannot be a real hand (degenerate, collapsed, or non-finite).
pub fn plausible(h: &Hand) -> bool {
    if h.iter().any(|p| !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite())) {
        return false;
    }
    let w = palm_width(h);
    let len = dist(h[0], h[9]);
    w > 0.01 && len > 0.02 && (0.4..=3.5).contains(&(len / w))
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    /// A synthetic upright right hand in normalised image coordinates (wrist at the bottom).
    /// `ext` = which of index/middle/ring/pinky are extended; `pinch` pulls the index tip to the thumb tip.
    pub fn hand(ext: [bool; 4], pinch: bool) -> Hand {
        let mut h = [Pt::default(); 21];
        let base_x = [0.46, 0.50, 0.54, 0.58]; // MCP x of index..pinky (palm width ~0.12)
        h[0] = Pt { x: 0.52, y: 0.80, z: 0.0 };
        for (k, &bx) in base_x.iter().enumerate() {
            let m = 5 + 4 * k;
            let mcp_y = 0.60;
            h[m] = Pt { x: bx, y: mcp_y, z: 0.0 };
            if ext[k] {
                h[m + 1] = Pt { x: bx, y: mcp_y - 0.07, z: 0.0 };
                h[m + 2] = Pt { x: bx, y: mcp_y - 0.12, z: 0.0 };
                h[m + 3] = Pt { x: bx, y: mcp_y - 0.17, z: 0.0 };
            } else {
                // Curled: PIP up a little, tip folded back down near the palm.
                h[m + 1] = Pt { x: bx, y: mcp_y - 0.05, z: 0.0 };
                h[m + 2] = Pt { x: bx, y: mcp_y - 0.01, z: 0.0 };
                h[m + 3] = Pt { x: bx, y: mcp_y + 0.04, z: 0.0 };
            }
        }
        // Thumb: CMC, MCP, IP, TIP going up and out to the left of the index.
        h[1] = Pt { x: 0.47, y: 0.74, z: 0.0 };
        h[2] = Pt { x: 0.42, y: 0.69, z: 0.0 };
        h[3] = Pt { x: 0.38, y: 0.63, z: 0.0 };
        h[4] = Pt { x: 0.35, y: 0.57, z: 0.0 };
        if pinch {
            h[4] = Pt { x: h[8].x - 0.012, y: h[8].y + 0.01, z: 0.0 };
        }
        h
    }

    pub fn palm() -> Hand {
        hand([true; 4], false)
    }
    pub fn fist() -> Hand {
        hand([false; 4], false)
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn poses_are_recognised() {
        assert_eq!(pose(&palm()), Pose::Palm);
        assert_eq!(pose(&fist()), Pose::Fist);
        assert_eq!(pose(&hand([true, true, false, false], false)), Pose::Peace);
        assert_eq!(pose(&hand([true, false, false, false], false)), Pose::Other);
    }

    #[test]
    fn a_fist_with_one_lazy_finger_is_still_a_fist_but_a_pointing_hand_is_not() {
        // Pinky sticks out slightly, the rest tightly curled.
        let mut h = fist();
        h[20] = Pt { x: h[17].x, y: h[17].y - 0.075, z: 0.0 }; // tip just beyond the PIP
        h[18] = Pt { x: h[17].x, y: h[17].y - 0.03, z: 0.0 };
        assert_eq!(pose(&h), Pose::Fist, "ratios {:?}", finger_ratios(&h));
        // Index fully extended, three curled: pointing, not a fist.
        assert_eq!(pose(&hand([true, false, false, false], false)), Pose::Other);
    }

    #[test]
    fn a_tilted_open_palm_is_still_a_palm_but_a_half_closed_hand_is_not() {
        // Pinky partly curled: ratio just over 1.0 while the other three are straight.
        let mut h = palm();
        h[20] = Pt { x: 0.58, y: 0.507, z: 0.0 };
        let r = finger_ratios(&h);
        assert!(r[3] > 1.0 && r[3] < 1.12, "{r:?}");
        assert_eq!(pose(&h), Pose::Palm);
        // Two fingers half curled is not a palm.
        h[16] = Pt { x: 0.54, y: 0.507, z: 0.0 };
        assert_ne!(pose(&h), Pose::Palm);
    }

    #[test]
    fn an_index_flick_needs_ring_and_pinky_open() {
        assert!(index_flick_ok(&hand([false, true, true, true], false)));
        assert!(index_flick_ok(&hand([false, false, true, true], false)), "middle may follow the index");
        // A hand closing into a fist: index, middle and ring already bending.
        assert!(!index_flick_ok(&hand([false, false, false, true], false)));
        assert!(!index_flick_ok(&fist()));
        assert!(finger_ratios(&palm())[0] > 1.2, "an open palm's index is straight, so it is not a flick");
    }

    #[test]
    fn three_and_four_blades_are_recognised_and_a_spread_hand_is_not_four() {
        assert_eq!(blade_pose(&hand([true, true, true, false], false)), Some(Blade::Three));
        assert_eq!(blade_pose(&palm()), Some(Blade::Four), "flat fingers together: spread {}", spread(&palm()));
        assert_eq!(blade_pose(&fist()), None);
        assert_eq!(blade_pose(&hand([true, true, false, false], false)), None, "peace is scroll");
        assert_eq!(blade_pose(&hand([true, false, false, false], false)), None, "pointing is not a gesture");
        // Fan the fingers out: index tip left, pinky tip right.
        let mut open = palm();
        open[8].x -= 0.06;
        open[20].x += 0.06;
        assert!(spread(&open) > FOUR_SPREAD_MAX, "{}", spread(&open));
        assert_eq!(blade_pose(&open), None);
    }

    #[test]
    fn ratios_separate_straight_from_curled_fingers() {
        let open = finger_ratios(&palm());
        let closed = finger_ratios(&fist());
        for i in 0..4 {
            assert!(open[i] > 1.2, "open finger {i}: {open:?}");
            assert!(closed[i] < 1.0, "curled finger {i}: {closed:?}");
        }
    }

    #[test]
    fn pose_does_not_depend_on_hand_size_or_position() {
        let mut h = palm();
        for p in h.iter_mut() {
            p.x = 0.3 + (p.x - 0.5) * 0.5;
            p.y = 0.2 + (p.y - 0.7) * 0.5;
        }
        assert_eq!(pose(&h), Pose::Palm);
    }

    #[test]
    fn pinch_ratio_separates_touching_from_apart() {
        assert!(pinch_ratio(&hand([true; 4], true), 8) < 0.3);
        assert!(pinch_ratio(&palm(), 8) > 0.8);
    }

    #[test]
    fn a_fist_is_not_an_index_pinch() {
        let mut f = fist();
        f[4] = Pt { x: f[8].x, y: f[8].y, z: 0.0 }; // thumb on index, like a closed fist
        assert!(pinch_ratio(&f, 8) < 0.1);
        assert!(!index_pinch_pose_ok(&f));
        assert!(index_pinch_pose_ok(&hand([true; 4], true)));
    }

    #[test]
    fn palm_center_barely_moves_when_the_index_pinches() {
        let open = palm_center(&palm());
        let pinched = palm_center(&hand([true; 4], true));
        let (dx, dy) = (open.0 - pinched.0, open.1 - pinched.1);
        assert!((dx * dx + dy * dy).sqrt() < 0.002);
    }

    #[test]
    fn implausible_landmarks_are_rejected() {
        assert!(plausible(&palm()));
        let mut bad = palm();
        bad[3].x = f64::NAN;
        assert!(!plausible(&bad));
        assert!(!plausible(&[Pt::default(); 21]));
    }
}
