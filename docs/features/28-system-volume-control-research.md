# Feature 28 — System Volume Control Research (Windows, macOS, Linux)

> **Status:** Research complete — implementation pending
> **Date:** 2026-09-02
> **Scope:** How to programmatically get, set, mute, and adjust system
> volume on Windows, macOS, and Linux — including permissions, sandbox
> constraints, API choices, and Rust integration paths.

---

## TL;DR

**Does NEXUS currently control system volume?**

**No.** NEXUS has a `VolumeMute` intent variant in `command_executor.rs`
with platform-specific mute implementations (PowerShell `keybd_event` on
Windows, `osascript` on macOS, `wpctl`/`pactl`/`amixer` on Linux), but:

1. The **intent parser never routes to it** — neither
   `intent_parser.rs` (Rust deterministic) nor `parser.ts` (frontend)
   recognizes "mute", "volume", "set volume to 50", "increase volume",
   or any volume-related phrase.
2. There is **no `set_volume` or `get_volume` intent** — only `volume_mute`.
3. There is **no volume-up / volume-down** intent.
4. The Windows mute implementation uses `keybd_event(0xAD)` (the mute
   virtual-key), which **toggles** mute rather than setting it
   deterministically — it's a hack, not a real API call.
5. The macOS implementation uses `osascript -e "set volume with output
   muted"` which works but **requires AppleScript permission** (TCC
   prompt) and is slow (~200-500ms per call).
6. The Linux implementation tries `wpctl` → `pactl` → `amixer` in
   sequence, which is correct but only does mute toggle, not volume
   set/get.

**Can it be done properly on all three platforms?**

**Yes.** Each OS has a native API that works without admin/root:

| OS | API | Admin required? | Sandbox OK? | Rust crate |
|----|-----|-----------------|-------------|------------|
| Windows | Core Audio `IAudioEndpointVolume` (COM) | No | N/A (no sandbox) | `windows` crate (already in deps) |
| macOS | CoreAudio `AudioObjectSetPropertyData` | No | Yes (no entitlement needed for output volume) | `coreaudio-rs` or FFI |
| Linux | PipeWire `wpctl` / PulseAudio `pactl` / ALSA `amixer` | No (user must be in `audio` group or use session bus) | N/A | Shell commands or `libpulse-binding` |

---

## Part 1 — Windows

### 1.1 The API: Core Audio Endpoint Volume

Windows exposes system volume control through the **Core Audio API**
(WASAPI), specifically the `IAudioEndpointVolume` interface.

**API hierarchy:**
```
MMDeviceEnumerator (COM class)
  └── IMMDeviceEnumerator
       └── GetDefaultAudioEndpoint(eRender, eMultimedia) → IMMDevice
            └── Activate(IID_IAudioEndpointVolume) → IAudioEndpointVolume
                 ├── GetMasterVolumeLevelScalar() → f32 (0.0 to 1.0)
                 ├── SetMasterVolumeLevelScalar(f32) → ()
                 ├── GetMute() → BOOL
                 ├── SetMute(BOOL) → ()
                 ├── VolumeStepUp() → ()  (one notch)
                 ├── VolumeStepDown() → ()  (one notch)
                 ├── GetVolumeRange() → (minDB, maxDB, stepDB)
                 └── RegisterControlChangeNotify(callback) → ()
```

**Key methods:**

| Method | Purpose | Units |
|--------|---------|-------|
| `GetMasterVolumeLevelScalar` | Get current volume | Normalized 0.0–1.0 |
| `SetMasterVolumeLevelScalar` | Set volume | Normalized 0.0–1.0 |
| `GetMasterVolumeLevel` | Get current volume | Decibels (float) |
| `SetMasterVolumeLevel` | Set volume | Decibels (float) |
| `GetMute` | Get mute state | BOOL |
| `SetMute` | Set mute state | BOOL |
| `VolumeStepUp` | Increase by one notch | — |
| `VolumeStepDown` | Decrease by one notch | — |

**Why `Scalar` over `Level` (decibels):**
The scalar methods (`SetMasterVolumeLevelScalar`) use a **normalized
0.0–1.0 range** with audio-tapered perception — each slider position
produces a perceptually-uniform loudness change. The decibel methods
require knowing the device's dB range (via `GetVolumeRange`) and are
overkill for a voice assistant. **Use scalar.**

### 1.2 Permissions

**No admin privileges required.** The Core Audio API is available to
all user-mode processes. The COM call chain
(`CoCreateInstance(MMDeviceEnumerator)` → `IMMDeviceEnumerator` →
`IMMDevice` → `IAudioEndpointVolume`) works with standard user
permissions.

