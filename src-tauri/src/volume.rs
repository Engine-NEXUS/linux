//! Platform-specific system volume control.
//!
//! Before NEXUS speaks (TTS), the system output volume is set to a
//! user-configured level (default 75%) so the user always hears NEXUS
//! at a consistent volume. After TTS completes, the original volume is
//! restored.
//!
//! The bookkeeping lives in [`Controller`], a small state machine that is
//! independent of the OS (it talks to a [`VolumeBackend`]) so it is unit-tested:
//!
//! - **Epoch leases.** `force_restore`/`defer_restore` bump an epoch; a lease
//!   from an older epoch is a no-op when dropped, so a barge-in can never let a
//!   stale speaker decrement the count of the utterance that replaced it.
//! - **Deferred restore.** The last release restores the original volume only
//!   after `restore_delay`; any acquire inside that window cancels it, so
//!   ack -> reply -> next sentence stays at the TTS level with no dip.
//! - **Heartbeat watchdog.** Playback loops call [`heartbeat`]; a lease that has
//!   gone silent for `watchdog` (hung/leaked task) is force-released. Long speech
//!   is never cut off because it keeps beating.
//! - **Durable baseline.** The original volume is written to `volume_baseline.json`
//!   and restored at the next start, so a crash/kill while at the TTS level cannot
//!   make that level the new "original".
//!
//! - Windows: Core Audio COM `IAudioEndpointVolume`
//! - macOS: CoreAudio `AudioObjectGetPropertyData` / `AudioObjectSetPropertyData`
//! - Linux: `wpctl` → `pactl` → `amixer` shell commands

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long after the last utterance the original volume is restored.
const RESTORE_DELAY_MS: u64 = 1200;
/// A lease with no heartbeat for this long is considered leaked.
const WATCHDOG_MS: u64 = 10_000;
const WATCHDOG_POLL_MS: u64 = 500;
/// A baseline file older than this is ignored at startup (the user has likely moved the slider since).
const BASELINE_MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;
const BASELINE_FILE: &str = "volume_baseline.json";

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// OS access, injectable for tests.
pub trait VolumeBackend: Send + Sync {
    fn get(&self) -> Result<f32, String>;
    fn set(&self, level: f32) -> Result<(), String>;
}

struct SystemBackend;

impl VolumeBackend for SystemBackend {
    fn get(&self) -> Result<f32, String> {
        get_system_volume()
    }
    fn set(&self, level: f32) -> Result<(), String> {
        set_system_volume(level)
    }
}

#[derive(Default)]
struct State {
    /// Live leases of the current epoch.
    count: usize,
    /// Bumped by force/defer restore; leases of an older epoch are stale.
    epoch: u64,
    /// The user's volume before we raised/lowered it; `Some` while we own the volume.
    baseline: Option<f32>,
    /// Bumped by every acquire and every scheduled restore; a timer only fires if unchanged.
    restore_gen: u64,
    watchdog_running: bool,
}

pub struct Controller {
    st: Mutex<State>,
    /// Serialises the OS calls (read the current volume, set the TTS volume,
    /// restore the original) so their order is exactly the same as when they
    /// all happened under the state mutex — but WITHOUT holding the state
    /// mutex across them. The backend is the OS: on Linux it is a
    /// `wpctl`/`pactl`/`amixer` subprocess, and a wedged audio tool would
    /// otherwise freeze every other lease holder. The lease is taken on
    /// every spoken reply, so a hung tool there means no reply is ever
    /// spoken and the user's volume stays pinned at the TTS level.
    transition: Mutex<()>,
    backend: Arc<dyn VolumeBackend>,
    baseline_dir: Mutex<Option<PathBuf>>,
    last_beat: AtomicU64,
    restore_delay: Duration,
    watchdog: Duration,
    watchdog_poll: Duration,
}

/// RAII lease: releases its speaker slot when dropped, unless a force/defer restore
/// already ended its epoch.
pub struct TtsVolumeLease {
    ctrl: Arc<Controller>,
    epoch: u64,
    active: bool,
}

impl Drop for TtsVolumeLease {
    fn drop(&mut self) {
        if self.active {
            self.active = false;
            self.ctrl.release(self.epoch);
        }
    }
}

impl Controller {
    pub fn new(backend: Arc<dyn VolumeBackend>) -> Arc<Self> {
        Self::with_timing(
            backend,
            Duration::from_millis(RESTORE_DELAY_MS),
            Duration::from_millis(WATCHDOG_MS),
            Duration::from_millis(WATCHDOG_POLL_MS),
        )
    }

