# Quick Reference — Implementation Patterns for Future Study

This document captures the key implementation patterns and "how-to" recipes from the session for future reference.

---

## Pattern 1: ADR-05 Screenshot-Capture Blur (The Working Design)

### Architecture
```
Rust (pre-show)          Rust (live loop)          Frontend
┌─────────────┐         ┌─────────────┐          ┌─────────────┐
│ prepare_    │         │ spawn_      │          │ SidebarApp  │
│ sidebar()   │────────▶│ sidebar_    │────────▶ │ listen()    │
│ set_size/   │         │ live_blur() │          │ set --var   │
│ set_pos()   │         │ 1 Hz +      │          │ ::after     │
│ capture_    │         │ hash diff   │          │ {bg: var()} │
│ backdrop()  │         │ emit event  │          │             │
└─────────────┘         └─────────────┘          └─────────────┘
```

### Critical Timing
- **Pre-show capture MUST run before `win.show()`** — otherwise captures sidebar itself
- `capture_and_emit_backdrop()` checks `already_visible` flag
- Fresh window: `already_visible=false` → capture runs
- Existing window: `already_visible=true` → capture skipped, event emitted as fast path

### Frame-Hash Diffing (Live Loop)
```rust
// sidebar_backdrop.rs
pub fn frame_hash(bgra: &[u8]) -> u64 {
    let mut hash: u64 = 5381;
    for &byte in bgra.iter().step_by(16) {  // Sample every 16th byte
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}

// Live loop only runs full pipeline if hash changed
if should_emit {
    if let Some(data_uri) = blur_bgra_to_jpeg_fast(&raw_bgra, ...) {
        app.emit("sidebar:backdrop", data_uri);
    }
}
```

---

## Pattern 2: Unified Sidebar (One HWND, Four Views)

### Window Creation (Lazy, On-Demand)
```rust
// dyn_windows.rs
pub fn get_or_create_window<R: Runtime>(
    app: &tauri::AppHandle<R>,
    config: WindowConfig,
) -> Result<tauri::WebviewWindow<R>, String> {
    // 1. Check existing
    if let Some(win) = app.get_webview_window(config.label) {
        return Ok(win);
    }
    // 2. Create new with platform effects
    let win = WebviewWindowBuilder::new(...)
        .transparent(true)
        .build()?;
    
    // 3. Apply DWM rounded corners + capture exclusion (NOT material APIs)
    #[cfg(target_os = "windows")]
    if matches!(config.label, "sidebar" | "calibrate-toolbar") {
        crate::dwm_corners::round_corners(&win);
        // WDA_EXCLUDEFROMCAPTURE = 17
        unsafe { SetWindowDisplayAffinity(HWND(hwnd.0 as _), WINDOW_DISPLAY_AFFINITY(17)) };
    }
    Ok(win)
}
```

### View Switching (React, No HWND Swap)
```tsx
// UnifiedSidebar.tsx
<div className="unified-view-host">
  {mounted.has("assistant") && (
    <div className="unified-view" style={{ display: view === "assistant" ? "flex" : "none" }}>
      <SidebarApp onDock={dock} />
    </div>
  )}
  {mounted.has("settings") && (
    <div className="unified-view" style={{ display: view === "settings" ? "flex" : "none" }}>
      <SettingsSidebarApp onDock={dock} />
    </div>
  )}
  // ... architect, pr-list, spatial
</div>
```

### Rust Side: `sidebar:set_view` Event
```rust
// commands.rs
pub(crate) fn finish_sidebar<R: Runtime>(...) {
    win.show()?;
    win.set_focus()?;
    crate::dwm_corners::round_corners(win);  // Re-assert corners on show
    app.emit("sidebar:set_view", json!({ "view": view, "dock": dock }))?;
}
```

---

## Pattern 3: Pending Content Race-Free Pattern

### Problem
Window created on-demand → WebView2 loads HTML → React mounts → registers listeners
But Rust emits events immediately after `win.show()` → events lost if listener not ready

