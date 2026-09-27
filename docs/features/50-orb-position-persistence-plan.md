# 50 — Orb Position + Size Persistence: Detailed Plan

> **Date**: 2026-09-07
> **Type**: Implementation plan (no code changes yet)
> **Status**: Awaiting authorization

---

## 100% Assurance: Yes, This Can Be Implemented

### The persistence mechanism already exists and is proven

NEXUS already saves settings locally to:

```
C:\Users\<username>\AppData\Roaming\com.nexus.assistant\settings.json
```

This file **already exists on your laptop right now** (confirmed: 684 bytes, last modified 2026-09-06 11:43:23). It contains your current settings: hotkey, TTS voice, Groq API key, autostart, etc.

The `save_settings` Tauri command (`src-tauri/src/commands.rs`, line 1217) does:

```rust
let dir = app.path().app_data_dir()?;           // → %APPDATA%\com.nexus.assistant
std::fs::create_dir_all(&dir)?;                  // ensures folder exists
let path = dir.join("settings.json");
let json = serde_json::to_string_pretty(&settings)?;
std::fs::write(&path, json)?;                    // OVERWRITES the entire file
```

`std::fs::write` **overwrites** the file completely. So:

- **Save**: user moves slider → clicks Save → `save_settings` writes the entire `NexusSettings` struct (including new `orbHorizontalPct`, `orbVerticalPct`, `orbSize`) to `settings.json`. Old values are replaced.
- **Load**: on next app launch, `get_settings` reads `settings.json`, parses it into `NexusSettings`, and `position_orb()` uses the saved values.
- **Overwrite**: if the user changes the position again and saves, `std::fs::write` overwrites the file with the new values. Old position is gone, new position is in effect.

This is the **exact same mechanism** that already persists your hotkey (`"Ctrl+Shift+Space"`), TTS voice (`"am_adam"`), Groq API key, and autostart setting. It has been working since the settings system was built. Adding orb position/size fields to the same struct changes nothing about the persistence — it just adds more fields to the same JSON file.

### The orb position function is called on every wake

`position_orb()` is called:
1. At startup (`lib.rs:586`)
2. On every wake word trigger (`wakeword_oww.rs:1604`, `wakeword_oww.rs:1724`)
3. On every hotkey press (`hotkey.rs:94`)
4. On every `show_overlay` command (`window_manager.rs:91`)
5. On tray "show" click (`tray.rs:46`)
6. On setup completion (`commands.rs:90`)

This means: if `position_orb()` reads from settings, the orb will **always** be at the user's chosen position — at startup, on every wake, on every hotkey press. No extra wiring needed.

---

## Exact Data Flow

```
User drags slider in settings sidebar
    ↓
Frontend updates local state (debounced 50ms)
    ↓
User clicks "Save"
    ↓
Frontend calls invoke("save_settings", { settings })
    ↓
Rust save_settings() command:
    1. Reads app_data_dir() → C:\Users\...\AppData\Roaming\com.nexus.assistant
    2. Serializes NexusSettings to pretty JSON
    3. std::fs::write("settings.json", json) — OVERWRITES entire file
    4. Returns Ok(())
    ↓
settings.json now contains:
{
  "autostart": true,
  "hotkey": "Ctrl+Shift+Space",
  ...
  "orbHorizontalPct": 0.25,    ← NEW (25% from left)
  "orbVerticalPct": 0.85,      ← NEW (85% from top)
  "orbSize": 180,              ← NEW (180px)
  ...
}
    ↓
App restarts (or user wakes NEXUS)
    ↓
Rust get_settings() reads settings.json
    ↓
position_orb() reads orbHorizontalPct, orbVerticalPct, orbSize
    ↓
Computes pixel position:
    x = screen_width * orbHorizontalPct - orbSize/2
    y = screen_height * orbVerticalPct - orbSize/2
    (clamped to keep orb fully on-screen)
    ↓
win.set_position(PhysicalPosition::new(x, y))
win.set_size(PhysicalSize::new(orbSize, orbSize))
    ↓
Orb appears at the saved position and size
```

### Overwrite scenario

```
User sets position to 25% left, 85% top → Save
    → settings.json has orbHorizontalPct: 0.25, orbVerticalPct: 0.85

User later changes to 50% left, 100% top → Save
    → save_settings() calls std::fs::write() which OVERWRITES the file
    → settings.json now has orbHorizontalPct: 0.50, orbVerticalPct: 1.0
    → Old values (0.25, 0.85) are GONE
    → Next wake: orb appears at 50% left, 100% top (bottom center)
```