**No special manifest or UAC elevation needed.** Windows does not gate
audio endpoint volume behind any permission dialog. Any application can
read and write the master volume at any time.

**Windows Sandbox:** Windows doesn't have a macOS-style app sandbox. UWP
apps have capability declarations but NEXUS is a Win32 desktop app, not
UWP. No capability needed.

**Caveat from Microsoft docs:**
> "A client of IAudioEndpointVolume must take care to avoid the
> potentially disruptive effects on other audio applications of altering
> the master volume levels of audio endpoint devices. Typically, the
> user has exclusive control over the master volume levels through the
> Windows volume-control program, Sndvol.exe."

This is a UX warning, not a permission restriction. NEXUS changing the
volume on voice command is exactly the intended use case.

### 1.3 Rust implementation

The `windows` crate (v0.36, already in `Cargo.toml`) includes the
`Win32_Media_Audio` feature which provides `IAudioEndpointVolume`,
`IMMDeviceEnumerator`, `MMDeviceEnumerator`, etc.

NEXUS already uses these APIs in `meeting_detect.rs`:
```rust
use windows::Win32::Media::Audio::{
    IAudioSessionEnumerator, IAudioSessionManager2, IMMDeviceEnumerator,
    MMDeviceEnumerator,
};
```

**Example implementation pattern:**
```rust
use windows::Win32::Media::Audio::{
    eRender, eMultimedia, Endpoints::IAudioEndpointVolume,
    IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoCreateInstance, CoUninitialize,
    CLSCTX_ALL, COINIT_MULTITHREADED,
};

fn get_endpoint_volume() -> Result<f32, String> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok();
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("CoCreateInstance: {e}"))?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|e| format!("GetDefaultAudioEndpoint: {e}"))?;
        let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)
            .map_err(|e| format!("Activate: {e}"))?;
        let level = volume.GetMasterVolumeLevelScalar()
            .map_err(|e| format!("GetMasterVolumeLevelScalar: {e}"))?;
        Ok(level)
    }
}

fn set_endpoint_volume(level: f32) -> Result<(), String> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok();
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("CoCreateInstance: {e}"))?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|e| format!("GetDefaultAudioEndpoint: {e}"))?;
        let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)
            .map_err(|e| format!("Activate: {e}"))?;
        volume.SetMasterVolumeLevelScalar(level, &windows::core::GUID::zeroed())
            .map_err(|e| format!("SetMasterVolumeLevelScalar: {e}"))?;
        Ok(())
    }
}
```

**Note on COM threading:** `CoInitializeEx` with `COINIT_MULTITHREADED`
is safe for short-lived calls. NEXUS already uses this pattern in
`meeting_detect.rs`. For repeated calls, initialize COM once per thread.

**Note on the `windows` crate version:** NEXUS uses `windows = "0.36"`.
The `IAudioEndpointVolume` interface is in the `Endpoints` submodule:
`windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume`. The
exact import path may differ slightly between crate versions — verify
with `cargo doc`.

### 1.4 What the current NEXUS code does (and why it's wrong)

The current `volume_mute()` in `command_executor.rs`:
```rust
#[cfg(target_os = "windows")]
{
    let ps = "$signature = '[DllImport(\"user32.dll\")] ...';
              $key = Add-Type ...;
              $key::keybd_event(0xAD, 0, 0, 0);
              $key::keybd_event(0xAD, 0, 2, 0)";
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-Command", ps])
        .creation_flags(0x08000000)
        .spawn();
}
```

**Problems:**
1. **Spawns PowerShell** — ~200-500ms latency, ~20MB RAM for the
   `powershell.exe` process.
2. **`keybd_event(0xAD)` is the VK_VOLUME_MUTE key** — it **toggles**
   mute, it doesn't set it deterministically. If the system is already
   muted, this **unmutes** it.
3. **Fire-and-forget** — `spawn()` not `status()`, so we don't know if
   it succeeded.
4. **No volume set/get** — only mute toggle.
5. **The first `Media.SoundPlayer().PlaySync()` call** is leftover
   debug code that plays a system sound — it should be removed.

**The fix:** Replace with direct `IAudioEndpointVolume::SetMute(TRUE)`
COM call. No subprocess, no PowerShell, ~1ms latency, deterministic.

### 1.5 Alternative: `nircmd.exe`

Some apps bundle NirSoft's `nircmd.exe` for volume control:
```
nircmd.exe mutesysvolume 0    # unmute
nircmd.exe mutesysvolume 1    # mute
nircmd.exe mutesysvolume 2    # toggle
nircmd.exe setsysvolume 32768 # set to 50%
```

