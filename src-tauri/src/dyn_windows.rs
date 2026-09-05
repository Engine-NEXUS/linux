//! Dynamic window creation — windows are created on-demand instead of at
//! startup to save RAM. Each WebView2 window spawns ~7 processes (~250 MB),
//! so creating 4 invisible windows at startup wastes ~1 GB.
//!
//! Only the `main` (orb) window is created at startup (via tauri.conf.json).
//! All other windows (setup, settings, sidebar, architect) are created here
//! when first needed, and destroyed (not hidden) when closed.

use tauri::{Manager, Runtime, WebviewWindowBuilder, WebviewUrl};

/// Window configs — mirrors the old tauri.conf.json entries.
/// Kept here so the window attributes are in one place.

pub struct WindowConfig {
    pub label: &'static str,
    pub title: &'static str,
    pub url: &'static str,
    pub width: f64,
    pub height: f64,
    pub min_width: Option<f64>,
    pub min_height: Option<f64>,
    pub resizable: bool,
    pub decorations: bool,
    pub transparent: bool,
    pub always_on_top: bool,
    pub skip_taskbar: bool,
    pub shadow: bool,
    pub focus: bool,
    pub center: bool,
    #[allow(dead_code)]
    pub hidden_title: bool,
}

impl WindowConfig {
    /// Fullscreen transparent stage — the orb roams inside it (frontend
    /// transform). The native window never moves (Wayland forbids client
    /// positioning), so fullscreen + click-through is the only portable
    /// free-move approach. Maximized (not exclusive fullscreen) keeps the
    /// taskbar/panel visible and stays on the primary monitor.
    /// ponytail: single-monitor stage; multi-monitor roam needs one stage
    /// per monitor via `current_monitors()` + per-monitor windows.
    pub fn main() -> Self {
        Self {
            label: "main", title: "NEXUS", url: "index.html",
            width: 200., height: 200., min_width: None, min_height: None,
            resizable: false, decorations: false, transparent: true,
            always_on_top: true, skip_taskbar: true, shadow: false,
            focus: false, center: false, hidden_title: true,
        }
    }

    pub fn setup() -> Self {
        Self {
            label: "setup", title: "NEXUS Setup", url: "setup.html",
            width: 520., height: 680., min_width: None, min_height: None,
            resizable: false, decorations: true, transparent: false,
            always_on_top: false, skip_taskbar: false, shadow: true,
            focus: true, center: true, hidden_title: false,
        }
    }
    pub fn settings() -> Self {
        Self {
            label: "settings", title: "NEXUS Settings", url: "settings.html",
            width: 600., height: 720., min_width: None, min_height: None,
            resizable: false, decorations: true, transparent: false,
            always_on_top: false, skip_taskbar: false, shadow: true,
            focus: true, center: true, hidden_title: false,
        }
    }
    pub fn sidebar() -> Self {
        Self {
            label: "sidebar", title: "NEXUS Response", url: "sidebar.html",
            width: 600., height: 1000., min_width: Some(600.), min_height: Some(1000.),
            resizable: false, decorations: false, transparent: true,
            always_on_top: true, skip_taskbar: true, shadow: false,
            focus: false, center: false, hidden_title: true,
        }
    }
    pub fn architect() -> Self {
        Self {
            label: "architect", title: "NEXUS Architecture Mapper", url: "architect.html",
            width: 1400., height: 900., min_width: Some(900.), min_height: Some(600.),
            resizable: true, decorations: true, transparent: false,
            always_on_top: false, skip_taskbar: false, shadow: true,
            focus: true, center: true, hidden_title: false,
        }
    }
    /// Architect sidebar — 900px wide, transparent, undecorated, always-on-top.
    /// Same liquid-glass styling as the response sidebar but wider, used for
    /// the Architecture Mapper (Phase 1 layers, Phase 2 dependency graph,
    /// hotspots, cycles, blast radius).
    pub fn architect_sidebar() -> Self {
        Self {
            label: "architect-sidebar", title: "NEXUS Architecture Mapper", url: "architect.html",
            width: 900., height: 1000., min_width: Some(900.), min_height: Some(1000.),
            resizable: false, decorations: false, transparent: true,
            always_on_top: true, skip_taskbar: true, shadow: false,
            focus: false, center: false, hidden_title: true,
        }
    }
}

