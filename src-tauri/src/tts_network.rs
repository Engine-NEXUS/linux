//! TTS network state tracker + Kokoro lifecycle manager.
#![allow(dead_code)]
//!
//! Design:
//!   - Edge TTS (cloud) is the PRIMARY engine.
//!   - Kokoro (local) is the FALLBACK — only used when network is down.
//!   - When network recovers, Kokoro stays loaded for 10 minutes
//!     (hysteresis to avoid thrashing on flaky connections).
//!   - After 10 minutes of stable network, Kokoro is UNLOADED to save RAM.
//!   - If network drops again, Kokoro reloads on next fallback.
//!
//! Network check: pings Microsoft's Edge TTS endpoint every 30 seconds
//! and before each TTS call. Uses a 3-second timeout.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Network is up (Edge TTS reachable).
static NETWORK_UP: AtomicBool = AtomicBool::new(true);

/// Kokoro is currently loaded in RAM.
static LOCAL_LOADED: AtomicBool = AtomicBool::new(false);

/// When the network last came back up (for the 10-min unload timer).
static NETWORK_RECOVERED_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// When we last checked the network (throttle checks to 30s).
static LAST_NETWORK_CHECK: Mutex<Option<Instant>> = Mutex::new(None);

/// Kokoro stays loaded for this long after network recovers before unloading.
const LOCAL_GRACE_PERIOD: Duration = Duration::from_secs(600); // 10 minutes

