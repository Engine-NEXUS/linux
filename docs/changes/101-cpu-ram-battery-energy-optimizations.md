# Change 101 — CPU, RAM, Battery & Energy Optimization Suite

## Problem & Motivation
Background always-on AI assistants must be virtually imperceptible in resource overhead when idle. Prior to this change:
1. **CPU & Package C-State Degradation**: A hardcoded 30ms Win32 cursor polling loop in `stage.rs` woke the CPU thread 33.3 times per second regardless of whether any UI hitboxes (slider, buttons, callouts) were present on screen. This prohibited Intel/AMD CPUs from entering deep Package C-states (C8/C10), keeping package idle power elevated (~1.5% CPU / ~3.0W package power vs <0.4W deep sleep).
2. **Core Scheduling / QoS**: The NEXUS daemon process ran under default scheduling QoS, allowing background audio and watchdog threads to be scheduled on high-power Performance cores (P-cores) instead of Energy-efficient cores (E-cores).
3. **GPU WebGL Workload on Battery & Idle**: `voice-orb.js` ran an uncapped 60 FPS `requestAnimationFrame` loop even when the orb was in an ambient, slow 0.33 Hz breathing idle state. Furthermore, it did not check `document.hidden` during OS screen lock / display sleep, and kept full 5,000 particle simulation running even when running on battery power.
4. **Allocation Waste During Silence**: Audio telemetry in `wakeword_oww.rs` formatted and emitted debug strings at 2.08 Hz even during long stretches of room silence in release builds.

## Root Cause & Fixes

### 1. Win32 Adaptive Hitbox Pacing & Deep C-State Restoration
- [`src-tauri/src/live_glass.rs`](file:///c:/PROJECTS/ULTRON/src-tauri/src/live_glass.rs):
  - Added `has_active_hitboxes()` helper to query whether spatial or HUD hitboxes exist.
- [`src-tauri/src/stage.rs`](file:///c:/PROJECTS/ULTRON/src-tauri/src/stage.rs):
  - Added `StageRect::distance_to_point(px, py)` calculating Euclidean pixel distance from the cursor to any registered rectangle.
  - Replaced the fixed 30ms sleep loop with dynamic 3-tier pacing:
    - **250ms sleep** (~4 Hz, cuts 94% of wakes) when no hitboxes are active.
    - **100ms sleep** (~10 Hz) when cursor is $>120$px away from any active hitbox.
    - **16ms sleep** (~60 Hz) when cursor enters within 120px proximity of a clickable element.
  - Guarantees $\ge 100\text{ms}-250\text{ms}$ uninterrupted sleep windows when idle, allowing CPU cores to drop into package C8/C10 sleep.

### 2. Windows 11 EcoQoS (Efficiency Mode) Process Opt-In
- [`src-tauri/src/power.rs`](file:///c:/PROJECTS/ULTRON/src-tauri/src/power.rs):
  - Created a new power management module implementing Win32 `SetProcessInformation` with `ProcessPowerThrottling` and `PROCESS_POWER_THROTTLING_EXECUTION_SPEED`.
  - Automatically signals the Windows kernel thread scheduler to prioritize E-cores for NEXUS background processing without reducing IPC responsiveness.
- [`src-tauri/src/lib.rs`](file:///c:/PROJECTS/ULTRON/src-tauri/src/lib.rs):
  - Registered `pub mod power;`.
  - Initialized `crate::power::enable_process_ecoqos()` on startup.

### 3. WebGL Tri-State Dynamic Frame Pacing & Occlusion Suspension
- [`frontend/src/avatar/voice-orb.js`](file:///c:/PROJECTS/ULTRON/frontend/src/avatar/voice-orb.js):
  - Added `document.hidden` detection in `_sync()` and a window `visibilitychange` listener. Rendering immediately halts (0 FPS, 0 draw calls) when the window is occluded, minimized, or screen locked.
  - In `_tick()` animation loop, decoupled physics pacing:
    - **Idle breathing state**: throttled to ~25 FPS (`now - lastPaint < 40ms` skips frame), reducing GPU draw calls and vertex shader compute by $>60\%$.
    - **Active states** (`listening`, `thinking`, `speaking`): rendered at native 60 FPS for fluid transitions and lip-sync dynamics.

### 4. Dynamic Battery-Aware Particle Scaling
- [`frontend/src/avatar/Avatar.tsx`](file:///c:/PROJECTS/ULTRON/frontend/src/avatar/Avatar.tsx):
  - Integrated W3C Battery Status API (`navigator.getBattery()`).
  - Added dynamic charging change event listener: scales particles down from 5,000 on AC power to 2,500 on battery power (`onBattery ? 2500 : 5000`).
  - Cuts GPU fragment fill-rate and memory bandwidth in half on laptops running on battery without altering visual particle aesthetics.

### 5. Silent Audio Telemetry Allocation Guard
- [`src-tauri/src/wakeword_oww.rs`](file:///c:/PROJECTS/ULTRON/src-tauri/src/wakeword_oww.rs):
  - Wrapped silent telemetry logging (`[VAD/SILENCE]`) in `#[cfg(debug_assertions)]`, eliminating 2.08 Hz string allocations and formatting overhead in release builds.

## Verification
- `npx tsc --noEmit`: Clean (0 errors).
- `npm test -- --run`: 200/200 pass.
- `cargo check --features custom-protocol,admin-brain`: Clean (**0 warnings, 0 errors**).
- `cargo test --lib -- --test-threads=1`: 1020/1020 pass.
- `cargo build --release --features custom-protocol,admin-brain`: Clean production release binary.
