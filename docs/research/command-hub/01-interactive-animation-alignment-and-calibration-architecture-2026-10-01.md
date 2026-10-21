# Research 01 — Interactive Animation Alignment, Real-Time Desktop Calibration & HUD Overlay Architecture

**Date:** 2026-10-01  
**Category:** Command Hub / Visual UX / Window Management  
**Status:** DESIGN COMPLETE  
**Related Docs:** Feature 80 (`80-production-orb-creator-mcp-pipeline.md`), Feature 81 (`81-dual-grammar-modal-partitioning-and-foreground-grounding.md`), Change 57 (`57-ghost-fifo-queue-and-hotkey-realignment.md`)  

---

## 1. Problem Space & User Frustrations

### 1.1 The Blind Alignment Problem
In voice-first and ambient AI desktop assistants, positioning and sizing visual indicators (the Wakeup Orb, Ghost Mode Waves, and Loading Indicators) is critical to user ergonomics. Users have diverse desktop environments:
- Multi-monitor setups with varied resolutions (1080p, 1440p, 4K) and DPI scale factors (100%, 125%, 150%, 200%).
- Varied taskbar alignments (bottom, top, hidden).
- Differing aesthetic preferences (e.g. discreet corner orb vs large central floating sphere).

Previously, NEXUS offered two basic percentage sliders embedded inside the 520px-wide Command Hub sidebar, paired with a tiny 2D mock box. This caused several severe issues:
1. **The Sidebar Obscured the Desktop:** The 520px sidebar blocked the user's view of half the screen while attempting to position the orb.
2. **Disconnected Static Mock:** The tiny mock box inside the sidebar could not depict how the actual Lottie strokes, glows, and drop shadows interacted with desktop wallpapers and active application windows.
3. **Ghost Waves and Loading Were Locked:** While the orb could be moved, Ghost Waves were tied to the orb container, and the Loading Indicator was hardcoded to `(screen.width - 87, 9)` in the top-right corner without any sizing or repositioning controls.
4. **No Reversibility:** If the user dragged a slider and preferred their previous placement, there was no undo/redo capability.

---

## 2. Architectural Solution: On-Screen Calibration HUD

To deliver a truly professional, zero-friction calibration experience, NEXUS introduces an **On-Screen Calibration HUD (`calibrate-toolbar`)**:

```
                       [ Command Hub: Display Tab ]
                                     │
                    Click: "Launch On-Screen Calibrator"
                                     │
                                     ▼
        ┌────────────────────────────────────────────────────────┐
        │  1. Hide Command Hub Sidebar (reveals whole desktop)   │
        │  2. Spawn Top-Center Floating HUD (calibrate-toolbar)  │
        │  3. Show Real Desktop Preview of Selected Animation    │
        └────────────────────────────────────────────────────────┘
                                     │
           ┌─────────────────────────┼─────────────────────────┐
           │                         │                         │
     [ Wakeup Tab ]            [ Waves Tab ]            [ Loading Tab ]
           │                         │                         │
     Preview: Orb              Preview: Waves           Preview: Spinner
           │                         │                         │
           └─────────────────────────┼─────────────────────────┘
                                     │
                   User Drags Sliders (X, Y, Size)
                                     │
                                     ▼
      ┌──────────────────────────────────────────────────────────┐
      │ Rust window_manager::set_*_position (Sync <16ms update)  │
      │ Pushes state snapshot to Undo/Redo stack                 │
      └──────────────────────────────────────────────────────────┘
                                     │
                     ┌───────────────┴───────────────┐
                     │                               │
            Click: [ ✕ Cancel ]             Click: [ ✓ Save ]
                     │                               │
       Revert to initial snapshot        Persist to settings.json
                     │                               │
                     └───────────────┬───────────────┘
                                     │
                                     ▼
        ┌────────────────────────────────────────────────────────┐
        │  1. Destroy Calibration Toolbar (free ~40MB RAM)       │
        │  2. Restore Command Hub Sidebar with feedback toast    │
        └────────────────────────────────────────────────────────┘
```

---

## 3. Detailed Component Architecture

### 3.1 Top-Center Floating HUD Window (`WindowConfig::calibrate_toolbar`)
- **Dimensions:** 580px wide × 230px high.
- **Placement:** Centered horizontally at the top edge of the primary display (`x = (screenWidth - 580 * scale) / 2`, `y = 24 * scale`).
- **Window Flags:**
  - `decorations: false`
  - `transparent: true`
  - `always_on_top: true`
  - `skip_taskbar: true`
  - `shadow: true`
  - `focus: true`
- **Visual Design:** Liquid-glass acrylic aesthetic (`rgba(15, 23, 42, 0.80)`), high-radius backdrop blur (`36px`), hairline border (`rgba(255, 255, 255, 0.12)`), subtle drop shadow (`0 20px 50px rgba(0, 0, 0, 0.5)`).