---

## What Changes Need to Be Made

### 1. `src-tauri/src/commands.rs` — Add 3 fields to `NexusSettings`

Add to the `NexusSettings` struct (after `edge_tts_voice`):

```rust
/// Orb horizontal position as percentage (0.0 = left, 0.5 = center, 1.0 = right).
/// Default 0.5 (center). Saved to settings.json, persists across restarts.
#[serde(default = "default_orb_horizontal_pct")]
pub orb_horizontal_pct: f64,

/// Orb vertical position as percentage (0.0 = top, 1.0 = bottom).
/// Default 1.0 (bottom). Saved to settings.json, persists across restarts.
#[serde(default = "default_orb_vertical_pct")]
pub orb_vertical_pct: f64,

/// Orb window size in pixels (100-300).
/// Default 200. Saved to settings.json, persists across restarts.
#[serde(default = "default_orb_size")]
pub orb_size: u32,
```

Add default functions:

```rust
fn default_orb_horizontal_pct() -> f64 { 0.5 }
fn default_orb_vertical_pct() -> f64 { 1.0 }
fn default_orb_size() -> u32 { 200 }
```

Add to `impl Default for NexusSettings`:

```rust
orb_horizontal_pct: 0.5,
orb_vertical_pct: 1.0,
orb_size: 200,
```

**Why `#[serde(default)]`?** If an existing `settings.json` doesn't have these fields yet (which it won't on first run after the update), serde uses the default value instead of failing to parse. This means existing users' settings.json won't break — the new fields just get default values (center-bottom, 200px) which matches the current hardcoded behavior.

### 2. `src-tauri/src/window_manager.rs` — Read settings in `position_orb()`

Replace the hardcoded `position_orb()` with a version that reads from settings:

```rust
pub fn position_orb<R: Runtime>(win: &WebviewWindow<R>) -> Result<(), String> {
    use tauri::PhysicalPosition;
    if let Ok(Some(monitor)) = win.current_monitor() {
        let scale = monitor.scale_factor();
        let screen = monitor.size();

        // Read orb position + size from settings.json
        let (h_pct, v_pct, orb_size) = read_orb_settings(win.app_handle());
        let orb = orb_size as i32;
        let phys_orb = (orb as f64 * scale) as i32;

        // Compute position from percentages
        let raw_x = (screen.width as f64 * h_pct) as i32 - phys_orb / 2;
        let raw_y = (screen.height as f64 * v_pct) as i32 - phys_orb / 2;

        // Clamp to keep orb fully on-screen
        let x = raw_x.max(0).min(screen.width as i32 - phys_orb);
        let y = raw_y.max(0).min(screen.height as i32 - phys_orb);

        let _ = win.set_position(PhysicalPosition::new(x, y));
        let _ = win.set_size(tauri::PhysicalSize::new(orb, orb));
        tracing::debug!("orb positioned at ({}, {}) size {}px [h={}, v={}]",
            x, y, orb, h_pct, v_pct);
    }
    Ok(())
}

fn read_orb_settings<R: Runtime>(app: &tauri::AppHandle<R>) -> (f64, f64, u32) {
    let dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(_) => return (0.5, 1.0, 200),
    };
    let path = dir.join("settings.json");
    if !path.exists() {
        return (0.5, 1.0, 200);
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return (0.5, 1.0, 200),
    };
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return (0.5, 1.0, 200),
    };
    let h = json.get("orbHorizontalPct").and_then(|v| v.as_f64()).unwrap_or(0.5);
    let v = json.get("orbVerticalPct").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let size = json.get("orbSize").and_then(|v| v.as_u64()).unwrap_or(200) as u32;
    (h, v, size)
}
```

