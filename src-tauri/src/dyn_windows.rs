//! Dynamic window creation — windows are created on-demand instead of at
//! startup to save RAM. Each WebView2 window spawns ~7 processes (~250 MB),
//! so creating invisible windows at startup wastes RAM for nothing.
//!
//! Only `stage` is created at startup (single-stage migration — it hosts
//! the always-on orb + loading indicator as positioned divs, see
//! `window_manager.rs`). All other windows (setup, settings, sidebar,
//! calibrate-toolbar) are created here when first needed, and destroyed
//! (not hidden) when closed.

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
    // `main` (orb) and `loading-indicator` were retired in the single-stage
    // migration — both now render as positioned divs inside `stage()`
    // below (see window_manager.rs's orb_rect/emit_loading_rect + the
    // frontend's OrbFrame/LoadingIndicator components), not their own OS
    // windows.

    pub fn setup() -> Self {
        Self {
            label: "setup", title: "NEXUS Setup", url: "setup.html",
            width: 520., height: 680., min_width: None, min_height: None,
            resizable: false, decorations: false, transparent: true,
            always_on_top: false, skip_taskbar: false, shadow: true,
            focus: true, center: true, hidden_title: true,
        }
    }
    pub fn settings() -> Self {
        Self {
            label: "settings", title: "NEXUS Settings", url: "settings.html",
            width: 600., height: 720., min_width: None, min_height: None,
            resizable: false, decorations: false, transparent: true,
            always_on_top: false, skip_taskbar: false, shadow: true,
            focus: true, center: true, hidden_title: true,
        }
    }
    /// Unified dynamic sidebar — ONE window hosting every panel view
    /// (Assistant, Command Hub, Architect, PR List) switched in React
    /// without creating/destroying HWNDs. Base geometry is the right-dock
    /// default; `show_sidebar_view` re-sizes/re-positions per view
    /// (520×980 dock / 740×900 center modal / 960×950 architect).
    pub fn sidebar() -> Self {
        Self {
            label: "sidebar", title: "NEXUS Response", url: "sidebar.html",
            width: 520., height: 980., min_width: None, min_height: None,
            resizable: false, decorations: false, transparent: true,
            always_on_top: true, skip_taskbar: true, shadow: false,
            focus: false, center: false, hidden_title: true,
        }
    }

    /// Stage shell — fullscreen transparent overlay, always-on (single-
    /// stage migration). Hosts the voice orb, the loading indicator, the
    /// ghost ring, and stage notices/annotations. WS_EX_TOOLWINDOW so
    /// fullscreen video underneath keeps playing; NOT capture-excluded.
    pub fn stage() -> Self {
        Self {
            label: "stage", title: "NEXUS Stage", url: "stage.html",
            width: 1920., height: 1080., min_width: None, min_height: None,
            resizable: false, decorations: false, transparent: true,
            always_on_top: true, skip_taskbar: true, shadow: false,
            focus: false, center: false, hidden_title: true,
        }
    }

    /// Animation-calibration companion pill — 540×92 floating HUD at the
    /// top-center of the primary display (task §2). Created only while the
    /// calibration session is live, destroyed on save/cancel.
    pub fn calibrate_toolbar() -> Self {
        Self {
            label: "calibrate-toolbar", title: "NEXUS Calibrator", url: "companion-hud.html",
            width: 540., height: 92., min_width: None, min_height: None,
            resizable: false, decorations: false, transparent: true,
            always_on_top: true, skip_taskbar: true, shadow: true,
            focus: true, center: false, hidden_title: true,
        }
    }

    /// Hand mode camera window: a small click-through preview (camera + skeleton + status) in the
    /// bottom-left corner. It must be VISIBLE (a hidden WebView2 throttles video) and it is the
    /// on-screen proof that the camera is on. Created when hand mode starts, destroyed when it ends.
    pub fn hand() -> Self {
        Self {
            label: "hand", title: "NEXUS Hand Mode", url: "hand.html",
            width: 300., height: 250., min_width: None, min_height: None,
            resizable: false, decorations: false, transparent: false,
            always_on_top: true, skip_taskbar: true, shadow: true,
            focus: false, center: false, hidden_title: true,
        }
    }

    /// Hand mode's camera page with NO visible preview (the default): an 8x8 transparent click-through
    /// window in the top-right corner. It must stay "visible" to the OS (a hidden or covered WebView2
    /// throttles), but nobody should see it. `handShowCamera` swaps in `hand()` for debugging.
    pub fn hand_hidden() -> Self {
        Self {
            label: "hand", title: "NEXUS Hand Mode", url: "hand.html",
            width: 8., height: 8., min_width: None, min_height: None,
            resizable: false, decorations: false, transparent: true,
            always_on_top: true, skip_taskbar: true, shadow: false,
            focus: false, center: false, hidden_title: true,
        }
    }

    /// Orb Customization Studio window — 800px wide (twice Settings width),
    /// positioned to the left of the Settings sidebar.
    pub fn orb_studio() -> Self {
        Self {
            label: "orb-studio", title: "NEXUS Orb Studio", url: "orb-studio.html",
            width: 800., height: 980., min_width: Some(720.), min_height: Some(600.),
            resizable: true, decorations: false, transparent: true,
            always_on_top: true, skip_taskbar: false, shadow: true,
            focus: true, center: false, hidden_title: true,
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

    // Linux: the fullscreen stage is sized from the monitor. GTK ignores
    // `maximized` on a window that is still hidden (the stage is created
    // hidden and shown a moment later), so without this the overlay keeps
    // the placeholder 1920x1080 size and does not cover the screen. At
    // creation the monitor may not be known yet — `window_manager::
    // size_stage_to_monitor` fixes it up right after the first show.
    #[cfg(target_os = "linux")]
    let (width, height) = if config.label == "stage" {
        match app.primary_monitor() {
            Ok(Some(m)) => {
                let scale = m.scale_factor();
                (m.size().width as f64 / scale, m.size().height as f64 / scale)
            }
            _ => (config.width, config.height),
        }
    } else {
        (config.width, config.height)
    };
    #[cfg(not(target_os = "linux"))]
    let (width, height) = (config.width, config.height);

    let mut builder = WebviewWindowBuilder::new(app, config.label, WebviewUrl::App(config.url.into()))
        .title(config.title)
        .inner_size(width, height)
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

    // Note: hidden_title and drag_drop_enabled are not available on the
    // Tauri 2 WebviewWindowBuilder. They were set in tauri.conf.json before.
    // For the sidebar, drag-drop is disabled by default on non-decorated windows.
    // hidden_title is a macOS-only feature that's not critical for functionality.

    let win = builder.build().map_err(|e| format!("Failed to create {} window: {e}", config.label))?;

    // Linux: WebKitGTK denies getUserMedia unless the webview has a
    // `permission-request` handler, and the handler lives ON the webview —
    // so every webview NEXUS creates needs one. The stage window in
    // particular is destroyed and rebuilt by the blackout watchdog, and a
    // rebuilt webview without a handler silently kills the orb's mic path.
    #[cfg(target_os = "linux")]
    crate::mic_permissions::allow_media_for_window(&win);

    // Linux: configure stage as layer-shell overlay (Wayland) or non-focusable
    // notification window (X11) before it is mapped.
    #[cfg(target_os = "linux")]
    if config.label == "stage" {
        crate::overlay_linux::init_stage(&win);
    }

    // Apply platform-specific effects
    #[cfg(target_os = "windows")]
    {
        // Compact glass windows get DWM rounded corners + capture exclusion only.
        // ADR-05 (docs/architecture/06-liquid-glass-screenshot-blur.md, Option A
        // rejection): DWM material backdrops (DWMSBT_TRANSIENTWINDOW / Acrylic /
        // ACCENT_ENABLE_BLURBEHIND) are NEVER applied to these non-activating
        // windows — DWM renders a SOLID OPAQUE FALLBACK for inactive windows and
        // the material API overrides tao's transparency, turning the whole
        // window pitch black (the 2026-10-01 blackout root cause). The blur
        // comes from the ADR-05 screenshot-capture pipeline instead
        // (sidebar_backdrop.rs -> `sidebar:backdrop` -> `.sidebar-card::after`).
        let is_glass_window = matches!(
            config.label,
            "sidebar" | "calibrate-toolbar" | "settings" | "setup" | "orb-studio"
        );
        if is_glass_window {
            crate::dwm_corners::round_corners(&win);

            if let Ok(hwnd) = win.hwnd() {
                use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WINDOW_DISPLAY_AFFINITY};
                use windows::Win32::Foundation::HWND;
                // WDA_EXCLUDEFROMCAPTURE is 0x00000011 (17)
                unsafe {
                    let _ = SetWindowDisplayAffinity(HWND(hwnd.0 as _), WINDOW_DISPLAY_AFFINITY(17));
                }
            }
        } else {
            // Forensic line: proves at runtime which windows skipped DWM
            // glass (fullscreen-blackout investigations start here — if a
            // blackout window shows an "applied" line above, the exclusion
            // regressed; if it shows this line, glass is innocent).
            tracing::info!("live_glass: skipped for '{}' (compact-windows-only policy)", config.label);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if config.label == "sidebar" {
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
///
/// TEMPORARY DEV MODE: `DEV_KEEP_WINDOWS_ALIVE` keeps the unified sidebar
/// panel resident (hide instead of destroy) so UI changes can be
/// cross-checked live without rebuilding + relaunching nexus. FLIP TO
/// FALSE BEFORE ANY RELEASE BUILD.
const DEV_KEEP_WINDOWS_ALIVE: bool = true;

pub fn destroy_window<R: Runtime>(
    app: &tauri::AppHandle<R>,
    label: &str,
) -> Result<(), String> {
    if DEV_KEEP_WINDOWS_ALIVE && label == "sidebar" {
        if let Some(win) = app.get_webview_window(label) {
            tracing::info!("dyn_windows: DEV keep-alive — hiding '{}' instead of destroying", label);
            let _ = win.hide();
        }
        return Ok(());
    }
    if let Some(win) = app.get_webview_window(label) {
        tracing::info!("dyn_windows: destroying '{}' window (freeing ~250 MB)", label);
        let result: Result<(), tauri::Error> = win.destroy();
        result.map_err(|e| format!("Failed to destroy {label} window: {e}"))?;
    }
    Ok(())
}

/// Open (or re-point) a window that shows an EXTERNAL web page as a normal
/// top-level document. Used for Google Maps: google.com refuses to render in an
/// iframe (X-Frame-Options / frame-ancestors), so the authentic app can only be
/// shown as a top-level webview. The label is deliberately absent from every
/// capability file, so the remote page gets no Tauri IPC access.
/// `geom` = logical (x, y, w, h). Works on WebView2, WKWebView and WebKitGTK.
pub fn open_external_window<R: Runtime>(
    app: &tauri::AppHandle<R>,
    label: &str,
    url: &str,
    title: &str,
    geom: (f64, f64, f64, f64),
) -> Result<(), String> {
    let parsed: tauri::Url = url.parse().map_err(|e| format!("bad url: {e}"))?;
    if parsed.scheme() != "https" {
        return Err("external windows must use https".into());
    }
    let (x, y, w, h) = geom;
    if let Some(win) = app.get_webview_window(label) {
        // Re-point the existing window (keeps the user's place in the app shell).
        let js = format!("window.location.replace({})", serde_json::to_string(url).map_err(|e| e.to_string())?);
        win.eval(&js).map_err(|e| format!("navigate: {e}"))?;
        let _ = win.set_size(tauri::LogicalSize::new(w, h));
        let _ = win.set_position(tauri::LogicalPosition::new(x, y));
        let _ = win.show();
        #[cfg(target_os = "linux")]
        if label != "stage" {
            let _ = win.set_focus();
        }
        #[cfg(not(target_os = "linux"))]
        let _ = win.set_focus();
        return Ok(());
    }
    let win = WebviewWindowBuilder::new(app, label, WebviewUrl::External(parsed))
        .title(title)
        .inner_size(w, h)
        .position(x, y)
        .resizable(true)
        .decorations(true)
        .focused(true)
        .build()
        .map_err(|e| format!("Failed to create {label} window: {e}"))?;
    #[cfg(target_os = "linux")]
    if label != "stage" {
        let _ = win.set_focus();
    }
    #[cfg(not(target_os = "linux"))]
    let _ = win.set_focus();
    Ok(())
}