### 3.2 Target Selector: 3 Visual Modalities
The HUD provides a prominent 3-way segmented pill control:
1. **🌟 Wakeup Orb:**
   - Controls: `orb_horizontal_pct` (0.0–1.0), `orb_vertical_pct` (0.0–1.0), `orb_size` (100–300px).
   - Desktop Preview: The smiling orb (`idle-smile` holding frame 300) appears at the specified coordinates and tracks slider changes in real time.
2. **🌊 Ghost Waves:**
   - Controls: `waves_horizontal_pct` (0.0–1.0), `waves_vertical_pct` (0.0–1.0), `waves_size` (100–300px).
   - Desktop Preview: The 3-bar animated soundwave (`#259ed6`, `#ef4f25`, `#fbdf38`) or `waves.json` Lottie actively animates so the user can verify wave motion and spacing against their workspace.
3. **⏳ Loading Indicator:**
   - Controls: `loading_horizontal_pct` (0.0–1.0), `loading_vertical_pct` (0.0–1.0), `loading_size` (40–160px).
   - Desktop Preview: The 3-circle Lottie loading indicator actively loops at the designated position.

### 3.3 The 3 Live Real-Time Sliders
Every target exposes identical, intuitive controls:
1. **Horizontal (Left ↔ Right):** Maps 0% (left display boundary) to 100% (right display boundary). Features a badge showing percentage and landmark (e.g., `"50% (Center)"`, `"0% (Left)"`, `"100% (Right)"`).
2. **Vertical (Top ↔ Bottom):** Maps 0% (top display boundary) to 100% (bottom display boundary). Features a landmark badge (e.g., `"100% (Bottom)"`, `"0% (Top)"`, `"50% (Middle)"`).
3. **Size / Scale:** Maps pixel dimensions within safety boundaries. For the Orb and Waves, `100px` to `300px`; for the Loading Indicator, `40px` to `160px`.

### 3.4 Low-Latency Native Preview Synchronization
When dragging a slider, React dispatches `invoke("set_orb_position")`, `invoke("set_waves_position")`, or `invoke("set_loading_position")`.
- Rust executes `set_position()` and `set_size()` directly on the underlying OS `HWND` via Tauri's Windows backend.
- The update executes in sub-16ms (within a single display frame at 60Hz), ensuring buttery-smooth synchronous movement on screen as the slider moves.
- To prevent mouse capture collisions during calibration, the preview window enables `set_ignore_cursor_events(true)` so the user's cursor never snags on the previewing element.

### 3.5 State History Engine (Undo / Redo)
The HUD maintains a discrete snapshot history:
```typescript
interface TargetRect {
  hPct: number;
  vPct: number;
  size: number;
}

interface CalibrationSnapshot {
  wakeup: TargetRect;
  waves: TargetRect;
  loading: TargetRect;
}

interface HistoryState {
  past: CalibrationSnapshot[];
  present: CalibrationSnapshot;
  future: CalibrationSnapshot[];
}
```
- Slider drag start captures baseline state.
- Slider release (`onPointerUp` / `onChangeEnd`) commits the new `present` state and pushes the prior `present` into `past`.
- **Undo (`↶`)**: Pops from `past`, pushes current `present` to `future`, and re-invokes Rust positioning with the restored state.
- **Redo (`↷`)**: Pops from `future`, pushes current `present` to `past`, and applies the forward state.
- Keyboard shortcuts (`Ctrl+Z` for Undo, `Ctrl+Y` / `Ctrl+Shift+Z` for Redo) are natively wired.

---

## 4. Modal Round-Trip & Data Persistence

### 4.1 Cancel Lifecycle (`✕ Cancel`)
1. User clicks Cancel or presses `Escape`.
2. Calibration HUD restores the initial pre-calibration snapshot via Rust IPC.
3. Calibration window is destroyed via `dyn_windows::destroy_window("calibrate-toolbar")`.
4. Command Hub sidebar is re-shown via `show_settings_sidebar()`.

### 4.2 Save Lifecycle (`✓ Save Changes`)
1. User clicks Save or presses `Ctrl+Enter`.
2. All 9 parameters are passed to `save_settings`:
   - `orb_horizontal_pct`, `orb_vertical_pct`, `orb_size`
   - `waves_horizontal_pct`, `waves_vertical_pct`, `waves_size`
   - `loading_horizontal_pct`, `loading_vertical_pct`, `loading_size`
3. Rust writes the updated `NexusSettings` struct atomically to `%APPDATA%/com.nexus.assistant/settings.json`.
4. Calibration window is destroyed.
5. Command Hub sidebar is re-shown with a positive confirmation toast notification: *"Animation alignment saved successfully."*
