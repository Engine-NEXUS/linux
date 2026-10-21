//! Is the cloud voice (Edge TTS) actually working? A circuit breaker driven by real synthesis results.
//!
//! Why: in the 2026-10-15 logs EVERY Edge request returned HTTP 403, on every run. The old code
//! called that "transient" and tested "is the cloud back?" with a reachability probe of bing.com, so it
//! (a) paid a failed Edge attempt before every spoken line and (b) declared the already-rendered
//! local-voice acknowledgements "stale" and re-rendered all of them on the live path, which is what made
//! "Ok sir." arrive 7 seconds late.
//!
//! The rule is simple and clock-injected so it can be tested:
//!   * 2 failures in a row (no success in between) -> DEGRADED for 10 minutes: Edge is not even tried;
//!   * after the 10 minutes ONE real attempt is allowed; a failure re-degrades at once, a success resets;
//!   * "recently OK" (a success in the last 5 minutes) is the only thing that makes a local-voice cached line stale.

use std::sync::Mutex;

/// Failures in a row before the breaker opens.
pub const DEGRADE_AFTER: u32 = 2;
/// How long Edge is skipped once the breaker opens.
pub const DEGRADED_FOR_MS: u64 = 10 * 60_000;
/// A success counts as "the cloud voice works" for this long.
pub const RECENTLY_OK_MS: u64 = 5 * 60_000;

#[derive(Debug, Default, Clone)]
pub struct EdgeHealth {
    consecutive_fails: u32,
    degraded_until_ms: u64,
    last_ok_ms: Option<u64>,
}

impl EdgeHealth {
    pub const fn new() -> Self {
        Self { consecutive_fails: 0, degraded_until_ms: 0, last_ok_ms: None }
    }

    pub fn on_ok(&mut self, now_ms: u64) {
        self.consecutive_fails = 0;
        self.degraded_until_ms = 0;
        self.last_ok_ms = Some(now_ms);
    }

    pub fn on_fail(&mut self, now_ms: u64) {
        self.consecutive_fails = self.consecutive_fails.saturating_add(1);
        if self.consecutive_fails >= DEGRADE_AFTER {
            self.degraded_until_ms = now_ms.saturating_add(DEGRADED_FOR_MS);
        }
    }

    /// True while Edge should be skipped.
    pub fn degraded(&self, now_ms: u64) -> bool {
        self.degraded_until_ms > now_ms
    }

    /// True when a real synthesis succeeded recently.
    pub fn recently_ok(&self, now_ms: u64) -> bool {
        self.last_ok_ms.is_some_and(|t| now_ms.saturating_sub(t) <= RECENTLY_OK_MS)
    }

    pub fn consecutive_fails(&self) -> u32 {
        self.consecutive_fails
    }
}

static EDGE: Mutex<EdgeHealth> = Mutex::new(EdgeHealth::new());

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn lock() -> std::sync::MutexGuard<'static, EdgeHealth> {
    EDGE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn edge_note_ok() {
    let mut g = lock();
    let was_degraded = g.degraded(now_ms());
    g.on_ok(now_ms());
    if was_degraded {
        tracing::info!("[tts_health] Edge TTS works again - cloud voice re-enabled");
    }
}

pub fn edge_note_fail(why: &str) {
    let mut g = lock();
    let was = g.degraded(now_ms());
    g.on_fail(now_ms());
    if g.degraded(now_ms()) && !was {
        tracing::warn!(
            "[tts_health] Edge TTS failed {} times in a row ({why}) - skipping it for {} min, using the local voice",
            g.consecutive_fails(),
            DEGRADED_FOR_MS / 60_000
        );
    }
}

/// Edge should not be tried right now.
pub fn edge_degraded() -> bool {
    lock().degraded(now_ms())
}

/// A real Edge synthesis succeeded in the last few minutes.
pub fn edge_recently_ok() -> bool {
    lock().recently_ok(now_ms())
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    *lock() = EdgeHealth::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_failure_does_not_open_the_breaker_two_do() {
        let mut h = EdgeHealth::new();
        h.on_fail(1_000);
        assert!(!h.degraded(1_001), "a single 403 may be a blip");
        h.on_fail(2_000);
        assert!(h.degraded(2_001), "two in a row = broken");
    }

    #[test]
    fn a_success_between_failures_resets_the_count() {
        let mut h = EdgeHealth::new();
        h.on_fail(1_000);
        h.on_ok(2_000);
        h.on_fail(3_000);
        assert!(!h.degraded(3_001));
    }

    #[test]
    fn the_breaker_closes_after_ten_minutes_and_one_failed_probe_reopens_it_at_once() {
        let mut h = EdgeHealth::new();
        h.on_fail(0);
        h.on_fail(1);
        assert!(h.degraded(DEGRADED_FOR_MS));
        assert!(!h.degraded(DEGRADED_FOR_MS + 2), "after the window one real attempt is allowed");
        h.on_fail(DEGRADED_FOR_MS + 3);
        assert!(h.degraded(DEGRADED_FOR_MS + 4), "the probe failed: degraded again immediately");
    }

    #[test]
    fn a_successful_probe_closes_the_breaker_for_good() {
        let mut h = EdgeHealth::new();
        h.on_fail(0);
        h.on_fail(1);
        h.on_ok(DEGRADED_FOR_MS + 5);
        assert!(!h.degraded(DEGRADED_FOR_MS + 6));
        assert!(h.recently_ok(DEGRADED_FOR_MS + 7));
        assert_eq!(h.consecutive_fails(), 0);
    }

    #[test]
    fn recently_ok_expires_and_a_failure_never_makes_it_ok() {
        let mut h = EdgeHealth::new();
        assert!(!h.recently_ok(0), "never succeeded");
        h.on_fail(10);
        assert!(!h.recently_ok(11));
        h.on_ok(100);
        assert!(h.recently_ok(100 + RECENTLY_OK_MS));
        assert!(!h.recently_ok(100 + RECENTLY_OK_MS + 1));
    }

    #[test]
    fn the_real_world_log_403_on_every_request_never_looks_ok() {
        // 9 pregeneration phrases + 3 live lines, all 403.
        let mut h = EdgeHealth::new();
        for t in 0..12u64 {
            h.on_fail(t * 500);
        }
        assert!(h.degraded(6_000));
        assert!(!h.recently_ok(6_000), "bing.com being reachable must not matter any more");
    }
}
