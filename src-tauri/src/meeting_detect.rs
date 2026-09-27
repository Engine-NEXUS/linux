//! Meeting / privacy mode detection.
//!
//! Prevents NEXUS from waking or speaking during active meetings (Google Meet,
//! Zoom, Teams, Discord, etc.) by detecting when another application is
//! actively capturing microphone audio.
//!
//! ## Architecture (3 layers + manual override)
//!
//! **Layer 0 — Manual pause** (tray menu):
//!   User clicks "Pause NEXUS" → `manual_pause = true`.
//!   Overrides everything. Must be manually cleared via "Resume NEXUS".
//!
//! **Layer 1 — WASAPI session detection** (Windows, primary):
//!   Polls `IAudioSessionManager2` every 2 seconds.
//!   Enumerates active audio capture sessions on the default microphone.
//!   If any *other* process has an active capture session → `meeting_active = true`.
//!   Skips NEXUS's own PID and the Windows Audio Service.
//!   Works for ANY app that uses the mic — no app list needed.
//!
//! **Layer 2 — Process name detection** (cross-platform fallback):
//!   Uses `sysinfo` to check for known meeting app processes.
//!   Less precise than WASAPI (can't tell if Chrome has a Meet tab vs browsing),
//!   but works on macOS/Linux and as a Windows backup.
//!
//! **Layer 3 — TTS-aware muting** (all platforms):
//!   Frontend emits `tts-started` / `tts-ended` events.
//!   While TTS is playing, wake detection is suppressed to prevent
//!   NEXUS from hearing its own voice and re-triggering.
//!
//! ## Decision logic
//!
//! ```text
//! should_suppress_wake() =
//!   manual_pause      // Layer 0: user explicitly paused
//!   || tts_playing    // Layer 3: NEXUS is speaking
//!   || meeting_active // Layer 1+2: another app is using the mic
//! ```
//!
//! When suppressed:
//!   - Wake-word detection is paused (audio is still consumed but not classified)
//!   - Tier 3 command detection is paused
//!   - No wake events are emitted
//!   - The hotkey still works (explicit user action)
//!   - TTS is suppressed in meeting mode (frontend checks the state)

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Shared state for meeting/privacy mode.
///
/// All flags use `AtomicBool` so they can be read from the audio callback
/// thread (which runs at real-time priority) without locking.
#[derive(Debug)]
pub struct MeetingState {
    /// Manual pause — set by tray menu "Pause NEXUS".
    /// Overrides all other layers. Must be cleared by "Resume NEXUS".
    pub manual_pause: AtomicBool,

    /// A meeting/call is active (another app is using the microphone).
    /// Set by WASAPI detection (Windows) or process detection (fallback).
    pub meeting_active: AtomicBool,

    /// NEXUS is currently speaking TTS.
    /// Set by frontend events `tts-started` / `tts-ended`.
    pub tts_playing: AtomicBool,

    /// Meeting detection is enabled (can be disabled in settings).
    pub detection_enabled: AtomicBool,

    /// Monotonic epoch for computing elapsed time in the audio callback.
    /// Set at construction; never changes.
    epoch: Instant,

    /// Monotonic millisecond timestamp (relative to `epoch`) when TTS
    /// playback ended. Used by the post-TTS mute gate in the audio callback
    /// to suppress wake detection for a grace period after TTS finishes,
    /// preventing TTS echo from re-triggering the wake word.
    ///
    /// 0 means TTS has never ended (or hasn't started yet), so the gate
    /// is not active.
    last_tts_end_ms: AtomicU64,
}

impl MeetingState {
    pub fn new() -> Self {
        Self {
            manual_pause: AtomicBool::new(false),
            meeting_active: AtomicBool::new(false),
            tts_playing: AtomicBool::new(false),
            detection_enabled: AtomicBool::new(true),
            epoch: Instant::now(),
            last_tts_end_ms: AtomicU64::new(0),
        }
    }

    /// Returns `true` if wake-word and command detection should be suppressed.
    ///
    /// Called from the audio callback on every chunk — must be fast.
    /// All reads use `Ordering::Relaxed` (no cross-thread synchronization needed
    /// for a simple boolean flag that is allowed to be slightly stale).
    #[inline]
    #[allow(dead_code)]
    pub fn should_suppress_wake(&self) -> bool {
        self.manual_pause.load(Ordering::Relaxed)
            || self.tts_playing.load(Ordering::Relaxed)
            || (self.detection_enabled.load(Ordering::Relaxed)
                && self.meeting_active.load(Ordering::Relaxed))
    }