    pub fn with_timing(
        backend: Arc<dyn VolumeBackend>,
        restore_delay: Duration,
        watchdog: Duration,
        watchdog_poll: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            st: Mutex::new(State::default()),
            transition: Mutex::new(()),
            backend,
            baseline_dir: Mutex::new(None),
            last_beat: AtomicU64::new(now_millis()),
            restore_delay,
            watchdog,
            watchdog_poll,
        })
    }

    pub fn set_baseline_dir(&self, dir: PathBuf) {
        *self.baseline_dir.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir);
    }

    fn baseline_path(&self) -> Option<PathBuf> {
        self.baseline_dir
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|d| d.join(BASELINE_FILE))
    }

    fn persist_baseline(&self, value: f32) {
        if let Some(p) = self.baseline_path() {
            let body = format!("{{\"value\":{value},\"ts\":{}}}", now_millis());
            if let Err(e) = std::fs::write(&p, body) {
                tracing::warn!("volume: could not persist baseline: {e}");
            }
        }
    }

    fn clear_baseline_file(&self) {
        if let Some(p) = self.baseline_path() {
            let _ = std::fs::remove_file(p);
        }
    }

    pub fn heartbeat(&self) {
        self.last_beat.store(now_millis(), Ordering::Relaxed);
    }

    /// Put the original volume back. The OS call happens OUTSIDE the state
    /// mutex — the caller takes the baseline out of the state first.
    fn restore_to(&self, level: f32) {
        tracing::info!("[VOL] restoring original volume {:.2}", level);
        let _ = self.backend.set(level);
    }

    pub fn acquire(self: &Arc<Self>, tts_volume: f32) -> Option<TtsVolumeLease> {
        let tts_volume = tts_volume.clamp(0.0, 1.0);
        // The backend is the OS (on Linux: a `wpctl`/`pactl`/`amixer`
        // subprocess). It must never run while the state mutex is held: the
        // lease is taken on every spoken reply, so a wedged audio tool would
        // stop every reply from being spoken. The transition mutex keeps the
        // order of the OS calls exactly as it was.
        let _transition = self.transition.lock().unwrap_or_else(|e| e.into_inner());
        self.heartbeat();

        // (1) Under the lock: cancel a pending restore and note whether the
        // original volume still has to be read.
        let need_baseline = {
            let mut st = self.st.lock().unwrap_or_else(|e| e.into_inner());
            st.restore_gen = st.restore_gen.wrapping_add(1); // cancel any pending restore
            st.baseline.is_none()
        };

        // (2) Outside the lock: read the current volume.
        let current = if need_baseline {
            match self.backend.get() {
                Ok(cur) => Some(cur),
                Err(e) => {
                    // Cannot read => cannot promise to restore => do not touch the volume.
                    tracing::warn!("[VOL] cannot read current volume ({e}); leaving volume untouched");
                    return None;
                }
            }
        } else {
            None
        };

        // (3) Under the lock: remember the original volume.
        if let Some(cur) = current {
            let mut st = self.st.lock().unwrap_or_else(|e| e.into_inner());
            if st.baseline.is_none() {
                st.baseline = Some(cur);
                self.persist_baseline(cur);
                tracing::info!("[VOL] saved original {:.2}, setting {:.2} for TTS", cur, tts_volume);
            }
        }

        // (4) Outside the lock: set the TTS volume.
        if let Err(e) = self.backend.set(tts_volume) {
            tracing::warn!("[VOL] failed to set TTS volume: {e}");
            // Nothing else owns the volume → put the original back.
            let baseline = {
                let mut st = self.st.lock().unwrap_or_else(|e| e.into_inner());
                if st.count == 0 {
                    st.baseline.take()
                } else {
                    None
                }
            };
            if let Some(b) = baseline {
                self.restore_to(b);
                self.clear_baseline_file();
            }
            return None;
        }

        // (5) Under the lock: take the lease.
        let (epoch, start_watchdog) = {
            let mut st = self.st.lock().unwrap_or_else(|e| e.into_inner());
            st.count += 1;
            tracing::debug!("[VOL] acquire epoch={} count={}", st.epoch, st.count);
            let epoch = st.epoch;
            if st.watchdog_running {
                (epoch, false)
            } else {
                st.watchdog_running = true;
                (epoch, true)
            }
        };
        drop(_transition);
        if start_watchdog {
            let me = Arc::clone(self);
            std::thread::spawn(move || me.watchdog_loop());
        }
        Some(TtsVolumeLease { ctrl: Arc::clone(self), epoch, active: true })
    }

    fn release(self: &Arc<Self>, epoch: u64) {
        let _transition = self.transition.lock().unwrap_or_else(|e| e.into_inner());
        let mut st = self.st.lock().unwrap_or_else(|e| e.into_inner());
        if epoch != st.epoch {
            tracing::debug!("[VOL] stale lease (epoch {} != {}) ignored", epoch, st.epoch);
            return;
        }
        st.count = st.count.saturating_sub(1);
        tracing::debug!("[VOL] release epoch={} count={}", st.epoch, st.count);
        if st.count == 0 {
            self.schedule_restore(&mut st);
        }
    }

    /// Caller holds the lock; arms a timer that restores unless an acquire intervenes.
    fn schedule_restore(self: &Arc<Self>, st: &mut State) {
        st.restore_gen = st.restore_gen.wrapping_add(1);
        let gen = st.restore_gen;
        let me = Arc::clone(self);
        let delay = self.restore_delay;
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            me.fire_deferred(gen);
        });
    }

    fn fire_deferred(&self, gen: u64) {
        let _transition = self.transition.lock().unwrap_or_else(|e| e.into_inner());
        let baseline = {
            let mut st = self.st.lock().unwrap_or_else(|e| e.into_inner());
            if gen != st.restore_gen || st.count != 0 {
                return;
            }
            st.baseline.take()
        };
        // The OS call happens outside the state mutex.
        if let Some(b) = baseline {
            self.restore_to(b);
        }
        self.clear_baseline_file();
    }

    /// Immediate restore (explicit stop, exit, watchdog). Existing leases become stale.
    pub fn force_restore(&self) {
        let _transition = self.transition.lock().unwrap_or_else(|e| e.into_inner());
        let baseline = {
            let mut st = self.st.lock().unwrap_or_else(|e| e.into_inner());
            st.epoch = st.epoch.wrapping_add(1);
            st.count = 0;
            st.restore_gen = st.restore_gen.wrapping_add(1);
            st.baseline.take()
        };
        // The OS call happens outside the state mutex.
        if let Some(b) = baseline {
            self.restore_to(b);
        }
        self.clear_baseline_file();
    }

    /// Barge-in: existing leases become stale now, but the volume is restored only if no new
    /// utterance claims it within the restore delay (the next `speak()` follows immediately).
    pub fn defer_restore(self: &Arc<Self>) {
        let _transition = self.transition.lock().unwrap_or_else(|e| e.into_inner());
        let mut st = self.st.lock().unwrap_or_else(|e| e.into_inner());
        st.epoch = st.epoch.wrapping_add(1);
        st.count = 0;
        if st.baseline.is_some() {
            self.schedule_restore(&mut st);
        }
    }

    fn watchdog_loop(self: Arc<Self>) {
        loop {
            std::thread::sleep(self.watchdog_poll);
            let _transition = self.transition.lock().unwrap_or_else(|e| e.into_inner());
            // Read the state under the lock; the OS call happens outside it.
            let mut restore_to: Option<f32> = None;
            let mut stop = false;
            {
                let mut st = self.st.lock().unwrap_or_else(|e| e.into_inner());
                if st.baseline.is_none() {
                    st.watchdog_running = false;
                    stop = true;
                } else if st.count > 0 {
                    let silent = now_millis().saturating_sub(self.last_beat.load(Ordering::Relaxed));
                    if silent as u128 > self.watchdog.as_millis() {
                        tracing::warn!("[VOL] watchdog: no playback heartbeat for {silent} ms, force-restoring");
                        st.epoch = st.epoch.wrapping_add(1);
                        st.count = 0;
                        st.restore_gen = st.restore_gen.wrapping_add(1);
                        st.watchdog_running = false;
                        restore_to = st.baseline.take();
                        stop = true;
                    }
                }
            }
            if stop {
                if let Some(b) = restore_to {
                    self.restore_to(b);
                    self.clear_baseline_file();
                }
                return;
            }
        }
    }

    /// Startup: if a previous run died while holding the volume, put it back.
    pub fn recover_from_crash(&self) {
        let Some(p) = self.baseline_path() else { return };
        let Ok(text) = std::fs::read_to_string(&p) else { return };
        let _ = std::fs::remove_file(&p);
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return };
        let (Some(value), Some(ts)) = (
            v.get("value").and_then(|x| x.as_f64()),
            v.get("ts").and_then(|x| x.as_u64()),
        ) else {
            return;
        };
        let age = now_millis().saturating_sub(ts);
        if age > BASELINE_MAX_AGE_MS {
            tracing::info!("[VOL] ignoring stale baseline file ({} h old)", age / 3_600_000);
            return;
        }
        let value = (value as f32).clamp(0.0, 1.0);
        tracing::info!("[VOL] previous run did not restore the volume; restoring {:.2}", value);
        let _ = self.backend.set(value);
    }

    #[cfg(test)]
    fn snapshot(&self) -> (usize, u64, Option<f32>) {
        let st = self.st.lock().unwrap();
        (st.count, st.epoch, st.baseline)
    }
}

