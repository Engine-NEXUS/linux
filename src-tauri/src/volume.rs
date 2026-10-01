//! Platform-specific system volume control.
//!
//! Before NEXUS speaks (TTS), the system output volume is set to a
//! user-configured level (default 75%) so the user always hears NEXUS
//! at a consistent volume. After TTS completes, the original volume is
//! restored.
//!
//! - Windows: Core Audio COM `IAudioEndpointVolume`
//! - macOS: CoreAudio `AudioObjectGetPropertyData` / `AudioObjectSetPropertyData`
//! - Linux: `wpctl` → `pactl` → `amixer` shell commands

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

/// Global saved volume — captured before TTS, restored after.
/// Stored as bits of an f32 in an AtomicI32 (AtomicF32 is not in stable Rust).
/// -1.0 (bits: 0xBF800000) means "no volume saved" (not yet in TTS mode).
static SAVED_VOLUME: AtomicI32 = AtomicI32::new((-1.0f32).to_bits() as i32);

/// Whether we're currently in TTS-volume mode (volume has been changed
/// and not yet restored). Prevents rapid consecutive speaks from
/// overwriting the saved volume.
static TTS_VOLUME_ACTIVE: AtomicBool = AtomicBool::new(false);

fn load_saved_volume() -> f32 {
    f32::from_bits(SAVED_VOLUME.load(Ordering::SeqCst) as u32)
}

fn store_saved_volume(vol: f32) {
    SAVED_VOLUME.store(vol.to_bits() as i32, Ordering::SeqCst);
}

/// Save the current system volume and set it to `tts_volume` (0.0–1.0).
///
/// If we're already in TTS-volume mode (rapid consecutive speak), the
/// saved volume is NOT overwritten — the original user volume is
/// preserved and will be restored when the last speak finishes.
///
/// Returns true if the volume was successfully changed, false if it
/// failed (in which case TTS should proceed without volume adjustment).
pub fn save_and_set_volume(tts_volume: f32) -> bool {
    // If already in TTS mode, don't overwrite the saved volume
    if TTS_VOLUME_ACTIVE.load(Ordering::SeqCst) {
        tracing::debug!(
            "volume: already in TTS mode, keeping saved volume, setting to {:.2}",
            tts_volume
        );
        return set_system_volume(tts_volume).is_ok();
    }

    let saved = get_system_volume().unwrap_or(-1.0);
    if saved < 0.0 {
        tracing::warn!("volume: failed to get current volume, skipping TTS volume adjust");
        return false;
    }

    store_saved_volume(saved);
    TTS_VOLUME_ACTIVE.store(true, Ordering::SeqCst);

    tracing::info!(
        "volume: saved {:.2}, setting to {:.2} for TTS",
        saved,
        tts_volume
    );

    if set_system_volume(tts_volume).is_err() {
        tracing::warn!("volume: failed to set TTS volume, will restore original");
        // Restore the active flag so we don't leak state
        TTS_VOLUME_ACTIVE.store(false, Ordering::SeqCst);
        store_saved_volume(-1.0);
        return false;
    }

    true
}

/// Restore the system volume to the value saved by `save_and_set_volume`.
///
/// If we're not in TTS-volume mode, this is a no-op. If the second of
/// two rapid consecutive speaks calls this, it's a no-op (the first
/// speak's restore already happened, or the second speak set the flag
/// again — but the saved volume is the same).
///
/// Actually, for rapid consecutive speaks, each speak calls
/// `save_and_set_volume` (which is a no-op save if already active) and
/// `restore_volume` (which restores and clears the flag). So the
/// sequence is:
///   speak1: save(10%) → set(75%) → speak → restore(10%) → clear flag
///   speak2: save(10%) → set(75%) → speak → restore(10%) → clear flag
/// If speak2 starts before speak1's restore:
///   speak1: save(10%) → set(75%) → speak → [barge-in or overlap]
///   speak2: already active → set(75%) → speak → restore(10%) → clear
///   speak1: restore → no-op (flag already cleared by speak2)
/// This is correct — the original 10% is restored after the last speak.
pub fn restore_volume() {
    if !TTS_VOLUME_ACTIVE.load(Ordering::SeqCst) {
        return;
    }

    let saved = load_saved_volume();
    if saved >= 0.0 {
        tracing::info!("volume: restoring to {:.2}", saved);
        let _ = set_system_volume(saved);
    }

    store_saved_volume(-1.0);
    TTS_VOLUME_ACTIVE.store(false, Ordering::SeqCst);
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

#[cfg(target_os = "linux")]
pub fn get_system_volume() -> Result<f32, String> {
    use std::process::Command;

    // Try wpctl (PipeWire/WirePlumber)
    if let Ok(output) = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_SINK@"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Output: "Volume: 0.50" or "Volume: 0.50 [MUTED]"
        if let Some(line) = stdout.lines().next() {
            if let Some(vol_str) = line.split_whitespace().nth(1) {
                if let Ok(vol) = vol_str.parse::<f32>() {
                    return Ok(vol);
                }
            }
        }
    }

    // Fall back to pactl (PulseAudio)
    if let Ok(output) = Command::new("pactl")
        .args(["get-sink-volume", "@DEFAULT_SINK@"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Output: "Volume: front-left: 32768 /  50% / -6.02 dB,   front-right: ..."
        // Parse the first percentage
        if let Some(idx) = stdout.find('%') {
            let before = &stdout[..idx];
            if let Some(num_str) = before.rsplit(' ').next() {
                if let Ok(pct) = num_str.parse::<f32>() {
                    return Ok(pct / 100.0);
                }
            }
        }
    }

    // Fall back to amixer (ALSA)
    if let Ok(output) = Command::new("amixer").args(["get", "Master"]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(idx) = stdout.find('%') {
            let before = &stdout[..idx];
            if let Some(num_str) = before.rsplit(' ').next() {
                if let Ok(pct) = num_str.parse::<f32>() {
                    return Ok(pct / 100.0);
                }
            }
        }
    }

    Err("volume: all Linux volume tools failed".to_string())
}

#[cfg(target_os = "linux")]
pub fn set_system_volume(level: f32) -> Result<(), String> {
    use std::process::Command;

    let level = level.clamp(0.0, 1.0);
    let pct = (level * 100.0).round() as i32;
    let pct_str = format!("{}%", pct);

    // Try wpctl → pactl → amixer
    let result = Command::new("wpctl")
        .args(["set-volume", "@DEFAULT_SINK@", &pct_str])
        .status()
        .or_else(|_| {
            Command::new("pactl")
                .args(["set-sink-volume", "@DEFAULT_SINK@", &pct_str])
                .status()
        })
        .or_else(|_| {
            Command::new("amixer")
                .args(["-q", "set", "Master", &pct_str])
                .status()
        });

    match result {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("volume: set command exited with {s}")),
        Err(e) => Err(format!("volume: all Linux volume tools failed: {e}")),
    }
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