    /// Returns `true` if TTS should be suppressed (meeting mode is active).
    ///
    /// The frontend checks this before calling `speak()`.
    /// Manual pause does NOT suppress TTS — the user might want to hear
    /// responses even when they've paused wake detection.
    /// Only auto-detected meetings suppress TTS.
    #[inline]
    pub fn should_suppress_tts(&self) -> bool {
        self.detection_enabled.load(Ordering::Relaxed)
            && self.meeting_active.load(Ordering::Relaxed)
    }

    /// Returns `true` if the user has manually paused NEXUS.
    #[inline]
    pub fn is_paused(&self) -> bool {
        self.manual_pause.load(Ordering::Relaxed)
    }

    /// Returns `true` if a meeting is currently detected.
    #[inline]
    pub fn is_meeting_active(&self) -> bool {
        self.meeting_active.load(Ordering::Relaxed)
    }

    /// Toggle manual pause. Returns the new state.
    pub fn toggle_pause(&self) -> bool {
        let now = !self.manual_pause.load(Ordering::Relaxed);
        self.manual_pause.store(now, Ordering::Relaxed);
        now
    }

    /// Set manual pause explicitly.
    ///
    /// Unused in production (the tray uses `toggle_pause`), but exercised by
    /// the unit tests in this file. Kept for the test suite and as a stable
    /// API for future callers (e.g. a settings-window toggle).
    #[allow(dead_code)]
    pub fn set_paused(&self, paused: bool) {
        self.manual_pause.store(paused, Ordering::Relaxed);
    }

    /// Set TTS playing state (called from frontend event handler or tts.rs).
    ///
    /// When transitioning from `true` → `false`, records the end time so the
    /// post-TTS mute gate in the audio callback can suppress wake detection
    /// for a grace period. This prevents TTS echo from re-triggering the wake
    /// word after `tts_playing` goes false.
    pub fn set_tts_playing(&self, playing: bool) {
        let was_playing = self.tts_playing.swap(playing, Ordering::Relaxed);
        if was_playing && !playing {
            let now_ms = Instant::now()
                .duration_since(self.epoch)
                .as_millis() as u64;
            self.last_tts_end_ms.store(now_ms, Ordering::Relaxed);
        }
    }

    /// Returns the number of milliseconds since TTS playback ended.
    ///
    /// Returns `u64::MAX` if TTS has never ended (so the gate is inactive).
    /// Called from the audio callback on every chunk — must be fast.
    #[inline]
    pub fn ms_since_tts_ended(&self) -> u64 {
        let end_ms = self.last_tts_end_ms.load(Ordering::Relaxed);
        if end_ms == 0 {
            return u64::MAX;
        }
        let now_ms = Instant::now()
            .duration_since(self.epoch)
            .as_millis() as u64;
        now_ms.saturating_sub(end_ms)
    }

    /// Set meeting active state (called from detection polling loop).
    pub fn set_meeting_active(&self, active: bool) {
        self.meeting_active.store(active, Ordering::Relaxed);
    }
}

