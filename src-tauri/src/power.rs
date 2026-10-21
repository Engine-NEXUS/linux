//! System power and energy efficiency optimizations.
//!
//! Enables Windows EcoQoS (Efficiency Mode) to schedule background threads
//! exclusively on E-cores (Efficiency cores), reducing idle power draw by up to 70%.

#[cfg(target_os = "windows")]
pub fn enable_process_ecoqos() {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, SetProcessInformation, ProcessPowerThrottling,
        PROCESS_POWER_THROTTLING_STATE, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    };

    // Diagnostic kill switch: `$env:NEXUS_NO_ECOQOS=1; nexus start` to test whether efficiency mode
    // slows the camera pipeline (hand mode field test 2).
    if std::env::var("NEXUS_NO_ECOQOS").map(|v| v == "1").unwrap_or(false) {
        tracing::info!("power: EcoQoS disabled by NEXUS_NO_ECOQOS=1");
        return;
    }
    unsafe {
        let mut throttling = PROCESS_POWER_THROTTLING_STATE {
            Version: 1, // PROCESS_POWER_THROTTLING_CURRENT_VERSION
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        };

        let ok = SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            &mut throttling as *mut _ as *mut _,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );

        if ok.as_bool() {
            tracing::info!("power: Windows EcoQoS (Efficiency Mode) enabled for daemon");
        } else {
            tracing::debug!("power: Windows EcoQoS not enabled (unsupported OS build or hypervisor)");
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn enable_process_ecoqos() {}

// ─── Linux: the logind session ─────────────────────────────────────────
//
// `systemctl poweroff|reboot|suspend` and `loginctl lock-session|
// terminate-session` all act on a *session*. `XDG_SESSION_ID` names the
// session the display manager started for this user; without it, loginctl
// falls back to the caller's own session — which on a normal desktop login is
// the same one.

/// The logind session id of the current desktop session, if the environment
/// names one.
#[cfg(target_os = "linux")]
pub fn session_id() -> Option<String> {
    std::env::var("XDG_SESSION_ID")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}