// ─── Global instance + stable free-function API ────────────────────────

static CONTROLLER: OnceLock<Arc<Controller>> = OnceLock::new();

fn ctrl() -> &'static Arc<Controller> {
    CONTROLLER.get_or_init(|| Controller::new(Arc::new(SystemBackend)))
}

/// Startup hook: enable the durable baseline in `app_data_dir` and recover from a crashed run.
pub fn init(app_data_dir: PathBuf) {
    let c = ctrl();
    c.set_baseline_dir(app_data_dir);
    c.recover_from_crash();
}

/// Acquire an RAII lease for TTS volume (sets the system volume to `tts_volume`).
pub fn acquire_volume_lease(tts_volume: f32) -> Option<TtsVolumeLease> {
    ctrl().acquire(tts_volume)
}

/// Playback loops call this (about once a second is plenty) so long speech is never force-restored.
pub fn heartbeat() {
    ctrl().heartbeat();
}

/// Immediate restore: explicit user stop, app exit.
pub fn force_restore_volume() {
    ctrl().force_restore();
}

/// Barge-in restore: stale leases are ended now, the volume returns after a short grace unless
/// the next utterance claims it first.
pub fn defer_restore_volume() {
    ctrl().defer_restore();
}

// ─── Windows: Core Audio COM IAudioEndpointVolume ──────────────────────