/// Get an existing window, or create it on-demand if it doesn't exist.
/// Returns the window reference. The caller is responsible for showing it.
pub fn get_or_create_window<R: Runtime>(
    app: &tauri::AppHandle<R>,
    config: WindowConfig,
) -> Result<tauri::WebviewWindow<R>, String> {
    // Try existing first
    if let Some(win) = app.get_webview_window(config.label) {
        return Ok(win);
    }

    // Create new window
    tracing::info!("dyn_windows: creating '{}' window on-demand", config.label);

    let mut builder = WebviewWindowBuilder::new(app, config.label, WebviewUrl::App(config.url.into()))
        .title(config.title)
        .inner_size(config.width, config.height)
        .resizable(config.resizable)
        .decorations(config.decorations)
        .transparent(config.transparent)
        .always_on_top(config.always_on_top)
        .skip_taskbar(config.skip_taskbar)
        .shadow(config.shadow)
        .focused(config.focus)
        .visible(false); // Start hidden — caller will show after positioning

    if let Some(mw) = config.min_width {
        if let Some(mh) = config.min_height {
            builder = builder.min_inner_size(mw, mh);
        }
    }

    if config.center {
        builder = builder.center();
    }

    // Fullscreen stage for the orb ("main"): maximized transparent window
    // covering the work area. The orb roams inside via frontend transform.
    if config.label == "main" {
        builder = builder.maximized(true);
    }

    // Note: hidden_title and drag_drop_enabled are not available on the
    // Tauri 2 WebviewWindowBuilder. They were set in tauri.conf.json before.
    // For the sidebar, drag-drop is disabled by default on non-decorated windows.
    // hidden_title is a macOS-only feature that's not critical for functionality.

    let win = builder.build().map_err(|e| format!("Failed to create {} window: {e}", config.label))?;

    // Apply platform-specific effects
    #[cfg(target_os = "windows")]
    {
        if config.label == "sidebar" || config.label == "architect-sidebar" {
            crate::dwm_corners::round_corners(&win);

            if let Ok(hwnd) = win.hwnd() {
                use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WINDOW_DISPLAY_AFFINITY};
                use windows::Win32::Foundation::HWND;
                // WDA_EXCLUDEFROMCAPTURE is 0x00000011 (17)
                unsafe {
                    let _ = SetWindowDisplayAffinity(HWND(hwnd.0 as _), WINDOW_DISPLAY_AFFINITY(17));
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if config.label == "sidebar" || config.label == "architect-sidebar" {
            // NOTE: loading-indicator deliberately does NOT get vibrancy —
            // it must be fully transparent with no blur (per user spec).
            use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};
            let _ = apply_vibrancy(
                &win,
                NSVisualEffectMaterial::Sidebar,
                Some(NSVisualEffectState::Active),
                Some(20.0),
            );
        }
    }

    tracing::info!("dyn_windows: '{}' window created", config.label);
    Ok(win)
}

/// Destroy a window and its WebView2 process tree.
/// This is the RAM-saving alternative to `hide()` — hide() keeps the
/// WebView2 processes alive (~250 MB per window), destroy() kills them.
pub fn destroy_window<R: Runtime>(
    app: &tauri::AppHandle<R>,
    label: &str,
) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(label) {
        tracing::info!("dyn_windows: destroying '{}' window (freeing ~250 MB)", label);
        let result: Result<(), tauri::Error> = win.destroy();
        result.map_err(|e| format!("Failed to destroy {label} window: {e}"))?;
    }
    Ok(())
}