impl Default for MeetingState {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Meeting detection polling ───────────────────────────────────────

/// Known conferencing application process names.
///
/// Only used on non-Windows platforms: Windows uses WASAPI session
/// enumeration (`check_wasapi_microphone_usage`), which detects actual
/// microphone usage rather than mere process presence.
#[cfg(not(target_os = "windows"))]
const MEETING_PROCESS_NAMES: &[&str] = &[
    // macOS
    "zoom.us",
    "Microsoft Teams",
    "Skype",
    "Cisco Webex Meetings",
    "webex",
    // Linux
    "zoom",
    "teams",
    "skypeforlinux",
    "webex",
];

/// Check if any known meeting application process is running.
///
/// Uses the `sysinfo` crate. Non-Windows only — this is less precise than
/// WASAPI because it detects whether a meeting app is *running*, not
/// whether it is actively using the microphone.
#[cfg(not(target_os = "windows"))]
fn check_meeting_processes() -> bool {
    use sysinfo::System;
    let mut sys = System::new_all();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    sys.processes().values().any(|process| {
        let name = process.name().to_string_lossy();
        MEETING_PROCESS_NAMES
            .iter()
            .any(|meeting_name| name.eq_ignore_ascii_case(meeting_name))
    })
}

/// Run the meeting detection polling loop.
///
/// Polls every 2 seconds. On Windows, uses WASAPI session enumeration
/// (detects actual mic usage by other processes). On macOS/Linux, uses
/// process name detection (less precise — detects if a meeting app is
/// running, not if it's actively using the mic).
///
/// Hysteresis: requires the meeting signal to be stable for 1 consecutive
/// poll before activating, and absent for 2 consecutive polls before
/// deactivating. This prevents flicker from transient audio sessions.
pub async fn run_detection_loop(state: Arc<MeetingState>) {
    tracing::info!("meeting detection: polling loop started (2s interval)");

    let poll_interval = std::time::Duration::from_secs(2);
    // Hysteresis counters
    let mut active_votes = 0u32;
    let mut inactive_votes = 0u32;
    const ACTIVATE_THRESHOLD: u32 = 1; // Activate after 1 positive poll (~2s)
    const DEACTIVATE_THRESHOLD: u32 = 2; // Deactivate after 2 negative polls (~4s)

    let mut current_state = false;

    loop {
        tokio::time::sleep(poll_interval).await;

        if !state.detection_enabled.load(Ordering::Relaxed) {
            if current_state {
                state.set_meeting_active(false);
                current_state = false;
                tracing::info!("meeting detection: disabled, clearing meeting state");
            }
            continue;
        }

        // Layer 1: WASAPI (Windows) — detect actual mic usage by other processes
        #[cfg(target_os = "windows")]
        let detected = check_wasapi_microphone_usage();

        // Layer 2: Process detection (macOS/Linux fallback)
        #[cfg(not(target_os = "windows"))]
        let detected = check_meeting_processes();

        // Apply hysteresis
        if detected {
            active_votes = active_votes.saturating_add(1);
            inactive_votes = 0;
            if !current_state && active_votes >= ACTIVATE_THRESHOLD {
                current_state = true;
                state.set_meeting_active(true);
                tracing::info!(
                    "meeting detection: meeting ACTIVE — suppressing wake & TTS"
                );
            }
        } else {
            inactive_votes = inactive_votes.saturating_add(1);
            active_votes = 0;
            if current_state && inactive_votes >= DEACTIVATE_THRESHOLD {
                current_state = false;
                state.set_meeting_active(false);
                tracing::info!(
                    "meeting detection: meeting ended — resuming wake & TTS"
                );
            }
        }
    }
}

// ─── Windows WASAPI microphone session detection ─────────────────────

#[cfg(target_os = "windows")]
fn check_wasapi_microphone_usage() -> bool {
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED,
    };

    // Initialize COM for this thread
    let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED) };
    let com_initialized = hr.is_ok();

    let result = unsafe { wasapi_check_inner() };

    if com_initialized {
        unsafe { CoUninitialize() };
    }

    result
}

/// Get all descendant PIDs of the given parent PID (children, grandchildren, etc.).
///
/// Uses Toolhelp32Snapshot to walk the process tree. This is needed because
/// NEXUS's audio capture is done by `msedgewebview2.exe` child processes,
/// not by `nexus.exe` itself. Without this, NEXUS detects its own WebView2
/// children as "meetings" and suppresses itself.
#[cfg(target_os = "windows")]
fn get_descendant_pids(parent_pid: u32) -> std::collections::HashSet<u32> {
    use std::collections::HashSet;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::Foundation::CloseHandle;

    let mut descendants: HashSet<u32> = HashSet::new();

    let snapshot = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
        Ok(s) => s,
        Err(_) => return descendants,
    };

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    // Build a map of PID → parent PID
    let mut parent_map: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    if unsafe { Process32FirstW(snapshot, &mut entry) }.as_bool() {
        loop {
            parent_map.insert(entry.th32ProcessID, entry.th32ParentProcessID);
            if !unsafe { Process32NextW(snapshot, &mut entry) }.as_bool() {
                break;
            }
        }
    }
    let _ = unsafe { CloseHandle(snapshot) };

    // BFS from parent_pid to find all descendants
    let mut queue = vec![parent_pid];
    while let Some(pid) = queue.pop() {
        for (&child_pid, &parent) in &parent_map {
            if parent == pid && !descendants.contains(&child_pid) {
                descendants.insert(child_pid);
                queue.push(child_pid);
            }
        }
    }

    descendants
}