**Verdict:** Don't use. It requires bundling a third-party executable,
and the COM API is already available via the `windows` crate. `nircmd`
is useful for scripts, not for a Rust application.

---

## Part 2 — macOS

### 2.1 The API: CoreAudio AudioObjectSetPropertyData

macOS exposes system volume through the **CoreAudio framework**. The
key function is `AudioObjectSetPropertyData` with the
`kAudioDevicePropertyVolumeScalar` or
`kAudioHardwareServiceDeviceProperty_VirtualMasterVolume` property
selector.

**API hierarchy:**
```
AudioObjectGetPropertyData(kAudioObjectSystemObject,
    kAudioHardwarePropertyDefaultOutputDevice)
  → AudioDeviceID (the default output device)

AudioObjectSetPropertyData(deviceID,
    kAudioHardwareServiceDeviceProperty_VirtualMasterVolume,
    scope=output, element=master)
  → Float32 volume (0.0 to 1.0)

AudioObjectSetPropertyData(deviceID,
    kAudioDevicePropertyMute,
    scope=output, element=master)
  → UInt32 (0 = unmuted, 1 = muted)
```

**Key properties:**

| Property | Purpose | Type |
|----------|---------|------|
| `kAudioHardwareServiceDeviceProperty_VirtualMasterVolume` | Get/set master volume | Float32 (0.0–1.0) |
| `kAudioDevicePropertyVolumeScalar` | Get/set per-channel volume | Float32 (0.0–1.0) |
| `kAudioDevicePropertyMute` | Get/set mute state | UInt32 (0 or 1) |
| `kAudioHardwarePropertyDefaultOutputDevice` | Get default output device | AudioDeviceID |

**Why `VirtualMasterVolume` over `VolumeScalar`:**
`VirtualMasterVolume` operates on the **master channel** (element 0) and
is the preferred API for volume slider apps. `VolumeScalar` on channel 0
(master) returns `'who?'` (error) on some devices — you have to set
channels 1 (left) and 2 (right) individually. `VirtualMasterVolume`
handles this internally.

**Note:** On macOS 15+ (Sequoia), there's a new higher-level API:
```swift
let system = AudioHardwareSystem.shared
try system.setOutputDeviceVolume(deviceID, volume)
```
But this is Swift-only and not available via C FFI. For Rust, use the
classic `AudioObjectSetPropertyData` API which still works on all macOS
versions.

### 2.2 Permissions

**No special entitlement needed for OUTPUT volume.** CoreAudio property
reads and writes work fine in the sandbox without any entitlement. It
is hardware control, not file or network access, so the sandbox does
not gate it.

**Confirmed by ShowMode's sandbox research:**
> "Sandbox status: CoreAudio property reads and writes work fine in the
> sandbox without any entitlement. It is hardware control, not file or
> network access, so the sandbox does not gate it."

**Microphone INPUT** requires `com.apple.security.device.audio-input`
entitlement + `NSMicrophoneUsageDescription` in Info.plist, but
**speaker OUTPUT volume** does NOT require any entitlement.