### Solution: Static Pending + Fetch on Mount
```rust
// commands.rs
static PENDING_SIDEBAR: Mutex<Option<PendingSidebar>> = Mutex::new(None);

pub async fn show_sidebar_with_content(...) {
    let prep = prepare_sidebar(...).await?;
    let backdrop = capture_and_emit_backdrop(...);
    
    // Store for fresh-window race
    *PENDING_SIDEBAR.lock().unwrap() = Some(PendingSidebar { query, text, backdrop, ... });
    
    finish_sidebar(...);  // Shows window, emits sidebar:set_view
    
    // Fast path for existing window
    if window_existed {
        app.emit("sidebar:show", json!({ query, text }));
        if let Some(uri) = backdrop { app.emit("sidebar:backdrop", uri); }
    }
}

// Frontend fetches on mount
#[tauri::command]
pub fn get_pending_sidebar_content() -> Result<Option<Value>> {
    let mut pending = PENDING_SIDEBAR.lock().unwrap();
    let data = pending.take();  // Take clears it
    Ok(data.map(|p| json!({ "query": p.query, "backdrop": p.backdrop, ... })))
}
```

### Frontend Mount
```tsx
// SidebarApp.tsx
useEffect(() => {
    invoke("get_pending_sidebar_content").then((pending) => {
        if (pending?.backdrop) {
            document.documentElement.style.setProperty("--sidebar-backdrop-image", `url(${pending.backdrop})`);
        }
        // ... render content
    });
}, []);
```

---

## Pattern 4: Transparent Drag Region with Embedded Buttons

### CSS Structure
```css
/* Header row — full width drag region */
.settings-header-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 4px 0 8px 0;
    border-bottom: 1px solid rgba(255, 255, 255, 0.1);
    user-select: none;
    -webkit-user-select: none;
    cursor: grab;           /* Drag cursor */
}
.settings-header-row:active { cursor: grabbing; }

/* Buttons inside — pointer-events: auto so clicks don't drag */
.sidebar-dock-btn,
.settings-tab {
    pointer-events: auto;
    cursor: pointer;
}
```

### React
```tsx
<header className="settings-header-row" data-tauri-drag-region>
    <div className="settings-tabs">  {/* Left — tabs */}
        <button className="settings-tab">Display</button>
        ...
    </div>
    <div className="sidebar-dock-controls">  {/* Right — dock */}
        <button className="sidebar-dock-btn">◧</button>
        <button className="sidebar-dock-btn">◨</button>
    </div>
</header>
```

### Key Points
- `data-tauri-drag-region` on the flex container → entire row draggable
- Buttons have `pointer-events: auto` → clicks pass to button, not drag
- `user-select: none` prevents text selection during drag

---

## Pattern 5: Frosted Glass Dock Buttons

### CSS
```css
.sidebar-dock-btn {
    width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 9px;
    background: transparent;              /* Key: transparent */
    backdrop-filter: blur(8px);           /* Key: blur behind button */
    color: #ffffff;
    font-size: 13px;
    cursor: pointer;
    transition: background 0.18s ease, transform 0.12s ease, border-color 0.18s ease;
}

.sidebar-dock-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    border-color: rgba(255, 255, 255, 0.25);
}

.sidebar-dock-btn:active {
    transform: scale(0.94);
}
```

### Why This Works
- `background: transparent` — doesn't block the card's `::after` backdrop
- `backdrop-filter: blur(8px)` — button itself gets frosted glass effect
- Border provides subtle definition without blocking blur

---

## Pattern 6: Realistic Blur Tuning (Start Menu Style)

### Parameters
| Parameter | Location | Start Menu Value | Effect |
|-----------|----------|------------------|--------|
| Blur sigma | `commands.rs:870`, `sidebar_backdrop.rs:160` | 10 | Lower = text readable |
| Gradient top | All `::after` | 0.65 | Higher = darker top |
| Gradient bottom | All `::after` | 0.45 | Higher = darker bottom |
| Live interval | `sidebar_backdrop.rs:144` | 1000ms | 1 Hz refresh |

### Gradient Formula
```css
background-image:
    linear-gradient(180deg, rgba(8, 8, 10, 0.65) 0%, rgba(8, 8, 10, 0.45) 100%),
    var(--sidebar-backdrop-image, none);
background-size: cover;
background-position: center;
```

### Why This Matches Start Menu
- Windows Start Menu uses ~σ=8-12 blur with a dark overlay (~60% opacity)
- Text behind is frosted but legible at normal reading distance
- Our σ=10 + 0.65→0.45 gradient achieves the same perceptual result

---

## Pattern 7: Build Command (Critical)

