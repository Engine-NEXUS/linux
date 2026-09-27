# Feature 29 — TTS Auto-Volume: Set System Volume Before Speaking

> **Status:** PLAN — not yet implemented
> **Date:** 2026-09-02
> **Scope:** Before NEXUS speaks (TTS), automatically set the system
> output volume to a user-configured level (default 75%). After TTS
> completes, restore the original volume. Works on Windows, macOS, and
> Linux.

---

## TL;DR

**User says:** "NEXUS, analyse PR 24 in zync"
**NEXUS says:** "On it sir"

But before NEXUS speaks, the system volume is temporarily set to 75%
(the configured level), so the user always hears NEXUS at a consistent
volume — regardless of what they had their system volume set to (e.g.,
they had it at 10% while watching a video). After NEXUS finishes
speaking, the volume is restored to 10%.

**Flow:**
```
User speaks command
  → NEXUS decides to speak
  → SAVE current system volume (e.g., 0.10)
  → SET system volume to configured TTS level (e.g., 0.75)
  → NEXUS speaks "On it sir"
  → TTS playback completes
  → RESTORE system volume to saved value (0.10)
```

**Configuration:**
- New setting: `ttsVolume` (0-100, default 75)
- Set in the Settings window → Audio & Voice tab
- Also settable in the Setup wizard → Preferences step
- Stored in `settings.json` alongside `ttsVoice`, `speechRate`, etc.

---

## Architecture

### Where to intercept: `speak_text` in `tts.rs`

The volume save/set/restore must happen in the **Rust `speak_text`
command** (`src-tauri/src/tts.rs`), NOT in the frontend `speak()`
function. Reasons:

1. **`speak_text` is the single chokepoint** — all TTS audio goes
   through this one function. The frontend `speak()` calls
   `invoke("speak_text", ...)` which enters `speak_text` in Rust.
2. **Volume control is platform-specific** — only Rust can call
   Windows COM, macOS CoreAudio, and Linux shell commands.
3. **The rodio playback happens inside `speak_text`** — the volume
   must be set before rodio starts playing and restored after rodio
   finishes.
4. **Barge-in safety** — if the user barges in (Ctrl+Space) during
   TTS, the volume must still be restored. The `TTS_GENERATION` check
   in `speak_text` already handles this — we add the restore call
   in the same code path.

### Modified `speak_text` flow

```
speak_text(text, voice, speed, state, meeting)
  │
  ├── 1. meeting.set_tts_playing(true)
  ├── 2. Capture TTS_GENERATION
  ├── 3. Lazy-load Kokoro engine
  ├── 4. Synthesize audio (spawn_blocking)
  │
  ├── 5. NEW: Save current system volume
  │      └── windows: IAudioEndpointVolume::GetMasterVolumeLevelScalar()
  │      └── macos:   AudioObjectGetPropertyData(VirtualMasterVolume)
  │      └── linux:   wpctl get-volume @DEFAULT_SINK@ → parse
  │
  ├── 6. NEW: Set system volume to configured TTS level
  │      └── windows: IAudioEndpointVolume::SetMasterVolumeLevelScalar(0.75)
  │      └── macos:   AudioObjectSetPropertyData(VirtualMasterVolume, 0.75)
  │      └── linux:   wpctl set-volume @DEFAULT_SINK@ 75%
  │
  ├── 7. Play audio via rodio (spawn_blocking)
  │      └── polls TTS_GENERATION for barge-in
  │
  ├── 8. NEW: Restore system volume to saved value
  │      └── windows: IAudioEndpointVolume::SetMasterVolumeLevelScalar(saved)
  │      └── macos:   AudioObjectSetPropertyData(VirtualMasterVolume, saved)
  │      └── linux:   wpctl set-volume @DEFAULT_SINK@ {saved}%
  │
  ├── 9. 500ms grace period
  └── 10. meeting.set_tts_playing(false)
```

### Edge cases