**AppleScript (`osascript`) alternative:**
The current NEXUS code uses `osascript -e "set volume with output
muted"`. This works but:
1. Requires **Automation permission** (TCC prompt: "NEXUS wants to
   control System Events")
2. Is slow (~200-500ms per call — spawns `osascript` process)
3. `set volume with output muted` only mutes; `set volume X` (0-100)
   sets volume but requires a separate AppleScript call
4. The TCC prompt appears on first use and can be revoked in System
   Settings → Privacy & Security → Automation

**Direct CoreAudio calls bypass TCC entirely** for output volume — no
prompt, no permission, no revocation risk. This is strictly better than
the AppleScript approach.

### 2.3 Rust implementation

There are two approaches:

**Option A: `coreaudio-rs` crate**
```toml
[target.'cfg(target_os = "macos")'.dependencies]
coreaudio-rs = "0.4"
```
```rust
use coreaudio_rs::audio_unit::{AudioUnit, IOType};

let audio_unit = AudioUnit::new(IOType::DefaultOutput)?;
audio_unit.set_output_volume(0.5)?;
```
**Problem:** `coreaudio-rs` focuses on audio streams, not device
volume. The `set_output_volume` method may not exist or may only
control per-stream volume, not system master volume.

**Option B: Direct FFI to CoreAudio C API**
```rust
#[repr(C)]
struct AudioObjectPropertyAddress {
    mSelector: u32,
    mScope: u32,
    mElement: u32,
}

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

const kAudioObjectSystemObject: u32 = 1;
const kAudioHardwarePropertyDefaultOutputDevice: u32 = /* 'dOut' */;
const kAudioHardwareServiceDeviceProperty_VirtualMasterVolume: u32 = /* 'vmvc' */;
const kAudioDevicePropertyScopeOutput: u32 = /* 'outp' */;
const kAudioObjectPropertyElementMaster: u32 = 0;

fn set_macos_volume(volume: f32) -> Result<(), String> {
    unsafe {
        // 1. Get default output device
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

        // 2. Set volume
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

**The four-char constants** (`'dOut'`, `'vmvc'`, `'outp'`) are
FourCC codes defined in CoreAudio headers. In Rust:
```rust
const kAudioHardwarePropertyDefaultOutputDevice: u32 =
    u32::from_be_bytes(*b"dOut");
const kAudioHardwareServiceDeviceProperty_VirtualMasterVolume: u32 =
    u32::from_be_bytes(*b"vmvc");
const kAudioDevicePropertyScopeOutput: u32 =
    u32::from_be_bytes(*b"outp");
const kAudioObjectPropertyScopeGlobal: u32 =
    u32::from_be_bytes(*b"glob");
```

**Option C: Keep `osascript` as fallback**
For simplicity, the current `osascript` approach can be kept as a
fallback. But replace the TCC-gated `set volume` with direct CoreAudio
FFI for production use.

### 2.4 Mute on macOS

Mute uses `kAudioDevicePropertyMute` (FourCC: `'mute'`):
```rust
let addr = AudioObjectPropertyAddress {
    mSelector: kAudioDevicePropertyMute,
    mScope: kAudioDevicePropertyScopeOutput,
    mElement: kAudioObjectPropertyElementMaster,
};
let mute_val: u32 = 1; // 1 = muted, 0 = unmuted
AudioObjectSetPropertyData(device_id, &addr, 0, ptr::null(),
    size_of::<u32>() as u32, &mute_val as *const _ as *const _);
```

**Caveat:** Not every output device exposes a settable mute property on
the master channel. Built-in MacBook speakers and AirPods do. Some HDMI
outputs and USB DACs don't. The code should check
`AudioObjectIsPropertySettable` first, and fall back to setting volume
to 0.0 as a "soft mute" if hardware mute isn't available.

---

## Part 3 — Linux

### 3.1 The API landscape

Linux audio has **three layers**, each with its own volume control API:

```
┌─────────────────────────────────────────────┐
│  Application (NEXUS)                        │
├─────────────────────────────────────────────┤
│  Layer 3: PipeWire (modern, default on      │
│           Fedora 34+, Ubuntu 22.04+, etc.)  │
│  Tool: wpctl (WirePlumber CLI)              │
│  API:  PipeWire native / pw-cli             │
├─────────────────────────────────────────────┤
│  Layer 2: PulseAudio (legacy, still common) │
│  Tool: pactl                                │
│  API:  libpulse (C) / pulseaudio D-Bus      │
├─────────────────────────────────────────────┤
│  Layer 1: ALSA (kernel-level, always present)│
│  Tool: amixer                               │
│  API:  libasound (alsa-lib)                 │
└─────────────────────────────────────────────┘
```

**Detection strategy:** Try `wpctl` first (PipeWire), fall back to
`pactl` (PulseAudio), fall back to `amixer` (ALSA direct). This is
what the current NEXUS mute code already does — and it's correct.

### 3.2 PipeWire / WirePlumber (`wpctl`)

**PipeWire** is the modern audio server on Linux, replacing PulseAudio.
**WirePlumber** is its session manager. The `wpctl` CLI tool controls
volume, mute, and default device selection.

**Commands:**
```bash
# Get default sink ID
wpctl status | grep "Default Audio Sink"  # or use @DEFAULT_SINK@

# Set volume to 50%
wpctl set-volume @DEFAULT_SINK@ 0.5

# Set volume to 75%
wpctl set-volume @DEFAULT_SINK@ 75%

# Increase volume by 10%
wpctl set-volume @DEFAULT_SINK@ 10%+

# Decrease volume by 10%
wpctl set-volume @DEFAULT_SINK@ 10%-

# Mute
wpctl set-mute @DEFAULT_SINK@ 1

# Unmute
wpctl set-mute @DEFAULT_SINK@ 0

# Toggle mute
wpctl set-mute @DEFAULT_SINK@ toggle

# Get current volume (parse from status output)
wpctl get-volume @DEFAULT_SINK@
# Output: "Volume: 0.50" or "Volume: 0.50 [MUTED]"
```

**Special identifiers:**
- `@DEFAULT_SINK@` — default output device (speakers/headphones)
- `@DEFAULT_SOURCE@` — default input device (microphone)
- Numeric ID from `wpctl status` — specific device

**Permissions:**
- **No root required.** `wpctl` talks to the user's PipeWire daemon
  via a Unix socket (`$XDG_RUNTIME_DIR/pipewire-0`).
- **User must be in the `audio` group** on some distributions (those
  that use ALSA group-based access control). On modern distros with
  ConsoleKit/logind, the active session user gets automatic access.
- **PipeWire uses a Polkit-like security model** for Flatpak apps, but
  native (non-sandboxed) apps like NEXUS have full access.
- **WirePlumber client access control:** WirePlumber inspects each
  client and assigns permissions. Native apps get full `rwx`
  permissions by default. Only Flatpak/sandboxed apps get restricted
  permissions.

### 3.3 PulseAudio (`pactl`)

**PulseAudio** is the legacy audio server, still present on many
distributions (or running as a compatibility layer on top of PipeWire
via `pipewire-pulse`).

**Commands:**
```bash
# Set volume to 50%
pactl set-sink-volume @DEFAULT_SINK@ 50%

# Set volume to 80% (can exceed 100% — PulseAudio allows boost)
pactl set-sink-volume @DEFAULT_SINK@ 80%

# Increase by 5%
pactl set-sink-volume @DEFAULT_SINK@ +5%

# Decrease by 5%
pactl set-sink-volume @DEFAULT_SINK@ -5%

# Mute
pactl set-sink-mute @DEFAULT_SINK@ 1

# Unmute
pactl set-sink-mute @DEFAULT_SINK@ 0

# Toggle mute
pactl set-sink-mute @DEFAULT_SINK@ toggle

# Get current volume
pactl get-sink-volume @DEFAULT_SINK@
# Output: "Volume: front-left: 50% / ... front-right: 50% / ..."
```

**Permissions:**
- **No root required.** `pactl` connects to the user's PulseAudio
  daemon via `$XDG_RUNTIME_DIR/pulse/native`.
- **Connection failure** (`Connection refused`) happens when:
  - PulseAudio daemon isn't running (check `pulseaudio --check`)
  - User isn't in the `audio` group (on group-based distros)
  - `PULSE_SERVER` env var points to wrong socket
  - Running as root (PulseAudio refuses root by default — use
    `PULSE_RUNTIME_PATH=/run/user/1000/pulse`)

**Volume > 100%:** PulseAudio allows volumes above 100% (up to 65536/65535
= ~100% in raw values, but `set-sink-volume` accepts percentages up to
`65536` which is ~100% and beyond). This is "boost" / "pre-amp" and can
distort audio. NEXUS should cap at 100% to avoid distortion.

### 3.4 ALSA (`amixer`)

**ALSA** is the kernel-level audio layer. When PipeWire and PulseAudio
are both unavailable, `amixer` talks directly to the ALSA mixer
interface.

**Commands:**
```bash
# Set Master volume to 50%
amixer -q set Master 50%

# Set Master volume to 80%
amixer -q set Master 80%

# Increase by 10%
amixer -q set Master 10%+

# Mute
amixer -q set Master mute

# Unmute
amixer -q set Master unmute

# Toggle mute
amixer -q set Master toggle

# Get current volume
amixer get Master
# Output: "  Front Left: Playback 50% [...] [-20.00dB] [...]"
```

**Permissions:**
- **User must have read/write access to `/dev/snd/controlC*`** device
  nodes.
- On Debian/Ubuntu: group `audio`, permissions `crw-rw---- root audio`
- On Fedora/Arch with logind: active session user gets ACL access via
  `systemd-logind` + `udev` rules
- **Root is NOT required** but the user must be in the `audio` group
  on distributions that use group-based access control.

**Limitations:**
- ALSA directly controls hardware mixer levels. On modern systems with
  PipeWire/PulseAudio, the ALSA "Master" control may not reflect the
  actual software volume that the user sees in their desktop volume
  slider.
- ALSA doesn't support per-application volume — only hardware-level
  master volume.
- Some USB DACs don't have a "Master" control — you need to find the
  correct control name via `amixer controls`.

### 3.5 Rust implementation for Linux

**Option A: Shell commands (current approach, recommended)**
```rust
fn set_linux_volume(volume: f32) -> Result<(), String> {
    let pct = (volume * 100.0).round() as i32;
    // Try PipeWire first, then PulseAudio, then ALSA
    Command::new("wpctl")
        .args(["set-volume", "@DEFAULT_SINK@", &format!("{pct}%")])
        .creation_flags(0x08000000)
        .status()
        .or_else(|_| Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &format!("{pct}%")])
            .status())
        .or_else(|_| Command::new("amixer")
            .args(["-q", "set", "Master", &format!("{pct}%")])
            .status())
        .map_err(|e| format!("All volume tools failed: {e}"))?;
    Ok(())
}
```

**Option B: `libpulse-binding` crate**
```toml
[target.'cfg(target_os = "linux")'.dependencies]
libpulse-binding = "2.0"
```
```rust
use libpulse_binding::context::Context;
use libpulse_binding::volume::{Volume, VolumeDB};