### NEVER use bare `cargo build --release`
```bash
# WRONG — loads http://localhost:5173 (dev server)
cargo build --release

# CORRECT — embeds frontend assets via custom-protocol feature
npm --prefix frontend run build
cargo build --release --features custom-protocol  # inside src-tauri/

# OR official script (handles everything)
pwsh ./scripts/build.ps1
```

### Why It Matters
```rust
// tauri-macros/src/context.rs
dev: cfg!(not(feature = "custom-protocol")),  // Feature OFF = dev URL
```
- Feature ON → `tauri://localhost` (embedded assets)
- Feature OFF → `http://localhost:5173` (dev server)

### Verify Binary URL
```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"
Start-Process .\src-tauri\target\release\nexus.exe
Start-Sleep 15
Invoke-RestMethod http://127.0.0.1:9222/json/list | Select-Object title, url
# Expected: http://tauri.localhost/...
# Bad: http://localhost:5173/...
```

---

## Pattern 8: Stale Binary Trap

### The Problem
Start Menu `NEXUS.lnk` → `%LOCALAPPDATA%\NEXUS\nexus.exe` = **2026-08-31 stale build**
Predates every fix in this session.

### Rule
```markdown
**Always launch via `nexus start` after `nexus build`**
- `nexus start` kills existing instances
- Runs `target\release\nexus.exe` (fresh build)
- Start Menu shortcut is NEVER updated by build
```

---

## Debugging Checklist

### Blur Not Working?
1. Check `commands.rs` — `DEV_LIVE_BLUR_DISABLED = false`?
2. Check `sidebar_backdrop.rs` — `LIVE_BLUR_INTERVAL_MS = 1000`?
3. Check dist CSS — `--sidebar-backdrop-image` present 4 times?
4. Check `capture_backdrop` sigma = 10?
5. Check gradient opacity = 0.65→0.45 in all 4 views?

### Two Windows Visible?
1. `show_settings_sidebar` should NOT call `orb.show()`
2. Only one HWND: `label: "sidebar"`

### Dock Buttons Block Blur?
1. `unified.css` — `.sidebar-dock-btn` background = `transparent`?
2. `backdrop-filter: blur(8px)` present?

### Header Row Not Merged?
1. `SettingsSidebarApp.tsx` — single `<header className="settings-header-row">`?
2. `settings-sidebar.css` — `.settings-header-row` with `justify-content: space-between`?

### Build Fails?
1. `orchestrator.rs` — using `set_pending_sidebar_text()` not private static?
2. `sidebar_backdrop.rs` — `_sigma` parameter prefixed?

---

## Key File Locations

```
src-tauri/src/
├── commands.rs           # Sidebar show/capture, pending content, live blur toggle
├── sidebar_backdrop.rs   # GDI capture, fast_blur, frame_hash, constants
├── dyn_windows.rs        # Lazy window creation, DWM corners (no material APIs)
├── orchestrator.rs       # OCR → pending content pattern
└── lib.rs                # Module registration

frontend/src/
├── sidebar/
│   ├── SidebarApp.tsx          # Assistant view + header row
│   ├── UnifiedSidebar.tsx      # Shell + view routing
│   ├── sidebar.css             # .sidebar-card::after, .settings-header-row
│   └── unified.css             # .sidebar-dock-btn (shared)
├── settings-sidebar/
│   ├── SettingsSidebarApp.tsx  # Merged header row
│   └── settings-sidebar.css    # .settings-container::after, .settings-header-row
├── architect/
│   ├── ArchitectApp.tsx        # Header row with dock
│   └── architect.css           # .architect-app::after
├── pr-list/
│   ├── PrListApp.tsx           # Header row with dock
│   └── pr-list.css             # .pr-list-container::after
└── sidebar/
    └── SpatialDashboard.tsx    # Header row with dock
```

---

## One-Line Recipes

| Task | Command |
|------|---------|
| Full build | `node nexus.mjs build` |
| TypeScript check | `cd frontend && npx tsc --noEmit` |
| Run tests | `cd frontend && npm test -- --run` |
| Launch fresh binary | `nexus start` |
| Verify binary URL | `cd src-tauri && $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"; Start-Process .\target\release\nexus.exe; Start-Sleep 15; irm http://127.0.0.1:9222/json/list \| select title,url` |
| Check dist CSS | `cat frontend/dist/assets/sidebar-*.css \| grep -c "sidebar-backdrop-image"` |
| Kill stuck process | `taskkill /f /im nexus.exe` |