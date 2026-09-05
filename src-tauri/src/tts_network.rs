//! TTS network state tracker + Piper lifecycle manager.
#![allow(dead_code)]
//!
//! Design:
//!   - Edge TTS (cloud) is the PRIMARY engine.
//!   - Piper (local) is the FALLBACK — only used when network is down.
//!   - When network recovers, Piper stays loaded for 10 minutes
//!     (hysteresis to avoid thrashing on flaky connections).
//!   - After 10 minutes of stable network, Piper is UNLOADED to save RAM.
//!   - If network drops again, Piper reloads on next fallback.
//!
//! Network check: pings Microsoft's Edge TTS endpoint every 30 seconds
//! and before each TTS call. Uses a 3-second timeout.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Network is up (Edge TTS reachable).
static NETWORK_UP: AtomicBool = AtomicBool::new(true);

/// Piper is currently loaded in RAM.
static PIPER_LOADED: AtomicBool = AtomicBool::new(false);

/// When the network last came back up (for the 10-min unload timer).
static NETWORK_RECOVERED_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// When we last checked the network (throttle checks to 30s).
static LAST_NETWORK_CHECK: Mutex<Option<Instant>> = Mutex::new(None);

/// Piper stays loaded for this long after network recovers before unloading.
const PIPER_GRACE_PERIOD: Duration = Duration::from_secs(600); // 10 minutes

/// Minimum interval between network checks.
const NETWORK_CHECK_INTERVAL: Duration = Duration::from_secs(30); // 30 seconds

/// Network check timeout.
const NETWORK_CHECK_TIMEOUT: Duration = Duration::from_secs(3);

/// Check if Edge TTS is reachable (network up).
///
/// This is throttled to once per 30 seconds. If called within 30s of
/// the last check, returns the cached result.
pub async fn check_network() -> bool {
    // Check if we need to re-check (throttle)
    {
        let last = LAST_NETWORK_CHECK.lock().unwrap();
        if let Some(t) = *last {
            if t.elapsed() < NETWORK_CHECK_INTERVAL {
                return NETWORK_UP.load(Ordering::Relaxed);
            }
        }
    }

    // Update last check time
    {
        let mut last = LAST_NETWORK_CHECK.lock().unwrap();
        *last = Some(Instant::now());
    }

    // Actually check the network
    let up = crate::tts_edge::is_available().await;

    let was_up = NETWORK_UP.load(Ordering::Relaxed);
    NETWORK_UP.store(up, Ordering::Relaxed);

    // Track network recovery
    if !was_up && up {
        // Network just came back up
        let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
        *recovered = Some(Instant::now());
        tracing::info!("[tts_net] network recovered — Piper will unload in 10 minutes");
    } else if was_up && !up {
        // Network just went down
        let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
        *recovered = None;
        tracing::warn!("[tts_net] network down — falling back to Piper");
    }

    up
}

/// Force a network check (bypasses the 30s throttle).
/// Used when we need an immediate answer (e.g. before a TTS call).
pub async fn check_network_now() -> bool {
    // Actually check the network
    let up = crate::tts_edge::is_available().await;

    let was_up = NETWORK_UP.load(Ordering::Relaxed);
    NETWORK_UP.store(up, Ordering::Relaxed);

    // Update last check time
    {
        let mut last = LAST_NETWORK_CHECK.lock().unwrap();
        *last = Some(Instant::now());
    }

    // Track network recovery
    if !was_up && up {
        let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
        *recovered = Some(Instant::now());
        tracing::info!("[tts_net] network recovered — Piper will unload in 10 minutes");
    } else if was_up && !up {
        let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
        *recovered = None;
        tracing::warn!("[tts_net] network down — falling back to Piper");
    }

    up
}

/// Check if the network is up (cached, no async check).
pub fn is_network_up() -> bool {
    NETWORK_UP.load(Ordering::Relaxed)
}

/// Force network state to down (called when Edge TTS fails despite network check passing).
pub fn set_network_down() {
    let was_up = NETWORK_UP.load(Ordering::Relaxed);
    if was_up {
        NETWORK_UP.store(false, Ordering::Relaxed);
        let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
        *recovered = None;
        tracing::warn!("[tts_net] Edge TTS failed — marking network as down");
    }
}

/// Record a successful Edge TTS synthesis (called when cloud synthesis works).
/// Clears a stale down-flag so the next call tries Edge first again, and
/// starts the 10-minute Piper-unload hysteresis timer.
pub fn set_network_up() {
    let was_up = NETWORK_UP.load(Ordering::Relaxed);
    if !was_up {
        NETWORK_UP.store(true, Ordering::Relaxed);
        let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
        *recovered = Some(Instant::now());
        tracing::info!("[tts_net] Edge TTS succeeded — marking network as up");
    }
}