let mut ctx = Context::new("nexus", None);
// ... connect, get sink, set volume ...
```
**Problem:** PulseAudio-only, doesn't work with pure PipeWire (unless
`pipewire-pulse` is installed). Adds a native dependency. Shell commands
are simpler and more portable.

**Option C: `pipewire` crate**
```toml
[target.'cfg(target_os = "linux")'.dependencies]
pipewire = "0.8"
```
**Problem:** PipeWire-only, doesn't work with PulseAudio or ALSA-only
systems. The `pipewire` crate is also unstable and has a steep learning
curve. Shell commands are the pragmatic choice.

**Recommendation: Stick with shell commands** (`wpctl` → `pactl` → `amixer`).
They're fast (~10-50ms), require no native dependencies, and handle all
three Linux audio stacks. The current NEXUS code already uses this
pattern for mute — just extend it to volume set/get.

---

## Part 4 — Cross-Platform Architecture for NEXUS

### 4.1 Proposed intent types

```typescript
// frontend/src/intent/parser.ts
| { action: "volume_set"; level: number }      // 0-100
| { action: "volume_get" }                      // returns current level
| { action: "volume_mute" }                     // mute
| { action: "volume_unmute" }                   // unmute
| { action: "volume_up"; steps?: number }       // increase by N notches
| { action: "volume_down"; steps?: number }     // decrease by N notches
```

**Note:** The current code only has `volume_mute` (toggle). We need
separate `volume_mute` and `volume_unmute` intents because "mute" should
always mute (even if already muted) and "unmute" should always unmute.
The toggle behavior is confusing for voice commands — if the user says
"mute" and the system is already muted, they don't want it to unmute.

### 4.2 Proposed parser patterns

```typescript
// "set volume to 50" / "volume 50" / "set volume to 50 percent"
const volSetMatch = text.match(
    /^(?:set\s+)?(?:the\s+)?(?:volume|vol)\s+(?:to\s+)?(\d{1,3})(?:\s*(?:%|percent))?$/i
);
if (volSetMatch) {
    const level = Math.min(100, Math.max(0, parseInt(volSetMatch[1])));
    return { action: "volume_set", level };
}

