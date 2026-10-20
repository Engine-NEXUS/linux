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
//!
//! **Linux:** WebKitGTK has the same problem, with the same fix: without a
//! `permission-request` handler it **denies** `getUserMedia`, so the orb
//! never hears and the hand-mode camera never opens. The handler below
//! allows NEXUS's own origins and denies foreign ones. On top of that the
//! app reports the real permission state ("granted" / "denied" / "no_device")
//! instead of pretending to grant something the OS never asked about.

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

/// The origins the Linux media-permission handler treats as NEXUS's own.
///
/// The shared list plus the `tauri://localhost` custom protocol: Tauri v2
/// serves the embedded frontend from `tauri://localhost` on Linux (on
/// Windows the same frontend is served from `http://tauri.localhost`), so
/// without it the production build would deny its own mic request.
#[cfg(any(target_os = "linux", test))]
const OWN_ORIGIN_PREFIXES: &[&str] = &[
    "http://tauri.localhost",
    "https://tauri.localhost",
    "http://localhost",
    "https://localhost",
    "ipc://localhost",
    "tauri://localhost",
];

/// The same list without the Linux-only custom protocol — compiled on the
/// other platforms so the rule can be unit-tested everywhere.
#[cfg(all(not(target_os = "linux"), test))]
const OWN_ORIGIN_PREFIXES: &[&str] = &[
    "http://tauri.localhost",
    "https://tauri.localhost",
    "http://localhost",
    "https://localhost",
    "ipc://localhost",
];

/// Is this requesting origin one of NEXUS's own app origins?
///
/// The decision rule of the media-permission handler: NEXUS's own pages
/// (the embedded frontend, the dev server, the IPC origin) may use the
/// mic/camera; anything else must be denied.
///
/// A prefix only counts when it ends on an origin boundary: `http://localhost`
/// matches `http://localhost:5173` and `http://localhost/index.html`, but not
/// `http://localhost.evil.com`.
///
/// Pure — unit-tested without a webview.
#[cfg(any(target_os = "linux", test))]
pub fn origin_is_own(origin: &str) -> bool {
    OWN_ORIGIN_PREFIXES.iter().any(|p| {
        let Some(rest) = origin.strip_prefix(p) else {
            return false;
        };
        rest.is_empty() || rest.starts_with(['/', ':', '?', '#'])
    })
}

/// Call once from the Tauri setup hook, after the windows exist.
pub fn init<R: Runtime>(_app: &tauri::App<R>) {
    #[cfg(target_os = "windows")]
    {
        for label in ["stage", "setup"] {
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

    // Linux: WebKitGTK denies getUserMedia unless a `permission-request`
    // handler allows it — the same reason the WebView2 handler exists. Hook
    // the two windows that exist at setup time.
    #[cfg(target_os = "linux")]
    {
        for label in ["stage", "setup"] {
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

    // Linux: the permission *state* is reported from the OS side — a capture
    // device that opens means the session allows capture, one that does not
    // open means it does not. The only honest signal is whether a capture
    // device actually opens, so report exactly that.
    #[cfg(target_os = "linux")]
    {
        match crate::audio_linux::capture_permission_state() {
            "granted" => tracing::info!(
                "permissions: Linux has no per-app microphone prompt — capture is decided by the device opening (granted)"
            ),
            other => tracing::warn!(
                "permissions: no usable capture device on Linux ({other}) — voice input will not work until a microphone is available"
            ),
        }
    }
}

/// Same auto-allow for a window created later (hand mode's camera window). Own origins only.
pub fn allow_media_for_window<R: Runtime>(_win: &tauri::WebviewWindow<R>) {
    #[cfg(target_os = "windows")]
    {
        let label = _win.label().to_string();
        let res = _win.with_webview(move |webview| unsafe {
            register_media_permission_handler(&webview, &label);
        });
        if let Err(e) = res {
            tracing::warn!("permissions: failed to hook dynamic webview: {e}");
        }
    }

    // Linux: WebKitGTK denies getUserMedia unless a `permission-request`
    // handler allows it, so a dynamically created window (hand mode's camera
    // window) needs the same handler the static windows get.
    #[cfg(target_os = "linux")]
    {
        let label = _win.label().to_string();
        let label_for_warn = label.clone();
        let res = _win.with_webview(move |webview| {
            register_media_permission_handler_linux(&webview, &label);
        });
        if let Err(e) = res {
            tracing::warn!("permissions: failed to hook '{label_for_warn}' webview: {e}");
        }
    }
}

/// Auto-allow mic/camera permission requests on Linux (WebKitGTK).
///
/// Without this, WebKitGTK **denies** `getUserMedia` and the orb never
/// hears — `orbRuntime.ts`'s mic stream never opens and the hand-mode camera
/// never opens either. Only NEXUS's own origins (tauri.localhost, localhost,
/// ipc) are allowed; a foreign origin is denied outright — we never hand the
/// mic or the camera to third-party content.
#[cfg(target_os = "linux")]
fn register_media_permission_handler_linux(
    webview: &tauri::webview::PlatformWebview,
    label: &str,
) {
    use webkit2gtk::{PermissionRequestExt, WebViewExt};

    let label_owned: String = label.to_string();
    let wv: webkit2gtk::WebView = webview.inner();
    wv.connect_permission_request(move |w, req| {
        let origin = w.uri().map(|u| u.to_string()).unwrap_or_default();
        if origin_is_own(&origin) {
            req.allow();
            tracing::info!("permissions: mic/camera auto-allowed on '{label_owned}' ({origin})");
        } else {
            req.deny();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// NEXUS's own pages may use the mic/camera.
    #[test]
    fn our_own_origins_are_allowed() {
        assert!(origin_is_own("http://tauri.localhost"));
        assert!(origin_is_own("https://tauri.localhost"));
        assert!(origin_is_own("http://localhost:5173"));
        assert!(origin_is_own("https://localhost:1420"));
        assert!(origin_is_own("http://localhost:5173/index.html"));
        assert!(origin_is_own("ipc://localhost"));
    }

    /// On Linux the embedded frontend is served from the `tauri://localhost`
    /// custom protocol — without it the production build denies its own mic.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_linux_custom_protocol_origin_is_allowed() {
        assert!(origin_is_own("tauri://localhost"));
        assert!(origin_is_own("tauri://localhost/index.html"));
    }

    /// Everything else is denied — the mic and the camera never go to
    /// third-party content, and an unknown origin (a webview that failed to
    /// report its URI) is denied too. A prefix only counts on an origin
    /// boundary, so `localhost.evil.com` is not ours.
    #[test]
    fn foreign_origins_are_denied() {
        assert!(!origin_is_own("https://example.com"));
        assert!(!origin_is_own("https://evil.test/steal"));
        assert!(!origin_is_own("http://tauri.localhost.evil.com"));
        assert!(!origin_is_own("http://localhost.evil.com"));
        assert!(!origin_is_own("file:///etc/passwd"));
        assert!(!origin_is_own(""));
    }
}