#[cfg(target_os = "windows")]
pub fn get_system_volume() -> Result<f32, String> {
    use windows::Win32::Media::Audio::{
        eRender, eMultimedia, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    use windows::core::Interface;

    unsafe {
        // Ensure COM is initialized on this thread (safe to call repeatedly)
        let _ = CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED);

        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("volume: CoCreateInstance failed: {e}"))?;

        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|e| format!("volume: GetDefaultAudioEndpoint failed: {e}"))?;

        // windows 0.36 uses raw Activate (same pattern as meeting_detect.rs)
        let iid = IAudioEndpointVolume::IID;
        let mut ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        device
            .Activate(&iid, CLSCTX_ALL, std::ptr::null(), &mut ptr as *mut _)
            .map_err(|e| format!("volume: Activate IAudioEndpointVolume failed: {e}"))?;
        let endpoint_volume: IAudioEndpointVolume = std::mem::transmute(ptr);

        let level = endpoint_volume
            .GetMasterVolumeLevelScalar()
            .map_err(|e| format!("volume: GetMasterVolumeLevelScalar failed: {e}"))?;

        Ok(level)
    }
}

#[cfg(target_os = "windows")]
pub fn set_system_volume(level: f32) -> Result<(), String> {
    use windows::Win32::Media::Audio::{
        eRender, eMultimedia, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    use windows::core::{GUID, Interface};

    let level = level.clamp(0.0, 1.0);

    unsafe {
        let _ = CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED);

        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("volume: CoCreateInstance failed: {e}"))?;

        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|e| format!("volume: GetDefaultAudioEndpoint failed: {e}"))?;

        let iid = IAudioEndpointVolume::IID;
        let mut ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        device
            .Activate(&iid, CLSCTX_ALL, std::ptr::null(), &mut ptr as *mut _)
            .map_err(|e| format!("volume: Activate IAudioEndpointVolume failed: {e}"))?;
        let endpoint_volume: IAudioEndpointVolume = std::mem::transmute(ptr);

        endpoint_volume
            .SetMasterVolumeLevelScalar(level, &GUID::zeroed())
            .map_err(|e| format!("volume: SetMasterVolumeLevelScalar failed: {e}"))?;

        Ok(())
    }
}

// ─── macOS: CoreAudio FFI ──────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[repr(C)]
struct AudioObjectPropertyAddress {
    mSelector: u32,
    mScope: u32,
    mElement: u32,
}

#[cfg(target_os = "macos")]
extern "C" {
    fn AudioObjectGetPropertyData(
        inObjectID: u32,
        inAddress: *const AudioObjectPropertyAddress,
        inQualifierDataSize: u32,
        inQualifierData: *const std::ffi::c_void,
        ioDataSize: *mut u32,
        outData: *mut std::ffi::c_void,
    ) -> i32;

    fn AudioObjectSetPropertyData(
        inObjectID: u32,
        inAddress: *const AudioObjectPropertyAddress,
        inQualifierDataSize: u32,
        inQualifierData: *const std::ffi::c_void,
        inDataSize: u32,
        inData: *const std::ffi::c_void,
    ) -> i32;
}

#[cfg(target_os = "macos")]
mod consts {
    pub const K_AUDIO_OBJECT_SYSTEM_OBJECT: u32 = 1;
    pub const K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL: u32 = u32::from_be_bytes(*b"glob");
    pub const K_AUDIO_OBJECT_PROPERTY_ELEMENT_MASTER: u32 = 0;
    pub const K_AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE: u32 = u32::from_be_bytes(*b"dOut");
    pub const K_AUDIO_HARDWARE_SERVICE_DEVICE_PROPERTY_VIRTUAL_MASTER_VOLUME: u32 =
        u32::from_be_bytes(*b"vmvc");
    pub const K_AUDIO_DEVICE_PROPERTY_SCOPE_OUTPUT: u32 = u32::from_be_bytes(*b"outp");
}

#[cfg(target_os = "macos")]
fn get_default_output_device() -> Result<u32, String> {
    use consts::*;

    unsafe {
        let mut device_id: u32 = 0;
        let mut size: u32 = std::mem::size_of::<u32>() as u32;
        let addr = AudioObjectPropertyAddress {
            mSelector: K_AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE,
            mScope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
            mElement: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MASTER,
        };
        let status = AudioObjectGetPropertyData(
            K_AUDIO_OBJECT_SYSTEM_OBJECT,
            &addr,
            0,
            std::ptr::null(),
            &mut size,
            &mut device_id as *mut _ as *mut _,
        );
        if status != 0 {
            return Err(format!("volume: GetDefaultOutputDevice failed: {status}"));
        }
        Ok(device_id)
    }
}

