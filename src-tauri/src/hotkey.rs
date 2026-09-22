//! Global hotkey (Ctrl/Cmd+Space) → state-dependent action.
//!
//! On press:
//!   - If any window (sidebar, architect-sidebar, pr-list-sidebar, settings,
//!     setup) is visible → close it only (do NOT wake).
//!   - If no window is visible → wake the assistant (do NOT touch windows).
//!   - If the assistant is speaking → barge-in: the frontend wake handler
//!     stops TTS and starts listening (handled in main.tsx startListening).
//!
//! This means:
//!   - Pressing the hotkey twice (with sidebar visible) first closes the
//!     sidebar, then wakes NEXUS on the second press.
//!   - Wake-word activation does NOT close the sidebar (handled separately
//!     in `wakeword_oww.rs`, which never emits `sidebar:hide`).
//!   - The hotkey never does both at once — it's one or the other based on
//!     the current sidebar visibility state.
//!
//! NOTE: The global-shortcut plugin is not available on Linux.
//! This entire module is compiled only on Windows and macOS.

#![cfg(not(target_os = "linux"))]

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const HOTKEYS: &[&str] = &[
    "CommandOrControl+Space",
    "CommandOrControl+Shift+S",
];

pub fn init<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    for &hk in HOTKEYS {
        let sc: Shortcut = match hk.parse() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to parse hotkey '{hk}': {e}");
                continue;
            }
        };

        let handle = app.clone();
        if let Err(e) = app.global_shortcut().on_shortcut(sc, move |_app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                // Ctrl+Shift+S → open settings sidebar directly
                if hk == "CommandOrControl+Shift+S" {
                    tracing::info!("hotkey ({}) → opening settings sidebar", hk);
                    let app_clone = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = crate::commands::show_settings_sidebar(app_clone).await;
                    });
                    return;
                }

                // Ctrl+Space → close any visible window, or wake NEXUS
                // Check if any sidebar/window is currently visible.
                // If so, close it and do NOT wake NEXUS.
                let sidebar_visible = handle
                    .get_webview_window("sidebar")
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false);
                let architect_visible = handle
                    .get_webview_window("architect-sidebar")
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false);
                let pr_list_visible = handle
                    .get_webview_window("pr-list-sidebar")
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false);
                let settings_sidebar_visible = handle
                    .get_webview_window("settings-sidebar")
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false);
                let settings_visible = handle
                    .get_webview_window("settings")
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false);
                let setup_visible = handle
                    .get_webview_window("setup")
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false);

                if sidebar_visible || architect_visible || pr_list_visible || settings_sidebar_visible || settings_visible || setup_visible {
                    // A window is visible → close it only, do NOT wake NEXUS.
                    tracing::info!("hotkey ({}) → window visible, closing window(s) only", hk);
                    // Destroy whichever window(s) are open to free ~250 MB each.
                    let _ = crate::dyn_windows::destroy_window(&handle, "sidebar");
                    let _ = crate::dyn_windows::destroy_window(&handle, "architect-sidebar");
                    let _ = crate::dyn_windows::destroy_window(&handle, "pr-list-sidebar");
                    let _ = crate::dyn_windows::destroy_window(&handle, "settings-sidebar");
                    let _ = crate::dyn_windows::destroy_window(&handle, "settings");
                    let _ = crate::dyn_windows::destroy_window(&handle, "setup");
                } else {
                    // Sidebar is hidden → wake NEXUS, do NOT touch sidebar.
                    tracing::info!("hotkey ({}) → sidebar hidden, waking NEXUS", hk);

                    // Only pre-start local STT sidecar if cloud STT won't be used
                    // (saves ~340 MB RAM when Groq cloud STT is active).
                    let groq_key = crate::commands::read_groq_api_key(&handle);
                    let local_only = crate::commands::read_local_stt_only(&handle);
                    if groq_key.is_empty() || local_only {
                        std::thread::spawn(|| {
                            crate::lazy_stt::ensure_stt_running();
                        });
                    } else {
                        tracing::info!("hotkey: Groq cloud STT configured, skipping local sidecar pre-start (saves RAM)");
                    }

                    // Start Rust-side STT capture (same as wake word path).
                    // Captures audio from the cpal stream — no getUserMedia needed.
                    crate::wakeword_oww::start_stt_capture();

                    if let Some(win) = handle.get_webview_window("main") {
                        let _ = win.show();
                        let _ = crate::window_manager::configure_non_activating_overlay(&win);
                        let _ = win.set_ignore_cursor_events(false);

                        // Call the frontend wake handler directly.
                        let _ = win.eval("window.__NEXUS_WAKE__ && window.__NEXUS_WAKE__()");
                    }
                }
            }
        }) {
            tracing::warn!("Failed to register handler for hotkey '{hk}': {e}");
        } else {
            tracing::info!("Registered global hotkey handler: {hk}");
        }
    }

    Ok(())
}