// "mute" / "mute the volume" / "mute audio"
if (/^(?:mute|silence)(?:\s+(?:the\s+)?(?:volume|audio|sound))?$/i.test(text)) {
    return { action: "volume_mute" };
}

// "unmute" / "unmute the volume"
if (/^unmute(?:\s+(?:the\s+)?(?:volume|audio|sound))?$/i.test(text)) {
    return { action: "volume_unmute" };
}

// "volume up" / "increase volume" / "louder" / "turn it up"
if (/^(?:volume\s+up|increase\s+(?:the\s+)?(?:volume|vol)|turn\s+(?:it\s+)?up|louder)(?:\s+(?:by\s+)?(\d{1,2})(?:%|percent)?)?$/i.test(text)) {
    const m = text.match(/(\d{1,2})/);
    return { action: "volume_up", steps: m ? parseInt(m[1]) : 10 };
}

// "volume down" / "decrease volume" / "quieter" / "turn it down"
if (/^(?:volume\s+down|decrease\s+(?:the\s+)?(?:volume|vol)|turn\s+(?:it\s+)?down|quieter|softer)(?:\s+(?:by\s+)?(\d{1,2})(?:%|percent)?)?$/i.test(text)) {
    const m = text.match(/(\d{1,2})/);
    return { action: "volume_down", steps: m ? parseInt(m[1]) : 10 };
}
```

### 4.3 Proposed Rust executor

```rust
// src-tauri/src/command_executor.rs