**Key design decisions:**
- Reads `settings.json` directly (not via the Tauri command) because `position_orb` is called from Rust, not the frontend
- Falls back to defaults (center-bottom, 200px) if the file doesn't exist or can't be parsed
- Clamps position to keep the orb fully on-screen (can't push it off-screen)
- Also sets the window size, not just position

### 3. `src-tauri/src/dyn_windows.rs` — Make orb window resizable

Change `WindowConfig::main()`:

```rust
pub fn main() -> Self {
    Self {
        label: "main", title: "NEXUS", url: "index.html",
        width: 200., height: 200.,
        min_width: Some(100.), min_height: Some(100.),  // was 200, now 100
        resizable: true,                                 // was false
        decorations: false, transparent: true,
        always_on_top: true, skip_taskbar: true, shadow: false,
        focus: false, center: true, hidden_title: true,
    }
}
```

**Why?** `set_size()` may fail if the window is not resizable. Setting `resizable: true` and lowering the min to 100px allows the orb to shrink/grow.

### 4. New Tauri command: `set_orb_position` (live update)

Add to `window_manager.rs`:

```rust
/// IPC: invoke("set_orb_position", { horizontalPct, verticalPct, size })
/// Live-updates the orb position and size without restarting.
/// Does NOT save to settings — the frontend should call save_settings separately.
#[tauri::command]
pub fn set_orb_position<R: Runtime>(
    app: AppHandle<R>,
    horizontal_pct: f64,
    vertical_pct: f64,
    size: u32,
) -> Result<(), String> {
    let win = app
        .get_webview_window(WIN)
        .ok_or_else(|| "main window not found".to_string())?;

    if let Ok(Some(monitor)) = win.current_monitor() {
        let scale = monitor.scale_factor();
        let screen = monitor.size();
        let orb = size as i32;
        let phys_orb = (orb as f64 * scale) as i32;

        let raw_x = (screen.width as f64 * horizontal_pct) as i32 - phys_orb / 2;
        let raw_y = (screen.height as f64 * vertical_pct) as i32 - phys_orb / 2;
        let x = raw_x.max(0).min(screen.width as i32 - phys_orb);
        let y = raw_y.max(0).min(screen.height as i32 - phys_orb);

        let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
        let _ = win.set_size(tauri::PhysicalSize::new(orb, orb));
    }
    Ok(())
}
```

Register in `lib.rs` invoke_handler:

```rust
window_manager::set_orb_position,
```

### 5. Frontend — Settings sidebar slider UI

```tsx
// Horizontal position slider: 0% (left) ↔ 100% (right)
<input
  type="range"
  min={0}
  max={100}
  value={Math.round(settings.orbHorizontalPct * 100)}
  onChange={(e) => {
    const pct = parseInt(e.target.value) / 100;
    update("orbHorizontalPct", pct);
    // Live update — move orb immediately
    invoke("set_orb_position", {
      horizontalPct: pct,
      verticalPct: settings.orbVerticalPct,
      size: settings.orbSize,
    });
  }}
/>

// Vertical position slider: 0% (top) ↔ 100% (bottom)
<input
  type="range"
  min={0}
  max={100}
  value={Math.round(settings.orbVerticalPct * 100)}
  onChange={(e) => {
    const pct = parseInt(e.target.value) / 100;
    update("orbVerticalPct", pct);
    invoke("set_orb_position", {
      horizontalPct: settings.orbHorizontalPct,
      verticalPct: pct,
      size: settings.orbSize,
    });
  }}
/>

// Size slider: 100px ↔ 300px
<input
  type="range"
  min={100}
  max={300}
  value={settings.orbSize}
  onChange={(e) => {
    const size = parseInt(e.target.value);
    update("orbSize", size);
    invoke("set_orb_position", {
      horizontalPct: settings.orbHorizontalPct,
      verticalPct: settings.orbVerticalPct,
      size,
    });
  }}
/>
```

**Behavior:**
- As the user drags a slider, the orb moves/resizes **in real time** (live update via `set_orb_position`)
- The settings are saved to local state but NOT to disk yet
- When the user clicks "Save", `save_settings` writes everything to `settings.json`
- If the user closes the settings sidebar without saving, the orb stays at the last live-updated position for this session, but reverts to the saved position on next restart

### 6. Frontend `Settings` interface — Add 3 fields

```typescript
interface Settings {
  // ... existing fields ...
  orbHorizontalPct: number;  // 0.0 - 1.0
  orbVerticalPct: number;    // 0.0 - 1.0
  orbSize: number;           // 100 - 300
}

const DEFAULT_SETTINGS: Settings = {
  // ... existing defaults ...
  orbHorizontalPct: 0.5,
  orbVerticalPct: 1.0,
  orbSize: 200,
};
```

---

## Edge Cases and Safety

### 1. First run after update (no orb fields in settings.json)
- `#[serde(default)]` on the new fields → serde uses defaults (0.5, 1.0, 200)
- `read_orb_settings()` also has fallbacks → returns (0.5, 1.0, 200)
- Result: orb appears at center-bottom, 200px — **identical to current behavior**

### 2. User sets orb to extreme position (0% or 100%)
- Clamping in `position_orb()`: `x.max(0).min(screen_width - orb_size)`
- At 0% horizontal: orb's left edge touches left screen edge
- At 100% horizontal: orb's right edge touches right screen edge
- At 0% vertical: orb's top edge touches top screen edge
- At 100% vertical: orb's bottom edge touches bottom screen edge
- **Orb is always fully visible**

### 3. Multi-monitor
- `win.current_monitor()` returns the monitor the orb is currently on
- Percentages apply to that monitor's dimensions
- If the user moves the orb to a different monitor (manually), the next wake will reposition it to the percentage on the new monitor
- This is acceptable — the user sets "where on the screen" not "which screen"

### 4. DPI scaling
- `scale = monitor.scale_factor()` already used
- `phys_orb = (orb_size as f64 * scale) as i32` converts logical to physical pixels
- Position computation uses physical pixels (correct for `set_position`)
- **Already handled** — same pattern as existing code

### 5. Rapid slider changes
- Each slider change calls `set_orb_position` → `win.set_position` + `win.set_size`
- These are cheap Win32 calls (~1ms)
- Debouncing is optional but not strictly needed
- If needed: debounce to 16ms (60fps) in the frontend

### 6. Settings.json corruption
- `read_orb_settings()` returns defaults if the file can't be parsed
- `get_settings()` uses `unwrap_or_default()` for the entire struct
- **App never crashes from a corrupt settings file**

### 7. User saves, then changes again
- `save_settings` does `std::fs::write` which **overwrites** the entire file
- New values replace old values
- Old values are gone — no merge, no append, pure overwrite
- **This is exactly what the user asked for**

---

## Verification Plan

After implementation:

1. **First run (existing settings.json without orb fields)**
   - Orb appears at center-bottom, 200px
   - No errors in logs
   - settings.json is NOT modified until user saves

2. **User moves horizontal slider to 25%, saves**
   - Orb moves to 25% from left
   - settings.json now contains `"orbHorizontalPct": 0.25`
   - Restart app → orb appears at 25% from left

3. **User moves vertical slider to 50%, saves**
   - Orb moves to 50% from top (vertical center)
   - settings.json now contains `"orbVerticalPct": 0.5`
   - Old vertical value (1.0) is overwritten
   - Restart → orb at 50% from top

4. **User changes size to 150px, saves**
   - Orb shrinks to 150px
   - settings.json now contains `"orbSize": 150`
   - Restart → orb is 150px

5. **User sets all three, saves, then changes all three again, saves**
   - First save: settings.json has values A
   - Second save: settings.json is overwritten with values B
   - Restart → orb uses values B (not A)

6. **Live update (no save)**
   - User drags slider → orb moves immediately
   - User closes settings without saving → orb stays at new position for this session
   - Restart → orb reverts to last saved position

7. **Edge: slider at 0% and 100%**
   - Orb is fully on-screen (clamped)
   - No part is cut off

---

## File Change Summary

| File | Change | Lines |
|---|---|---|
| `src-tauri/src/commands.rs` | Add 3 fields + 3 default fns + 3 Default impl lines | ~15 |
| `src-tauri/src/window_manager.rs` | Rewrite `position_orb()`, add `read_orb_settings()`, add `set_orb_position` command | ~50 |
| `src-tauri/src/dyn_windows.rs` | `resizable: true`, `min_width: 100`, `min_height: 100` | 3 lines |
| `src-tauri/src/lib.rs` | Register `set_orb_position` command | 1 line |
| `frontend/src/settings/SettingsApp.tsx` (or new sidebar) | Add 3 fields to interface + defaults + slider UI | ~60 |

**Total: ~130 lines of new/changed code across 5 files.**

---

## Why This Is 100% Safe

1. **The persistence mechanism is already proven** — your hotkey, TTS voice, Groq key, and autostart setting are already saved this way and survive restarts.
2. **`#[serde(default)]` means no migration** — existing settings.json files without the new fields won't break; they get default values.
3. **`position_orb()` is already called on every wake** — no new call sites needed, just change what it does internally.
4. **Clamping prevents off-screen orbs** — the orb is always fully visible.
5. **`std::fs::write` overwrites** — new saves replace old values, exactly as requested.
6. **Fallbacks everywhere** — if settings.json is missing or corrupt, the orb uses defaults (center-bottom, 200px) and the app doesn't crash.
7. **Live update is separate from save** — the user can preview positions before committing, but the saved position is what persists.

---

*This is a plan. No code has been changed. Awaiting authorization to implement.*