#[cfg(target_os = "macos")]
pub fn get_system_volume() -> Result<f32, String> {
    use consts::*;

    unsafe {
        let device_id = get_default_output_device()?;
        let mut volume: f32 = 0.0;
        let mut size: u32 = std::mem::size_of::<f32>() as u32;
        let addr = AudioObjectPropertyAddress {
            mSelector: K_AUDIO_HARDWARE_SERVICE_DEVICE_PROPERTY_VIRTUAL_MASTER_VOLUME,
            mScope: K_AUDIO_DEVICE_PROPERTY_SCOPE_OUTPUT,
            mElement: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MASTER,
        };
        let status = AudioObjectGetPropertyData(
            device_id,
            &addr,
            0,
            std::ptr::null(),
            &mut size,
            &mut volume as *mut _ as *mut _,
        );
        if status != 0 {
            return Err(format!("volume: GetVolume failed: {status}"));
        }
        Ok(volume)
    }
}

#[cfg(target_os = "macos")]
pub fn set_system_volume(level: f32) -> Result<(), String> {
    use consts::*;

    let level = level.clamp(0.0, 1.0);

    unsafe {
        let device_id = get_default_output_device()?;
        let vol = level;
        let addr = AudioObjectPropertyAddress {
            mSelector: K_AUDIO_HARDWARE_SERVICE_DEVICE_PROPERTY_VIRTUAL_MASTER_VOLUME,
            mScope: K_AUDIO_DEVICE_PROPERTY_SCOPE_OUTPUT,
            mElement: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MASTER,
        };
        let status = AudioObjectSetPropertyData(
            device_id,
            &addr,
            0,
            std::ptr::null(),
            std::mem::size_of::<f32>() as u32,
            &vol as *const f32 as *const _,
        );
        if status != 0 {
            return Err(format!("volume: SetVolume failed: {status}"));
        }
        Ok(())
    }
}

// ─── Linux: wpctl → pactl → amixer shell commands ─────────────────────

/// Parse `wpctl get-volume` output.
///
/// `wpctl` prints a 0..1 float: "Volume: 0.50" or "Volume: 0.50 [MUTED]".
/// Pure — unit-tested.
#[cfg(target_os = "linux")]
pub fn parse_wpctl_volume(stdout: &str) -> Option<f32> {
    let line = stdout.lines().next()?;
    let value = line.split_whitespace().nth(1)?;
    let v = value.parse::<f32>().ok()?;
    if (0.0..=1.0).contains(&v) {
        Some(v)
    } else {
        None
    }
}

/// Parse a percentage out of `pactl get-sink-volume` / `amixer get Master`
/// output and return it as a 0..1 level.
///
/// pactl: "Volume: front-left: 32768 /  50% / -6.02 dB,   front-right: …"
/// amixer: "  Front Left: Playback 65536 [100%] [0.00dB]"
///
/// Both tools put the number directly in front of the '%', but pactl
/// separates it with a space ("/  50% /") while amixer wraps it in brackets
/// ("[100%]"). So the number is read as the run of digits that ENDS at the
/// '%', whatever precedes it — the previous "last space-separated token"
/// reading could only ever parse the pactl shape and made the amixer
/// fallback dead code (it always returned `None`).
///
/// Pure — unit-tested.
#[cfg(target_os = "linux")]
pub fn parse_percent_volume(stdout: &str) -> Option<f32> {
    let idx = stdout.find('%')?;
    let before = &stdout[..idx];
    // The digits that end at the '%': "… /  50" → "50", "… [100" → "100".
    let digits: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let num: String = digits.chars().rev().collect();
    let pct = num.parse::<f32>().ok()?;
    if (0.0..=100.0).contains(&pct) {
        Some(pct / 100.0)
    } else {
        None
    }
}

/// How long a single volume tool may take before it is killed. A wedged
/// PipeWire/PulseAudio server makes `wpctl`/`pactl`/`amixer` block forever,
/// and the volume lease is taken on every spoken reply — so a hung tool must
/// cost one timeout, not the reply.
#[cfg(target_os = "linux")]
const TOOL_TIMEOUT_MS: u64 = 1_000;

/// Run one volume tool with a hard deadline.
#[cfg(target_os = "linux")]
fn run_tool(tool: &str, args: &[&str]) -> crate::proc_linux::BoundedOutcome {
    let mut cmd = std::process::Command::new(tool);
    cmd.args(args);
    crate::proc_linux::run_bounded(&mut cmd, TOOL_TIMEOUT_MS)
}