#[serde(rename = "volume_set")]
VolumeSet { level: u8 },      // 0-100
#[serde(rename = "volume_get")]
VolumeGet,
#[serde(rename = "volume_mute")]
VolumeMute,
#[serde(rename = "volume_unmute")]
VolumeUnmute,
#[serde(rename = "volume_up")]
VolumeUp { steps: Option<u8> },
#[serde(rename = "volume_down")]
VolumeDown { steps: Option<u8> },
```

**Platform dispatch:**
```rust
fn volume_set(level: u8) -> Result<CommandResult, String> {
    let level = level.min(100);
    let scalar = level as f32 / 100.0;

    #[cfg(target_os = "windows")]
    {
        windows_set_volume(scalar)?;
    }
    #[cfg(target_os = "macos")]
    {
        macos_set_volume(scalar)?;
    }
    #[cfg(target_os = "linux")]
    {
        linux_set_volume(level)?;
    }

    Ok(CommandResult {
        success: true,
        message: format!("Volume set to {level}%, sir."),
    })
}
```

### 4.4 Latency comparison

| Platform | Current (subprocess) | Proposed (native API) | Speedup |
|----------|---------------------|----------------------|---------|
| Windows | PowerShell `keybd_event` (~300ms) | COM `IAudioEndpointVolume` (~1ms) | 300x |
| macOS | `osascript` (~300ms) | CoreAudio FFI (~1ms) | 300x |
| Linux | `wpctl`/`pactl`/`amixer` (~30ms) | Same (shell is already fast) | 1x |

Linux shell commands are already fast enough (~30ms). Windows and macOS
benefit massively from native API calls — from ~300ms to ~1ms.

### 4.5 Volume change notifications

If NEXUS wants to display the current volume in the sidebar (e.g. "Volume
is at 50%, sir"), it needs to read the current volume:

**Windows:** `IAudioEndpointVolume::GetMasterVolumeLevelScalar()` → f32
**macOS:** `AudioObjectGetPropertyData(kAudioHardwareServiceDeviceProperty_VirtualMasterVolume)` → Float32
**Linux:** `wpctl get-volume @DEFAULT_SINK@` → parse "Volume: 0.50"

For real-time volume change notifications (e.g. the user changes volume
with their keyboard and NEXUS should reflect it):

**Windows:** `IAudioEndpointVolume::RegisterControlChangeNotify(callback)`
— registers a COM callback that fires on any volume change.

**macOS:** `AudioObjectAddPropertyListener` with a property address —
installs a callback that fires on volume changes.

**Linux:** PipeWire/PulseAudio events via D-Bus or `pactl subscribe` —
emits events when sink volumes change.

**For NEXUS's use case**, real-time notifications are unnecessary —
NEXUS only needs to read the volume when the user asks "what's the
volume?" or after setting it to confirm. A simple `get_volume()` call
is sufficient.

---

## Part 5 — Summary of Findings

### 5.1 Can NEXUS control volume on all platforms?

| Platform | Mute | Set Volume | Get Volume | Up/Down | No admin? | No extra deps? |
|----------|------|-----------|-----------|---------|-----------|----------------|
| Windows | Yes (COM) | Yes (COM) | Yes (COM) | Yes (COM) | Yes | Yes (`windows` crate already in deps) |
| macOS | Yes (CoreAudio) | Yes (CoreAudio) | Yes (CoreAudio) | Yes (CoreAudio) | Yes (no entitlement for output) | Yes (FFI to system framework) |
| Linux | Yes (wpctl/pactl/amixer) | Yes (wpctl/pactl/amixer) | Yes (wpctl/pactl/amixer) | Yes (wpctl/pactl/amixer) | Yes (user group or logind) | Yes (shell commands) |

### 5.2 What's broken in the current NEXUS code

1. **No volume intent parsing** — the parser doesn't recognize any
   volume commands. `VolumeMute` exists in the Rust enum but is
   unreachable.
2. **Mute toggles instead of setting** — `keybd_event(0xAD)` toggles
   mute on Windows. If already muted, "mute" unmutes.
3. **No `set_volume` / `get_volume` / `volume_up` / `volume_down`** —
   only `volume_mute`.
4. **Windows uses PowerShell subprocess** — 300ms latency, 20MB RAM
   overhead, fire-and-forget (no error checking).
5. **macOS uses AppleScript** — requires TCC Automation permission,
   300ms latency. Direct CoreAudio calls need no permission and are
   300x faster.
6. **Linux is correct** — the `wpctl` → `pactl` → `amixer` fallback
   chain is the right approach. Just needs to be extended beyond mute.

### 5.3 Recommended implementation plan

1. **Add volume intents to `parser.ts`** — `volume_set`, `volume_get`,
   `volume_mute`, `volume_unmute`, `volume_up`, `volume_down`.
2. **Add volume intents to `command_executor.rs`** — extend the enum
   and add platform-specific implementations.
3. **Windows: use `IAudioEndpointVolume` COM API** — already available
   via the `windows` crate. No subprocess, no PowerShell, ~1ms latency.
4. **macOS: use CoreAudio FFI** — direct `AudioObjectSetPropertyData`
   calls. No `osascript`, no TCC prompt, ~1ms latency.
5. **Linux: keep shell commands** — `wpctl`/`pactl`/`amixer` are
   already fast (~30ms) and handle all three audio stacks.
6. **Separate mute and unmute** — don't toggle. "Mute" always mutes,
   "unmute" always unmutes.
7. **Cap volume at 100%** — prevent distortion from >100% boost.
8. **Return confirmation** — "Volume set to 50%, sir." with the actual
   achieved level (read back to confirm).

### 5.4 Edge cases to handle

| Case | Windows | macOS | Linux |
|------|---------|-------|-------|
| No audio device | `GetDefaultAudioEndpoint` returns error | `AudioObjectGetPropertyData` returns non-zero | `wpctl`/`pactl`/`amixer` exit non-zero |
| Device doesn't support volume (HDMI passthrough) | `SetMasterVolumeLevelScalar` returns error | `AudioObjectIsPropertySettable` returns false | `amixer` has no "Master" control |
| Multiple audio devices | Use default endpoint (eRender, eMultimedia) | Use `kAudioHardwarePropertyDefaultOutputDevice` | Use `@DEFAULT_SINK@` |
| Bluetooth headphones disconnected mid-call | COM call fails — catch and report | CoreAudio call fails — catch and report | `wpctl`/`pactl` fails — catch and report |
| Volume > 100% (boost) | `SetMasterVolumeLevelScalar` accepts >1.0 but may clip | CoreAudio accepts >1.0 but may clip | `pactl` allows >100% but `wpctl` caps at 1.0 by default |
| User in `audio` group (Linux) | N/A | N/A | Required on some distros. Check `id` output. |
| PipeWire not running (Linux) | N/A | N/A | `wpctl` fails → fall back to `pactl` → fall back to `amixer` |

---

## References

### Windows
- [IAudioEndpointVolume interface](https://learn.microsoft.com/en-us/windows/win32/api/endpointvolume/nn-endpointvolume-iaudioendpointvolume)
- [SetMasterVolumeLevelScalar method](https://learn.microsoft.com/en-us/windows/win32/api/endpointvolume/nf-endpointvolume-iaudioendpointvolume-setmastervolumelevelscalar)
- [EndpointVolume API](https://learn.microsoft.com/en-us/windows/win32/coreaudio/endpointvolume-api)
- [Audio-tapered volume controls](https://learn.microsoft.com/en-us/windows/win32/coreaudio/audio-tapered-volume-controls)
- [windows crate Rust docs](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Media/Audio/Endpoints/struct.IAudioEndpointVolume.html)

### macOS
- [Technical Q&A QA1016: Changing the volume of audio devices](https://developer.apple.com/library/archive/qa/qa1016/_index.html)
- [Sandboxing a Mac system utility: four APIs, four trade-offs](https://getshowmode.com/blog/sandboxing-a-system-utility/)
- [Change OS X system volume programmatically (StackOverflow)](https://stackoverflow.com/questions/17715111/change-os-x-system-volume-programmatically)
- [macOS permissions: TCC, hardened runtime, entitlements](https://github.com/djmunro/hush/blob/main/docs/macos-permissions.md)
- [Enabling App Sandbox entitlements](https://developer.apple.com/library/archive/documentation/Miscellaneous/Reference/EntitlementKeyReference/Chapters/EnablingAppSandbox.html)

### Linux
- [wpctl(1) — WirePlumber documentation](https://pipewire.pages.freedesktop.org/wireplumber/tools/wpctl.html)
- [PipeWire — ArchWiki](https://wiki.archlinux.org/title/PipeWire)
- [PulseAudio D-Bus Interface](https://wiki.freedesktop.org/www/Software/PulseAudio/Documentation/Developer/Clients/DBus/)
- [ALSA control interface](https://www.alsa-project.org/alsa-doc/alsa-lib/control.html)
- [amixer man page](https://man.archlinux.org/man/amixer.1.en.txt)
- [WirePlumber client access control](https://pipewire.pages.freedesktop.org/wireplumber/policies/client_access.html)
- [PulseAudio Perfect Setup — audio group](https://www.freedesktop.org/wiki/Software/PulseAudio/Documentation/User/PerfectSetup/)
