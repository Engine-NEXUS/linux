# Change 102 — Sidebar Text Formatting, Top-Right Dock Buttons & Settings Full-Height Span

## Problem & Motivation
1. **Settings Sidebar Height Cutoff**: In `commands.rs`, `sidebar_geometry("settings", ...)` locked the height to `(monitor_h - 40.0).min(780.0)`, leaving a ~280px void between the bottom of the Command Hub card and the taskbar on 1080p and higher displays. The user requested extending the height to cover all the way down to the bottom.
2. **Dock Left & Dock Right Buttons**: In `SettingsSidebarApp.tsx`, the dock controls in `hx-header-dock` were unstyled bare unicode text buttons (`background: none; border: 0; color: var(--hx-faint); font-size: 13px; padding: 2px 5px;`) constrained inside a 72px grid cell, making them look faint, unclickable, and misaligned with the window border.
3. **Sidebar Text Formatting & Misalignment**:
   - In `sidebar.css`, heading styles (`.nexus-h1` through `.nexus-h6` and `h1` through `h6`) were previously removed, causing markdown headings to render with raw browser default styles with huge margins (`margin-top: 0.67em; margin-bottom: 0.67em`) that distorted text baselines. Furthermore, the first child in `.nexus-markdown-body` lacked top margin normalization (`:first-child { margin-top: 0 }`).
   - In `HubHome.tsx`, the 3-column insights stats grid ("API keys", "MCP connected", "Requests today") had mismatched vertical baselines because column 3 contained a subtitle line while columns 1 and 2 did not.

## Root Cause & Fixes

### 1. Settings Sidebar Full-Height Window Span
- [`src-tauri/src/commands.rs`](file:///c:/PROJECTS/ULTRON/src-tauri/src/commands.rs):
  - Removed the `.min(780.0)` cap in `sidebar_geometry`:
    ```rust
    "settings" => {
        let h = (monitor_h - 40.0).min(1040.0).max(400.0);
        dock_x(400.0, h)
    }
    ```
  - Spans from $y=20.0$ down to $y=1060.0$ (covering all the way down to the taskbar/bottom margin).
  - Updated `sidebar_geometry_tests` (`settings_right_docked` and `settings_docked_goes_right`) to assert $1040.0$ height on 1080p monitors.

### 2. Dock Left & Dock Right Buttons at Top Right
- [`frontend/src/settings-sidebar/SettingsSidebarApp.tsx`](file:///c:/PROJECTS/ULTRON/frontend/src/settings-sidebar/SettingsSidebarApp.tsx):
  - Structured `.hx-header` with `.hx-header-left`, absolute `.hx-title`, and `.hx-header-dock` with `data-tauri-drag-region` on header regions.
- [`frontend/src/settings-sidebar/hub/hub.css`](file:///c:/PROJECTS/ULTRON/frontend/src/settings-sidebar/hub/hub.css):
  - Styled `.hx-header-dock button`: $28\times28\text{px}$ rounded button with border, subtle shadow, background card tint, hover glow, and active scale transition matching the desktop window design language.
  - Positioned `.hx-header-dock` cleanly at the top right of the header bar (`margin-left: auto; z-index: 1;`).
- [`frontend/src/sidebar/sidebar.css`](file:///c:/PROJECTS/ULTRON/frontend/src/sidebar/sidebar.css):
  - Refined `.sidebar-header-row` padding (`12px 14px 4px 14px`) and eliminated disruptive negative margins to position the Assistant sidebar dock buttons neatly at the top right.

### 3. Sidebar Text Formatting & Baseline Alignment
- [`frontend/src/sidebar/sidebar.css`](file:///c:/PROJECTS/ULTRON/frontend/src/sidebar/sidebar.css):
  - Restored full typography hierarchy for `.nexus-markdown-body h1..h6` and `.nexus-h1..h6`:
    - `h1`: 20px, bold, tight letter-spacing, line-height 1.3, crisp text shadow.
    - `h2`: 16.5px, 650 weight, line-height 1.35.
    - `h3`: 14.5px, 600 weight, line-height 1.4.
    - `h4..h6`: 13.5px, 600 weight.
  - Added `.nexus-markdown-body > :first-child { margin-top: 0 !important; }` so responses begin at a neat, consistent vertical position.
- [`frontend/src/settings-sidebar/hub/HubHome.tsx`](file:///c:/PROJECTS/ULTRON/frontend/src/settings-sidebar/hub/HubHome.tsx) & [`hub.css`](file:///c:/PROJECTS/ULTRON/frontend/src/settings-sidebar/hub/hub.css):
  - Added subtitle placeholder rows (`.hx-stat-sub--placeholder`) to columns 1 and 2 in `.hx-stats`. Values and labels now share exact horizontal and vertical baselines across all 3 stats.
  - Refined `.hx-section` headings (12px, 600 weight, uppercase, 0.05em tracking) and `.hx-row` typography (`line-height: 1.3; gap: 12px; padding: 11px 14px;`).

## Verification
- `cargo test --lib -- commands::sidebar_geometry_tests`: 13/13 pass.
- `npx tsc --noEmit`: Clean (0 errors).
- `npm test -- --run`: 200/200 pass.
- `cargo check --features custom-protocol,admin-brain`: Clean (0 warnings, 0 errors).
- `npm run build`: Clean (built in 9.91s).
- `cargo test --lib -- --test-threads=1`: 1020/1020 pass.
- `cargo build --release --features custom-protocol,admin-brain`: Clean release binary.
