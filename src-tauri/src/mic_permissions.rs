//! WebView2 permission handler — auto-approves mic/camera for NEXUS's own pages.
//!
//! Root cause of the repeated mic prompt: wry (Tauri's webview layer) only
//! registers a `PermissionRequested` handler for the clipboard. For microphone
//! and camera, WebView2 falls back to its built-in permission dialog, whose
//! decisions are not reliably persisted across sessions in standalone WebView2
//! apps — so the prompt re-appears after every restart.
//!
//! This module registers our own handler on each WebView2 that auto-allows
//! MICROPHONE and CAMERA permission requests, but ONLY when the requesting
//! origin is one of NEXUS's own app origins:
//!   - http(s)://tauri.localhost   (production, embedded frontend)
//!   - http(s)://localhost:*       (dev mode, Vite)
//!   - ipc://localhost             (Tauri IPC origin)
//!
//! Any other permission kind (geolocation, notifications, ...) or any external
//! origin falls through to WebView2's default dialog — we never silently grant
//! anything to third-party content.
//!
//! Because the grant is programmatic, it happens instantly at every request —
//! no dialog, ever, regardless of user-profile state.

#[cfg(target_os = "windows")]
use tauri::Manager;
#[cfg(target_os = "linux")]
use tauri::Manager;
use tauri::Runtime;

/// Origins that are allowed to use the mic/camera without prompting.
#[allow(unused)]
const ALLOWED_ORIGIN_PREFIXES: &[&str] = &[
    "http://tauri.localhost",
    "https://tauri.localhost",
    "http://localhost",
    "https://localhost",
    "ipc://localhost",
];

/// Call once from the Tauri setup hook, after the windows exist.
pub fn init<R: Runtime>(_app: &tauri::App<R>) {
    #[cfg(target_os = "linux")]
    {
        for label in ["main", "setup"] {
            let Some(win) = _app.get_webview_window(label) else {
                continue;
            };
            let label_owned = label.to_string();
            let res = win.with_webview(move |webview| {
                register_media_permission_handler_linux(&webview, &label_owned);
            });
            if let Err(e) = res {
                tracing::warn!("permissions: failed to hook '{label}' webview: {e}");
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        for label in ["main", "setup"] {
            let Some(win) = _app.get_webview_window(label) else {
                continue;
            };
            let label_owned = label.to_string();
            let label_for_warn = label_owned.clone();
            let res = win.with_webview(move |webview| {
                #[cfg(target_os = "windows")]
                unsafe {
                    register_media_permission_handler(&webview, &label_owned);
                    set_low_memory_mode(&webview, &label_owned);
                }
                #[cfg(not(target_os = "windows"))]
                let _ = (webview, &label_owned);
            });
            if let Err(e) = res {
                tracing::warn!("permissions: failed to hook '{label_for_warn}' webview: {e}");
            }
        }
    }
}

/// Auto-allow mic/camera permission requests on Linux (WebKitGTK).
/// Without this, WebKitGTK denies getUserMedia and the orb never hears —
/// `main.tsx` startListening resets to idle with no voice. Only allows
/// NEXUS's own origins (tauri.localhost, localhost, ipc); foreign origins
/// fall through to the default (deny) decision.
#[cfg(target_os = "linux")]
fn register_media_permission_handler_linux(
    webview: &tauri::webview::PlatformWebview,
    label: &str,
) {
    use webkit2gtk::{WebViewExt, PermissionRequestExt};

    let label_owned: String = label.to_string();
    let wv: webkit2gtk::WebView = webview.inner();
    wv.connect_permission_request(move |w, req| {
        let origin = w.uri().map(|u| u.to_string()).unwrap_or_default();
        let own = ALLOWED_ORIGIN_PREFIXES.iter().any(|p| origin.starts_with(p));
        if own {
            req.allow();
            tracing::info!("permissions: mic/camera auto-allowed on '{label_owned}' ({origin})");
        } else {
            tracing::warn!("permissions: denied foreign origin '{origin}' on '{label_owned}'");
        }
        // true = we handled the decision, stop other handlers.
        true
    });
    tracing::info!("permissions: mic/camera auto-allow registered on '{label}'");
}

/// Register the PermissionRequested handler on the WebView2 instance.
/// The COM event subscription holds a reference to the handler, so it stays
/// alive for the lifetime of the webview — no need to store the token.
#[cfg(target_os = "windows")]
unsafe fn register_media_permission_handler(
    webview: &tauri::webview::PlatformWebview,
    label: &str,
) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_KIND_CAMERA,
        COREWEBVIEW2_PERMISSION_KIND_MICROPHONE, COREWEBVIEW2_PERMISSION_STATE_ALLOW,
    };
    use webview2_com::{take_pwstr, PermissionRequestedEventHandler};

    let controller = webview.controller();
    let core = match controller.CoreWebView2() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("permissions: CoreWebView2 unavailable on '{label}': {e}");
            return;
        }
    };

    // EventRegistrationToken is an i64 out-param we don't need to keep.
    let mut token: i64 = 0;
    let result = core.add_PermissionRequested(
        &PermissionRequestedEventHandler::create(Box::new(|_sender, args| {
            let Some(args) = args else {
                return Ok(());
            };

            let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
            args.PermissionKind(&mut kind)?;

            let is_media = kind == COREWEBVIEW2_PERMISSION_KIND_MICROPHONE
                || kind == COREWEBVIEW2_PERMISSION_KIND_CAMERA;
            if !is_media {
                // Not ours to decide — let WebView2 show its default dialog.
                return Ok(());
            }

            // Check the requesting origin. get_Uri returns a CoTaskMem-allocated
            // PWSTR; take_pwstr converts it to a String and frees it.
            let mut uri = windows_core::PWSTR::null();
            let origin = if args.Uri(&mut uri).is_ok() {
                take_pwstr(uri)
            } else {
                String::new()
            };

            if ALLOWED_ORIGIN_PREFIXES.iter().any(|p| origin.starts_with(p)) {
                args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
            }
            // Foreign origin asking for mic/camera: leave state untouched so
            // WebView2 shows its default prompt.

            Ok(())
        })),
        &mut token,
    );

    match result {
        Ok(()) => tracing::info!("permissions: mic/camera auto-allow registered on '{label}'"),
        Err(e) => tracing::warn!("permissions: add_PermissionRequested failed on '{label}': {e}"),
    }
}

/// Set WebView2 to low-memory mode so it drops cached data and swaps to disk.
/// Saves ~40 MB on the orb window when idle. The window is still responsive —
/// WebView2 ramps back up automatically when the user interacts.
#[cfg(target_os = "windows")]
unsafe fn set_low_memory_mode(
    webview: &tauri::webview::PlatformWebview,
    label: &str,
) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_23, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
    };
    use windows_core::Interface;

    let controller = webview.controller();
    let core = match controller.CoreWebView2() {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("webview-mem: CoreWebView2 unavailable on '{label}': {e}");
            return;
        }
    };

    // Cast to ICoreWebView2_23 to access SetMemoryUsageTargetLevel
    let core23: ICoreWebView2_23 = match core.cast() {
        Ok(c) => c,
        Err(_) => {
            tracing::debug!("webview-mem: ICoreWebView2_23 not available on '{label}' (older runtime)");
            return;
        }
    };

    match core23.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW) {
        Ok(()) => tracing::info!("webview-mem: low-memory mode set on '{label}'"),
        Err(e) => tracing::debug!("webview-mem: SetMemoryUsageTargetLevel failed on '{label}': {e}"),
    }
}