| Case | Behavior |
|------|----------|
| Barge-in (Ctrl+Space) during TTS | Volume restored in the barge-in return path (step 7's early return) |
| TTS synthesis fails | Volume was never changed (save/set happens AFTER synthesis) |
| Meeting mode suppresses TTS | `speak()` in frontend returns early — `speak_text` never called |
| Web Speech API fallback | Volume save/set/restore only works in `speak_text` (Rust). Web Speech fallback in frontend won't have volume control. This is acceptable — Web Speech is a last resort. |
| No audio device | Get/Set volume calls fail gracefully — TTS still plays (rodio uses its own output stream) |
| Configured volume = 0 | NEXUS would be silent. Clamp minimum to 1% to prevent this. |
| Rapid consecutive speaks | Each speak saves/restores independently. If the second speak starts before the first restores, the second save captures the TTS volume (not the user's original). Fix: use a global `saved_volume` that's only captured if not already in TTS mode. |

---

## Part 1 — Windows Implementation

### 1.1 API

**Get volume:**
```rust
IAudioEndpointVolume::GetMasterVolumeLevelScalar() → f32 (0.0 to 1.0)
```

**Set volume:**
```rust
IAudioEndpointVolume::SetMasterVolumeLevelScalar(f32, &GUID::zeroed())
```

**Full COM call chain:**
```rust
use windows::Win32::Media::Audio::{
    eRender, eMultimedia, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::core::{Interface, GUID};

fn get_default_endpoint_volume() -> Result<f32, String> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("CoCreateInstance: {e}"))?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|e| format!("GetDefaultAudioEndpoint: {e}"))?;
        // windows 0.36 uses raw Activate (same pattern as meeting_detect.rs)
        let iid = IAudioEndpointVolume::IID;
        let mut ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        device.Activate(&iid, CLSCTX_ALL, std::ptr::null(), &mut ptr as *mut _)?;
        let volume: IAudioEndpointVolume = std::mem::transmute(ptr);
        let level = volume.GetMasterVolumeLevelScalar()
            .map_err(|e| format!("GetMasterVolumeLevelScalar: {e}"))?;
        Ok(level)
    }
}

fn set_default_endpoint_volume(level: f32) -> Result<(), String> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("CoCreateInstance: {e}"))?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|e| format!("GetDefaultAudioEndpoint: {e}"))?;
        let iid = IAudioEndpointVolume::IID;
        let mut ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        device.Activate(&iid, CLSCTX_ALL, std::ptr::null(), &mut ptr as *mut _)?;
        let volume: IAudioEndpointVolume = std::mem::transmute(ptr);
        volume.SetMasterVolumeLevelScalar(level, &GUID::zeroed())
            .map_err(|e| format!("SetMasterVolumeLevelScalar: {e}"))?;
        Ok(())
    }
}
```

**This is the exact same pattern NEXUS already uses in
`meeting_detect.rs`** (lines 345-380) — `CoCreateInstance` →
`GetDefaultAudioEndpoint` → `Activate` → transmute. The only difference
is the interface (`IAudioEndpointVolume` instead of
`IAudioSessionManager2`).

### 1.2 Feature flag

The `IAudioEndpointVolume` interface is in the
`Win32_Media_Audio_Endpoints` submodule, which requires the
`Win32_Media_Audio_Endpoints` feature in the `windows` crate.

**Current `Cargo.toml`:**
```toml
windows = { version = "0.36", features = [
    "Win32_Media_Audio",
    # ...
] }
```

**Required change:**
```toml
windows = { version = "0.36", features = [
    "Win32_Media_Audio",
    "Win32_Media_Audio_Endpoints",   # ← ADD THIS
    # ...
] }
```

### 1.3 Permissions

**No admin required.** No UAC. No manifest. No TCC. The Core Audio COM
API is available to all user-mode processes. This is the same API
family NEXUS already uses for meeting detection.

### 1.4 Latency

- `CoCreateInstance` + `GetDefaultAudioEndpoint` + `Activate`: ~1-2ms
- `GetMasterVolumeLevelScalar`: ~0.1ms
- `SetMasterVolumeLevelScalar`: ~0.1ms
- **Total save+set: ~2ms** (imperceptible)
- **Total restore: ~2ms** (imperceptible)

### 1.5 COM threading

NEXUS already calls `CoCreateInstance` without explicit
`CoInitializeEx` in `meeting_detect.rs` — Tauri's main thread has COM
initialized. For `speak_text` (which runs on a tokio worker thread),
we need to ensure COM is initialized on that thread.

**Solution:** Call `CoInitializeEx` at the start of the volume
functions with `COINIT_MULTITHREADED`, and ignore the "already
initialized" return code. This is safe and standard practice.

```rust
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

fn get_default_endpoint_volume() -> Result<f32, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        // ... rest of the function
    }
}
```

---

## Part 2 — macOS Implementation

### 2.1 API

**Get volume:**
```c
AudioObjectGetPropertyData(deviceID,
    &{kAudioHardwareServiceDeviceProperty_VirtualMasterVolume,
      kAudioDevicePropertyScopeOutput,
      kAudioObjectPropertyElementMaster},
    0, NULL, &size, &volume)  // Float32
```

**Set volume:**
```c
AudioObjectSetPropertyData(deviceID,
    &{kAudioHardwareServiceDeviceProperty_VirtualMasterVolume,
      kAudioDevicePropertyScopeOutput,
      kAudioObjectPropertyElementMaster},
    0, NULL, sizeof(Float32), &volume)  // Float32
```

**Step 1: Get default output device:**
```c
AudioObjectGetPropertyData(kAudioObjectSystemObject,
    &{kAudioHardwarePropertyDefaultOutputDevice,
      kAudioObjectPropertyScopeGlobal,
      kAudioObjectPropertyElementMaster},
    0, NULL, &size, &deviceID)  // AudioDeviceID
```

### 2.2 Rust FFI

macOS CoreAudio is a C framework. We use direct FFI:

```rust
#[repr(C)]
struct AudioObjectPropertyAddress {
    mSelector: u32,
    mScope: u32,
    mElement: u32,
}

// FourCC constants (from CoreAudio headers)
const kAudioObjectSystemObject: u32 = 1;
const kAudioObjectPropertyScopeGlobal: u32 = u32::from_be_bytes(*b"glob");
const kAudioObjectPropertyElementMaster: u32 = 0;
const kAudioHardwarePropertyDefaultOutputDevice: u32 = u32::from_be_bytes(*b"dOut");
const kAudioHardwareServiceDeviceProperty_VirtualMasterVolume: u32 = u32::from_be_bytes(*b"vmvc");
const kAudioDevicePropertyScopeOutput: u32 = u32::from_be_bytes(*b"outp");

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

fn get_default_output_device() -> Result<u32, String> {
    unsafe {
        let mut device_id: u32 = 0;
        let mut size: u32 = std::mem::size_of::<u32>() as u32;
        let addr = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMaster,
        };
        let status = AudioObjectGetPropertyData(
            kAudioObjectSystemObject, &addr, 0, std::ptr::null(),
            &mut size, &mut device_id as *mut _ as *mut _,
        );
        if status != 0 { return Err(format!("GetDefaultOutputDevice: {status}")); }
        Ok(device_id)
    }
}

fn get_macos_volume() -> Result<f32, String> {
    unsafe {
        let device_id = get_default_output_device()?;
        let mut volume: f32 = 0.0;
        let mut size: u32 = std::mem::size_of::<f32>() as u32;
        let addr = AudioObjectPropertyAddress {
            mSelector: kAudioHardwareServiceDeviceProperty_VirtualMasterVolume,
            mScope: kAudioDevicePropertyScopeOutput,
            mElement: kAudioObjectPropertyElementMaster,
        };
        let status = AudioObjectGetPropertyData(
            device_id, &addr, 0, std::ptr::null(),
            &mut size, &mut volume as *mut _ as *mut _,
        );
        if status != 0 { return Err(format!("GetVolume: {status}")); }
        Ok(volume)
    }
}

fn set_macos_volume(volume: f32) -> Result<(), String> {
    unsafe {
        let device_id = get_default_output_device()?;
        let vol = volume;
        let addr = AudioObjectPropertyAddress {
            mSelector: kAudioHardwareServiceDeviceProperty_VirtualMasterVolume,
            mScope: kAudioDevicePropertyScopeOutput,
            mElement: kAudioObjectPropertyElementMaster,
        };
        let status = AudioObjectSetPropertyData(
            device_id, &addr, 0, std::ptr::null(),
            std::mem::size_of::<f32>() as u32,
            &vol as *const f32 as *const _,
        );
        if status != 0 { return Err(format!("SetVolume: {status}")); }
        Ok(())
    }
}
```

### 2.3 Permissions

**No entitlement needed for output volume.** CoreAudio property reads
and writes work in the sandbox without any entitlement. The
`com.apple.security.device.audio-input` entitlement is only for
microphone INPUT, not speaker OUTPUT.

The current `osascript` approach requires TCC Automation permission.
Direct CoreAudio calls **bypass TCC entirely** — no prompt, no
permission, no revocation risk.

### 2.4 Latency

- `AudioObjectGetPropertyData` (device ID): ~0.1ms
- `AudioObjectGetPropertyData` (volume): ~0.1ms
- `AudioObjectSetPropertyData` (volume): ~0.1ms
- **Total save+set: ~0.3ms** (imperceptible)
- **Total restore: ~0.3ms** (imperceptible)

### 2.5 Linker

CoreAudio is a system framework on macOS. The FFI functions are in
`/System/Library/Frameworks/CoreAudio.framework`. No additional
dependency needed — Rust links to system frameworks automatically on
macOS.

To be explicit in `Cargo.toml` (optional):
```toml
[target.'cfg(target_os = "macos")'.dependencies]
core-audio = { version = "0.4", features = ["audio_unit"] }
```
Or just use direct FFI (no dependency needed).

---

## Part 3 — Linux Implementation

### 3.1 API

Linux uses shell commands with a three-layer fallback:

**Get volume:**
```bash
# PipeWire (wpctl)
wpctl get-volume @DEFAULT_SINK@
# Output: "Volume: 0.50" or "Volume: 0.50 [MUTED]"
# Parse: extract the float, multiply by 100

# PulseAudio (pactl)
pactl get-sink-volume @DEFAULT_SINK@
# Output: "Volume: front-left: 32768 /  50% / -6.02 dB,   front-right: ..."
# Parse: grep first percentage

# ALSA (amixer)
amixer get Master
# Output: "  Front Left: Playback 50% [...] [-20.00dB] [...]"
# Parse: grep first percentage
```

**Set volume:**
```bash
# PipeWire
wpctl set-volume @DEFAULT_SINK@ 75%

# PulseAudio
pactl set-sink-volume @DEFAULT_SINK@ 75%

# ALSA
amixer -q set Master 75%
```

### 3.2 Rust implementation

```rust
fn get_linux_volume() -> Result<f32, String> {
    // Try wpctl first
    if let Ok(output) = std::process::Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_SINK@"])
        .creation_flags(0x08000000)
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Parse "Volume: 0.50" or "Volume: 0.50 [MUTED]"
        if let Some(line) = stdout.lines().next() {
            if let Some(vol_str) = line.split_whitespace().nth(1) {
                if let Ok(vol) = vol_str.parse::<f32>() {
                    return Ok(vol);
                }
            }
        }
    }

    // Fall back to pactl
    if let Ok(output) = std::process::Command::new("pactl")
        .args(["get-sink-volume", "@DEFAULT_SINK@"])
        .creation_flags(0x08000000)
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Parse "Volume: front-left: 32768 /  50% / ..."
        let pct = stdout.split('%').next()
            .and_then(|s| s.rsplit(' ').next())
            .and_then(|s| s.parse::<f32>().ok());
        if let Some(pct) = pct {
            return Ok(pct / 100.0);
        }
    }

    // Fall back to amixer
    if let Ok(output) = std::process::Command::new("amixer")
        .args(["get", "Master"])
        .creation_flags(0x08000000)
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let pct = stdout.split('%').next()
            .and_then(|s| s.rsplit(' ').next())
            .and_then(|s| s.parse::<f32>().ok());
        if let Some(pct) = pct {
            return Ok(pct / 100.0);
        }
    }

    Err("All volume tools failed".to_string())
}

fn set_linux_volume(volume: f32) -> Result<(), String> {
    let pct = (volume * 100.0).round() as i32;
    let pct_str = format!("{}%", pct);

    // Try wpctl → pactl → amixer
    std::process::Command::new("wpctl")
        .args(["set-volume", "@DEFAULT_SINK@", &pct_str])
        .creation_flags(0x08000000)
        .status()
        .or_else(|_| std::process::Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &pct_str])
            .creation_flags(0x08000000)
            .status())
        .or_else(|_| std::process::Command::new("amixer")
            .args(["-q", "set", "Master", &pct_str])
            .creation_flags(0x08000000)
            .status())
        .map_err(|e| format!("All volume tools failed: {e}"))?;
    Ok(())
}
```

### 3.3 Permissions

- **No root required.** Shell commands talk to the user's audio daemon.
- **User must be in `audio` group** on some distros, or gets automatic
  access via `systemd-logind` active session.
- **No TCC/sandbox** on Linux.

### 3.4 Latency

- `wpctl get-volume`: ~10-20ms (process spawn + PipeWire IPC)
- `wpctl set-volume`: ~10-20ms
- **Total save+set: ~30ms** (imperceptible)
- **Total restore: ~30ms** (imperceptible)

---

## Part 4 — Settings Integration

### 4.1 New setting: `ttsVolume`

Add to `NexusSettings` struct in `commands.rs`:

```rust
pub struct NexusSettings {
    // ... existing fields ...
    #[serde(default = "default_tts_volume")]
    pub tts_volume: u8,  // 0-100, default 75
}

fn default_tts_volume() -> u8 {
    75
}
```

Add to `Default` impl:
```rust
impl Default for NexusSettings {
    fn default() -> Self {
        Self {
            // ... existing fields ...
            tts_volume: 75,
        }
    }
}
```

### 4.2 Settings UI

Add a slider to the "Audio & Voice" tab in `SettingsApp.tsx`:

```tsx
<div className="setting-row">
  <label>TTS Volume</label>
  <input
    type="range"
    min="1"
    max="100"
    value={settings.ttsVolume ?? 75}
    onChange={(e) => setSettings({
      ...settings,
      ttsVolume: parseInt(e.target.value)
    })}
  />
  <span>{settings.ttsVolume ?? 75}%</span>
</div>
```

### 4.3 Setup wizard

Add to the "Preferences" step in `SetupApp.tsx`:
- A slider for "NEXUS Volume" with default 75%
- Explanation: "NEXUS will set your system volume to this level while speaking"

### 4.4 Passing the setting to `speak_text`

**Option A: Read settings inside `speak_text` (recommended)**
```rust
#[tauri::command]
pub async fn speak_text(
    text: String,
    voice: Option<String>,
    speed: Option<f32>,
    state: State<'_, TtsState>,
    meeting: State<'_, Arc<MeetingState>>,
    app: tauri::AppHandle,  // ← ADD to read settings
) -> Result<(), String> {
    // Read tts_volume from settings.json
    let tts_volume = read_tts_volume(&app);
    // ... save / set / speak / restore ...
}
```

This is simpler than passing it from the frontend and ensures the
volume is always applied even if the frontend `speak()` function is
bypassed (e.g., by a future Rust-side TTS trigger).

**Option B: Pass from frontend**
```typescript
await invoke("speak_text", { text, voice: voiceId, speed, ttsVolume: settings.ttsVolume });
```

**Recommendation: Option A** — read settings in Rust. This ensures the
volume is always applied regardless of how `speak_text` is called.

---

## Part 5 — Implementation Plan

### Step 1: Add `Win32_Media_Audio_Endpoints` feature to `Cargo.toml`

```toml
windows = { version = "0.36", features = [
    "Win32_Media_Audio",
    "Win32_Media_Audio_Endpoints",   # ← ADD
    # ...
] }
```

### Step 2: Create `src-tauri/src/volume.rs` module

New module with platform-specific get/set volume functions:
- `pub fn get_system_volume() -> Result<f32, String>` — returns 0.0–1.0
- `pub fn set_system_volume(level: f32) -> Result<(), String>` — accepts 0.0–1.0
- Windows: COM `IAudioEndpointVolume` (same pattern as `meeting_detect.rs`)
- macOS: CoreAudio FFI (`AudioObjectGetPropertyData` / `AudioObjectSetPropertyData`)
- Linux: shell commands (`wpctl` → `pactl` → `amixer`)

### Step 3: Add `tts_volume` to `NexusSettings`

- Add field to struct in `commands.rs`
- Add default value (75) in `Default` impl
- Add `#[serde(default = "default_tts_volume")]`

### Step 4: Modify `speak_text` in `tts.rs`

Add volume save/set/restore around the rodio playback:
```rust
// After synthesis, before playback:
let saved_volume = volume::get_system_volume().unwrap_or(-1.0);
if saved_volume >= 0.0 {
    let _ = volume::set_system_volume(tts_volume as f32 / 100.0);
}

// ... rodio playback ...

// After playback (including barge-in early return):
if saved_volume >= 0.0 {
    let _ = volume::set_system_volume(saved_volume);
}
```

**Critical: The restore must happen in ALL exit paths:**
1. Normal playback completion
2. Barge-in (TTS_GENERATION mismatch)
3. Rodio error
4. Audio thread panic

### Step 5: Add volume slider to Settings UI

- Add to "Audio & Voice" tab in `SettingsApp.tsx`
- Range: 1-100, default 75
- Label: "TTS Volume — NEXUS sets system volume to this level while speaking"

### Step 6: Add volume slider to Setup wizard

- Add to "Preferences" step in `SetupApp.tsx`
- Same slider, same default

### Step 7: Register `volume` module in `lib.rs`

```rust
mod volume;
```

### Step 8: Build and test

1. `npm --prefix frontend run build`
2. `cargo build --release --features custom-protocol`
3. Launch NEXUS
4. Set system volume to 10% manually
5. Say "NEXUS" → "hello"
6. Verify: volume jumps to 75% during "Hello, I am Sky" → restores to 10% after
7. Test barge-in: say "NEXUS" → start speaking → Ctrl+Space → verify volume restores
8. Test with volume at 100%: verify NEXUS doesn't go above 75%
9. Test with volume at 0%: verify NEXUS still speaks at 75%

### Step 9: Set default to 75% for current computer

The default `tts_volume: 75` in `NexusSettings::default()` ensures
that on first launch (no `settings.json` yet), the TTS volume is 75%.
This applies to the current computer and all new installations.

---

## Part 6 — Rapid Consecutive Speak Protection

If NEXUS speaks twice in rapid succession (e.g., "On it sir" followed
by "Here is the analysis, sir"), the second `speak_text` call might
start before the first one's restore completes. This would cause the
second save to capture the TTS volume (75%) instead of the user's
original volume (10%).

**Solution: Global saved-volume state**

```rust
use std::sync::atomic::{AtomicF32, AtomicBool, Ordering};

static SAVED_VOLUME: AtomicF32 = AtomicF32::new(-1.0);
static TTS_VOLUME_ACTIVE: AtomicBool = AtomicBool::new(false);

fn save_volume() {
    if TTS_VOLUME_ACTIVE.load(Ordering::SeqCst) {
        // Already in TTS mode — don't overwrite the saved volume
        return;
    }
    let vol = get_system_volume().unwrap_or(-1.0);
    SAVED_VOLUME.store(vol, Ordering::SeqCst);
    TTS_VOLUME_ACTIVE.store(true, Ordering::SeqCst);
}

fn restore_volume() {
    if !TTS_VOLUME_ACTIVE.load(Ordering::SeqCst) {
        return;
    }
    let saved = SAVED_VOLUME.load(Ordering::SeqCst);
    if saved >= 0.0 {
        let _ = set_system_volume(saved);
    }
    TTS_VOLUME_ACTIVE.store(false, Ordering::SeqCst);
}
```

This ensures:
- First `speak_text`: saves 10%, sets 75%, speaks, restores 10%
- Second `speak_text` (starts during first restore): sees
  `TTS_VOLUME_ACTIVE=true`, doesn't overwrite saved 10%, sets 75%,
  speaks, restores 10%
- If second `speak_text` starts after first restore:
  `TTS_VOLUME_ACTIVE=false`, saves current (10%), sets 75%, speaks,
  restores 10%

---

## Part 7 — Testing Checklist

### Build verification

- [ ] `Cargo.toml` has `Win32_Media_Audio_Endpoints` feature
- [ ] `volume.rs` module compiles on Windows (COM path)
- [ ] `volume.rs` module compiles on macOS (FFI path) — verify on macOS
- [ ] `volume.rs` module compiles on Linux (shell path) — verify on Linux
- [ ] Frontend build succeeds with new settings slider
- [ ] Rust release build succeeds

### Runtime verification (Windows)

- [ ] Set system volume to 10% → say "NEXUS" → volume jumps to 75% during TTS → restores to 10%
- [ ] Set system volume to 100% → say "NEXUS" → volume drops to 75% during TTS → restores to 100%
- [ ] Set system volume to 50% → say "NEXUS" → volume goes to 75% → restores to 50%
- [ ] Barge-in: say "NEXUS" → Ctrl+Space during TTS → volume restores immediately
- [ ] Rapid consecutive: "On it sir" → "Here is the analysis" → volume stays at 75% during both → restores after both
- [ ] Settings: change TTS volume to 50% → next speak uses 50%
- [ ] Settings: change TTS volume to 100% → next speak uses 100%
- [ ] No audio device: get/set volume fails gracefully, TTS still plays

### Settings UI

- [ ] Slider appears in Audio & Voice tab
- [ ] Slider default is 75%
- [ ] Slider saves to settings.json
- [ ] Slider value persists across restarts
- [ ] Setup wizard has the same slider

---

## Files to Change

| File | Change |
|------|--------|
| `src-tauri/Cargo.toml` | Add `Win32_Media_Audio_Endpoints` feature |
| `src-tauri/src/volume.rs` | NEW: platform-specific get/set volume functions |
| `src-tauri/src/lib.rs` | Register `volume` module |
| `src-tauri/src/tts.rs` | Add save/set/restore volume in `speak_text` |
| `src-tauri/src/commands.rs` | Add `tts_volume` field to `NexusSettings` |
| `frontend/src/settings/SettingsApp.tsx` | Add TTS volume slider in Audio tab |
| `frontend/src/setup/SetupApp.tsx` | Add TTS volume slider in Preferences step |

---

## Why Not Use Per-Stream Volume (rodio)?

rodio's `Sink::set_volume()` controls the **per-stream volume** of the
TTS audio, not the system volume. If the user has their system volume
at 10%, even with rodio at 100%, the output would be 10% of maximum.

To make NEXUS always audible at a consistent level, we MUST control the
**system master volume**, not just the per-stream volume. This is what
the user requested: "nexus will always shift the system volume to
certain number."

However, we could ALSO set rodio's per-stream volume to 1.0 (100%) to
ensure the TTS audio itself isn't attenuated. This is already the
default (rodio plays at 100% unless `set_volume` is called).

---

## Why Not Use `osascript` on macOS?

The current `volume_mute()` in `command_executor.rs` uses:
```rust
Command::new("osascript").args(["-e", "set volume with output muted"])
```

This works but has problems for the TTS auto-volume feature:
1. **TCC Automation permission required** — first use triggers a
   system dialog asking for permission to control System Events
2. **~300ms latency** — spawns `osascript` process
3. **`set volume X` (0-100) is a separate AppleScript call** — can't
   get current volume with AppleScript easily (need
   `output volume of (get volume settings)`)
4. **Permission can be revoked** — if the user revokes Automation
   permission in System Settings, NEXUS loses volume control

Direct CoreAudio FFI has none of these problems:
- No TCC permission needed for output volume
- ~0.3ms latency
- Get and set in the same API
- Cannot be revoked (it's hardware control, not privacy-sensitive)

---

## References

### Windows
- [IAudioEndpointVolume interface](https://learn.microsoft.com/en-us/windows/win32/api/endpointvolume/nn-endpointvolume-iaudioendpointvolume)
- [windows crate Rust docs — IAudioEndpointVolume](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Media/Audio/Endpoints/struct.IAudioEndpointVolume.html)
- [windows crate 0.36 features list](https://docs.rs/crate/windows/0.36.1/features)
- [windows-rs issue #1676 — COM Activate pattern](https://github.com/microsoft/windows-rs/issues/1676)
- NEXUS `meeting_detect.rs` lines 335-380 — existing COM pattern

### macOS
- [Technical Q&A QA1016: Changing the volume of audio devices](https://developer.apple.com/library/archive/qa/qa1016/_index.html)
- [Change OS X system volume programmatically](https://stackoverflow.com/questions/17715111/change-os-x-system-volume-programmatically)
- [Sandboxing a Mac system utility](https://getshowmode.com/blog/sandboxing-a-system-utility/)
- [SMALLPEARL: Get/set volume in OS X](https://www.smallpearl.com/blog/setget-volume-in-os-x)

### Linux
- [wpctl(1) — WirePlumber documentation](https://pipewire.pages.freedesktop.org/wireplumber/tools/wpctl.html)
- [PipeWire — ArchWiki](https://wiki.archlinux.org/title/PipeWire)
- [pactl get-sink-volume parsing](https://unix.stackexchange.com/questions/132230/read-out-pulseaudio-volume-from-commandline-i-want-pactl-get-sink-volume)