#[cfg(target_os = "windows")]
unsafe fn wasapi_check_inner() -> bool {
    use windows::Win32::Media::Audio::{
        eCapture, eConsole, AudioSessionStateActive, IAudioSessionControl2,
        IAudioSessionEnumerator, IAudioSessionManager2, IMMDeviceEnumerator,
        MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    use windows::core::{Interface, GUID};

    // 1. Get the IMMDeviceEnumerator
    let enumerator: IMMDeviceEnumerator =
        match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("meeting detection: CoCreateInstance failed: {e}");
                return false;
            }
        };

    // 2. Get the default capture (microphone) endpoint
    let device = match enumerator.GetDefaultAudioEndpoint(eCapture, eConsole) {
        Ok(d) => d,
        Err(e) => {
            tracing::debug!("meeting detection: no default capture endpoint: {e}");
            return false;
        }
    };

    // 3. Get IAudioSessionManager2 from the device
    //    windows 0.36 uses the raw COM Activate method (not generic)
    let iid_iaudiosessionmanager2: GUID = IAudioSessionManager2::IID;
    let mut ptr: *mut std::ffi::c_void = std::ptr::null_mut();
    let hr = device.Activate(
        &iid_iaudiosessionmanager2,
        CLSCTX_ALL,
        std::ptr::null(),
        &mut ptr as *mut *mut _,
    );
    let mgr: IAudioSessionManager2 = match hr {
        Ok(()) => std::mem::transmute::<*mut std::ffi::c_void, IAudioSessionManager2>(ptr),
        Err(e) => {
            tracing::warn!("meeting detection: Activate IAudioSessionManager2 failed: {e}");
            return false;
        }
    };

    // 4. Enumerate audio sessions
    let session_list: IAudioSessionEnumerator = match mgr.GetSessionEnumerator() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("meeting detection: GetSessionEnumerator failed: {e}");
            return false;
        }
    };

    let count = match session_list.GetCount() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("meeting detection: GetCount failed: {e}");
            return false;
        }
    };

    let our_pid = std::process::id();

    // Get all descendant PIDs of NEXUS (msedgewebview2.exe children, etc.)
    // This prevents NEXUS from detecting its own WebView2 audio capture as a "meeting".
    let descendant_pids = get_descendant_pids(our_pid);

    // 5. Check each session
    for i in 0..count {
        let ctrl = match session_list.GetSession(i) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Get IAudioSessionControl2 for process ID
        let ctrl2: IAudioSessionControl2 = match ctrl.cast() {
            Ok(c2) => c2,
            Err(_) => continue,
        };

        let pid = match ctrl2.GetProcessId() {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Skip our own process (NEXUS is always capturing for wake word)
        if pid == our_pid {
            continue;
        }

        // Skip all descendant processes (msedgewebview2.exe children, etc.)
        // NEXUS uses WebView2 for mic capture — those child processes must not
        // trigger false meeting detection.
        if descendant_pids.contains(&pid) {
            continue;
        }

        // Get process name to skip system audio service and NEXUS's WebView2
        let proc_name = get_process_name(pid).unwrap_or_default();
        if proc_name.contains("AudioSrv") || proc_name.contains("audiodg") {
            continue;
        }

        // Belt and suspenders: skip any msedgewebview2.exe process.
        // NEXUS uses WebView2 (msedgewebview2.exe) for audio capture.
        // The Edge browser uses msedge.exe (not msedgewebview2.exe),
        // so this won't miss real meetings in Edge.
        if proc_name.eq_ignore_ascii_case("msedgewebview2.exe") {
            continue;
        }

        // Skip Windows system processes that have audio capture sessions
        // but are NOT meeting apps. These can trigger false positives when
        // the user has Settings, Search, or other system UI open.
        const SYSTEM_PROCESS_EXCLUSIONS: &[&str] = &[
            "SystemSettings.exe",        // Windows Settings (mic privacy page, etc.)
            "SearchApp.exe",              // Windows Search
            "ShellExperienceHost.exe",    // Windows Shell
            "ApplicationFrameHost.exe",   // UWP app host
            "RuntimeBroker.exe",          // UWP broker
            "svchost.exe",                // Windows services
            "dwm.exe",                    // Desktop Window Manager
            "explorer.exe",               // Windows Explorer
            "TextInputHost.exe",          // Text input (touch keyboard, etc.)
            "WindowsInternal.ComposableShell.Experiences.TextInput.InputApp.exe",
        ];
        if SYSTEM_PROCESS_EXCLUSIONS.iter().any(|s| proc_name.eq_ignore_ascii_case(s)) {
            continue;
        }

        // Check if this session is actively capturing
        let state = match ctrl.GetState() {
            Ok(s) => s,
            Err(_) => continue,
        };

        if state == AudioSessionStateActive {
            tracing::debug!(
                "meeting detection: active capture session from '{}' (PID {})",
                proc_name,
                pid
            );
            return true;
        }
    }

    false
}

