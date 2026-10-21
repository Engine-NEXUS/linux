//! Snap-to-control: a hand is a coarse pointer (a few millimetres of landmark noise is tens of
//! pixels on screen), so a click that lands just OUTSIDE a button moves to the nearest control.
//!
//! Pure geometry; the caller supplies control rectangles (from UI Automation, ~25-80 ms once per
//! click, never per frame). A click already inside a control is left exactly where it is.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn contains(&self, p: (i32, i32)) -> bool {
        p.0 >= self.x && p.0 < self.x + self.w && p.1 >= self.y && p.1 < self.y + self.h
    }

    fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }

    fn area(&self) -> i64 {
        self.w as i64 * self.h as i64
    }

    /// Distance from `p` to the nearest point of the rectangle (0 inside).
    fn distance(&self, p: (i32, i32)) -> f64 {
        let dx = (self.x - p.0).max(0).max(p.0 - (self.x + self.w - 1)) as f64;
        let dy = (self.y - p.1).max(0).max(p.1 - (self.y + self.h - 1)) as f64;
        (dx * dx + dy * dy).sqrt()
    }
}

/// Containers (a whole pane or window) are not click targets: they would swallow every snap.
const MAX_W: i32 = 900;
const MAX_H: i32 = 500;
const MIN_SIDE: i32 = 8;

fn plausible_target(r: &Rect) -> bool {
    r.w >= MIN_SIDE && r.h >= MIN_SIDE && r.w <= MAX_W && r.h <= MAX_H
}

/// Where the click should really go.
pub fn snap_point(p: (i32, i32), rects: &[Rect], radius: i32) -> (i32, i32) {
    let targets: Vec<&Rect> = rects.iter().filter(|r| plausible_target(r)).collect();
    // Already on a control: keep the user's own aim.
    if targets.iter().any(|r| r.contains(p)) {
        return p;
    }
    targets
        .into_iter()
        .map(|r| (r.distance(p), r.area(), r))
        .filter(|(d, _, _)| *d <= radius as f64)
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)))
        .map(|(_, _, r)| r.center())
        .unwrap_or(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn a_click_inside_a_control_is_untouched() {
        assert_eq!(snap_point((110, 55), &[r(100, 50, 80, 30)], 40), (110, 55));
    }

    #[test]
    fn a_click_just_outside_snaps_to_the_nearest_control_centre() {
        let rects = [r(100, 50, 80, 30), r(300, 50, 80, 30)];
        assert_eq!(snap_point((92, 60), &rects, 40), (140, 65));
        assert_eq!(snap_point((312, 100), &rects, 40), (340, 65));
    }

    #[test]
    fn nothing_within_the_radius_means_no_snap() {
        assert_eq!(snap_point((600, 600), &[r(100, 50, 80, 30)], 40), (600, 600));
    }

    #[test]
    fn whole_window_containers_and_specks_are_not_targets() {
        let rects = [r(0, 0, 1900, 1000), r(500, 500, 3, 3)];
        assert_eq!(snap_point((505, 505), &rects, 40), (505, 505));
    }

    #[test]
    fn the_closer_of_two_controls_wins_and_ties_go_to_the_smaller_one() {
        let rects = [r(100, 50, 80, 30), r(200, 50, 80, 30)];
        assert_eq!(snap_point((190, 60), &rects, 40), (240, 65));
        // Equidistant (10 px each side): the smaller control is the more specific target.
        let tie = [r(100, 50, 80, 30), r(199, 50, 10, 30)];
        assert_eq!(snap_point((189, 60), &tie, 40), (204, 65));
    }
}