#[cfg(target_os = "linux")]
pub fn get_system_volume() -> Result<f32, String> {
    use crate::proc_linux::BoundedOutcome;

    // Try wpctl (PipeWire/WirePlumber)
    match run_tool("wpctl", &["get-volume", "@DEFAULT_SINK@"]) {
        BoundedOutcome::Finished { success: true, stdout } => {
            if let Some(v) = parse_wpctl_volume(&stdout) {
                return Ok(v);
            }
        }
        BoundedOutcome::TimedOut => {
            tracing::warn!("volume: 'wpctl get-volume' timed out after {TOOL_TIMEOUT_MS} ms");
        }
        _ => {}
    }

    // Fall back to pactl (PulseAudio)
    match run_tool("pactl", &["get-sink-volume", "@DEFAULT_SINK@"]) {
        BoundedOutcome::Finished { success: true, stdout } => {
            if let Some(v) = parse_percent_volume(&stdout) {
                return Ok(v);
            }
        }
        BoundedOutcome::TimedOut => {
            tracing::warn!("volume: 'pactl get-sink-volume' timed out after {TOOL_TIMEOUT_MS} ms");
        }
        _ => {}
    }

    // Fall back to amixer (ALSA)
    match run_tool("amixer", &["get", "Master"]) {
        BoundedOutcome::Finished { success: true, stdout } => {
            if let Some(v) = parse_percent_volume(&stdout) {
                return Ok(v);
            }
        }
        BoundedOutcome::TimedOut => {
            tracing::warn!("volume: 'amixer get Master' timed out after {TOOL_TIMEOUT_MS} ms");
        }
        _ => {}
    }

    Err("volume: all Linux volume tools failed".to_string())
}

#[cfg(target_os = "linux")]
pub fn set_system_volume(level: f32) -> Result<(), String> {
    use crate::proc_linux::BoundedOutcome;

    let level = level.clamp(0.0, 1.0);
    let pct = (level * 100.0).round() as i32;
    let pct_str = format!("{}%", pct);

    // wpctl → pactl → amixer. Each tool is tried in turn: a tool that is
    // installed but refuses the request must not stop the next one. Every
    // tool is bounded, so a wedged audio server costs one timeout.
    let attempts: Vec<(&str, Vec<&str>)> = vec![
        (
            "wpctl",
            vec!["set-volume", "@DEFAULT_SINK@", pct_str.as_str()],
        ),
        (
            "pactl",
            vec!["set-sink-volume", "@DEFAULT_SINK@", pct_str.as_str()],
        ),
        ("amixer", vec!["-q", "set", "Master", pct_str.as_str()]),
    ];

    let mut last_err = String::new();
    for (tool, args) in attempts {
        match run_tool(tool, &args) {
            BoundedOutcome::Finished { success: true, .. } => return Ok(()),
            BoundedOutcome::Finished { success: false, .. } => {
                last_err = format!("{tool} exited with an error");
                tracing::debug!("volume: {tool} failed ({last_err}); trying the next tool");
            }
            BoundedOutcome::TimedOut => {
                last_err = format!("{tool} timed out after {TOOL_TIMEOUT_MS} ms");
                tracing::warn!("volume: {tool} timed out; trying the next tool");
            }
            BoundedOutcome::NotStarted => {
                last_err = format!("{tool} is not installed");
            }
        }
    }

    Err(format!("volume: all Linux volume tools failed ({last_err})"))
}