/// Check if Piper is currently loaded.
pub fn is_piper_loaded() -> bool {
    PIPER_LOADED.load(Ordering::Relaxed)
}

/// Mark Piper as loaded.
pub fn mark_piper_loaded() {
    PIPER_LOADED.store(true, Ordering::Relaxed);
}

/// Mark Piper as unloaded.
pub fn mark_piper_unloaded() {
    PIPER_LOADED.store(false, Ordering::Relaxed);
}

/// Check if Piper should be unloaded (network has been up for 10+ minutes).
///
/// Returns true if:
///   1. Network is currently up
///   2. Network recovered more than 10 minutes ago
///   3. Piper is currently loaded
pub fn should_unload_piper() -> bool {
    if !is_network_up() {
        return false;
    }
    if !is_piper_loaded() {
        return false;
    }
    let recovered = NETWORK_RECOVERED_AT.lock().unwrap();
    match *recovered {
        Some(t) => t.elapsed() >= PIPER_GRACE_PERIOD,
        None => false,
    }
}

/// Start a background thread that periodically checks the network and
/// unloads Piper after 10 minutes of stable network.
///
/// This runs forever (until the process exits). It checks every 60 seconds.
pub fn start_network_monitor() {
    std::thread::spawn(move || {
        tracing::info!("[tts_net] network monitor started (60s interval)");
        loop {
            std::thread::sleep(Duration::from_secs(60));

            // Check if it's time to unload Piper
            if should_unload_piper() {
                tracing::info!("[tts_net] network stable for 10+ minutes — unloading Piper");
                // Unload Piper via the global engine reference.
                // This frees ~80 MB RAM.
                // We need to run this on the tokio runtime.
                let handle = tauri::async_runtime::handle();
                let _ = handle.spawn(async {
                    crate::tts::unload_piper_global().await;
                });
                mark_piper_unloaded();
                let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
                *recovered = None;
                tracing::info!("[tts_net] Piper unloaded — network stable, ~80 MB RAM freed");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These tests mutate shared process-wide statics (NETWORK_UP,
    /// PIPER_LOADED, NETWORK_RECOVERED_AT). Rust runs tests in parallel
    /// threads, so each test takes this lock first — otherwise they flake
    /// by observing each other's state.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn reset_state() {
        NETWORK_UP.store(true, Ordering::Relaxed);
        mark_piper_unloaded();
        *NETWORK_RECOVERED_AT.lock().unwrap() = None;
        *LAST_NETWORK_CHECK.lock().unwrap() = None;
    }

    #[test]
    fn test_defaults() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_state();
        // Network should default to up (optimistic)
        assert!(is_network_up());
        // Piper should default to not loaded
        assert!(!is_piper_loaded());
    }

    #[test]
    fn test_piper_loaded_flag() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_state();
        mark_piper_loaded();
        assert!(is_piper_loaded());
        mark_piper_unloaded();
        assert!(!is_piper_loaded());
    }

    #[test]
    fn test_should_unload_piper_when_network_down() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_state();
        NETWORK_UP.store(false, Ordering::Relaxed);
        mark_piper_loaded();
        assert!(!should_unload_piper());
    }

    #[test]
    fn test_should_unload_piper_when_not_loaded() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_state();
        NETWORK_UP.store(true, Ordering::Relaxed);
        mark_piper_unloaded();
        assert!(!should_unload_piper());
    }

    #[test]
    fn test_should_unload_piper_when_recently_recovered() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_state();
        NETWORK_UP.store(true, Ordering::Relaxed);
        mark_piper_loaded();
        // Set recovery to now — should NOT unload (less than 10 min)
        {
            let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
            *recovered = Some(Instant::now());
        }
        assert!(!should_unload_piper());
        // Clear
        {
            let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
            *recovered = None;
        }
    }

    #[test]
    fn test_network_up_down_roundtrip() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_state();
        // Regression: a lying probe used to pin the flag down while the
        // network worked. A successful synthesis must clear a stale down.
        NETWORK_UP.store(false, Ordering::Relaxed);
        set_network_up();
        assert!(is_network_up());
        set_network_down();
        assert!(!is_network_up());
    }

    #[test]
    fn test_should_unload_piper_after_10_min() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_state();
        NETWORK_UP.store(true, Ordering::Relaxed);
        mark_piper_loaded();
        // Set recovery to 11 minutes ago — should unload
        {
            let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
            *recovered = Some(Instant::now() - Duration::from_secs(660));
        }
        assert!(should_unload_piper());
        // Clean up
        mark_piper_unloaded();
        {
            let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
            *recovered = None;
        }
    }
}
