//! One Euro filter (Casiez, Roussel, Vogel, CHI 2012) — speed-adaptive low-pass for a noisy pointer.
//!
//! Slow motion is smoothed hard (kills jitter of a resting hand); fast motion is barely smoothed
//! (no lag while sweeping). Written from the published algorithm, not copied from any project.
//! Time is passed in by the caller (seconds), so the filter is deterministic and testable.

use std::f64::consts::PI;

#[derive(Debug, Clone, Copy)]
pub struct OneEuroParams {
    /// Cutoff (Hz) at rest. Lower = steadier, laggier.
    pub min_cutoff: f64,
    /// How fast the cutoff grows with speed. Higher = less lag on fast moves.
    pub beta: f64,
    /// Cutoff (Hz) for the derivative estimate.
    pub d_cutoff: f64,
}

#[derive(Debug, Clone)]
pub struct OneEuro {
    p: OneEuroParams,
    prev: Option<(f64, f64, f64)>, // (t, filtered x, filtered dx/dt)
}

fn alpha(cutoff: f64, dt: f64) -> f64 {
    let tau = 1.0 / (2.0 * PI * cutoff);
    1.0 / (1.0 + tau / dt)
}

impl OneEuro {
    pub fn new(p: OneEuroParams) -> Self {
        Self { p, prev: None }
    }

    /// Forget history (hand lost): the next sample is taken as-is.
    pub fn reset(&mut self) {
        self.prev = None;
    }

    pub fn filter(&mut self, x: f64, t_secs: f64) -> f64 {
        let Some((pt, px, pdx)) = self.prev else {
            self.prev = Some((t_secs, x, 0.0));
            return x;
        };
        let dt = (t_secs - pt).max(1e-3);
        let dx = (x - px) / dt;
        let dx_hat = pdx + alpha(self.p.d_cutoff, dt) * (dx - pdx);
        let cutoff = self.p.min_cutoff + self.p.beta * dx_hat.abs();
        let x_hat = px + alpha(cutoff, dt) * (x - px);
        self.prev = Some((t_secs, x_hat, dx_hat));
        x_hat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: OneEuroParams = OneEuroParams { min_cutoff: 1.2, beta: 12.0, d_cutoff: 1.0 };

    /// Deterministic pseudo-noise in [-1, 1].
    fn noise(i: u32) -> f64 {
        let v = (i.wrapping_mul(2654435761) >> 8) as f64 / (u32::MAX >> 8) as f64;
        v * 2.0 - 1.0
    }

    #[test]
    fn first_sample_passes_through() {
        let mut f = OneEuro::new(P);
        assert_eq!(f.filter(0.4, 0.0), 0.4);
    }

    #[test]
    fn a_resting_hand_is_calmed_down() {
        let mut f = OneEuro::new(P);
        let (mut raw_dev, mut out_dev) = (0.0, 0.0);
        for i in 0..300u32 {
            let raw = 0.5 + 0.004 * noise(i);
            let out = f.filter(raw, i as f64 / 30.0);
            if i >= 30 {
                raw_dev += (raw - 0.5).abs();
                out_dev += (out - 0.5).abs();
            }
        }
        assert!(out_dev < raw_dev * 0.5, "filter should at least halve rest jitter: {out_dev} vs {raw_dev}");
    }

    #[test]
    fn a_fast_sweep_does_not_lag_far_behind() {
        let mut f = OneEuro::new(P);
        let mut last = 0.0;
        for i in 0..30u32 {
            // 1.0 frame-widths per second, sweeping for one second.
            let raw = i as f64 / 30.0;
            last = f.filter(raw, i as f64 / 30.0);
        }
        let raw = 29.0 / 30.0;
        assert!(raw - last < 0.12, "lag at speed must stay small, got {}", raw - last);
    }

    #[test]
    fn reset_forgets_the_past() {
        let mut f = OneEuro::new(P);
        f.filter(0.1, 0.0);
        f.filter(0.1, 0.03);
        f.reset();
        assert_eq!(f.filter(0.9, 5.0), 0.9);
    }

    #[test]
    fn a_zero_time_step_does_not_blow_up() {
        let mut f = OneEuro::new(P);
        f.filter(0.2, 1.0);
        let v = f.filter(0.8, 1.0);
        assert!(v.is_finite() && (0.2..=0.8).contains(&v));
    }
}