// ─── Unsupported platforms ─────────────────────────────────────────────

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn get_system_volume() -> Result<f32, String> {
    Err("volume: unsupported platform".to_string())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn set_system_volume(_level: f32) -> Result<(), String> {
    Err("volume: unsupported platform".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct Fake {
        vol: Mutex<f32>,
        sets: AtomicUsize,
    }
    impl Fake {
        fn new(v: f32) -> Arc<Self> {
            Arc::new(Self { vol: Mutex::new(v), sets: AtomicUsize::new(0) })
        }
        fn now(&self) -> f32 {
            *self.vol.lock().unwrap()
        }
    }
    impl VolumeBackend for Fake {
        fn get(&self) -> Result<f32, String> {
            Ok(*self.vol.lock().unwrap())
        }
        fn set(&self, l: f32) -> Result<(), String> {
            *self.vol.lock().unwrap() = l;
            self.sets.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn rig(initial: f32) -> (Arc<Fake>, Arc<Controller>) {
        let fake = Fake::new(initial);
        let c = Controller::with_timing(
            fake.clone(),
            Duration::from_millis(80),
            Duration::from_millis(400),
            Duration::from_millis(40),
        );
        (fake, c)
    }

    fn sleep(ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.001
    }

    #[test]
    fn sets_tts_level_then_restores_after_delay() {
        let (fake, c) = rig(0.20);
        let lease = c.acquire(0.70).expect("lease");
        assert!(near(fake.now(), 0.70));
        drop(lease);
        assert!(near(fake.now(), 0.70), "restore must be deferred, not immediate");
        sleep(250);
        assert!(near(fake.now(), 0.20), "original volume must come back");
        assert_eq!(c.snapshot().2, None);
    }

    #[test]
    fn back_to_back_utterances_never_dip() {
        let (fake, c) = rig(0.20);
        drop(c.acquire(0.70).unwrap());
        sleep(30); // inside the restore window
        let second = c.acquire(0.70).unwrap();
        sleep(200); // the first timer would have fired by now
        assert!(near(fake.now(), 0.70), "volume dipped between utterances");
        drop(second);
        sleep(250);
        assert!(near(fake.now(), 0.20));
    }

    #[test]
    fn baseline_is_not_overwritten_by_tts_level() {
        // The old heuristic stored the already-raised level as the "original" on the next speak.
        let (fake, c) = rig(0.20);
        drop(c.acquire(0.70).unwrap());
        sleep(20);
        let _l = c.acquire(0.70).unwrap();
        assert_eq!(c.snapshot().2, Some(0.20));
        drop(_l);
        sleep(250);
        assert!(near(fake.now(), 0.20));
    }

    #[test]
    fn stale_lease_after_force_restore_is_a_noop() {
        let (fake, c) = rig(0.20);
        let old = c.acquire(0.70).unwrap();
        c.force_restore(); // barge-in
        assert!(near(fake.now(), 0.20));
        let new = c.acquire(0.70).unwrap();
        drop(old); // the cancelled task finally unwinds
        sleep(200);
        assert!(near(fake.now(), 0.70), "stale lease restored the volume under the new utterance");
        assert_eq!(c.snapshot().0, 1, "stale lease changed the live count");
        drop(new);
        sleep(250);
        assert!(near(fake.now(), 0.20));
    }

    #[test]
    fn defer_restore_then_next_utterance_keeps_level() {
        let (fake, c) = rig(0.20);
        let old = c.acquire(0.70).unwrap();
        c.defer_restore(); // stopTts() before the next speak()
        let new = c.acquire(0.70).unwrap();
        drop(old);
        sleep(200);
        assert!(near(fake.now(), 0.70));
        drop(new);
        sleep(250);
        assert!(near(fake.now(), 0.20));
    }

    #[test]
    fn defer_restore_with_no_follow_up_restores() {
        let (fake, c) = rig(0.20);
        let old = c.acquire(0.70).unwrap();
        c.defer_restore();
        drop(old);
        sleep(250);
        assert!(near(fake.now(), 0.20));
    }

    #[test]
    fn unmatched_release_cannot_underflow_or_steal() {
        let (_fake, c) = rig(0.20);
        let a = c.acquire(0.70).unwrap();
        c.release(c.snapshot().1); // a release with no matching acquire (old narrate bug)
        c.release(c.snapshot().1);
        c.release(c.snapshot().1);
        assert_eq!(c.snapshot().0, 0);
        drop(a); // its own release saturates at 0
        assert_eq!(c.snapshot().0, 0);
    }

    #[test]
    fn long_lease_with_heartbeat_survives_the_watchdog() {
        let (fake, c) = rig(0.20);
        let lease = c.acquire(0.70).unwrap();
        for _ in 0..20 {
            sleep(60); // 1.2 s total, three times the 400 ms watchdog
            c.heartbeat();
        }
        assert!(near(fake.now(), 0.70), "long speech was force-restored");
        drop(lease);
        sleep(250);
        assert!(near(fake.now(), 0.20));
    }

    #[test]
    fn leaked_lease_is_released_by_the_watchdog() {
        let (fake, c) = rig(0.20);
        let lease = c.acquire(0.70).unwrap();
        std::mem::forget(lease); // a task that died without dropping
        sleep(900);
        assert!(near(fake.now(), 0.20), "leaked lease kept the volume raised");
        assert_eq!(c.snapshot().0, 0);
    }

    #[test]
    fn baseline_file_is_written_and_removed() {
        let dir = std::env::temp_dir().join(format!("nexus_vol_{}", now_millis()));
        std::fs::create_dir_all(&dir).unwrap();
        let (_fake, c) = rig(0.35);
        c.set_baseline_dir(dir.clone());
        let lease = c.acquire(0.70).unwrap();
        assert!(dir.join(BASELINE_FILE).exists());
        drop(lease);
        sleep(250);
        assert!(!dir.join(BASELINE_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crash_recovery_restores_the_original() {
        let dir = std::env::temp_dir().join(format!("nexus_vol_crash_{}", now_millis()));
        std::fs::create_dir_all(&dir).unwrap();
        // Run 1 dies while speaking at 70%: the baseline file stays behind.
        {
            let (fake, c) = rig(0.25);
            c.set_baseline_dir(dir.clone());
            let lease = c.acquire(0.70).unwrap();
            std::mem::forget(lease);
            assert!(near(fake.now(), 0.70));
        }
        // Run 2 starts with the volume still at 70%.
        let (fake2, c2) = rig(0.70);
        c2.set_baseline_dir(dir.clone());
        c2.recover_from_crash();
        assert!(near(fake2.now(), 0.25), "volume not restored after a crash");
        assert!(!dir.join(BASELINE_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_baseline_file_is_ignored() {
        let dir = std::env::temp_dir().join(format!("nexus_vol_old_{}", now_millis()));
        std::fs::create_dir_all(&dir).unwrap();
        let old_ts = now_millis() - BASELINE_MAX_AGE_MS - 1000;
        std::fs::write(dir.join(BASELINE_FILE), format!("{{\"value\":0.1,\"ts\":{old_ts}}}")).unwrap();
        let (fake, c) = rig(0.60);
        c.set_baseline_dir(dir.clone());
        c.recover_from_crash();
        assert!(near(fake.now(), 0.60));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unreadable_volume_leaves_system_untouched() {
        struct Broken;
        impl VolumeBackend for Broken {
            fn get(&self) -> Result<f32, String> {
                Err("no device".into())
            }
            fn set(&self, _: f32) -> Result<(), String> {
                panic!("must not set when the original cannot be read")
            }
        }
        let c = Controller::new(Arc::new(Broken));
        assert!(c.acquire(0.7).is_none());
    }

    /// Live OS round-trip (touches the real volume briefly; restores it).
    ///
    /// On Linux the real volume lives behind `wpctl` / `pactl` / `amixer` — a unit test must
    /// never spawn those against the user's audio server, so the live probe is skipped there
    /// and the pure parsers below carry the coverage.
    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "spawns wpctl/pactl/amixer against the real audio server"
    )]
    fn test_volume_roundtrip() {
        let vol = get_system_volume();
        if let Ok(v) = vol {
            assert!(v >= 0.0 && v <= 1.0);
        }
    }

    // ─── Linux tool output parsing ───────────────────────────────────

    #[cfg(target_os = "linux")]
    mod linux_parsing {
        use super::*;

        #[test]
        fn wpctl_output_is_parsed_as_a_zero_to_one_level() {
            assert!(near(parse_wpctl_volume("Volume: 0.50").unwrap(), 0.50));
            assert!(near(parse_wpctl_volume("Volume: 0.50 [MUTED]").unwrap(), 0.50));
            assert!(near(parse_wpctl_volume("Volume: 1.00").unwrap(), 1.00));
            assert!(near(parse_wpctl_volume("Volume: 0.00").unwrap(), 0.00));
        }

        #[test]
        fn wpctl_output_that_is_not_a_level_is_rejected() {
            assert!(parse_wpctl_volume("").is_none());
            assert!(parse_wpctl_volume("Volume:").is_none());
            // wpctl prints a 0..1 float, never a percentage — "50%" is not a level.
            assert!(parse_wpctl_volume("Volume: 50%").is_none());
        }

        /// pactl prints the percentage as a bare number ("/  50% /"), amixer
        /// wraps it in brackets ("[100%]") — both must parse.
        #[test]
        fn pactl_and_amixer_output_is_parsed_as_a_percentage() {
            let pactl = "Volume: front-left: 32768 /  50% / -6.02 dB,   front-right: 32768 /  50% / -6.02 dB";
            assert!(near(parse_percent_volume(pactl).unwrap(), 0.50));
            let amixer = "Simple mixer control 'Master',0\n  Front Left: Playback 65536 [100%] [0.00dB]";
            assert!(near(parse_percent_volume(amixer).unwrap(), 1.00));
            assert!(near(parse_percent_volume("Playback 0 [0%]").unwrap(), 0.0));
        }

        /// The verbatim `pactl get-sink-volume @DEFAULT_SINK@` output of a
        /// mono sink — one channel, one percentage.
        #[test]
        fn a_mono_pactl_sink_is_parsed() {
            let pactl = "Volume: mono: 65536 / 100% / 0.00 dB";
            assert!(near(parse_percent_volume(pactl).unwrap(), 1.00));
        }

        /// The verbatim `amixer get Master` output: a header, the channel
        /// lines, and the percentage in brackets on each of them.
        #[test]
        fn a_full_amixer_block_is_parsed() {
            let amixer = "Simple mixer control 'Master',0\nCapabilities: pvolume pswitch pswitch-joined\nPlayback channels: Front Left - Front Right\nLimits: Playback 0 - 65536\nMono:\nFront Left: Playback 32768 [50%] [-6.02dB]\nFront Right: Playback 32768 [50%] [-6.02dB]\n";
            assert!(near(parse_percent_volume(amixer).unwrap(), 0.50));
        }

        #[test]
        fn percent_output_without_a_number_is_rejected() {
            assert!(parse_percent_volume("").is_none());
            assert!(parse_percent_volume("no percentage here").is_none());
            // A '%' with no digits in front of it is not a level.
            assert!(parse_percent_volume("%").is_none());
            assert!(parse_percent_volume("Volume: front-left: 32768 /  % / -6.02 dB").is_none());
        }
    }
}