#[cfg(target_os = "windows")]
fn get_process_name(pid: u32) -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::K32GetModuleFileNameExW;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
            false,
            pid,
        )
        .ok()?;

        let mut buffer: Vec<u16> = std::iter::repeat(0).take(1024).collect();
        let length = K32GetModuleFileNameExW(Some(handle), None, &mut buffer);
        let _ = CloseHandle(handle);

        if length == 0 {
            return None;
        }

        buffer.truncate(length as usize);
        let os_string = OsString::from_wide(&buffer);
        let path_str = os_string.to_string_lossy().to_string();
        path_str.rsplit('\\').next().map(|s| s.to_string())
    }
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
fn get_process_name(_pid: u32) -> Option<String> {
    None
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_meeting_state_default() {
        let state = MeetingState::new();
        assert!(!state.should_suppress_wake());
        assert!(!state.is_paused());
        assert!(!state.is_meeting_active());
    }

    #[test]
    fn test_manual_pause() {
        let state = MeetingState::new();
        state.set_paused(true);
        assert!(state.should_suppress_wake());
        assert!(state.is_paused());
    }

    #[test]
    fn test_meeting_active() {
        let state = MeetingState::new();
        state.set_meeting_active(true);
        assert!(state.should_suppress_wake());
        assert!(state.is_meeting_active());
    }

    #[test]
    fn test_tts_playing() {
        let state = MeetingState::new();
        state.set_tts_playing(true);
        assert!(state.should_suppress_wake());
    }

    #[test]
    fn test_detection_disabled_overrides_meeting() {
        let state = MeetingState::new();
        state.detection_enabled.store(false, Ordering::Relaxed);
        state.set_meeting_active(true);
        // Meeting is active but detection is disabled → should NOT suppress
        assert!(!state.should_suppress_wake());
    }

    #[test]
    fn test_manual_pause_works_even_with_detection_disabled() {
        let state = MeetingState::new();
        state.detection_enabled.store(false, Ordering::Relaxed);
        state.set_paused(true);
        // Manual pause always works, even if auto-detection is disabled
        assert!(state.should_suppress_wake());
    }

    #[test]
    fn test_tts_playing_works_even_with_detection_disabled() {
        let state = MeetingState::new();
        state.detection_enabled.store(false, Ordering::Relaxed);
        state.set_tts_playing(true);
        // TTS muting always works, even if auto-detection is disabled
        assert!(state.should_suppress_wake());
    }

    #[test]
    fn test_should_suppress_tts() {
        let state = MeetingState::new();
        // No meeting → TTS allowed
        assert!(!state.should_suppress_tts());

        // Meeting active → TTS suppressed
        state.set_meeting_active(true);
        assert!(state.should_suppress_tts());

        // Manual pause alone does NOT suppress TTS
        state.set_meeting_active(false);
        state.set_paused(true);
        assert!(!state.should_suppress_tts());
    }

    #[test]
    fn test_toggle_pause() {
        let state = MeetingState::new();
        assert!(!state.is_paused());
        assert!(state.toggle_pause()); // → true
        assert!(state.is_paused());
        assert!(!state.toggle_pause()); // → false
        assert!(!state.is_paused());
    }
}