/// Minimum interval between network checks.
const NETWORK_CHECK_INTERVAL: Duration = Duration::from_secs(30); // 30 seconds

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
        tracing::info!("[tts_net] network recovered — Kokoro will unload in 10 minutes");
    } else if was_up && !up {
        // Network just went down
        let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
        *recovered = None;
        tracing::warn!("[tts_net] network down — falling back to Kokoro");
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
        tracing::info!("[tts_net] network recovered — Kokoro will unload in 10 minutes");
    } else if was_up && !up {
        let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
        *recovered = None;
        tracing::warn!("[tts_net] network down — falling back to Kokoro");
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
/// starts the 10-minute Kokoro-unload hysteresis timer.
pub fn set_network_up() {
    let was_up = NETWORK_UP.load(Ordering::Relaxed);
    if !was_up {
        NETWORK_UP.store(true, Ordering::Relaxed);
        let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
        *recovered = Some(Instant::now());
        tracing::info!("[tts_net] Edge TTS succeeded — marking network as up");
    }
}

/// Check if Kokoro is currently loaded.
pub fn is_local_loaded() -> bool {
    LOCAL_LOADED.load(Ordering::Relaxed)
}

/// When the local engine last synthesized (drives the idle unload).
static LOCAL_LAST_USED: Mutex<Option<Instant>> = Mutex::new(None);

/// Mark Kokoro as loaded / just used (called on every local synthesis).
pub fn mark_local_loaded() {
    LOCAL_LOADED.store(true, Ordering::Relaxed);
    if let Ok(mut t) = LOCAL_LAST_USED.lock() {
        *t = Some(Instant::now());
    }
}

/// Mark Kokoro as unloaded.
pub fn mark_local_unloaded() {
    LOCAL_LOADED.store(false, Ordering::Relaxed);
}

/// Watchdog re-probe decision (Feature 83 P4): only while flagged down,
/// at most every NETWORK_CHECK_INTERVAL. While up, per-turn checks +
/// failure paths own the up→down direction — the watchdog only heals.
/// Pure over injected clock values (unit-tested, no statics touched).
pub fn probe_due_for(
    last_check: Option<Instant>,
    up: bool,
    now: Instant,
) -> bool {
    if up {
        return false;
    }
    match last_check {
        None => true,
        Some(t) => now.duration_since(t) >= NETWORK_CHECK_INTERVAL,
    }
}

/// Live view of the watchdog decision against process state.
pub fn probe_due() -> bool {
    let last = LAST_NETWORK_CHECK.lock().unwrap().clone();
    probe_due_for(last, is_network_up(), Instant::now())
}

/// `voice:status` payload emitted when the watchdog detects restoration.
/// Shape pinned by unit test (frontend validator must accept it).
pub fn cloud_restored_payload() -> serde_json::Value {
    serde_json::json!({ "status": "cloud-restored" })
}

/// Pure unload policy. The local engine holds ~260-370 MB, so it is freed when EITHER
///   * the network has been up for >= `LOCAL_GRACE_PERIOD` (the cloud is speaking again), OR
///   * it has been idle (no local synthesis) for >= `LOCAL_IDLE_UNLOAD` — this is what keeps RAM
///     low while the user stays offline but is not talking.
/// It reloads lazily (~1-2 s) on the next offline utterance.
pub fn unload_due(
    loaded: bool,
    network_up: bool,
    recovered_for: Option<Duration>,
    idle_for: Option<Duration>,
) -> bool {
    if !loaded {
        return false;
    }
    let stable_online = network_up && matches!(recovered_for, Some(d) if d >= LOCAL_GRACE_PERIOD);
    let idle = matches!(idle_for, Some(d) if d >= LOCAL_IDLE_UNLOAD);
    stable_online || idle
}

/// Idle time (no local synthesis) after which the engine is unloaded even while offline.
const LOCAL_IDLE_UNLOAD: Duration = Duration::from_secs(600); // 10 minutes

/// Live view of the unload policy against process state.
pub fn should_unload_local() -> bool {
    // While the cloud voice is broken the local engine IS the voice: keep it warm.
    if crate::tts_health::edge_degraded() {
        return false;
    }
    let recovered_for = NETWORK_RECOVERED_AT.lock().unwrap().map(|t| t.elapsed());
    let idle_for = LOCAL_LAST_USED.lock().ok().and_then(|t| t.map(|i| i.elapsed()));
    unload_due(is_local_loaded(), is_network_up(), recovered_for, idle_for)
}

/// Start a background thread that periodically checks the network and
/// unloads Kokoro after 10 minutes of stable network.
///
/// This runs forever (until the process exits). It checks every 60 seconds.
pub fn start_network_monitor(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        tracing::info!("[tts_net] network monitor started (30s while down / 60s while up)");
        loop {
            // P4 watchdog: while flagged down, actively re-probe so cloud
            // restoration is detected WITHOUT waiting for the next utterance.
            // The flip is silent — no speech interrupted; the next synthesis
            // goes Edge automatically via tts_engine_for, and the hub pill
            // updates live through the cloud-restored event.
            if probe_due() {
                let app_clone = app.clone();
                let handle = tauri::async_runtime::handle();
                let _ = handle.spawn(async move {
                    use tauri::Emitter;
                    if check_network_now().await && is_network_up() {
                        let _ = app_clone.emit("voice:status", cloud_restored_payload());
                        // cloud is back: make sure the equipped persona's offline voice is installed
                        // (it could not be downloaded while offline). No-op when already present.
                        if let Some(engine) = crate::tts::kokoro_engine_handle() {
                            crate::tts_swap::sync_selected_voice(&app_clone, engine);
                        }
                        tracing::info!(
                            "[tts_net] watchdog: cloud restored — next synthesis goes Edge"
                        );
                    }
                });
            }

            // Check if it's time to unload Kokoro
            if should_unload_local() {
                tracing::info!("[tts_net] network stable for 10+ minutes — unloading Kokoro");
                // Unload Kokoro via the global engine reference.
                // This frees the local model's RAM.
                // We need to run this on the tokio runtime.
                let handle = tauri::async_runtime::handle();
                let _ = handle.spawn(async {
                    crate::tts::unload_local_global().await;
                });
                mark_local_unloaded();
                let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
                *recovered = None;
                tracing::info!("[tts_net] Kokoro unloaded — network stable, ~260-370 MB RAM freed");
            }

            // Sleep cadence follows the flag: aggressive while down (fast
            // restore detection), relaxed while up (per-turn probes cover it).
            std::thread::sleep(if is_network_up() {
                Duration::from_secs(60)
            } else {
                Duration::from_secs(30)
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These tests mutate shared process-wide statics (NETWORK_UP,
    /// LOCAL_LOADED, NETWORK_RECOVERED_AT). Rust runs tests in parallel
    /// threads, so each test takes this lock first — otherwise they flake
    /// by observing each other's state.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn reset_state() {
        NETWORK_UP.store(true, Ordering::Relaxed);
        mark_local_unloaded();
        *NETWORK_RECOVERED_AT.lock().unwrap() = None;
        *LAST_NETWORK_CHECK.lock().unwrap() = None;
    }

    #[test]
    fn test_unload_due_policy() {
        let m = |s| Some(Duration::from_secs(s));
        // not loaded => never
        assert!(!unload_due(false, true, m(9999), m(9999)));
        // loaded, online only 5 min, used 1 min ago => keep (hysteresis)
        assert!(!unload_due(true, true, m(300), m(60)));
        // online stable >= 10 min => unload even if used recently
        assert!(unload_due(true, true, m(600), m(5)));
        // offline and idle >= 10 min => unload (RAM back while offline & silent)
        assert!(unload_due(true, false, None, m(601)));
        // offline and actively used => keep loaded
        assert!(!unload_due(true, false, None, m(30)));
        // offline, never used (just loaded for the cache) and no timestamp => keep (unknown idle)
        assert!(!unload_due(true, false, None, None));
        // online but recovery time unknown, not idle => keep
        assert!(!unload_due(true, true, None, m(10)));
    }

    #[test]
    fn test_probe_due_matrix() {
        // Pure over injected clock — no statics touched, no lock needed.
        let now = Instant::now();
        let stale = now - Duration::from_secs(31);
        let fresh = now - Duration::from_secs(5);
        // Down + never checked → probe immediately.
        assert!(probe_due_for(None, false, now));
        // Down + stale check → probe.
        assert!(probe_due_for(Some(stale), false, now));
        // Down + fresh check → wait (throttle).
        assert!(!probe_due_for(Some(fresh), false, now));
        // Up → never (per-turn checks + failure paths own down-transitions).
        assert!(!probe_due_for(None, true, now));
        assert!(!probe_due_for(Some(stale), true, now));
    }

    #[test]
    fn test_cloud_restored_payload_shape() {
        // The hub validator must accept exactly this shape.
        let v = cloud_restored_payload();
        assert_eq!(v["status"], "cloud-restored");
    }

    #[test]
    fn test_defaults() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_state();
        // Network should default to up (optimistic)
        assert!(is_network_up());
        // Kokoro should default to not loaded
        assert!(!is_local_loaded());
    }

    #[test]
    fn test_piper_loaded_flag() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_state();
        mark_local_loaded();
        assert!(is_local_loaded());
        mark_local_unloaded();
        assert!(!is_local_loaded());
    }

    #[test]
    fn test_should_unload_local_when_network_down() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_state();
        NETWORK_UP.store(false, Ordering::Relaxed);
        mark_local_loaded();
        assert!(!should_unload_local());
    }

    #[test]
    fn test_should_unload_local_when_not_loaded() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_state();
        NETWORK_UP.store(true, Ordering::Relaxed);
        mark_local_unloaded();
        assert!(!should_unload_local());
    }

    #[test]
    fn test_should_unload_local_when_recently_recovered() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_state();
        NETWORK_UP.store(true, Ordering::Relaxed);
        mark_local_loaded();
        // Set recovery to now — should NOT unload (less than 10 min)
        {
            let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
            *recovered = Some(Instant::now());
        }
        assert!(!should_unload_local());
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
    fn test_should_unload_local_after_10_min() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_state();
        NETWORK_UP.store(true, Ordering::Relaxed);
        mark_local_loaded();
        // Set recovery to 11 minutes ago — should unload
        {
            let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
            *recovered = Some(Instant::now() - Duration::from_secs(660));
        }
        assert!(should_unload_local());
        // Clean up
        mark_local_unloaded();
        {
            let mut recovered = NETWORK_RECOVERED_AT.lock().unwrap();
            *recovered = None;
        }
    }
}
